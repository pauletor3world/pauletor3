import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Activity,
  BarChart3,
  CheckCircle2,
  History,
  Home,
  Minus,
  ScrollText,
  Settings,
  Square,
  Users,
  Globe2,
  X,
  AlertTriangle,
  Info,
} from "lucide-react";
import type { ReactNode } from "react";
import { humanState } from "../lib/format";
import type { Toast } from "../lib/hooks";
import type { Status } from "../lib/types";
import { cx } from "./ui";

export type Page = "home" | "world" | "timeline" | "stats" | "health" | "logs" | "players" | "settings";

export function TitleBar({ status }: { status: Status | null }) {
  const w = getCurrentWindow();
  const st = status ? humanState(status) : null;
  const dot = st
    ? { free: "bg-ink-faint", busy: "bg-sun-400 animate-breathe", live: "bg-moss-400 animate-breathe", warn: "bg-ember-400" }[st.tone]
    : "bg-ink-faint";
  return (
    <div data-tauri-drag-region className="h-10 shrink-0 flex items-center justify-between pl-4 pr-1 bg-night-950/60 border-b border-white/[0.05]">
      <div data-tauri-drag-region className="flex items-center gap-2.5 pointer-events-none">
        <img src="/logo.png" alt="" className="size-5 rounded-md" />
        <span className="font-display font-semibold tracking-tight text-[13px]">pAuletOr 3</span>
        {st && (
          <span className="flex items-center gap-1.5 text-[11.5px] text-ink-muted ml-2">
            <span className={cx("size-1.5 rounded-full", dot)} />
            {st.title}
          </span>
        )}
      </div>
      <div className="flex">
        <WinBtn label="Minimizar" onClick={() => w.minimize()}>
          <Minus className="size-3.5" />
        </WinBtn>
        <WinBtn label="Maximizar" onClick={() => w.toggleMaximize()}>
          <Square className="size-3" />
        </WinBtn>
        <WinBtn label="Fechar para a bandeja" danger onClick={() => w.hide()}>
          <X className="size-4" />
        </WinBtn>
      </div>
    </div>
  );
}

function WinBtn({ children, onClick, label, danger }: { children: ReactNode; onClick: () => void; label: string; danger?: boolean }) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cx(
        "h-10 w-11 flex items-center justify-center text-ink-muted transition-colors cursor-pointer",
        danger ? "hover:bg-ember-500 hover:text-white" : "hover:bg-white/5 hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

const NAV: { id: Page; label: string; icon: ReactNode; owner?: boolean }[] = [
  { id: "home", label: "Início", icon: <Home className="size-[18px]" /> },
  { id: "world", label: "Mundo e turma", icon: <Globe2 className="size-[18px]" /> },
  { id: "timeline", label: "Diário", icon: <History className="size-[18px]" /> },
  { id: "stats", label: "Estatísticas", icon: <BarChart3 className="size-[18px]" /> },
  { id: "health", label: "Saúde", icon: <Activity className="size-[18px]" /> },
  { id: "logs", label: "Console", icon: <ScrollText className="size-[18px]" /> },
  { id: "players", label: "Jogadores", icon: <Users className="size-[18px]" />, owner: true },
  { id: "settings", label: "Configurações", icon: <Settings className="size-[18px]" /> },
];

export function Sidebar({ page, onPage, isOwner, version, healthBad }: { page: Page; onPage: (p: Page) => void; isOwner: boolean; version: string; healthBad: boolean }) {
  return (
    <nav className="w-[210px] shrink-0 flex flex-col gap-1 p-3 border-r border-white/[0.05] bg-night-950/30">
      {NAV.filter((n) => !n.owner || isOwner).map((n) => (
        <button
          key={n.id}
          onClick={() => onPage(n.id)}
          className={cx(
            "group relative flex items-center gap-3 h-10 px-3 rounded-xl text-[13.5px] font-medium transition-all cursor-pointer",
            page === n.id ? "bg-white/[0.07] text-ink" : "text-ink-muted hover:text-ink hover:bg-white/[0.03]",
          )}
        >
          {page === n.id && <span className="absolute left-0 top-2.5 bottom-2.5 w-[3px] rounded-full bg-moss-400" />}
          <span className={cx(page === n.id ? "text-moss-400" : "text-ink-faint group-hover:text-ink-soft")}>{n.icon}</span>
          {n.label}
          {n.id === "health" && healthBad && <span className="ml-auto size-2 rounded-full bg-sun-400" />}
        </button>
      ))}
      <div className="mt-auto px-3 pt-4 text-[11px] text-ink-faint">pAuletOr 3 · v{version}</div>
    </nav>
  );
}

export function Toasts({ toasts, dismiss }: { toasts: Toast[]; dismiss: (id: number) => void }) {
  return (
    <div className="fixed right-4 bottom-4 z-40 flex flex-col gap-2 w-[340px]">
      {toasts.map((t) => {
        const icon =
          t.level === "success" ? (
            <CheckCircle2 className="size-5 text-moss-400" />
          ) : t.level === "error" ? (
            <AlertTriangle className="size-5 text-ember-400" />
          ) : t.level === "warning" ? (
            <AlertTriangle className="size-5 text-sun-400" />
          ) : (
            <Info className="size-5 text-sky-400" />
          );
        return (
          <div key={t.id} className="flex gap-3 rounded-2xl bg-night-800/95 ring-1 ring-white/10 shadow-2xl p-3.5 animate-rise backdrop-blur">
            <div className="shrink-0 mt-0.5">{icon}</div>
            <div className="flex-1 min-w-0">
              <div className="font-semibold text-[13.5px]">{t.title}</div>
              {t.body && <div className="text-[12.5px] text-ink-soft mt-0.5">{t.body}</div>}
            </div>
            <button onClick={() => dismiss(t.id)} className="text-ink-faint hover:text-ink cursor-pointer" aria-label="Fechar">
              <X className="size-4" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
