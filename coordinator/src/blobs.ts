// Armazenamento dos arquivos do mundo (objetos, manifestos e alguns JSON), atrás de uma
// interface. Duas implementações:
// - "do" (padrão, sem cartão): SQLite de Durable Objects, dividido em 16 partes por mundo
//   (cada Durable Object grátis guarda até 1 GB; cada linha até 2 MB, então arquivos grandes
//   viram vários pedaços). A troca de versão de um arquivo é atômica.
// - "r2": bucket R2 (precisa de cartão cadastrado na Cloudflare; 10 GB grátis).
// Tudo chega cifrado pelo app com a chave do mundo: aqui só passam bytes opacos.

import { DurableObject } from "cloudflare:workers";

export interface BlobEntry {
  name: string;
  size: number;
}

export interface BlobStore {
  /** Grava (substitui) e devolve o tamanho anterior (0 se não existia). */
  put(key: string, body: ReadableStream<Uint8Array>, size: number): Promise<number>;
  get(key: string): Promise<{ body: ReadableStream<Uint8Array>; size: number } | null>;
  /** Apaga e devolve os bytes liberados. */
  delete(keys: string[]): Promise<number>;
  /** Lista as chaves com o prefixo (ex.: "objects/") — nomes sem o prefixo. */
  list(prefix: string): Promise<BlobEntry[]>;
}

export const SHARDS = 16;
const CHUNK = 1_900_000; // < 2 MB por linha

export function shardOf(key: string): number {
  const m = /^objects\/([0-9a-f])/.exec(key);
  if (m) return parseInt(m[1], 16) % SHARDS;
  let h = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h % SHARDS;
}

interface Env {
  BLOBS: DurableObjectNamespace<BlobShardDO>;
  BUCKET?: R2Bucket;
}

/** Uma parte do armazenamento de um mundo. */
export class BlobShardDO extends DurableObject {
  private sql: SqlStorage;

  constructor(ctx: DurableObjectState, env: unknown) {
    super(ctx, env as never);
    this.sql = ctx.storage.sql;
    this.sql.exec(`CREATE TABLE IF NOT EXISTS blobs (key TEXT PRIMARY KEY, size INTEGER NOT NULL, chunks INTEGER NOT NULL)`);
    this.sql.exec(`CREATE TABLE IF NOT EXISTS chunks (key TEXT NOT NULL, idx INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY (key, idx))`);
  }

  async put(key: string, body: ReadableStream<Uint8Array>): Promise<number> {
    // Grava os pedaços sob uma chave temporária; só no fim troca, numa transação.
    const tmp = `${key}#tmp-${crypto.randomUUID()}`;
    let idx = 0;
    let size = 0;
    let buf = new Uint8Array(CHUNK);
    let fill = 0;
    const flush = () => {
      if (fill === 0) return;
      this.sql.exec(`INSERT INTO chunks (key, idx, data) VALUES (?, ?, ?)`, tmp, idx++, buf.slice(0, fill));
      fill = 0;
    };
    const reader = body.getReader();
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        let off = 0;
        while (off < value.length) {
          const n = Math.min(CHUNK - fill, value.length - off);
          buf.set(value.subarray(off, off + n), fill);
          fill += n;
          off += n;
          size += n;
          if (fill === CHUNK) flush();
        }
      }
      flush();
    } catch (e) {
      this.sql.exec(`DELETE FROM chunks WHERE key = ?`, tmp);
      throw e;
    }
    let old = 0;
    this.ctx.storage.transactionSync(() => {
      old = this.sql.exec<{ size: number }>(`SELECT size FROM blobs WHERE key = ?`, key).toArray()[0]?.size ?? 0;
      this.sql.exec(`DELETE FROM chunks WHERE key = ?`, key);
      this.sql.exec(`UPDATE chunks SET key = ? WHERE key = ?`, key, tmp);
      this.sql.exec(
        `INSERT INTO blobs (key, size, chunks) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET size = excluded.size, chunks = excluded.chunks`,
        key,
        size,
        idx,
      );
    });
    return old;
  }

  async size(key: string): Promise<number | null> {
    return this.sql.exec<{ size: number }>(`SELECT size FROM blobs WHERE key = ?`, key).toArray()[0]?.size ?? null;
  }

  async get(key: string): Promise<{ body: ReadableStream<Uint8Array>; size: number } | null> {
    const meta = this.sql.exec<{ size: number; chunks: number }>(`SELECT size, chunks FROM blobs WHERE key = ?`, key).toArray()[0];
    if (!meta) return null;
    let i = 0;
    const sql = this.sql;
    const body = new ReadableStream<Uint8Array>({
      pull(ctrl) {
        if (i >= meta.chunks) {
          ctrl.close();
          return;
        }
        const row = sql.exec<{ data: ArrayBuffer }>(`SELECT data FROM chunks WHERE key = ? AND idx = ?`, key, i++).toArray()[0];
        if (!row) {
          ctrl.error(new Error("pedaço faltando"));
          return;
        }
        ctrl.enqueue(new Uint8Array(row.data));
      },
    });
    return { body, size: meta.size };
  }

  async delete(keys: string[]): Promise<number> {
    let freed = 0;
    this.ctx.storage.transactionSync(() => {
      for (const k of keys) {
        freed += this.sql.exec<{ size: number }>(`SELECT size FROM blobs WHERE key = ?`, k).toArray()[0]?.size ?? 0;
        this.sql.exec(`DELETE FROM chunks WHERE key = ?`, k);
        this.sql.exec(`DELETE FROM blobs WHERE key = ?`, k);
      }
    });
    return freed;
  }

  async list(prefix: string): Promise<BlobEntry[]> {
    // faixa [prefix, prefix + U+FFFF) em ordem de chave
    return this.sql
      .exec<{ key: string; size: number }>(`SELECT key, size FROM blobs WHERE key >= ? AND key < ? ORDER BY key`, prefix, prefix + "￿")
      .toArray()
      .map((r) => ({ name: r.key.slice(prefix.length), size: r.size }));
  }
}

