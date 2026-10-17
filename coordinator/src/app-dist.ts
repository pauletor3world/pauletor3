// Distribuição do app pelo próprio Worker do dono (e não por um site público):
// - GET  /d/:mundo.:segredo           instalador, para quem tem um convite válido;
// - GET  /u/:mundo/latest.json        atualizações (formato do updater do Tauri), só membros;
// - GET  /u/:mundo/installer          o instalador da atualização, só membros;
// - PUT  /v1/app/installer            o dono publica uma versão (x-p3-admin);
// - PUT  /v1/app/versao-minima        o dono define a versão mínima do app (x-p3-admin).
//
// Membros se autenticam no updater com a mesma assinatura Ed25519 do app, sobre o prefixo
// `/u/:mundo` (o updater usa os mesmos cabeçalhos para o JSON e para o download).
//
// Canais (preparado, não ativado): `?canal=teste` no latest.json/installer e no PUT publica e
// serve uma versão separada, para testar com amigos antes de liberar para todos. Sem o
// parâmetro, vale o canal "estavel" (o de sempre).

import { safeEqual, verifyRequest } from "./auth";
import { blobStore } from "./blobs";
import type { Env } from "./index";
import { ApiError } from "./world";

const APP_SPACE = "_app";
const KEY = "versions.json"; // reaproveita uma chave aceita pelo armazenamento (metadados)
const BIN = "objects/" + "0".repeat(64); // posição fixa do instalador atual
const MIN_KEY = "world-settings.json"; // configurações do app: { versao_minima }

export type Canal = "estavel" | "teste";

interface Release {
  version: string;
  notes: string;
  signature: string;
  pub_date: string;
  size: number;
  obrigatoria?: boolean;
}

export function canalDe(url: URL): Canal {
  return url.searchParams.get("canal") === "teste" ? "teste" : "estavel";
}

const keys = (c: Canal) => (c === "teste" ? { meta: "canal-teste/" + KEY, bin: "objects/" + "1".repeat(64) } : { meta: KEY, bin: BIN });

function requireAdmin(req: Request, env: Env) {
  const admin = req.headers.get("x-p3-admin") ?? "";
  if (!env.ADMIN_TOKEN || !safeEqual(admin.trim(), env.ADMIN_TOKEN.trim())) throw new ApiError(403, "forbidden", "chave de administração inválida");
}

async function release(env: Env, canal: Canal = "estavel"): Promise<Release | null> {
  const o = await blobStore(env, APP_SPACE).get(keys(canal).meta);
  if (!o) return null;
  return JSON.parse(await new Response(o.body).text()) as Release;
}

/** Notas em UTF-8: cabeçalhos HTTP não levam acento, então vêm em base64 (`x-p3-notes-b64`). */
function notesFrom(req: Request): string {
  const b64 = req.headers.get("x-p3-notes-b64");
  if (b64) {
    try {
      const bin = atob(b64.trim());
      return new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0))).slice(0, 8000);
    } catch {
      throw new ApiError(400, "bad_notes", "notas inválidas (base64)");
    }
  }
  return req.headers.get("x-p3-notes") ?? "";
}

export async function publishInstaller(req: Request, env: Env, url: URL): Promise<Response> {
  requireAdmin(req, env);
  const canal = canalDe(url);
  const version = req.headers.get("x-p3-version") ?? "";
  const signature = req.headers.get("x-p3-updater-signature") ?? "";
  const size = Number(req.headers.get("content-length") ?? "-1");
  if (!/^\d+\.\d+\.\d+([-+][\w.]+)?$/.test(version)) throw new ApiError(400, "bad_version", "versão inválida");
  if (!signature.trim()) throw new ApiError(400, "bad_signature", "assinatura do instalador ausente");
  if (!(size > 0) || !req.body) throw new ApiError(411, "length_required", "tamanho do instalador não informado");
  const store = blobStore(env, APP_SPACE);
  const k = keys(canal);
  await store.put(k.bin, req.body, size);
  const meta: Release = {
    version,
    notes: notesFrom(req),
    signature,
    pub_date: new Date().toISOString(),
    size,
    obrigatoria: req.headers.get("x-p3-obrigatoria") === "1",
  };
  const bytes = new TextEncoder().encode(JSON.stringify(meta));
  await store.put(k.meta, new Blob([bytes]).stream(), bytes.length);
  return Response.json({ ok: true, version, canal });
}

// ------------------------------------------------------------------ versão mínima

let minCache: { v: string | null; at: number } | null = null;
const MIN_CACHE_MS = 60_000;

