import { listen } from "@tauri-apps/api/event";
import { Download, Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Sidebar, TitleBar, Toasts, type Page } from "./components/Chrome";
import { Button, Modal } from "./components/ui";
import { api } from "./lib/api";
import { useStatus, useToasts } from "./lib/hooks";
import type { AppInfo } from "./lib/types";
import Health from "./pages/Health";
import HomePage from "./pages/Home";
import Logs from "./pages/Logs";
import Players from "./pages/Players";
import SettingsPage from "./pages/Settings";
import StatsPage from "./pages/Stats";
import Timeline from "./pages/Timeline";
import Wizard from "./pages/Wizard";
import World from "./pages/World";

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [page, setPage] = useState<Page>(() => (import.meta.env.DEV ? (new URLSearchParams(location.search).get("page") as Page | null) : null) ?? "home");
  const [update, setUpdate] = useState<{ version: string } | null>(null);
  const [updating, setUpdating] = useState(false);
  const [quitting, setQuitting] = useState(false);
  const [healthBad, setHealthBad] = useState(false);
  const { toasts, dismiss, push } = useToasts();
  const status = useStatus(!!info?.configured);

  const loadInfo = useCallback(() => api.appInfo().then(setInfo), []);

  useEffect(() => {
    loadInfo().finally(() => api.appReady());
    const un = listen("p3://quitting", () => setQuitting(true));
    return () => {
      un.then((f) => f());
    };
  }, [loadInfo]);

  // Atualização: vem do Worker do dono (só para membros). Confere ao abrir e a cada 6 horas.
  useEffect(() => {
    if (!info?.configured) return;
    const look = () =>
      api
        .updateCheck()
        .then((u) => u && setUpdate(u))
        .catch(() => {});
    look();
    const t = window.setInterval(look, 6 * 3600 * 1000);
    return () => window.clearInterval(t);
  }, [info?.configured]);

  useEffect(() => {
    if (!info?.configured) return;
    const look = () =>
      api
        .health()
        .then((c) => setHealthBad(c.some((x) => x.level !== "ok")))
        .catch(() => {});
    look();
    const t = window.setInterval(look, 5 * 60 * 1000);
    return () => window.clearInterval(t);
  }, [info?.configured]);

  const installUpdate = async () => {
    if (!update) return;
    if (status?.me_hosting) {
      push({ level: "warning", title: "Feche a sessão antes de atualizar", body: "A atualização reinicia o app." });
      return;
    }
    setUpdating(true);
    try {
      await api.updateInstall();
    } catch (e) {
      setUpdating(false);
      push({ level: "error", title: "A atualização falhou", body: String(e) });
    }
  };

  if (!info) {
    return <div className="h-full sky" />;
  }

  if (!info.configured) {
    return (
      <div className="h-full sky">
        <Wizard
          onDone={() => {
            loadInfo();
            setPage("home");
          }}
        />
      </div>
    );
  }

  const isOwner = status?.is_owner ?? info.is_owner;
  return (
    <div className="h-full flex sky">
      <Sidebar page={page} onPage={setPage} isOwner={isOwner} version={info.version} healthBad={healthBad} status={status} />
      <div className="flex-1 min-w-0 flex flex-col">
        <TitleBar>
          {update && (
            <span className="flex items-center gap-2.5 animate-fade">
              <span className="text-ink-soft">
                A versão <b className="text-ink">{update.version}</b> está pronta.
              </span>
              <Button variant="ghost" className="h-7 px-2.5 text-sun-300" busy={updating} icon={<Download className="size-4" />} onClick={installUpdate}>
                Atualizar agora
              </Button>
            </span>
          )}
        </TitleBar>
        <main className="flex-1 min-h-0 scroll">
          <div className="max-w-[1080px] mx-auto px-8 pt-3 pb-10">
            {page === "home" && <HomePage status={status} onPage={setPage} toast={push} />}
            {page === "world" && <World status={status} toast={push} />}
            {page === "timeline" && <Timeline status={status} toast={push} />}
            {page === "stats" && <StatsPage />}
            {page === "health" && <Health toast={push} onChange={(bad) => setHealthBad(bad)} onPage={setPage} />}
            {page === "logs" && <Logs status={status} toast={push} />}
            {page === "players" && isOwner && <Players status={status} toast={push} />}
            {page === "settings" && <SettingsPage status={status} isOwner={isOwner} toast={push} />}
          </div>
        </main>
      </div>
      <Toasts toasts={toasts} dismiss={dismiss} />
      <Modal open={quitting} title="Salvando o mundo antes de sair" onClose={() => {}}>
        <div className="flex items-center gap-3 py-2">
          <Loader2 className="size-5 animate-spin text-sun-400 shrink-0" />
          <span>Fechando o servidor e guardando o mundo. O app fecha sozinho quando terminar.</span>
        </div>
      </Modal>
    </div>
  );
}
