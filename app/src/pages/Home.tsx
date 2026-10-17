import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  ArrowRightLeft,
  Clock,
  Cloud,
  CloudOff,
  Copy,
  Crown,
  Gamepad2,
  BellRing,
  LifeBuoy,
  Loader2,
  Play,
  Power,
  Radio,
  RefreshCw,
  Save,
  UserPlus,
  UserRound,
  Users,
  Zap,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Avatar } from "../components/Avatar";
import type { Page } from "../components/Chrome";
import { Button, Card, CardTitle, Dot, Empty, ErrorBanner, InfoNote, Modal, Pill, Progress, Stat, cx } from "../components/ui";
import { api } from "../lib/api";
import { ago, bytes, clock, duration, heldLease, humanState, phaseLabel, syncLabel, syncPct } from "../lib/format";
import { useAction, useTick, type Toast } from "../lib/hooks";
import type { MemberView, Status } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

interface MainAction {
  label: string;
  sub?: string;
  icon: ReactNode;
  /** play: a luz da janela. alert: pêssego. wait: vidro escuro, sem clique. */
  kind: "play" | "alert" | "wait";
  onClick?: () => void;
}

/** A cena entra uma vez por abertura do app, não a cada volta ao Início. */
let sceneShown = false;

/** Depois do primeiro acesso que deu certo, o "Como entrar" dá lugar à última sessão. */
const ENTROU_KEY = "machencraft.ja-entrou";
function jaEntrou(): boolean {
  try {
    return localStorage.getItem(ENTROU_KEY) === "1";
  } catch {
    return false;
  }
}

