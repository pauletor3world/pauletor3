//! Código de convite: pacote criptografado com tudo que um amigo precisa para entrar.
//!
//! Formato: `P3.` + base64url( sal[16] | nonce[24] | texto cifrado )
//! - chave = Argon2id(senha, sal) com m = 64 MiB, t = 3, p = 1
//! - cifra = XChaCha20-Poly1305 (autenticada: senha errada ou código alterado = erro)
//!
//! A senha é passada separadamente (por voz, outra conversa etc.).

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const PREFIX: &str = "P3.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvitePayload {
    pub world_name: String,
    /// Backend do rclone (ex.: "drive") e parâmetros (ex.: token, scope, root_folder_id).
    pub remote_backend: String,
    pub remote_params: Vec<(String, String)>,
    /// Pasta raiz do mundo dentro do remote.
    pub remote_root: String,
    pub tailscale_auth_key: Option<String>,
    pub tailscale_key_expires: Option<chrono::NaiveDate>,
    pub duckdns_domain: Option<String>,
    pub duckdns_token: Option<String>,
    pub discord_webhook: Option<String>,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Drop for InvitePayload {
    fn drop(&mut self) {
        for (_, v) in self.remote_params.iter_mut() {
            v.zeroize();
        }
        if let Some(k) = self.tailscale_auth_key.as_mut() {
            k.zeroize();
        }
        if let Some(k) = self.duckdns_token.as_mut() {
            k.zeroize();
        }
        if let Some(k) = self.discord_webhook.as_mut() {
            k.zeroize();
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    #[error("isso não parece um código de convite do pAuletOr 3")]
    Format,
    #[error("senha errada (ou código incompleto)")]
    Decrypt,
    #[error("a senha precisa ter pelo menos 8 caracteres")]
    WeakPassword,
    #[error("erro interno: {0}")]
    Internal(String),
}

fn kdf(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, InviteError> {
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| InviteError::Internal(e.to_string()))?;
    let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    a.hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|e| InviteError::Internal(e.to_string()))?;
    Ok(key)
}

pub fn seal(payload: &InvitePayload, password: &str) -> Result<String, InviteError> {
    if password.chars().count() < 8 {
        return Err(InviteError::WeakPassword);
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce);
    let key = kdf(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&*key).map_err(|e| InviteError::Internal(e.to_string()))?;
    let plain = Zeroizing::new(serde_json::to_vec(payload).map_err(|e| InviteError::Internal(e.to_string()))?);
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), plain.as_slice())
        .map_err(|e| InviteError::Internal(e.to_string()))?;
    let mut blob = Vec::with_capacity(16 + 24 + ct.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    Ok(format!("{PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(blob)))
}

pub fn open(code: &str, password: &str) -> Result<InvitePayload, InviteError> {
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let body = compact.strip_prefix(PREFIX).ok_or(InviteError::Format)?;
    let blob = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(body).map_err(|_| InviteError::Format)?;
    if blob.len() < 16 + 24 + 16 {
        return Err(InviteError::Format);
    }
    let (salt, rest) = blob.split_at(16);
    let (nonce, ct) = rest.split_at(24);
    let key = kdf(password, salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&*key).map_err(|e| InviteError::Internal(e.to_string()))?;
    let plain = Zeroizing::new(cipher.decrypt(XNonce::from_slice(nonce), ct).map_err(|_| InviteError::Decrypt)?);
    serde_json::from_slice(&plain).map_err(|_| InviteError::Format)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InvitePayload {
        InvitePayload {
            world_name: "pAuletOr 3".into(),
            remote_backend: "drive".into(),
            remote_params: vec![("token".into(), "{\"access_token\":\"x\"}".into())],
            remote_root: "pAuletOr3".into(),
            tailscale_auth_key: Some("tskey-auth-abc".into()),
            tailscale_key_expires: None,
            duckdns_domain: Some("pauletor3".into()),
            duckdns_token: Some("dd".into()),
            discord_webhook: None,
            created_by: "Ana".into(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn seal_and_open() {
        let code = seal(&sample(), "senha-forte-123").unwrap();
        assert!(code.starts_with("P3."));
        assert!(!code.contains("tskey"), "nada em texto puro");
        let back = open(&code, "senha-forte-123").unwrap();
        assert_eq!(back.tailscale_auth_key.as_deref(), Some("tskey-auth-abc"));
        // aceita quebras de linha/espaços colados junto
        let spaced = format!("  {}\n{}  ", &code[..20], &code[20..]);
        assert!(open(&spaced, "senha-forte-123").is_ok());
    }

    #[test]
    fn wrong_password_and_tamper() {
        let code = seal(&sample(), "senha-forte-123").unwrap();
        assert!(matches!(open(&code, "senha-errada!"), Err(InviteError::Decrypt)));
        let mut chars: Vec<char> = code.chars().collect();
        let i = chars.len() - 5;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        assert!(open(&tampered, "senha-forte-123").is_err());
        assert!(matches!(open("oi", "x"), Err(InviteError::Format)));
        assert!(matches!(seal(&sample(), "curta"), Err(InviteError::WeakPassword)));
    }
}
