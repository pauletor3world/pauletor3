//! Posse da vez de hospedar, em cima da [`Coordinator`].
//!
//! Na v1 a trava era um arquivo no Drive e precisava de janela de acomodação, desempate e
//! conferência dupla. Agora a coordenação decide atomicamente; aqui fica só o lado do cliente:
//! heartbeat, detecção de vez perdida e de isolamento (o host sem contato com a coordenação).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::coord::{rules, AcquireRequest, Coordinator, Head, LeaseInfo, LeasePurpose, LeaseStatus};
use crate::{NmError, NmResult};

#[derive(Clone, Debug)]
pub struct LeaseConfig {
    pub heartbeat_every: Duration,
    /// Sem heartbeat confirmado por este tempo, o host se considera isolado.
    pub isolated_after: Duration,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            heartbeat_every: Duration::from_secs(rules::HEARTBEAT_SECS),
            // um pouco antes da expiração, para dar tempo de reagir
            isolated_after: Duration::from_secs((rules::LEASE_TTL_SECS - 5) as u64),
        }
    }
}

#[derive(Clone)]
pub struct LeaseManager {
    pub coord: Arc<dyn Coordinator>,
    pub cfg: LeaseConfig,
}

impl LeaseManager {
    pub fn new(coord: Arc<dyn Coordinator>, cfg: LeaseConfig) -> Self {
        Self { coord, cfg }
    }

    pub async fn status(&self) -> NmResult<LeaseStatus> {
        Ok(self.coord.view().await?.lease)
    }

    pub async fn acquire(&self, req: AcquireRequest) -> NmResult<LeaseGuard> {
        let info = self.coord.acquire(req).await?;
        Ok(LeaseGuard::start(self.clone(), info))
    }

    /// Pede a vez de novo enquanto a resposta for "outro tem preferência" (failover).
    /// `replica` é relido a cada tentativa: a réplica deste PC pode avançar enquanto espera.
    pub async fn acquire_patiently(&self, mut req: AcquireRequest, max_wait: Duration, replica: impl Fn() -> u64) -> NmResult<LeaseGuard> {
        let deadline = tokio::time::Instant::now() + max_wait;
        loop {
            req.replica_seq = req.replica_seq.max(replica());
            match self.acquire(req.clone()).await {
                Err(NmError::NotBestCandidate { .. }) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                r => return r,
            }
        }
    }

    pub fn new_request(session_id: &str, player: &str, purpose: LeasePurpose, endpoint: Option<String>, replica_seq: u64) -> AcquireRequest {
        AcquireRequest {
            session_id: session_id.to_string(),
            player: player.to_string(),
            machine: crate::util::machine_name(),
            endpoint,
            purpose,
            replica_seq,
            app_version: crate::APP_VERSION.to_string(),
        }
    }
}

struct Shared {
    info: tokio::sync::Mutex<LeaseInfo>,
    online: std::sync::Mutex<Vec<String>>,
    handing_over: std::sync::atomic::AtomicBool,
}

