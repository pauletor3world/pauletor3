// O Durable Object de um mundo: a autoridade sobre a vez de hospedar (lease), a versão
// oficial (HEAD), os membros, os convites e quem está online. Um Durable Object processa um
// pedido por vez, então cada operação aqui é atômica (compare-and-swap de verdade).
//
// Custo: o plano grátis limita escritas no SQLite (100 mil linhas/dia). Por isso a presença
// fica só na memória, o heartbeat do host vai ao disco no máximo a cada 15 s e o resto só é
// gravado quando muda.

import { DurableObject } from "cloudflare:workers";
import { randomId, sha256Hex } from "./auth";
import {
  FORMAT_VERSION,
  LEASE_TTL_MS,
  LeaseInfo,
  LeasePurpose,
  LeaseStatus,
  PRESENCE_FRESH_MS,
  Presence,
  SeenPresence,
  decideAcquire,
  handoffVersionConflict,
  leaseStatus,
  validNick,
} from "./rules";

/** Erro com significado para o app. Atravessa a RPC do Durable Object serializado na
 * mensagem (a RPC preserva a mensagem, mas não os campos extras). */
export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    public human: string,
    public extra: Record<string, unknown> = {},
  ) {
    super("P3ERR" + JSON.stringify({ status, error: code, message: human, ...extra }));
  }
  toJSON() {
    return { error: this.code, message: this.human, ...this.extra };
  }
  /** Reconstrói a partir de um erro que veio pela RPC. */
  static from(e: unknown): { status: number; body: Record<string, unknown> } | null {
    const msg = e instanceof Error ? e.message : String(e);
    const i = msg.indexOf("P3ERR");
    if (i < 0) return null;
    try {
      const o = JSON.parse(msg.slice(i + 5)) as Record<string, unknown> & { status: number };
      const { status, ...body } = o;
      return { status, body };
    } catch {
      return null;
    }
  }
}

export type Role = "owner" | "member";

export interface Member {
  id: string;
  nick: string;
  role: Role;
  approved: boolean;
  joined_at: string;
  last_seen: string | null;
  presence: Presence | null;
}

export interface Head {
  format: number;
  manifest: string;
  seq: number;
  session_number: number;
  updated_at: string;
  updated_by: string;
}

export interface Invite {
  token?: string;
  id: string;
  created_at: string;
  expires_at: string;
  uses_left: number;
  preapproved: boolean;
}

export interface DiaryEvent {
  at: string;
  kind: string;
  player: string | null;
  text: string;
}

export interface WorldView {
  world_id: string;
  name: string;
  lease: LeaseStatus;
  head: Head | null;
  members: Member[];
  now: string;
  me: Member | null;
  /** "Chamar pro jogo": entregue uma vez, na presença de quem foi chamado. */
  chamado?: { de: string; em: string };
}

export interface AcquireRequest {
  session_id: string;
  player: string;
  machine: string;
  endpoint: string | null;
  purpose: LeasePurpose;
  replica_seq: number;
  app_version: string;
}

export interface PrivateSettings {
  discord_webhook: string | null;
}

interface Env {
  MAX_MEMBERS?: string;
}

export const RPC_METHODS = [
  "init",
  "checkMember",
  "view",
  "acquire",
  "heartbeat",
  "release",
  "head",
  "publishHead",
  "presence",
  "postEvents",
  "events",
  "createInvite",
  "invites",
  "revokeInvite",
  "inviteInfo",
  "redeem",
  "setMember",
  "privateSettings",
  "setPrivateSettings",
  "usage",
  "addUsage",
  "call",
] as const;
export type RpcMethod = (typeof RPC_METHODS)[number];
export type RpcResult = { ok: true; v: unknown } | { ok: false; status: number; body: Record<string, unknown> };

const HEARTBEAT_PERSIST_MS = 15_000;
/** Um chamado vale por 15 min (quem abre o app depois disso não recebe aviso velho). */
const CALL_TTL_MS = 15 * 60_000;
const LAST_SEEN_PERSIST_MS = 10 * 60_000;
const MAX_EVENTS = 2000;

