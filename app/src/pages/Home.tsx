import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  ArrowRightLeft,
  Clock,
  Copy,
  Gamepad2,
  HardDrive,
  LifeBuoy,
  Play,
  Power,
  Save,
  ShieldAlert,
  Sparkles,
  UserRound,
  Users,
} from "lucide-react";
import { useState } from "react";
import { Avatar } from "../components/Avatar";
import type { Page } from "../components/Chrome";
import { Button, Card, CardTitle, Empty, ErrorBanner, InfoNote, Modal, Pill, Progress, cx } from "../components/ui";
import { api } from "../lib/api";
import { ago, bytes, clock, duration, heldLock, humanState, phaseLabel, syncLabel } from "../lib/format";
import { useAction, useTick, type Toast } from "../lib/hooks";
import type { LockInfo, Status } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

export default function HomePage({ status, onPage, toast }: { status: Status | null; onPage: (p: Page) => void; toast: ToastFn }) {
  useTick(10000);
  const { busy, error, setError, run } = useAction();
  const [staleOpen, setStaleOpen] = useState<LockInfo | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);
  const [confirmPass, setConfirmPass] = useState(false);

  if (!status) {
    return (
      <div className="space-y-5">
        <div className="h-[260px] rounded-[var(--radius-card)] bg-night-850/60 animate-pulse" />
        <div className="grid grid-cols-2 gap-5">
          <div className="h-40 rounded-[var(--radius-card)] bg-night-850/60 animate-pulse" />
          <div className="h-40 rounded-[var(--radius-card)] bg-night-850/60 animate-pulse" />
        </div>
      </div>
    );
  }

  const st = humanState(status);
  const held = heldLock(status.lock);
  const stale = status.lock?.kind === "stale" ? status.lock.lock : null;
  const working = status.phase !== "idle" && status.phase !== "hosting";
  const err = error ?? status.last_error;
  const online = status.me_hosting ? status.online : held?.online ?? [];
  const host = status.me_hosting ? status.player_name : held?.player;

  const copyAddress = async () => {
    if (!status.address) return;
    await writeText(status.address);
    toast({ level: "success", title: "Endereço copiado", body: status.address });
  };

  // ---- botão principal, conforme o contexto
  let main: { label: string; sub?: string; icon: React.ReactNode; variant: "primary" | "sun" | "secondary"; onClick?: () => void; disabled?: boolean };
  if (status.recovery_pending && status.phase === "idle") {
    main = { label: "Recuperar sessão", sub: "Enviar o que ficou neste PC", icon: <LifeBuoy className="size-6" />, variant: "sun", onClick: () => run("recover", api.recover) };
  } else if (working) {
    main = { label: phaseLabel[status.phase], sub: "Aguarde…", icon: <Sparkles className="size-6" />, variant: "secondary", disabled: true };
  } else if (status.me_hosting) {
    main = { label: "Abrir o Minecraft", sub: "O mundo já está aberto", icon: <Gamepad2 className="size-6" />, variant: "primary", onClick: () => run("open", api.openLauncher) };
  } else if (held && held.purpose === "maintenance") {
    main = { label: "Aguardando manutenção", sub: `${held.player} está ajustando o mundo`, icon: <Clock className="size-6" />, variant: "secondary", disabled: true };
  } else if (held && held.handing_over) {
    main = { label: "Assumir agora", sub: `${held.player} está passando a vez`, icon: <ArrowRightLeft className="size-6" />, variant: "sun", onClick: () => run("take", () => api.takeTurn(true)) };
  } else if (held) {
    main = {
      label: `Entrar no mundo do ${held.player}`,
      sub: "Multiplayer → pAuletOr 3",
      icon: <Gamepad2 className="size-6" />,
      variant: "primary",
      onClick: () =>
        run("open", async () => {
          await api.openLauncher().catch(async () => {
            await copyAddress();
          });
        }),
    };
  } else if (stale) {
    main = { label: "Assumir o mundo", sub: `A sessão de ${stale.player} caiu`, icon: <ShieldAlert className="size-6" />, variant: "sun", onClick: () => setStaleOpen(stale) };
  } else {
    main = { label: "Jogar", sub: "Abrir o mundo neste PC", icon: <Play className="size-6 fill-current" />, variant: "primary", onClick: () => run("play", () => api.play(true)) };
  }

  const sync = status.sync;
  const pct = sync ? (sync.bytes_total > 0 ? (sync.bytes_done / sync.bytes_total) * 100 : sync.files_total > 0 ? (sync.files_done / sync.files_total) * 100 : 0) : 0;

  return (
    <div className="space-y-5">
      {/* ---------------------------------------------------------------- herói */}
      <Card className="hills overflow-hidden relative animate-rise">
        <div className="absolute -right-10 -top-16 size-64 rounded-full bg-sun-400/20 blur-3xl pointer-events-none" />
        <svg className="absolute inset-x-0 bottom-0 w-full h-[55%] pointer-events-none" viewBox="0 0 1000 200" preserveAspectRatio="none" aria-hidden>
          <path d="M0 120 C 150 60, 300 70, 450 110 S 750 60, 1000 95 L1000 200 L0 200 Z" fill="rgb(44 120 104 / 0.22)" />
          <path d="M0 150 C 180 100, 320 120, 520 145 S 820 105, 1000 135 L1000 200 L0 200 Z" fill="rgb(52 160 112 / 0.22)" />
          <path d="M0 182 C 200 150, 380 165, 600 178 S 860 155, 1000 172 L1000 200 L0 200 Z" fill="rgb(98 204 128 / 0.22)" />
        </svg>
        <div className="relative p-7 flex items-end justify-between gap-8">
          <div className="min-w-0">
            <div className="flex items-center gap-2 mb-3">
              <Pill tone={st.tone === "live" ? "moss" : st.tone === "warn" ? "ember" : st.tone === "busy" ? "sun" : "muted"}>
                <span
                  className={cx(
                    "size-1.5 rounded-full",
                    st.tone === "live" ? "bg-moss-400 animate-breathe" : st.tone === "busy" ? "bg-sun-400 animate-breathe" : st.tone === "warn" ? "bg-ember-400" : "bg-ink-faint",
                  )}
                />
                {st.tone === "live" ? "Aberto" : st.tone === "busy" ? "Ocupado" : st.tone === "warn" ? "Atenção" : "Livre"}
              </Pill>
              {!status.remote_ok && <Pill tone="ember">Sem conexão com a nuvem</Pill>}
            </div>
            <h1 className="font-display text-[44px] leading-[1.02] font-bold tracking-tight">{status.world_name}</h1>
            <p className="text-[17px] text-ink-soft mt-2">{st.title}</p>
            <div className="flex flex-wrap gap-x-5 gap-y-1.5 mt-5 text-[12.5px] text-ink-muted">
              <span className="flex items-center gap-1.5">
                <Save className="size-3.5" /> Último salvamento {ago(status.last_checkpoint_at ?? status.head?.created_at)}
              </span>
              {status.head && (
                <span className="flex items-center gap-1.5">
                  <UserRound className="size-3.5" /> Por último: {status.head.host_player}
                </span>
              )}
              {status.head && (
                <span className="flex items-center gap-1.5">
                  <HardDrive className="size-3.5" /> {bytes(status.head.total_size)}
                </span>
              )}
            </div>
          </div>
          <div className="shrink-0 w-[280px]">
            <button
              onClick={main.onClick}
              disabled={main.disabled || !!busy}
              className={cx(
                "group w-full rounded-2xl px-5 py-4 text-left transition-all duration-200 cursor-pointer active:scale-[0.985]",
                "disabled:cursor-default disabled:opacity-80",
                main.variant === "primary" && "bg-moss-500 text-night-950 hover:bg-moss-400 shadow-[var(--shadow-glow)] hover:-translate-y-0.5",
                main.variant === "sun" && "bg-sun-400 text-night-950 hover:bg-sun-300 shadow-[0_12px_40px_-10px_rgb(255_196_92/0.55)] hover:-translate-y-0.5",
                main.variant === "secondary" && "bg-night-700/90 text-ink ring-1 ring-white/10",
              )}
            >
              <div className="flex items-center gap-3.5">
                <span className={cx("size-11 rounded-xl flex items-center justify-center", main.variant === "secondary" ? "bg-white/5" : "bg-night-950/15")}>
                  {main.icon}
                </span>
                <span className="min-w-0">
                  <span className="block font-display text-[19px] font-bold leading-tight truncate">{busy ? "Um instante…" : main.label}</span>
                  {main.sub && <span className="block text-[12px] opacity-75 mt-0.5 truncate">{main.sub}</span>}
                </span>
              </div>
            </button>
          </div>
        </div>
        {working && (
          <div className="relative px-7 pb-6 animate-fade">
            <Progress
              value={pct}
              label={sync ? syncLabel[sync.phase] : phaseLabel[status.phase]}
              sub={
                sync
                  ? sync.bytes_total > 0
                    ? `${bytes(sync.bytes_done)} de ${bytes(sync.bytes_total)}${sync.files_total ? ` · ${sync.files_done}/${sync.files_total} arquivos` : ""}`
                    : sync.files_total
                      ? `${sync.files_done}/${sync.files_total} arquivos`
                      : ""
                  : ""
              }
            />
          </div>
        )}
      </Card>

      <ErrorBanner
        error={err}
        onClose={() => setError(null)}
        action={
          err?.code === "stale_lock" && err.lock ? (
            <Button variant="sun" onClick={() => setStaleOpen(err.lock)}>
              Entender e assumir
            </Button>
          ) : err?.code === "tailscale_down" ? (
            <Button onClick={() => onPage("health")}>Abrir Saúde do sistema</Button>
          ) : undefined
        }
      />

      {/* ---------------------------------------------------------------- sessão (host) */}
      {status.me_hosting && (
        <Card className="animate-rise">
          <CardTitle aside={<Pill tone="moss">Você é o host</Pill>}>Sua sessão</CardTitle>
          <div className="px-5 pb-5">
            <div className="grid grid-cols-3 gap-3 mb-4">
              <Metric label="Aberto há" value={status.session_started_at ? duration((Date.now() - new Date(status.session_started_at).getTime()) / 1000) : "—"} />
              <Metric label="Próximo salvamento" value={clock(status.next_checkpoint_at)} />
              <Metric label="Minecraft neste PC" value={status.host_client_running ? "Aberto" : "Fechado"} />
            </div>
            {status.friends_still_playing && (
              <div className="mb-4">
                <InfoNote tone="warn">
                  <b className="text-ink">Seus amigos ainda estão jogando.</b> Não desligue o PC: o mundo fecha e é salvo sozinho quando o último sair.
                </InfoNote>
              </div>
            )}
            <div className="flex flex-wrap gap-2">
              <Button icon={<Save className="size-4" />} busy={busy === "save"} onClick={() => run("save", api.saveNow).then(() => toast({ level: "success", title: "Mundo salvo na nuvem", body: "" }))}>
                Salvar agora
              </Button>
              <Button icon={<ArrowRightLeft className="size-4" />} onClick={() => setConfirmPass(true)}>
                Passar a vez
              </Button>
              <Button variant="danger" icon={<Power className="size-4" />} onClick={() => setConfirmStop(true)}>
                Encerrar sessão
              </Button>
            </div>
          </div>
        </Card>
      )}

      <div className="grid grid-cols-5 gap-5">
        {/* ------------------------------------------------------------ online */}
        <Card className="col-span-3 animate-rise">
          <CardTitle aside={<span className="text-[12px] text-ink-muted">{online.length} online</span>}>Jogadores online</CardTitle>
          <div className="px-5 pb-5">
            {online.length === 0 ? (
              <Empty icon={<Users className="size-6" />} title={host ? "Ninguém online ainda" : "O mundo está dormindo"}>
                {host ? "Quem entrar aparece aqui na hora." : "Quando alguém abrir o mundo, você vê quem está jogando aqui."}
              </Empty>
            ) : (
              <ul className="grid grid-cols-2 gap-2.5">
                {online.map((p) => (
                  <li key={p} className="flex items-center gap-3 rounded-xl bg-night-950/40 ring-1 ring-white/5 p-2.5 animate-rise">
                    <Avatar name={p} size={38} ring={p === host} />
                    <div className="min-w-0">
                      <div className="font-semibold truncate">{p}</div>
                      <div className="text-[11.5px] text-ink-muted">{p === host ? "Hospedando" : "Jogando"}</div>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </Card>

        {/* ------------------------------------------------------------ endereço */}
        <Card className="col-span-2 animate-rise">
          <CardTitle>Como entrar</CardTitle>
          <div className="px-5 pb-5 space-y-3">
            <ol className="space-y-2 text-ink-soft text-[13px]">
              <li className="flex gap-2.5">
                <Step n={1} /> Abra o Minecraft <b className="text-ink">1.21.11</b>
              </li>
              <li className="flex gap-2.5">
                <Step n={2} /> Multiplayer → <b className="text-ink">pAuletOr 3</b>
              </li>
            </ol>
            <div className="rounded-xl bg-night-950/60 ring-1 ring-white/5 p-3">
              <div className="text-[11px] uppercase tracking-wider text-ink-faint mb-1">Endereço</div>
              <div className="flex items-center justify-between gap-2">
                <code className="font-mono text-[13px] text-moss-300 truncate selectable">{status.address ?? "aparece quando alguém abrir o mundo"}</code>
                {status.address && (
                  <button onClick={copyAddress} className="shrink-0 text-ink-muted hover:text-ink cursor-pointer" title="Copiar endereço" aria-label="Copiar endereço">
                    <Copy className="size-4" />
                  </button>
                )}
              </div>
            </div>
          </div>
        </Card>
      </div>

      {/* ---------------------------------------------------------------- modais */}
      <Modal
        open={!!staleOpen}
        title={`A sessão de ${staleOpen?.player ?? ""} parece ter caído`}
        onClose={() => setStaleOpen(null)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setStaleOpen(null)}>
              Agora não
            </Button>
            <Button
              variant="sun"
              busy={busy === "takeover"}
              onClick={() => {
                const s = staleOpen;
                setStaleOpen(null);
                if (s) run("takeover", () => api.play(true, [s.session_id]));
              }}
            >
              Assumir o mundo
            </Button>
          </>
        }
      >
        <div className="space-y-3 text-[13.5px]">
          <p>
            O PC de <b className="text-ink">{staleOpen?.player}</b> parou de dar sinal {ago(staleOpen?.heartbeat_at)} e o servidor dele não responde. Isso costuma ser queda de energia ou de
            internet.
          </p>
          <p>
            Se você assumir, o mundo continua a partir do <b className="text-ink">último salvamento</b> que chegou à nuvem. Quando o PC dele voltar, o que tiver lá fica guardado na linha do tempo —
            nada é sobrescrito.
          </p>
        </div>
      </Modal>
      <Modal
        open={confirmStop}
        title="Encerrar a sessão?"
        onClose={() => setConfirmStop(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmStop(false)}>
              Continuar jogando
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                setConfirmStop(false);
                run("stop", api.stopSession);
              }}
            >
              Salvar e encerrar
            </Button>
          </>
        }
      >
        {online.length > 0 ? `${online.join(", ")} ${online.length > 1 ? "estão" : "está"} online e ${online.length > 1 ? "serão desconectados" : "será desconectado"}.` : "Ninguém está online."} O mundo será salvo e enviado
        para a nuvem.
      </Modal>
      <Modal
        open={confirmPass}
        title="Passar a vez?"
        onClose={() => setConfirmPass(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmPass(false)}>
              Cancelar
            </Button>
            <Button
              variant="sun"
              onClick={() => {
                setConfirmPass(false);
                run("pass", api.passTurn);
              }}
            >
              Passar a vez
            </Button>
          </>
        }
      >
        Todos recebem um aviso no chat com contagem regressiva de 30 segundos. Depois o mundo é salvo e qualquer amigo pode clicar em <b className="text-ink">Assumir agora</b> no app para continuar
        do PC dele, no mesmo endereço.
      </Modal>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl bg-night-950/50 ring-1 ring-white/5 px-3.5 py-3">
      <div className="text-[11px] uppercase tracking-wider text-ink-faint">{label}</div>
      <div className="font-display text-[18px] font-semibold mt-0.5 tabular-nums">{value}</div>
    </div>
  );
}

function Step({ n }: { n: number }) {
  return <span className="size-5 shrink-0 rounded-full bg-moss-500/15 text-moss-300 text-[11px] font-bold flex items-center justify-center">{n}</span>;
}
