// Autenticação por assinatura: cada app tem uma chave Ed25519 (a mesma identidade do iroh).
// Não há senha nem conta. O pedido assina
//   p3v1\n{MÉTODO}\n{caminho?query}\n{ts}\n{sha256 do corpo | "-"}
// e manda x-p3-id (chave pública hex), x-p3-ts (ms) e x-p3-sig (hex).

const MAX_SKEW_MS = 5 * 60_000;

export class AuthError extends Error {
  constructor(
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

export function hexToBytes(h: string): Uint8Array {
  if (!/^([0-9a-f]{2})*$/i.test(h)) throw new AuthError("bad_signature", "hex inválido");
  const out = new Uint8Array(h.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function bytesToHex(b: ArrayBuffer | Uint8Array): string {
  return [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");
}

export async function sha256Hex(data: ArrayBuffer | Uint8Array | string): Promise<string> {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  return bytesToHex(await crypto.subtle.digest("SHA-256", bytes));
}

export interface Signed {
  id: string;
  body: ArrayBuffer | null;
}

/**
 * Confere a assinatura. `signBody` = o corpo entra na assinatura (JSON). Blobs não entram:
 * são endereçados por hash e conferidos por quem baixa.
 */
export async function verifyRequest(req: Request, url: URL, signBody: boolean, now = Date.now()): Promise<Signed> {
  const id = req.headers.get("x-p3-id") ?? "";
  const ts = Number(req.headers.get("x-p3-ts") ?? "0");
  const sig = req.headers.get("x-p3-sig") ?? "";
  if (!/^[0-9a-f]{64}$/.test(id) || !/^[0-9a-f]{128}$/.test(sig) || !Number.isFinite(ts)) {
    throw new AuthError("bad_signature", "pedido sem assinatura válida");
  }
  if (Math.abs(now - ts) > MAX_SKEW_MS) {
    throw new AuthError("bad_signature", "relógio do PC muito diferente da hora certa; acerte a data e a hora do Windows");
  }
  let body: ArrayBuffer | null = null;
  let bodyHash = "-";
  if (signBody && req.method !== "GET" && req.method !== "DELETE") {
    body = await req.arrayBuffer();
    bodyHash = await sha256Hex(body);
  }
  const msg = `p3v1\n${req.method}\n${url.pathname}${url.search}\n${ts}\n${bodyHash}`;
  const key = await crypto.subtle.importKey("raw", hexToBytes(id), { name: "Ed25519" }, false, ["verify"]);
  const ok = await crypto.subtle.verify({ name: "Ed25519" }, key, hexToBytes(sig), new TextEncoder().encode(msg));
  if (!ok) throw new AuthError("bad_signature", "assinatura não confere");
  return { id, body };
}

/** Compara segredos em tempo constante. */
export function safeEqual(a: string, b: string): boolean {
  const x = new TextEncoder().encode(a);
  const y = new TextEncoder().encode(b);
  if (x.length !== y.length) return false;
  let d = 0;
  for (let i = 0; i < x.length; i++) d |= x[i] ^ y[i];
  return d === 0;
}

export function randomId(bytes = 12): string {
  const b = crypto.getRandomValues(new Uint8Array(bytes));
  // base32 minúsculo sem ambiguidade (bom para links)
  const alphabet = "abcdefghijkmnpqrstuvwxyz23456789";
  let out = "";
  for (const x of b) out += alphabet[x % alphabet.length];
  return out;
}