export default function HomePage({ status, onPage, toast }: { status: Status | null; onPage: (p: Page) => void; toast: ToastFn }) {
  const isOpening = !!status && ((status.phase !== "idle" && status.phase !== "hosting") || !!status.failover);
  useTick(isOpening ? 1000 : 10000);
  const [openedAt, setOpenedAt] = useState<number | null>(null);
  useEffect(() => {
    setOpenedAt((t) => (isOpening ? (t ?? Date.now()) : null));
  }, [isOpening]);
  const openingFor = openedAt === null ? null : Math.max(0, Math.round((Date.now() - openedAt) / 1000));
  const [entrou, setEntrou] = useState(jaEntrou);
  useEffect(() => {
    if (status?.playing && !entrou) {
      try {
        localStorage.setItem(ENTROU_KEY, "1");
      } catch {
        // sem armazenamento: o "Como entrar" continua aparecendo
      }
      setEntrou(true);
    }
  }, [status?.playing, entrou]);
  const { busy, error, setError, run } = useAction();
  const [confirmStop, setConfirmStop] = useState(false);
  const [enter] = useState(() => !sceneShown);
  useEffect(() => {
    sceneShown = true;
  }, []);

  if (!status) {
    return (
      <div className="space-y-6">
        <Scene enter={enter} mood="night">
          <div className="h-full flex items-end p-8">
            <Loader2 className="size-5 animate-spin text-ink-soft" />
          </div>
        </Scene>
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
  const friendsOn = status.me_hosting ? status.online.filter((n) => n !== status.player_name) : [];

  const copyAddress = async () => {
    await writeText(status.address);
    toast({ level: "success", title: "Endereço copiado", body: status.address });
  };

  // ---- o botão principal, conforme o contexto
  let main: MainAction;
  if (status.recovery_pending && status.phase === "idle") {
    main = { label: "Recuperar sessão", sub: "Enviar o que ficou neste PC", icon: <LifeBuoy className="size-6" />, kind: "alert", onClick: () => run("recover", api.recover) };
  } else if (status.failover) {
    main = { label: "Trocando de host", sub: "Fique no jogo: você volta sozinho", icon: <RefreshCw className="size-6 animate-spin" />, kind: "wait" };
  } else if (working) {
    main = { label: phaseLabel[status.phase], sub: "Já já fica pronto", icon: <Loader2 className="size-6 animate-spin" />, kind: "wait" };
  } else if ((status.me_hosting || held?.purpose === "hosting") && status.client_starting) {
    // do clique até o jogo abrir: um Minecraft só, sem segundo clique
    main = {
      label: status.client_prep ? "Preparando o Minecraft" : "Abrindo o Minecraft",
      sub: status.client_prep ? "Conferindo os arquivos do jogo" : "A janela do jogo já vai aparecer",
      icon: <Loader2 className="size-6 animate-spin" />,
      kind: "wait",
    };
  } else if ((status.me_hosting || held?.purpose === "hosting") && (status.playing || status.client_open)) {
    main = {
      label: status.playing ? "Jogando" : "Entrando no mundo",
      sub: status.playing ? "Seu Minecraft está dentro do mundo" : "O Minecraft está aberto, entrando no servidor",
      icon: status.playing ? <Gamepad2 className="size-6" /> : <Loader2 className="size-6 animate-spin" />,
      kind: "wait",
    };
  } else if (status.me_hosting) {
    main = { label: "Abrir o Minecraft", sub: "O mundo está aberto no seu PC", icon: <Gamepad2 className="size-6" />, kind: "play", onClick: () => run("open", api.openGame) };
  } else if (held && held.purpose === "maintenance") {
    main = { label: "Aguarde um pouco", sub: `${held.player} está ajustando o mundo`, icon: <Clock className="size-6" />, kind: "wait" };
  } else if (held && held.handing_over) {
    main = { label: "Trocando de host", sub: `${held.player} está passando o mundo`, icon: <ArrowRightLeft className="size-6" />, kind: "wait" };
  } else if (held) {
    main = {
      label: "Entrar",
      sub: online.length > 0 ? `${online.length} no jogo agora` : "O mundo já está aberto",
      icon: <Play className="size-6 fill-current" />,
      kind: "play",
      onClick: () => run("play", () => api.play(true)),
    };
  } else {
    main = {
      label: "Jogar",
      sub: replicaOk ? "Você abre o mundo, em segundos" : "Você abre o mundo",
      icon: <Play className="size-6 fill-current" />,
      kind: "play",
      onClick: () => run("play", () => api.play(true)),
    };
  }
  // abrindo / trocando: contador de segundos no botão
  if (main.kind === "wait" && (working || status.failover) && openingFor !== null) {
    main = { ...main, label: `${status.failover ? "Trocando" : "Abrindo"}... ${openingFor} s` };
  }

  const sync = status.sync;
  const pct = syncPct(status) ?? 0;
  const prep = status.client_prep;
  const prepPct = prep ? (prep.bytes_total > 0 ? Math.min(100, (prep.bytes_done / prep.bytes_total) * 100) : 0) : 0;
  const strip = working || !!prep;
  // a cena conta o estado: noite (dormindo), amanhecer (abrindo/trocando), dia (aberto)
  const opening = working || !!status.failover || !!held?.handing_over;
  const mood: "night" | "dawn" | "day" = opening ? "dawn" : status.me_hosting || held?.purpose === "hosting" ? "day" : "night";
  const stateWord = opening ? "Abrindo" : st.tone === "live" ? "Aberto" : st.tone === "busy" ? "Ocupado" : st.tone === "warn" ? "Atenção" : "Dormindo";
  const heroLine = status.me_hosting
    ? `Você está hospedando${online.length > 0 ? `, ${online.length} no jogo` : ""}`
    : opening
      ? status.failover ?? "Abrindo o mundo..."
      : held?.purpose === "hosting"
        ? `${held.player} está jogando`
        : held
          ? st.title
          : "O mundo está dormindo. Ninguém jogando agora.";
  const lastWho = status.head ? (status.head.host_player === status.player_name ? "Você" : status.head.host_player) : null;

  return (
    <div className="space-y-6">
      {/* ---------------------------------------------------------------- a cena */}
      <Scene enter={enter} mood={mood}>
        <div className={cx("scene-copy relative h-full flex flex-col p-8", strip && "pb-[104px]")}>
          <div className="flex items-center gap-2 flex-wrap">
            <span className="inline-flex items-center gap-2 h-7 px-3 rounded-full bg-night-950/55 backdrop-blur-sm ring-1 ring-inset ring-white/10 text-[12.5px] font-bold">
              <Dot tone={st.tone} />
              {stateWord}
            </span>
            {status.replicating && (
              <span className="inline-flex items-center gap-1.5 h-7 px-3 rounded-full bg-night-950/55 backdrop-blur-sm ring-1 ring-inset ring-white/10 text-[12.5px] text-sky-300 font-semibold">
                <RefreshCw className="size-3.5 animate-spin" /> Atualizando a cópia deste PC
              </span>
            )}
          </div>

          <h1 className="font-display text-[92px] leading-[0.82] mt-5 text-white [text-shadow:0_2px_0_rgb(6_12_27/0.45),0_10px_40px_rgb(6_12_27/0.6)] max-w-[12ch]">
            {status.world_name}
          </h1>
          <p className="text-[17px] font-medium text-ink mt-3 max-w-[44ch] [text-shadow:0_1px_12px_rgb(6_12_27/0.8)]">{heroLine}</p>
          <div className="flex items-center gap-x-4 gap-y-1 flex-wrap mt-3 text-[13px] text-ink-soft [text-shadow:0_1px_10px_rgb(6_12_27/0.9)]">
            <span className="flex items-center gap-1.5">
              <Save className="size-3.5" /> Salvo {ago(status.last_checkpoint_at ?? status.head?.created_at)}
            </span>
            {status.head && lastWho && (
              <span className="flex items-center gap-1.5">
                <UserRound className="size-3.5" /> {lastWho} jogou {ago(status.head.created_at)}
              </span>
            )}
          </div>

          <div className="mt-auto flex items-end gap-3 flex-wrap">
            <MainButton main={main} busy={!!busy} />
            {status.me_hosting && !working && (
              <div className="flex gap-2 pb-1">
                {friendsOn.length > 0 && (
                  <Button variant="sun" icon={<ArrowRightLeft className="size-4" />} onClick={() => setConfirmStop(true)}>
                    Passar o host
                  </Button>
                )}
                <Button className="bg-night-950/60 backdrop-blur-md" icon={<Power className="size-4" />} onClick={() => setConfirmStop(true)}>
                  Fechar o mundo
                </Button>
              </div>
            )}
          </div>
        </div>

        {strip && (
          <div className="absolute inset-x-0 bottom-0 px-8 py-5 bg-night-950/70 backdrop-blur-md border-t border-white/[0.08] animate-fade">
            {working ? (
              <Progress
                value={pct}
                label={sync ? syncLabel[sync.phase] : phaseLabel[status.phase]}
                sub={
                  sync
                    ? sync.bytes_total > 0
                      ? `${bytes(sync.bytes_done)} de ${bytes(sync.bytes_total)}${sync.files_total ? `, ${sync.files_done} de ${sync.files_total} arquivos` : ""}`
                      : sync.files_total
                        ? `${sync.files_done} de ${sync.files_total} arquivos`
                        : ""
                    : ""
                }
              />
            ) : (
              prep && (
                <Progress
                  tone="sky"
                  value={prepPct}
                  label="Preparando o Minecraft"
                  sub={`${prep.bytes_total > 0 ? `${bytes(prep.bytes_done)} de ${bytes(prep.bytes_total)}` : "Conferindo arquivos"} (só na primeira vez, direto da Mojang)`}
                />
              )
            )}
          </div>
        )}
      </Scene>

      <ErrorBanner
        error={err}
        onClose={() => setError(null)}
        action={err?.code === "remote" || err?.code === "p2p" ? <Button onClick={() => onPage("health")}>Abrir Saúde do sistema</Button> : undefined}
      />

      {/* ---------------------------------------------------------------- sessão (host) */}
      {status.me_hosting && (
        <Card className="p-5">
          <div className="flex items-center gap-4">
            <span className="size-10 rounded-[10px] bg-moss-500/16 text-moss-300 flex items-center justify-center shrink-0">
              <Crown className="size-5" />
            </span>
            <div className="min-w-0">
              <div className="font-bold text-[15px]">Você é o host</div>
              <div className="text-[12.5px] text-ink-muted">O mundo roda neste PC enquanto você hospeda</div>
            </div>
            <div className="ml-auto flex gap-2">
              <Button icon={<Save className="size-4" />} busy={busy === "save"} onClick={() => run("save", api.saveNow).then(() => toast({ level: "success", title: "Mundo salvo", body: "" }))}>
                Salvar agora
              </Button>
            </div>
          </div>
          <div className="grid grid-cols-3 mt-5 pt-4 border-t border-white/[0.06]">
            <Stat label="Aberto há" value={status.session_started_at ? duration((Date.now() - new Date(status.session_started_at).getTime()) / 1000) : "—"} />
            <Stat label="Próximo salvamento às" value={clock(status.next_checkpoint_at)} className="pl-5 border-l border-white/[0.06]" />
            <Stat label="Seu Minecraft" value={status.host_client_running ? "Aberto" : "Fechado"} className="pl-5 border-l border-white/[0.06]" />
          </div>
          {status.friends_still_playing && (
            <div className="mt-4">
              <InfoNote tone="warn">
                <b className="text-ink">Seus amigos ainda estão jogando.</b> Pode fechar o seu jogo: o mundo continua para eles e fecha sozinho quando o último sair. Se precisar desligar
                o PC, use <b className="text-ink">Passar o host</b>: o mundo vai para um amigo sem derrubar ninguém.
              </InfoNote>
            </div>
          )}
        </Card>
      )}

      <div className="grid grid-cols-[1.5fr_1fr] gap-6">
        {/* ------------------------------------------------------------ grupo */}
        <Card>
          <CardTitle
            aside={
              status.is_owner ? (
                <Button variant="ghost" className="h-8 px-3" icon={<UserPlus className="size-4" />} onClick={() => onPage("world")}>
                  Convidar
                </Button>
              ) : (
                <span className="text-[12.5px] text-ink-muted">{online.length} no jogo</span>
              )
            }
          >
            O grupo
          </CardTitle>
          <div className="px-3 pb-3">
            {status.members.length === 0 ? (
              <Empty icon={<Users className="size-6" />} title="Carregando o grupo…">
                Assim que a nuvem do mundo responder, todo mundo aparece aqui.
              </Empty>
            ) : (
              <ul className="grid grid-cols-2 gap-0.5">
                {status.members
                  .filter((m) => m.approved)
                  .sort((a, b) => rank(b, online) - rank(a, online))
                  .map((m) => (
                    <MemberRow
                      key={m.id}
                      m={m}
                      inGame={online.includes(m.nick)}
                      host={m.nick === host}
                      busy={busy === `call-${m.id}`}
                      onCall={
                        !m.is_me && !online.includes(m.nick) && m.nick !== host
                          ? () =>
                              run(`call-${m.id}`, () => api.callMember(m.id)).then((r) => {
                                if (r !== undefined) toast({ level: "success", title: `Chamamos ${m.nick}`, body: m.online ? "O app dele avisa agora." : "O app dele avisa quando abrir." });
                              })
                          : undefined
                      }
                    />
                  ))}
              </ul>
            )}
          </div>
        </Card>

        {/* ------------------------------------------------------------ como entrar / última sessão */}
        <Card>
          <CardTitle>{entrou && status.head ? "Última sessão" : "Como entrar"}</CardTitle>
          <div className="px-5 pb-5 space-y-4">
            {entrou && status.head ? (
              <div className="space-y-1.5">
                <div className="text-[15px] font-bold text-ink">
                  {status.head.host_player === status.player_name ? "Você hospedou" : `${status.head.host_player} hospedou`}
                  {status.head.duration_secs > 0 && ` por ${duration(status.head.duration_secs)}`}
                </div>
                <div className="text-[13px] text-ink-soft">
                  {status.head.players.length > 1 ? `Com ${status.head.players.filter((p) => p !== status.head!.host_player).join(", ")}` : "Sozinho no mundo"}
                  {" · "}
                  {ago(status.head.created_at)}
                </div>
              </div>
            ) : (
            <ol className="space-y-2.5 text-ink-soft text-[13.5px]">
              <li className="flex items-center gap-3">
                <Step n={1} /> <span>Clique em <b className="text-ink">Jogar</b> e o Minecraft abre</span>
              </li>
              <li className="flex items-center gap-3">
                <Step n={2} /> <span>No Multiplayer, entre em <b className="text-ink">{status.world_name}</b></span>
              </li>
            </ol>
            )}
            <div className={cx("rounded-xl", entrou && status.head && "hidden")}><div className="rounded-xl bg-night-950/60 ring-1 ring-inset ring-white/[0.06] px-3.5 py-3">
              <div className="text-[12.5px] text-ink-muted mb-1">Endereço, sempre o mesmo</div>
              <div className="flex items-center justify-between gap-2">
                <code className="font-mono text-[14px] text-sun-300 truncate selectable">{status.address}</code>
                <button onClick={copyAddress} className="shrink-0 size-8 -mr-1.5 rounded-lg flex items-center justify-center text-ink-muted hover:text-ink hover:bg-white/[0.06] cursor-pointer" title="Copiar endereço" aria-label="Copiar endereço">
                  <Copy className="size-4" />
                </button>
              </div>
            </div></div>
            <div className="flex flex-wrap gap-1.5">
              {status.coord_ok ? (
                <Pill tone="muted">
                  <Cloud className="size-3.5" /> Nuvem ok
                </Pill>
              ) : (
                <Pill tone="ember">
                  <CloudOff className="size-3.5" /> Sem nuvem
                </Pill>
              )}
              {status.net.path_to_host && (
                <Pill tone={status.net.path_to_host === "direta" ? "moss" : "dusk"}>
                  {status.net.path_to_host === "direta" ? <Zap className="size-3.5" /> : <Radio className="size-3.5" />}
                  Conexão {status.net.path_to_host}
                  {status.net.rtt_ms != null && `, ${status.net.rtt_ms} ms`}
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
        title={friendsOn.length > 0 ? "Passar o host?" : "Encerrar a sessão?"}
        onClose={() => setConfirmStop(false)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmStop(false)}>
              Continuar jogando
            </Button>
            <Button
              variant={friendsOn.length > 0 ? "sun" : "danger"}
              onClick={() => {
                setConfirmStop(false);
                run("stop", api.stopSession);
              }}
            >
              {friendsOn.length > 0 ? "Passar o host" : "Salvar e encerrar"}
            </Button>
          </>
        }
      >
        {friendsOn.length > 0
          ? `O mundo vai para o PC de ${friendsOn.length > 1 ? "um amigo que está jogando" : friendsOn[0]}. Ninguém precisa sair: o jogo de quem está dentro mostra "Entrando..." por alguns segundos e volta sozinho, de onde parou. Depois disso você pode desligar o PC.`
          : "Ninguém mais está online. O mundo será salvo e guardado."}
      </Modal>
    </div>
  );
}

/**
 * A cena da identidade, recortada no céu e na casa. A casa tem luz quando o mundo está
 * aberto, pisca enquanto ele se prepara e fica apagada quando o mundo está livre.
 */
/** Cena do topo: GIF da noite com o mundo fechado e do dia com ele aberto (troca com um esmaecer). */
function Scene({ enter, mood, children }: { enter: boolean; mood: "night" | "dawn" | "day"; children: ReactNode }) {
  return (
    <section
      className={cx(
        "scene relative h-[400px] rounded-[22px] overflow-hidden bg-[#0b1430] ring-1 ring-white/10 shadow-[var(--shadow-pop)]",
        enter && "scene-enter",
        `scene-${mood}`,
      )}
    >
      <div className="scene-art" aria-hidden>
        <img src="/cena-noite.gif" alt="" draggable={false} />
        <img src="/cena-dia.gif" alt="" draggable={false} className="scene-day-art" />
        <div className="scene-tint" />
      </div>
      <div className="absolute inset-0 pointer-events-none bg-[linear-gradient(100deg,rgb(6_12_27/0.62)_0%,rgb(6_12_27/0.32)_38%,transparent_62%)]" />
      <div className="absolute inset-0 pointer-events-none rounded-[22px] shadow-[inset_0_1px_0_rgb(255_255_255/0.12)]" />
      {children}
    </section>
  );
}

function MainButton({ main, busy }: { main: MainAction; busy: boolean }) {
  const disabled = main.kind === "wait" || !main.onClick || busy;
  return (
    <button
      onClick={main.onClick}
      disabled={disabled}
      className={cx(
        "group flex items-center gap-4 h-[68px] pl-3 pr-7 rounded-[14px] text-left cursor-pointer disabled:cursor-default",
        main.kind === "play" && "bevel bg-sun-400 text-sun-900 [--bevel-glow:rgb(255_210_120/0.7)]",
        main.kind === "alert" && "bevel bg-dusk-400 text-sun-900 [--bevel-glow:rgb(240_164_116/0.6)]",
        main.kind === "wait" && "bg-night-950/60 backdrop-blur-md text-ink ring-1 ring-inset ring-white/12",
        busy && main.kind !== "wait" && "brightness-95",
      )}
    >
      <span className={cx("size-11 rounded-[10px] flex items-center justify-center shrink-0", main.kind === "wait" ? "bg-white/[0.07]" : "bg-sun-900/12")}>
        {busy && main.kind !== "wait" ? <Loader2 className="size-6 animate-spin" /> : main.icon}
      </span>
      <span className="min-w-0">
        <span className="block text-[25px] font-extrabold tracking-tight leading-[1.05]">{busy && main.kind !== "wait" ? "Um instante" : main.label}</span>
        {main.sub && <span className={cx("block text-[12.5px] mt-1.5 font-medium", main.kind === "wait" ? "text-ink-soft" : "text-sun-900/75")}>{main.sub}</span>}
      </span>
    </button>
  );
}

function rank(m: MemberView, online: string[]) {
  return (m.hosting ? 8 : 0) + (online.includes(m.nick) ? 4 : 0) + (m.online ? 2 : 0) + (m.is_me ? 1 : 0);
}

function MemberRow({ m, inGame, host, onCall, busy }: { m: MemberView; inGame: boolean; host: boolean; onCall?: () => void; busy?: boolean }) {
  const label = host ? "Hospedando" : inGame ? "No jogo" : m.online ? "Com o app aberto" : m.last_seen ? `Visto ${ago(m.last_seen)}` : "Ainda não apareceu";
  return (
    <li className={cx("group flex items-center gap-2.5 rounded-xl px-2 py-1.5", (inGame || host) && "bg-moss-500/[0.08]")}>
      <div className="relative">
        <Avatar name={m.nick} size={30} />
        <span
          className={cx(
            "absolute -right-1 -bottom-1 size-3.5 rounded-[4px] ring-[3px] ring-night-850",
            host ? "bg-sun-400" : inGame ? "bg-moss-400" : m.online ? "bg-sky-400" : "bg-night-600",
          )}
        />
      </div>
      <div className="min-w-0">
        <div className="font-bold truncate flex items-center gap-1.5">
          {m.nick}
          {m.role === "owner" && <Crown className="size-3.5 text-sun-400 shrink-0" aria-label="Dono" />}
          {m.is_me && <span className="text-[12px] text-ink-faint font-medium">você</span>}
        </div>
        <div className={cx("text-[12.5px] truncate", host ? "text-sun-300" : inGame ? "text-moss-300" : "text-ink-muted")}>{label}</div>
      </div>
      {onCall && (
        <button
          onClick={onCall}
          disabled={busy}
          title={`Chamar ${m.nick} pro jogo`}
          aria-label={`Chamar ${m.nick} pro jogo`}
          className="ml-auto shrink-0 h-7 px-2 rounded-lg text-[12px] font-semibold text-ink-muted hover:text-sun-300 hover:bg-white/[0.06] opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition cursor-pointer flex items-center gap-1"
        >
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : <BellRing className="size-3.5" />} Chamar
        </button>
      )}
    </li>
  );
}

function Step({ n }: { n: number }) {
  return <span className="size-6 shrink-0 rounded-[6px] bg-sun-400/14 text-sun-300 font-display text-[19px] flex items-center justify-center">{n}</span>;
}
