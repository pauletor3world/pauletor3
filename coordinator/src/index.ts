// Coordenação do Machenia (Cloudflare Worker). Contrato em ../docs/api.md.
//
// GET  /health                                  saúde
// GET  /c/:token                                página do convite (link que o amigo abre)
// POST /v1/worlds                               cria mundo (x-p3-admin)
// POST /v1/join/:token                          entra por convite
// *    /v1/worlds/:w/...                        operações do mundo (só membros)

import { compareVersions, downloadForInvite, minVersion, publishInstaller, setMinVersion, updater } from "./app-dist";
import { AuthError, randomId, safeEqual, verifyRequest } from "./auth";
import { blobStore } from "./blobs";
import { invitePage } from "./invite-page";
import { validBlobPath } from "./rules";
import { ApiError, type RpcMethod, type RpcResult, WorldDO } from "./world";

export { WorldDO } from "./world";
export { BlobShardDO } from "./blobs";

export interface Env {
  WORLD: DurableObjectNamespace<WorldDO>;
  BLOBS: DurableObjectNamespace<import("./blobs").BlobShardDO>;
  BUCKET?: R2Bucket;
  /** "do" (padrão) ou "r2". */
  STORAGE?: string;
  /** Segredo (wrangler secret put ADMIN_TOKEN): quem tem cria mundos. */
  ADMIN_TOKEN?: string;
  /** Link de download do instalador (página do convite). */
  DOWNLOAD_URL?: string;
  /** Cota da nuvem por mundo, em bytes. */
  QUOTA_BYTES?: string;
  MAX_MEMBERS?: string;
  /** Versão mínima do app (reserva; o dono define a valer com PUT /v1/app/versao-minima). */
  VERSAO_MINIMA?: string;
}

const json = (v: unknown, status = 200) =>
  new Response(JSON.stringify(v), { status, headers: { "content-type": "application/json; charset=utf-8" } });

/** Acesso ao Durable Object do mundo: cada chamada devolve o valor ou lança o ApiError. */
function worldStub(env: Env, worldId: string) {
  if (!/^[a-z0-9]{6,32}$/.test(worldId)) throw new ApiError(404, "no_world", "mundo não existe");
  const stub = env.WORLD.get(env.WORLD.idFromName(worldId));
  const call = async (m: RpcMethod, ...args: unknown[]): Promise<any> => {
    const r = (await stub.rpc(m, ...args)) as unknown as RpcResult;
    if (r.ok) return r.v;
    const { error, message, ...extra } = r.body as { error: string; message: string };
    throw new ApiError(r.status, error, message, extra);
  };
  return new Proxy({} as Record<RpcMethod, (...a: unknown[]) => Promise<any>>, {
    get: (_t, m: string) => (...a: unknown[]) => call(m as RpcMethod, ...a),
  });
}

async function bodyJson<T>(body: ArrayBuffer | null): Promise<T> {
  try {
    return JSON.parse(new TextDecoder().decode(body ?? new ArrayBuffer(0))) as T;
  } catch {
    throw new ApiError(400, "bad_json", "corpo inválido");
  }
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    try {
      return await route(req, env, url);
    } catch (e) {
      if (e instanceof ApiError) return json(e.toJSON(), e.status);
      if (e instanceof AuthError) return json({ error: e.code, message: e.message }, 401);
      // erros que voltam pela RPC do Durable Object perdem a classe: reconstrói pela mensagem
      const r = ApiError.from(e);
      if (r) return json(r.body, r.status);
      console.error("erro interno", e instanceof Error ? e.message : String(e));
      return json({ error: "internal", message: "erro interno da coordenação" }, 500);
    }
  },
} satisfies ExportedHandler<Env>;

