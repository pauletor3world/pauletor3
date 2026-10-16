import { ArrowRightLeft, Award, DoorOpen, History, LogIn, LogOut, RotateCcw, Save, Skull, Sparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Button, Card, Empty, ErrorBanner, InfoNote, Modal, PageHeader, Pill, cx } from "../components/ui";
import { api } from "../lib/api";
import { bytes, clock, dateLong, dateTime, duration, kindLabel } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { DiaryEvent, ManifestKind, ManifestSummary, Status } from "../lib/types";

const tone: Record<ManifestKind, "moss" | "muted" | "ember" | "dusk" | "sky" | "sun"> = {
  session: "moss",
  checkpoint: "muted",
  recovered: "ember",
  restore: "dusk",
  import: "sky",
  edit: "sun",
};

export default function Timeline({ status, toast }: { status: Status | null; toast: (t: Omit<Toast, "id" | "type">) => void }) {
  const [items, setItems] = useState<ManifestSummary[] | null>(null);
  const [showCheckpoints, setShowCheckpoints] = useState(false);
  const [tab, setTab] = useState<"diary" | "versions">("diary");
  const [target, setTarget] = useState<ManifestSummary | null>(null);
  const { busy, error, setError, run } = useAction();

  const load = useCallback(() => {
    run("load", api.timeline).then((t) => t && setItems(t));
  }, [run]);
  useEffect(load, [load]);

  const free = status?.lease?.kind === "free" && !status.me_hosting;
  const headId = status?.head?.id;

  const groups = useMemo(() => {
    const list = (items ?? []).filter((m) => showCheckpoints || m.kind !== "checkpoint" || m.id === headId);
    const map = new Map<string, ManifestSummary[]>();
    for (const m of list) {
      const k = new Date(m.created_at).toDateString();
      map.set(k, [...(map.get(k) ?? []), m]);
    }
    return [...map.entries()];
  }, [items, showCheckpoints, headId]);

  const doRestore = async () => {
    const t = target;
    setTarget(null);
    if (!t) return;
    const ok = await run("restore", () => api.restore(t.id));
    if (ok !== undefined) {
      toast({ level: "success", title: "Versão restaurada", body: `O mundo voltou para ${dateTime(t.created_at)}.` });
      load();
    }
  };

  return (
    <div>
      <PageHeader
        title="Diário do mundo"
        subtitle="A história da turma e cada versão salva. Qualquer versão pode ser restaurada — nada é apagado."
        aside={
          tab === "versions" && <label className="flex items-center gap-2 text-[12.5px] text-ink-muted cursor-pointer">
            <input type="checkbox" checked={showCheckpoints} onChange={(e) => setShowCheckpoints(e.target.checked)} className="accent-[var(--color-moss-500)]" />
            Mostrar salvamentos automáticos
          </label>
        }
      />
      <div className="flex gap-1 mb-4 p-1 rounded-xl bg-night-950/40 ring-1 ring-white/5 w-fit">
        {(["diary", "versions"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={cx("h-8 px-4 rounded-lg text-[13px] font-medium cursor-pointer transition-colors", tab === t ? "bg-white/[0.08] text-ink" : "text-ink-muted hover:text-ink")}
          >
            {t === "diary" ? "Diário" : "Versões salvas"}
          </button>
        ))}
      </div>
      {tab === "diary" && <DiaryFeed />}
      {tab === "versions" && (
      <div className="space-y-4">
        <ErrorBanner error={error} onClose={() => setError(null)} />
        {!free && items && items.length > 0 && <InfoNote>Para restaurar uma versão, ninguém pode estar hospedando.</InfoNote>}
        {items === null && !error && (
          <div className="space-y-3">
            {[0, 1, 2].map((i) => (
              <div key={i} className="h-20 rounded-2xl bg-night-850/60 animate-pulse" />
            ))}
          </div>
        )}
        {items && items.length === 0 && (
          <Card>
            <Empty icon={<History className="size-6" />} title="Nenhuma versão ainda">
              A primeira aparece aqui assim que alguém jogar e o mundo for salvo.
            </Empty>
          </Card>
        )}
        {groups.map(([day, list]) => (
          <section key={day} className="animate-rise">
            <h2 className="text-[12px] uppercase tracking-wider text-ink-faint font-semibold mb-2 ml-1">{dateLong(list[0].created_at)}</h2>
            <Card className="divide-y divide-white/[0.05]">
              {list.map((m) => (
                <div key={m.id} className="flex items-center gap-4 px-5 py-3.5">
                  <div className="w-12 text-[12.5px] text-ink-muted tabular-nums">{clock(m.created_at)}</div>
                  <Avatar name={m.host_player} size={34} />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="font-semibold">{m.host_player}</span>
                      <Pill tone={tone[m.kind]}>{kindLabel[m.kind]}</Pill>
                      {m.id === headId && <Pill tone="moss">Versão atual</Pill>}
                    </div>
                    <div className="text-[12.5px] text-ink-muted mt-0.5 truncate">
                      {m.kind === "session" || m.kind === "checkpoint" ? `${duration(m.duration_secs)} · ` : ""}
                      {m.players.length ? `jogaram: ${m.players.join(", ")} · ` : ""}
                      {bytes(m.total_size)}
                      {m.note ? ` · ${m.note}` : ""}
                    </div>
                  </div>
                  {m.id !== headId && (
                    <Button variant="ghost" icon={<RotateCcw className="size-4" />} disabled={!free} busy={busy === "restore" && target?.id === m.id} onClick={() => setTarget(m)}>
                      Restaurar
                    </Button>
                  )}
                </div>
              ))}
            </Card>
          </section>
        ))}
      </div>
      )}
      <Modal
        open={!!target}
        title="Restaurar esta versão?"
        onClose={() => setTarget(null)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setTarget(null)}>
              Cancelar
            </Button>
            <Button variant="sun" icon={<RotateCcw className="size-4" />} onClick={doRestore}>
              Restaurar
            </Button>
          </>
        }
      >
        {target && (
          <div className="space-y-3 text-[13.5px]">
            <p>
              O mundo volta a ficar como estava em <b className="text-ink">{dateTime(target.created_at)}</b>, na sessão de <b className="text-ink">{target.host_player}</b>.
            </p>
            <p>
              Tudo o que aconteceu depois <b className="text-ink">continua guardado</b> na linha do tempo: a restauração cria uma versão nova e você pode desfazer restaurando a versão atual.
            </p>
            <p className="text-ink-muted">O diário no Discord registra quem restaurou e para qual sessão.</p>
          </div>
        )}
      </Modal>
    </div>
  );
}

