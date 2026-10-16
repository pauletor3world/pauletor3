import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy, Crown, Link2, MessageCircle, Send, ShieldCheck, Trash2, UserMinus, UserPlus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Avatar } from "../components/Avatar";
import { Button, Card, CardTitle, Empty, ErrorBanner, Field, InfoNote, Modal, PageHeader, Pill, Toggle, cx, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { ago, dateTime } from "../lib/format";
import { useAction, type Toast } from "../lib/hooks";
import type { Invite, MemberView, Status } from "../lib/types";

type ToastFn = (t: Omit<Toast, "id" | "type">) => void;

export default function World({ status, toast }: { status: Status | null; toast: ToastFn }) {
  const owner = !!status?.is_owner;
  const { busy, error, setError, run } = useAction();
  const [link, setLink] = useState<string | null>(null);
  const [needApproval, setNeedApproval] = useState(false);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [webhook, setWebhook] = useState("");
  const [removing, setRemoving] = useState<MemberView | null>(null);

  const load = useCallback(() => {
    if (!owner) return;
    api.invites().then(setInvites).catch(() => {});
    api.getPrivate().then((p) => setWebhook(p.discord_webhook ?? "")).catch(() => {});
  }, [owner]);
  useEffect(load, [load]);

  const members = status?.members ?? [];
  const pending = members.filter((m) => !m.approved);
  const approved = members.filter((m) => m.approved);

  const newInvite = () =>
    run("invite", async () => {
      const l = await api.makeInvite(48, 5, needApproval);
      setLink(l);
      load();
    });

  const copy = async () => {
    if (!link) return;
    await writeText(link);
    toast({ level: "success", title: "Link copiado", body: "Mande para os amigos. Vale por 48 horas." });
  };

  const whatsapp = () => {
    if (!link) return;
    const msg = `Bora jogar no ${status?.world_name ?? "nosso mundo"}! Abre este link no PC, baixa o app e entra: ${link}`;
    openUrl(`https://wa.me/?text=${encodeURIComponent(msg)}`);
  };

  return (
    <div className="space-y-5">
      <PageHeader title="O mundo e a turma" subtitle={owner ? "Convide, aprove e cuide de quem entra." : "Quem faz parte deste mundo."} />
      <ErrorBanner error={error} onClose={() => setError(null)} />

      {owner && (
        <Card className="animate-rise overflow-hidden">
          <CardTitle aside={<Pill tone="sun">Dono</Pill>}>Convidar amigos</CardTitle>
          <div className="px-5 pb-5 space-y-4">
            <p className="text-ink-soft text-[13.5px]">
              Gere um link e mande pelo WhatsApp. O amigo abre no PC, baixa o app e entra — <b className="text-ink">sem criar conta</b>, sem configurar internet.
            </p>
            {link ? (
              <div className="rounded-2xl bg-night-950/60 ring-1 ring-moss-400/20 p-4 animate-rise">
                <div className="flex items-center gap-2 text-[12px] text-moss-300 mb-2">
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
              <div className="flex items-end justify-between gap-4 flex-wrap">
                <Toggle checked={needApproval} onChange={setNeedApproval} label="Eu aprovo quem entrar" hint="Desligado: quem tiver o link entra direto." />
                <Button variant="primary" icon={<UserPlus className="size-4" />} busy={busy === "invite"} onClick={newInvite}>
                  Gerar link de convite
                </Button>
              </div>
            )}
          </div>
        </Card>
      )}

      {owner && pending.length > 0 && (
        <Card className="animate-rise ring-1 ring-sun-400/30">
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

      <Card className="animate-rise">
        <CardTitle aside={<span className="text-[12px] text-ink-muted">{approved.length} membro(s)</span>}>Membros</CardTitle>
        <ul className="px-5 pb-5 divide-y divide-white/[0.05]">
          {approved.map((m) => (
            <li key={m.id} className="flex items-center gap-3 py-2.5">
              <Avatar name={m.nick} size={36} ring={m.hosting} />
              <div className="flex-1 min-w-0">
                <div className="font-semibold flex items-center gap-1.5">
                  {m.nick}
                  {m.role === "owner" && <Crown className="size-3.5 text-sun-400" />}
                  {m.is_me && <span className="text-[11px] text-ink-faint font-normal">(você)</span>}
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
        <div className="grid grid-cols-2 gap-5">
          <Card className="animate-rise">
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

          <Card className="animate-rise">
            <CardTitle>Diário no Discord (opcional)</CardTitle>
            <div className="px-5 pb-5 space-y-3">
              <p className="text-[13px] text-ink-soft">
                O diário já aparece aqui no app. Se quiser, ele também vai para um canal do Discord: quem entrou, mortes, conquistas e trocas de host.
              </p>
              <Field label="Webhook do canal" hint="Discord → Editar canal → Integrações → Webhooks → Copiar URL.">
                <input className={inputCls} value={webhook} placeholder="https://discord.com/api/webhooks/…" onChange={(e) => setWebhook(e.target.value)} />
              </Field>
              <div className="flex gap-2">
                <Button
                  variant="primary"
                  icon={<ShieldCheck className="size-4" />}
                  busy={busy === "wh"}
                  onClick={() => run("wh", async () => (await api.setWebhook(webhook), toast({ level: "success", title: "Diário configurado", body: "" })))}
                >
                  Salvar
                </Button>
                <Button
                  icon={<Send className="size-4" />}
                  disabled={!webhook}
                  busy={busy === "test"}
                  onClick={() => run("test", async () => (await api.testWebhook(webhook), toast({ level: "success", title: "Mensagem de teste enviada", body: "" })))}
                >
                  Testar
                </Button>
              </div>
              <InfoNote>O endereço fica guardado só na nuvem do mundo; os amigos não precisam configurar nada.</InfoNote>
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
        O app dele perde o acesso ao mundo na hora. O progresso dele no jogo continua salvo no mundo; se ele voltar por um convite novo, continua de onde parou.
      </Modal>
    </div>
  );
}