/// Posse da vez. Mantém o heartbeat em segundo plano.
/// Soltar o guard **não** libera a vez (uma queda deve deixar a vez expirar sozinha):
/// use [`LeaseGuard::release`].
pub struct LeaseGuard {
    mgr: LeaseManager,
    session_number: u64,
    shared: Arc<Shared>,
    lost_rx: watch::Receiver<bool>,
    isolated_rx: watch::Receiver<bool>,
    stop_tx: watch::Sender<bool>,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl LeaseGuard {
    fn start(mgr: LeaseManager, info: LeaseInfo) -> Self {
        let session_number = info.session_number;
        let shared = Arc::new(Shared {
            info: tokio::sync::Mutex::new(info),
            online: Default::default(),
            handing_over: false.into(),
        });
        let (lost_tx, lost_rx) = watch::channel(false);
        let (isolated_tx, isolated_rx) = watch::channel(false);
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let m = mgr.clone();
        let sh = shared.clone();
        let task = tokio::spawn(async move {
            let mut last_ok = tokio::time::Instant::now();
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(m.cfg.heartbeat_every) => {}
                    _ = stop_rx.changed() => return,
                }
                if *stop_rx.borrow() {
                    return;
                }
                let online = sh.online.lock().unwrap().clone();
                let ho = sh.handing_over.load(std::sync::atomic::Ordering::SeqCst);
                match m.coord.heartbeat(session_number, &online, ho).await {
                    Ok(l) => {
                        *sh.info.lock().await = l;
                        last_ok = tokio::time::Instant::now();
                        if *isolated_tx.borrow() {
                            tracing::info!("contato com a coordenação restabelecido");
                            let _ = isolated_tx.send(false);
                        }
                    }
                    Err(NmError::LostLock) => {
                        tracing::error!("a vez não é mais nossa: outro PC assumiu");
                        let _ = lost_tx.send(true);
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "heartbeat falhou; tento de novo");
                        if last_ok.elapsed() >= m.cfg.isolated_after && !*isolated_tx.borrow() {
                            tracing::error!("sem contato com a coordenação há {:?}", last_ok.elapsed());
                            let _ = isolated_tx.send(true);
                        }
                    }
                }
            }
        });
        Self { mgr, session_number, shared, lost_rx, isolated_rx, stop_tx, task: std::sync::Mutex::new(Some(task)) }
    }

    pub fn session_number(&self) -> u64 {
        self.session_number
    }

    pub async fn info(&self) -> LeaseInfo {
        self.shared.info.lock().await.clone()
    }

    /// Atualiza quem está online / "passando a vez" e manda um heartbeat imediato.
    pub async fn update(&self, online: Option<Vec<String>>, handing_over: Option<bool>) -> NmResult<()> {
        if let Some(o) = online {
            *self.shared.online.lock().unwrap() = o;
        }
        if let Some(h) = handing_over {
            self.shared.handing_over.store(h, std::sync::atomic::Ordering::SeqCst);
        }
        self.beat().await
    }

    async fn beat(&self) -> NmResult<()> {
        if *self.lost_rx.borrow() {
            return Err(NmError::LostLock);
        }
        let online = self.shared.online.lock().unwrap().clone();
        let ho = self.shared.handing_over.load(std::sync::atomic::Ordering::SeqCst);
        let l = self.mgr.coord.heartbeat(self.session_number, &online, ho).await?;
        *self.shared.info.lock().await = l;
        Ok(())
    }

    /// Vez perdida (outro PC assumiu). Definitivo.
    pub fn lost(&self) -> watch::Receiver<bool> {
        self.lost_rx.clone()
    }

    /// Sem contato com a coordenação há quase o tempo de expiração. Pode voltar a `false`.
    pub fn isolated(&self) -> watch::Receiver<bool> {
        self.isolated_rx.clone()
    }

    /// Cerca de segurança: confirma com a coordenação que a vez ainda é nossa.
    pub async fn verify(&self) -> NmResult<()> {
        self.beat().await
    }

    /// Publica o HEAD com CAS (vez atual + HEAD esperado).
    pub async fn publish_head(&self, expected: Option<&str>, manifest: &str) -> NmResult<Head> {
        if *self.lost_rx.borrow() {
            return Err(NmError::LostLock);
        }
        self.mgr.coord.publish_head(self.session_number, expected, manifest).await
    }

    /// Libera a vez. Só chame depois que o HEAD final foi gravado.
    pub async fn release(&self) -> NmResult<()> {
        self.stop_heartbeat().await;
        self.mgr.coord.release(self.session_number, false).await
    }

    /// Passa o host: libera a vez para quem está jogando assumir na hora (failover planejado).
    pub async fn release_handoff(&self) -> NmResult<()> {
        self.stop_heartbeat().await;
        self.mgr.coord.release(self.session_number, true).await
    }

    /// Para o heartbeat sem liberar (a vez expira sozinha).
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

