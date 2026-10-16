// Distribuição do app pelo próprio Worker do dono (e não por um site público):
// - GET  /d/:mundo.:segredo           instalador, para quem tem um convite válido;
// - GET  /u/:mundo/latest.json        atualizações (formato do updater do Tauri), só membros;
// - GET  /u/:mundo/installer          o instalador da atualização, só membros;
// - PUT  /v1/app/installer            o dono publica uma versão (x-p3-admin).
//
// Membros se autenticam no updater com a mesma assinatura Ed25519 do app, sobre o prefixo
// `/u/:mundo` (o updater usa os mesmos cabeçalhos para o JSON e para o download).

import { safeEqual, verifyRequest } from "./auth";
import { blobStore } from "./blobs";
import type { Env } from "./index";
import { ApiError } from "./world";

const APP_SPACE = "_app";
const KEY = "versions.json"; // reaproveita uma chave aceita pelo armazenamento (metadados)
const BIN = "objects/" + "0".repeat(64); // posição fixa do instalador atual

interface Release {
  version: string;
  notes: string;
  signature: string;
  pub_date: string;
  size: number;
}

async function release(env: Env): Promise<Release | null> {
  const o = await blobStore(env, APP_SPACE).get(KEY);
  if (!o) return null;
  return JSON.parse(await new Response(o.body).text()) as Release;
}

export async function publishInstaller(req: Request, env: Env): Promise<Response> {
  const admin = req.headers.get("x-p3-admin") ?? "";
  if (!env.ADMIN_TOKEN || !safeEqual(admin.trim(), env.ADMIN_TOKEN.trim())) throw new ApiError(403, "forbidden", "chave de administração inválida");
  const version = req.headers.get("x-p3-version") ?? "";
  const signature = req.headers.get("x-p3-updater-signature") ?? "";
  const size = Number(req.headers.get("content-length") ?? "-1");
  if (!/^\d+\.\d+\.\d+([-+][\w.]+)?$/.test(version)) throw new ApiError(400, "bad_version", "versão inválida");
  if (!(size > 0) || !req.body) throw new ApiError(411, "length_required", "tamanho do instalador não informado");
  const store = blobStore(env, APP_SPACE);
  await store.put(BIN, req.body, size);
  const meta: Release = { version, notes: req.headers.get("x-p3-notes") ?? "", signature, pub_date: new Date().toISOString(), size };
  const bytes = new TextEncoder().encode(JSON.stringify(meta));
  await store.put(KEY, new Blob([bytes]).stream(), bytes.length);
  return Response.json({ ok: true, version });
}

async function serveInstaller(env: Env): Promise<Response | null> {
  const r = await release(env);
  const o = r ? await blobStore(env, APP_SPACE).get(BIN) : null;
  if (!r || !o) return null;
  return new Response(o.body, {
    headers: {
      "content-type": "application/vnd.microsoft.portable-executable",
      "content-length": String(o.size),
      "content-disposition": `attachment; filename="pAuletOr3-${r.version}-setup.exe"`,
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
  if (rest === "latest.json") {
    const r = await release(env);
    if (!r) return new Response(null, { status: 204 });
    return Response.json({
      version: r.version,
      notes: r.notes,
      pub_date: r.pub_date,
      platforms: { "windows-x86_64": { signature: r.signature, url: `${url.origin}/u/${worldId}/installer` } },
    });
  }
  if (rest === "installer") {
    return (await serveInstaller(env)) ?? new Response(null, { status: 404 });
  }
  throw new ApiError(404, "not_found", "rota não existe");
}
