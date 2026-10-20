import { listen } from "@tauri-apps/api/event";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, ChevronDown, Crown, FolderOpen, Loader2, Plus, Ticket, UserRound, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api, asHuman } from "../lib/api";
import { bytes, syncLabel } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { AddWorldDefaults, LevelInfo, Mundo, SyncProgress } from "../lib/types";
import { Button, Dot, ErrorBanner, Field, InfoNote, Modal, Progress, cx, inputCls } from "./ui";

type Tone = "live" | "busy" | "warn" | "free";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

const looksLikeInvite = (s: string) => /^(P3J1\.|(machenia|machencraft|pauletor3):\/\/entrar\/|https?:\/\/\S+\/c\/[a-z0-9]+\.[a-z0-9]+#)/.test(s.trim());
const nickOk = (n: string) => /^[A-Za-z0-9_]{3,16}$/.test(n);

/** Seletor no topo da barra lateral: o mundo aberto, os outros deste PC e "Adicionar mundo". */
export function WorldSwitcher({
  name,
  tone,
  short,
  onAdd,
  onHome,
  toast,
}: {
  name: string;
  tone: Tone;
  short: string;
  onAdd: () => void;
  onHome: () => void;
  toast: ToastFn;
}) {
  const [open, setOpen] = useState(false);
  const [worlds, setWorlds] = useState<Mundo[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const load = useCallback(() => api.worlds().then(setWorlds).catch(() => setWorlds([])), []);
  useEffect(() => {
    if (!open) return;
    load();
    const fora = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", fora);
    return () => window.removeEventListener("mousedown", fora);
  }, [open, load]);

  const others = (worlds ?? []).filter((w) => !w.active);
  const go = async (w: Mundo) => {
    setBusy(w.dir);
    try {
      await api.switchWorld(w.dir);
      setOpen(false);
      toast({ level: "success", title: `Agora no ${w.name}`, body: "No Minecraft, o endereço continua o mesmo: localhost." });
    } catch (e) {
      const h = asHuman(e);
      toast({ level: "error", title: h.title, body: h.detail });
    } finally {
      setBusy(null);
    }
  };
  const remove = async (w: Mundo) => {
    try {
      await api.removeWorld(w.dir);
      toast({ level: "info", title: `${w.name} saiu da lista`, body: `O mundo continua com o grupo; os arquivos ficaram em ${w.dir}.` });
      load();
    } catch (e) {
      const h = asHuman(e);
      toast({ level: "error", title: h.title, body: h.detail });
    }
  };

  return (
    <div ref={ref} className="relative mx-3 mb-4">
      <button
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-haspopup="menu"
        className="w-full flex items-center gap-3 rounded-xl px-3 py-2.5 text-left bg-white/[0.04] hover:bg-white/[0.07] ring-1 ring-inset ring-white/[0.05] transition-colors cursor-pointer"
      >
        <Dot tone={tone} />
        <span className="min-w-0 flex-1">
          <span className="block text-[13.5px] font-bold text-ink truncate">{name}</span>
          <span className="block text-[12px] text-ink-muted truncate">{short}</span>
        </span>
        <ChevronDown className={cx("size-4 text-ink-faint transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <div role="menu" className="absolute left-0 right-0 top-[calc(100%+6px)] z-40 rounded-xl bg-night-800 ring-1 ring-white/10 shadow-[var(--shadow-pop)] p-1.5 animate-pop">
          <button
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onHome();
            }}
            className="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left bg-white/[0.06] cursor-pointer"
          >
            <Dot tone={tone} />
            <span className="text-[13px] font-semibold truncate">{name}</span>
            <CheckCircle2 className="size-4 text-moss-400 ml-auto shrink-0" aria-label="Aberto agora" />
          </button>
          {worlds === null && <div className="h-9 mx-1 my-1 rounded-lg bg-white/[0.03] animate-pulse" />}
          {others.map((w) => (
            <div key={w.dir} className="group flex items-center rounded-lg hover:bg-white/[0.05]">
              <button
                role="menuitem"
                onClick={() => go(w)}
                disabled={!!busy}
                title={`Trocar para ${w.name}`}
                className="flex-1 min-w-0 flex items-center gap-2.5 px-2.5 py-2 text-left cursor-pointer disabled:opacity-60"
              >
                {busy === w.dir ? <Loader2 className="size-3 animate-spin text-sun-400 shrink-0" /> : <span className="size-2 rounded-[2px] bg-ink-faint/60 shrink-0" />}
                <span className="text-[13px] font-semibold text-ink-soft truncate">{w.name}</span>
                {w.owner && <Crown className="size-3.5 text-sun-400/70 shrink-0" aria-label="Você é o dono" />}
              </button>
              {!w.first && (
                <button
                  onClick={() => remove(w)}
                  title="Tirar da lista deste PC (o mundo continua com o grupo)"
                  aria-label={`Tirar ${w.name} da lista`}
                  className="opacity-0 group-hover:opacity-100 focus:opacity-100 p-2 text-ink-faint hover:text-ember-400 cursor-pointer"
                >
                  <X className="size-3.5" />
                </button>
              )}
            </div>
          ))}
          <div className="my-1 h-px bg-white/[0.06]" />
          <button
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onAdd();
            }}
            className="w-full flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] font-semibold text-ink-muted hover:text-ink hover:bg-white/[0.05] cursor-pointer"
          >
            <Plus className="size-4" /> Adicionar mundo
          </button>
        </div>
      )}
    </div>
  );
}

