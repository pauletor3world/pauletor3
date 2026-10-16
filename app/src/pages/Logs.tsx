import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ArrowDownToLine, ClipboardCopy, Search, SendHorizonal, Terminal } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, Empty, ErrorBanner, PageHeader, cx, inputCls } from "../components/ui";
import { api } from "../lib/api";
import { useAction, type Toast } from "../lib/hooks";
import type { Status } from "../lib/types";

const MAX = 3000;

function lineTone(l: string) {
  if (/\/(ERROR|FATAL)\]/.test(l) || l.includes("Exception")) return "text-ember-400";
  if (/\/WARN\]/.test(l)) return "text-sun-300";
  if (/ joined the game| left the game/.test(l)) return "text-moss-300";
  if (/has (made the advancement|reached the goal|completed the challenge)/.test(l)) return "text-dusk-400";
  return "text-ink-soft";
}

export default function Logs({ status, toast }: { status: Status | null; toast: (t: Omit<Toast, "id" | "type">) => void }) {
  const [lines, setLines] = useState<string[]>([]);
  const [q, setQ] = useState("");
  const [cmd, setCmd] = useState("");
  const [follow, setFollow] = useState(true);
  const box = useRef<HTMLDivElement>(null);
  const { busy, error, setError, run } = useAction();

  useEffect(() => {
    api.consoleHistory().then(setLines).catch(() => {});
    const un = listen<string>("p3://log", (e) => setLines((ls) => (ls.length >= MAX ? [...ls.slice(-MAX + 1), e.payload] : [...ls, e.payload])));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return needle ? lines.filter((l) => l.toLowerCase().includes(needle)) : lines;
  }, [lines, q]);

  useEffect(() => {
    if (follow && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [shown, follow]);

  const send = async () => {
    const c = cmd.trim();
    if (!c) return;
    const r = await run("cmd", () => api.consoleCmd(c));
    if (r !== undefined) {
      setCmd("");
      setLines((ls) => [...ls, `> ${c}`, ...(r ? [r] : [])]);
    }
  };

  const copyDiag = async () => {
    const r = await run("diag", api.diagnostics);
    if (r) {
      await writeText(r);
      toast({ level: "success", title: "Diagnóstico copiado", body: "Relatório anonimizado, pronto para colar." });
    }
  };

  return (
    <div className="flex flex-col h-[calc(100vh-130px)]">
      <PageHeader
        title="Console do servidor"
        subtitle={status?.me_hosting ? "Ao vivo" : "Mostra o console quando este PC está hospedando."}
        aside={
          <Button variant="ghost" icon={<ClipboardCopy className="size-4" />} busy={busy === "diag"} onClick={copyDiag}>
            Copiar diagnóstico
          </Button>
        }
      />
      <ErrorBanner error={error} onClose={() => setError(null)} />
      <div className="flex gap-2 mb-3 mt-1">
        <div className="relative flex-1">
          <Search className="size-4 absolute left-3 top-3 text-ink-faint" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Buscar no console…" className={cx(inputCls, "pl-9")} />
        </div>
        <Button variant={follow ? "secondary" : "ghost"} icon={<ArrowDownToLine className="size-4" />} onClick={() => setFollow((f) => !f)} title="Acompanhar o fim">
          {follow ? "Acompanhando" : "Pausado"}
        </Button>
      </div>
      <Card className="flex-1 min-h-0 flex flex-col overflow-hidden">
        {shown.length === 0 ? (
          <Empty icon={<Terminal className="size-6" />} title={q ? "Nada encontrado" : "Console vazio"}>
            {q ? "Tente outra busca." : "As mensagens do servidor aparecem aqui enquanto você hospeda."}
          </Empty>
        ) : (
          <div
            ref={box}
            onWheel={() => box.current && setFollow(box.current.scrollTop + box.current.clientHeight >= box.current.scrollHeight - 8)}
            className="flex-1 scroll px-4 py-3 font-mono text-[12px] leading-[1.6] selectable"
          >
            {shown.map((l, i) => (
              <div key={i} className={cx("whitespace-pre-wrap break-all", lineTone(l))}>
                {l}
              </div>
            ))}
          </div>
        )}
        {status?.me_hosting && (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
            className="flex gap-2 border-t border-white/[0.05] p-3"
          >
            <input value={cmd} onChange={(e) => setCmd(e.target.value)} placeholder="Comando do servidor (ex.: say Olá, time set day)" className={cx(inputCls, "font-mono text-[12.5px]")} />
            <Button variant="primary" type="submit" busy={busy === "cmd"} icon={<SendHorizonal className="size-4" />}>
              Enviar
            </Button>
          </form>
        )}
      </Card>
    </div>
  );
}
