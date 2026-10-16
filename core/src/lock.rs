//! Trava de host (lease com heartbeat).
//!
//! O Google Drive não tem compare-and-swap e ainda permite dois arquivos com o mesmo nome.
//! Um único `lock.json` escrito por dois PCs ao mesmo tempo poderia virar duas cópias.
//! Por isso cada candidato grava a **sua própria** reivindicação em `locks/<session_id>.json`
//! (nomes únicos, sem colisão) e a decisão é determinística:
//!
//! 1. Lista `locks/`. Se houver reivindicação viva de outro, desiste (ou oferece assumir se estiver velha).
//! 2. Grava a sua como `claiming`.
//! 3. Espera a janela de acomodação (`settle`) e lista de novo.
//! 4. Se houver outra `active`, desiste. Entre as `claiming`, vence a de menor
//!    `(claimed_at, session_id)`; todos os PCs leem os mesmos arquivos e chegam ao mesmo vencedor.
//! 5. O vencedor marca a sua como `active`, espera mais uma janela e confere de novo.
//!
//! Limitação documentada: se a gravação de um PC demorar mais que `settle` para aparecer na
//! listagem do outro, os dois podem achar que venceram. A conferência dupla (passo 5) e a
//! verificação antes de gravar o HEAD (`LockGuard::verify`) reduzem muito essa janela.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::remote::{layout, read_json, write_json, Remote};
use crate::{NmError, NmResult, FORMAT_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockPurpose {
    /// Hospedando o mundo.
    Hosting,
    /// Manutenção (restaurar versão, coleta de lixo). Ninguém joga.
    Maintenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Claiming,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockInfo {
    pub format: u32,
    pub session_id: String,
    pub player: String,
    pub machine: String,
    pub tailscale_ip: Option<String>,
    pub address: Option<String>,
    pub app_version: String,
    pub purpose: LockPurpose,
    pub state: LockState,
    pub claimed_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub heartbeat_at: DateTime<Utc>,
    /// O host está passando a vez (amigos podem "Assumir agora" assim que a trava sumir).
    #[serde(default)]
    pub handing_over: bool,
    /// Jogadores online agora (para os apps dos amigos mostrarem e avisarem).
    #[serde(default)]
    pub online: Vec<String>,
}

impl LockInfo {
    pub fn new(session_id: &str, player: &str, purpose: LockPurpose) -> Self {
        let now = Utc::now();
        Self {
            format: FORMAT_VERSION,
            session_id: session_id.to_string(),
            player: player.to_string(),
            machine: crate::util::machine_name(),
            tailscale_ip: None,
            address: None,
            app_version: crate::APP_VERSION.to_string(),
            purpose,
            state: LockState::Claiming,
            claimed_at: now,
            started_at: now,
            heartbeat_at: now,
            handing_over: false,
            online: vec![],
        }
    }

    fn order_key(&self) -> (DateTime<Utc>, &str) {
        (self.claimed_at, self.session_id.as_str())
    }
}

/// Testa se o dono de uma trava ainda está vivo (porta do jogo respondendo no IP do Tailscale).
#[async_trait::async_trait]
pub trait Liveness: Send + Sync {
    async fn alive(&self, lock: &LockInfo) -> bool;
}

/// Tenta abrir conexão TCP na porta do jogo do dono.
pub struct TcpLiveness {
    pub port: u16,
    pub timeout: Duration,
}

#[async_trait::async_trait]
impl Liveness for TcpLiveness {
    async fn alive(&self, lock: &LockInfo) -> bool {
        let Some(ip) = &lock.tailscale_ip else { return false };
        let addr = format!("{ip}:{}", self.port);
        matches!(
            tokio::time::timeout(self.timeout, tokio::net::TcpStream::connect(addr)).await,
            Ok(Ok(_))
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LockStatus {
    Free,
    Held { lock: LockInfo },
    Stale { lock: LockInfo },
}

#[derive(Clone)]
pub struct LockConfig {
    pub settle: Duration,
    pub stale_after: Duration,
    pub heartbeat_every: Duration,
}

impl Default for LockConfig {
    fn default() -> Self {
        Self {
            settle: Duration::from_secs(6),
            stale_after: Duration::from_secs(5 * 60),
            heartbeat_every: Duration::from_secs(60),
        }
    }
}

#[derive(Clone)]
pub struct LockManager {
    remote: Arc<dyn Remote>,
    liveness: Arc<dyn Liveness>,
    pub cfg: LockConfig,
}

impl LockManager {
    pub fn new(remote: Arc<dyn Remote>, liveness: Arc<dyn Liveness>, cfg: LockConfig) -> Self {
        Self { remote, liveness, cfg }
    }

    /// Todas as reivindicações no remote.
    pub async fn claims(&self) -> NmResult<Vec<LockInfo>> {
        let mut out = Vec::new();
        for e in self.remote.list(layout::LOCKS).await? {
            let Some(id) = e.name.strip_suffix(".json") else { continue };
            match read_json::<LockInfo>(&*self.remote, &layout::lock(id)).await {
                Ok(Some(l)) => {
                    if l.format > FORMAT_VERSION {
                        return Err(NmError::FormatTooNew { found: l.format });
                    }
                    out.push(l)
                }
                Ok(None) => {}
                Err(err) => tracing::warn!(%err, file = %e.name, "reivindicação ilegível ignorada"),
            }
        }
        out.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
        Ok(out)
    }

    pub fn heartbeat_old(&self, l: &LockInfo) -> bool {
        let age = Utc::now().signed_duration_since(l.heartbeat_at);
        age.to_std().map(|a| a > self.cfg.stale_after).unwrap_or(false)
    }

    /// Velha = heartbeat antigo **e** dono não responde. Nunca só um dos dois.
    pub async fn is_stale(&self, l: &LockInfo) -> bool {
        self.heartbeat_old(l) && !self.liveness.alive(l).await
    }

    /// Estado atual da vez.
    pub async fn status(&self) -> NmResult<LockStatus> {
        let claims = self.claims().await?;
        let pick = claims
            .iter()
            .find(|l| l.state == LockState::Active)
            .or_else(|| claims.first());
        match pick {
            None => Ok(LockStatus::Free),
            Some(l) if self.is_stale(l).await => Ok(LockStatus::Stale { lock: l.clone() }),
            Some(l) => Ok(LockStatus::Held { lock: l.clone() }),
        }
    }

    /// Tenta pegar a vez. `takeover` = ids de reivindicações velhas que o usuário confirmou assumir.
    pub async fn acquire(&self, mut me: LockInfo, takeover: &[String]) -> NmResult<LockGuard> {
        let existing = self.claims().await?;
        for other in existing.iter().filter(|l| l.session_id != me.session_id) {
            if self.is_stale(other).await {
                if takeover.contains(&other.session_id) {
                    tracing::warn!(session = %other.session_id, player = %other.player, "assumindo trava abandonada");
                    self.remote.delete(&layout::lock(&other.session_id)).await?;
                } else {
                    return Err(NmError::StaleLock(Box::new(other.clone())));
                }
            } else {
                return Err(NmError::HeldBy(Box::new(other.clone())));
            }
        }

        me.state = LockState::Claiming;
        me.claimed_at = Utc::now();
        me.heartbeat_at = me.claimed_at;
        write_json(&*self.remote, &layout::lock(&me.session_id), &me).await?;

        tokio::time::sleep(self.cfg.settle).await;
        if let Err(e) = self.check_winner(&me).await {
            let _ = self.remote.delete(&layout::lock(&me.session_id)).await;
            return Err(e);
        }

        me.state = LockState::Active;
        me.heartbeat_at = Utc::now();
        write_json(&*self.remote, &layout::lock(&me.session_id), &me).await?;

        // Conferência dupla depois de ativo.
        tokio::time::sleep(self.cfg.settle).await;
        if let Err(e) = self.check_winner(&me).await {
            let _ = self.remote.delete(&layout::lock(&me.session_id)).await;
            return Err(e);
        }

        Ok(LockGuard::start(self.clone(), me))
    }

    async fn check_winner(&self, me: &LockInfo) -> NmResult<()> {
        let claims = self.claims().await?;
        if !claims.iter().any(|l| l.session_id == me.session_id) {
            return Err(NmError::LostLock);
        }
        let mut contenders = Vec::new();
        for l in claims {
            if l.session_id == me.session_id {
                contenders.push(l);
                continue;
            }
            if self.is_stale(&l).await {
                continue;
            }
            contenders.push(l);
        }
        // Uma trava ativa de outro, mais antiga que a minha, vence sempre.
        contenders.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
        let winner = contenders
            .iter()
            .filter(|l| l.session_id != me.session_id && l.state == LockState::Active)
            .find(|l| l.order_key() < me.order_key())
            .or_else(|| contenders.first())
            .expect("a minha reivindicação está na lista");
        if winner.session_id == me.session_id {
            Ok(())
        } else if winner.state == LockState::Active {
            Err(NmError::HeldBy(Box::new(winner.clone())))
        } else {
            Err(NmError::LockRace)
        }
    }

    /// Remove uma reivindicação (usado na liberação e na recuperação de queda do próprio PC).
    pub async fn remove(&self, session_id: &str) -> NmResult<()> {
        self.remote.delete(&layout::lock(session_id)).await
    }
}

/// Posse da vez. Mantém o heartbeat em segundo plano.
/// Soltar o guard **não** libera a trava (uma queda deve deixar a trava para a recuperação):
/// use [`LockGuard::release`].
pub struct LockGuard {
    mgr: LockManager,
    info: Arc<tokio::sync::Mutex<LockInfo>>,
    lost_rx: watch::Receiver<bool>,
    stop_tx: watch::Sender<bool>,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl LockGuard {
    fn start(mgr: LockManager, info: LockInfo) -> Self {
        let info = Arc::new(tokio::sync::Mutex::new(info));
        let (lost_tx, lost_rx) = watch::channel(false);
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let m = mgr.clone();
        let inf = info.clone();
        let every = mgr.cfg.heartbeat_every;
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(every) => {}
                    _ = stop_rx.changed() => return,
                }
                if *stop_rx.borrow() {
                    return;
                }
                let mut i = inf.lock().await;
                // Se alguém apagou a minha trava (assumiu), não recria: avisa que perdemos a vez.
                match m.remote.read(&layout::lock(&i.session_id)).await {
                    Ok(None) => {
                        tracing::error!("minha trava sumiu do remote: outro PC assumiu");
                        let _ = lost_tx.send(true);
                        return;
                    }
                    Ok(Some(_)) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "heartbeat: não consegui ler a trava; tento de novo no próximo ciclo");
                        continue;
                    }
                }
                i.heartbeat_at = Utc::now();
                if let Err(e) = write_json(&*m.remote, &layout::lock(&i.session_id), &*i).await {
                    tracing::warn!(error = %e, "heartbeat falhou; tento de novo no próximo ciclo");
                }
            }
        });
        Self { mgr, info, lost_rx, stop_tx, task: std::sync::Mutex::new(Some(task)) }
    }

    pub async fn info(&self) -> LockInfo {
        self.info.lock().await.clone()
    }

    /// Atualiza campos da trava (IP, endereço, "passando a vez") e grava imediatamente.
    pub async fn update(&self, f: impl FnOnce(&mut LockInfo)) -> NmResult<()> {
        let mut i = self.info.lock().await;
        f(&mut i);
        i.heartbeat_at = Utc::now();
        write_json(&*self.mgr.remote, &layout::lock(&i.session_id), &*i).await
    }

    pub fn lost(&self) -> watch::Receiver<bool> {
        self.lost_rx.clone()
    }

    /// Cerca de segurança: confirma que ainda somos os donos antes de gravar o HEAD.
    pub async fn verify(&self) -> NmResult<()> {
        if *self.lost_rx.borrow() {
            return Err(NmError::LostLock);
        }
        let me = self.info().await;
        let claims = self.mgr.claims().await?;
        if !claims.iter().any(|l| l.session_id == me.session_id) {
            return Err(NmError::LostLock);
        }
        if let Some(other) = claims
            .iter()
            .find(|l| l.session_id != me.session_id && l.state == LockState::Active && l.order_key() < me.order_key())
        {
            return Err(NmError::HeldBy(Box::new(other.clone())));
        }
        Ok(())
    }

    /// Libera a vez. Só chame depois que o HEAD final foi gravado.
    pub async fn release(&self) -> NmResult<()> {
        self.stop_heartbeat().await;
        let id = self.info.lock().await.session_id.clone();
        self.mgr.remove(&id).await
    }

    /// Para o heartbeat sem liberar (a trava fica para a recuperação).
    pub async fn abandon(&self) {
        self.stop_heartbeat().await;
    }

    async fn stop_heartbeat(&self) {
        let _ = self.stop_tx.send(true);
        let t = self.task.lock().unwrap().take();
        if let Some(t) = t {
            let _ = t.await;
        }
    }
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockGuard").finish_non_exhaustive()
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::FsRemote;

    struct Dead;
    #[async_trait::async_trait]
    impl Liveness for Dead {
        async fn alive(&self, _: &LockInfo) -> bool {
            false
        }
    }
    struct Alive;
    #[async_trait::async_trait]
    impl Liveness for Alive {
        async fn alive(&self, _: &LockInfo) -> bool {
            true
        }
    }

    fn fast() -> LockConfig {
        LockConfig {
            settle: Duration::from_millis(80),
            stale_after: Duration::from_secs(300),
            heartbeat_every: Duration::from_millis(50),
        }
    }

    fn mgr(r: &FsRemote, live: Arc<dyn Liveness>) -> LockManager {
        LockManager::new(Arc::new(r.clone()), live, fast())
    }

    #[tokio::test]
    async fn acquire_and_release() {
        let d = tempfile::tempdir().unwrap();
        let r = FsRemote::new(d.path());
        let m = mgr(&r, Arc::new(Dead));
        let g = m.acquire(LockInfo::new("s1", "Ana", LockPurpose::Hosting), &[]).await.unwrap();
        assert!(matches!(m.status().await.unwrap(), LockStatus::Held { .. }));
        let err = m.acquire(LockInfo::new("s2", "Bia", LockPurpose::Hosting), &[]).await.unwrap_err();
        assert!(matches!(err, NmError::HeldBy(ref l) if l.player == "Ana"));
        g.verify().await.unwrap();
        g.release().await.unwrap();
        assert!(matches!(m.status().await.unwrap(), LockStatus::Free));
    }

    async fn race(n: usize) {
        let d = tempfile::tempdir().unwrap();
        let r = FsRemote::new(d.path());
        let mut tasks = vec![];
        for i in 0..n {
            let m = mgr(&r, Arc::new(Dead));
            tasks.push(tokio::spawn(async move {
                m.acquire(LockInfo::new(&format!("s{i}"), &format!("P{i}"), LockPurpose::Hosting), &[]).await
            }));
        }
        let mut winners = vec![];
        for t in tasks {
            if let Ok(g) = t.await.unwrap() {
                winners.push(g);
            }
        }
        assert_eq!(winners.len(), 1, "exatamente um host");
        // Os perdedores apagaram as suas reivindicações.
        let m = mgr(&r, Arc::new(Dead));
        assert_eq!(m.claims().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn race_two_clients() {
        for _ in 0..5 {
            race(2).await;
        }
    }

    #[tokio::test]
    async fn race_three_clients() {
        for _ in 0..5 {
            race(3).await;
        }
    }

    #[tokio::test]
    async fn stale_lock_needs_explicit_takeover() {
        let d = tempfile::tempdir().unwrap();
        let r = FsRemote::new(d.path());
        let mut old = LockInfo::new("velha", "Ana", LockPurpose::Hosting);
        old.state = LockState::Active;
        old.heartbeat_at = Utc::now() - chrono::Duration::minutes(10);
        write_json(&r, &layout::lock("velha"), &old).await.unwrap();

        // Dono ainda responde na porta: não é velha, mesmo com heartbeat antigo.
        let alive = mgr(&r, Arc::new(Alive));
        assert!(matches!(alive.status().await.unwrap(), LockStatus::Held { .. }));
        assert!(matches!(
            alive.acquire(LockInfo::new("nova", "Bia", LockPurpose::Hosting), &["velha".into()]).await,
            Err(NmError::HeldBy(_))
        ));

        // Dono morto: oferece assumir, mas não assume sozinho.
        let m = mgr(&r, Arc::new(Dead));
        assert!(matches!(m.status().await.unwrap(), LockStatus::Stale { .. }));
        assert!(matches!(
            m.acquire(LockInfo::new("nova", "Bia", LockPurpose::Hosting), &[]).await,
            Err(NmError::StaleLock(_))
        ));
        let g = m.acquire(LockInfo::new("nova", "Bia", LockPurpose::Hosting), &["velha".into()]).await.unwrap();
        assert_eq!(g.info().await.player, "Bia");
        assert_eq!(m.claims().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn heartbeat_detects_takeover() {
        let d = tempfile::tempdir().unwrap();
        let r = FsRemote::new(d.path());
        let m = mgr(&r, Arc::new(Dead));
        let g = m.acquire(LockInfo::new("s1", "Ana", LockPurpose::Hosting), &[]).await.unwrap();
        let before = g.info().await.heartbeat_at;
        tokio::time::sleep(Duration::from_millis(150)).await;
        let stored: LockInfo = read_json(&r, &layout::lock("s1")).await.unwrap().unwrap();
        assert!(stored.heartbeat_at > before, "heartbeat atualiza a trava");
        r.delete(&layout::lock("s1")).await.unwrap();
        let mut lost = g.lost();
        tokio::time::timeout(Duration::from_secs(2), lost.wait_for(|v| *v)).await.unwrap().unwrap();
        assert!(matches!(g.verify().await, Err(NmError::LostLock)));
        assert!(r.read(&layout::lock("s1")).await.unwrap().is_none(), "não recria a trava");
    }

    #[tokio::test]
    async fn newer_format_refuses() {
        let d = tempfile::tempdir().unwrap();
        let r = FsRemote::new(d.path());
        let mut l = LockInfo::new("x", "Ana", LockPurpose::Hosting);
        l.format = 99;
        write_json(&r, &layout::lock("x"), &l).await.unwrap();
        let m = mgr(&r, Arc::new(Dead));
        assert!(matches!(m.status().await, Err(NmError::FormatTooNew { found: 99 })));
    }
}
