import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  ArrowRightLeft,
  Clock,
  Cloud,
  CloudOff,
  Copy,
  Crown,
  Gamepad2,
  HardDrive,
  LifeBuoy,
  Play,
  Power,
  Radio,
  RefreshCw,
  Save,
  Sparkles,
  UserPlus,
  UserRound,
  Users,
  Zap,
} from "lucide-react";
import { useState } from "react";
import { Avatar } from "../components/Avatar";
import type { Page } from "../components/Chrome";
import { Button, Card, CardTitle, Empty, ErrorBanner, InfoNote, Modal, Pill, Progress, cx } from "../components/ui";
import { api } from "../lib/api";
import { ago, bytes, clock, duration, heldLease, humanState, phaseLabel, syncLabel, syncPct } from "../lib/format";
import { useAction, useTick, type Toast } from "../lib/hooks";
import type { MemberView, Status } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

interface MainAction {
  label: string;
  sub?: string;
  icon: React.ReactNode;
  variant: "primary" | "sun" | "secondary";
  onClick?: () => void;
  disabled?: boolean;
}

export default function HomePage({ status, onPage, toast }: { status: Status | null; onPage: (p: Page) => void; toast: ToastFn }) {
  useTick(10000);
  const { busy, error, setError, run } = useAction();
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
  const held = heldLease(status.lease);
  const working = status.phase !== "idle" && status.phase !== "hosting";
  const err = error ?? status.last_error;
  const online = status.me_hosting ? status.online : held?.online ?? [];
  const host = status.me_hosting ? status.player_name : held?.purpose === "hosting" ? held.player : undefined;
  const replicaOk = status.head_seq == null || status.replica_seq >= status.head_seq;

  const copyAddress = async () => {
    await writeText(status.address);
    toast({ level: "success", title: "Endereço copiado", body: status.address });
  };

  // ---- o botão principal, conforme o contexto
  let main: MainAction;
  if (status.recovery_pending && status.phase === "idle") {
    main = { label: "Recuperar sessão", sub: "Enviar o que ficou neste PC", icon: <LifeBuoy className="size-6" />, variant: "sun", onClick: () => run("recover", api.recover) };
  } else if (status.failover) {
    main = { label: "Trocando de host…", sub: "Não feche o jogo", icon: <RefreshCw className="size-6 animate-spin" />, variant: "secondary", disabled: true };
  } else if (working) {
    main = { label: phaseLabel[status.phase], sub: "Aguarde…", icon: <Sparkles className="size-6" />, variant: "secondary", disabled: true };
  } else if (status.me_hosting) {
    main = { label: "Abrir o Minecraft", sub: "O mundo está aberto no seu PC", icon: <Gamepad2 className="size-6" />, variant: "primary", onClick: () => run("open", api.openGame) };
  } else if (held && held.purpose === "maintenance") {
    main = { label: "Aguarde um pouco", sub: `${held.player} está ajustando o mundo`, icon: <Clock className="size-6" />, variant: "secondary", disabled: true };
  } else if (held && held.handing_over) {
    main = { label: "Assumir agora", sub: `${held.player} está passando a vez`, icon: <ArrowRightLeft className="size-6" />, variant: "sun", onClick: () => run("take", () => api.takeTurn(true)) };
  } else if (held) {
    main = { label: "Jogar", sub: `Entrar no mundo aberto por ${held.player}`, icon: <Play className="size-6 fill-current" />, variant: "primary", onClick: () => run("play", () => api.play(true)) };
  } else {
    main = {
      label: "Jogar",
      sub: replicaOk ? "O mundo acorda no seu PC em segundos" : "O mundo acorda no seu PC",
      icon: <Play className="size-6 fill-current" />,
      variant: "primary",
      onClick: () => run("play", () => api.play(true)),
    };
  }

  const sync = status.sync;
  const pct = syncPct(status) ?? 0;

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
            <div className="flex items-center gap-2 mb-3 flex-wrap">
              <Pill tone={st.tone === "live" ? "moss" : st.tone === "warn" ? "ember" : st.tone === "busy" ? "sun" : "muted"}>
                <span
                  className={cx(
                    "size-1.5 rounded-full",
                    st.tone === "live" ? "bg-moss-400 animate-breathe" : st.tone === "busy" ? "bg-sun-400 animate-breathe" : st.tone === "warn" ? "bg-ember-400" : "bg-ink-faint",
                  )}
                />
                {st.tone === "live" ? "Aberto" : st.tone === "busy" ? "Ocupado" : st.tone === "warn" ? "Atenção" : "Livre"}
              </Pill>
              {status.replicating && (
                <Pill tone="sky">
                  <RefreshCw className="size-3 animate-spin" /> Atualizando cópia
                </Pill>
              )}
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
          <div className="shrink-0 w-[290px]">
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
                <span className={cx("size-11 rounded-xl flex items-center justify-center", main.variant === "secondary" ? "bg-white/5" : "bg-night-950/15")}>{main.icon}</span>
                <span className="min-w-0">
                  <span className="block font-display text-[21px] font-bold leading-tight truncate">{busy ? "Um instante…" : main.label}</span>
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
        action={err?.code === "remote" || err?.code === "p2p" ? <Button onClick={() => onPage("health")}>Abrir Saúde do sistema</Button> : undefined}
      />

      {/* ---------------------------------------------------------------- sessão (host) */}
      {status.me_hosting && (
        <Card className="animate-rise">
          <CardTitle aside={<Pill tone="moss">Você é o host</Pill>}>Sua sessão</CardTitle>
          <div className="px-5 pb-5">
            <div className="grid grid-cols-3 gap-3 mb-4">
              <Metric label="Aberto há" value={status.session_started_at ? duration((Date.now() - new Date(status.session_started_at).getTime()) / 1000) : "—"} />
              <Metric label="Próximo salvamento" value={clock(status.next_checkpoint_at)} />
              <Metric label="Seu Minecraft" value={status.host_client_running ? "Aberto" : "Fechado"} />
            </div>
            {status.friends_still_playing && (
              <div className="mb-4">
                <InfoNote tone="warn">
                  <b className="text-ink">Seus amigos ainda estão jogando.</b> Pode fechar o seu jogo: o mundo continua para eles e fecha sozinho quando o último sair. Se precisar
                  desligar o PC, use <b className="text-ink">Passar a vez</b>.
                </InfoNote>
              </div>
            )}
            <div className="flex flex-wrap gap-2">
              <Button icon={<Save className="size-4" />} busy={busy === "save"} onClick={() => run("save", api.saveNow).then(() => toast({ level: "success", title: "Mundo salvo", body: "" }))}>
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
        {/* ------------------------------------------------------------ turma */}
        <Card className="col-span-3 animate-rise">
          <CardTitle
            aside={
              status.is_owner ? (
                <Button variant="ghost" className="h-7" icon={<UserPlus className="size-4" />} onClick={() => onPage("world")}>
                  Convidar
                </Button>
              ) : (
                <span className="text-[12px] text-ink-muted">{online.length} no jogo</span>
              )
            }
          >
            A turma
          </CardTitle>
          <div className="px-5 pb-5">
            {status.members.length === 0 ? (
              <Empty icon={<Users className="size-6" />} title="Carregando a turma…">
                Assim que a nuvem do mundo responder, todo mundo aparece aqui.
              </Empty>
            ) : (
              <ul className="grid grid-cols-2 gap-2.5">
                {status.members
                  .filter((m) => m.approved)
                  .sort((a, b) => rank(b, online) - rank(a, online))
                  .map((m) => (
                    <MemberRow key={m.id} m={m} inGame={online.includes(m.nick)} host={m.nick === host} />
                  ))}
              </ul>
            )}
          </div>
        </Card>

        {/* ------------------------------------------------------------ como entrar */}
        <Card className="col-span-2 animate-rise">
          <CardTitle>Como entrar</CardTitle>
          <div className="px-5 pb-5 space-y-3">
            <ol className="space-y-2 text-ink-soft text-[13px]">
              <li className="flex gap-2.5">
                <Step n={1} /> Clique em <b className="text-ink">Jogar</b> (o Minecraft abre)
              </li>
              <li className="flex gap-2.5">
                <Step n={2} /> Multiplayer → <b className="text-ink">pAuletOr 3</b>
              </li>
            </ol>
            <div className="rounded-xl bg-night-950/60 ring-1 ring-white/5 p-3">
              <div className="text-[11px] uppercase tracking-wider text-ink-faint mb-1">Endereço (sempre o mesmo)</div>
              <div className="flex items-center justify-between gap-2">
                <code className="font-mono text-[14px] text-moss-300 truncate selectable">{status.address}</code>
                <button onClick={copyAddress} className="shrink-0 text-ink-muted hover:text-ink cursor-pointer" title="Copiar endereço" aria-label="Copiar endereço">
                  <Copy className="size-4" />
                </button>
              </div>
            </div>
            <div className="flex flex-wrap gap-1.5 text-[11.5px]">
              {status.coord_ok ? (
                <Pill tone="muted">
                  <Cloud className="size-3" /> Nuvem ok
                </Pill>
              ) : (
                <Pill tone="ember">
                  <CloudOff className="size-3" /> Sem nuvem
                </Pill>
              )}
              {status.net.path_to_host && (
                <Pill tone={status.net.path_to_host === "direta" ? "moss" : "sun"}>
                  {status.net.path_to_host === "direta" ? <Zap className="size-3" /> : <Radio className="size-3" />}
                  Conexão {status.net.path_to_host}
                  {status.net.rtt_ms != null && ` · ${status.net.rtt_ms} ms`}
                </Pill>
              )}
              <Pill tone={replicaOk ? "muted" : "sky"}>{replicaOk ? "Cópia local em dia" : "Cópia local atualizando"}</Pill>
            </div>
          </div>
        </Card>
      </div>

      {/* ---------------------------------------------------------------- modais */}
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
        {online.length > 0
          ? `${online.join(", ")} ${online.length > 1 ? "estão" : "está"} online e ${online.length > 1 ? "serão desconectados" : "será desconectado"}. Se preferir que continuem, use Passar a vez.`
          : "Ninguém está online."}{" "}
        O mundo será salvo e guardado.
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
        Todos recebem um aviso no chat com 30 segundos de contagem. O mundo é salvo e um amigo que estiver jogando assume sozinho — é só entrar de novo, no mesmo endereço.
      </Modal>
    </div>
  );
}

function rank(m: MemberView, online: string[]) {
  return (m.hosting ? 8 : 0) + (online.includes(m.nick) ? 4 : 0) + (m.online ? 2 : 0) + (m.is_me ? 1 : 0);
}

function MemberRow({ m, inGame, host }: { m: MemberView; inGame: boolean; host: boolean }) {
  const label = host ? "Hospedando" : inGame ? "No jogo" : m.online ? "Com o app aberto" : m.last_seen ? `Visto ${ago(m.last_seen)}` : "Ainda não apareceu";
  return (
    <li className={cx("flex items-center gap-3 rounded-xl ring-1 p-2.5 animate-rise", inGame || host ? "bg-moss-500/[0.06] ring-moss-400/15" : "bg-night-950/40 ring-white/5")}>
      <div className="relative">
        <Avatar name={m.nick} size={38} ring={host} />
        <span
          className={cx(
            "absolute -right-0.5 -bottom-0.5 size-3 rounded-full ring-2 ring-night-850",
            inGame || host ? "bg-moss-400" : m.online ? "bg-sky-400" : "bg-ink-faint/60",
          )}
        />
      </div>
      <div className="min-w-0">
        <div className="font-semibold truncate flex items-center gap-1.5">
          {m.nick}
          {m.role === "owner" && <Crown className="size-3.5 text-sun-400" aria-label="Dono" />}
          {m.is_me && <span className="text-[11px] text-ink-faint font-normal">(você)</span>}
        </div>
        <div className="text-[11.5px] text-ink-muted truncate">{label}</div>
      </div>
    </li>
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
