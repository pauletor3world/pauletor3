import { History, RotateCcw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Button, Card, Empty, ErrorBanner, InfoNote, Modal, PageHeader, Pill } from "../components/ui";
import { api } from "../lib/api";
import { bytes, clock, dateLong, dateTime, duration, kindLabel } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { ManifestKind, ManifestSummary, Status } from "../lib/types";

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
  const [target, setTarget] = useState<ManifestSummary | null>(null);
  const { busy, error, setError, run } = useAction();

  const load = useCallback(() => {
    run("load", api.timeline).then((t) => t && setItems(t));
  }, [run]);
  useEffect(load, [load]);

  const free = status?.lock?.kind === "free" && !status.me_hosting;
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
        title="Linha do tempo"
        subtitle="Cada sessão vira uma versão. Qualquer uma pode ser restaurada — nada é apagado."
        aside={
          <label className="flex items-center gap-2 text-[12.5px] text-ink-muted cursor-pointer">
            <input type="checkbox" checked={showCheckpoints} onChange={(e) => setShowCheckpoints(e.target.checked)} className="accent-[var(--color-moss-500)]" />
            Mostrar salvamentos automáticos
          </label>
        }
      />
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
