//! App desktop do Machen Craft (Tauri 2). Toda a lógica mora no `machencraft-core`;
//! aqui ficam a ponte com a interface, a bandeja, as notificações e o ciclo de vida da janela.

mod commands;
mod setup;
mod tray;
#[cfg(windows)]
mod winshutdown;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use machencraft_core::events::{Event, EventSink, NoticeLevel};
use machencraft_core::paths::{self, Paths};
use machencraft_core::session::{EndReason, Engine, Phase};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub const CONSOLE_LINES: usize = 3000;

pub struct AppState {
    pub engine: RwLock<Option<Arc<Engine>>>,
    pub console: Mutex<VecDeque<String>>,
    pub quitting: AtomicBool,
    pub start_minimized: bool,
    /// Convite recebido por link (`machencraft://entrar/...`) esperando o assistente.
    pub pending_invite: Mutex<Option<String>>,
    /// Atualização encontrada, esperando o "Atualizar agora".
    pub update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    log_guard: Mutex<Option<tracing_appender_guard::Guard>>,
}

/// Guarda o `WorkerGuard` do log sem expor o tipo do tracing-appender no app.
mod tracing_appender_guard {
    pub type Guard = Box<dyn std::any::Any + Send>;
}

impl AppState {
    pub fn engine(&self) -> Option<Arc<Engine>> {
        self.engine.read().unwrap().clone()
    }
}

/// Abre (ou reabre) o motor na pasta de dados escolhida.
pub fn open_engine(app: &AppHandle, data_dir: PathBuf) -> anyhow::Result<Arc<Engine>> {
    let state = app.state::<AppState>();
    if let Some(e) = state.engine() {
        if e.paths.root == data_dir {
            return Ok(e);
        }
    }
    let p = Paths::new(&data_dir);
    p.ensure()?;
    {
        let mut g = state.log_guard.lock().unwrap();
        if g.is_none() {
            if let Ok(guard) = machencraft_core::logging::init(&p.logs(), cfg!(debug_assertions)) {
                *g = Some(Box::new(guard));
            }
        }
    }
    let engine = Engine::open(p, event_sink(app.clone()))?;
    *state.engine.write().unwrap() = Some(engine.clone());
    start_background(app.clone(), engine.clone());
    Ok(engine)
}

fn event_sink(app: AppHandle) -> EventSink {
    Arc::new(move |e: Event| {
        match &e {
            Event::StatusChanged => {
                if let Some(engine) = app.state::<AppState>().engine() {
                    let st = engine.status();
                    #[cfg(windows)]
                    winshutdown::set_busy(st.me_hosting || !matches!(st.phase, Phase::Idle | Phase::WaitingTurn));
                    tray::update(&app, &st);
                    let _ = app.emit("p3://status", &st);
                }
            }
            Event::Sync(p) => {
                let _ = app.emit("p3://sync", p);
            }
            Event::ServerLog { line } => {
                {
                    let st = app.state::<AppState>();
                    let mut c = st.console.lock().unwrap();
                    c.push_back(line.clone());
                    while c.len() > CONSOLE_LINES {
                        c.pop_front();
                    }
                }
                let _ = app.emit("p3://log", line);
            }
            Event::PlayerJoined { .. } | Event::PlayerLeft { .. } => {
                let _ = app.emit("p3://players", ());
            }
            Event::Notice { level, title, body } => {
                let _ = app.emit("p3://notice", &e);
                let toasts = app
                    .state::<AppState>()
                    .engine()
                    .map(|en| en.config.try_read().map(|c| c.notify_toasts).unwrap_or(true))
                    .unwrap_or(true);
                let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
                if toasts && (!focused || matches!(level, NoticeLevel::Error)) {
                    let _ = app.notification().builder().title(title).body(body).show();
                }
            }
        }
    })
}

