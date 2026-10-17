//! Convite por link. Um amigo recebe **um link** (pelo WhatsApp, por exemplo), abre, baixa o
//! app e entra. Sem senha, sem conta, sem configurar nada.
//!
//! ```text
//! https://<coordenação>/c/<mundo>.<segredo>#<chave do mundo, base64url>
//! ```
//! - `<mundo>.<segredo>` é o token do convite: a coordenação guarda só o hash do segredo,
//!   com validade e limite de usos, e o dono pode revogar.
//! - O que vem depois do `#` (a chave que cifra a nuvem do mundo) **nunca é enviado ao
//!   servidor** pelo navegador: a página de convite repassa para o app.
//!
//! Para colar no app (quando o link não abre sozinho) existe a forma de código:
//! `P3J1.` + base64url(JSON `{u, t, k}`).

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const CODE_PREFIX: &str = "P3J1.";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    /// URL base da coordenação (ex.: `https://pauletor3.exemplo.workers.dev`).
    #[serde(rename = "u")]
    pub base: String,
    /// `<mundo>.<segredo>`.
    #[serde(rename = "t")]
    pub token: String,
    /// Chave do mundo (base64url de 32 bytes).
    #[serde(rename = "k")]
    pub key: String,
}

impl std::fmt::Debug for Invite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invite").field("base", &self.base).field("world", &self.world_id()).finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum InviteError {
    #[error("isso não parece um convite do Machenia")]
    Format,
}

impl Invite {
    pub fn world_id(&self) -> &str {
        self.token.split('.').next().unwrap_or_default()
    }

    pub fn world_key(&self) -> Result<Zeroizing<[u8; 32]>, InviteError> {
        let v = Zeroizing::new(B64.decode(&self.key).map_err(|_| InviteError::Format)?);
        let arr: [u8; 32] = v.as_slice().try_into().map_err(|_| InviteError::Format)?;
        Ok(Zeroizing::new(arr))
    }

    pub fn link(&self) -> String {
        format!("{}/c/{}#{}", self.base.trim_end_matches('/'), self.token, self.key)
    }

    pub fn code(&self) -> String {
        format!("{CODE_PREFIX}{}", B64.encode(serde_json::to_vec(self).unwrap_or_default()))
    }

    pub fn new(base: &str, token: &str, world_key: &[u8; 32]) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), token: token.to_string(), key: B64.encode(world_key) }
    }

    /// Aceita o link, o código `P3J1.` ou o deep link `machenia://entrar/<código>` (ou os antigos `machencraft://` e `pauletor3://`).
    pub fn parse(input: &str) -> Result<Self, InviteError> {
        let s = input.trim().trim_matches(|c| c == '<' || c == '>' || c == '"');
        let s = ["machenia://entrar/", "machencraft://entrar/", "pauletor3://entrar/"].iter().find_map(|p| s.strip_prefix(p)).unwrap_or(s);
        if let Some(rest) = s.strip_prefix(CODE_PREFIX) {
            let bytes = B64.decode(rest.trim_end_matches('/')).map_err(|_| InviteError::Format)?;
            let inv: Invite = serde_json::from_slice(&bytes).map_err(|_| InviteError::Format)?;
            inv.validate()?;
            return Ok(inv);
        }
        // https://host/c/<token>#<key>
        let (before, key) = s.split_once('#').ok_or(InviteError::Format)?;
        let idx = before.find("/c/").ok_or(InviteError::Format)?;
        let base = &before[..idx];
        let token = before[idx + 3..].trim_end_matches('/');
        if !(base.starts_with("https://") || base.starts_with("http://127.0.0.1") || base.starts_with("http://localhost")) {
            return Err(InviteError::Format);
        }
        let inv = Invite { base: base.to_string(), token: token.to_string(), key: key.to_string() };
        inv.validate()?;
        Ok(inv)
    }

    fn validate(&self) -> Result<(), InviteError> {
        let ok_token = self.token.split_once('.').map(|(w, s)| {
            !w.is_empty() && !s.is_empty() && self.token.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        });
        if ok_token != Some(true) {
            return Err(InviteError::Format);
        }
        self.world_key()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_code_and_deeplink_roundtrip() {
        let inv = Invite::new("https://p3.exemplo.workers.dev/", "w123.segredoABC", &[9u8; 32]);
        let link = inv.link();
        assert!(link.starts_with("https://p3.exemplo.workers.dev/c/w123.segredoABC#"));
        assert_eq!(Invite::parse(&link).unwrap(), inv);
        assert_eq!(Invite::parse(&inv.code()).unwrap(), inv);
        assert_eq!(Invite::parse(&format!("machenia://entrar/{}", inv.code())).unwrap(), inv);
        assert_eq!(Invite::parse(&format!("machencraft://entrar/{}", inv.code())).unwrap(), inv);
        assert_eq!(Invite::parse(&format!("pauletor3://entrar/{}", inv.code())).unwrap(), inv);
        assert_eq!(inv.world_id(), "w123");
        assert_eq!(*inv.world_key().unwrap(), [9u8; 32]);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Invite::parse("oi"), Err(InviteError::Format));
        assert_eq!(Invite::parse("https://x/c/semponto#AAAA"), Err(InviteError::Format));
        assert_eq!(Invite::parse("ftp://x/c/a.b#AAAA"), Err(InviteError::Format));
        // chave curta
        assert_eq!(Invite::parse("https://x/c/a.b#AAAA"), Err(InviteError::Format));
    }
}
