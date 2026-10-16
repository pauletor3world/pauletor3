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
  const [page, setPage] = useState<Page>("home");
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
      <div className="h-full flex flex-col sky">
        <TitleBar status={null} />
        <Wizard
          onDone={() => {
            loadInfo();
            setPage("home");
          }}
        />
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col sky">
      <TitleBar status={status} />
      {update && (
        <div className="flex items-center justify-between gap-3 px-5 py-2 bg-sky-400/10 border-b border-sky-400/20 text-[12.5px] animate-fade">
          <span className="text-ink-soft">
            Nova versão <b className="text-ink">{update.version}</b> disponível.
          </span>
          <Button variant="ghost" className="h-7" busy={updating} icon={<Download className="size-4" />} onClick={installUpdate}>
            Atualizar agora
          </Button>
        </div>
      )}
      <div className="flex-1 flex min-h-0">
        <Sidebar page={page} onPage={setPage} isOwner={status?.is_owner ?? info.is_owner} version={info.version} healthBad={healthBad} />
        <main className="flex-1 min-w-0 scroll">
          <div className="max-w-[1080px] mx-auto px-8 py-7">
            {page === "home" && <HomePage status={status} onPage={setPage} toast={push} />}
            {page === "world" && <World status={status} toast={push} />}
            {page === "timeline" && <Timeline status={status} toast={push} />}
            {page === "stats" && <StatsPage />}
            {page === "health" && <Health toast={push} onChange={(bad) => setHealthBad(bad)} onPage={setPage} />}
            {page === "logs" && <Logs status={status} toast={push} />}
            {page === "players" && (status?.is_owner ?? info.is_owner) && <Players status={status} toast={push} />}
            {page === "settings" && <SettingsPage status={status} isOwner={status?.is_owner ?? info.is_owner} toast={push} />}
          </div>
        </main>
      </div>
      <Toasts toasts={toasts} dismiss={dismiss} />
      <Modal open={quitting} title="Salvando o mundo antes de sair" onClose={() => {}}>
        <div className="flex items-center gap-3 py-2">
          <Loader2 className="size-5 animate-spin text-moss-400" />
          <span>Fechando o servidor e guardando o mundo. O app fecha sozinho quando terminar.</span>
        </div>
      </Modal>
    </div>
  );
}
