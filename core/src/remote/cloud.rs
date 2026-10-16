//! A nuvem do mundo, servida pelo Worker da coordenação (`/v1/worlds/:id/blob/...`).
//! Por trás do Worker o armazenamento pode ser o próprio Durable Object (padrão, sem cartão)
//! ou um bucket R2 — o app não sabe nem precisa saber.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde::Serialize;

use super::{ProgressFn, Remote, RemoteEntry, TransferProgress};
use crate::coord::http::{ApiClient, Body};
use crate::{NmError, NmResult};

/// Transferências em paralelo (o Worker grátis permite 6 conexões por pedido; do lado do
/// cliente o limite é só educação com a nuvem e com a internet de casa).
const PARALLEL: usize = 4;

#[derive(Clone, Debug)]
pub struct CloudRemote {
    api: ApiClient,
}

impl CloudRemote {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    fn blob(&self, path: &str) -> NmResult<String> {
        if path.split('/').any(|p| p.is_empty() || p == ".." || p == ".") || path.contains('\\') {
            return Err(NmError::Integrity(format!("caminho inválido na nuvem: {path}")));
        }
        Ok(self.api.world_path(&format!("blob/{path}")))
    }
}

#[async_trait]
impl Remote for CloudRemote {
    async fn read(&self, path: &str) -> NmResult<Option<Vec<u8>>> {
        let resp = self.api.send(reqwest::Method::GET, &self.blob(path)?, Body::None, &[]).await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let b = resp.bytes().await.map_err(|e| NmError::Remote(format!("download interrompido: {e}")))?;
        Ok(Some(b.to_vec()))
    }

    async fn write(&self, path: &str, data: &[u8]) -> NmResult<()> {
        self.api.send(reqwest::Method::PUT, &self.blob(path)?, Body::Blob(data), &[]).await?;
        Ok(())
    }

    async fn delete(&self, path: &str) -> NmResult<()> {
        self.api.send(reqwest::Method::DELETE, &self.blob(path)?, Body::None, &[]).await?;
        Ok(())
    }

    async fn list(&self, dir: &str) -> NmResult<Vec<RemoteEntry>> {
        let path = self.api.world_path(&format!("list?dir={dir}"));
        self.api.json(reqwest::Method::GET, &path, None::<&()>).await
    }

    async fn upload_many(&self, local_dir: &Path, names: &[String], remote_dir: &str, progress: ProgressFn) -> NmResult<()> {
        let total_bytes: u64 = names.iter().map(|n| std::fs::metadata(local_dir.join(n)).map(|m| m.len()).unwrap_or(0)).sum();
        let done_b = Arc::new(AtomicU64::new(0));
        let done_f = Arc::new(AtomicU64::new(0));
        let n = names.len() as u64;
        stream::iter(names.iter().cloned())
            .map(|name| {
                let (done_b, done_f, progress) = (done_b.clone(), done_f.clone(), progress.clone());
                async move {
                    let data = tokio::fs::read(local_dir.join(&name)).await?;
                    let mut last = None;
                    for attempt in 0..3 {
                        match self.write(&format!("{remote_dir}/{name}"), &data).await {
                            Ok(()) => {
                                last = None;
                                break;
                            }
                            Err(e) => {
                                tracing::warn!(attempt, %name, error = %e, "envio falhou; tentando de novo");
                                last = Some(e);
                                tokio::time::sleep(std::time::Duration::from_secs(2 << attempt)).await;
                            }
                        }
                    }
                    if let Some(e) = last {
                        return Err(e);
                    }
                    let b = done_b.fetch_add(data.len() as u64, Ordering::SeqCst) + data.len() as u64;
                    let f = done_f.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(TransferProgress { bytes_done: b, bytes_total: total_bytes, files_done: f, files_total: n });
                    Ok::<_, NmError>(())
                }
            })
            .buffer_unordered(PARALLEL)
            .try_collect::<Vec<_>>()
            .await?;
        Ok(())
    }

    async fn download_many(&self, remote_dir: &str, names: &[String], local_dir: &Path, progress: ProgressFn) -> NmResult<()> {
        tokio::fs::create_dir_all(local_dir).await?;
        let done_b = Arc::new(AtomicU64::new(0));
        let done_f = Arc::new(AtomicU64::new(0));
        let n = names.len() as u64;
        stream::iter(names.iter().cloned())
            .map(|name| {
                let (done_b, done_f, progress) = (done_b.clone(), done_f.clone(), progress.clone());
                async move {
                    let mut got = None;
                    for attempt in 0..3 {
                        match self.read(&format!("{remote_dir}/{name}")).await {
                            Ok(Some(b)) => {
                                got = Some(b);
                                break;
                            }
                            Ok(None) => return Err(NmError::Integrity(format!("{remote_dir}/{name} sumiu da nuvem"))),
                            Err(e) => {
                                tracing::warn!(attempt, %name, error = %e, "download falhou; tentando de novo");
                                tokio::time::sleep(std::time::Duration::from_secs(2 << attempt)).await;
                            }
                        }
                    }
                    let data = got.ok_or_else(|| NmError::Remote(format!("não consegui baixar {name}")))?;
                    crate::util::write_atomic(&local_dir.join(&name), &data)?;
                    let b = done_b.fetch_add(data.len() as u64, Ordering::SeqCst) + data.len() as u64;
                    let f = done_f.fetch_add(1, Ordering::SeqCst) + 1;
                    progress(TransferProgress { bytes_done: b, bytes_total: 0, files_done: f, files_total: n });
                    Ok::<_, NmError>(())
                }
            })
            .buffer_unordered(PARALLEL)
            .try_collect::<Vec<_>>()
            .await?;
        Ok(())
    }

    async fn delete_many(&self, dir: &str, names: &[String]) -> NmResult<()> {
        #[derive(Serialize)]
        struct Req<'a> {
            dir: &'a str,
            names: &'a [String],
        }
        for chunk in names.chunks(200) {
            let _: serde_json::Value = self
                .api
                .json(reqwest::Method::POST, &self.api.world_path("blob-delete"), Some(&Req { dir, names: chunk }))
                .await?;
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("nuvem do mundo em {}", self.api.base)
    }
}
