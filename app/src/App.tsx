import { listen } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { NotificationBell, UpdateGate, UpdatedBanner, type NoticeItem } from "./components/Atualizacao";
import { Sidebar, ThemeToggle, TitleBar, Toasts, type Page } from "./components/Chrome";
import { AddWorldModal } from "./components/Mundos";
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
  // Adicionar mundo (menu de mundos ou link de convite com o app já configurado)
  const [adding, setAdding] = useState<{ invite: string | null } | null>(null);
  // muda a cada troca de mundo: as páginas montam de novo com os dados do outro mundo
  const [worldKey, setWorldKey] = useState("");
  const { toasts, dismiss, push } = useToasts();
  const status = useStatus(!!info?.configured);
  // Atualização do app: o serviço em Rust procura ao abrir e a cada 1 h e baixa sozinho.
  const upd = useAtualizacao(!!info?.configured);

  const loadInfo = useCallback(() => api.appInfo().then(setInfo), []);

  useEffect(() => {
    // sem catch: se falhar, a janela aparece mesmo assim (efeito abaixo não roda)
    loadInfo().catch(() => api.appReady());
    const un = listen("p3://quitting", () => setQuitting(true));
    return () => {
      un.then((f) => f());
    };
  }, [loadInfo]);

  // Só mostra a janela quando a tela já está completa: com o mundo configurado,
  // espera o primeiro status (com teto de 1,5 s) para não abrir com o HUD vazio e piscar.
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (shown || !info) return;
    if (info.configured && !status) {
      const t = window.setTimeout(() => setShown(true), 1500);
      return () => window.clearTimeout(t);
    }
    // um quadro para o React pintar antes de exibir
    const r = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(r);
  }, [info, status, shown]);
  useEffect(() => {
    if (shown) api.appReady();
  }, [shown]);

  // troca de mundo: volta ao Início, com tudo relido
  useEffect(() => {
    const un = listen<string>("p3://world-changed", (e) => {
      setWorldKey(e.payload);
      setNotices([]);
      setPage("home");
      loadInfo().catch(() => {});
    });
    return () => {
      un.then((f) => f());
    };
  }, [loadInfo]);

  // convite aberto com o app já configurado: é mais um mundo
  useEffect(() => {
    if (!info?.configured) return;
    const un = listen<string>("p3://invite", (e) => setAdding({ invite: e.payload }));
    return () => {
      un.then((f) => f());
    };
  }, [info?.configured]);

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
        onAddWorld={() => setAdding({ invite: null })}
        toast={push}
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
          <div key={worldKey} className="max-w-[1080px] mx-auto px-8 pt-3 pb-6">
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
      <AddWorldModal open={!!adding} initialInvite={adding?.invite} onClose={() => setAdding(null)} toast={push} />
      <Modal open={quitting} title="Salvando o mundo antes de sair" onClose={() => {}}>
        <div className="flex items-center gap-3 py-2">
          <Loader2 className="size-5 animate-spin text-sun-400 shrink-0" />
          <span>Fechando o servidor e guardando o mundo. O app fecha sozinho quando terminar.</span>
        </div>
      </Modal>
    </div>
  );
}
