//! Salvamento durante a partida.
//!
//! `save-off` → `save-all flush` → espelho rápido para o staging → `save-on`.
//! O envio para a nuvem acontece depois, a partir do staging, com o jogo já gravando normalmente.

use std::time::Instant;

use crate::mirror::{self, MirrorStats};
use crate::paths::Paths;
use crate::server::ServerProcess;
use crate::NmResult;

#[derive(Debug, Clone, Copy)]
pub struct CheckpointStats {
    pub mirror: MirrorStats,
    /// Quanto tempo o salvamento automático do jogo ficou desligado.
    pub pause_ms: u128,
}

pub async fn snapshot_live(server: &ServerProcess, paths: &Paths) -> NmResult<CheckpointStats> {
    server.cmd("save-off").await?;
    let started = Instant::now();
    let result = async {
        // Com "flush", o comando só responde depois que tudo foi gravado em disco.
        server.cmd("save-all flush").await?;
        let (src, dst) = (paths.server(), paths.staging());
        let st = tokio::task::spawn_blocking(move || mirror::mirror(&src, &dst))
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
        Ok::<_, crate::NmError>(st)
    }
    .await;
    // save-on SEMPRE, mesmo se o espelho falhou.
    let on = server.cmd("save-on").await;
    let pause_ms = started.elapsed().as_millis();
    let st = result?;
    on?;
    tracing::info!(copied = st.copied, bytes = st.copied_bytes, pause_ms, "checkpoint local pronto");
    Ok(CheckpointStats { mirror: st, pause_ms })
}

/// Espelho com o servidor parado (fim de sessão, recuperação).
pub async fn snapshot_offline(paths: &Paths) -> NmResult<MirrorStats> {
    let (src, dst) = (paths.server(), paths.staging());
    Ok(tokio::task::spawn_blocking(move || mirror::mirror(&src, &dst))
        .await
        .map_err(|e| anyhow::anyhow!(e))??)
}
