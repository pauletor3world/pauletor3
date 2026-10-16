import { listen } from "@tauri-apps/api/event";
import { enable as autostartOn } from "@tauri-apps/plugin-autostart";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Cloud,
  Copy,
  Crown,
  FolderOpen,
  Globe,
  HardDriveDownload,
  Mountain,
  Network,
  PartyPopper,
  ShieldCheck,
  Ticket,
  UserRound,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Button, ErrorBanner, Field, InfoNote, Progress, cx, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { bytes, dateTime, syncLabel } from "../lib/format";
import { useAction } from "../lib/hooks";
import type { LauncherCandidate, ManifestSummary, SyncProgress, WorldNet, WorldSettings } from "../lib/types";

type Role = "owner" | "friend";
type StepId = "welcome" | "you" | "cloud" | "world" | "invite_in" | "network" | "pc" | "invite_out" | "done";

const STEPS: Record<Role, StepId[]> = {
  owner: ["welcome", "you", "cloud", "world", "network", "pc", "invite_out", "done"],
  friend: ["welcome", "you", "invite_in", "network", "pc", "done"],
};

const META: Record<StepId, { title: string; icon: ReactNode }> = {
  welcome: { title: "Boas-vindas", icon: <Mountain className="size-4" /> },
  you: { title: "Você", icon: <UserRound className="size-4" /> },
  cloud: { title: "Nuvem", icon: <Cloud className="size-4" /> },
  world: { title: "Mundo", icon: <Globe className="size-4" /> },
  invite_in: { title: "Convite", icon: <Ticket className="size-4" /> },
  network: { title: "Rede", icon: <Network className="size-4" /> },
  pc: { title: "Este PC", icon: <ShieldCheck className="size-4" /> },
  invite_out: { title: "Convites", icon: <Ticket className="size-4" /> },
  done: { title: "Pronto", icon: <PartyPopper className="size-4" /> },
};

