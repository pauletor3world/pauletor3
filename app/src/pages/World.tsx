import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy, Crown, Link2, MessageCircle, Trash2, UserMinus, UserPlus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Button, Card, CardTitle, Empty, ErrorBanner, Modal, PageHeader, Pill, Toggle, cx } from "../components/ui";
import { api } from "../lib/api";
import { ago, dateTime } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { Invite, MemberView, Status } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

export default function World({ status, toast }: { status: Status | null; toast: ToastFn }) {
  const owner = !!status?.is_owner;
  const { busy, error, setError, run } = useAction();
  const [link, setLink] = useState<string | null>(null);
  // padrão seguro: o dono aprova quem entra e o link vale para uma pessoa só
  const [needApproval, setNeedApproval] = useState(true);
  const [single, setSingle] = useState(true);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [removing, setRemoving] = useState<MemberView | null>(null);

  const load = useCallback(() => {
    if (!owner) return;
    api.invites().then(setInvites).catch(() => {});
  }, [owner]);
  useEffect(load, [load]);

  const members = status?.members ?? [];
  const pending = members.filter((m) => !m.approved);
  const approved = members.filter((m) => m.approved);

  const newInvite = () =>
    run("invite", async () => {
      const l = await api.makeInvite(48, single ? 1 : 5, needApproval);
      setLink(l);
      load();
    });

  const copy = async () => {
    if (!link) return;
    await writeText(link);
    toast({ level: "success", title: "Link copiado", body: single ? "Vale para uma pessoa, por 48 horas." : "Vale para até 5 pessoas, por 48 horas." });
  };

  const whatsapp = () => {
    if (!link) return;
    const msg = `Bora jogar no ${status?.world_name ?? "nosso mundo"}! Abre este link no PC, baixa o app e entra: ${link}`;
    openUrl(`https://wa.me/?text=${encodeURIComponent(msg)}`);
  };

  return (
    <div className="space-y-5">
      <PageHeader title="O grupo" subtitle={owner ? "Convide, aprove e cuide de quem entra." : "Quem faz parte deste mundo."} />
      <ErrorBanner error={error} onClose={() => setError(null)} />

      {owner && (
        <Card className="overflow-hidden">
          <CardTitle aside={<Pill tone="sun">Dono</Pill>}>Convidar amigos</CardTitle>
          <div className="px-5 pb-5 space-y-4">
            <p className="text-ink-soft text-[13.5px]">
              Gere um link e mande pelo WhatsApp. O amigo abre no PC, baixa o app e entra — <b className="text-ink">sem criar conta</b>, sem configurar internet.
            </p>
            {link ? (
              <div className="rounded-2xl bg-night-950/60 ring-1 ring-inset ring-sun-400/25 p-4">
                <div className="flex items-center gap-2 text-[12.5px] font-semibold text-sun-300 mb-2">
                  <Link2 className="size-4" /> Link pronto
                </div>
                <code className="block font-mono text-[12.5px] text-ink-soft break-all selectable">{link}</code>
                <div className="flex flex-wrap gap-2 mt-3">
                  <Button icon={<Copy className="size-4" />} onClick={copy}>
                    Copiar link
                  </Button>
                  <Button variant="primary" icon={<MessageCircle className="size-4" />} onClick={whatsapp}>
                    Mandar no WhatsApp
                  </Button>
                  <Button variant="ghost" onClick={() => setLink(null)}>
                    Fechar
                  </Button>
                </div>
              </div>
            ) : (
              <div className="space-y-1">
                <Toggle checked={needApproval} onChange={setNeedApproval} label="Eu aprovo quem entrar" hint="Recomendado. Desligado, quem tiver o link entra direto." />
                <Toggle checked={single} onChange={setSingle} label="Link para uma pessoa só" hint="Recomendado. Desligado, o mesmo link vale para até 5 pessoas." />
                <div className="flex justify-end pt-2">
                  <Button variant="primary" icon={<UserPlus className="size-4" />} busy={busy === "invite"} onClick={newInvite}>
                    Gerar link de convite
                  </Button>
                </div>
              </div>
            )}
          </div>
        </Card>
      )}

      {owner && pending.length > 0 && (
        <Card className="ring-1 ring-sun-400/30">
          <CardTitle aside={<Pill tone="sun">{pending.length} esperando</Pill>}>Pedidos para entrar</CardTitle>
          <ul className="px-5 pb-5 space-y-2">
            {pending.map((m) => (
              <li key={m.id} className="flex items-center gap-3 rounded-xl bg-night-950/40 ring-1 ring-white/5 p-2.5">
                <Avatar name={m.nick} size={36} />
                <div className="flex-1 min-w-0">
                  <div className="font-semibold">{m.nick}</div>
                  <div className="text-[11.5px] text-ink-muted">entrou pelo convite {ago(m.last_seen)}</div>
                </div>
                <Button variant="ghost" icon={<Trash2 className="size-4" />} onClick={() => run("rm" + m.id, () => api.setMember(m.id, null, true))}>
                  Recusar
                </Button>
                <Button variant="primary" icon={<Check className="size-4" />} busy={busy === "ap" + m.id} onClick={() => run("ap" + m.id, () => api.setMember(m.id, true, false))}>
                  Aprovar
                </Button>
              </li>
            ))}
          </ul>
        </Card>
      )}

      <Card>
        <CardTitle aside={<span className="text-[12px] text-ink-muted">{approved.length} {approved.length === 1 ? "membro" : "membros"}</span>}>Membros</CardTitle>
        <ul className="px-5 pb-5 divide-y divide-white/[0.05]">
          {approved.map((m) => (
            <li key={m.id} className="flex items-center gap-3 py-2.5">
              <Avatar name={m.nick} size={36} ring={m.hosting} />
              <div className="flex-1 min-w-0">
                <div className="font-semibold flex items-center gap-1.5">
                  {m.nick}
                  {m.role === "owner" && <Crown className="size-3.5 text-sun-400" />}
                  {m.is_me && <span className="text-[12px] text-ink-faint font-medium">você</span>}
                </div>
                <div className="text-[11.5px] text-ink-muted">
                  {m.hosting ? "Hospedando agora" : m.playing ? "Jogando" : m.online ? "Online" : m.last_seen ? `Visto ${ago(m.last_seen)}` : "Ainda não apareceu"}
                  {m.path && ` · conexão ${m.path}`}
                </div>
              </div>
              {owner && m.role !== "owner" && (
                <Button variant="ghost" icon={<UserMinus className="size-4" />} onClick={() => setRemoving(m)}>
                  Remover
                </Button>
              )}
            </li>
          ))}
        </ul>
      </Card>

      {owner && (
        <div>
          <Card>
            <CardTitle>Convites ativos</CardTitle>
            <div className="px-5 pb-5">
              {invites.length === 0 ? (
                <Empty icon={<Link2 className="size-6" />} title="Nenhum convite">
                  Os links que você gerar aparecem aqui, com validade e usos.
                </Empty>
              ) : (
                <ul className="space-y-2">
                  {invites.map((i) => {
                    const expired = new Date(i.expires_at).getTime() < Date.now() || i.uses_left === 0;
                    return (
                      <li key={i.id} className={cx("flex items-center gap-3 rounded-xl bg-night-950/40 ring-1 ring-white/5 p-2.5", expired && "opacity-50")}>
                        <div className="flex-1 min-w-0 text-[12.5px]">
                          <div className="text-ink">{expired ? "Expirado" : `Vale até ${dateTime(i.expires_at)}`}</div>
                          <div className="text-ink-muted">
                            {i.uses_left} uso(s) restante(s) · {i.preapproved ? "entra direto" : "com aprovação"}
                          </div>
                        </div>
                        <Button variant="ghost" icon={<Trash2 className="size-4" />} onClick={() => run("rv" + i.id, async () => (await api.revokeInvite(i.id), load()))}>
                          Revogar
                        </Button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </Card>

        </div>
      )}

      <Modal
        open={!!removing}
        title={`Remover ${removing?.nick ?? ""}?`}
        onClose={() => setRemoving(null)}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(null)}>
              Cancelar
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                const m = removing;
                setRemoving(null);
                if (m) run("rm", () => api.setMember(m.id, null, true));
              }}
            >
              Remover
            </Button>
          </>
        }
      >
        O app dele perde o acesso ao mundo na hora. O inventário e o progresso dele ficam guardados no mundo: ele pode voltar com um convite novo e continua de onde parou.
      </Modal>
    </div>
  );
}
