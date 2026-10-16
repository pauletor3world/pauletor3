//! Identidade do jogador e licença, atrás de interfaces.
//!
//! Hoje (build pessoal): o jogador é o **nick** que ele escolhe e o PC é identificado pela
//! chave do app; criar mundo exige a chave de administração do Worker do dono. Quando houver
//! uma versão vendida, entram aqui um `MicrosoftIdentity` (login Microsoft → UUID e posse do
//! jogo) e uma licença paga, sem mexer no resto do núcleo.

use serde::{Deserialize, Serialize};

/// Quem é o jogador, do ponto de vista do servidor de Minecraft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerId {
    pub nick: String,
    /// UUID do jogador no servidor, quando conhecido.
    pub uuid: Option<String>,
}

pub trait IdentityProvider: Send + Sync {
    /// Nome curto para logs e diagnóstico.
    fn kind(&self) -> &'static str;
    /// O jogador deste PC.
    fn player(&self) -> PlayerId;
    /// UUID que o servidor vai usar para um membro (para a whitelist). `None` = desconhecido.
    fn uuid_for(&self, nick: &str) -> Option<String>;
}

/// Build pessoal: identidade = nick; UUID offline (o que o servidor em modo offline calcula).
pub struct NickIdentity {
    pub nick: String,
}

impl IdentityProvider for NickIdentity {
    fn kind(&self) -> &'static str {
        "nick"
    }
    fn player(&self) -> PlayerId {
        PlayerId { nick: self.nick.clone(), uuid: self.uuid_for(&self.nick) }
    }
    fn uuid_for(&self, nick: &str) -> Option<String> {
        if crate::OFFLINE_MODE {
            Some(crate::players::offline_uuid(nick))
        } else {
            // Com conta original o UUID vem da Microsoft; sem o login Microsoft (ainda não
            // implementado) não dá para saber, e a whitelist fica desligada.
            None
        }
    }
}

pub fn identity(nick: &str) -> Box<dyn IdentityProvider> {
    Box::new(NickIdentity { nick: nick.to_string() })
}

/// Limites de uso (por mundo).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    pub max_worlds: u32,
    pub max_members: u32,
}

pub trait License: Send + Sync {
    fn kind(&self) -> &'static str;
    fn limits(&self) -> Limits;
    /// Credencial para criar mundos na coordenação, se este PC tiver.
    fn create_world_credential(&self) -> Option<String>;
}

/// Build pessoal: quem publicou o Worker tem a chave de administração; não há cobrança.
pub struct AdminTokenLicense {
    pub admin_token: Option<String>,
}

impl License for AdminTokenLicense {
    fn kind(&self) -> &'static str {
        "pessoal"
    }
    fn limits(&self) -> Limits {
        Limits { max_worlds: 3, max_members: 10 }
    }
    fn create_world_credential(&self) -> Option<String> {
        self.admin_token.clone()
    }
}