impl std::fmt::Debug for LeaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaseGuard").field("session_number", &self.session_number).finish_non_exhaustive()
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coord::mem::MemHub;

    fn fast() -> LeaseConfig {
        LeaseConfig { heartbeat_every: Duration::from_millis(30), isolated_after: Duration::from_millis(120) }
    }

    fn req(id: &str) -> AcquireRequest {
        LeaseManager::new_request(id, id, LeasePurpose::Hosting, None, 0)
    }

    #[tokio::test]
    async fn race_many_clients_one_winner() {
        for _ in 0..20 {
            let hub = MemHub::new("M", "p0", "P0");
            for i in 1..5 {
                hub.add_member(&format!("p{i}"), &format!("P{i}"));
            }
            let mut tasks = vec![];
            for i in 0..5 {
                let m = LeaseManager::new(Arc::new(hub.client(&format!("p{i}"))), fast());
                tasks.push(tokio::spawn(async move { m.acquire(req(&format!("s{i}"))).await }));
            }
            let mut winners = 0;
            for t in tasks {
                if t.await.unwrap().is_ok() {
                    winners += 1;
                }
            }
            assert_eq!(winners, 1, "exatamente um host");
        }
    }

    #[tokio::test]
    async fn heartbeat_detects_takeover() {
        let hub = MemHub::new("M", "a", "Ana");
        hub.add_member("b", "Bia");
        let ma = LeaseManager::new(Arc::new(hub.client("a")), fast());
        let g = ma.acquire(req("s1")).await.unwrap();
        g.update(Some(vec!["Ana".into()]), None).await.unwrap();
        assert_eq!(g.info().await.online, vec!["Ana".to_string()]);
        // a abandona; o relógio passa; b assume
        g.abandon().await;
        hub.clock.advance(chrono::Duration::seconds(rules::LEASE_TTL_SECS + 1));
        let mb = LeaseManager::new(Arc::new(hub.client("b")), fast());
        let gb = mb.acquire(req("s2")).await.unwrap();
        assert!(matches!(g.verify().await, Err(NmError::LostLock)));
        assert!(matches!(g.publish_head(None, "x").await, Err(NmError::LostLock)));
        gb.publish_head(None, "x").await.unwrap();
    }

    #[tokio::test]
    async fn lost_signal_fires() {
        let hub = MemHub::new("M", "a", "Ana");
        hub.add_member("b", "Bia");
        let ma = LeaseManager::new(Arc::new(hub.client("a")), fast());
        let g = ma.acquire(req("s1")).await.unwrap();
        // simula: a coordenação expirou a vez e b assumiu enquanto a ainda "batia"
        hub.clock.advance(chrono::Duration::seconds(rules::LEASE_TTL_SECS + 1));
        let mb = LeaseManager::new(Arc::new(hub.client("b")), fast());
        let _gb = mb.acquire(req("s2")).await.unwrap();
        let mut lost = g.lost();
        tokio::time::timeout(Duration::from_secs(2), lost.wait_for(|v| *v)).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn isolation_detected_and_cleared() {
        let hub = MemHub::new("M", "a", "Ana");
        let ca = hub.client("a");
        let ma = LeaseManager::new(Arc::new(ca.clone()), fast());
        let g = ma.acquire(req("s1")).await.unwrap();
        ca.set_offline(true);
        let mut iso = g.isolated();
        tokio::time::timeout(Duration::from_secs(2), iso.wait_for(|v| *v)).await.unwrap().unwrap();
        ca.set_offline(false);
        tokio::time::timeout(Duration::from_secs(2), iso.wait_for(|v| !*v)).await.unwrap().unwrap();
        g.release().await.unwrap();
        assert!(ma.status().await.unwrap().is_free());
    }
}
