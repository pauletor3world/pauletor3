import { exports } from "cloudflare:workers";
import { describe, expect, it } from "vitest";
import { bytesToHex, sha256Hex } from "../src/auth";
import { compareVersions } from "../src/app-dist";
import { decideAcquire, handoffVersionConflict, LEASE_TTL_MS, FAILOVER_PREFERENCE_MS, type LeaseInfo } from "../src/rules";

const BASE = "https://coord.test";
const worker = (exports as unknown as { default: { fetch(r: Request): Promise<Response> } }).default;

interface Who {
  id: string;
  key: CryptoKey;
}

async function newApp(): Promise<Who> {
  const kp = (await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"])) as CryptoKeyPair;
  const raw = await crypto.subtle.exportKey("raw", kp.publicKey);
  return { id: bytesToHex(raw as ArrayBuffer), key: kp.privateKey };
}

async function call(who: Who, method: string, path: string, body?: unknown, opts: { blob?: Uint8Array; headers?: Record<string, string>; ts?: number } = {}) {
  const ts = opts.ts ?? Date.now();
  const url = new URL(BASE + path);
  let payload: BodyInit | undefined;
  let bodyHash = "-";
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  if (opts.blob) {
    payload = opts.blob;
    headers["content-length"] = String(opts.blob.length);
  } else if (body !== undefined) {
    const s = JSON.stringify(body);
    payload = s;
    bodyHash = await sha256Hex(s);
    headers["content-type"] = "application/json";
  }
  const msg = `p3v1\n${method}\n${url.pathname}${url.search}\n${ts}\n${bodyHash}`;
  const sig = await crypto.subtle.sign({ name: "Ed25519" }, who.key, new TextEncoder().encode(msg));
  headers["x-p3-id"] = who.id;
  headers["x-p3-ts"] = String(ts);
  headers["x-p3-sig"] = bytesToHex(sig);
  return worker.fetch(new Request(url, { method, headers, body: payload }));
}

async function ok<T = any>(r: Response | Promise<Response>): Promise<T> {
  const res = await r;
  const text = await res.text();
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${text}`);
  return JSON.parse(text) as T;
}

async function err(r: Response | Promise<Response>): Promise<{ status: number; error: string; [k: string]: any }> {
  const res = await r;
  const body = (await res.json()) as any;
  expect(res.ok).toBe(false);
  return { status: res.status, ...body };
}

async function world() {
  const owner = await newApp();
  const { world_id } = await ok(call(owner, "POST", "/v1/worlds", { name: "pAuletOr 3", nick: "mchado" }, { headers: { "x-p3-admin": "admin-de-teste" } }));
  return { owner, w: `/v1/worlds/${world_id}`, world_id };
}

async function join(owner: Who, w: string, nick: string, preapproved = true) {
  const friend = await newApp();
  const inv = await ok(call(owner, "POST", `${w}/invites`, { hours: 1, uses: 1, preapproved }));
  const r = await ok(call(friend, "POST", `/v1/join/${inv.token}`, { nick }));
  return { friend, inv, r };
}

const acquireReq = (sid: string, seq = 0) => ({
  session_id: sid,
  player: "x",
  machine: "pc",
  endpoint: null,
  purpose: "hosting",
  replica_seq: seq,
  app_version: "t",
});

describe("autenticação", () => {
  it("cria mundo só com a chave de administração", async () => {
    const a = await newApp();
    const e = await err(call(a, "POST", "/v1/worlds", { name: "x", nick: "abc" }, { headers: { "x-p3-admin": "errada" } }));
    expect(e.error).toBe("forbidden");
  });

  it("recusa assinatura errada e relógio fora", async () => {
    const { owner, w } = await world();
    const other = await newApp();
    // assinatura de outra chave com o id do dono
    const forged = { id: owner.id, key: other.key };
    expect((await err(call(forged, "GET", `${w}/view`))).error).toBe("bad_signature");
    expect((await err(call(owner, "GET", `${w}/view`, undefined, { ts: Date.now() - 3_600_000 }))).error).toBe("bad_signature");
  });

  it("estranhos não leem nada do mundo", async () => {
    const { w } = await world();
    const stranger = await newApp();
    expect((await err(call(stranger, "GET", `${w}/view`))).error).toBe("not_member");
    expect((await err(call(stranger, "GET", `${w}/blob/versions.json`))).error).toBe("not_member");
  });
});

describe("convites", () => {
  it("link abre a página, entra uma vez, respeita aprovação", async () => {
    const { owner, w } = await world();
    const inv = await ok(call(owner, "POST", `${w}/invites`, { hours: 1, uses: 1, preapproved: false }));
    const page = await worker.fetch(new Request(`${BASE}/c/${inv.token}`));
    const html = await page.text();
    expect(html).toContain("mchado");
    expect(html).toContain("pAuletOr 3");
    expect(html).toContain("não afiliado à Mojang");

    const friend = await newApp();
    const r = await ok(call(friend, "POST", `/v1/join/${inv.token}`, { nick: "Sara_1" }));
    expect(r).toMatchObject({ name: "pAuletOr 3", approved: false, owner_nick: "mchado" });
    // pendente: vê só o básico e não opera
    const v = await ok(call(friend, "GET", `${w}/view`));
    expect(v.members).toEqual([]);
    expect((await err(call(friend, "GET", `${w}/head`))).error).toBe("pending_approval");
    // uso único
    const third = await newApp();
    expect((await err(call(third, "POST", `/v1/join/${inv.token}`, { nick: "Outro" }))).error).toBe("invite_invalid");
    // página do convite gasto
    expect(await (await worker.fetch(new Request(`${BASE}/c/${inv.token}`))).text()).toContain("não vale mais");
    // dono aprova
    await ok(call(owner, "POST", `${w}/members/${friend.id}`, { approved: true, remove: false }));
    await ok(call(friend, "GET", `${w}/head`));
    // amigo não convida
    expect((await err(call(friend, "POST", `${w}/invites`, {}))).error).toBe("forbidden");
  });

  it("nick repetido e revogação", async () => {
    const { owner, w } = await world();
    const inv = await ok(call(owner, "POST", `${w}/invites`, { hours: 1, uses: 3 }));
    const f = await newApp();
    expect((await err(call(f, "POST", `/v1/join/${inv.token}`, { nick: "MCHADO" }))).error).toBe("nick_taken");
    await ok(call(owner, "DELETE", `${w}/invites/${inv.id}`));
    expect((await err(call(f, "POST", `/v1/join/${inv.token}`, { nick: "Bia_2" }))).error).toBe("invite_invalid");
  });
});

describe("vez de hospedar e HEAD", () => {
  it("um host por vez, CAS no HEAD, session_number monotônico", async () => {
    const { owner, w } = await world();
    const { friend } = await join(owner, w, "Bia_2");
    const la = await ok<LeaseInfo>(call(owner, "POST", `${w}/lease/acquire`, acquireReq("s1")));
    const held = await err(call(friend, "POST", `${w}/lease/acquire`, acquireReq("s2")));
    expect(held.error).toBe("held_by");
    expect(held.lease.holder).toBe(owner.id);

    expect((await err(call(friend, "POST", `${w}/head`, { session_number: la.session_number, expected: null, manifest: "m1" }))).error).toBe("lost_lock");
    const h = await ok(call(owner, "POST", `${w}/head`, { session_number: la.session_number, expected: null, manifest: "m1" }));
    expect(h.seq).toBe(1);
    const moved = await err(call(owner, "POST", `${w}/head`, { session_number: la.session_number, expected: null, manifest: "m2" }));
    expect(moved).toMatchObject({ error: "head_moved", current: "m1" });

    await ok(call(owner, "POST", `${w}/lease/heartbeat`, { session_number: la.session_number, online: ["mchado"], handing_over: false }));
    await ok(call(owner, "POST", `${w}/lease/release`, { session_number: la.session_number }));
    const lb = await ok<LeaseInfo>(call(friend, "POST", `${w}/lease/acquire`, acquireReq("s2", 1)));
    expect(lb.session_number).toBeGreaterThan(la.session_number);
    // a vez velha não publica nem bate mais
    expect((await err(call(owner, "POST", `${w}/head`, { session_number: la.session_number, expected: "m1", manifest: "m3" }))).error).toBe("lost_lock");
    expect((await err(call(owner, "POST", `${w}/lease/heartbeat`, { session_number: la.session_number, online: [], handing_over: false }))).error).toBe("lost_lock");
  });

  it("passar o host: a vez fica vencida com handing_over e quem joga assume", async () => {
    const { owner, w } = await world();
    const { friend } = await join(owner, w, "Amigo");
    const la = await ok<LeaseInfo>(call(owner, "POST", `${w}/lease/acquire`, acquireReq("s1")));
    await ok(call(owner, "POST", `${w}/lease/release`, { session_number: la.session_number, handoff: true }));
    const v = await ok<{ lease: { kind: string; previous: LeaseInfo | null } }>(call(friend, "GET", `${w}/view`));
    expect(v.lease.kind).toBe("free");
    expect(v.lease.previous?.handing_over).toBe(true);
    expect(v.lease.previous?.holder).toBe(owner.id);
    const lb = await ok<LeaseInfo>(call(friend, "POST", `${w}/lease/acquire`, acquireReq("s2", 0)));
    expect(lb.holder).toBe(friend.id);
  });

  it("corrida: 6 pedidos ao mesmo tempo, um vencedor", async () => {
    const { owner, w } = await world();
    const apps = [owner, ...(await Promise.all([1, 2, 3, 4, 5].map((i) => join(owner, w, `Amigo${i}`).then((j) => j.friend))))];
    const rs = await Promise.all(apps.map((a, i) => call(a, "POST", `${w}/lease/acquire`, acquireReq(`s${i}`))));
    expect(rs.filter((r: Response) => r.ok).length).toBe(1);
  });

  it("presença devolve a fotografia e marca quem está online", async () => {
    const { owner, w } = await world();
    const { friend } = await join(owner, w, "Bia_2");
    const v = await ok(call(friend, "POST", `${w}/presence`, { endpoint: "{}", replica_seq: 3, playing: true, host_reachable: null, app_version: "t" }));
    const me = v.members.find((m: any) => m.id === friend.id);
    expect(me.presence).toMatchObject({ replica_seq: 3, playing: true });
    expect(v.lease.kind).toBe("free");
  });
});

describe("regras (mesmos casos do Rust)", () => {
  const lease = (holder: string, hb: number): LeaseInfo => ({
    format: 1,
    session_number: 7,
    session_id: "s",
    holder,
    player: "Ana",
    machine: "pc",
    endpoint: null,
    app_version: "x",
    purpose: "hosting",
    acquired_at: new Date(hb).toISOString(),
    heartbeat_at: new Date(hb).toISOString(),
    vouched_at: null,
    handing_over: false,
    online: [],
  });
  const pres = (seq: number, playing: boolean) => ({ endpoint: null, replica_seq: seq, playing, host_reachable: null, app_version: "" });

  it("failover prefere a réplica mais nova e depois abre", () => {
    const now = Date.now();
    const l = lease("a", now - LEASE_TTL_MS - 5000);
    const seen = [
      { member: "b", presence: pres(10, true), at: now },
      { member: "c", presence: pres(8, true), at: now },
    ];
    expect(decideAcquire(now, l, "c", 8, seen)).toEqual({ kind: "not_best", best_seq: 10 });
    expect(decideAcquire(now, l, "b", 10, seen).kind).toBe("grant");
    expect(decideAcquire(now + FAILOVER_PREFERENCE_MS + 1000, l, "c", 8, seen).kind).toBe("grant");
  });

  it("aviso de membro mantém a vez viva", () => {
    const now = Date.now();
    const l = lease("a", now - 120_000);
    l.vouched_at = new Date(now - 5000).toISOString();
    expect(decideAcquire(now, l, "b", 0, []).kind).toBe("held");
  });
});

describe("nuvem do mundo", () => {
  it("guarda, lista, lê e apaga; arquivos grandes em pedaços", async () => {
    const { owner, w } = await world();
    const big = new Uint8Array(4_200_000);
    for (let i = 0; i < big.length; i++) big[i] = (i * 31) & 0xff;
    const hash = await sha256Hex(big);
    await ok(call(owner, "PUT", `${w}/blob/objects/${hash}`, undefined, { blob: big }));
    const small = new TextEncoder().encode('{"v":1}');
    await ok(call(owner, "PUT", `${w}/blob/versions.json`, undefined, { blob: small }));

    const got = await call(owner, "GET", `${w}/blob/objects/${hash}`);
    expect(got.status).toBe(200);
    const back = new Uint8Array(await got.arrayBuffer());
    expect(back.length).toBe(big.length);
    expect(await sha256Hex(back)).toBe(hash);

    const list = await ok(call(owner, "GET", `${w}/list?dir=objects`));
    expect(list).toEqual([{ name: hash, size: big.length }]);

    // substituir não duplica o uso
    await ok(call(owner, "PUT", `${w}/blob/versions.json`, undefined, { blob: small }));
    await ok(call(owner, "POST", `${w}/blob-delete`, { dir: "objects", names: [hash] }));
    expect((await call(owner, "GET", `${w}/blob/objects/${hash}`)).status).toBe(404);
    expect(await ok(call(owner, "GET", `${w}/list?dir=objects`))).toEqual([]);
  });

  it("cota e caminhos inválidos", async () => {
    const { owner, w } = await world();
    const tooBig = new Uint8Array(5_000_001);
    const e = await err(call(owner, "PUT", `${w}/blob/objects/${"a".repeat(64)}`, undefined, { blob: tooBig }));
    expect(e.error).toBe("quota");
    expect((await err(call(owner, "GET", `${w}/blob/..%2Fetc`))).error).toBe("bad_path");
    expect((await err(call(owner, "GET", `${w}/blob/objects/naoehash`))).error).toBe("bad_path");
  });
});

describe("distribuição do app pelo Worker", () => {
  it("instalador só com convite válido; atualização só para membros", async () => {
    const { owner, w, world_id } = await world();
    const exe = new Uint8Array(3_000_000).fill(77);
    const put = await worker.fetch(
      new Request(`${BASE}/v1/app/installer`, {
        method: "PUT",
        headers: { "x-p3-admin": "admin-de-teste", "x-p3-version": "2.0.0", "x-p3-updater-signature": "c2ln", "content-length": String(exe.length) },
        body: exe,
      }),
    );
    expect(put.status).toBe(200);
    const inv = await ok(call(owner, "POST", `${w}/invites`, { hours: 1, uses: 1 }));
    const page = await (await worker.fetch(new Request(`${BASE}/c/${inv.token}`))).text();
    expect(page).toContain(`/d/${inv.token}`);
    const dl = await worker.fetch(new Request(`${BASE}/d/${inv.token}`));
    expect(dl.status).toBe(200);
    expect(dl.headers.get("content-disposition")).toContain("Machenia-2.0.0-setup.exe");
    expect((await dl.arrayBuffer()).byteLength).toBe(exe.length);
    expect((await worker.fetch(new Request(`${BASE}/d/${world_id}.naovalenada`))).status).toBe(403);

    // updater: assinatura sobre o prefixo /u/:mundo
    const signed = async (who: Who, path: string) => {
      const ts = Date.now();
      const msg = `p3v1\nGET\n/u/${world_id}\n${ts}\n-`;
      const sig = await crypto.subtle.sign({ name: "Ed25519" }, who.key, new TextEncoder().encode(msg));
      return worker.fetch(new Request(`${BASE}${path}`, { headers: { "x-p3-id": who.id, "x-p3-ts": String(ts), "x-p3-sig": bytesToHex(sig) } }));
    };
    const latest = await signed(owner, `/u/${world_id}/latest.json`);
    expect(latest.status).toBe(200);
    const j = (await latest.json()) as any;
    expect(j.version).toBe("2.0.0");
    expect(j.platforms["windows-x86_64"].url).toBe(`${BASE}/u/${world_id}/installer`);
    expect((await signed(owner, `/u/${world_id}/installer`)).status).toBe(200);
    const stranger = await newApp();
    expect((await signed(stranger, `/u/${world_id}/latest.json`)).status).toBe(403);
  });
});

describe("versão do app", () => {
  const admin = (method: string, path: string, body?: unknown, headers: Record<string, string> = {}) =>
    worker.fetch(
      new Request(`${BASE}${path}`, {
        method,
        headers: { "x-p3-admin": "admin-de-teste", "content-type": "application/json", ...headers },
        body: body === undefined ? undefined : JSON.stringify(body),
      }),
    );

  it("compara versões", () => {
    expect(compareVersions("2.0.3", "2.1.0")).toBe(-1);
    expect(compareVersions("2.1.0", "2.1.0")).toBe(0);
    expect(compareVersions("2.10.0", "2.9.9")).toBe(1);
    expect(compareVersions("2.1.0-beta.1", "2.1.0")).toBe(-1);
    expect(compareVersions("t", "2.1.0")).toBe(0);
  });

  it("troca planejada só entre a mesma versão (mesmos casos do Rust)", () => {
    const now = Date.now();
    const l: LeaseInfo = {
      format: 1, session_number: 7, session_id: "s", holder: "a", player: "Ana", machine: "pc", endpoint: null,
      app_version: "2.0.3", purpose: "hosting", acquired_at: new Date(now - LEASE_TTL_MS).toISOString(),
      heartbeat_at: new Date(now - LEASE_TTL_MS).toISOString(), vouched_at: null, handing_over: true, online: [],
    };
    expect(handoffVersionConflict(now, l, "b", "2.1.0")).toBe(true);
    expect(handoffVersionConflict(now, l, "b", "2.0.3")).toBe(false);
    expect(handoffVersionConflict(now, l, "a", "2.1.0")).toBe(false);
    expect(handoffVersionConflict(now + FAILOVER_PREFERENCE_MS + 1000, l, "b", "2.1.0")).toBe(false);
    expect(handoffVersionConflict(now, { ...l, handing_over: false }, "b", "2.1.0")).toBe(false);
  });

  it("a coordenação recusa a troca planejada entre versões diferentes", async () => {
    const { owner, w } = await world();
    const { friend } = await join(owner, w, "Amigo_v");
    const la = await ok<LeaseInfo>(call(owner, "POST", `${w}/lease/acquire`, { ...acquireReq("s1"), app_version: "2.0.3" }));
    await ok(call(owner, "POST", `${w}/lease/release`, { session_number: la.session_number, handoff: true }));
    const e = await err(call(friend, "POST", `${w}/lease/acquire`, { ...acquireReq("s2"), app_version: "2.1.0" }));
    expect(e.error).toBe("app_version_mismatch");
    expect(e.host_version).toBe("2.0.3");
    await ok(call(friend, "POST", `${w}/lease/acquire`, { ...acquireReq("s3"), app_version: "2.0.3" }));
  });

  it("versão mínima: o dono define sem publicar app, chega na fotografia e bloqueia hospedar", async () => {
    const { owner, w } = await world();
    expect((await admin("PUT", "/v1/app/versao-minima", { versao_minima: "2.1.0" }, { "x-p3-admin": "errada" })).status).toBe(403);
    expect((await admin("PUT", "/v1/app/versao-minima", { versao_minima: "abc" })).status).toBe(400);
    expect((await admin("PUT", "/v1/app/versao-minima", { versao_minima: "2.1.0" })).status).toBe(200);
    const v = await ok(call(owner, "GET", `${w}/view`));
    expect(v.versao_minima).toBe("2.1.0");
    const p = await ok(call(owner, "POST", `${w}/presence`, { endpoint: null, replica_seq: 0, playing: false, host_reachable: null, app_version: "2.0.3" }));
    expect(p.versao_minima).toBe("2.1.0");
    const e = await err(call(owner, "POST", `${w}/lease/acquire`, { ...acquireReq("s1"), app_version: "2.0.3" }));
    expect(e.error).toBe("app_outdated");
    expect(e.versao_minima).toBe("2.1.0");
    await ok(call(owner, "POST", `${w}/lease/release`, { session_number: (await ok<LeaseInfo>(call(owner, "POST", `${w}/lease/acquire`, { ...acquireReq("s2"), app_version: "2.1.0" }))).session_number }));
    expect((await admin("PUT", "/v1/app/versao-minima", { versao_minima: null })).status).toBe(200);
    expect((await ok(call(owner, "GET", `${w}/view`))).versao_minima).toBe(null);
  });

  it("notas em UTF-8, obrigatória e canal de teste separado", async () => {
    const { owner, world_id } = await world();
    const notas = "Correções e atualização automática ✓";
    const b64 = btoa(String.fromCharCode(...new TextEncoder().encode(notas)));
    const put = (q: string, version: string, extra: Record<string, string> = {}) =>
      worker.fetch(
        new Request(`${BASE}/v1/app/installer${q}`, {
          method: "PUT",
          headers: { "x-p3-admin": "admin-de-teste", "x-p3-version": version, "x-p3-updater-signature": "c2ln", "x-p3-notes-b64": b64, "content-length": "10", ...extra },
          body: new Uint8Array(10).fill(1),
        }),
      );
    expect((await put("", "2.2.0", { "x-p3-obrigatoria": "1" })).status).toBe(200);
    expect((await put("?canal=teste", "2.3.0-beta.1")).status).toBe(200);
    const signed = async (path: string) => {
      const ts = Date.now();
      const msg = `p3v1
GET
/u/${world_id}
${ts}
-`;
      const sig = await crypto.subtle.sign({ name: "Ed25519" }, owner.key, new TextEncoder().encode(msg));
      return worker.fetch(new Request(`${BASE}${path}`, { headers: { "x-p3-id": owner.id, "x-p3-ts": String(ts), "x-p3-sig": bytesToHex(sig) } }));
    };
    const est = (await (await signed(`/u/${world_id}/latest.json`)).json()) as any;
    expect(est.version).toBe("2.2.0");
    expect(est.notes).toBe(notas);
    expect(est.obrigatoria).toBe(true);
    const teste = (await (await signed(`/u/${world_id}/latest.json?canal=teste`)).json()) as any;
    expect(teste.version).toBe("2.3.0-beta.1");
    expect(teste.platforms["windows-x86_64"].url).toContain("canal=teste");
    expect((await signed(`/u/${world_id}/installer?canal=teste`)).status).toBe(200);
    expect((await put("", "2.2.1", { "x-p3-updater-signature": "" })).status).toBe(400);
  });
});

describe("chamar pro jogo", () => {
  it("o chamado chega uma vez, na presença de quem foi chamado", async () => {
    const { owner, w } = await world();
    const { friend } = await join(owner, w, "Bia_c");
    await ok(call(friend, "POST", `${w}/call`, { to: owner.id }));
    const p = { endpoint: null, replica_seq: 0, playing: false, host_reachable: null, app_version: "t" };
    const v1 = await ok(call(owner, "POST", `${w}/presence`, p));
    expect(v1.chamado?.de).toBe("Bia_c");
    const v2 = await ok(call(owner, "POST", `${w}/presence`, p));
    expect(v2.chamado).toBeUndefined();
    expect((await err(call(friend, "POST", `${w}/call`, { to: "naoexiste" }))).error).toBe("not_member");
  });
});
