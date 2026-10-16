//! App desktop do pAuletOr 3 (Tauri 2). Toda a lógica mora no `pauletor3-core`;
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

use pauletor3_core::events::{Event, EventSink, NoticeLevel};
use pauletor3_core::paths::{self, Paths};
use pauletor3_core::session::{EndReason, Engine, Phase};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub const CONSOLE_LINES: usize = 3000;

pub struct AppState {
    pub engine: RwLock<Option<Arc<Engine>>>,
    pub console: Mutex<VecDeque<String>>,
    pub quitting: AtomicBool,
    pub start_minimized: bool,
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

/// Caminho do rclone que vai junto com o app (sidecar).
pub fn rclone_path() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_default();
    let candidates = [
        exe_dir.join("rclone.exe"),
        exe_dir.join("rclone-x86_64-pc-windows-msvc.exe"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join("rclone-x86_64-pc-windows-msvc.exe"),
    ];
    candidates.iter().find(|p| p.exists()).cloned().unwrap_or_else(|| exe_dir.join("rclone.exe"))
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
            if let Ok(guard) = pauletor3_core::logging::init(&p.logs(), cfg!(debug_assertions)) {
                *g = Some(Box::new(guard));
            }
        }
    }
    // Só para desenvolvimento/demonstração: usa uma pasta local como "nuvem".
    let engine = match std::env::var_os("PAULETOR3_LOCAL_REMOTE") {
        Some(dir) => {
            let r = pauletor3_core::remote::RcloneRemote::local(rclone_path(), std::path::Path::new(&dir), p.tmp());
            Engine::with_remote(p, Arc::new(r), event_sink(app.clone()))?
        }
        None => Engine::open(p, rclone_path(), event_sink(app.clone()))?,
    };
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

/// Tarefas de fundo: diário, atualização periódica do estado e recuperação pendente.
fn start_background(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(engine.journal.clone().run());
    let e2 = engine.clone();
    tauri::async_runtime::spawn(async move {
        // Recuperação automática de uma sessão que caiu neste PC.
        let cfg = e2.config.read().await.clone();
        if cfg.setup_complete && e2.status().recovery_pending {
            if let Err(err) = e2.recover().await {
                tracing::error!(error = %err, "recuperação automática falhou");
            }
        }
        loop {
            let st = e2.status();
            if e2.config.read().await.setup_complete {
                if let Err(err) = e2.refresh().await {
                    tracing::debug!(error = %err, "refresh falhou");
                }
            }
            let wait = match st.phase {
                Phase::WaitingTurn => 5,
                _ if st.me_hosting => 30,
                _ => 20,
            };
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    });
    let _ = app;
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
            }
        }
        app2.exit(0);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    pauletor3_core::shutdown::request_early_notification();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
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
            log_guard: Mutex::new(None),
        })
        .setup(|app| {
            let handle = app.handle().clone();
            tray::create(&handle)?;
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
            commands::set_secret,
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
            setup::setup_suggest_dir,
            setup::setup_init,
            setup::drive_authorize,
            setup::world_probe,
            setup::import_world,
            setup::create_world,
            setup::tailscale_status,
            setup::tailscale_install,
            setup::tailscale_up,
            setup::save_network,
            setup::test_duckdns,
            setup::install_firewall,
            setup::remove_firewall,
            setup::ensure_serversdat,
            setup::prepare_runtime,
            setup::create_invite,
            setup::accept_invite,
            setup::finish_setup,
        ])
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o pAuletOr 3")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                // Sem janelas visíveis o app continua vivo na bandeja.
                if code.is_none() && !app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
