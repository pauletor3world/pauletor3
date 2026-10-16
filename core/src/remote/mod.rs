//! Abstração do armazenamento da nuvem do mundo. O núcleo só fala com "um remote" que sabe
//! ler/gravar arquivos e transferir objetos em lote. Implementações: [`CloudRemote`] (o Worker
//! da coordenação), [`FsRemote`] (pasta local, testes) e [`EncryptedRemote`] (cifra por cima
//! de qualquer outra com a chave do mundo).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::NmResult;

pub mod cloud;
pub mod crypt;
pub mod fs;

pub use cloud::CloudRemote;
pub use crypt::EncryptedRemote;
pub use fs::FsRemote;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEntry {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
}

pub type ProgressFn = Arc<dyn Fn(TransferProgress) + Send + Sync>;

pub fn no_progress() -> ProgressFn {
    Arc::new(|_| {})
}

/// Caminhos no remote são sempre relativos à raiz do mundo e usam `/`.
#[async_trait]
pub trait Remote: Send + Sync {
    /// Lê um arquivo pequeno. `None` se não existir.
    async fn read(&self, path: &str) -> NmResult<Option<Vec<u8>>>;
    /// Grava um arquivo pequeno (substitui se existir).
    async fn write(&self, path: &str, data: &[u8]) -> NmResult<()>;
    /// Apaga um arquivo. Não é erro se não existir.
    async fn delete(&self, path: &str) -> NmResult<()>;
    /// Lista os arquivos (não recursivo) de uma pasta. Vazio se a pasta não existir.
    async fn list(&self, dir: &str) -> NmResult<Vec<RemoteEntry>>;
    /// Envia `names` (arquivos de `local_dir`) para `remote_dir`, em lote.
    async fn upload_many(&self, local_dir: &Path, names: &[String], remote_dir: &str, progress: ProgressFn) -> NmResult<()>;
    /// Baixa `names` de `remote_dir` para `local_dir`, em lote.
    async fn download_many(&self, remote_dir: &str, names: &[String], local_dir: &Path, progress: ProgressFn) -> NmResult<()>;
    /// Apaga vários arquivos de uma pasta, em lote.
    async fn delete_many(&self, dir: &str, names: &[String]) -> NmResult<()>;
    /// Uma descrição curta (para logs e diagnóstico, sem segredos).
    fn describe(&self) -> String;
}

/// Nomes dos arquivos e pastas no remote.
pub mod layout {
    pub const OBJECTS: &str = "objects";
    pub const MANIFESTS: &str = "manifests";
    pub const VERSIONS: &str = "versions.json";
    pub const WORLD_SETTINGS: &str = "world-settings.json";
    pub const STATS: &str = "stats.json";

    pub fn manifest(id: &str) -> String {
        format!("{MANIFESTS}/{id}.json")
    }
}

pub async fn read_json<T: serde::de::DeserializeOwned>(r: &dyn Remote, path: &str) -> NmResult<Option<T>> {
    match r.read(path).await? {
        None => Ok(None),
        Some(b) => Ok(Some(serde_json::from_slice(&b).map_err(|e| {
            crate::NmError::Integrity(format!("{path} ilegível: {e}"))
        })?)),
    }
}

pub async fn write_json<T: Serialize>(r: &dyn Remote, path: &str, v: &T) -> NmResult<()> {
    r.write(path, &serde_json::to_vec_pretty(v)?).await
}
