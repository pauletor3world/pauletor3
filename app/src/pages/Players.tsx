import { Link2, Users } from "lucide-react";
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

  const free = status?.lock?.kind === "free" && !status.me_hosting;

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
      <PageHeader title="Jogadores do mundo" subtitle="Ligue o progresso antigo de cada pessoa ao nick que ela usa." />
      <div className="space-y-4">
        <InfoNote>
          O servidor reconhece cada pessoa pelo <b className="text-ink">nick</b>. Progresso salvo em single-player ou com conta logada fica num identificador diferente (marcado como{" "}
          <b className="text-ink">conta com login</b>). Vincule ao nick certo antes de jogar, senão a pessoa entra sem inventário. Nada é apagado.
        </InfoNote>
        {!free && <InfoNote tone="warn">Para vincular progresso, ninguém pode estar hospedando.</InfoNote>}
        <ErrorBanner error={error} onClose={() => setError(null)} />
        <Card className="divide-y divide-white/[0.05]">
          {players === null && !error && <div className="h-40 animate-pulse" />}
          {players && players.length === 0 && (
            <Empty icon={<Users className="size-6" />} title="Nenhum jogador salvo no mundo">
              Quem entrar no servidor aparece aqui.
            </Empty>
          )}
          {players?.map((p) => (
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
              <Button variant={p.uuid_version === 4 ? "sun" : "ghost"} icon={<Link2 className="size-4" />} disabled={!free} onClick={() => setTarget(p)}>
                Vincular a um nick
              </Button>
            </div>
          ))}
        </Card>
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