/** Padrão: SQLite de Durable Objects, 16 partes por mundo. */
export class DoBlobStore implements BlobStore {
  constructor(
    private env: Env,
    private worldId: string,
  ) {}

  private shard(i: number) {
    return this.env.BLOBS.get(this.env.BLOBS.idFromName(`${this.worldId}:${i}`));
  }

  put(key: string, body: ReadableStream<Uint8Array>): Promise<number> {
    return this.shard(shardOf(key)).put(key, body);
  }

  async get(key: string) {
    return this.shard(shardOf(key)).get(key);
  }

  async delete(keys: string[]): Promise<number> {
    const by = new Map<number, string[]>();
    for (const k of keys) by.set(shardOf(k), [...(by.get(shardOf(k)) ?? []), k]);
    const freed = await Promise.all([...by.entries()].map(([i, ks]) => this.shard(i).delete(ks)));
    return freed.reduce((a, b) => a + b, 0);
  }

  async list(prefix: string): Promise<BlobEntry[]> {
    const parts = await Promise.all([...Array(SHARDS).keys()].map((i) => this.shard(i).list(prefix)));
    return parts.flat().sort((a, b) => (a.name < b.name ? -1 : 1));
  }
}

/** Opcional: bucket R2 (`STORAGE = "r2"` e binding `BUCKET`). */
export class R2BlobStore implements BlobStore {
  constructor(
    private bucket: R2Bucket,
    private worldId: string,
  ) {}

  private k(key: string) {
    return `w/${this.worldId}/${key}`;
  }

  async put(key: string, body: ReadableStream<Uint8Array>, size: number): Promise<number> {
    const old = (await this.bucket.head(this.k(key)))?.size ?? 0;
    const fixed = new FixedLengthStream(size);
    const piping = body.pipeTo(fixed.writable);
    await Promise.all([this.bucket.put(this.k(key), fixed.readable), piping]);
    return old;
  }

  async get(key: string) {
    const o = await this.bucket.get(this.k(key));
    return o ? { body: o.body, size: o.size } : null;
  }

  async delete(keys: string[]): Promise<number> {
    let freed = 0;
    for (let i = 0; i < keys.length; i += 1000) {
      const batch = keys.slice(i, i + 1000);
      for (const k of batch) freed += (await this.bucket.head(this.k(k)))?.size ?? 0;
      await this.bucket.delete(batch.map((k) => this.k(k)));
    }
    return freed;
  }

  async list(prefix: string): Promise<BlobEntry[]> {
    const out: BlobEntry[] = [];
    let cursor: string | undefined;
    const full = this.k(prefix);
    do {
      const r = await this.bucket.list({ prefix: full, cursor, limit: 1000 });
      for (const o of r.objects) out.push({ name: o.key.slice(full.length), size: o.size });
      cursor = r.truncated ? r.cursor : undefined;
    } while (cursor);
    return out;
  }
}

export function blobStore(env: Env & { STORAGE?: string }, worldId: string): BlobStore {
  if (env.STORAGE === "r2") {
    if (!env.BUCKET) throw new Error("STORAGE=r2 mas o binding BUCKET não está configurado");
    return new R2BlobStore(env.BUCKET, worldId);
  }
  return new DoBlobStore(env, worldId);
}
