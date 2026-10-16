// As regras da coordenação — espelho exato de `core/src/coord/rules.rs`.
// Os testes dos dois lados usam os mesmos casos.

export const LEASE_TTL_MS = 30_000;
export const FAILOVER_PREFERENCE_MS = 15_000;
export const PRESENCE_FRESH_MS = 60_000;
export const FORMAT_VERSION = 1;

export type LeasePurpose = "hosting" | "maintenance";

export interface LeaseInfo {
  format: number;
  session_number: number;
  session_id: string;
  holder: string;
  player: string;
  machine: string;
  endpoint: string | null;
  app_version: string;
  purpose: LeasePurpose;
  acquired_at: string;
  heartbeat_at: string;
  vouched_at: string | null;
  handing_over: boolean;
  online: string[];
}

export interface Presence {
  endpoint: string | null;
  replica_seq: number;
  playing: boolean;
  host_reachable: number | null;
  app_version: string;
  path?: string | null;
  /** Porta do proxy local deste PC (para o /transfer na troca de host). */
  proxy_port?: number | null;
}

export interface SeenPresence {
  member: string;
  presence: Presence;
  at: number; // ms
}

export type LeaseStatus = { kind: "free"; previous: LeaseInfo | null } | { kind: "held"; lease: LeaseInfo };

export type AcquireDecision =
  | { kind: "grant"; previous: LeaseInfo | null }
  | { kind: "held"; lease: LeaseInfo }
  | { kind: "not_best"; best_seq: number };

const ms = (iso: string | null): number => (iso ? Date.parse(iso) : 0);

export function expiresAt(l: LeaseInfo): number {
  return Math.max(ms(l.heartbeat_at), ms(l.vouched_at)) + LEASE_TTL_MS;
}

export function isExpired(l: LeaseInfo, now: number): boolean {
  return now >= expiresAt(l);
}

export function decideAcquire(
  now: number,
  lease: LeaseInfo | null,
  requester: string,
  replicaSeq: number,
  presences: SeenPresence[],
): AcquireDecision {
  if (!lease) return { kind: "grant", previous: null };
  // O próprio PC (app reiniciado depois de uma queda) pode retomar.
  if (lease.holder === requester) return { kind: "grant", previous: lease };
  if (!isExpired(lease, now)) return { kind: "held", lease };
  if (now < expiresAt(lease) + FAILOVER_PREFERENCE_MS) {
    let best = 0;
    for (const p of presences) {
      if (p.member === lease.holder || !p.presence.playing) continue;
      if (now - p.at > PRESENCE_FRESH_MS) continue;
      best = Math.max(best, p.presence.replica_seq);
    }
    if (replicaSeq < best) return { kind: "not_best", best_seq: best };
  }
  return { kind: "grant", previous: lease };
}

export function leaseStatus(now: number, lease: LeaseInfo | null): LeaseStatus {
  if (!lease) return { kind: "free", previous: null };
  if (isExpired(lease, now)) return { kind: "free", previous: lease };
  return { kind: "held", lease };
}

export function validNick(n: string): boolean {
  return /^[A-Za-z0-9_]{3,16}$/.test(n);
}

/** Ids de manifesto e hashes de objeto: o que pode virar caminho de blob. */
export function validBlobPath(path: string): boolean {
  if (/^objects\/[0-9a-f]{64}$/.test(path)) return true;
  if (/^manifests\/[A-Za-z0-9._-]{1,120}\.json$/.test(path)) return true;
  return ["versions.json", "world-settings.json", "stats.json"].includes(path);
}
