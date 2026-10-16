import { listen } from "@tauri-apps/api/event";
import { enable as autostartOn } from "@tauri-apps/plugin-autostart";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Copy,
  Crown,
  FolderOpen,
  Globe,
  MessageCircle,
  Mountain,
  PartyPopper,
  ShieldCheck,
  Sparkles,
  Ticket,
  UserRound,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Button, ErrorBanner, Field, InfoNote, Progress, cx, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { bytes, syncLabel } from "../lib/format";
import { useAction } from "../lib/hooks";
import type { JoinResult, LauncherCandidate, LevelInfo, SyncProgress } from "../lib/types";

type Role = "owner" | "friend";
type StepId = "welcome" | "pc" | "world" | "join" | "invite_out" | "done";

const STEPS: Record<Role, StepId[]> = {
  owner: ["welcome", "pc", "world", "invite_out", "done"],
  friend: ["welcome", "pc", "join", "done"],
};

const META: Record<StepId, { title: string; icon: ReactNode }> = {
  welcome: { title: "Boas-vindas", icon: <Mountain className="size-4" /> },
  pc: { title: "Este PC", icon: <ShieldCheck className="size-4" /> },
  world: { title: "O mundo", icon: <Globe className="size-4" /> },
  join: { title: "Entrar", icon: <Ticket className="size-4" /> },
  invite_out: { title: "Convidar", icon: <Ticket className="size-4" /> },
  done: { title: "Pronto", icon: <PartyPopper className="size-4" /> },
};

