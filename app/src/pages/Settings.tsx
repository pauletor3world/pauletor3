import { disable as autostartOff, enable as autostartOn, isEnabled as autostartIs } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import { ChevronDown, FolderOpen, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Button, Card, CardTitle, ErrorBanner, InfoNote, PageHeader, Pill, Toggle, cx, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { bytes } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { Category, ClientVersion, JournalStatus, LauncherCandidate, SettingsView, Status, WorldSettings } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

export default function SettingsPage({ status, isOwner, toast }: { status: Status | null; isOwner: boolean; toast: ToastFn }) {
  const [s, setS] = useState<SettingsView | null>(null);
  const [launchers, setLaunchers] = useState<LauncherCandidate[]>([]);
  const [versions, setVersions] = useState<ClientVersion[]>([]);
  const [auto, setAuto] = useState(false);
  const [more, setMore] = useState(false);
  const { busy, error, setError, run } = useAction();

  const load = useCallback(() => {
    run("load", api.settings).then((v) => v && setS(v));
  }, [run]);
  useEffect(() => {
    load();
    api.detectLaunchers().then(setLaunchers).catch(() => {});
    api.clientVersions().then(setVersions).catch(() => {});
    autostartIs().then(setAuto).catch(() => {});
  }, [load]);

  const save = async (patch: Parameters<typeof api.saveSettings>[0], msg = "Salvo") => {
    const r = await run("save", () => api.saveSettings(patch));
    if (r !== undefined) {
      toast({ level: "success", title: msg, body: "" });
      load();
    }
  };

  if (!s) {
    return (
      <div>
        <PageHeader title="Configurações" />
        <ErrorBanner error={error} onClose={() => setError(null)} />
        <div className="h-80 rounded-[var(--radius-card)] bg-night-850/60 animate-pulse" />
      </div>
    );
  }
  const c = s.config;
  const free = status?.lease?.kind === "free" && !status.me_hosting;

  return (
    <div>
      <PageHeader title="Configurações" />
      <div className="space-y-5">
        <ErrorBanner error={error} onClose={() => setError(null)} />

        <Section title="Jogo">
          <Row label="Seu nick" hint="Para trocar, peça ao dono do mundo.">
            <span className="text-[14px] font-semibold text-ink">{c.player_name}</span>
          </Row>
          {s.direct_launch_available && (
            <Toggle
              checked={c.direct_launch}
              onChange={(v) => save({ direct_launch: v }, v ? "O Jogar abre o Minecraft direto" : "O Jogar abre o launcher")}
              label="Abrir o Minecraft pelo app"
              hint="O Jogar já entra no mundo, sem launcher."
            />
          )}
          {s.direct_launch_available && c.direct_launch ? (
            <Row label="Versão do Minecraft" hint={versions.length > 1 ? "OptiFine e Fabric da sua .minecraft aparecem aqui." : undefined}>
              <select className={cx(inputCls, "w-[260px]")} value={c.client_version ?? ""} aria-label="Versão do Minecraft" onChange={(e) => save({ client_version: e.target.value }, "Versão do Minecraft escolhida")}>
                {versions.map((v) => (
                  <option key={v.id} value={v.id}>
                    {v.label}
                  </option>
                ))}
                {c.client_version && !versions.some((v) => v.id === c.client_version) && <option value={c.client_version}>{c.client_version} (não encontrada)</option>}
              </select>
            </Row>
          ) : (
            <Row label="Launcher" hint="Abre quando você clica em Jogar.">
              <div className="flex gap-2 w-[340px]">
                <select className={inputCls} value={c.launcher_path ?? ""} aria-label="Launcher" onChange={(e) => save({ launcher_path: e.target.value }, "Launcher escolhido")}>
                  <option value="">— escolha —</option>
                  {launchers.map((l) => (
                    <option key={l.path} value={l.path}>
                      {l.label}
                    </option>
                  ))}
                  {c.launcher_path && !launchers.some((l) => l.path === c.launcher_path) && <option value={c.launcher_path}>{c.launcher_path}</option>}
                </select>
                <Button
                  icon={<FolderOpen className="size-4" />}
                  title="Procurar"
                  onClick={async () => {
                    const f = await open({ filters: [{ name: "Programa", extensions: ["exe"] }] });
                    if (typeof f === "string") save({ launcher_path: f }, "Launcher escolhido");
                  }}
                />
              </div>
            </Row>
          )}
        </Section>

        {isOwner && <WorldSection free={free} toast={toast} />}

        <Section title="Avisos">
          <Toggle checked={c.notify_toasts} onChange={(v) => save({ notify_toasts: v })} label="Notificações do Windows" hint="Quando alguém abre o mundo ou algo dá errado." />
          <Toggle
            checked={auto}
            onChange={async (v) => {
              await (v ? autostartOn() : autostartOff()).catch(() => {});
              setAuto(await autostartIs().catch(() => v));
            }}
            label="Iniciar com o Windows"
            hint="Fica na bandeja, sem atrapalhar."
          />
        </Section>

        <button
          onClick={() => setMore((m) => !m)}
          aria-expanded={more}
          className="flex items-center gap-2 mx-auto text-[13px] font-semibold text-ink-muted hover:text-ink cursor-pointer py-2"
        >
          <ChevronDown className={cx("size-4 transition-transform", !more && "-rotate-90")} />
          {more ? "Menos opções" : "Mais opções"}
        </button>

        {more && (
          <>
            <Section title="Desempenho">
              <Row label="Pasta .minecraft">
                <PathSetting value={c.minecraft_dir ?? ""} directory onSave={(v) => save({ minecraft_dir: v })} />
              </Row>
              {s.direct_launch_available && c.direct_launch && (
                <MemorySetting
                  label="Memória do jogo"
                  value={c.client_ram_mb ?? 0}
                  auto={s.auto_client_heap_mb}
                  total={s.total_ram_mb}
                  recommended={[3072, 6144]}
                  onSave={(v) => save({ client_ram_mb: v })}
                />
              )}
              <MemorySetting
                label="Memória do servidor"
                value={c.ram_mb ?? 0}
                auto={s.auto_heap_mb}
                total={s.total_ram_mb}
                recommended={[4096, 8192]}
                onSave={(v) => save({ ram_mb: v })}
              />
              <ChoiceSetting
                label="Salvar na nuvem a cada"
                value={c.checkpoint_minutes}
                options={[5, 10, 15]}
                unit="min"
                hint="Numa queda de luz, perde-se no máximo isso."
                onSave={(v) => save({ checkpoint_minutes: v })}
              />
              <ChoiceSetting
                label="Esperar quem caiu por"
                value={c.grace_minutes}
                options={[1, 2, 5, 10]}
                unit="min"
                hint="Antes de fechar o mundo."
                onSave={(v) => save({ grace_minutes: v })}
              />
            </Section>

            <Section title="O mundo sempre no ar">
              <Toggle checked={c.auto_failover} onChange={(v) => save({ auto_failover: v })} label="Assumir se o host cair" hint="Se você estiver jogando, este PC continua o mundo." />
              <Toggle checked={c.wake_on_join} onChange={(v) => save({ wake_on_join: v })} label="Acordar o mundo pelo Minecraft" hint="Entrar no servidor já abre o mundo neste PC." />
              <Toggle checked={c.background_replica} onChange={(v) => save({ background_replica: v })} label="Manter uma cópia neste PC" hint="O Jogar abre em segundos." />
              <Toggle checked={c.notify_join_leave} onChange={(v) => save({ notify_join_leave: v })} label="Avisar quando alguém entrar" />
              <TextSetting
                label="Porta no Minecraft"
                value={String(c.proxy_port)}
                hint="Troque só se outro programa usar a 25565."
                onSave={(v) => save({ proxy_port: Number(v) || 25565 })}
              />
            </Section>

            <JournalSection s={s} reload={load} isOwner={isOwner} toast={toast} />

            <Section title="Armazenamento">
              <Row label="Dados do app" hint={s.data_dir}>
                {isOwner && (
                  <Button
                    icon={<Trash2 className="size-4" />}
                    busy={busy === "gc"}
                    disabled={!free}
                    onClick={async () => {
                      const r = await run("gc", () => api.gc(false));
                      if (r) toast({ level: "success", title: "Espaço liberado", body: `${r.manifests_deleted} versões antigas e ${bytes(r.bytes_freed)} removidos da nuvem.` });
                    }}
                  >
                    Liberar espaço na nuvem
                  </Button>
                )}
              </Row>
            </Section>
          </>
        )}
      </div>
    </div>
  );
}

function Section({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <Card>
      <CardTitle aside={aside}>{title}</CardTitle>
      <div className="px-5 pb-2 divide-y divide-white/[0.05]">{children}</div>
    </Card>
  );
}

/** Uma linha de configuração: nome e explicação curta à esquerda, controle à direita. */
function Row({ label, hint, children, below }: { label: string; hint?: ReactNode; children?: ReactNode; below?: ReactNode }) {
  return (
    <div className="py-3">
      <div className="flex items-center justify-between gap-6">
        <div className="min-w-0">
          <div className="text-ink font-medium">{label}</div>
          {hint && <div className="text-[12.5px] text-ink-muted mt-0.5 truncate">{hint}</div>}
        </div>
        <div className="shrink-0">{children}</div>
      </div>
      {below}
    </div>
  );
}

function TextSetting({ label, value, hint, onSave }: { label: string; value: string; hint?: string; onSave: (v: string) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <Row label={label} hint={hint}>
      <div className="flex gap-2 items-center">
        {v !== value && (
          <Button variant="primary" className="h-9" onClick={() => onSave(v.trim())}>
            Salvar
          </Button>
        )}
        <input className={cx(inputCls, "w-[110px] text-center")} value={v} onChange={(e) => setV(e.target.value)} aria-label={label} />
      </div>
    </Row>
  );
}

const gb = (mb: number) => `${(mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1).replace(".", ",")} GB`;

/** Memória: "Automático (5 GB)" ou "Personalizado" com controle deslizante e a faixa recomendada. */
function MemorySetting({
  label,
  value,
  auto,
  total,
  recommended,
  onSave,
}: {
  label: string;
  value: number;
  auto: number;
  total: number;
  recommended: [number, number];
  onSave: (mb: number) => void;
}) {
  const max = Math.max(2048, Math.floor((total - 2048) / 512) * 512);
  const [custom, setCustom] = useState(value > 0);
  const [v, setV] = useState(value > 0 ? value : Math.min(auto, max));
  useEffect(() => {
    setCustom(value > 0);
    setV(value > 0 ? value : Math.min(auto, max));
  }, [value, auto, max]);
  return (
    <Row
      label={label}
      hint={custom ? `Recomendado: ${gb(recommended[0])} a ${gb(recommended[1])}.` : `De ${gb(total)} neste PC.`}
      below={
        custom && (
          <div className="mt-2.5 flex items-center gap-3">
            <input type="range" min={2048} max={max} step={512} value={v} onChange={(e) => setV(Number(e.target.value))} className="flex-1" aria-label={label} />
            <span className="w-14 text-right tabular-nums text-[13px] font-semibold">{gb(v)}</span>
            {v !== value && (
              <Button variant="primary" className="h-8" onClick={() => onSave(v)}>
                Salvar
              </Button>
            )}
          </div>
        )
      }
    >
      <select
        className={cx(inputCls, "w-[200px]")}
        value={custom ? "custom" : "auto"}
        aria-label={label}
        onChange={(e) => {
          if (e.target.value === "auto") {
            setCustom(false);
            onSave(0);
          } else setCustom(true);
        }}
      >
        <option value="auto">Automático ({gb(auto)})</option>
        <option value="custom">Personalizado</option>
      </select>
    </Row>
  );
}

/** Opções prontas (ex.: 5 / 10 / 15 min). */
function ChoiceSetting({ label, value, options, unit, hint, onSave }: { label: string; value: number; options: number[]; unit: string; hint?: string; onSave: (v: number) => void }) {
  const all = options.includes(value) ? options : [...options, value].sort((a, b) => a - b);
  return (
    <Row label={label} hint={hint}>
      <select className={cx(inputCls, "w-[120px]")} value={value} aria-label={label} onChange={(e) => onSave(Number(e.target.value))}>
        {all.map((o) => (
          <option key={o} value={o}>
            {o} {unit}
          </option>
        ))}
      </select>
    </Row>
  );
}

function PathSetting({ value, directory, onSave }: { value: string; directory?: boolean; onSave: (v: string) => void }) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-[12.5px] text-ink-muted max-w-[260px] truncate" title={value}>
        {value || "—"}
      </span>
      <Button
        icon={<FolderOpen className="size-4" />}
        onClick={async () => {
          const f = await open({ directory, defaultPath: value || undefined });
          if (typeof f === "string") onSave(f);
        }}
      >
        Trocar
      </Button>
    </div>
  );
}