const iso = (t: number) => new Date(t).toISOString();

export class WorldDO extends DurableObject<Env> {
  private sql: SqlStorage;
  private lease: LeaseInfo | null = null;
  private leasePersistedAt = 0;
  private online = new Map<string, { p: Presence; at: number }>();
  private lastSeenPersisted = new Map<string, number>();
  /** "Chamar pro jogo": quem chamou cada membro (entregue uma vez, na presença dele). */
  private calls = new Map<string, { de: string; at: number }>();

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      this.sql.exec(`CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL)`);
      this.sql.exec(`CREATE TABLE IF NOT EXISTS members (
        id TEXT PRIMARY KEY, nick TEXT NOT NULL, role TEXT NOT NULL, approved INTEGER NOT NULL,
        joined_at TEXT NOT NULL, last_seen TEXT)`);
      this.sql.exec(`CREATE TABLE IF NOT EXISTS invites (
        id TEXT PRIMARY KEY, secret_hash TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL,
        expires_at TEXT NOT NULL, uses_left INTEGER NOT NULL, preapproved INTEGER NOT NULL)`);
      this.sql.exec(`CREATE TABLE IF NOT EXISTS events (
        seq INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, kind TEXT NOT NULL, player TEXT, text TEXT NOT NULL)`);
      this.lease = this.getMeta<LeaseInfo>("lease");
    });
  }

  // ------------------------------------------------------------------ porta de entrada (RPC)

  /**
   * Única porta usada pelo Worker: chama a operação e devolve sucesso ou o erro já pronto
   * para virar resposta HTTP (erros esperados não "estouram" pela RPC nem sujam os logs).
   */
  async rpc(method: RpcMethod, ...args: unknown[]): Promise<RpcResult> {
    if (!RPC_METHODS.includes(method)) return { ok: false, status: 404, body: { error: "not_found", message: "operação desconhecida" } };
    try {
      const f = (this as unknown as Record<string, (...a: unknown[]) => unknown>)[method];
      return { ok: true, v: await f.apply(this, args) };
    } catch (e) {
      if (e instanceof ApiError) return { ok: false, status: e.status, body: e.toJSON() };
      throw e;
    }
  }

  // ------------------------------------------------------------------ armazenamento

  private getMeta<T>(k: string): T | null {
    const rows = this.sql.exec<{ v: string }>(`SELECT v FROM meta WHERE k = ?`, k).toArray();
    return rows.length ? (JSON.parse(rows[0].v) as T) : null;
  }

  private setMeta(k: string, v: unknown) {
    if (v === null || v === undefined) this.sql.exec(`DELETE FROM meta WHERE k = ?`, k);
    else this.sql.exec(`INSERT INTO meta (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v`, k, JSON.stringify(v));
  }

  private saveLease(force: boolean, now: number) {
    if (force || now - this.leasePersistedAt >= HEARTBEAT_PERSIST_MS) {
      this.setMeta("lease", this.lease);
      this.leasePersistedAt = now;
    }
  }

  private memberRow(id: string): Member | null {
    const r = this.sql
      .exec<{ id: string; nick: string; role: string; approved: number; joined_at: string; last_seen: string | null }>(
        `SELECT * FROM members WHERE id = ?`,
        id,
      )
      .toArray();
    if (!r.length) return null;
    return { ...r[0], role: r[0].role as Role, approved: !!r[0].approved, presence: null };
  }

  private allMembers(now: number): Member[] {
    return this.sql
      .exec<{ id: string; nick: string; role: string; approved: number; joined_at: string; last_seen: string | null }>(
        `SELECT * FROM members ORDER BY role DESC, joined_at`,
      )
      .toArray()
      .map((r) => {
        const pr = this.online.get(r.id);
        const fresh = pr && now - pr.at <= PRESENCE_FRESH_MS ? pr.p : null;
        return {
          ...r,
          role: r.role as Role,
          approved: !!r.approved,
          last_seen: pr ? iso(pr.at) : r.last_seen,
          presence: fresh,
        };
      });
  }

  private requireMember(id: string, approved = true): Member {
    if (!this.getMeta<string>("name")) throw new ApiError(404, "no_world", "mundo não existe");
    const m = this.memberRow(id);
    if (!m) throw new ApiError(403, "not_member", "este PC não é membro do mundo");
    if (approved && !m.approved) throw new ApiError(403, "pending_approval", "aguardando aprovação do dono");
    return m;
  }

  private requireOwner(id: string): Member {
    const m = this.requireMember(id);
    if (m.role !== "owner") throw new ApiError(403, "forbidden", "só o dono pode fazer isso");
    return m;
  }

  // ------------------------------------------------------------------ mundo

  async init(worldId: string, name: string, ownerId: string, ownerNick: string, now = Date.now()) {
    if (this.getMeta<string>("name")) throw new ApiError(409, "exists", "mundo já existe");
    if (!validNick(ownerNick)) throw new ApiError(400, "bad_nick", "nick inválido (3 a 16 letras, números ou _)");
    const clean = name.trim().slice(0, 40) || "Nosso Mundo";
    this.ctx.storage.transactionSync(() => {
      this.setMeta("world_id", worldId);
      this.setMeta("name", clean);
      this.setMeta("session_counter", 0);
      this.setMeta("created_at", iso(now));
      this.sql.exec(
        `INSERT INTO members (id, nick, role, approved, joined_at) VALUES (?, ?, 'owner', 1, ?)`,
        ownerId,
        ownerNick,
        iso(now),
      );
    });
    return { world_id: worldId };
  }

  async checkMember(id: string): Promise<{ role: Role; approved: boolean }> {
    const m = this.requireMember(id);
    return { role: m.role, approved: m.approved };
  }

  async view(caller: string, now = Date.now()): Promise<WorldView> {
    const me = this.requireMember(caller, false);
    const name = this.getMeta<string>("name") ?? "";
    const world_id = this.getMeta<string>("world_id") ?? "";
    if (!me.approved) {
      // aguardando aprovação: só o básico
      return { world_id, name, lease: { kind: "free", previous: null }, head: null, members: [], now: iso(now), me };
    }
    const members = this.allMembers(now);
    return {
      world_id,
      name,
      lease: leaseStatus(now, this.lease),
      head: this.getMeta<Head>("head"),
      members,
      now: iso(now),
      me: members.find((m) => m.id === caller) ?? me,
    };
  }

  // ------------------------------------------------------------------ vez de hospedar

  async acquire(caller: string, req: AcquireRequest, now = Date.now()): Promise<LeaseInfo> {
    this.requireMember(caller);
    const seen: SeenPresence[] = [...this.online.entries()].map(([member, v]) => ({ member, presence: v.p, at: v.at }));
    const appVersion = String(req.app_version ?? "").slice(0, 32);
    if (handoffVersionConflict(now, this.lease, caller, appVersion)) {
      throw new ApiError(409, "app_version_mismatch", "quem está passando o host usa outra versão do app", { host_version: this.lease!.app_version });
    }
    const d = decideAcquire(now, this.lease, caller, req.replica_seq ?? 0, seen);
    if (d.kind === "held") throw new ApiError(409, "held_by", `${d.lease.player} já está hospedando`, { lease: d.lease });
    if (d.kind === "not_best") throw new ApiError(409, "not_best", "outro membro tem uma cópia mais nova", { best_seq: d.best_seq });
    const counter = (this.getMeta<number>("session_counter") ?? 0) + 1;
    this.lease = {
      format: FORMAT_VERSION,
      session_number: counter,
      session_id: String(req.session_id).slice(0, 64),
      holder: caller,
      player: String(req.player).slice(0, 16),
      machine: String(req.machine ?? "").slice(0, 64),
      endpoint: req.endpoint ? String(req.endpoint).slice(0, 4096) : null,
      app_version: String(req.app_version ?? "").slice(0, 32),
      purpose: req.purpose === "maintenance" ? "maintenance" : "hosting",
      acquired_at: iso(now),
      heartbeat_at: iso(now),
      vouched_at: null,
      handing_over: false,
      online: [],
    };
    this.ctx.storage.transactionSync(() => {
      this.setMeta("session_counter", counter);
      this.saveLease(true, now);
    });
    return this.lease;
  }

  private ownLease(caller: string, sessionNumber: number): LeaseInfo {
    const l = this.lease;
    if (!l || l.holder !== caller || l.session_number !== sessionNumber) {
      throw new ApiError(409, "lost_lock", "a vez não é mais deste PC");
    }
    return l;
  }

  async heartbeat(caller: string, sessionNumber: number, online: string[], handingOver: boolean, now = Date.now()): Promise<LeaseInfo> {
    this.requireMember(caller);
    const l = this.ownLease(caller, sessionNumber);
    const changed = handingOver !== l.handing_over || JSON.stringify(online) !== JSON.stringify(l.online);
    l.heartbeat_at = iso(now);
    l.online = (online ?? []).slice(0, 50).map((s) => String(s).slice(0, 16));
    l.handing_over = !!handingOver;
    this.saveLease(changed, now);
    return l;
  }

  /**
   * Libera a vez. Com `handoff`, a vez não some: vira "vencida agora" com `handing_over`, e quem
   * estava jogando assume pelo caminho do failover (a réplica mais nova tem preferência).
   */
  async release(caller: string, sessionNumber: number, handoff = false, now = Date.now()): Promise<{}> {
    const l = this.lease;
    if (l && l.holder === caller && l.session_number === sessionNumber) {
      if (handoff) {
        l.heartbeat_at = iso(now - LEASE_TTL_MS);
        l.vouched_at = null;
        l.handing_over = true;
        this.saveLease(true, now);
      } else {
        this.lease = null;
        this.setMeta("lease", null);
      }
    }
    return {};
  }

  async head(caller: string): Promise<Head | null> {
    this.requireMember(caller);
    return this.getMeta<Head>("head");
  }

  async publishHead(caller: string, sessionNumber: number, expected: string | null, manifest: string, now = Date.now()): Promise<Head> {
    this.requireMember(caller);
    this.ownLease(caller, sessionNumber);
    if (!/^[A-Za-z0-9._-]{1,120}$/.test(manifest)) throw new ApiError(400, "bad_manifest", "id de manifesto inválido");
    const cur = this.getMeta<Head>("head");
    if ((cur?.manifest ?? null) !== (expected ?? null)) {
      throw new ApiError(409, "head_moved", "a versão oficial mudou", { current: cur?.manifest ?? null });
    }
    const h: Head = {
      format: FORMAT_VERSION,
      manifest,
      seq: (cur?.seq ?? 0) + 1,
      session_number: sessionNumber,
      updated_at: iso(now),
      updated_by: caller,
    };
    this.setMeta("head", h);
    return h;
  }

  async presence(caller: string, p: Presence, now = Date.now()): Promise<WorldView> {
    this.requireMember(caller, false);
    const clean: Presence = {
      endpoint: p.endpoint ? String(p.endpoint).slice(0, 4096) : null,
      replica_seq: Number(p.replica_seq) || 0,
      playing: !!p.playing,
      host_reachable: p.host_reachable == null ? null : Number(p.host_reachable),
      app_version: String(p.app_version ?? "").slice(0, 32),
      path: p.path ? String(p.path).slice(0, 16) : null,
      proxy_port: Number.isInteger(p.proxy_port) && p.proxy_port! > 0 && p.proxy_port! < 65536 ? p.proxy_port : null,
    };
    this.online.set(caller, { p: clean, at: now });
    // Um membro alcança o host por P2P: a vez continua viva mesmo se o heartbeat do host
    // não chega (só a internet do host com a nuvem caiu).
    const l = this.lease;
    // Quem está passando o host não é sustentado: quem joga assume na hora.
    if (l && !l.handing_over && clean.host_reachable != null && l.session_number === clean.host_reachable && l.holder !== caller) {
      l.vouched_at = iso(now);
      this.saveLease(false, now);
    }
    if (now - (this.lastSeenPersisted.get(caller) ?? 0) > LAST_SEEN_PERSIST_MS) {
      this.sql.exec(`UPDATE members SET last_seen = ? WHERE id = ?`, iso(now), caller);
      this.lastSeenPersisted.set(caller, now);
    }
    const v = await this.view(caller, now);
    const c = this.calls.get(caller);
    if (c) {
      this.calls.delete(caller);
      if (now - c.at < CALL_TTL_MS) return { ...v, chamado: { de: c.de, em: iso(c.at) } };
    }
    return v;
  }

  /** Chama um membro pro jogo: o app dele mostra a notificação na próxima presença. */
  async call(caller: string, to: string, now = Date.now()): Promise<{}> {
    const me = this.requireMember(caller);
    if (!this.memberRow(String(to))) throw new ApiError(404, "not_member", "esse membro não está no mundo");
    this.calls.set(String(to), { de: me.nick, at: now });
    return {};
  }

  // ------------------------------------------------------------------ diário

  async postEvents(caller: string, events: DiaryEvent[]): Promise<{}> {
    this.requireMember(caller);
    for (const e of (events ?? []).slice(0, 50)) {
      this.sql.exec(
        `INSERT INTO events (at, kind, player, text) VALUES (?, ?, ?, ?)`,
        String(e.at).slice(0, 40),
        String(e.kind).slice(0, 32),
        e.player ? String(e.player).slice(0, 16) : null,
        String(e.text).slice(0, 500),
      );
    }
    this.sql.exec(`DELETE FROM events WHERE seq <= (SELECT MAX(seq) FROM events) - ?`, MAX_EVENTS);
    return {};
  }

  async events(caller: string, limit: number): Promise<DiaryEvent[]> {
    this.requireMember(caller);
    return this.sql
      .exec<{ at: string; kind: string; player: string | null; text: string }>(
        `SELECT at, kind, player, text FROM events ORDER BY seq DESC LIMIT ?`,
        Math.min(Math.max(limit || 100, 1), 500),
      )
      .toArray();
  }

  // ------------------------------------------------------------------ convites e membros

  async createInvite(caller: string, opts: { hours?: number; uses?: number; preapproved?: boolean }, now = Date.now()): Promise<Invite> {
    this.requireOwner(caller);
    const worldId = this.getMeta<string>("world_id")!;
    const secret = randomId(24);
    const hours = Math.min(Math.max(Number(opts.hours) || 48, 1), 24 * 30);
    const uses = Math.min(Math.max(Number(opts.uses) || 5, 1), 50);
    const inv: Invite = {
      id: randomId(8),
      created_at: iso(now),
      expires_at: iso(now + hours * 3600_000),
      uses_left: uses,
      preapproved: opts.preapproved !== false,
    };
    this.sql.exec(
      `INSERT INTO invites (id, secret_hash, created_at, expires_at, uses_left, preapproved) VALUES (?, ?, ?, ?, ?, ?)`,
      inv.id,
      await sha256Hex(secret),
      inv.created_at,
      inv.expires_at,
      inv.uses_left,
      inv.preapproved ? 1 : 0,
    );
    return { ...inv, token: `${worldId}.${secret}` };
  }

  async invites(caller: string): Promise<Invite[]> {
    this.requireOwner(caller);
    return this.sql
      .exec<{ id: string; created_at: string; expires_at: string; uses_left: number; preapproved: number }>(
        `SELECT id, created_at, expires_at, uses_left, preapproved FROM invites ORDER BY created_at DESC`,
      )
      .toArray()
      .map((r) => ({ ...r, preapproved: !!r.preapproved }));
  }

  async revokeInvite(caller: string, id: string): Promise<{}> {
    this.requireOwner(caller);
    this.sql.exec(`DELETE FROM invites WHERE id = ?`, id);
    return {};
  }

  /** Informações públicas de um convite (para a página do link). Não gasta usos. */
  async inviteInfo(secret: string, now = Date.now()): Promise<{ valid: boolean; name: string; owner_nick: string }> {
    const name = this.getMeta<string>("name") ?? "";
    const owner = this.sql.exec<{ nick: string }>(`SELECT nick FROM members WHERE role = 'owner' LIMIT 1`).toArray()[0]?.nick ?? "";
    const r = this.sql
      .exec<{ expires_at: string; uses_left: number }>(`SELECT expires_at, uses_left FROM invites WHERE secret_hash = ?`, await sha256Hex(secret))
      .toArray()[0];
    return { valid: !!r && Date.parse(r.expires_at) > now && r.uses_left > 0, name, owner_nick: owner };
  }

  async redeem(secret: string, caller: string, nick: string, now = Date.now(), maxMembers = 10) {
    if (!validNick(nick)) throw new ApiError(400, "bad_nick", "nick inválido (3 a 16 letras, números ou _)");
    const hash = await sha256Hex(secret);
    const world_id = this.getMeta<string>("world_id") ?? "";
    const name = this.getMeta<string>("name") ?? "";
    const owner_nick = this.sql.exec<{ nick: string }>(`SELECT nick FROM members WHERE role = 'owner' LIMIT 1`).toArray()[0]?.nick ?? "";
    const existing = this.memberRow(caller);
    if (existing) return { world_id, name, approved: existing.approved, owner_nick };
    const inv = this.sql
      .exec<{ id: string; expires_at: string; uses_left: number; preapproved: number }>(
        `SELECT id, expires_at, uses_left, preapproved FROM invites WHERE secret_hash = ?`,
        hash,
      )
      .toArray()[0];
    if (!inv || Date.parse(inv.expires_at) <= now || inv.uses_left <= 0) {
      throw new ApiError(403, "invite_invalid", "convite inválido ou expirado; peça um novo ao dono");
    }
    const count = this.sql.exec<{ n: number }>(`SELECT COUNT(*) AS n FROM members`).one().n;
    if (count >= maxMembers) throw new ApiError(403, "forbidden", "o mundo já tem o número máximo de membros");
    const taken = this.sql.exec<{ n: number }>(`SELECT COUNT(*) AS n FROM members WHERE lower(nick) = lower(?)`, nick).one().n;
    if (taken) throw new ApiError(409, "nick_taken", "já existe alguém com esse nick neste mundo");
    this.ctx.storage.transactionSync(() => {
      this.sql.exec(`UPDATE invites SET uses_left = uses_left - 1 WHERE id = ?`, inv.id);
      this.sql.exec(
        `INSERT INTO members (id, nick, role, approved, joined_at) VALUES (?, ?, 'member', ?, ?)`,
        caller,
        nick,
        inv.preapproved ? 1 : 0,
        iso(now),
      );
    });
    return { world_id, name, approved: !!inv.preapproved, owner_nick };
  }

  async setMember(caller: string, id: string, approved: boolean | null, remove: boolean): Promise<{}> {
    this.requireOwner(caller);
    if (id === caller) throw new ApiError(400, "forbidden", "o dono não pode se remover");
    if (remove) {
      this.sql.exec(`DELETE FROM members WHERE id = ? AND role != 'owner'`, id);
      this.online.delete(id);
    } else if (approved !== null && approved !== undefined) {
      this.sql.exec(`UPDATE members SET approved = ? WHERE id = ? AND role != 'owner'`, approved ? 1 : 0, id);
    }
    return {};
  }

  async privateSettings(caller: string): Promise<PrivateSettings> {
    this.requireMember(caller);
    return this.getMeta<PrivateSettings>("private") ?? { discord_webhook: null };
  }

  async setPrivateSettings(caller: string, s: PrivateSettings): Promise<{}> {
    this.requireOwner(caller);
    const w = s.discord_webhook ? String(s.discord_webhook).trim() : null;
    if (w && !/^https:\/\/(discord|discordapp)\.com\/api\/webhooks\/\d+\/[\w-]+$/.test(w)) {
      throw new ApiError(400, "bad_webhook", "isso não parece um webhook do Discord");
    }
    this.setMeta("private", { discord_webhook: w || null });
    return {};
  }

  // ------------------------------------------------------------------ uso da nuvem

  async usage(): Promise<number> {
    return this.getMeta<number>("usage") ?? 0;
  }

  async addUsage(delta: number): Promise<number> {
    const u = Math.max(0, (this.getMeta<number>("usage") ?? 0) + delta);
    this.setMeta("usage", u);
    return u;
  }
}
