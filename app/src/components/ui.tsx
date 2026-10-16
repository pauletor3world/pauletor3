import { AlertTriangle, Check, Info, Loader2, X } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { useEffect } from "react";
import type { HumanError } from "../lib/types";

export function cx(...c: (string | false | null | undefined)[]) {
  return c.filter(Boolean).join(" ");
}

type Variant = "primary" | "secondary" | "ghost" | "danger" | "sun";

export function Button({
  variant = "secondary",
  busy,
  icon,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; busy?: boolean; icon?: ReactNode }) {
  const v: Record<Variant, string> = {
    primary: "bg-moss-500 text-night-950 hover:bg-moss-400 shadow-[var(--shadow-glow)]",
    sun: "bg-sun-400 text-night-950 hover:bg-sun-300",
    secondary: "bg-night-700 text-ink hover:bg-night-600 ring-1 ring-white/5",
    ghost: "text-ink-soft hover:text-ink hover:bg-white/5",
    danger: "bg-ember-500/15 text-ember-400 hover:bg-ember-500/25 ring-1 ring-ember-500/30",
  };
  return (
    <button
      {...rest}
      disabled={rest.disabled || busy}
      className={cx(
        "inline-flex items-center justify-center gap-2 rounded-xl px-4 h-10 text-[13px] font-semibold transition-all duration-150",
        "active:scale-[0.98] disabled:opacity-50 disabled:pointer-events-none cursor-pointer",
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
  return (
    <section
      className={cx(
        "rounded-[var(--radius-card)] bg-night-850/80 ring-1 ring-white/[0.06] shadow-[var(--shadow-card)] backdrop-blur-sm",
        className,
      )}
    >
      {children}
    </section>
  );
}

export function CardTitle({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 px-5 pt-4 pb-2">
      <h3 className="font-display text-[15px] font-semibold tracking-tight text-ink">{children}</h3>
      {aside}
    </div>
  );
}

export function PageHeader({ title, subtitle, aside }: { title: string; subtitle?: string; aside?: ReactNode }) {
  return (
    <header className="flex items-end justify-between gap-4 mb-6 animate-rise">
      <div>
        <h1 className="font-display text-[28px] leading-tight font-bold tracking-tight">{title}</h1>
        {subtitle && <p className="text-ink-muted mt-1">{subtitle}</p>}
      </div>
      {aside}
    </header>
  );
}

export function Pill({ tone = "muted", children }: { tone?: "muted" | "moss" | "sun" | "ember" | "sky" | "dusk"; children: ReactNode }) {
  const t = {
    muted: "bg-white/5 text-ink-soft",
    moss: "bg-moss-500/15 text-moss-300",
    sun: "bg-sun-400/15 text-sun-300",
    ember: "bg-ember-500/15 text-ember-400",
    sky: "bg-sky-400/15 text-sky-400",
    dusk: "bg-dusk-400/15 text-dusk-400",
  }[tone];
  return <span className={cx("inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[11.5px] font-medium", t)}>{children}</span>;
}

export function Toggle({ checked, onChange, label, hint }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <label className="flex items-center justify-between gap-4 py-2.5 cursor-pointer group">
      <span>
        <span className="block text-ink">{label}</span>
        {hint && <span className="block text-[12px] text-ink-muted mt-0.5">{hint}</span>}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={cx(
          "relative h-6 w-11 shrink-0 rounded-full p-0 transition-colors cursor-pointer",
          checked ? "bg-moss-500" : "bg-night-600",
        )}
      >
        <span
          className={cx(
            "absolute left-0 top-0.5 size-5 rounded-full bg-white shadow transition-transform",
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
      <span className="block text-[12.5px] font-medium text-ink-soft mb-1.5">{label}</span>
      {children}
      {hint && <span className="block text-[12px] text-ink-muted mt-1.5">{hint}</span>}
    </label>
  );
}

export const inputCls =
  "w-full h-10 rounded-xl bg-night-950/70 ring-1 ring-white/10 px-3 text-ink placeholder:text-ink-faint focus:ring-moss-500/60 outline-none transition";

export function ErrorBanner({ error, onClose, action }: { error: HumanError | null; onClose?: () => void; action?: ReactNode }) {
  if (!error) return null;
  return (
    <div className="flex items-start gap-3 rounded-2xl bg-ember-500/10 ring-1 ring-ember-500/25 p-4 animate-rise selectable">
      <AlertTriangle className="size-5 text-ember-400 shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <div className="font-semibold text-ink">{error.title}</div>
        {error.detail && <div className="text-[12.5px] text-ink-soft mt-0.5 break-words">{error.detail}</div>}
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
    info: ["bg-sky-400/10 ring-sky-400/20", <Info key="i" className="size-4 text-sky-400 shrink-0 mt-0.5" />],
    warn: ["bg-sun-400/10 ring-sun-400/25", <AlertTriangle key="w" className="size-4 text-sun-400 shrink-0 mt-0.5" />],
    ok: ["bg-moss-500/10 ring-moss-500/25", <Check key="o" className="size-4 text-moss-400 shrink-0 mt-0.5" />],
  }[tone];
  return (
    <div className={cx("flex gap-2.5 rounded-xl ring-1 p-3 text-[12.5px] text-ink-soft", t[0] as string)}>
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-night-950/70 backdrop-blur-sm animate-fade" onMouseDown={onClose}>
      <div
        className={cx("w-full rounded-3xl bg-night-850 ring-1 ring-white/10 shadow-2xl animate-rise", wide ? "max-w-2xl" : "max-w-md")}
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="px-6 pt-5 pb-1 font-display text-lg font-semibold">{title}</div>
        <div className="px-6 py-3 text-ink-soft">{children}</div>
        {footer && <div className="flex justify-end gap-2 px-6 pb-5 pt-2">{footer}</div>}
      </div>
    </div>
  );
}

export function Empty({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-14 px-6 animate-fade">
      <div className="size-14 rounded-2xl bg-night-700/80 ring-1 ring-white/5 flex items-center justify-center text-ink-muted mb-4">{icon}</div>
      <div className="font-display text-[17px] font-semibold">{title}</div>
      {children && <div className="text-ink-muted mt-1.5 max-w-sm">{children}</div>}
    </div>
  );
}

/** Barra de progresso real (bytes e arquivos), nunca um spinner infinito. */
export function Progress({ value, label, sub }: { value: number; label?: string; sub?: string }) {
  const v = Math.max(0, Math.min(100, value));
  return (
    <div className="w-full">
      {(label || sub) && (
        <div className="flex justify-between text-[12px] mb-1.5">
          <span className="text-ink-soft font-medium">{label}</span>
          <span className="text-ink-muted tabular-nums">{sub}</span>
        </div>
      )}
      <div className="h-2 rounded-full bg-night-950/80 ring-1 ring-white/5 overflow-hidden">
        <div
          className="h-full rounded-full bg-gradient-to-r from-moss-600 via-moss-500 to-moss-300 progress-stripes animate-shimmer transition-[width] duration-500"
          style={{ width: `${v}%` }}
        />
      </div>
    </div>
  );
}
