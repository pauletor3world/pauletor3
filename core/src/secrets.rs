//! Segredos locais, protegidos com DPAPI do Windows (atrelados ao usuário do Windows).
//! Nunca em texto puro no disco. Fora do Windows (testes), usa um arquivo marcado como inseguro.

use std::path::Path;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Secrets {
    /// Senha que criptografa a config do rclone (gerada pelo app).
    pub rclone_config_pass: Option<String>,
    pub tailscale_auth_key: Option<String>,
    pub duckdns_token: Option<String>,
    pub discord_webhook: Option<String>,
    /// Backend do rclone e parâmetros do remote (JSON), para gerar convites.
    pub remote_backend: Option<String>,
    pub remote_params_json: Option<String>,
}

const MAGIC_DPAPI: &[u8] = b"NMDPAPI1";
const MAGIC_PLAIN: &[u8] = b"NMPLAIN1";

impl Secrets {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut plain = if let Some(rest) = raw.strip_prefix(MAGIC_DPAPI) {
            unprotect(rest)?
        } else if let Some(rest) = raw.strip_prefix(MAGIC_PLAIN) {
            if cfg!(windows) {
                anyhow::bail!("arquivo de segredos sem proteção encontrado no Windows; recusando");
            }
            rest.to_vec()
        } else {
            anyhow::bail!("arquivo de segredos irreconhecível");
        };
        let s = serde_json::from_slice(&plain);
        plain.zeroize();
        Ok(s?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let mut plain = serde_json::to_vec(self)?;
        let out = if cfg!(windows) {
            let mut v = MAGIC_DPAPI.to_vec();
            v.extend(protect(&plain)?);
            v
        } else {
            let mut v = MAGIC_PLAIN.to_vec();
            v.extend_from_slice(&plain);
            v
        };
        plain.zeroize();
        crate::util::write_atomic(path, &out)
    }
}

#[cfg(windows)]
fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, windows::core::w!("pAuletOr3"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as _));
        Ok(v)
    }
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|e| anyhow::anyhow!("não consegui abrir os segredos (outro usuário do Windows?): {e}"))?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as _));
        Ok(v)
    }
}

#[cfg(not(windows))]
fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_not_plaintext() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.bin");
        let mut s = Secrets::default();
        s.duckdns_token = Some("token-super-secreto".into());
        s.save(&p).unwrap();
        let raw = std::fs::read(&p).unwrap();
        if cfg!(windows) {
            assert!(!String::from_utf8_lossy(&raw).contains("token-super-secreto"), "cifrado no disco");
        }
        assert_eq!(Secrets::load(&p).unwrap().duckdns_token.as_deref(), Some("token-super-secreto"));
        assert!(Secrets::load(&d.path().join("nao-existe")).unwrap().duckdns_token.is_none());
    }
}
