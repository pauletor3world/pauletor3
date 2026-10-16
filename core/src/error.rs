use serde::Serialize;

use crate::coord::LeaseInfo;

pub type NmResult<T> = Result<T, NmError>;

/// Erros com significado para o usuário. Cada variante vira uma mensagem humana na
/// interface (`human()`), e `Other` carrega a cadeia técnica para o diagnóstico.
#[derive(Debug, thiserror::Error)]
pub enum NmError {
    #[error("o mundo está sendo hospedado por {}", .0.player)]
    HeldBy(Box<LeaseInfo>),

    #[error("outro membro tem uma cópia mais nova do mundo (versão {best_seq}) e tem a preferência")]
    NotBestCandidate { best_seq: u64 },

    #[error("perdemos a vez de hospedar (outro computador assumiu)")]
    LostLock,

    #[error("a versão oficial do mundo mudou no meio do caminho (agora é {current:?})")]
    HeadMoved { current: Option<String> },

    #[error("o mundo usa um formato mais novo ({found}); atualize o app")]
    FormatTooNew { found: u32 },

    #[error("versões do servidor não conferem: {0}")]
    VersionMismatch(String),

    #[error("falha de integridade: {0}")]
    Integrity(String),

    #[error("a nuvem não respondeu: {0}")]
    Remote(String),

    #[error("sem permissão: {0}")]
    Forbidden(String),

    #[error("conexão P2P: {0}")]
    P2p(String),

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
    pub lease: Option<LeaseInfo>,
}

impl NmError {
    pub fn code(&self) -> &'static str {
        match self {
            NmError::HeldBy(_) => "held_by",
            NmError::NotBestCandidate { .. } => "not_best_candidate",
            NmError::LostLock => "lost_lock",
            NmError::HeadMoved { .. } => "head_moved",
            NmError::FormatTooNew { .. } => "format_too_new",
            NmError::VersionMismatch(_) => "version_mismatch",
            NmError::Integrity(_) => "integrity",
            NmError::Remote(_) => "remote",
            NmError::Forbidden(_) => "forbidden",
            NmError::P2p(_) => "p2p",
            NmError::NotConfigured(_) => "not_configured",
            NmError::Cancelled => "cancelled",
            NmError::Other(_) => "other",
        }
    }

    /// Erros transitórios: vale tentar de novo sozinho.
    pub fn is_transient(&self) -> bool {
        matches!(self, NmError::Remote(_) | NmError::P2p(_) | NmError::NotBestCandidate { .. })
    }

    pub fn human(&self) -> HumanError {
        let (title, lease) = match self {
            NmError::HeldBy(l) => (format!("{} já está hospedando o mundo", l.player), Some((**l).clone())),
            NmError::NotBestCandidate { .. } => ("Um amigo com o mundo mais atualizado está assumindo".into(), None),
            NmError::LostLock => ("Outro computador assumiu o mundo".into(), None),
            NmError::HeadMoved { .. } => ("O mundo mudou enquanto você salvava".into(), None),
            NmError::FormatTooNew { .. } => ("Atualize o app para continuar".into(), None),
            NmError::VersionMismatch(_) => ("As versões do servidor não batem".into(), None),
            NmError::Integrity(_) => ("Um arquivo do mundo veio diferente do esperado".into(), None),
            NmError::Remote(_) => ("Não consegui falar com a nuvem do mundo".into(), None),
            NmError::Forbidden(_) => ("Você ainda não tem acesso a isso".into(), None),
            NmError::P2p(_) => ("Não consegui conectar com o PC do amigo".into(), None),
            NmError::NotConfigured(_) => ("Falta terminar a configuração".into(), None),
            NmError::Cancelled => ("Cancelado".into(), None),
            NmError::Other(_) => ("Algo deu errado".into(), None),
        };
        HumanError {
            code: self.code(),
            title,
            detail: format!("{:#}", anyhow::anyhow!("{self}")).replace('\n', " "),
            lease,
        }
    }
}
