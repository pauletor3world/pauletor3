import type { LeaseStatus, ManifestKind, Phase, Status, SyncPhaseName } from "./types";

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

export function heldLease(l: LeaseStatus | null) {
  return l && l.kind === "held" ? l.lease : null;
}

/** Porcentagem da sincronização em andamento. */
export function syncPct(s: Status): number | null {
  const x = s.sync;
  if (!x) return null;
  if (x.bytes_total > 0) return Math.min(100, (x.bytes_done / x.bytes_total) * 100);
  if (x.files_total > 0) return (x.files_done / x.files_total) * 100;
  return 0;
}

/** O estado do mundo em linguagem humana. */
export function humanState(s: Status): { title: string; tone: "free" | "busy" | "live" | "warn" } {
  if (s.recovery_pending && s.phase === "idle") return { title: "Sessão interrompida neste PC — vamos recuperar", tone: "warn" };
  if (s.failover) return { title: s.failover, tone: "busy" };
  if (s.phase !== "idle" && s.phase !== "hosting") {
    const pct = syncPct(s);
    return { title: `${phaseLabel[s.phase]}${pct != null ? ` ${Math.floor(pct)}%` : ""}…`, tone: "busy" };
  }
  if (s.me_hosting) return { title: `Você está hospedando · ${s.online.length} online`, tone: "live" };
  const l = heldLease(s.lease);
  if (l) {
    if (l.purpose === "maintenance") return { title: `${l.player} está fazendo manutenção`, tone: "busy" };
    if (l.handing_over) return { title: `${l.player} está passando a vez`, tone: "busy" };
    return { title: `${l.player} está hospedando · ${l.online.length} online`, tone: "live" };
  }
  if (!s.coord_ok) return { title: "Sem conexão com a nuvem do mundo", tone: "warn" };
  if (s.replicating) return { title: "Livre · atualizando a cópia deste PC", tone: "free" };
  return { title: "Livre — clique em Jogar e o mundo acorda", tone: "free" };
}

/** Cor estável por nome (para avatares e gráficos). */
export function hue(name: string): number {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h % 360;
}