/// Tarefas de fundo: diário, recuperação pendente e a rede (presença, proxy local, réplica e
/// troca automática de host — o laço mora no núcleo).
fn start_background(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(engine.journal.clone().run());
    tauri::async_runtime::spawn(async move {
        let cfg = engine.config.read().await.clone();
        if !cfg.ready() {
            return;
        }
        if engine.status().recovery_pending {
            if let Err(err) = engine.recover().await {
                tracing::error!(error = %err, "recuperação automática falhou");
            }
        }
        // tenta até a rede subir (sem internet no boot, por exemplo)
        for attempt in 0..u32::MAX {
            match engine.start_network().await {
                Ok(()) => break,
                Err(err) => {
                    tracing::warn!(error = %err, attempt, "rede P2P não iniciou; tentando de novo");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
        }
        // Minecraft conferido (e baixado se faltar) antes do primeiro Jogar
        engine.prefetch_client().await;
        let _ = app;
    });
}

/// Recebe um link `machencraft://entrar/...` (ou o antigo `pauletor3://`) do convite.
fn handle_deep_links(app: &AppHandle, urls: Vec<String>) {
    for u in urls {
        if u.starts_with("machencraft://") || u.starts_with("pauletor3://") {
            *app.state::<AppState>().pending_invite.lock().unwrap() = Some(u.clone());
            let _ = app.emit("p3://invite", u);
            show_main(app);
        }
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Sair do app: se estiver hospedando, salva e envia antes.
pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(engine) = app2.state::<AppState>().engine() {
            if engine.is_hosting().await {
                show_main(&app2);
                let _ = app2.emit("p3://quitting", ());
                let _ = engine.stop(EndReason::UserStop).await;
                // passou o host para um amigo: ele pega a versão final daqui, por P2P
                engine.wait_successor(std::time::Duration::from_secs(90)).await;
            }
        }
        app2.exit(0);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    machencraft_core::shutdown::request_early_notification();
    tauri::Builder::default()
        // precisa ser o primeiro plugin; com "deep-link", o link chega ao app que já está aberto
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            engine: RwLock::new(None),
            console: Mutex::new(VecDeque::new()),
            quitting: AtomicBool::new(false),
            start_minimized,
            pending_invite: Mutex::new(None),
            update: tokio::sync::Mutex::new(None),
            log_guard: Mutex::new(None),
        })
        .setup(|app| {
            let handle = app.handle().clone();
            tray::create(&handle)?;
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                #[cfg(debug_assertions)]
                let _ = app.deep_link().register_all();
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    handle_deep_links(&handle, urls.into_iter().map(|u| u.to_string()).collect());
                }
                let h2 = handle.clone();
                app.deep_link().on_open_url(move |ev| handle_deep_links(&h2, ev.urls().into_iter().map(|u| u.to_string()).collect()));
            }
            if let Some(dir) = paths::resolve_data_dir() {
                if let Err(e) = open_engine(&handle, dir) {
                    tracing::error!(error = %e, "não consegui abrir a pasta de dados");
                }
            }
            #[cfg(windows)]
            if let Some(w) = app.get_webview_window("main") {
                if let Ok(hwnd) = w.hwnd() {
                    winshutdown::install(handle.clone(), hwnd.0 as isize);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Fechar a janela só esconde (o app continua na bandeja).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !window.app_handle().state::<AppState>().quitting.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_ready,
            commands::app_info,
            commands::get_status,
            commands::refresh,
            commands::play,
            commands::open_game,
            commands::take_turn,
            commands::pass_turn,
            commands::stop_session,
            commands::save_now,
            commands::recover,
            commands::timeline,
            commands::restore,
            commands::stats,
            commands::health,
            commands::run_fix,
            commands::diagnostics,
            commands::get_settings,
            commands::save_settings,
            commands::get_private,
            commands::set_webhook,
            commands::invites,
            commands::revoke_invite,
            commands::set_member,
            commands::diary,
            commands::get_world_settings,
            commands::save_world_settings,
            commands::console_cmd,
            commands::console_history,
            commands::detect_launchers,
            commands::open_launcher,
            commands::world_players,
            commands::migrate_player,
            commands::gc,
            commands::journal_status,
            commands::test_webhook,
            commands::quit_app,
            commands::hide_window,
            commands::update_check,
            commands::update_install,
            setup::setup_suggest_dir,
            setup::setup_init,
            setup::invite_preview,
            setup::create_world,
            setup::join_world,
            setup::probe_world_folder,
            setup::make_invite,
            setup::ensure_serversdat,
            setup::prepare_runtime,
            setup::finish_setup,
        ])
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o Machen Craft")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                // Sem janelas visíveis o app continua vivo na bandeja.
                if code.is_none() && !app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