function useSync(active: boolean) {
  const [p, setP] = useState<SyncProgress | null>(null);
  useEffect(() => {
    if (!active) {
      setP(null);
      return;
    }
    const un = listen<SyncProgress>("p3://sync", (e) => setP(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [active]);
  return p;
}

function Tab({ active, onClick, icon, children }: { active: boolean; onClick: () => void; icon: ReactNode; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      className={cx(
        "flex-1 flex items-center justify-center gap-2 h-9 rounded-[9px] text-[13.5px] font-semibold transition-colors cursor-pointer",
        active ? "bg-night-600 text-ink" : "text-ink-muted hover:text-ink",
      )}
    >
      {icon}
      {children}
    </button>
  );
}

function Nick({ nick, setNick }: { nick: string; setNick: (s: string) => void }) {
  return (
    <Field label="Seu nick no Minecraft" hint={nick && !nickOk(nick) ? "Use de 3 a 16 letras, números ou _." : "O mesmo do jogo: é por ele que o servidor reconhece o seu inventário."}>
      <div className="relative">
        <UserRound className="size-4 absolute left-3 top-1/2 -translate-y-1/2 text-ink-faint" />
        <input className={cx(inputCls, "pl-9")} value={nick} onChange={(e) => setNick(e.target.value)} />
      </div>
    </Field>
  );
}

/** Adicionar mais um mundo: entrar por convite ou (dono) criar/importar. Ao terminar, o app passa a usar ele. */
export function AddWorldModal({ open: isOpen, onClose, initialInvite, toast }: { open: boolean; onClose: () => void; initialInvite?: string | null; toast: ToastFn }) {
  const [mode, setMode] = useState<"join" | "create">("join");
  const [defaults, setDefaults] = useState<AddWorldDefaults | null>(null);
  const [invite, setInvite] = useState("");
  const [nick, setNick] = useState("");
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [level, setLevel] = useState<LevelInfo | null>(null);
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [pending, setPending] = useState<{ name: string; owner: string } | null>(null);
  const { busy, error, setError, run } = useAction();
  const p = useSync(busy === "create");

  useEffect(() => {
    if (!isOpen) return;
    setError(null);
    setPending(null);
    setFolder("");
    setLevel(null);
    setName("");
    setToken("");
    api
      .addWorldDefaults()
      .then((d) => {
        setDefaults(d);
        setNick(d.player_name);
        setUrl(d.coordinator_url ?? "");
      })
      .catch(() => {});
    if (initialInvite) {
      setInvite(initialInvite);
      setMode("join");
    } else {
      setInvite("");
      readText()
        .then((t) => t && looksLikeInvite(t) && setInvite(t.trim()))
        .catch(() => {});
    }
  }, [isOpen, initialInvite, setError]);

  const sameCoord = !!defaults?.coordinator_url && url.trim().replace(/\/+$/, "") === defaults.coordinator_url;
  const needToken = !(sameCoord && defaults?.has_admin_token);

  const pick = async () => {
    const f = await open({ directory: true, title: "Escolha a pasta do mundo (a que tem o level.dat)" });
    if (typeof f !== "string") return;
    const info = await run("probe", () => api.probeWorldFolder(f));
    if (info) {
      setFolder(f);
      setLevel(info);
      if (!name.trim()) setName(info.level_name.slice(0, 40));
    }
  };

  const join = async () => {
    const r = await run("join", () => api.addWorldJoin(invite.trim(), nick));
    if (!r) return;
    if (r.approved) {
      toast({ level: "success", title: `Você entrou no ${r.name}`, body: "Já é o mundo aberto. Clique em Jogar quando quiser." });
      onClose();
    } else {
      setPending({ name: r.name, owner: r.owner_nick });
    }
  };
  const create = async () => {
    const r = await run("create", () =>
      api.addWorldCreate({ coordinator_url: url.trim() || undefined, admin_token: token.trim() || undefined, name: name.trim(), nick, folder: folder || undefined }),
    );
    if (r === undefined) return;
    toast({ level: "success", title: `${name.trim()} criado`, body: "Já é o mundo aberto. Mande o convite pela aba Grupo." });
    onClose();
  };

  const working = busy === "join" || busy === "create";
  const pct = p ? (p.bytes_total > 0 ? (p.bytes_done / p.bytes_total) * 100 : p.files_total ? (p.files_done / p.files_total) * 100 : 0) : 0;
  const footer = pending ? (
    <Button variant="primary" onClick={onClose}>
      Entendi
    </Button>
  ) : (
    <>
      <Button variant="ghost" onClick={onClose} disabled={working}>
        Cancelar
      </Button>
      {mode === "join" ? (
        <Button variant="primary" onClick={join} busy={busy === "join"} disabled={!nickOk(nick) || !looksLikeInvite(invite)}>
          Entrar no mundo
        </Button>
      ) : (
        <Button variant="primary" onClick={create} busy={busy === "create"} disabled={!nickOk(nick) || !name.trim() || !url.trim() || (needToken && !token.trim())}>
          Criar o mundo
        </Button>
      )}
    </>
  );

  return (
    <Modal open={isOpen} title="Adicionar mundo" onClose={() => !working && onClose()} footer={footer} wide>
      {pending ? (
        <InfoNote tone="warn">
          Pedido enviado! <b className="text-ink">{pending.owner}</b> precisa aprovar sua entrada em <b className="text-ink">{pending.name}</b>. O mundo já está na sua lista e o app
          avisa quando liberar.
        </InfoNote>
      ) : (
        <div className="space-y-4">
          <p className="text-[13.5px] text-ink-muted">
            Cada mundo tem a sua cópia e o seu grupo. Um fica aberto por vez: o que estiver escolhido aqui é o que aparece no Minecraft em <b className="text-ink-soft">localhost</b>.
          </p>
          <div className="flex gap-1 p-1 rounded-xl bg-night-950/60 ring-1 ring-inset ring-white/[0.06]">
            <Tab active={mode === "join"} onClick={() => setMode("join")} icon={<Ticket className="size-4" />}>
              Recebi um convite
            </Tab>
            <Tab active={mode === "create"} onClick={() => setMode("create")} icon={<Crown className="size-4" />}>
              Criar um mundo
            </Tab>
          </div>

          {mode === "join" ? (
            <>
              <Field label="Convite" hint={invite && !looksLikeInvite(invite) ? "Isso não parece um convite. Copie o link inteiro, inclusive a parte depois do #." : undefined}>
                <div className="flex gap-2">
                  <input
                    className={cx(inputCls, "font-mono text-[12px]")}
                    value={invite}
                    onChange={(e) => setInvite(e.target.value)}
                    placeholder="https://…/c/…#…  ou  P3J1.…"
                    autoFocus
                  />
                  <Button onClick={() => readText().then((t) => t && setInvite(t.trim())).catch(() => {})}>Colar</Button>
                </div>
              </Field>
              <Nick nick={nick} setNick={setNick} />
            </>
          ) : (
            <>
              <div className="grid grid-cols-2 gap-3">
                <Field label="Nome do mundo">
                  <input className={inputCls} value={name} onChange={(e) => setName(e.target.value)} maxLength={40} placeholder="ex.: Ilha do Grupo" autoFocus />
                </Field>
                <Nick nick={nick} setNick={setNick} />
              </div>
              <div className="rounded-[var(--radius-card)] bg-night-850/70 ring-1 ring-inset ring-white/[0.07] p-4 flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="font-semibold truncate">{folder ? level?.level_name || "Mundo escolhido" : "Mundo novo"}</div>
                  <div className="text-[12.5px] text-ink-muted">
                    {folder && level ? `Minecraft ${level.version_name} · ${level.difficulty} · ${level.hardcore ? "hardcore" : level.game_type}` : "Ou traga um mundo que vocês já têm (a pasta com o level.dat)."}
                  </div>
                </div>
                <div className="flex gap-2 shrink-0">
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
              <div className="grid grid-cols-2 gap-3">
                <Field label="Endereço da coordenação" hint={sameCoord ? "A mesma do mundo aberto." : undefined}>
                  <input className={inputCls} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…workers.dev" />
                </Field>
                {needToken ? (
                  <Field label="Chave de administração" hint="A que o publicar-coordenacao.ps1 mostrou.">
                    <input className={inputCls} type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="cole aqui" />
                  </Field>
                ) : (
                  <Field label="Chave de administração" hint="Já guardada neste PC.">
                    <input className={inputCls} disabled value="••••••••••••" />
                  </Field>
                )}
              </div>
              {busy === "create" && (
                <InfoNote>
                  {folder ? "Cifrando e guardando o mundo na nuvem. Na primeira vez demora alguns minutos (depende da sua internet)." : "Criando o mundo…"}
                </InfoNote>
              )}
              {busy === "create" && p && (
                <Progress value={pct} label={syncLabel[p.phase]} sub={p.bytes_total ? `${bytes(p.bytes_done)} de ${bytes(p.bytes_total)}` : `${p.files_done}/${p.files_total}`} />
              )}
            </>
          )}
          <ErrorBanner error={error} />
        </div>
      )}
    </Modal>
  );
}