function JournalSection({ s, reload, isOwner, toast }: { s: SettingsView; reload: () => void; isOwner: boolean; toast: ToastFn }) {
  const [js, setJs] = useState<JournalStatus | null>(null);
  const [webhook, setWebhook] = useState("");
  const [saved, setSaved] = useState("");
  const { busy, error, setError, run } = useAction();
  useEffect(() => {
    api.journalStatus().then(setJs).catch(() => {});
  }, [s]);
  useEffect(() => {
    if (isOwner)
      api
        .getPrivate()
        .then((p) => {
          setWebhook(p.discord_webhook ?? "");
          setSaved(p.discord_webhook ?? "");
        })
        .catch(() => {});
  }, [isOwner]);
  const cats = s.journal_categories;
  const setCat = (k: Category, on: boolean) => {
    const next = cats.map(([c, , v]) => [c, c === k ? on : v] as [Category, boolean]);
    run("cat", () => api.saveSettings({ journal: next })).then(reload);
  };
  return (
    <Section
      title="Discord"
      aside={js && (js.pending > 0 || js.last_error || js.last_sent_at) ? <Pill tone={js.last_error ? "sun" : "moss"}>{js.last_error ? "com problema" : js.pending ? `${js.pending} na fila` : "em dia"}</Pill> : undefined}
    >
      {(error || js?.last_error) && (
        <div className="py-3 space-y-2">
          <ErrorBanner error={error} onClose={() => setError(null)} />
          {js?.last_error && <InfoNote tone="warn">{js.last_error}</InfoNote>}
        </div>
      )}
      {isOwner ? (
        <Row
          label="Canal do diário"
          hint="Cole o webhook do canal (Editar canal → Integrações → Webhooks)."
          below={
            <div className="mt-2.5 flex gap-2">
              <input className={inputCls} value={webhook} placeholder="https://discord.com/api/webhooks/…" aria-label="Webhook do Discord" onChange={(e) => setWebhook(e.target.value)} />
              {webhook !== saved ? (
                <Button
                  variant="primary"
                  busy={busy === "wh"}
                  onClick={() =>
                    run("wh", async () => {
                      await api.setWebhook(webhook);
                      setSaved(webhook);
                      toast({ level: "success", title: "Discord configurado", body: "" });
                    })
                  }
                >
                  Salvar
                </Button>
              ) : (
                <Button
                  disabled={!webhook}
                  busy={busy === "test"}
                  onClick={() => run("test", async () => (await api.testWebhook(webhook), toast({ level: "success", title: "Mensagem de teste enviada", body: "" })))}
                >
                  Testar
                </Button>
              )}
            </div>
          }
        />
      ) : (
        <Row label="Canal do diário" hint="Configurado pelo dono do mundo." />
      )}
      <div className="py-1 grid grid-cols-2 gap-x-8">
        {cats.map(([k, label, on]) => (
          <Toggle key={k} checked={on} onChange={(v) => setCat(k, v)} label={label} />
        ))}
      </div>
    </Section>
  );
}

