import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Activity,
  AlertTriangle,
  BarChart3,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Moon,
  Plus,
  Sun,
  Globe2,
  History,
  House,
  Info,
  Minus,
  ScrollText,
  Settings,
  Square,
  Users,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { humanState } from "../lib/format";
import type { Toast } from "../lib/hooks";
import type { Status } from "../lib/types";
import { Avatar } from "./Avatar";
import { Dot, cx } from "./ui";

export type Page = "home" | "world" | "timeline" | "stats" | "health" | "logs" | "players" | "settings";

/** Faixa de arrastar a janela, com os botões do Windows. */
export function TitleBar({ children, actions }: { children?: ReactNode; actions?: ReactNode }) {
  const w = getCurrentWindow();
  return (
    <div data-tauri-drag-region className="h-10 shrink-0 flex items-center justify-between pl-5">
      <div data-tauri-drag-region className="min-w-0 flex items-center gap-2 text-[12.5px] text-ink-muted">
        {children}
      </div>
      <div className="flex">
        {actions}
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
        "h-10 w-12 flex items-center justify-center text-ink-muted transition-colors cursor-pointer",
        danger ? "hover:bg-ember-500 hover:text-white" : "hover:bg-white/[0.06] hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

/** Logo + nome, no topo da barra lateral e do assistente. */
export function Brand({ className }: { className?: string }) {
  return (
    <div data-tauri-drag-region className={cx("flex items-center gap-3", className)}>
      <img src="/logo.png" alt="" className="size-9 pointer-events-none" draggable={false} />
      <span className="font-display text-[27px] leading-none text-ink pointer-events-none">Machen Craft</span>
    </div>
  );
}

/** Uso diário em cima; "Avançado" (Saúde, Console, Jogadores) recolhido; Configurações embaixo. */
const NAV: { id: Page; label: string; icon: ReactNode; owner?: boolean; group: "daily" | "advanced" | "settings" }[] = [
  { id: "home", label: "Início", icon: <House className="size-[18px]" />, group: "daily" },
  { id: "world", label: "Grupo", icon: <Globe2 className="size-[18px]" />, group: "daily" },
  { id: "timeline", label: "Diário", icon: <History className="size-[18px]" />, group: "daily" },
  { id: "stats", label: "Estatísticas", icon: <BarChart3 className="size-[18px]" />, group: "daily" },
  { id: "health", label: "Saúde", icon: <Activity className="size-[18px]" />, group: "advanced" },
  { id: "logs", label: "Console", icon: <ScrollText className="size-[18px]" />, group: "advanced" },
  { id: "players", label: "Jogadores", icon: <Users className="size-[18px]" />, owner: true, group: "advanced" },
  { id: "settings", label: "Configurações", icon: <Settings className="size-[18px]" />, group: "settings" },
];

const ADV_KEY = "machencraft.menu-avancado";

/** Tema claro/escuro (segue o sistema até a pessoa escolher). */
export function ThemeToggle({ theme, onTheme }: { theme: "light" | "dark"; onTheme: (t: "light" | "dark") => void }) {
  const next = theme === "dark" ? "light" : "dark";
  return (
    <button
      aria-label={theme === "dark" ? "Usar tema claro" : "Usar tema escuro"}
      title={theme === "dark" ? "Tema claro" : "Tema escuro"}
      onClick={() => onTheme(next)}
      className="h-10 w-11 flex items-center justify-center text-ink-muted hover:text-ink hover:bg-white/[0.06] transition-colors cursor-pointer"
    >
      {theme === "dark" ? <Sun className="size-[17px]" /> : <Moon className="size-[17px]" />}
    </button>
  );
}

export function Sidebar({
  page,
  onPage,
  isOwner,
  version,
  healthBad,
  status,
  updateAvailable,
  onUpdate,
}: {
  page: Page;
  onPage: (p: Page) => void;
  isOwner: boolean;
  version: string;
  healthBad: boolean;
  status: Status | null;
  /** Rodapé vira "Atualização disponível" (abre o cartão de atualização). */
  updateAvailable?: boolean;
  onUpdate?: () => void;
}) {
  const st = status ? humanState(status) : null;
  const short = !status
    ? "Conectando…"
    : st?.tone === "live"
      ? `Aberto, ${status.me_hosting ? status.online.length : status.lease?.kind === "held" ? status.lease.lease.online.length : 0} no jogo`
      : st?.tone === "busy"
        ? "Abrindo…"
        : st?.tone === "warn"
          ? "Precisa de atenção"
          : "Dormindo";
  const items = NAV.filter((n) => !n.owner || isOwner);
  const [adv, setAdv] = useState(() => {
    try {
      return localStorage.getItem(ADV_KEY) === "1";
    } catch {
      return false;
    }
  });
  const advActive = items.some((n) => n.group === "advanced" && n.id === page);
  const advOpen = adv || advActive;
  const toggleAdv = () => {
    const v = !adv;
    setAdv(v);
    try {
      localStorage.setItem(ADV_KEY, v ? "1" : "0");
    } catch {
      // só uma preferência de tela
    }
  };
  const [worlds, setWorlds] = useState(false);
  const worldsRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!worlds) return;
    const fora = (e: MouseEvent) => worldsRef.current && !worldsRef.current.contains(e.target as Node) && setWorlds(false);
    window.addEventListener("mousedown", fora);
    return () => window.removeEventListener("mousedown", fora);
  }, [worlds]);
  const item = (n: (typeof NAV)[number]) => {
    const active = page === n.id;
    return (
      <button
        key={n.id}
        onClick={() => onPage(n.id)}
        aria-current={active ? "page" : undefined}
        className={cx(
          "group flex items-center gap-3 h-9 px-3 rounded-[10px] text-[14px] font-semibold transition-colors cursor-pointer",
          active ? "bg-white/[0.08] text-ink" : "text-ink-muted hover:text-ink hover:bg-white/[0.04]",
        )}
      >
        <span className={cx("transition-colors", active ? "text-sun-400" : "text-ink-faint group-hover:text-ink-soft")}>{n.icon}</span>
        {n.label}
        {n.id === "health" && healthBad && <span className="ml-auto size-2 rounded-[2px] bg-dusk-400" aria-label="Algo precisa de atenção" />}
      </button>
    );
  };
  return (
    <nav className="w-[232px] shrink-0 flex flex-col bg-night-950/45 border-r border-white/[0.05]">
      <div data-tauri-drag-region className="px-5 pt-5 pb-5">
        <Brand />
      </div>

      {/* seletor de mundos (hoje um; pronto para os planos com mais mundos) */}
      <div ref={worldsRef} className="relative mx-3 mb-4">
        <button
          onClick={() => setWorlds((v) => !v)}
          aria-expanded={worlds}
          className="w-full flex items-center gap-3 rounded-xl px-3 py-2.5 text-left bg-white/[0.04] hover:bg-white/[0.07] ring-1 ring-inset ring-white/[0.05] transition-colors cursor-pointer"
        >
          <Dot tone={st?.tone ?? "free"} />
          <span className="min-w-0 flex-1">
            <span className="block text-[13.5px] font-bold text-ink truncate">{status?.world_name ?? "Seu mundo"}</span>
            <span className="block text-[12px] text-ink-muted truncate">{short}</span>
          </span>
          <ChevronDown className={cx("size-4 text-ink-faint transition-transform", worlds && "rotate-180")} />
        </button>
        {worlds && (
          <div className="absolute left-0 right-0 top-[calc(100%+6px)] z-40 rounded-xl bg-night-800 ring-1 ring-white/10 shadow-[var(--shadow-pop)] p-1.5 animate-pop">
            <button
              onClick={() => {
                setWorlds(false);
                onPage("home");
              }}
              className="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left bg-white/[0.06] cursor-pointer"
            >
              <Dot tone={st?.tone ?? "free"} />
              <span className="text-[13px] font-semibold truncate">{status?.world_name ?? "Seu mundo"}</span>
              <CheckCircle2 className="size-4 text-moss-400 ml-auto" />
            </button>
            <div className="flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] text-ink-faint" title="Mais de um mundo chega com os planos">
              <Plus className="size-4" /> Outro mundo <span className="ml-auto text-[11px]">em breve</span>
            </div>
          </div>
        )}
      </div>

      <div className="flex flex-col gap-0.5 px-3">{items.filter((n) => n.group === "daily").map(item)}</div>
      <div className="mx-6 my-3 h-px bg-white/[0.05]" />
      <div className="flex flex-col gap-0.5 px-3">
        <button
          onClick={toggleAdv}
          aria-expanded={advOpen}
          className="flex items-center gap-3 h-9 px-3 rounded-[10px] text-[13px] font-semibold text-ink-faint hover:text-ink-soft cursor-pointer"
        >
          {advOpen ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
          Avançado
          {!advOpen && healthBad && <span className="ml-auto size-2 rounded-[2px] bg-dusk-400" aria-label="Algo precisa de atenção em Saúde" />}
        </button>
        {advOpen && items.filter((n) => n.group === "advanced").map(item)}
        {items.filter((n) => n.group === "settings").map(item)}
      </div>

      <div className="mt-auto flex items-center gap-3 px-5 py-4 border-t border-white/[0.05]">
        {status?.player_name ? <Avatar name={status.player_name} size={30} /> : <span className="size-[30px] rounded-[30%] bg-night-700" />}
        <div className="min-w-0">
          <div className="text-[13px] font-bold truncate">{status?.player_name ?? "—"}</div>
          {updateAvailable ? (
            <button onClick={onUpdate} className="text-[11.5px] font-semibold text-sun-400 hover:text-sun-300 cursor-pointer">
              Atualização disponível
            </button>
          ) : (
            <div className="text-[11.5px] text-ink-faint">Versão {version}</div>
          )}
        </div>
      </div>
    </nav>
  );
}

