//! Cifra tudo o que vai para a nuvem com a **chave do mundo** (XChaCha20-Poly1305).
//!
//! A chave nasce no PC do dono e só viaja no convite (no fragmento `#...` do link, que o
//! navegador nunca envia ao servidor). A coordenação e o armazenamento guardam apenas bytes
//! cifrados; os nomes dos objetos continuam sendo o sha256 do conteúdo original, então a
//! deduplicação e a conferência de hash funcionam igual.
//!
//! Formato: `P3E1` | nonce (24 bytes) | texto cifrado + tag (16 bytes).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroizing;

use super::{ProgressFn, Remote, RemoteEntry};
use crate::{NmError, NmResult};

const MAGIC: &[u8; 4] = b"P3E1";
/// Bytes a mais por arquivo cifrado.
pub const OVERHEAD: u64 = 4 + 24 + 16;

pub struct EncryptedRemote {
    inner: Arc<dyn Remote>,
    key: Zeroizing<[u8; 32]>,
    tmp: std::path::PathBuf,
}

impl EncryptedRemote {
    pub fn new(inner: Arc<dyn Remote>, key: [u8; 32], tmp: std::path::PathBuf) -> Self {
        Self { inner, key: Zeroizing::new(key), tmp }
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(self.key.as_ref().into())
    }

    pub fn seal(&self, plain: &[u8]) -> NmResult<Vec<u8>> {
        let mut nonce = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = self
            .cipher()
            .encrypt(XNonce::from_slice(&nonce), plain)
            .map_err(|_| NmError::Other(anyhow::anyhow!("falha ao cifrar")))?;
        let mut out = Vec::with_capacity(plain.len() + OVERHEAD as usize);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend(ct);
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> NmResult<Vec<u8>> {
        if sealed.len() < OVERHEAD as usize || &sealed[..4] != MAGIC {
            return Err(NmError::Integrity("arquivo da nuvem não está no formato cifrado".into()));
        }
        self.cipher()
            .decrypt(XNonce::from_slice(&sealed[4..28]), &sealed[28..])
            .map_err(|_| NmError::Integrity("arquivo da nuvem não abre com a chave do mundo (chave errada ou arquivo alterado)".into()))
    }
}

#[async_trait]
impl Remote for EncryptedRemote {
    async fn read(&self, path: &str) -> NmResult<Option<Vec<u8>>> {
        match self.inner.read(path).await? {
            Some(b) => Ok(Some(self.open(&b)?)),
            None => Ok(None),
        }
    }

    async fn write(&self, path: &str, data: &[u8]) -> NmResult<()> {
        self.inner.write(path, &self.seal(data)?).await
    }

    async fn delete(&self, path: &str) -> NmResult<()> {
        self.inner.delete(path).await
    }

    async fn list(&self, dir: &str) -> NmResult<Vec<RemoteEntry>> {
        Ok(self
            .inner
            .list(dir)
            .await?
            .into_iter()
            .map(|e| RemoteEntry { name: e.name, size: e.size.saturating_sub(OVERHEAD) })
            .collect())
    }

    async fn upload_many(&self, local_dir: &Path, names: &[String], remote_dir: &str, progress: ProgressFn) -> NmResult<()> {
        // cifra em lotes para não ocupar o disco com uma cópia inteira do mundo
        let staging = self.tmp.join(format!("cifrar-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&staging)?;
        let res = async {
            for chunk in names.chunks(64) {
                for n in chunk {
                    let plain = Zeroizing::new(tokio::fs::read(local_dir.join(n)).await?);
                    tokio::fs::write(staging.join(n), self.seal(&plain)?).await?;
                }
                self.inner.upload_many(&staging, chunk, remote_dir, progress.clone()).await?;
                for n in chunk {
                    let _ = std::fs::remove_file(staging.join(n));
                }
            }
            Ok::<_, NmError>(())
        }
        .await;
        let _ = crate::util::remove_dir_all_retry(&staging);
        res
    }

    async fn download_many(&self, remote_dir: &str, names: &[String], local_dir: &Path, progress: ProgressFn) -> NmResult<()> {
        self.inner.download_many(remote_dir, names, local_dir, progress).await?;
        for n in names {
            let p = local_dir.join(n);
            let sealed = tokio::fs::read(&p).await?;
            match self.open(&sealed) {
                Ok(plain) => crate::util::write_atomic(&p, &plain)?,
                // quem chamou confere o hash e baixa de novo; aqui só não deixa lixo cifrado
                Err(e) => {
                    tracing::warn!(name = %n, error = %e, "objeto da nuvem não abriu");
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        Ok(())
    }

    async fn delete_many(&self, dir: &str, names: &[String]) -> NmResult<()> {
        self.inner.delete_many(dir, names).await
    }

    fn describe(&self) -> String {
        format!("{} (cifrada)", self.inner.describe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::FsRemote;

    #[tokio::test]
    async fn roundtrip_and_ciphertext_at_rest() {
        let d = tempfile::tempdir().unwrap();
        let fs = FsRemote::new(d.path().join("nuvem"));
        let r = EncryptedRemote::new(Arc::new(fs.clone()), [7u8; 32], d.path().join("tmp"));
        r.write("versions.json", b"{\"segredo\":1}").await.unwrap();
        let raw = fs.read("versions.json").await.unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("segredo"));
        assert_eq!(r.read("versions.json").await.unwrap().unwrap(), b"{\"segredo\":1}");

        let local = d.path().join("local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("a"), vec![1u8; 1000]).unwrap();
        r.upload_many(&local, &["a".into()], "objects", crate::remote::no_progress()).await.unwrap();
        assert_eq!(r.list("objects").await.unwrap()[0].size, 1000, "tamanho lógico");
        let out = d.path().join("out");
        r.download_many("objects", &["a".into()], &out, crate::remote::no_progress()).await.unwrap();
        assert_eq!(std::fs::read(out.join("a")).unwrap(), vec![1u8; 1000]);

        // chave errada não abre
        let wrong = EncryptedRemote::new(Arc::new(fs), [8u8; 32], d.path().join("tmp"));
        assert!(matches!(wrong.read("versions.json").await, Err(NmError::Integrity(_))));
    }
}
