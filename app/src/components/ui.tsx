import { AlertTriangle, Check, Info, Loader2, X } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { useEffect } from "react";
import type { HumanError } from "../lib/types";

export function cx(...c: (string | false | null | undefined)[]) {
  return c.filter(Boolean).join(" ");
}

type Variant = "primary" | "secondary" | "ghost" | "danger" | "sun";

/**
 * primary: a luz da janela, para a ação principal de cada tela.
 * sun: pêssego do pôr do sol, para ações que pedem atenção (passar o host, restaurar).
 */
export function Button({
  variant = "secondary",
  busy,
  icon,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; busy?: boolean; icon?: ReactNode }) {
  const v: Record<Variant, string> = {
    primary: "bevel bg-sun-400 text-sun-900",
    sun: "bevel bg-dusk-400 text-sun-900 [--bevel-glow:rgb(240_164_116/0.5)]",
    secondary: "bg-night-700/80 text-ink hover:bg-night-600 ring-1 ring-inset ring-white/[0.07] transition-colors",
    ghost: "text-ink-soft hover:text-ink hover:bg-white/[0.06] transition-colors",
    danger: "bg-ember-500/12 text-ember-400 hover:bg-ember-500/22 ring-1 ring-inset ring-ember-500/30 transition-colors",
  };
  return (
    <button
      {...rest}
      disabled={rest.disabled || busy}
      className={cx(
        "inline-flex items-center justify-center gap-2 rounded-[10px] px-4 h-10 text-[13.5px] font-semibold whitespace-nowrap",
        "disabled:opacity-45 disabled:pointer-events-none cursor-pointer",
        v[variant],
        className,
      )}
    >
      {busy ? <Loader2 className="size-4 animate-spin" /> : icon}
      {children}
    </button>
  );
}

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return <section className={cx("rounded-[var(--radius-card)] bg-night-850/75 ring-1 ring-white/[0.06] shadow-[var(--shadow-card)]", className)}>{children}</section>;
}

export function CardTitle({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 px-5 pt-4 pb-3">
      <h3 className="text-[15px] font-bold text-ink">{children}</h3>
      {aside}
    </div>
  );
}

export function PageHeader({ title, subtitle, aside }: { title: string; subtitle?: string; aside?: ReactNode }) {
  return (
    <header className="flex items-end justify-between gap-6 mb-7">
      <div className="min-w-0">
        <h1 className="text-[34px] font-extrabold tracking-tight leading-[1.05] text-ink">{title}</h1>
        {subtitle && <p className="text-ink-muted mt-2.5 text-[14px] max-w-[60ch]">{subtitle}</p>}
      </div>
      {aside && <div className="shrink-0">{aside}</div>}
    </header>
  );
}

type Tone = "muted" | "moss" | "sun" | "ember" | "sky" | "dusk";

export function Pill({ tone = "muted", children }: { tone?: Tone; children: ReactNode }) {
  const t = {
    muted: "bg-white/[0.06] text-ink-soft",
    moss: "bg-moss-500/16 text-moss-300",
    sun: "bg-sun-400/14 text-sun-300",
    ember: "bg-ember-500/16 text-ember-400",
    sky: "bg-sky-400/14 text-sky-300",
    dusk: "bg-dusk-400/14 text-dusk-300",
  }[tone];
  return <span className={cx("inline-flex items-center gap-1.5 rounded-full px-2.5 h-6 text-[12px] font-semibold whitespace-nowrap", t)}>{children}</span>;
}

/** Ponto de estado: aceso (no ar), trabalhando, problema ou apagado. */
export function Dot({ tone, className }: { tone: "live" | "busy" | "warn" | "free"; className?: string }) {
  const c = { live: "bg-moss-400 animate-breathe", busy: "bg-sky-400 animate-breathe", warn: "bg-ember-400", free: "bg-[#7d8fd6]" }[tone];
  return <span className={cx("size-2 rounded-[2px] shrink-0", c, className)} />;
}

export function Toggle({ checked, onChange, label, hint }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <label className="flex items-center justify-between gap-6 py-3 cursor-pointer">
      <span className="min-w-0">
        <span className="block text-ink font-medium">{label}</span>
        {hint && <span className="block text-[12.5px] text-ink-muted mt-0.5 max-w-[68ch]">{hint}</span>}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={cx("relative h-6 w-11 shrink-0 rounded-full p-0 transition-colors cursor-pointer", checked ? "bg-moss-500" : "bg-night-600")}
      >
        <span
          className={cx(
            "absolute left-0 top-0.5 size-5 rounded-full bg-white shadow-[0_1px_3px_rgb(0_0_0/0.35)] transition-transform duration-200 ease-[var(--ease-out-expo)]",
            checked ? "translate-x-[22px]" : "translate-x-0.5",
          )}
        />
      </button>
    </label>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="block">
      <span className="block text-[13px] font-semibold text-ink-soft mb-1.5">{label}</span>
      {children}
      {hint && <span className="block text-[12.5px] text-ink-muted mt-1.5">{hint}</span>}
    </label>
  );
}

