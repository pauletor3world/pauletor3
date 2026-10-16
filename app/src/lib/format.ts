import type { LockStatus, ManifestKind, Phase, Status, SyncPhaseName } from "./types";

export function bytes(n: number): string {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${n} B` : `${v.toFixed(v < 10 ? 1 : 0).replace(".", ",")} ${u[i]}`;
}

export function duration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h === 0 && m === 0) return "menos de 1 min";
  if (h === 0) return `${m} min`;
  if (m === 0) return `${h} h`;
  return `${h} h ${m} min`;
}

export function hours(secs: number): string {
  const h = secs / 3600;
  if (h < 1) return `${Math.round(secs / 60)} min`;
  return `${h.toFixed(h < 10 ? 1 : 0).replace(".", ",")} h`;
}

const rtf = new Intl.RelativeTimeFormat("pt-BR", { numeric: "auto" });

export function ago(iso: string | null | undefined): string {
  if (!iso) return "—";
  const diff = (new Date(iso).getTime() - Date.now()) / 1000;
  const abs = Math.abs(diff);
  if (abs < 45) return "agora mesmo";
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), "hour");
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), "day");
  return new Date(iso).toLocaleDateString("pt-BR");
}

export function clock(iso: string | null | undefined): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
}

export function dateLong(iso: string): string {
  return new Date(iso).toLocaleDateString("pt-BR", { weekday: "long", day: "numeric", month: "long" });
}

export function dateTime(iso: string): string {
  return new Date(iso).toLocaleString("pt-BR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
}

export const kindLabel: Record<ManifestKind, string> = {
  session: "Sessão",
  checkpoint: "Salvamento automático",
  recovered: "Recuperado após queda",
  restore: "Restauração",
  import: "Importação",
  edit: "Ajuste de jogadores",
};

export const syncLabel: Record<SyncPhaseName, string> = {
  scanning: "Conferindo arquivos",
  uploading: "Enviando para a nuvem",
  downloading: "Baixando",
  verifying: "Verificando",
  assembling: "Montando o mundo",
  publishing: "Publicando versão",
};

export const phaseLabel: Record<Phase, string> = {
  idle: "",
  preparing: "Preparando o mundo",
  starting: "Abrindo o servidor",
  hosting: "Hospedando",
  stopping: "Salvando o mundo",
  recovering: "Recuperando a última sessão",
  maintenance: "Manutenção",
  waiting_turn: "Esperando a vez",
};

export function heldLock(l: LockStatus | null) {
  return l && l.kind === "held" ? l.lock : null;
}

/** O estado do mundo em linguagem humana. */
export function humanState(s: Status): { title: string; tone: "free" | "busy" | "live" | "warn" } {
  if (s.recovery_pending && s.phase === "idle") return { title: "Sessão interrompida neste PC — vamos recuperar", tone: "warn" };
  if (s.phase !== "idle" && s.phase !== "hosting") {
    const pct = s.sync && s.sync.bytes_total > 0 ? ` ${Math.floor((s.sync.bytes_done / s.sync.bytes_total) * 100)}%` : "";
    return { title: `${phaseLabel[s.phase]}${pct}…`, tone: "busy" };
  }
  if (s.me_hosting) {
    const n = s.online.length;
    return { title: `Você está hospedando · ${n} online`, tone: "live" };
  }
  const l = s.lock;
  if (l?.kind === "held") {
    if (l.lock.purpose === "maintenance") return { title: `${l.lock.player} está fazendo manutenção`, tone: "busy" };
    if (l.lock.handing_over) return { title: `${l.lock.player} está passando a vez`, tone: "busy" };
    return { title: `${l.lock.player} está hospedando · ${l.lock.online.length} online`, tone: "live" };
  }
  if (l?.kind === "stale") return { title: `A sessão de ${l.lock.player} parece ter caído`, tone: "warn" };
  return { title: "Livre — ninguém jogando", tone: "free" };
}

/** Cor estável por nome (para avatares e gráficos). */
export function hue(name: string): number {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h % 360;
}