export default function Wizard({ onDone }: { onDone: () => void }) {
  const [role, setRole] = useState<Role>("owner");
  const [i, setI] = useState(0);
  const steps = STEPS[role];
  const step = steps[i];
  const next = () => setI((x) => Math.min(x + 1, steps.length - 1));
  const back = () => setI((x) => Math.max(x - 1, 0));

  return (
    <div className="flex-1 flex min-h-0">
      <aside className="w-[240px] shrink-0 p-6 border-r border-white/[0.05] bg-night-950/30">
        <img src="/logo.png" alt="" className="size-12 rounded-2xl mb-4 shadow-lg" />
        <div className="font-display text-xl font-bold">pAuletOr 3</div>
        <div className="text-ink-muted text-[12.5px] mb-7">Primeira configuração</div>
        <ol className="space-y-1">
          {steps.map((s, k) => (
            <li
              key={s}
              className={cx(
                "flex items-center gap-2.5 h-9 px-3 rounded-xl text-[13px]",
                k === i ? "bg-white/[0.07] text-ink font-semibold" : k < i ? "text-moss-300" : "text-ink-faint",
              )}
            >
              <span className="size-5 flex items-center justify-center">{k < i ? <Check className="size-4" /> : META[s].icon}</span>
              {META[s].title}
            </li>
          ))}
        </ol>
      </aside>
      <main className="flex-1 scroll">
        <div className="max-w-[640px] mx-auto px-10 py-10" key={step}>
          {step === "welcome" && <Welcome role={role} setRole={setRole} next={next} />}
          {step === "you" && <You role={role} next={next} back={back} />}
          {step === "cloud" && <CloudStep next={next} back={back} />}
          {step === "world" && <WorldStep next={next} back={back} />}
          {step === "invite_in" && <InviteIn next={next} back={back} />}
          {step === "network" && <NetworkStep role={role} next={next} back={back} />}
          {step === "pc" && <PcStep next={next} back={back} />}
          {step === "invite_out" && <InviteOut next={next} back={back} />}
          {step === "done" && <Done onDone={onDone} />}
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
  return [p, setP] as const;
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

function Welcome({ role, setRole, next }: { role: Role; setRole: (r: Role) => void; next: () => void }) {
  return (
    <div>
      <StepHeader kicker="Bem-vindo" title="Vamos deixar o mundo pronto">
        O pAuletOr 3 mora na nuvem e é hospedado por turnos, no PC de quem está jogando. Ninguém precisa deixar servidor ligado, e todo mundo entra sempre pelo mesmo endereço.
      </StepHeader>
      <div className="grid grid-cols-2 gap-3 animate-rise">
        <RoleCard active={role === "owner"} onClick={() => setRole("owner")} icon={<Crown className="size-5" />} title="Sou o dono" text="Vou conectar a nuvem, trazer o mundo e convidar os amigos. Feito uma vez só." />
        <RoleCard active={role === "friend"} onClick={() => setRole("friend")} icon={<Ticket className="size-5" />} title="Recebi um convite" text="Tenho um código e uma senha. Leva menos de 5 minutos." />
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

function You({ role, next, back }: { role: Role; next: () => void; back: () => void }) {
  const [dir, setDir] = useState("");
  const [free, setFree] = useState<number | null>(null);
  const [nick, setNick] = useState("");
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
  const nickOk = /^[A-Za-z0-9_]{3,16}$/.test(nick);
  const go = async () => {
    const r = await run("init", () => api.setupInit({ data_dir: dir, player_name: nick, eula, owner: role === "owner", minecraft_dir: mc, launcher_path: launcher }));
    if (r !== undefined) next();
  };
  return (
    <div>
      <StepHeader kicker="Passo 1" title="Você e o seu PC">
        O nick tem que ser <b className="text-ink">exatamente</b> o mesmo do Minecraft: é por ele que o servidor reconhece o seu inventário.
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <Field label="Seu nick no Minecraft" hint={nick && !nickOk ? "Use de 3 a 16 letras, números ou _." : undefined}>
          <input className={inputCls} value={nick} onChange={(e) => setNick(e.target.value)} placeholder="ex.: mchado" autoFocus />
        </Field>
        <Field label="Onde guardar os dados do app" hint={`O mundo, o Java e o servidor ficam aqui (uns ${bytes(4 * 1024 ** 3)} com folga).${free ? ` Livre nesse disco: ${bytes(free)}.` : ""}`}>
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
          <Field label="Launcher" hint={launchers.length ? undefined : "Nenhum encontrado; dá para escolher depois."}>
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
            , necessário para rodar o servidor oficial quando for a minha vez de hospedar.
          </span>
        </label>
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={go} disabled={!nickOk || !eula || !dir} busy={busy === "init"} />
    </div>
  );
}

function CloudStep({ next, back }: { next: () => void; back: () => void }) {
  const [ok, setOk] = useState(false);
  const [cid, setCid] = useState("");
  const [secret, setSecret] = useState("");
  const { busy, error, run } = useAction();
  useEffect(() => {
    api.worldProbe().then(() => setOk(true)).catch(() => {});
  }, []);
  return (
    <div>
      <StepHeader kicker="Passo 2" title="Conectar a nuvem do mundo">
        O mundo fica guardado no Google Drive da <b className="text-ink">conta dedicada ao pAuletOr 3</b>. Ao clicar, o navegador abre: entre com essa conta (não a pessoal) e autorize o acesso.
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Client ID do Google" hint="Termina com .apps.googleusercontent.com">
            <input className={inputCls} value={cid} onChange={(e) => setCid(e.target.value)} placeholder="1234…apps.googleusercontent.com" />
          </Field>
          <Field label="Client secret">
            <input className={inputCls} type="password" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="GOCSPX-…" />
          </Field>
        </div>
        <InfoNote tone="warn">
          O acesso compartilhado do rclone ao Google Drive está sendo desligado em 2026, então o mundo usa uma chave própria (grátis, sem cartão). O passo a passo está no README, em
          “Google Drive: chave própria”. Com ela, o app só enxerga os arquivos que ele mesmo criou.
        </InfoNote>
        <InfoNote>
          No navegador, entre com a conta do mundo e autorize. Se aparecer “o Google não verificou este app”, clique em <b className="text-ink">Avançado → Acessar pAuletOr 3</b>: o app é seu. O acesso
          fica guardado criptografado neste PC.
        </InfoNote>
        {ok ? (
          <InfoNote tone="ok">Nuvem conectada e respondendo.</InfoNote>
        ) : (
          <Button
            variant="primary"
            icon={<Cloud className="size-4" />}
            busy={busy === "drive"}
            disabled={!cid.trim() || !secret.trim()}
            onClick={() => run("drive", () => api.driveAuthorize(cid, secret)).then((r) => r !== undefined && setOk(true))}
          >
            {busy === "drive" ? "Esperando o login no navegador…" : "Conectar Google Drive"}
          </Button>
        )}
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={next} disabled={!ok} />
    </div>
  );
}

function WorldStep({ next, back }: { next: () => void; back: () => void }) {
  const [head, setHead] = useState<ManifestSummary | null | undefined>(undefined);
  const [seed, setSeed] = useState("");
  const [p, setP] = useSyncProgress();
  const { busy, error, run } = useAction();
  const probe = () =>
    run("probe", api.worldProbe).then((r) => {
      if (r) setHead(r.head);
    });
  useEffect(() => {
    probe();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const importIt = async () => {
    const f = await open({ directory: true, title: "Escolha a pasta do mundo (a que tem o level.dat)" });
    if (typeof f !== "string") return;
    setP(null);
    const r = await run("import", () => api.importWorld(f));
    if (r) probe();
  };
  const create = async () => {
    const s: WorldSettings = {
      format: 1,
      world_name: "pAuletOr 3",
      motd: "pAuletOr 3",
      difficulty: "normal",
      gamemode: "survival",
      view_distance: 10,
      simulation_distance: 10,
      max_players: 8,
      pvp: true,
      hardcore: false,
      seed: seed.trim(),
      spawn_protection: 0,
      allow_flight: false,
    };
    const r = await run("create", () => api.createWorld(s));
    if (r !== undefined) next();
  };
  return (
    <div>
      <StepHeader kicker="Passo 3" title="O mundo">
        Traga a pasta do <b className="text-ink">pAuletOr 3</b> para a nuvem. O app confere a versão (precisa ser 1.21.11 ou anterior) e usa a dificuldade do próprio mundo.
      </StepHeader>
      <div className="space-y-4 animate-rise">
        {head === undefined && !error && <div className="h-20 rounded-2xl bg-night-850/60 animate-pulse" />}
        {head && (
          <InfoNote tone="ok">
            Mundo na nuvem: versão de {dateTime(head.created_at)} por {head.host_player}, {bytes(head.total_size)} em {head.file_count} arquivos.
          </InfoNote>
        )}
        {head === null && (
          <div className="grid grid-cols-2 gap-3">
            <div className="rounded-2xl bg-night-850/70 ring-1 ring-white/[0.06] p-5">
              <HardDriveDownload className="size-6 text-moss-400 mb-3" />
              <div className="font-display text-[16px] font-semibold">Importar mundo existente</div>
              <p className="text-[12.5px] text-ink-muted mt-1 mb-4">Escolha a pasta do mundo (a que contém level.dat, region…).</p>
              <Button variant="primary" busy={busy === "import"} onClick={importIt}>
                Escolher pasta
              </Button>
            </div>
            <div className="rounded-2xl bg-night-850/70 ring-1 ring-white/[0.06] p-5">
              <Mountain className="size-6 text-sun-400 mb-3" />
              <div className="font-display text-[16px] font-semibold">Criar mundo novo</div>
              <input className={cx(inputCls, "mt-2 mb-3")} value={seed} onChange={(e) => setSeed(e.target.value)} placeholder="Seed (opcional)" />
              <Button busy={busy === "create"} onClick={create}>
                Criar no primeiro Jogar
              </Button>
            </div>
          </div>
        )}
        {busy === "import" && <SyncBar p={p} />}
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={next} disabled={!head} />
    </div>
  );
}

function InviteIn({ next, back }: { next: () => void; back: () => void }) {
  const [code, setCode] = useState("");
  const [pw, setPw] = useState("");
  const [ok, setOk] = useState<string | null>(null);
  const { busy, error, run } = useAction();
  return (
    <div>
      <StepHeader kicker="Passo 2" title="Cole o convite">
        O dono te mandou um código (começa com <code className="font-mono text-moss-300">P3.</code>) e uma senha separada.
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <Field label="Código de convite">
          <textarea className={cx(inputCls, "h-28 py-2 font-mono text-[11.5px]")} value={code} onChange={(e) => setCode(e.target.value)} placeholder="P3.…" />
        </Field>
        <Field label="Senha do convite">
          <input className={inputCls} type="password" value={pw} onChange={(e) => setPw(e.target.value)} />
        </Field>
        {ok ? (
          <InfoNote tone="ok">{ok}</InfoNote>
        ) : (
          <Button
            variant="primary"
            busy={busy === "inv"}
            disabled={!code.trim() || pw.length < 8}
            onClick={() => run("inv", () => api.acceptInvite(code, pw)).then((r) => r && setOk(`Convite de ${r.created_by} aceito: nuvem do ${r.world_name} conectada.`))}
          >
            {busy === "inv" ? "Abrindo o convite…" : "Abrir convite"}
          </Button>
        )}
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={next} disabled={!ok} />
    </div>
  );
}

function NetworkStep({ role, next, back }: { role: Role; next: () => void; back: () => void }) {
  const [ts, setTs] = useState<WorldNet | null>(null);
  const [key, setKey] = useState("");
  const [exp, setExp] = useState("");
  const [domain, setDomain] = useState("pauletor3");
  const [token, setToken] = useState("");
  const [hook, setHook] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const { busy, error, run } = useAction();
  const look = () => api.tailscaleStatus().then(setTs);
  useEffect(() => {
    look();
  }, []);
  const owner = role === "owner";
  const connected = !!ts && ts.on_world;

  return (
    <div>
      <StepHeader kicker={owner ? "Passo 4" : "Passo 3"} title="A rede privada do mundo">
        O jogo passa por uma rede privada (Tailscale): só quem tem o convite enxerga o servidor, e nenhuma porta fica aberta na internet.
      </StepHeader>
      <div className="space-y-5 animate-rise">
        <div className="rounded-2xl bg-night-850/70 ring-1 ring-white/[0.06] p-5 space-y-3">
          <div className="flex items-center justify-between">
            <div className="font-display text-[16px] font-semibold">Tailscale</div>
            {ts && <span className={cx("text-[12px] font-medium", connected ? "text-moss-300" : "text-sun-300")}>{!ts.installed ? "não instalado" : connected ? `na rede do mundo · ${ts.ipv4}` : ts.backend_state === "Running" ? `em outra rede (${ts.tailnet ?? "?"})` : ts.backend_state}</span>}
          </div>
          {ts && !ts.installed && (
            <>
              <p className="text-[12.5px] text-ink-muted">O instalador oficial do Tailscale vai abrir e pedir permissão de administrador. Pode aceitar.</p>
              <Button variant="primary" busy={busy === "install"} onClick={() => run("install", api.tailscaleInstall).then(look)}>
                Instalar o Tailscale
              </Button>
            </>
          )}
          {ts?.installed && !connected && (
            <>
              {ts.backend_state === "Running" && (
                <p className="text-[12.5px] text-ink-muted">
                  Este PC já usa o Tailscale em outra rede. O app adiciona a rede do mundo como um perfil a mais: a sua continua lá e dá para alternar pelo ícone do Tailscale.
                </p>
              )}
              {owner && (
                <div className="grid grid-cols-[1fr_170px] gap-2">
                  <Field label="Auth key reutilizável" hint="login.tailscale.com → Settings → Keys → Generate auth key (marque Reusable).">
                    <input className={inputCls} type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder="tskey-auth-…" />
                  </Field>
                  <Field label="Expira em">
                    <input className={inputCls} type="date" value={exp} onChange={(e) => setExp(e.target.value)} />
                  </Field>
                </div>
              )}
              <Button
                variant="primary"
                busy={busy === "up"}
                disabled={owner && !key}
                onClick={() =>
                  run("up", async () => {
                    if (owner) await api.saveNetwork({ tailscale_auth_key: key, tailscale_key_expires: exp || undefined });
                    return api.tailscaleUp(owner ? key : undefined);
                  }).then((s) => s && setTs(s))
                }
              >
                Entrar na rede do mundo
              </Button>
            </>
          )}
        </div>

        {owner && (
          <div className="rounded-2xl bg-night-850/70 ring-1 ring-white/[0.06] p-5 space-y-3">
            <div className="font-display text-[16px] font-semibold">Endereço fixo e diário</div>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Nome no DuckDNS" hint={`${domain || "…"}.duckdns.org`}>
                <input className={inputCls} value={domain} onChange={(e) => setDomain(e.target.value)} />
              </Field>
              <Field label="Token do DuckDNS">
                <input className={inputCls} type="password" value={token} onChange={(e) => setToken(e.target.value)} />
              </Field>
            </div>
            <Field label="Webhook do Discord (diário do mundo)" hint="Canal → Editar canal → Integrações → Webhooks → Copiar URL.">
              <input className={inputCls} type="password" value={hook} onChange={(e) => setHook(e.target.value)} placeholder="https://discord.com/api/webhooks/…" />
            </Field>
            <div className="flex gap-2">
              <Button
                busy={busy === "net"}
                onClick={() =>
                  run("net", async () => {
                    await api.saveNetwork({ duckdns_domain: domain, duckdns_token: token || undefined, discord_webhook: hook || undefined });
                    const parts: string[] = [];
                    if (hook) {
                      await api.testWebhook();
                      parts.push("mensagem de teste enviada ao Discord");
                    }
                    if (token && connected) {
                      const t = await api.testDuckdns();
                      parts.push(t.resolved.includes(t.expected) ? `${t.address} → ${t.expected}` : `${t.address} atualizado (o DNS pode levar 1 min)`);
                    }
                    setMsg(parts.length ? `Salvo: ${parts.join("; ")}.` : "Salvo.");
                  })
                }
              >
                Salvar e testar
              </Button>
            </div>
            {msg && <InfoNote tone="ok">{msg}</InfoNote>}
            <p className="text-[12px] text-ink-muted">Pode deixar em branco e configurar depois em Configurações.</p>
          </div>
        )}
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={next} disabled={!connected} />
    </div>
  );
}

function PcStep({ next, back }: { next: () => void; back: () => void }) {
  const [done, setDone] = useState<Record<string, boolean>>({});
  const [p, setP] = useSyncProgress();
  const { busy, error, run } = useAction();
  const mark = (k: string) => setDone((d) => ({ ...d, [k]: true }));
  const items: { k: string; title: string; text: string; action: () => Promise<unknown>; label: string }[] = [
    { k: "fw", title: "Firewall do Windows", text: "Libera a porta do jogo só para a rede privada. Pede permissão de administrador uma vez.", action: api.installFirewall, label: "Configurar" },
    { k: "dat", title: "Entrada no Minecraft", text: "Adiciona “pAuletOr 3” na lista de Multiplayer (com backup da lista). Feche o Minecraft antes.", action: api.ensureServersDat, label: "Adicionar" },
    {
      k: "rt",
      title: "Java e servidor",
      text: "Baixa o Java 21 e o servidor fixado (≈110 MB), conferindo cada arquivo.",
      action: () => {
        setP(null);
        return api.prepareRuntime();
      },
      label: "Baixar",
    },
  ];
  return (
    <div>
      <StepHeader kicker="Quase lá" title="Preparar este PC">
        Três ajustes rápidos para este PC poder hospedar quando for a sua vez.
      </StepHeader>
      <div className="space-y-3 animate-rise">
        {items.map((it) => (
          <div key={it.k} className="flex items-center gap-4 rounded-2xl bg-night-850/70 ring-1 ring-white/[0.06] p-4">
            <div className={cx("size-9 rounded-xl flex items-center justify-center", done[it.k] ? "bg-moss-500 text-night-950" : "bg-night-700 text-ink-muted")}>
              {done[it.k] ? <Check className="size-5" /> : <ShieldCheck className="size-5" />}
            </div>
            <div className="flex-1">
              <div className="font-semibold">{it.title}</div>
              <div className="text-[12.5px] text-ink-muted">{it.text}</div>
              {it.k === "rt" && busy === "rt" && <SyncBar p={p} />}
            </div>
            {!done[it.k] && (
              <Button busy={busy === it.k} onClick={() => run(it.k, it.action).then((r) => r !== undefined && mark(it.k))}>
                {it.label}
              </Button>
            )}
          </div>
        ))}
        <ErrorBanner error={error} />
        <p className="text-[12px] text-ink-muted">Se algum item falhar, dá para continuar e corrigir depois em Saúde do sistema.</p>
      </div>
      <Nav back={back} next={next} />
    </div>
  );
}

function InviteOut({ next, back }: { next: () => void; back: () => void }) {
  const [pw, setPw] = useState("");
  const [code, setCode] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const { busy, error, run } = useAction();
  return (
    <div>
      <StepHeader kicker="Último passo" title="Convide os amigos">
        Crie uma senha, gere o código e mande para cada amigo: o código por mensagem, a senha por outro caminho (voz, por exemplo). Dá para gerar outros depois em Configurações.
      </StepHeader>
      <div className="space-y-4 animate-rise">
        <div className="flex gap-2">
          <input className={inputCls} type="password" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Senha do convite (mín. 8 caracteres)" />
          <Button variant="primary" busy={busy === "inv"} disabled={pw.length < 8} onClick={() => run("inv", () => api.createInvite(pw)).then((c) => c && setCode(c))}>
            Gerar convite
          </Button>
        </div>
        {code && (
          <div className="rounded-xl bg-night-950/60 ring-1 ring-moss-500/25 p-3.5">
            <code className="block font-mono text-[11px] text-ink-soft break-all max-h-28 overflow-auto selectable">{code}</code>
            <Button
              className="mt-3"
              icon={copied ? <Check className="size-4" /> : <Copy className="size-4" />}
              onClick={async () => {
                await writeText(code);
                setCopied(true);
              }}
            >
              {copied ? "Copiado" : "Copiar código"}
            </Button>
          </div>
        )}
        <ErrorBanner error={error} />
      </div>
      <Nav back={back} next={next} nextLabel={code ? "Continuar" : "Pular por agora"} />
    </div>
  );
}

function Done({ onDone }: { onDone: () => void }) {
  const { busy, error, run } = useAction();
  return (
    <div className="text-center pt-6">
      <div className="mx-auto size-20 rounded-3xl hills ring-1 ring-white/10 flex items-center justify-center mb-6 animate-rise shadow-[var(--shadow-glow)]">
        <PartyPopper className="size-9 text-sun-300" />
      </div>
      <h1 className="font-display text-[32px] font-bold tracking-tight animate-rise">Tudo pronto!</h1>
      <p className="text-ink-soft mt-2 max-w-md mx-auto animate-rise">
        A partir de agora é só clicar em <b className="text-ink">Jogar</b>. Quando alguém estiver jogando, você vê na tela inicial e entra pelo Multiplayer → pAuletOr 3.
      </p>
      <div className="mt-8">
        <Button
          variant="primary"
          className="h-12 px-6 text-[15px]"
          busy={busy === "fin"}
          onClick={() =>
            run("fin", async () => {
              await api.finishSetup();
              await autostartOn().catch(() => {});
            }).then((r) => r !== undefined && onDone())
          }
        >
          Abrir o pAuletOr 3
        </Button>
      </div>
      <div className="mt-4 max-w-md mx-auto">
        <ErrorBanner error={error} />
      </div>
    </div>
  );
}