export function Toasts({ toasts, dismiss }: { toasts: Toast[]; dismiss: (id: number) => void }) {
  return (
    <div className="fixed right-5 bottom-5 z-40 flex flex-col gap-2 w-[350px]" aria-live="polite">
      {toasts.map((t) => {
        const icon =
          t.level === "success" ? (
            <CheckCircle2 className="size-5 text-moss-400" />
          ) : t.level === "error" ? (
            <AlertTriangle className="size-5 text-ember-400" />
          ) : t.level === "warning" ? (
            <AlertTriangle className="size-5 text-dusk-400" />
          ) : (
            <Info className="size-5 text-sky-400" />
          );
        return (
          <div key={t.id} className="flex gap-3 rounded-2xl bg-night-800 ring-1 ring-white/10 shadow-[var(--shadow-pop)] p-3.5 animate-pop">
            <div className="shrink-0 mt-0.5">{icon}</div>
            <div className="flex-1 min-w-0">
              <div className="font-bold text-[14px]">{t.title}</div>
              {t.body && <div className="text-[13px] text-ink-soft mt-0.5">{t.body}</div>}
            </div>
            <button onClick={() => dismiss(t.id)} className="text-ink-faint hover:text-ink cursor-pointer self-start" aria-label="Fechar">
              <X className="size-4" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