export const inputCls =
  "w-full h-10 rounded-[10px] bg-night-950/70 ring-1 ring-inset ring-white/[0.09] px-3 text-ink placeholder:text-ink-faint hover:ring-white/15 focus:ring-sun-400/70 outline-none transition disabled:opacity-50";

export function ErrorBanner({ error, onClose, action }: { error: HumanError | null; onClose?: () => void; action?: ReactNode }) {
  if (!error) return null;
  return (
    <div role="alert" className="flex items-start gap-3 rounded-[var(--radius-card)] bg-ember-500/10 ring-1 ring-inset ring-ember-500/25 p-4 animate-pop selectable">
      <AlertTriangle className="size-5 text-ember-400 shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <div className="font-bold text-ink">{error.title}</div>
        {error.detail && <div className="text-[13px] text-ink-soft mt-0.5 break-words">{error.detail}</div>}
        {action && <div className="mt-3">{action}</div>}
      </div>
      {onClose && (
        <button onClick={onClose} className="text-ink-muted hover:text-ink cursor-pointer" aria-label="Fechar">
          <X className="size-4" />
        </button>
      )}
    </div>
  );
}

export function InfoNote({ children, tone = "info" }: { children: ReactNode; tone?: "info" | "warn" | "ok" }) {
  const t = {
    info: ["bg-sky-400/[0.08]", <Info key="i" className="size-4 text-sky-400 shrink-0 mt-0.5" />],
    warn: ["bg-dusk-400/[0.1]", <AlertTriangle key="w" className="size-4 text-dusk-400 shrink-0 mt-0.5" />],
    ok: ["bg-moss-500/[0.1]", <Check key="o" className="size-4 text-moss-400 shrink-0 mt-0.5" />],
  }[tone];
  return (
    <div className={cx("flex gap-2.5 rounded-xl p-3 text-[13px] text-ink-soft", t[0] as string)}>
      {t[1]}
      <div>{children}</div>
    </div>
  );
}

export function Modal({
  open,
  title,
  children,
  onClose,
  footer,
  wide,
}: {
  open: boolean;
  title: string;
  children: ReactNode;
  onClose: () => void;
  footer?: ReactNode;
  wide?: boolean;
}) {
  useEffect(() => {
    if (!open) return;
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-night-950/75 backdrop-blur-[3px] animate-fade p-6" onMouseDown={onClose}>
      <div
        className={cx("w-full rounded-[20px] bg-night-800 ring-1 ring-white/10 shadow-[var(--shadow-pop)] animate-pop", wide ? "max-w-2xl" : "max-w-md")}
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="px-6 pt-6 pb-1 text-[19px] font-bold leading-snug">{title}</div>
        <div className="px-6 py-3 text-ink-soft leading-relaxed">{children}</div>
        {footer && <div className="flex justify-end gap-2 px-6 pb-6 pt-3">{footer}</div>}
      </div>
    </div>
  );
}

export function Empty({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-12 px-6 animate-fade">
      <div className="size-12 rounded-xl bg-night-700/70 flex items-center justify-center text-ink-muted mb-4">{icon}</div>
      <div className="text-[16px] font-bold">{title}</div>
      {children && <div className="text-ink-muted mt-1.5 max-w-sm">{children}</div>}
    </div>
  );
}

/** Barra de progresso real (bytes e arquivos), em blocos; nunca um spinner infinito. */
export function Progress({ value, label, sub, tone = "moss" }: { value: number; label?: string; sub?: string; tone?: "moss" | "sky" }) {
  const v = Math.max(0, Math.min(100, value));
  return (
    <div className="w-full">
      {(label || sub) && (
        <div className="flex justify-between gap-4 text-[12.5px] mb-2">
          <span className="text-ink font-semibold">{label}</span>
          <span className="text-ink-soft tabular-nums truncate">{sub}</span>
        </div>
      )}
      <div
        className="h-3 rounded-[3px] bg-night-950/80 ring-1 ring-inset ring-white/[0.06] overflow-hidden"
        role="progressbar"
        aria-valuenow={Math.round(v)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label}
      >
        <div
          className={cx("h-full blocks transition-[width] duration-500 ease-out", tone === "moss" ? "bg-moss-400" : "bg-sky-400")}
          style={{ width: `${v}%`, boxShadow: "inset 0 2px 0 rgb(255 255 255 / 0.3)" }}
        />
      </div>
    </div>
  );
}

/** Rótulo pequeno + valor, para números de uma sessão. */
export function Stat({ label, value, className }: { label: string; value: ReactNode; className?: string }) {
  return (
    <div className={cx("min-w-0", className)}>
      <div className="text-[12.5px] text-ink-muted">{label}</div>
      <div className="font-display text-[30px] leading-none mt-1.5 tabular-nums truncate">{value}</div>
    </div>
  );
}