async function route(req: Request, env: Env, url: URL): Promise<Response> {
  const p = url.pathname;
  if (p === "/health") return json({ ok: true, storage: env.STORAGE ?? "do" });
  if (p === "/" ) return new Response("Machenia — coordenação", { headers: { "content-type": "text/plain; charset=utf-8" } });

  // página do convite
  let m = /^\/c\/([a-z0-9]{6,32})\.([a-z0-9]{8,64})$/.exec(p);
  if (m && req.method === "GET") {
    const info = await worldStub(env, m[1]).inviteInfo(m[2]).catch(() => ({ valid: false, name: "", owner_nick: "" }));
    return new Response(invitePage(info, `/d/${m[1]}.${m[2]}`), {
      headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store", "referrer-policy": "no-referrer" },
    });
  }

  // instalador para quem tem convite
  m = /^\/d\/([a-z0-9]{6,32})\.([a-z0-9]{8,64})$/.exec(p);
  if (m && req.method === "GET") {
    const info = await worldStub(env, m[1]).inviteInfo(m[2]).catch(() => ({ valid: false }));
    return downloadForInvite(env, !!info.valid);
  }

  // atualizações do app (só membros)
  m = /^\/u\/([a-z0-9]{6,32})\/(latest\.json|installer)$/.exec(p);
  if (m && req.method === "GET") {
    const w = worldStub(env, m[1]);
    return updater(req, env, url, m[1], m[2], (id) => w.checkMember(id));
  }

  // o dono publica uma versão do app
  if (p === "/v1/app/installer" && req.method === "PUT") return publishInstaller(req, env, url);
  // o dono define a versão mínima do app (sem publicar app novo)
  if (p === "/v1/app/versao-minima" && req.method === "PUT") return setMinVersion(req, env);
  if (p === "/v1/app/versao-minima" && req.method === "GET") return json({ versao_minima: await minVersion(env) });

  // criar mundo
  if (p === "/v1/worlds" && req.method === "POST") {
    const admin = req.headers.get("x-p3-admin") ?? "";
    if (!env.ADMIN_TOKEN || !safeEqual(admin.trim(), env.ADMIN_TOKEN.trim())) throw new ApiError(403, "forbidden", "chave de administração inválida");
    const s = await verifyRequest(req, url, true);
    const b = await bodyJson<{ name: string; nick: string }>(s.body);
    const worldId = randomId(12);
    return json(await worldStub(env, worldId).init(worldId, String(b.name ?? ""), s.id, String(b.nick ?? "")));
  }

  // entrar por convite
  m = /^\/v1\/join\/([a-z0-9]{6,32})\.([a-z0-9]{8,64})$/.exec(p);
  if (m && req.method === "POST") {
    const s = await verifyRequest(req, url, true);
    const b = await bodyJson<{ nick: string }>(s.body);
    const max = Number(env.MAX_MEMBERS ?? "10");
    return json(await worldStub(env, m[1]).redeem(m[2], s.id, String(b.nick ?? ""), Date.now(), max));
  }

  // operações do mundo
  m = /^\/v1\/worlds\/([a-z0-9]{6,32})\/(.+)$/.exec(p);
  if (!m) throw new ApiError(404, "not_found", "rota não existe");
  const [, worldId, rest] = m;
  const w = worldStub(env, worldId);
  const isBlob = rest.startsWith("blob/");
  const s = await verifyRequest(req, url, !isBlob);
  const id = s.id;
  const method = req.method;

  if (isBlob) {
    const key = decodeURIComponent(rest.slice(5));
    if (!validBlobPath(key)) throw new ApiError(400, "bad_path", "caminho inválido");
    await w.checkMember(id);
    const store = blobStore(env, worldId);
    if (method === "GET") {
      const o = await store.get(key);
      if (!o) return json({ error: "not_found", message: "não existe" }, 404);
      return new Response(o.body, { headers: { "content-type": "application/octet-stream", "content-length": String(o.size) } });
    }
    if (method === "PUT") {
      const size = Number(req.headers.get("content-length") ?? "-1");
      if (!(size >= 0) || !req.body) throw new ApiError(411, "length_required", "tamanho do arquivo não informado");
      const quota = Number(env.QUOTA_BYTES ?? String(4_000_000_000));
      const used = await w.usage();
      if (used + size > quota) {
        throw new ApiError(413, "quota", `a nuvem do mundo atingiu o limite de ${(quota / 1e9).toFixed(1)} GB; libere espaço em Saúde → Liberar espaço`);
      }
      const old = await store.put(key, req.body, size);
      await w.addUsage(size - old);
      return json({});
    }
    if (method === "DELETE") {
      const freed = await store.delete([key]);
      await w.addUsage(-freed);
      return json({});
    }
    throw new ApiError(405, "method", "método não suportado");
  }

  if (rest === "list" && method === "GET") {
    await w.checkMember(id);
    const dir = url.searchParams.get("dir") ?? "";
    if (!["objects", "manifests"].includes(dir)) throw new ApiError(400, "bad_path", "pasta inválida");
    return json(await blobStore(env, worldId).list(`${dir}/`));
  }
  if (rest === "blob-delete" && method === "POST") {
    await w.checkMember(id);
    const b = await bodyJson<{ dir: string; names: string[] }>(s.body);
    const keys = (b.names ?? []).slice(0, 1000).map((n) => `${b.dir}/${n}`);
    if (!keys.every(validBlobPath)) throw new ApiError(400, "bad_path", "caminho inválido");
    const freed = await blobStore(env, worldId).delete(keys);
    await w.addUsage(-freed);
    return json({ freed });
  }

  switch (`${method} ${rest.replace(/\?.*$/, "")}`) {
    case "GET view":
      return json({ ...(await w.view(id)), versao_minima: await minVersion(env) });
    case "POST presence":
      return json({ ...(await w.presence(id, await bodyJson(s.body))), versao_minima: await minVersion(env) });
    case "POST lease/acquire": {
      const b = await bodyJson<{ app_version?: string }>(s.body);
      const minima = await minVersion(env);
      // abaixo da versão mínima: não hospeda (o app também bloqueia; aqui é a garantia)
      if (minima && compareVersions(String(b.app_version ?? ""), minima) < 0) {
        throw new ApiError(409, "app_outdated", "atualize o app para hospedar", { versao_minima: minima });
      }
      return json(await w.acquire(id, b));
    }
    case "POST call": {
      const b = await bodyJson<{ to: string }>(s.body);
      return json(await w.call(id, String(b.to ?? "")));
    }
    case "POST lease/heartbeat": {
      const b = await bodyJson<{ session_number: number; online: string[]; handing_over: boolean }>(s.body);
      return json(await w.heartbeat(id, b.session_number, b.online, b.handing_over));
    }
    case "POST lease/release": {
      const b = await bodyJson<{ session_number: number; handoff?: boolean }>(s.body);
      return json(await w.release(id, b.session_number, !!b.handoff));
    }
    case "GET head":
      return json(await w.head(id));
    case "POST head": {
      const b = await bodyJson<{ session_number: number; expected: string | null; manifest: string }>(s.body);
      return json(await w.publishHead(id, b.session_number, b.expected ?? null, b.manifest));
    }
    case "POST events":
      return json(await w.postEvents(id, await bodyJson(s.body)));
    case "GET events":
      return json(await w.events(id, Number(url.searchParams.get("limit") ?? "100")));
    case "POST invites":
      return json(await w.createInvite(id, await bodyJson(s.body)));
    case "GET invites":
      return json(await w.invites(id));
    case "GET private":
      return json(await w.privateSettings(id));
    case "PUT private":
      return json(await w.setPrivateSettings(id, await bodyJson(s.body)));
  }
  let mm = /^invites\/([a-z0-9]{4,32})$/.exec(rest);
  if (mm && method === "DELETE") return json(await w.revokeInvite(id, mm[1]));
  mm = /^members\/([0-9a-f]{64})$/.exec(rest);
  if (mm && method === "POST") {
    const b = await bodyJson<{ approved: boolean | null; remove: boolean }>(s.body);
    return json(await w.setMember(id, mm[1], b.approved ?? null, !!b.remove));
  }
  throw new ApiError(404, "not_found", "rota não existe");
}
