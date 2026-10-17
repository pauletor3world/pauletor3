//! Desligamento do Windows durante uma sessão: bloqueia com o motivo "Salvando o mundo…",
//! encerra a sessão (para o servidor, envia o mundo, libera a vez) e só então deixa o PC desligar.
//!
//! A janela principal recebe `WM_QUERYENDSESSION` (inclusive escondida). Respondemos FALSE e
//! registramos o motivo com `ShutdownBlockReasonCreate`; o Windows mostra a tela de
//! "aplicativos impedindo o desligamento" com o nosso texto.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;

use machencraft_core::session::EndReason;
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::WM_QUERYENDSESSION;

static APP: OnceLock<AppHandle> = OnceLock::new();
static BUSY: AtomicBool = AtomicBool::new(false);
static SAVING: AtomicBool = AtomicBool::new(false);
static HWND_VAL: AtomicIsize = AtomicIsize::new(0);

pub fn set_busy(b: bool) {
    BUSY.store(b, Ordering::SeqCst);
}

pub fn install(app: AppHandle, hwnd: isize) {
    let _ = APP.set(app);
    HWND_VAL.store(hwnd, Ordering::SeqCst);
    unsafe {
        let _ = SetWindowSubclass(HWND(hwnd as _), Some(subclass), 0x5033, 0);
    }
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if msg == WM_QUERYENDSESSION && BUSY.load(Ordering::SeqCst) {
        machencraft_core::shutdown::block(hwnd.0 as isize, "Machen Craft está salvando o mundo… não force o desligamento.");
        if !SAVING.swap(true, Ordering::SeqCst) {
            if let Some(app) = APP.get() {
                let app = app.clone();
                crate::show_main(&app);
                tauri::async_runtime::spawn(async move {
                    if let Some(engine) = app.state::<crate::AppState>().engine() {
                        let _ = engine.stop(EndReason::Shutdown).await;
                    }
                    machencraft_core::shutdown::unblock(HWND_VAL.load(Ordering::SeqCst));
                    // Com o mundo salvo, o app sai e o Windows segue com o desligamento.
                    app.state::<crate::AppState>().quitting.store(true, Ordering::SeqCst);
                    app.exit(0);
                });
            }
        }
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