function WorldSection({ free, toast }: { free: boolean; toast: ToastFn }) {
  const [w, setW] = useState<WorldSettings | null>(null);
  const [dirty, setDirty] = useState(false);
  const [extra, setExtra] = useState(false);
  const { busy, error, setError, run } = useAction();
  useEffect(() => {
    api.worldSettings().then(setW).catch(() => {});
  }, []);
  if (!w) return null;
  const set = <K extends keyof WorldSettings>(k: K, v: WorldSettings[K]) => {
    setW({ ...w, [k]: v });
    setDirty(true);
  };
  const num = (k: "view_distance" | "simulation_distance" | "max_players", label: string, min: number, max: number) => (
    <Row label={label}>
      <input className={cx(inputCls, "w-[110px] text-center")} type="number" min={min} max={max} value={w[k]} disabled={!free} aria-label={label} onChange={(e) => set(k, Number(e.target.value))} />
    </Row>
  );
  return (
    <Section
      title="Mundo"
      aside={
        dirty ? (
          <Button
            variant="primary"
            busy={busy === "w"}
            disabled={!free}
            onClick={() =>
              run("w", () => api.saveWorldSettings(w)).then((r) => {
                if (r !== undefined) {
                  setDirty(false);
                  toast({ level: "success", title: "Configurações do mundo salvas", body: "Valem a partir da próxima sessão, em qualquer PC." });
                }
              })
            }
          >
            Salvar
          </Button>
        ) : !free ? (
          <span className="text-[12px] text-ink-muted">Só muda com ninguém hospedando</span>
        ) : undefined
      }
    >
      {error && (
        <div className="py-3">
          <ErrorBanner error={error} onClose={() => setError(null)} />
        </div>
      )}
      <Row label="Dificuldade">
        <select className={cx(inputCls, "w-[200px]")} value={w.difficulty} disabled={!free} aria-label="Dificuldade" onChange={(e) => set("difficulty", e.target.value as WorldSettings["difficulty"])}>
          <option value="peaceful">Pacífico</option>
          <option value="easy">Fácil</option>
          <option value="normal">Normal</option>
          <option value="hard">Difícil</option>
        </select>
      </Row>
      <Row label="Modo de jogo">
        <select className={cx(inputCls, "w-[200px]")} value={w.gamemode} disabled={!free} aria-label="Modo de jogo" onChange={(e) => set("gamemode", e.target.value as WorldSettings["gamemode"])}>
          <option value="survival">Sobrevivência</option>
          <option value="creative">Criativo</option>
          <option value="adventure">Aventura</option>
        </select>
      </Row>
      <Toggle checked={w.pvp} onChange={(v) => free && set("pvp", v)} label="PvP" hint="Jogadores podem se atacar." />
      {extra && (
        <>
          <Row label="Mensagem do servidor">
            <input className={cx(inputCls, "w-[260px]")} value={w.motd} disabled={!free} aria-label="Mensagem do servidor" onChange={(e) => set("motd", e.target.value)} />
          </Row>
          {num("view_distance", "Distância de visão", 3, 32)}
          {num("simulation_distance", "Distância de simulação", 3, 32)}
          {num("max_players", "Máximo de jogadores", 1, 100)}
        </>
      )}
      <div className="py-2.5">
        <button onClick={() => setExtra((x) => !x)} className="text-[12.5px] font-semibold text-ink-muted hover:text-ink cursor-pointer">
          {extra ? "Menos ajustes do mundo" : "Mais ajustes do mundo"}
        </button>
      </div>
    </Section>
  );
}
