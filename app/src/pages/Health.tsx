import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CheckCircle2, ClipboardCopy, RefreshCw, TriangleAlert, XCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button, Card, ErrorBanner, PageHeader, cx } from "../components/ui";
import { api } from "../lib/api";
import { useAction, type Toast } from "../lib/hooks";
import type { Check } from "../lib/types";
import type { Page } from "../components/Chrome";

export default function Health({ toast, onChange, onPage }: { toast: (t: Omit<Toast, "id" | "type">) => void; onChange: (bad: boolean) => void; onPage: (p: Page) => void }) {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const { busy, error, setError, run } = useAction();

  const load = useCallback(() => {
    run("load", api.health).then((c) => {
      if (c) {
        setChecks(c);
        onChange(c.some((x) => x.level !== "ok"));
      }
    });
  }, [run, onChange]);
  useEffect(load, [load]);

  const fix = async (c: Check) => {
    if (!c.fix) return;
    if (c.fix === "open_settings") return onPage("settings");
    const msg = await run(c.id, () => api.runFix(c.fix!));
    if (msg) toast({ level: "success", title: c.title, body: msg });
    load();
  };

  const copyDiag = async () => {
    const r = await run("diag", api.diagnostics);
    if (r) {
      await writeText(r);
      toast({ level: "success", title: "Diagnóstico copiado", body: "Cole na conversa para pedir ajuda. Senhas, tokens e IPs foram removidos." });
    }
  };

  const okCount = checks?.filter((c) => c.level === "ok").length ?? 0;

  return (
    <div>
      <PageHeader
        title="Saúde do sistema"
        subtitle={checks ? `${okCount} de ${checks.length} itens em ordem` : "Conferindo…"}
        aside={
          <div className="flex gap-2">
            <Button variant="ghost" icon={<ClipboardCopy className="size-4" />} busy={busy === "diag"} onClick={copyDiag}>
              Copiar diagnóstico
            </Button>
            <Button icon={<RefreshCw className={cx("size-4", busy === "load" && "animate-spin")} />} onClick={load} disabled={busy === "load"}>
              Conferir de novo
            </Button>
          </div>
        }
      />
      <div className="space-y-4">
        <ErrorBanner error={error} onClose={() => setError(null)} />
        <Card className="divide-y divide-white/[0.05]">
          {(checks ?? Array.from({ length: 8 }, () => null)).map((c, i) =>
            c ? (
              <div key={c.id} className="flex items-center gap-4 px-5 py-4 animate-fade">
                {c.level === "ok" ? (
                  <CheckCircle2 className="size-5 text-moss-400 shrink-0" />
                ) : c.level === "warn" ? (
                  <TriangleAlert className="size-5 text-sun-400 shrink-0" />
                ) : (
                  <XCircle className="size-5 text-ember-400 shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <div className="font-semibold">{c.title}</div>
                  <div className="text-[12.5px] text-ink-muted mt-0.5 selectable">{c.detail}</div>
                </div>
                {c.fix && (
                  <Button variant={c.level === "error" ? "sun" : "secondary"} busy={busy === c.id} onClick={() => fix(c)}>
                    {c.fix_label ?? "Corrigir"}
                  </Button>
                )}
              </div>
            ) : (
              <div key={i} className="h-[68px] animate-pulse bg-white/[0.015]" />
            ),
          )}
        </Card>
      </div>
    </div>
  );
}
