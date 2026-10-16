//! Remote em pasta local, puro Rust. Usado nos testes unitários e como modelo de referência.
//! Pode simular falhas (`fail_after`) para testar envios interrompidos.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use super::{ProgressFn, Remote, RemoteEntry, TransferProgress};
use crate::{util, NmError, NmResult};

#[derive(Clone)]
pub struct FsRemote {
    root: PathBuf,
    /// Quantas transferências de objeto ainda podem dar certo antes de falhar. Negativo = sem limite.
    budget: Arc<AtomicI64>,
}

impl FsRemote {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), budget: Arc::new(AtomicI64::new(-1)) }
    }

    /// Faz a N-ésima transferência de objeto em diante falhar (simula queda de internet).
    pub fn fail_after(&self, n: i64) {
        self.budget.store(n, Ordering::SeqCst);
    }

    pub fn heal(&self) {
        self.budget.store(-1, Ordering::SeqCst);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn p(&self, rel: &str) -> NmResult<PathBuf> {
        util::join_rel(&self.root, rel).map_err(NmError::Other)
    }

    fn spend(&self) -> NmResult<()> {
        let b = self.budget.load(Ordering::SeqCst);
        if b < 0 {
            return Ok(());
        }
        if b == 0 {
            return Err(NmError::Remote("falha simulada no meio do envio".into()));
        }
        self.budget.store(b - 1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl Remote for FsRemote {
    async fn read(&self, path: &str) -> NmResult<Option<Vec<u8>>> {
        match std::fs::read(self.p(path)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn write(&self, path: &str, data: &[u8]) -> NmResult<()> {
        util::write_atomic(&self.p(path)?, data)?;
        Ok(())
    }

    async fn delete(&self, path: &str) -> NmResult<()> {
        match std::fs::remove_file(self.p(path)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn list(&self, dir: &str) -> NmResult<Vec<RemoteEntry>> {
        let d = self.p(dir)?;
        let rd = match std::fs::read_dir(&d) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut out = vec![];
        for e in rd {
            let e = e?;
            let meta = e.metadata()?;
            let name = e.file_name().to_string_lossy().into_owned();
            if meta.is_file() && !name.starts_with('.') {
                out.push(RemoteEntry { name, size: meta.len() });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn upload_many(&self, local_dir: &Path, names: &[String], remote_dir: &str, progress: ProgressFn) -> NmResult<()> {
        let total: u64 = names
            .iter()
            .map(|n| std::fs::metadata(local_dir.join(n)).map(|m| m.len()).unwrap_or(0))
            .sum();
        let mut p = TransferProgress { bytes_total: total, files_total: names.len() as u64, ..Default::default() };
        for n in names {
            self.spend()?;
            let src = local_dir.join(n);
            let dst = self.p(&format!("{remote_dir}/{n}"))?;
            std::fs::create_dir_all(dst.parent().unwrap())?;
            let tmp = dst.with_file_name(format!(".{n}.partial"));
            std::fs::copy(&src, &tmp)?;
            util::replace_file(&tmp, &dst)?;
            p.bytes_done += std::fs::metadata(&dst)?.len();
            p.files_done += 1;
            progress(p);
        }
        Ok(())
    }

    async fn download_many(&self, remote_dir: &str, names: &[String], local_dir: &Path, progress: ProgressFn) -> NmResult<()> {
        std::fs::create_dir_all(local_dir)?;
        let total: u64 = names
            .iter()
            .filter_map(|n| self.p(&format!("{remote_dir}/{n}")).ok())
            .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
            .sum();
        let mut p = TransferProgress { bytes_total: total, files_total: names.len() as u64, ..Default::default() };
        for n in names {
            self.spend()?;
            let src = self.p(&format!("{remote_dir}/{n}"))?;
            let dst = local_dir.join(n);
            let tmp = local_dir.join(format!(".{n}.partial"));
            std::fs::copy(&src, &tmp).map_err(|e| NmError::Remote(format!("{n}: {e}")))?;
            util::replace_file(&tmp, &dst)?;
            p.bytes_done += std::fs::metadata(&dst)?.len();
            p.files_done += 1;
            progress(p);
        }
        Ok(())
    }

    async fn delete_many(&self, dir: &str, names: &[String]) -> NmResult<()> {
        for n in names {
            self.delete(&format!("{dir}/{n}")).await?;
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("pasta local {}", self.root.display())
    }
}
