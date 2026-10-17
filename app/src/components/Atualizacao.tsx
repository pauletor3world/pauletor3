import { ArrowUpCircle, Bell, CheckCircle2, Download, Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Atualizacao } from "../lib/atualizacao";
import { temAtualizacao } from "../lib/atualizacao";
import { ago, bytes } from "../lib/format";
import type { Notice } from "../lib/types";
import { Button, Card, InfoNote, Progress, cx } from "./ui";

export interface UpdateProps {
  a: Atualizacao;
  baixado: number;
  total: number | null;
  atualizar: (modo: "agora" | "ao_fechar") => Promise<unknown>;
  cancelar: () => Promise<unknown>;
}

/** Notas da versão (do CHANGELOG): linhas com "- " viram lista; o resto, parágrafo. */
function Notas({ texto }: { texto: string }) {
  const blocos: { li: boolean; t: string }[] = [];
  for (const raw of texto.split(/\r?\n/)) {
    const l = raw.replace(/\*\*/g, "");
    if (!l.trim()) continue;
    if (/^\s*[-*]\s+/.test(l)) blocos.push({ li: true, t: l.replace(/^\s*[-*]\s+/, "") });
    else if (blocos.length && /^\s{2,}/.test(l)) blocos[blocos.length - 1].t += " " + l.trim();
    else blocos.push({ li: false, t: l.trim() });
  }
  return (
    <div className="text-[13px] text-ink-soft leading-relaxed space-y-1 selectable">
      {blocos.map((b, i) =>
        b.li ? (
          <div key={i} className="flex gap-2">
            <span className="text-sun-400 shrink-0">•</span>
            <span>{b.t}</span>
          </div>
        ) : (
          <p key={i}>{b.t}</p>
        ),
      )}
    </div>
  );
}

/** Cartão "Versão X disponível": notas, progresso do download e os botões de atualizar. */
export function UpdateCard({ a, baixado, total, atualizar, cancelar }: UpdateProps) {
  const [busy, setBusy] = useState(false);
  if (!a.oferta) return null;
  const o = a.oferta;
  const pct = total ? Math.min(100, (baixado / total) * 100) : 0;
  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } finally {
      setBusy(false);
    }
  };
  const esperando = a.estado === "aguardando_momento_seguro";
  const instalando = a.estado === "instalando";
  const aoFechar = a.pedido === "ao_fechar";
  const podePedir = !esperando && !instalando && !a.passando_host;

  return (
    <div className="space-y-3.5">
      <div className="flex items-start gap-3">
        <ArrowUpCircle className={cx("size-5 shrink-0 mt-0.5", o.obrigatoria || a.obrigatoria ? "text-dusk-400" : "text-sun-400")} />
        <div className="min-w-0">
          <div className="font-bold text-ink text-[15px]">{a.obrigatoria ? `Atualize para a versão ${o.versao}` : `Versão ${o.versao} disponível`}</div>
          <div className="text-[12.5px] text-ink-muted">
            {a.obrigatoria ? "O grupo combinou esta versão como a mínima para jogar." : `Você está na ${a.versao_atual}.`}
          </div>
        </div>
      </div>

      {o.notas && <Notas texto={o.notas} />}

      {a.estado === "baixando" && (
        <Progress
          value={pct}
          tone="sky"
          label={`Baixando ${total ? `${Math.round(pct)}%` : ""}`}
          sub={total ? `${bytes(baixado)} de ${bytes(total)}` : bytes(baixado)}
        />
      )}

      {a.erro && <InfoNote tone="warn">{a.erro}</InfoNote>}

      {esperando && a.passando_host && (
        <div className="flex items-center gap-2.5 text-[13px] text-ink-soft">
          <Loader2 className="size-4 animate-spin text-sun-400 shrink-0" />
          Passando o host para quem está jogando. O mundo fica salvo antes de instalar.
        </div>
      )}
      {esperando && !a.passando_host && (
        <div className="flex items-center justify-between gap-3">
          <span className="flex items-center gap-2.5 text-[13px] text-ink-soft">
            <Loader2 className="size-4 animate-spin text-sky-400 shrink-0" />
            Esperando o mundo terminar de sincronizar para instalar.
          </span>
          <Button variant="ghost" className="h-8" onClick={() => run(cancelar)} busy={busy}>
            Cancelar
          </Button>
        </div>
      )}
      {instalando && (
        <div className="flex items-center gap-2.5 text-[13px] text-ink-soft">
          <Loader2 className="size-4 animate-spin text-sun-400 shrink-0" />
          Instalando. O app fecha e reabre sozinho na versão nova.
        </div>
      )}

      {podePedir && (
        <div className="space-y-2">
          {a.sou_host && (
            <div className="text-[12.5px] text-ink-muted">Os amigos vão para a sala de espera por alguns segundos enquanto o host passa.</div>
          )}
          <div className="flex flex-wrap gap-2">
            <Button variant={a.sou_host ? "sun" : "primary"} icon={<Download className="size-4" />} busy={busy} onClick={() => run(() => atualizar("agora"))}>
              {a.sou_host ? "Passar o host e atualizar" : "Atualizar agora"}
            </Button>
            {!a.obrigatoria &&
              (aoFechar ? (
                <Button variant="ghost" onClick={() => run(cancelar)}>
                  Desfazer "ao fechar"
                </Button>
              ) : (
                <Button variant="secondary" onClick={() => run(() => atualizar("ao_fechar"))}>
                  Atualizar ao fechar
                </Button>
              ))}
          </div>
          {aoFechar && <div className="text-[12.5px] text-moss-300">Combinado: a atualização é instalada quando você fechar o app.</div>}
        </div>
      )}
    </div>
  );
}

