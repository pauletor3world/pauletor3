import { listen } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { NotificationBell, UpdateGate, UpdatedBanner, type NoticeItem } from "./components/Atualizacao";
import { Sidebar, ThemeToggle, TitleBar, Toasts, type Page } from "./components/Chrome";
import { Modal } from "./components/ui";
import { api } from "./lib/api";
import { atualizacaoApi, temAtualizacao, useAtualizacao } from "./lib/atualizacao";
import { useStatus, useToasts } from "./lib/hooks";
import { useTheme } from "./lib/theme";
import type { AppInfo, Notice } from "./lib/types";
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
  const [quitting, setQuitting] = useState(false);
  const { theme, setTheme } = useTheme();
  const [bellOpen, setBellOpen] = useState(() => import.meta.env.DEV && new URLSearchParams(location.search).has("bell"));
  const [notices, setNotices] = useState<NoticeItem[]>([]);
  const [healthBad, setHealthBad] = useState(false);
  const { toasts, dismiss, push } = useToasts();
  const status = useStatus(!!info?.configured);
  // Atualização do app: o serviço em Rust procura ao abrir e a cada 1 h e baixa sozinho.
  const upd = useAtualizacao(!!info?.configured);

  const loadInfo = useCallback(() => api.appInfo().then(setInfo), []);

  useEffect(() => {
    loadInfo().finally(() => api.appReady());
    const un = listen("p3://quitting", () => setQuitting(true));
    return () => {
      un.then((f) => f());
    };
  }, [loadInfo]);

  // histórico do sino ("Nover abriu o mundo", ...)
  useEffect(() => {
    const un = listen<Notice>("p3://notice", (e) => setNotices((n) => [...n.slice(-49), { ...e.payload, at: new Date().toISOString() }]));
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!info?.configured) return;
    const look = () =>
      api
        .health()
        .then((c) => setHealthBad(c.some((x) => x.level !== "ok")))
        .catch(() => {});
    look();
    const t = window.setInterval(look, 60 * 1000);
    return () => window.clearInterval(t);
    // a bolinha da Saúde acompanha o estado: confere de novo quando algo relevante muda
  }, [info?.configured, status?.replicating, status?.coord_ok, status?.recovery_pending, status?.phase, status?.head_seq]);


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
  const update = upd.a ? { ...upd, a: upd.a } : null;
  const bloqueado = !!(upd.a?.bloqueado || status?.desatualizado);
  return (
    <div className="h-full flex sky">
      <Sidebar
        page={page}
        onPage={setPage}
        isOwner={isOwner}
        version={info.version}
        healthBad={healthBad}
        status={status}
        updateAvailable={temAtualizacao(upd.a)}
        onUpdate={() => setBellOpen(true)}
      />
      <div className="flex-1 min-w-0 flex flex-col">
        <TitleBar
          actions={
            <>
              <ThemeToggle theme={theme} onTheme={setTheme} />
              <NotificationBell update={update} notices={notices} open={bellOpen} onOpen={setBellOpen} />
            </>
          }
        />
        <main className="flex-1 min-h-0 scroll">
          <div className="max-w-[1080px] mx-auto px-8 pt-3 pb-10">
            {upd.a?.acabou_de_atualizar && <UpdatedBanner versao={upd.a.acabou_de_atualizar} onClose={() => atualizacaoApi.visto()} />}
            {bloqueado && update ? (
              <UpdateGate {...update} />
            ) : (
              <>
            {page === "home" && <HomePage status={status} onPage={setPage} toast={push} />}
            {page === "world" && <World status={status} toast={push} />}
            {page === "timeline" && <Timeline status={status} toast={push} />}
            {page === "stats" && <StatsPage worldName={status?.world_name} />}
            {page === "health" && <Health toast={push} onChange={(bad) => setHealthBad(bad)} onPage={setPage} />}
            {page === "logs" && <Logs status={status} toast={push} />}
            {page === "players" && isOwner && <Players status={status} toast={push} />}
            {page === "settings" && <SettingsPage status={status} isOwner={isOwner} toast={push} />}
              </>
            )}
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