/** Versão mínima do app: a definida pelo dono (armazenamento) ou a variável VERSAO_MINIMA. */
export async function minVersion(env: Env, now = Date.now()): Promise<string | null> {
  if (minCache && now - minCache.at < MIN_CACHE_MS) return minCache.v;
  let v: string | null = null;
  try {
    const o = await blobStore(env, APP_SPACE).get(MIN_KEY);
    if (o) v = ((JSON.parse(await new Response(o.body).text()) as { versao_minima?: string | null }).versao_minima ?? null) || null;
  } catch (e) {
    console.error("versão mínima ilegível", e instanceof Error ? e.message : String(e));
  }
  v = v ?? (env.VERSAO_MINIMA?.trim() || null);
  minCache = { v, at: now };
  return v;
}

export async function setMinVersion(req: Request, env: Env): Promise<Response> {
  requireAdmin(req, env);
  let body: { versao_minima?: string | null };
  try {
    body = (await req.json()) as typeof body;
  } catch {
    throw new ApiError(400, "bad_json", "corpo inválido");
  }
  const v = body.versao_minima ? String(body.versao_minima).trim() : null;
  if (v && !/^\d+\.\d+\.\d+$/.test(v)) throw new ApiError(400, "bad_version", "versão inválida (use 2.1.0)");
  const bytes = new TextEncoder().encode(JSON.stringify({ versao_minima: v }));
  await blobStore(env, APP_SPACE).put(MIN_KEY, new Blob([bytes]).stream(), bytes.length);
  minCache = { v: v ?? (env.VERSAO_MINIMA?.trim() || null), at: Date.now() };
  return Response.json({ ok: true, versao_minima: minCache.v });
}

/** Compara versões x.y.z (pré-lançamento conta como menor). Inválida = 0 (não bloqueia). */
export function compareVersions(a: string, b: string): number {
  const parse = (s: string) => {
    const m = /^(\d+)\.(\d+)\.(\d+)(-[\w.]+)?/.exec(s.trim());
    return m ? { n: [Number(m[1]), Number(m[2]), Number(m[3])], pre: !!m[4] } : null;
  };
  const x = parse(a);
  const y = parse(b);
  if (!x || !y) return 0;
  for (let i = 0; i < 3; i++) if (x.n[i] !== y.n[i]) return x.n[i] < y.n[i] ? -1 : 1;
  if (x.pre !== y.pre) return x.pre ? -1 : 1;
  return 0;
}

// ------------------------------------------------------------------ instalador

async function serveInstaller(env: Env, canal: Canal = "estavel"): Promise<Response | null> {
  const r = await release(env, canal);
  const o = r ? await blobStore(env, APP_SPACE).get(keys(canal).bin) : null;
  if (!r || !o) return null;
  return new Response(o.body, {
    headers: {
      "content-type": "application/vnd.microsoft.portable-executable",
      "content-length": String(o.size),
      "content-disposition": `attachment; filename="Machenia-${r.version}-setup.exe"`,
      "cache-control": "no-store",
    },
  });
}

/** Instalador para quem abriu um convite válido. */
export async function downloadForInvite(env: Env, inviteValid: boolean): Promise<Response> {
  if (!inviteValid) {
    return new Response("Este convite expirou. Peça um novo para o dono do mundo.", { status: 403, headers: { "content-type": "text/plain; charset=utf-8" } });
  }
  const r = await serveInstaller(env);
  if (r) return r;
  if (env.DOWNLOAD_URL) return Response.redirect(env.DOWNLOAD_URL, 302);
  return new Response("O dono ainda não publicou o instalador.", { status: 404, headers: { "content-type": "text/plain; charset=utf-8" } });
}

/** Updater: confere a assinatura do membro sobre o prefixo /u/:mundo. */
export async function updater(req: Request, env: Env, url: URL, worldId: string, rest: string, checkMember: (id: string) => Promise<unknown>): Promise<Response> {
  const scoped = new URL(url.toString());
  scoped.pathname = `/u/${worldId}`;
  scoped.search = "";
  const s = await verifyRequest(new Request(scoped, { method: "GET", headers: req.headers }), scoped, false);
  await checkMember(s.id);
  const canal = canalDe(url);
  if (rest === "latest.json") {
    const r = await release(env, canal);
    if (!r) return new Response(null, { status: 204 });
    const minima = await minVersion(env);
    const q = canal === "teste" ? "?canal=teste" : "";
    return Response.json(
      {
        version: r.version,
        notes: r.notes,
        pub_date: r.pub_date,
        platforms: { "windows-x86_64": { signature: r.signature, url: `${url.origin}/u/${worldId}/installer${q}` } },
        // campos extras (o updater ignora; o app lê pelo raw_json)
        obrigatoria: !!r.obrigatoria,
        versao_minima: minima,
        canal,
      },
      { headers: { "cache-control": "no-store" } },
    );
  }
  if (rest === "installer") {
    return (await serveInstaller(env, canal)) ?? new Response(null, { status: 404 });
  }
  throw new ApiError(404, "not_found", "rota não existe");
}
