import { BarChart3, Flame, Moon, Skull, Timer, Trophy } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Card, CardTitle, Empty, ErrorBanner, PageHeader, Stat } from "../components/ui";
import { api } from "../lib/api";
import { duration, hours, hue } from "../lib/format";
import { useAction } from "../lib/hooks";
import type { Stats } from "../lib/types";

export default function StatsPage() {
  const [stats, setStats] = useState<Stats | null>(null);
  const { error, setError, run } = useAction();
  useEffect(() => {
    run("load", api.stats).then((s) => s && setStats(s));
  }, [run]);

  const players = useMemo(
    () =>
      Object.entries(stats?.players ?? {})
        .map(([name, p]) => ({ name, ...p }))
        .sort((a, b) => b.seconds - a.seconds),
    [stats],
  );
  const totalSecs = players.reduce((a, p) => a + p.seconds, 0);
  const topHost = Object.entries(stats?.hosts ?? {}).sort((a, b) => b[1] - a[1])[0];
  const deaths = useMemo(() => {
    const m: Record<string, number> = {};
    for (const d of Object.values(stats?.daily ?? {})) for (const [p, n] of Object.entries(d.deaths)) m[p] = (m[p] ?? 0) + n;
    return Object.entries(m).sort((a, b) => b[1] - a[1]);
  }, [stats]);

  // Últimos 70 dias para o mapa de calor.
  const heat = useMemo(() => {
    const days = new Set(stats?.days ?? []);
    const out: { date: string; on: boolean; secs: number }[] = [];
    const today = new Date();
    for (let i = 69; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(today.getDate() - i);
      const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      const daily = stats?.daily[key];
      const secs = daily ? Object.values(daily.seconds).reduce((a, b) => a + b, 0) : 0;
      out.push({ date: key, on: days.has(key), secs });
    }
    return out;
  }, [stats]);
  const maxHeat = Math.max(1, ...heat.map((h) => h.secs));

  return (
    <div>
      <PageHeader title="Estatísticas" subtitle="A história do mundo em números." />
      <ErrorBanner error={error} onClose={() => setError(null)} />
      {stats && players.length === 0 && stats.sessions === 0 ? (
        <Card>
          <Empty icon={<BarChart3 className="size-6" />} title="Ainda sem números">
            As estatísticas começam a contar a partir da primeira sessão pelo app.
          </Empty>
        </Card>
      ) : (
        stats && (
          <div className="space-y-5">
            <Card className="grid grid-cols-4 px-6 py-5">
              <Stat label="Tempo jogado" value={hours(totalSecs)} />
              <Stat label="Sessões" value={String(stats.sessions)} className="pl-6 border-l border-white/[0.06]" />
              <Stat label="Dias jogados" value={String(stats.days.length)} className="pl-6 border-l border-white/[0.06]" />
              <div className="pl-6 border-l border-white/[0.06] min-w-0">
                <Stat label="Quem mais hospedou" value={topHost ? topHost[0] : "—"} />
                {topHost && <div className="text-[12.5px] text-ink-faint mt-1">{topHost[1]} sessões</div>}
              </div>
            </Card>

            <div className="grid grid-cols-5 gap-5">
              <Card className="col-span-3">
                <CardTitle>Tempo jogado por pessoa</CardTitle>
                <ul className="px-5 pb-5 space-y-3">
                  {players.map((p) => (
                    <li key={p.name} className="flex items-center gap-3">
                      <Avatar name={p.name} size={30} />
                      <div className="flex-1 min-w-0">
                        <div className="flex justify-between text-[13px] mb-1">
                          <span className="font-semibold">{p.name}</span>
                          <span className="text-ink-muted tabular-nums">
                            {hours(p.seconds)} · {p.days.length} dia{p.days.length === 1 ? "" : "s"}
                          </span>
                        </div>
                        <div className="h-2 rounded-full bg-night-950/70 overflow-hidden">
                          <div
                            className="h-full rounded-full transition-[width] duration-700"
                            style={{ width: `${(p.seconds / Math.max(1, players[0].seconds)) * 100}%`, background: `hsl(${hue(p.name)} 60% 58%)` }}
                          />
                        </div>
                      </div>
                    </li>
                  ))}
                </ul>
              </Card>

              <Card className="col-span-2">
                <CardTitle>Marcos do mundo</CardTitle>
                <ul className="px-5 pb-5 space-y-2.5 text-[13px]">
                  <Milestone icon={<Flame className="size-4 text-ember-400" />} label="Primeira ida ao Nether" value={stats.firsts.nether} />
                  <Milestone icon={<Moon className="size-4 text-dusk-400" />} label="Primeira ida ao End" value={stats.firsts.end} />
                  <Milestone icon={<Trophy className="size-4 text-sun-400" />} label="Dragão derrotado" value={stats.firsts.dragon} />
                  <li className="flex items-center gap-2.5">
                    <Timer className="size-4 text-moss-400" />
                    <span className="text-ink-muted flex-1">Recorde de tempo seguido</span>
                    <span className="font-semibold">{stats.record ? `${stats.record[0]} · ${duration(stats.record[1])}` : "—"}</span>
                  </li>
                  <li className="flex items-center gap-2.5">
                    <Skull className="size-4 text-ink-muted" />
                    <span className="text-ink-muted flex-1">Mais mortes (60 dias)</span>
                    <span className="font-semibold">{deaths[0] ? `${deaths[0][0]} · ${deaths[0][1]}` : "ninguém morreu"}</span>
                  </li>
                </ul>
              </Card>
            </div>

            <Card>
              <CardTitle aside={<span className="text-[12px] text-ink-muted">últimos 70 dias</span>}>Dias jogados</CardTitle>
              <div className="px-5 pb-5">
                <div className="grid grid-flow-col grid-rows-7 gap-1.5 w-fit">
                  {heat.map((h) => (
                    <div
                      key={h.date}
                      title={`${new Date(h.date + "T12:00:00").toLocaleDateString("pt-BR")}${h.on ? ` · ${hours(h.secs)}` : ""}`}
                      className="size-3.5 rounded-[2px]"
                      style={{
                        background: h.on ? `rgb(138 203 92 / ${0.25 + 0.75 * (h.secs / maxHeat)})` : "rgb(255 255 255 / 0.04)",
                      }}
                    />
                  ))}
                </div>
              </div>
            </Card>
          </div>
        )
      )}
    </div>
  );
}

function Milestone({ icon, label, value }: { icon: React.ReactNode; label: string; value: [string, string] | null }) {
  return (
    <li className="flex items-center gap-2.5">
      {icon}
      <span className="text-ink-muted flex-1">{label}</span>
      <span className="font-semibold">{value ? `${value[0]} · ${new Date(value[1]).toLocaleDateString("pt-BR")}` : "ainda não"}</span>
    </li>
  );
}