const diaryIcon: Record<string, React.ReactNode> = {
  opened: <DoorOpen className="size-4 text-moss-300" />,
  joined: <LogIn className="size-4 text-sky-400" />,
  left: <LogOut className="size-4 text-ink-muted" />,
  death: <Skull className="size-4 text-ember-400" />,
  advancement: <Award className="size-4 text-sun-400" />,
  host_changed: <ArrowRightLeft className="size-4 text-sun-400" />,
  handover: <ArrowRightLeft className="size-4 text-sky-400" />,
  saved: <Save className="size-4 text-moss-300" />,
  restored: <RotateCcw className="size-4 text-dusk-400" />,
};

function DiaryFeed() {
  const [events, setEvents] = useState<DiaryEvent[] | null>(null);
  const { error, setError, run } = useAction();
  useEffect(() => {
    run("diary", () => api.diary(300)).then((e) => e && setEvents(e));
  }, [run]);
  const days = useMemo(() => {
    const m = new Map<string, DiaryEvent[]>();
    for (const e of events ?? []) {
      const k = new Date(e.at).toDateString();
      m.set(k, [...(m.get(k) ?? []), e]);
    }
    return [...m.entries()];
  }, [events]);
  return (
    <div className="space-y-4">
      <ErrorBanner error={error} onClose={() => setError(null)} />
      {events === null && !error && <div className="h-40 rounded-2xl bg-night-850/60 animate-pulse" />}
      {events && events.length === 0 && (
        <Card>
          <Empty icon={<Sparkles className="size-6" />} title="A história começa na primeira sessão">
            Quem entrou, mortes, conquistas e trocas de host aparecem aqui sozinhos.
          </Empty>
        </Card>
      )}
      {days.map(([day, list]) => (
        <Card key={day} className="animate-rise">
          <div className="px-5 pt-4 pb-2 text-[12px] uppercase tracking-wider text-ink-faint">{dateLong(list[0].at)}</div>
          <ul className="px-5 pb-4">
            {list.map((e, i) => (
              <li key={i} className="flex items-start gap-3 py-1.5">
                <span className="mt-0.5 shrink-0">{diaryIcon[e.kind] ?? <Sparkles className="size-4 text-ink-muted" />}</span>
                <span className="flex-1 text-[13.5px] text-ink-soft">{e.text}</span>
                <span className="text-[11.5px] text-ink-faint tabular-nums">{clock(e.at)}</span>
              </li>
            ))}
          </ul>
        </Card>
      ))}
    </div>
  );
}
