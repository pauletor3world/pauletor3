//! Eventos que o núcleo emite para a interface (e para a CLI).

use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    Scanning,
    Uploading,
    Downloading,
    Verifying,
    Assembling,
    Publishing,
}

impl SyncPhase {
    pub fn label(self) -> &'static str {
        match self {
            SyncPhase::Scanning => "Conferindo arquivos",
            SyncPhase::Uploading => "Enviando",
            SyncPhase::Downloading => "Baixando",
            SyncPhase::Verifying => "Verificando",
            SyncPhase::Assembling => "Montando o mundo",
            SyncPhase::Publishing => "Publicando versão",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SyncProgress {
    pub phase: SyncPhase,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
}

impl SyncProgress {
    pub fn percent(&self) -> f64 {
        if self.bytes_total == 0 {
            if self.files_total == 0 {
                0.0
            } else {
                self.files_done as f64 * 100.0 / self.files_total as f64
            }
        } else {
            (self.bytes_done as f64 * 100.0 / self.bytes_total as f64).min(100.0)
        }
    }
}

pub type SyncProgressFn = Arc<dyn Fn(SyncProgress) + Send + Sync>;

pub fn quiet() -> SyncProgressFn {
    Arc::new(|_| {})
}

/// Eventos de alto nível da sessão.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Mudou alguma coisa no estado geral; a interface deve reler o status.
    StatusChanged,
    Sync(SyncProgress),
    /// Linha do console do servidor.
    ServerLog { line: String },
    PlayerJoined { name: String },
    PlayerLeft { name: String },
    /// Mensagem curta para notificação/toast.
    Notice { level: NoticeLevel, title: String, body: String },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

pub fn no_events() -> EventSink {
    Arc::new(|_| {})
}
