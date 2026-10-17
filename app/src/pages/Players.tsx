import { ChevronDown, ChevronRight, Link2, Sparkles, Users } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Button, Card, Empty, ErrorBanner, InfoNote, Modal, PageHeader, Pill, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { ago } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { Status, WorldPlayer } from "../lib/types";

const dim: Record<string, string> = {
  "minecraft:overworld": "Superfície",
  "minecraft:the_nether": "Nether",
  "minecraft:the_end": "End",
};

/**
 * Vincular progresso: no servidor (modo offline) cada jogador é identificado pelo nick.
 * Quem jogou em single-player ou com login tem o progresso guardado num UUID diferente —
 * aqui o dono liga esse progresso ao nick certo.
 */
export default function Players({ status, toast }: { status: Status | null; toast: (t: Omit<Toast, "id" | "type">) => void }) {
  const [players, setPlayers] = useState<WorldPlayer[] | null>(null);
  const [target, setTarget] = useState<WorldPlayer | null>(null);
  const [nick, setNick] = useState("");
  const { busy, error, setError, run } = useAction();

  const load = useCallback(() => {
    run("load", api.worldPlayers).then((p) => p && setPlayers(p));
  }, [run]);
  useEffect(load, [load]);

  const free = status?.lease?.kind === "free" && !status.me_hosting;
  const [showOrphans, setShowOrphans] = useState(false);
  const identified = (players ?? []).filter((p) => p.nick);
  const orphans = (players ?? []).filter((p) => !p.nick);
  // Sugestão de dono: mesma posição de alguém já identificado, ou o único membro sem progresso.
  const members = (status?.members ?? []).filter((m) => m.approved).map((m) => m.nick);
  const semProgresso = members.filter((n) => !identified.some((p) => p.nick?.toLowerCase() === n.toLowerCase()));
  const suggest = (p: WorldPlayer): { nick: string; why: string } | null => {
    if (p.pos) {
      const near = identified.find(
        (q) => q.pos && q.dimension === p.dimension && Math.hypot(q.pos[0] - p.pos![0], q.pos[1] - p.pos![1], q.pos[2] - p.pos![2]) < 8,
      );
      if (near?.nick) return { nick: near.nick, why: `está no mesmo lugar que ${near.nick} (${p.pos.join(", ")})` };
    }
    if (semProgresso.length === 1 && orphans.length === 1) return { nick: semProgresso[0], why: `${semProgresso[0]} é o único do grupo ainda sem progresso` };
    return null;
  };
  const sugestoes = orphans.map((p) => ({ p, s: suggest(p) })).filter((x) => x.s);

  const migrate = async () => {
    const t = target;
    if (!t) return;
    const r = await run("migrate", () => api.migratePlayer(t.uuid, nick.trim()));
    if (r !== undefined) {
      toast({ level: "success", title: "Progresso vinculado", body: `${nick.trim()} vai entrar com o inventário, a posição e as conquistas desse jogador.` });
      setTarget(null);
      setNick("");
      load();
    }
  };

  return (
    <div>
      <PageHeader title="Jogadores do mundo" subtitle="Quem já tem progresso no mundo, e o progresso que ainda não tem dono." />
      <div className="space-y-4">
        {!free && <InfoNote tone="warn">Para vincular progresso, ninguém pode estar hospedando.</InfoNote>}
        <ErrorBanner error={error} onClose={() => setError(null)} />

        {/* assistente: só aparece quando há algo para vincular */}
        {sugestoes.length > 0 && (
          <Card className="p-5 space-y-3 ring-1 ring-sun-400/25">
            <div className="flex items-center gap-2 font-bold">
              <Sparkles className="size-4 text-sun-400" /> Achamos progresso que parece ter dono
            </div>
            {sugestoes.map(({ p, s }) => (
              <div key={p.uuid} className="flex items-center gap-3 flex-wrap">
                <div className="flex-1 min-w-0 text-[13.5px]">
                  Isso parece ser de <b className="text-ink">{s!.nick}</b>: {s!.why}. Nível {p.xp_level}, {p.items} itens.
                </div>
                <Button
                  variant="sun"
                  icon={<Link2 className="size-4" />}
                  disabled={!free}
                  onClick={() => {
                    setNick(s!.nick);
                    setTarget(p);
                  }}
                >
                  Vincular a {s!.nick}
                </Button>
              </div>
            ))}
          </Card>
        )}

        <h3 className="text-[13px] font-semibold text-ink-muted px-1">Identificados</h3>
        <Card className="divide-y divide-white/[0.05]">
          {players === null && !error && <div className="h-40 animate-pulse" />}
          {players && identified.length === 0 && (
            <Empty icon={<Users className="size-6" />} title="Ninguém identificado ainda">
              Quem entrar no servidor aparece aqui, pelo nick.
            </Empty>
          )}
          {identified.map((p) => (
            <div key={p.uuid} className="flex items-center gap-4 px-5 py-3.5">
              <Avatar name={p.nick ?? p.uuid} size={36} />
              <div className="flex-1 min-w-0">
                <div className="font-semibold">{p.nick}</div>
                <div className="text-[12.5px] text-ink-muted mt-0.5">
                  Último acesso {ago(p.last_played)} · nível {p.xp_level} · {p.items} itens · {dim[p.dimension] ?? p.dimension}
                </div>
              </div>
            </div>
          ))}
        </Card>

        {orphans.length > 0 && (
          <div>
            <button onClick={() => setShowOrphans((v) => !v)} className="flex items-center gap-1.5 text-[13px] font-semibold text-ink-muted hover:text-ink cursor-pointer px-1 py-1">
              {showOrphans ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />} Progresso sem dono ({orphans.length})
            </button>
            {showOrphans && (
        <Card className="divide-y divide-white/[0.05] mt-2">
          <div className="px-5 py-3">
            <InfoNote>
              Progresso de single-player ou de conta com login fica guardado com outro identificador. Vincule ao nick certo e a pessoa entra com inventário, posição e conquistas.
              Nada é apagado.
            </InfoNote>
          </div>
          {orphans.map((p) => (
            <div key={p.uuid} className="flex items-center gap-4 px-5 py-3.5">
              <Avatar name={p.nick ?? p.uuid} size={36} />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-semibold">{p.nick ?? <span className="font-mono text-[12.5px] text-ink-soft">{p.uuid}</span>}</span>
                  {p.uuid_version === 4 ? <Pill tone="sun">conta com login</Pill> : <Pill tone="muted">nick</Pill>}
                  {p.singleplayer_owner && <Pill tone="dusk">dono do single-player</Pill>}
                </div>
                <div className="text-[12.5px] text-ink-muted mt-0.5">
                  Último acesso {ago(p.last_played)} · nível {p.xp_level} · {p.items} itens · {dim[p.dimension] ?? p.dimension}
                  {p.pos ? ` (${p.pos.join(", ")})` : ""}
                </div>
              </div>
              <Button variant="ghost" icon={<Link2 className="size-4" />} disabled={!free} onClick={() => setTarget(p)}>
                Vincular a um nick
              </Button>
            </div>
          ))}
        </Card>
            )}
          </div>
        )}
      </div>
      <Modal
        open={!!target}
        title="Vincular progresso a um nick"
        onClose={() => setTarget(null)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setTarget(null)}>
              Cancelar
            </Button>
            <Button variant="primary" busy={busy === "migrate"} disabled={!/^[A-Za-z0-9_]{3,16}$/.test(nick.trim())} onClick={migrate}>
              Vincular
            </Button>
          </>
        }
      >
        {target && (
          <div className="space-y-3 text-[13.5px]">
            <p>
              Progresso de nível <b className="text-ink">{target.xp_level}</b>, {target.items} itens, último acesso {ago(target.last_played)}.
            </p>
            <input autoFocus value={nick} onChange={(e) => setNick(e.target.value)} placeholder="Nick exato do Minecraft (ex.: mchado)" className={inputCls} />
            <p className="text-ink-muted text-[12.5px]">
              O app copia inventário, posição, conquistas e estatísticas para esse nick e publica como uma nova versão. Se o nick já tinha progresso, ele fica guardado como cópia.
            </p>
          </div>
        )}
      </Modal>
    </div>
  );
}
