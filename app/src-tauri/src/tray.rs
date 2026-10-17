//! Ícone na bandeja com o estado do mundo e um menu rápido.

use std::sync::Mutex;

use machencraft_core::coord::LeaseStatus;
use machencraft_core::session::{Phase, Status};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "pauletor3";

struct TrayItems {
    play: MenuItem<Wry>,
    pass: MenuItem<Wry>,
    last_icon: Mutex<&'static str>,
}

fn icon(kind: &str) -> Image<'static> {
    let bytes: &'static [u8] = match kind {
        "hosting" => include_bytes!("../icons/tray-hosting.png"),
        "busy" => include_bytes!("../icons/tray-busy.png"),
        "problem" => include_bytes!("../icons/tray-problem.png"),
        _ => include_bytes!("../icons/tray-idle.png"),
    };
    Image::from_bytes(bytes).expect("ícone da bandeja")
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let play = MenuItem::with_id(app, "play", "Jogar", true, None::<&str>)?;
    let pass = MenuItem::with_id(app, "pass", "Passar o host", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Abrir o Machen Craft", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&play, &pass, &open, &sep, &quit])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon("idle"))
        .tooltip("Machen Craft")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "play" => {
                crate::show_main(app);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(e) = app.state::<crate::AppState>().engine() {
                        let st = e.status();
                        let res = match st.lease {
                            Some(LeaseStatus::Held { lease }) if lease.handing_over => e.take_turn(true).await,
                            // alguém hospeda: Jogar só abre o Minecraft (o proxy leva até o host)
                            _ => e.play(true).await,
                        };
                        if let Err(err) = res {
                            tracing::warn!(error = %err, "Jogar pela bandeja falhou");
                        }
                    }
                });
            }
            "pass" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(e) = app.state::<crate::AppState>().engine() {
                        let _ = e.pass_turn().await;
                    }
                });
            }
            "open" => crate::show_main(app),
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayItems { play, pass, last_icon: Mutex::new("idle") });
    Ok(())
}

pub fn describe(st: &Status) -> String {
    if st.me_hosting {
        let n = st.online.len();
        return match (st.friends_still_playing, n) {
            (true, _) => "Seus amigos ainda estão jogando — não desligue o PC".into(),
            (_, 0) => "Você está hospedando · ninguém online".into(),
            (_, n) => format!("Você está hospedando · {n} online"),
        };
    }
    match st.phase {
        Phase::Preparing => return "Preparando o mundo…".into(),
        Phase::Starting => return "Abrindo o servidor…".into(),
        Phase::Stopping => return "Salvando o mundo… não desligue o PC".into(),
        Phase::Recovering => return "Recuperando a última sessão…".into(),
        Phase::Maintenance => return "Manutenção em andamento…".into(),
        Phase::WaitingTurn => return "Esperando a vez…".into(),
        _ => {}
    }
    if let Some(f) = &st.failover {
        return f.clone();
    }
    match &st.lease {
        Some(LeaseStatus::Held { lease }) => format!("{} está hospedando · {} online", lease.player, lease.online.len()),
        _ => "O mundo está dormindo — ninguém jogando".into(),
    }
}

pub fn update(app: &AppHandle, st: &Status) {
    let Some(items) = app.try_state::<TrayItems>() else { return };
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let kind: &'static str = if st.last_error.is_some() || st.recovery_pending {
        "problem"
    } else if st.me_hosting || matches!(&st.lease, Some(LeaseStatus::Held { lease }) if lease.purpose == machencraft_core::coord::LeasePurpose::Hosting) {
        // mundo aberto (aqui ou no PC de um amigo): ícone aceso
        "hosting"
    } else if !matches!(st.phase, Phase::Idle) {
        "busy"
    } else {
        "idle"
    };
    {
        let mut last = items.last_icon.lock().unwrap();
        if *last != kind {
            let _ = tray.set_icon(Some(icon(kind)));
            *last = kind;
        }
    }
    let _ = tray.set_tooltip(Some(format!("Machen Craft — {}", describe(st))));
    let idle = matches!(st.phase, Phase::Idle);
    let (play_text, play_on) = match &st.lease {
        _ if st.me_hosting => ("Abrir o Minecraft", true),
        Some(LeaseStatus::Held { lease }) if lease.handing_over => ("Assumir agora", idle),
        Some(LeaseStatus::Held { .. }) => ("Entrar", true),
        _ => ("Jogar", idle && st.configured),
    };
    let _ = items.play.set_text(play_text);
    let _ = items.play.set_enabled(play_on);
    let _ = items.pass.set_enabled(st.me_hosting && matches!(st.phase, Phase::Hosting));
}
