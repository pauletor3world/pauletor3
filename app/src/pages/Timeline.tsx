import { ArrowRightLeft, Award, ChevronDown, ChevronRight, Copy, DoorOpen, History, LogIn, LogOut, RotateCcw, Save, Skull, Sparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Button, Card, Empty, ErrorBanner, InfoNote, Modal, PageHeader, Pill, cx, inputCls } from "../components/ui";
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

  const doCopy = async () => {
    const t = target;
    setTarget(null);
    if (!t) return;
    const name = await run("copy", () => api.restoreCopy(t.id));
    if (name) toast({ level: "success", title: "Cópia pronta no single-player", body: `Abra o Minecraft → Um jogador → "${name}". O mundo do grupo não mudou.` });
  };

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
        subtitle="A história do grupo e cada versão salva. Qualquer versão pode ser restaurada — nada é apagado."
        aside={
          tab === "versions" && <label className="flex items-center gap-2 text-[12.5px] text-ink-muted cursor-pointer">
            <input type="checkbox" checked={showCheckpoints} onChange={(e) => setShowCheckpoints(e.target.checked)} />
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
          <section key={day}>
            <h2 className="text-[13px] font-bold text-ink-muted mb-2.5 ml-1 first-letter:uppercase">{dateLong(list[0].created_at)}</h2>
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
                    <Button variant="ghost" icon={<RotateCcw className="size-4" />} busy={(busy === "restore" || busy === "copy") && target?.id === m.id} onClick={() => setTarget(m)}>
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
            <Button icon={<Copy className="size-4" />} onClick={doCopy} title="Vira um mundo de single-player; o mundo do grupo não muda">
              Restaurar como cópia
            </Button>
            <Button variant="sun" icon={<RotateCcw className="size-4" />} disabled={!free} onClick={doRestore}>
              Restaurar
            </Button>
          </>
        }
      >
        {target && (
          <div className="space-y-3 text-[13.5px]">
            <div className="rounded-xl bg-night-950/50 ring-1 ring-inset ring-white/[0.06] p-3.5 grid grid-cols-2 gap-y-1.5 gap-x-4 text-[13px]">
              <span className="text-ink-muted">Quando</span>
              <span className="text-ink font-semibold">{dateTime(target.created_at)}</span>
              <span className="text-ink-muted">Quem hospedava</span>
              <span className="text-ink font-semibold">{target.host_player}</span>
              <span className="text-ink-muted">Quem jogou</span>
              <span className="text-ink">{target.players.length ? target.players.join(", ") : "—"}</span>
              <span className="text-ink-muted">Tamanho</span>
              <span className="text-ink">{bytes(target.total_size)}</span>
            </div>
            <p>
              <b className="text-ink">Restaurar</b>: o mundo do grupo volta a ficar exatamente como estava nesse momento.{" "}
              <b className="text-ink">Restaurar como cópia</b>: vira um mundo de single-player para você testar, sem mexer no do grupo.
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

/** Destaques humanos ficam em evidência; salvar/entrar/sair vão para segundo plano. */
const HIGHLIGHT = new Set(["death", "advancement", "host_changed", "restored", "first", "milestone"]);
const TYPES: { id: string; label: string; match: (k: string) => boolean }[] = [
  { id: "all", label: "Tudo", match: () => true },
  { id: "highlights", label: "Destaques", match: (k) => HIGHLIGHT.has(k) },
  { id: "deaths", label: "Mortes", match: (k) => k === "death" },
  { id: "adv", label: "Conquistas", match: (k) => k === "advancement" },
  { id: "hosts", label: "Trocas de host", match: (k) => k === "host_changed" || k === "handover" },
  { id: "comings", label: "Entradas e saídas", match: (k) => k === "joined" || k === "left" || k === "opened" },
];

interface DiarySession {
  start: string;
  end: string;
  events: DiaryEvent[];
  players: string[];
  hostChanges: number;
}

/** Agrupa o diário por sessão: começa ao abrir o mundo (ou depois de 30 min sem nada). */
function sessions(events: DiaryEvent[]): DiarySession[] {
  const asc = [...events].sort((a, b) => a.at.localeCompare(b.at));
  const out: DiarySession[] = [];
  let cur: DiarySession | null = null;
  for (const e of asc) {
    const gap = cur ? new Date(e.at).getTime() - new Date(cur.end).getTime() : Infinity;
    if (!cur || e.kind === "opened" || gap > 30 * 60_000) {
      cur = { start: e.at, end: e.at, events: [], players: [], hostChanges: 0 };
      out.push(cur);
    }
    cur.events.push(e);
    cur.end = e.at;
    if (e.player && !cur.players.includes(e.player)) cur.players.push(e.player);
    if (e.kind === "host_changed") cur.hostChanges++;
  }
  return out.reverse();
}

function DiaryFeed() {
  const [events, setEvents] = useState<DiaryEvent[] | null>(null);
  const [who, setWho] = useState("");
  const [type, setType] = useState("all");
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const { error, setError, run } = useAction();
  useEffect(() => {
    run("diary", () => api.diary(300)).then((e) => e && setEvents(e));
  }, [run]);
  const people = useMemo(() => [...new Set((events ?? []).map((e) => e.player).filter((p): p is string => !!p))].sort(), [events]);
  const list = useMemo(() => {
    const t = TYPES.find((x) => x.id === type) ?? TYPES[0];
    return sessions(events ?? [])
      .map((s) => ({ ...s, shown: s.events.filter((e) => t.match(e.kind) && (!who || e.player === who)) }))
      .filter((s) => s.shown.length > 0);
  }, [events, who, type]);
  return (
    <div className="space-y-4">
      <ErrorBanner error={error} onClose={() => setError(null)} />
      {events && events.length > 0 && (
        <div className="flex gap-2 flex-wrap">
          <select className={cx(inputCls, "max-w-[220px]")} value={who} onChange={(e) => setWho(e.target.value)} aria-label="Filtrar por pessoa">
            <option value="">Todo mundo</option>
            {people.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
          <select className={cx(inputCls, "max-w-[220px]")} value={type} onChange={(e) => setType(e.target.value)} aria-label="Filtrar por tipo">
            {TYPES.map((t) => (
              <option key={t.id} value={t.id}>
                {t.label}
              </option>
            ))}
          </select>
        </div>
      )}
      {events === null && !error && <div className="h-40 rounded-2xl bg-night-850/60 animate-pulse" />}
      {events && events.length === 0 && (
        <Card>
          <Empty icon={<Sparkles className="size-6" />} title="A história começa na primeira sessão">
            Quem entrou, mortes, conquistas e trocas de host aparecem aqui sozinhos.
          </Empty>
        </Card>
      )}
      {events && events.length > 0 && list.length === 0 && (
        <Card>
          <Empty icon={<Sparkles className="size-6" />} title="Nada com esse filtro" />
        </Card>
      )}
      {list.map((s) => {
        const mins = (new Date(s.end).getTime() - new Date(s.start).getTime()) / 1000;
        const highlights = s.shown.filter((e) => HIGHLIGHT.has(e.kind));
        const expanded = !!open[s.start];
        return (
          <Card key={s.start}>
            <button
              onClick={() => setOpen((o) => ({ ...o, [s.start]: !o[s.start] }))}
              aria-expanded={expanded}
              className="w-full text-left px-5 pt-4 pb-3 flex items-center gap-3 cursor-pointer"
            >
              {expanded ? <ChevronDown className="size-4 text-ink-faint shrink-0" /> : <ChevronRight className="size-4 text-ink-faint shrink-0" />}
              <div className="min-w-0 flex-1">
                <div className="font-bold text-[14px] first-letter:uppercase">
                  {dateLong(s.start)} · {clock(s.start)} às {clock(s.end)}
                </div>
                <div className="text-[12.5px] text-ink-muted">
                  {s.players.length ? s.players.join(" e ") : "Ninguém"}
                  {mins >= 60 && ` · ${duration(mins)}`}
                  {s.hostChanges > 0 && ` · ${s.hostChanges} ${s.hostChanges === 1 ? "troca" : "trocas"} de host`}
                </div>
              </div>
            </button>
            <ul className="px-5 pb-4">
              {(expanded ? s.shown : highlights.slice(0, 4)).map((e, i) => (
                <li key={i} className={cx("flex items-start gap-3 py-1.5", !HIGHLIGHT.has(e.kind) && "opacity-70")}>
                  <span className="mt-0.5 shrink-0">{diaryIcon[e.kind] ?? <Sparkles className="size-4 text-ink-muted" />}</span>
                  <span className={cx("flex-1 text-[13.5px]", HIGHLIGHT.has(e.kind) ? "text-ink" : "text-ink-soft")}>{e.text}</span>
                  <span className="text-[11.5px] text-ink-faint tabular-nums">{clock(e.at)}</span>
                </li>
              ))}
              {!expanded && highlights.length === 0 && <li className="text-[12.5px] text-ink-faint py-1">Sessão tranquila. Abra para ver tudo.</li>}
            </ul>
          </Card>
        );
      })}
    </div>
  );
}