const looksLikeInvite = (s: string) => /^(P3J1\.|pauletor3:\/\/entrar\/|https?:\/\/\S+\/c\/[a-z0-9]+\.[a-z0-9]+#)/.test(s.trim());

export default function Wizard({ onDone }: { onDone: () => void }) {
  const [role, setRole] = useState<Role>("friend");
  const [invite, setInvite] = useState("");
  const [i, setI] = useState(0);
  const steps = STEPS[role];
  const step = steps[i];
  const next = () => setI((x) => Math.min(x + 1, steps.length - 1));
  const back = () => setI((x) => Math.max(x - 1, 0));

  // Convite que abriu o app (link) ou que está na área de transferência.
  useEffect(() => {
    api
      .suggestDir()
      .then((s) => {
        if (s.pending_invite) {
          setInvite(s.pending_invite);
          setRole("friend");
        } else {
          readText()
            .then((t) => {
              if (t && looksLikeInvite(t)) setInvite(t.trim());
            })
            .catch(() => {});
        }
      })
      .catch(() => {});
    const un = listen<string>("p3://invite", (e) => {
      setInvite(e.payload);
      setRole("friend");
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return (
    <div className="flex-1 flex min-h-0">
      <aside className="w-[240px] shrink-0 p-6 border-r border-white/[0.05] bg-night-950/30">
        <img src="/logo.png" alt="" className="size-12 rounded-2xl mb-4 shadow-lg" />
        <div className="font-display text-xl font-bold">pAuletOr 3</div>
        <div className="text-ink-muted text-[12.5px] mb-7">{role === "owner" ? "Criar o mundo" : "Entrar no mundo"}</div>
        <ol className="space-y-1">
          {steps.map((s, k) => (
            <li
              key={s}
              className={cx("flex items-center gap-2.5 h-9 px-3 rounded-xl text-[13px]", k === i ? "bg-white/[0.07] text-ink font-semibold" : k < i ? "text-moss-300" : "text-ink-faint")}
            >
              <span className="size-5 flex items-center justify-center">{k < i ? <Check className="size-4" /> : META[s].icon}</span>
              {META[s].title}
            </li>
          ))}
        </ol>
        <div className="mt-10 text-[11px] text-ink-faint leading-relaxed">Não afiliado à Mojang nem à Microsoft.</div>
      </aside>
      <main className="flex-1 scroll">
        <div className="max-w-[640px] mx-auto px-10 py-10" key={step}>
          {step === "welcome" && <Welcome role={role} setRole={setRole} next={next} hasInvite={!!invite} />}
          {step === "pc" && <PcStep next={next} back={back} />}
          {step === "world" && <WorldStep next={next} back={back} />}
          {step === "join" && <JoinStep invite={invite} setInvite={setInvite} next={next} back={back} />}
          {step === "invite_out" && <InviteOut next={next} />}
          {step === "done" && <Done onDone={onDone} owner={role === "owner"} />}
        </div>
      </main>
    </div>
  );
}

function StepHeader({ kicker, title, children }: { kicker: string; title: string; children?: ReactNode }) {
  return (
    <div className="mb-7 animate-rise">
      <div className="text-[12px] uppercase tracking-[0.14em] text-moss-400 font-semibold mb-2">{kicker}</div>
      <h1 className="font-display text-[30px] leading-tight font-bold tracking-tight">{title}</h1>
      {children && <div className="text-ink-soft mt-2 text-[14px] leading-relaxed">{children}</div>}
    </div>
  );
}

function Nav({ back, next, nextLabel = "Continuar", disabled, busy }: { back?: () => void; next: () => void; nextLabel?: string; disabled?: boolean; busy?: boolean }) {
  return (
    <div className="flex justify-between mt-8 pt-5 border-t border-white/[0.05]">
      {back ? (
        <Button variant="ghost" icon={<ArrowLeft className="size-4" />} onClick={back}>
          Voltar
        </Button>
      ) : (
        <span />
      )}
      <Button variant="primary" onClick={next} disabled={disabled} busy={busy}>
        {nextLabel} <ArrowRight className="size-4" />
      </Button>
    </div>
  );
}

function useSyncProgress() {
  const [p, setP] = useState<SyncProgress | null>(null);
  useEffect(() => {
    const un = listen<SyncProgress>("p3://sync", (e) => setP(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return p;
}

function SyncBar({ p }: { p: SyncProgress | null }) {
  if (!p) return null;
  const pct = p.bytes_total > 0 ? (p.bytes_done / p.bytes_total) * 100 : p.files_total ? (p.files_done / p.files_total) * 100 : 0;
  return (
    <div className="mt-4 animate-fade">
      <Progress value={pct} label={syncLabel[p.phase]} sub={p.bytes_total ? `${bytes(p.bytes_done)} de ${bytes(p.bytes_total)}` : `${p.files_done}/${p.files_total}`} />
    </div>
  );
}

// ------------------------------------------------------------------ passos

function Welcome({ role, setRole, next, hasInvite }: { role: Role; setRole: (r: Role) => void; next: () => void; hasInvite: boolean }) {
  return (
    <div>
      <StepHeader kicker="Bem-vindo" title="Um mundo de survival que nunca dorme">
        O mundo fica nos PCs da turma e numa cópia protegida na nuvem. Quem clica em <b className="text-ink">Jogar</b> vira o servidor; se essa pessoa sair, outro amigo assume
        sozinho. Ninguém paga host nem mexe em rede.
      </StepHeader>
      {hasInvite && (
        <div className="mb-4 animate-rise">
          <InfoNote tone="ok">Encontrei um convite. É só continuar.</InfoNote>
        </div>
      )}
      <div className="grid grid-cols-2 gap-3 animate-rise">
        <RoleCard active={role === "friend"} onClick={() => setRole("friend")} icon={<Ticket className="size-5" />} title="Recebi um convite" text="Um amigo me mandou um link. Leva 2 minutos." />
        <RoleCard active={role === "owner"} onClick={() => setRole("owner")} icon={<Crown className="size-5" />} title="Vou criar o mundo" text="Eu publiquei a coordenação e vou trazer o mundo para a turma." />
      </div>
      <Nav next={next} />
    </div>
  );
}

function RoleCard({ active, onClick, icon, title, text }: { active: boolean; onClick: () => void; icon: ReactNode; title: string; text: string }) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "text-left rounded-2xl p-5 ring-1 transition-all cursor-pointer",
        active ? "bg-moss-500/10 ring-moss-500/50 shadow-[var(--shadow-glow)]" : "bg-night-850/70 ring-white/[0.06] hover:ring-white/15",
      )}
    >
      <div className={cx("size-10 rounded-xl flex items-center justify-center mb-3", active ? "bg-moss-500 text-night-950" : "bg-night-700 text-ink-soft")}>{icon}</div>
      <div className="font-display text-[17px] font-semibold">{title}</div>
      <div className="text-[13px] text-ink-muted mt-1">{text}</div>
    </button>
  );
}

function PcStep({ next, back }: { next: () => void; back: () => void }) {
  const [dir, setDir] = useState("");
  const [free, setFree] = useState<number | null>(null);
  const [mc, setMc] = useState("");
  const [launchers, setLaunchers] = useState<LauncherCandidate[]>([]);
  const [launcher, setLauncher] = useState("");
  const [eula, setEula] = useState(false);
  const { busy, error, run } = useAction();
  useEffect(() => {
    api.suggestDir().then((s) => {
      setDir(s.existing_dir ?? s.suggested_dir);
      setFree(s.free_space);
      setMc(s.minecraft_dir ?? "");
      setLaunchers(s.launchers);
      if (s.launchers[0]) setLauncher(s.launchers[0].path);
    });
  }, []);
  const go = async () => {
    const r = await run("init", () => api.setupInit({ data_dir: dir, eula, minecraft_dir: mc, launcher_path: launcher }));
    if (r !== undefined) next();
  };
  return (
    <div>
      <StepHeader kicker="Este PC" title="Onde o mundo vai morar aqui">
        Já deixei tudo sugerido. Só confira e aceite o EULA do Minecraft.
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <Field label="Pasta dos dados do app" hint={`O mundo, o Java e o servidor ficam aqui (reserve uns ${bytes(4 * 1024 ** 3)}).${free ? ` Livre nesse disco: ${bytes(free)}.` : ""}`}>
          <div className="flex gap-2">
            <input className={inputCls} value={dir} onChange={(e) => setDir(e.target.value)} />
            <Button
              icon={<FolderOpen className="size-4" />}
              onClick={async () => {
                const f = await open({ directory: true, defaultPath: dir || undefined });
                if (typeof f === "string") setDir(f.endsWith("pAuletOr3") ? f : `${f}\\pAuletOr3`);
              }}
            >
              Trocar
            </Button>
          </div>
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Pasta .minecraft">
            <input className={inputCls} value={mc} onChange={(e) => setMc(e.target.value)} />
          </Field>
          <Field label="Launcher" hint={launchers.length ? "O botão Jogar abre ele." : "Nenhum encontrado; dá para escolher depois."}>
            <select className={inputCls} value={launcher} onChange={(e) => setLauncher(e.target.value)}>
              <option value="">— depois —</option>
              {launchers.map((l) => (
                <option key={l.path} value={l.path}>
                  {l.label}
                </option>
              ))}
            </select>
          </Field>
        </div>
        <label className="flex gap-3 items-start rounded-xl bg-night-850/70 ring-1 ring-white/[0.06] p-3.5 cursor-pointer">
          <input type="checkbox" checked={eula} onChange={(e) => setEula(e.target.checked)} className="mt-1 accent-[var(--color-moss-500)]" />
          <span className="text-[13px] text-ink-soft">
            Li e aceito o{" "}
            <button type="button" className="text-moss-300 underline cursor-pointer" onClick={() => openUrl("https://aka.ms/MinecraftEULA")}>
              EULA do Minecraft
            </button>
            , necessário para rodar o servidor oficial (baixado direto da Mojang) quando for a minha vez de hospedar.
          </span>
        </label>
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={go} disabled={!eula || !dir} busy={busy === "init"} />
    </div>
  );
}

function NickField({ nick, setNick, autoFocus }: { nick: string; setNick: (s: string) => void; autoFocus?: boolean }) {
  const ok = /^[A-Za-z0-9_]{3,16}$/.test(nick);
  return (
    <Field label="Seu nick no Minecraft" hint={nick && !ok ? "Use de 3 a 16 letras, números ou _." : "Exatamente o mesmo do jogo: é por ele que o servidor reconhece o seu inventário."}>
      <div className="relative">
        <UserRound className="size-4 absolute left-3 top-1/2 -translate-y-1/2 text-ink-faint" />
        <input className={cx(inputCls, "pl-9")} value={nick} onChange={(e) => setNick(e.target.value)} placeholder="ex.: Sara_23" autoFocus={autoFocus} />
      </div>
    </Field>
  );
}

function JoinStep({ invite, setInvite, next, back }: { invite: string; setInvite: (s: string) => void; next: () => void; back: () => void }) {
  const [nick, setNick] = useState("");
  const [joined, setJoined] = useState<JoinResult | null>(null);
  const { busy, error, run } = useAction();
  const nickOk = /^[A-Za-z0-9_]{3,16}$/.test(nick);
  const go = async () => {
    const r = await run("join", () => api.joinWorld(invite, nick));
    if (r) {
      setJoined(r);
      if (r.approved) next();
    }
  };
  return (
    <div>
      <StepHeader kicker="Entrar" title="Cole o convite e escolha seu nick">
        O convite é o link que seu amigo mandou (ou o código que a página do link copiou).
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <Field label="Convite" hint={invite && !looksLikeInvite(invite) ? "Isso não parece um convite. Copie o link inteiro, inclusive a parte depois do #." : undefined}>
          <div className="flex gap-2">
            <input className={cx(inputCls, "font-mono text-[12px]")} value={invite} onChange={(e) => setInvite(e.target.value)} placeholder="https://…/c/…#…  ou  P3J1.…" />
            <Button onClick={() => readText().then((t) => t && setInvite(t.trim())).catch(() => {})}>Colar</Button>
          </div>
        </Field>
        <NickField nick={nick} setNick={setNick} autoFocus={!!invite} />
        {joined && !joined.approved && (
          <InfoNote tone="warn">
            Pedido enviado! <b className="text-ink">{joined.owner_nick}</b> precisa aprovar sua entrada em <b className="text-ink">{joined.name}</b>. Pode continuar: o app avisa quando liberar.
          </InfoNote>
        )}
        <ErrorBanner error={error} />
      </div>
      {joined && !joined.approved ? (
        <Nav back={back} next={next} />
      ) : (
        <Nav back={back} next={go} nextLabel="Entrar no mundo" disabled={!nickOk || !looksLikeInvite(invite)} busy={busy === "join"} />
      )}
    </div>
  );
}

function WorldStep({ next, back }: { next: () => void; back: () => void }) {
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [name, setName] = useState("pAuletOr 3");
  const [nick, setNick] = useState("");
  const [folder, setFolder] = useState("");
  const [level, setLevel] = useState<LevelInfo | null>(null);
  const p = useSyncProgress();
  const { busy, error, setError, run } = useAction();
  const nickOk = /^[A-Za-z0-9_]{3,16}$/.test(nick);
  const pick = async () => {
    const f = await open({ directory: true, title: "Escolha a pasta do mundo (a que tem o level.dat)" });
    if (typeof f !== "string") return;
    setError(null);
    const info = await run("probe", () => api.probeWorldFolder(f));
    if (info) {
      setFolder(f);
      setLevel(info);
    }
  };
  const go = async () => {
    const r = await run("create", () => api.createWorld({ coordinator_url: url, admin_token: token, name, nick, folder: folder || undefined }));
    if (r !== undefined) next();
  };
  return (
    <div>
      <StepHeader kicker="O mundo" title="Criar o mundo da turma">
        Use o endereço e a chave que o script <code className="text-moss-300">publicar-coordenacao.ps1</code> mostrou no final (ficam também no arquivo que ele salvou).
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Endereço da coordenação">
            <input className={inputCls} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://pauletor3-coordenacao….workers.dev" />
          </Field>
          <Field label="Chave de administração">
            <input className={inputCls} type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="cole aqui" />
          </Field>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Nome do mundo">
            <input className={inputCls} value={name} onChange={(e) => setName(e.target.value)} maxLength={40} />
          </Field>
          <NickField nick={nick} setNick={setNick} />
        </div>
        <div className="rounded-2xl bg-night-850/70 ring-1 ring-white/[0.06] p-4">
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="font-semibold">{folder ? level?.level_name || "Mundo escolhido" : "Mundo novo"}</div>
              <div className="text-[12.5px] text-ink-muted">
                {folder
                  ? `Minecraft ${level?.version_name} · ${level?.difficulty} · ${level?.hardcore ? "hardcore" : level?.game_type}`
                  : "Ou traga um mundo que vocês já têm (a pasta com o level.dat)."}
              </div>
            </div>
            <div className="flex gap-2">
              {folder && (
                <Button
                  variant="ghost"
                  onClick={() => {
                    setFolder("");
                    setLevel(null);
                  }}
                >
                  Mundo novo
                </Button>
              )}
              <Button icon={<FolderOpen className="size-4" />} busy={busy === "probe"} onClick={pick}>
                {folder ? "Trocar" : "Importar mundo"}
              </Button>
            </div>
          </div>
        </div>
        {busy === "create" && (
          <InfoNote>
            {folder ? "Cifrando e guardando o mundo na nuvem. Na primeira vez demora alguns minutos (depende da sua internet); depois, só o que mudar trafega." : "Criando o mundo…"}
          </InfoNote>
        )}
        <SyncBar p={busy === "create" ? p : null} />
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={go} nextLabel="Criar o mundo" disabled={!nickOk || !url || !token || !name.trim()} busy={busy === "create"} />
    </div>
  );
}

function InviteOut({ next }: { next: () => void }) {
  const [link, setLink] = useState<string | null>(null);
  const { busy, error, run } = useAction();
  useEffect(() => {
    run("make", () => api.makeInvite(48, 5, false)).then((l) => l && setLink(l));
  }, [run]);
  return (
    <div>
      <StepHeader kicker="Convidar" title="Chame a turma">
        Mande este link para os amigos. Eles abrem no PC, baixam o app e entram — <b className="text-ink">sem conta e sem configurar nada</b>. Vale por 48 horas e 5 pessoas; dá para gerar
        outros depois na aba Mundo.
      </StepHeader>
      <div className="animate-rise space-y-3">
        {link ? (
          <div className="rounded-2xl bg-night-950/60 ring-1 ring-moss-400/25 p-4">
            <code className="block font-mono text-[12.5px] text-ink-soft break-all selectable">{link}</code>
            <div className="flex flex-wrap gap-2 mt-3">
              <Button icon={<Copy className="size-4" />} onClick={() => writeText(link)}>
                Copiar link
              </Button>
              <Button
                variant="primary"
                icon={<MessageCircle className="size-4" />}
                onClick={() => openUrl(`https://wa.me/?text=${encodeURIComponent(`Bora jogar! Abre este link no PC, baixa o app e entra: ${link}`)}`)}
              >
                Mandar no WhatsApp
              </Button>
            </div>
          </div>
        ) : (
          <div className="h-24 rounded-2xl bg-night-850/60 animate-pulse" />
        )}
        <ErrorBanner error={error} />
      </div>
      <Nav next={next} disabled={busy === "make"} />
    </div>
  );
}

function Done({ onDone, owner }: { onDone: () => void; owner: boolean }) {
  const [phase, setPhase] = useState<"working" | "ok">("working");
  useEffect(() => {
    (async () => {
      await autostartOn().catch(() => {});
      await api.finishSetup().catch(() => {});
      // Java e servidor em segundo plano: o primeiro Jogar fica rápido
      api.prepareRuntime().catch(() => {});
      setPhase("ok");
    })();
  }, []);
  return (
    <div className="text-center pt-6">
      <div className="mx-auto size-20 rounded-3xl bg-moss-500/15 text-moss-300 flex items-center justify-center mb-5 animate-rise">
        {phase === "ok" ? <PartyPopper className="size-9" /> : <Sparkles className="size-9 animate-pulse" />}
      </div>
      <h1 className="font-display text-[32px] font-bold tracking-tight animate-rise">Tudo pronto!</h1>
      <p className="text-ink-soft mt-2 max-w-[460px] mx-auto animate-rise">
        {owner ? "Seu mundo está na nuvem e a turma já pode entrar pelo convite." : "Você faz parte do mundo."} Clique em <b className="text-ink">Jogar</b>: se alguém estiver
        hospedando, você entra direto; se não, o mundo acorda no seu PC. No Minecraft, o mundo aparece no Multiplayer como <b className="text-ink">pAuletOr 3</b>.
      </p>
      <p className="text-[12.5px] text-ink-muted mt-3">O app abre com o Windows, discreto na bandeja, para manter sua cópia do mundo em dia.</p>
      <div className="mt-8">
        <Button variant="primary" onClick={onDone} disabled={phase !== "ok"}>
          Ir para o mundo <ArrowRight className="size-4" />
        </Button>
      </div>
    </div>
  );
}
