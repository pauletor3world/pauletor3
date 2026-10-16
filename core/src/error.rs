use serde::Serialize;

use crate::lock::LockInfo;

pub type NmResult<T> = Result<T, NmError>;

/// Erros com significado para o usuário. Cada variante vira uma mensagem humana na
/// interface (`human()`), e `Other` carrega a cadeia técnica para o diagnóstico.
#[derive(Debug, thiserror::Error)]
pub enum NmError {
    #[error("o mundo está sendo hospedado por {}", .0.player)]
    HeldBy(Box<LockInfo>),

    #[error("a vez de {} parece abandonada", .0.player)]
    StaleLock(Box<LockInfo>),

    #[error("perdemos a vez de hospedar (outro computador assumiu)")]
    LostLock,

    #[error("outro computador pediu a vez no mesmo instante")]
    LockRace,

    #[error("o mundo usa um formato mais novo ({found}); atualize o app")]
    FormatTooNew { found: u32 },

    #[error("versões do servidor não conferem: {0}")]
    VersionMismatch(String),

    #[error("falha de integridade: {0}")]
    Integrity(String),

    #[error("o remote não respondeu: {0}")]
    Remote(String),

    #[error("o Tailscale não está conectado")]
    TailscaleDown,

    #[error("ainda não configurado: {0}")]
    NotConfigured(String),

    #[error("operação cancelada")]
    Cancelled,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<std::io::Error> for NmError {
    fn from(e: std::io::Error) -> Self {
        NmError::Other(e.into())
    }
}

impl From<serde_json::Error> for NmError {
    fn from(e: serde_json::Error) -> Self {
        NmError::Other(e.into())
    }
}

/// Forma serializável do erro, usada pela interface.
#[derive(Debug, Clone, Serialize)]
pub struct HumanError {
    pub code: &'static str,
    pub title: String,
    pub detail: String,
    pub lock: Option<LockInfo>,
}

impl NmError {
    pub fn code(&self) -> &'static str {
        match self {
            NmError::HeldBy(_) => "held_by",
            NmError::StaleLock(_) => "stale_lock",
            NmError::LostLock => "lost_lock",
            NmError::LockRace => "lock_race",
            NmError::FormatTooNew { .. } => "format_too_new",
            NmError::VersionMismatch(_) => "version_mismatch",
            NmError::Integrity(_) => "integrity",
            NmError::Remote(_) => "remote",
            NmError::TailscaleDown => "tailscale_down",
            NmError::NotConfigured(_) => "not_configured",
            NmError::Cancelled => "cancelled",
            NmError::Other(_) => "other",
        }
    }

    pub fn human(&self) -> HumanError {
        let (title, lock) = match self {
            NmError::HeldBy(l) => (format!("{} já está hospedando o mundo", l.player), Some((**l).clone())),
            NmError::StaleLock(l) => (
                format!("A sessão de {} parece ter caído", l.player),
                Some((**l).clone()),
            ),
            NmError::LostLock => ("Outro computador assumiu o mundo".into(), None),
            NmError::LockRace => ("Alguém clicou em Jogar no mesmo instante".into(), None),
            NmError::FormatTooNew { .. } => ("Atualize o app para continuar".into(), None),
            NmError::VersionMismatch(_) => ("As versões do servidor não batem".into(), None),
            NmError::Integrity(_) => ("Um arquivo do mundo veio diferente do esperado".into(), None),
            NmError::Remote(_) => ("Não consegui falar com a nuvem".into(), None),
            NmError::TailscaleDown => ("O Tailscale está desconectado".into(), None),
            NmError::NotConfigured(_) => ("Falta terminar a configuração".into(), None),
            NmError::Cancelled => ("Cancelado".into(), None),
            NmError::Other(_) => ("Algo deu errado".into(), None),
        };
        HumanError {
            code: self.code(),
            title,
            detail: format!("{:#}", anyhow::anyhow!("{self}")).replace('\n', " "),
            lock,
        }
    }
}
