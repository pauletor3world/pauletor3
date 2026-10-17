import { disable as autostartOff, enable as autostartOn, isEnabled as autostartIs } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Button, Card, CardTitle, ErrorBanner, Field, InfoNote, PageHeader, Pill, Toggle, cx, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { bytes } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { Category, JournalStatus, LauncherCandidate, SettingsView, Status, WorldSettings } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

export default function SettingsPage({ status, isOwner, toast }: { status: Status | null; isOwner: boolean; toast: ToastFn }) {
  const [s, setS] = useState<SettingsView | null>(null);
  const [launchers, setLaunchers] = useState<LauncherCandidate[]>([]);
  const [auto, setAuto] = useState(false);
  const { busy, error, setError, run } = useAction();

  const load = useCallback(() => {
    run("load", api.settings).then((v) => v && setS(v));
  }, [run]);
  useEffect(() => {
    load();
    api.detectLaunchers().then(setLaunchers).catch(() => {});
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
      <PageHeader title="Configurações" subtitle={`Dados do app em ${s.data_dir}`} />
      <div className="space-y-5">
        <ErrorBanner error={error} onClose={() => setError(null)} />

        <Section title="Você e o jogo">
          <div className="grid grid-cols-2 gap-4">
            <TextSetting label="Seu nick no Minecraft" value={c.player_name} hint={c.direct_launch && s.direct_launch_available ? "É o nome que aparece no jogo. Não muda com o mundo aberto." : "Igual ao do launcher. Não muda com o mundo aberto."} onSave={(v) => save({ player_name: v })} />
            <PathSetting label="Pasta .minecraft" value={c.minecraft_dir ?? ""} directory onSave={(v) => save({ minecraft_dir: v })} />
          </div>
          {s.direct_launch_available && (
            <div className="mt-4">
              <Toggle
                checked={c.direct_launch}
                onChange={(v) => save({ direct_launch: v }, v ? "O Jogar abre o Minecraft direto" : "O Jogar abre o launcher")}
                label="Abrir o Minecraft direto pelo app"
                hint="Jogar já abre o jogo dentro do mundo, sem launcher e sem procurar o servidor. O app baixa o Minecraft oficial uma vez (aproveita o que já estiver na .minecraft) e confere cada arquivo."
              />
              {c.direct_launch && (
                <div className="grid grid-cols-2 gap-4 mt-3">
                  <NumberSetting
                    label="Memória do jogo (MB)"
                    value={c.client_ram_mb ?? 0}
                    hint={`0 = automático (${(s.auto_client_heap_mb / 1024).toFixed(1).replace(".", ",")} GB; menos quando você hospeda, para sobrar para o servidor)`}
                    onSave={(v) => save({ client_ram_mb: v })}
                  />
                </div>
              )}
            </div>
          )}
          <div className={cx("mt-4", s.direct_launch_available && c.direct_launch && "hidden")}>
            <Field label="Launcher" hint="Usado no botão Jogar para abrir o Minecraft.">
              <div className="flex gap-2">
                <select
                  className={inputCls}
                  value={c.launcher_path ?? ""}
                  onChange={(e) => save({ launcher_path: e.target.value }, "Launcher escolhido")}
                >
                  <option value="">— escolha —</option>
                  {launchers.map((l) => (
                    <option key={l.path} value={l.path}>
                      {l.label} — {l.path}
                    </option>
                  ))}
                  {c.launcher_path && !launchers.some((l) => l.path === c.launcher_path) && <option value={c.launcher_path}>{c.launcher_path}</option>}
                </select>
                <Button
                  icon={<FolderOpen className="size-4" />}
                  onClick={async () => {
                    const f = await open({ filters: [{ name: "Programa", extensions: ["exe"] }] });
                    if (typeof f === "string") save({ launcher_path: f }, "Launcher escolhido");
                  }}
                >
                  Procurar
                </Button>
              </div>
            </Field>
          </div>
        </Section>

        <Section title="Desempenho e salvamento">
          <div className="grid grid-cols-3 gap-4">
            <NumberSetting
              label="Memória do servidor (MB)"
              value={c.ram_mb ?? 0}
              hint={`0 = automático (${(s.auto_heap_mb / 1024).toFixed(0)} GB de ${(s.total_ram_mb / 1024).toFixed(0)} GB)`}
              onSave={(v) => save({ ram_mb: v })}
            />
            <NumberSetting label="Salvar na nuvem a cada (min)" value={c.checkpoint_minutes} hint="Numa queda de luz, perde-se no máximo isso." onSave={(v) => save({ checkpoint_minutes: v })} />
            <NumberSetting label="Tolerância ao sair (min)" value={c.grace_minutes} hint="Espera quem caiu voltar antes de fechar." onSave={(v) => save({ grace_minutes: v })} />
          </div>
        </Section>

        <Section title="Avisos">
          <Toggle checked={c.notify_toasts} onChange={(v) => save({ notify_toasts: v })} label="Notificações do Windows" hint="Quando alguém abre o mundo, quando ele é salvo, problemas." />
          <Toggle checked={c.notify_join_leave} onChange={(v) => save({ notify_join_leave: v })} label="Avisar quando alguém entrar" />
          <Toggle
            checked={auto}
            onChange={async (v) => {
              await (v ? autostartOn() : autostartOff()).catch(() => {});
              setAuto(await autostartIs().catch(() => v));
            }}
            label="Iniciar com o Windows"
            hint="Abre minimizado na bandeja, pronto para avisar e assumir."
          />
        </Section>

        <JournalSection s={s} reload={load} />

        <Section title="O mundo sempre no ar">
          <Toggle
            checked={c.auto_failover}
            onChange={(v) => save({ auto_failover: v })}
            label="Assumir sozinho se o host cair"
            hint="Se quem hospeda sair de repente e você estiver jogando, este PC continua o mundo do último salvamento."
          />
          <Toggle
            checked={c.wake_on_join}
            onChange={(v) => save({ wake_on_join: v })}
            label="Acordar o mundo pelo Minecraft"
            hint="Com o mundo livre, tentar entrar em pAuletOr 3 já abre o servidor neste PC."
          />
          <Toggle
            checked={c.background_replica}
            onChange={(v) => save({ background_replica: v })}
            label="Manter uma cópia do mundo neste PC"
            hint="Com o app aberto, a cópia se atualiza sozinha. Assim Jogar abre em segundos e o mundo nunca depende de um PC só."
          />
          <div className="grid grid-cols-2 gap-4 mt-3">
            <TextSetting
              label="Porta no Minecraft"
              value={String(c.proxy_port)}
              hint={`Endereço: ${c.proxy_port === 25565 ? "localhost" : `localhost:${c.proxy_port}`}. Troque só se outro programa usar a 25565 (reinicie o app depois).`}
              onSave={(v) => save({ proxy_port: Number(v) || 25565 })}
            />
            <TextSetting
              label="Salvamento automático (minutos)"
              value={String(c.checkpoint_minutes)}
              hint="Se o host cair, perde-se no máximo esse tempo."
              onSave={(v) => save({ checkpoint_minutes: Number(v) || 5 })}
            />
          </div>
        </Section>

        {isOwner && <WorldSection free={free} toast={toast} />}

        <Section title="Avançado">
          <div className="flex flex-wrap gap-2">
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
          </div>
          <p className="text-[12px] text-ink-muted mt-3">
            O app guarda as 20 últimas sessões e uma versão por dia dos últimos 30 dias. A limpeza também roda sozinha uma vez por dia, só com ninguém jogando.
          </p>
        </Section>
      </div>
    </div>
  );
}

function Section({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <Card>
      <CardTitle aside={aside}>{title}</CardTitle>
      <div className="px-5 pb-5">{children}</div>
    </Card>
  );
}

function TextSetting({ label, value, hint, onSave }: { label: string; value: string; hint?: string; onSave: (v: string) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <Field label={label} hint={hint}>
      <div className="flex gap-2">
        <input className={inputCls} value={v} onChange={(e) => setV(e.target.value)} />
        {v !== value && (
          <Button variant="primary" onClick={() => onSave(v.trim())}>
            Salvar
          </Button>
        )}
      </div>
    </Field>
  );
}

function NumberSetting({ label, value, hint, onSave }: { label: string; value: number; hint?: string; onSave: (v: number) => void }) {
  const [v, setV] = useState(String(value));
  useEffect(() => setV(String(value)), [value]);
  return (
    <Field label={label} hint={hint}>
      <div className="flex gap-2">
        <input className={inputCls} inputMode="numeric" value={v} onChange={(e) => setV(e.target.value.replace(/\D/g, ""))} />
        {v !== String(value) && v !== "" && (
          <Button variant="primary" onClick={() => onSave(Number(v))}>
            Salvar
          </Button>
        )}
      </div>
    </Field>
  );
}

function PathSetting({ label, value, directory, onSave }: { label: string; value: string; directory?: boolean; onSave: (v: string) => void }) {
  return (
    <Field label={label}>
      <div className="flex gap-2">
        <input className={inputCls} value={value} readOnly />
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
    </Field>
  );
}

function JournalSection({ s, reload }: { s: SettingsView; reload: () => void }) {
  const [js, setJs] = useState<JournalStatus | null>(null);
  const { error, setError, run } = useAction();
  useEffect(() => {
    api.journalStatus().then(setJs).catch(() => {});
  }, [s]);
  const cats = s.journal_categories;
  const setCat = (k: Category, on: boolean) => {
    const next = cats.map(([c, , v]) => [c, c === k ? on : v] as [Category, boolean]);
    run("cat", () => api.saveSettings({ journal: next })).then(reload);
  };
  return (
    <Section
      title="Diário do mundo (Discord)"
      aside={js && (js.pending > 0 || js.last_error || js.last_sent_at) ? <Pill tone={js.last_error ? "sun" : "moss"}>{js.last_error ? "com problema" : js.pending ? `${js.pending} na fila` : "em dia"}</Pill> : undefined}
    >
      <ErrorBanner error={error} onClose={() => setError(null)} />
      {js?.last_error && <InfoNote tone="warn">{js.last_error}</InfoNote>}
      <p className="text-[13px] text-ink-soft mb-1">O endereço do canal (webhook) é configurado pelo dono na aba Mundo e turma. Aqui você escolhe o que este PC publica quando hospeda.</p>
      <p className="text-[12px] text-ink-muted mt-2 mb-1">Só o app de quem hospeda publica os eventos da sessão, então nada sai duplicado.</p>
      <div className="grid grid-cols-2 gap-x-8">
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
  const { busy, error, setError, run } = useAction();
  useEffect(() => {
    api.worldSettings().then(setW).catch(() => {});
  }, []);
  if (!w) return null;
  const set = <K extends keyof WorldSettings>(k: K, v: WorldSettings[K]) => {
    setW({ ...w, [k]: v });
    setDirty(true);
  };
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
        ) : undefined
      }
    >
      <ErrorBanner error={error} onClose={() => setError(null)} />
      {!free && <InfoNote tone="warn">As configurações do mundo só mudam com ninguém hospedando.</InfoNote>}
      <div className="grid grid-cols-3 gap-4 mt-2">
        <Field label="Dificuldade">
          <select className={inputCls} value={w.difficulty} disabled={!free} onChange={(e) => set("difficulty", e.target.value as WorldSettings["difficulty"])}>
            <option value="peaceful">Pacífico</option>
            <option value="easy">Fácil</option>
            <option value="normal">Normal</option>
            <option value="hard">Difícil</option>
          </select>
        </Field>
        <Field label="Modo de jogo padrão">
          <select className={inputCls} value={w.gamemode} disabled={!free} onChange={(e) => set("gamemode", e.target.value as WorldSettings["gamemode"])}>
            <option value="survival">Sobrevivência</option>
            <option value="creative">Criativo</option>
            <option value="adventure">Aventura</option>
          </select>
        </Field>
        <Field label="Mensagem do servidor (MOTD)">
          <input className={inputCls} value={w.motd} disabled={!free} onChange={(e) => set("motd", e.target.value)} />
        </Field>
        <Field label="Distância de visão">
          <input className={inputCls} type="number" min={3} max={32} value={w.view_distance} disabled={!free} onChange={(e) => set("view_distance", Number(e.target.value))} />
        </Field>
        <Field label="Distância de simulação">
          <input className={inputCls} type="number" min={3} max={32} value={w.simulation_distance} disabled={!free} onChange={(e) => set("simulation_distance", Number(e.target.value))} />
        </Field>
        <Field label="Máximo de jogadores">
          <input className={inputCls} type="number" min={1} max={100} value={w.max_players} disabled={!free} onChange={(e) => set("max_players", Number(e.target.value))} />
        </Field>
      </div>
      <div className="mt-2">
        <Toggle checked={w.pvp} onChange={(v) => set("pvp", v)} label="PvP (jogadores podem se atacar)" />
      </div>
    </Section>
  );
}