/** Abaixo da versão mínima: no lugar das telas, só o pedido de atualizar. */
export function UpdateGate(p: UpdateProps) {
  return (
    <div className="min-h-[70vh] flex items-center justify-center animate-fade">
      <Card className="max-w-[520px] w-full p-7">
        <h1 className="text-[30px] font-extrabold tracking-tight leading-[1.1] text-ink mb-2">Atualize para continuar jogando com o grupo</h1>
        <p className="text-ink-muted text-[14px] mb-5">
          {p.a.versao_minima
            ? `O grupo está usando a versão ${p.a.versao_minima} ou mais nova. Você está na ${p.a.versao_atual}.`
            : "Esta versão do app ficou para trás."}
        </p>
        {p.a.oferta ? (
          <UpdateCard {...p} />
        ) : (
          <div className="flex items-center gap-2.5 text-[13px] text-ink-soft">
            <Loader2 className="size-4 animate-spin text-sky-400" />
            Procurando a versão nova…
          </div>
        )}
      </Card>
    </div>
  );
}

export interface NoticeItem extends Notice {
  at: string;
}

/** Sino na barra superior: selo quando há atualização ou aviso novo, e o histórico. */
export function NotificationBell({
  update,
  notices,
  open,
  onOpen,
}: {
  update: UpdateProps | null;
  notices: NoticeItem[];
  open: boolean;
  onOpen: (v: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [visto, setVisto] = useState(0);
  const novos = notices.length - visto;
  const comUpdate = !!update && temAtualizacao(update.a);
  useEffect(() => {
    if (!open) return;
    setVisto(notices.length);
    const fora = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && onOpen(false);
    window.addEventListener("mousedown", fora);
    return () => window.removeEventListener("mousedown", fora);
  }, [open, notices.length, onOpen]);

  return (
    <div ref={ref} className="relative">
      <button
        aria-label={comUpdate ? "Notificações: atualização disponível" : "Notificações"}
        title="Notificações"
        onClick={() => onOpen(!open)}
        className={cx(
          "relative h-10 w-11 flex items-center justify-center transition-colors cursor-pointer",
          open ? "text-ink bg-white/[0.06]" : "text-ink-muted hover:text-ink hover:bg-white/[0.06]",
        )}
      >
        <Bell className="size-[17px]" />
        {(comUpdate || novos > 0) && (
          <span className={cx("absolute top-2 right-2.5 size-2 rounded-[2px]", comUpdate ? "bg-sun-400" : "bg-sky-400")} aria-hidden />
        )}
      </button>
      {open && (
        <div className="absolute right-0 top-11 z-40 w-[380px] max-h-[70vh] scroll rounded-2xl bg-night-800 ring-1 ring-white/10 shadow-[var(--shadow-pop)] animate-pop">
          {comUpdate && update && (
            <div className="p-4 border-b border-white/[0.06]">
              <UpdateCard {...update} />
            </div>
          )}
          <div className="px-4 pt-3 pb-1 text-[12px] font-semibold text-ink-muted">Avisos recentes</div>
          {notices.length === 0 ? (
            <div className="px-4 pb-4 text-[13px] text-ink-faint">Nada por aqui ainda.</div>
          ) : (
            <ul className="pb-2">
              {[...notices].reverse().map((n, i) => (
                <li key={i} className="px-4 py-2 flex gap-2.5">
                  <span
                    className={cx(
                      "mt-1.5 size-1.5 rounded-[1px] shrink-0",
                      n.level === "error" ? "bg-ember-400" : n.level === "warning" ? "bg-dusk-400" : n.level === "success" ? "bg-moss-400" : "bg-sky-400",
                    )}
                  />
                  <span className="min-w-0">
                    <span className="block text-[13px] font-semibold text-ink">{n.title}</span>
                    {n.body && <span className="block text-[12.5px] text-ink-soft">{n.body}</span>}
                    <span className="block text-[11.5px] text-ink-faint mt-0.5">{ago(n.at)}</span>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

/** Aviso único depois de reabrir na versão nova. */
export function UpdatedBanner({ versao, onClose }: { versao: string; onClose: () => void }) {
  return (
    <div className="flex items-center gap-2.5 rounded-xl bg-moss-500/[0.12] ring-1 ring-inset ring-moss-500/25 px-4 py-2.5 mb-4 animate-pop">
      <CheckCircle2 className="size-4 text-moss-400 shrink-0" />
      <span className="text-[13.5px] text-ink flex-1">
        Atualizado para a versão <b>{versao}</b> ✓
      </span>
      <button onClick={onClose} className="text-[12.5px] text-ink-muted hover:text-ink cursor-pointer">
        Ok
      </button>
    </div>
  );
}
