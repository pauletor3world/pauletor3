//! Bloqueio do desligamento do Windows enquanto o mundo é salvo.
//!
//! API: `ShutdownBlockReasonCreate(hwnd, motivo)` mostra "Salvando o mundo..." na tela de
//! desligamento; a janela também precisa responder FALSE a `WM_QUERYENDSESSION`.
//! `SetProcessShutdownParameters` coloca o app cedo na fila de desligamento.
//! A janela (HWND) é fornecida pela interface (Tauri).

#[cfg(windows)]
pub fn block(hwnd: isize, reason: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Shutdown::ShutdownBlockReasonCreate;
    unsafe { ShutdownBlockReasonCreate(HWND(hwnd as _), &HSTRING::from(reason)).is_ok() }
}

#[cfg(windows)]
pub fn unblock(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Shutdown::ShutdownBlockReasonDestroy;
    unsafe {
        let _ = ShutdownBlockReasonDestroy(HWND(hwnd as _));
    }
}

/// Pede para o app ser avisado cedo no desligamento (antes de outros apps comuns).
#[cfg(windows)]
pub fn request_early_notification() {
    use windows::Win32::System::Threading::SetProcessShutdownParameters;
    unsafe {
        // 0x3FF = nível mais alto para apps comuns; SHUTDOWN_NORETRY = 1
        let _ = SetProcessShutdownParameters(0x3FF, 0);
    }
}

#[cfg(not(windows))]
pub fn block(_hwnd: isize, _reason: &str) -> bool {
    false
}
#[cfg(not(windows))]
pub fn unblock(_hwnd: isize) {}
#[cfg(not(windows))]
pub fn request_early_notification() {}
