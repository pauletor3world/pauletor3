//! Orquestração: o que acontece quando alguém clica em Jogar, durante a partida e no fim.
//!
//! [`Engine`] é o objeto único que a interface (Tauri) e a CLI usam.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use crate::config::{AddressMode, Config};
use crate::error::HumanError;
use crate::events::{Event, EventSink, NoticeLevel, SyncProgress, SyncProgressFn};
use crate::journal::{self, msg, Journal};
use crate::lock::{LockConfig, LockGuard, LockInfo, LockManager, LockPurpose, LockStatus, TcpLiveness};
use crate::logparse::LogEvent;
use crate::manifest::{ManifestKind, ManifestSummary};
use crate::paths::Paths;
use crate::remote::{RcloneRemote, Remote};
use crate::secrets::Secrets;
use crate::server::{LaunchOptions, ServerProcess};
use crate::stats::{SessionLog, Stats};
use crate::sync::{ActiveSession, LocalState, PushMeta, SyncEngine};
use crate::versions::{Versions, WorldSettings};
use crate::{checkpoint, duckdns, java, launcher, retention, server, serversdat, tailscale, util};
use crate::{NmError, NmResult, GAME_PORT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nada acontecendo neste PC.
    Idle,
    /// Preparando (baixando mundo, Java, servidor).
    Preparing,
    /// Servidor subindo.
    Starting,
    /// Hospedando.
    Hosting,
    /// Encerrando e enviando.
    Stopping,
    /// Recuperando uma sessão que caiu.
    Recovering,
    /// Manutenção (restauração, coleta de lixo, importação).
    Maintenance,
    /// Esperando a vez ficar livre para assumir.
    WaitingTurn,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub configured: bool,
    pub phase: Phase,
    pub player_name: String,
    pub world_name: String,
    pub lock: Option<LockStatus>,
    pub me_hosting: bool,
    pub head: Option<ManifestSummary>,
    pub online: Vec<String>,
    pub sync: Option<SyncProgress>,
    pub address: Option<String>,
    pub host_client_running: bool,
    pub friends_still_playing: bool,
    pub recovery_pending: bool,
    pub last_error: Option<HumanError>,
    pub session_started_at: Option<DateTime<Utc>>,
    pub next_checkpoint_at: Option<DateTime<Utc>>,
    pub last_checkpoint_at: Option<DateTime<Utc>>,
    pub remote_ok: bool,
    pub refreshed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Todo mundo saiu.
    Normal,
    /// Botão "Passar a vez".
    Handover,
    /// Pedido do usuário (Parar / Sair).
    UserStop,
    /// Windows desligando.
    Shutdown,
}

/// Uma sessão hospedada neste PC.
struct HostSession {
    session_id: String,
    started_at: DateTime<Utc>,
    guard: LockGuard,
    server: Arc<ServerProcess>,
    log: Mutex<SessionLog>,
    stats: Mutex<Stats>,
    /// Garante um envio por vez.
    push_lock: Mutex<()>,
    last_published: Mutex<Option<String>>,
    base_manifest: Option<String>,
    versions: Versions,
    client_seen: std::sync::atomic::AtomicBool,
    ending: std::sync::atomic::AtomicBool,
    tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

pub struct Engine {
    pub paths: Paths,
    pub config: RwLock<Config>,
    secrets: RwLock<Secrets>,
    remote_override: Option<Arc<dyn Remote>>,
    rclone_bin: PathBuf,
    events: EventSink,
    pub journal: Arc<Journal>,
    status: std::sync::RwLock<Status>,
    session: Mutex<Option<Arc<HostSession>>>,
    /// Serializa operações grandes (jogar, restaurar, recuperar...).
    op: Mutex<()>,
    pub lock_cfg: LockConfig,
    world_settings: RwLock<Option<WorldSettings>>,
}

impl Engine {
    /// Abre o motor com a config e os segredos deste PC.
    pub fn open(paths: Paths, rclone_bin: PathBuf, events: EventSink) -> anyhow::Result<Arc<Self>> {
        Self::build(paths, rclone_bin, events, None)
    }

    /// Para testes: usa um remote dado em vez do rclone.
    pub fn with_remote(paths: Paths, remote: Arc<dyn Remote>, events: EventSink) -> anyhow::Result<Arc<Self>> {
        Self::build(paths, PathBuf::from("rclone"), events, Some(remote))
    }

    fn build(paths: Paths, rclone_bin: PathBuf, events: EventSink, remote: Option<Arc<dyn Remote>>) -> anyhow::Result<Arc<Self>> {
        paths.ensure()?;
        let config = Config::load(&paths);
        let secrets = Secrets::load(&paths.secrets()).unwrap_or_else(|e| {
            tracing::error!(error = %e, "segredos ilegíveis; será preciso configurar de novo");
            Secrets::default()
        });
        let journal = Journal::open(
            paths.root.join("journal-queue.json"),
            config.journal.clone(),
            secrets.discord_webhook.clone(),
            "pAuletOr 3",
        );
        let status = Status {
            configured: config.setup_complete,
            phase: Phase::Idle,
            player_name: config.player_name.clone(),
            world_name: "pAuletOr 3".into(),
            lock: None,
            me_hosting: false,
            head: None,
            online: vec![],
            sync: None,
            address: config.public_address(None),
            host_client_running: false,
            friends_still_playing: false,
            recovery_pending: LocalState::load(&paths).dirty,
            last_error: None,
            session_started_at: None,
            next_checkpoint_at: None,
            last_checkpoint_at: None,
            remote_ok: false,
            refreshed_at: None,
        };
        let _ = crate::sync::repair_interrupted_swap(&paths);
        Ok(Arc::new(Self {
            paths,
            config: RwLock::new(config),
            secrets: RwLock::new(secrets),
            remote_override: remote,
            rclone_bin,
            events,
            journal,
            status: std::sync::RwLock::new(status),
            session: Mutex::new(None),
            op: Mutex::new(()),
            lock_cfg: LockConfig::default(),
            world_settings: RwLock::new(None),
        }))
    }

    // ------------------------------------------------------------------ utilidades

    pub fn status(&self) -> Status {
        self.status.read().unwrap().clone()
    }

    fn set_status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.write().unwrap());
        (self.events)(Event::StatusChanged);
    }

    fn phase(&self, p: Phase) {
        self.set_status(|s| {
            s.phase = p;
            if p != Phase::Preparing && p != Phase::Stopping && p != Phase::Recovering && p != Phase::Maintenance {
                s.sync = None;
            }
        });
    }

    fn notice(&self, level: NoticeLevel, title: &str, body: &str) {
        (self.events)(Event::Notice { level, title: title.into(), body: body.into() });
    }

    fn progress(self: &Arc<Self>) -> SyncProgressFn {
        let me = Arc::downgrade(self);
        Arc::new(move |p: SyncProgress| {
            if let Some(me) = me.upgrade() {
                me.status.write().unwrap().sync = Some(p);
                (me.events)(Event::Sync(p));
            }
        })
    }

    pub async fn secrets(&self) -> Secrets {
        self.secrets.read().await.clone()
    }

    pub async fn update_secrets(&self, f: impl FnOnce(&mut Secrets)) -> anyhow::Result<()> {
        let mut s = self.secrets.write().await;
        f(&mut s);
        s.save(&self.paths.secrets())?;
        self.journal.set_webhook(s.discord_webhook.clone());
        Ok(())
    }

    pub async fn update_config(&self, f: impl FnOnce(&mut Config)) -> anyhow::Result<Config> {
        let mut c = self.config.write().await;
        f(&mut c);
        c.save(&self.paths)?;
        self.journal.set_settings(c.journal.clone());
        let c2 = c.clone();
        drop(c);
        self.set_status(|s| {
            s.configured = c2.setup_complete;
            s.player_name = c2.player_name.clone();
        });
        Ok(c2)
    }

    pub fn rclone_config_path(&self) -> PathBuf {
        self.paths.root.join("rclone.conf")
    }

    pub fn rclone_bin(&self) -> &PathBuf {
        &self.rclone_bin
    }

    /// O remote configurado.
    pub async fn remote(&self) -> NmResult<Arc<dyn Remote>> {
        if let Some(r) = &self.remote_override {
            return Ok(r.clone());
        }
        let pass = self
            .secrets
            .read()
            .await
            .rclone_config_pass
            .clone()
            .ok_or_else(|| NmError::NotConfigured("nuvem não conectada".into()))?;
        let cfg = self.config.read().await;
        Ok(Arc::new(RcloneRemote::new(
            self.rclone_bin.clone(),
            self.rclone_config_path(),
            zeroize::Zeroizing::new(pass),
            &cfg.remote_root,
            self.paths.tmp(),
        )))
    }

    async fn sync_engine(&self) -> NmResult<SyncEngine> {
        Ok(SyncEngine::new(self.remote().await?, self.paths.clone()))
    }

    async fn lock_manager(&self) -> NmResult<LockManager> {
        Ok(LockManager::new(
            self.remote().await?,
            Arc::new(TcpLiveness { port: GAME_PORT, timeout: Duration::from_secs(4) }),
            self.lock_cfg.clone(),
        ))
    }

    pub async fn world_settings(&self) -> NmResult<WorldSettings> {
        if let Some(w) = self.world_settings.read().await.clone() {
            return Ok(w);
        }
        let r = self.remote().await?;
        let w = WorldSettings::load(&*r).await?.unwrap_or_default();
        *self.world_settings.write().await = Some(w.clone());
        Ok(w)
    }

    /// Muda as configurações do mundo (só com ninguém hospedando).
    pub async fn save_world_settings(&self, w: WorldSettings) -> NmResult<()> {
        let lm = self.lock_manager().await?;
        if !matches!(lm.status().await?, LockStatus::Free) {
            return Err(NmError::Other(anyhow::anyhow!("só dá para mudar com ninguém hospedando")));
        }
        let r = self.remote().await?;
        w.save(&*r).await?;
        self.journal.set_world_name(&w.world_name);
        *self.world_settings.write().await = Some(w.clone());
        self.set_status(|s| s.world_name = w.world_name.clone());
        Ok(())
    }

    // ------------------------------------------------------------------ estado (amigos)

    /// Relê a trava e o HEAD do remote. Barato: chamado a cada ~20 s pela interface.
    pub async fn refresh(self: &Arc<Self>) -> NmResult<()> {
        let remote = match self.remote().await {
            Ok(r) => r,
            Err(e) => {
                self.set_status(|s| s.remote_ok = false);
                return Err(e);
            }
        };
        let lm = self.lock_manager().await?;
        let lock = match lm.status().await {
            Ok(l) => l,
            Err(e) => {
                self.set_status(|s| s.remote_ok = false);
                return Err(e);
            }
        };
        let se = SyncEngine::new(remote.clone(), self.paths.clone());
        let head = match se.head().await? {
            Some(h) => se.fetch_manifest(&h.manifest).await.ok().map(|m| m.summary()),
            None => None,
        };
        if self.world_settings.read().await.is_none() {
            if let Ok(Some(w)) = WorldSettings::load(&*remote).await {
                self.journal.set_world_name(&w.world_name);
                self.set_status(|s| s.world_name = w.world_name.clone());
                *self.world_settings.write().await = Some(w);
            }
        }
        let me_hosting = self.session.lock().await.is_some();
        let prev = self.status();
        let cfg = self.config.read().await.clone();

        // Avisos para quem não está hospedando.
        if !me_hosting && cfg.notify_toasts {
            self.toast_changes(prev.lock.as_ref(), &lock, &cfg);
        }

        // Fallback de endereço: reescreve o servers.dat com o IP do host (com o Minecraft fechado).
        if let (AddressMode::DirectIp, LockStatus::Held { lock: l }, Some(mc)) = (cfg.address_mode, &lock, &cfg.minecraft_dir) {
            if let Some(ip) = &l.tailscale_ip {
                if !launcher::client_running(None) {
                    if let Err(e) = serversdat::ensure_entry(mc, ip, &self.paths.backups()) {
                        tracing::warn!(error = %e, "não consegui atualizar o servers.dat");
                    }
                }
            }
        }

        let online = match &lock {
            LockStatus::Held { lock } => lock.online.clone(),
            _ => vec![],
        };
        let address = match &lock {
            LockStatus::Held { lock } => lock.address.clone().or_else(|| cfg.public_address(lock.tailscale_ip.as_deref())),
            _ => cfg.public_address(None),
        };
        self.set_status(|s| {
            if !me_hosting {
                s.online = online;
            }
            s.lock = Some(lock);
            s.head = head;
            s.address = address;
            s.me_hosting = me_hosting;
            s.remote_ok = true;
            s.refreshed_at = Some(Utc::now());
            s.recovery_pending = LocalState::load(&self.paths).dirty;
        });
        Ok(())
    }

    fn toast_changes(&self, prev: Option<&LockStatus>, now: &LockStatus, cfg: &Config) {
        let Some(prev) = prev else { return };
        match (prev, now) {
            (LockStatus::Free, LockStatus::Held { lock }) => {
                self.notice(NoticeLevel::Info, &format!("{} abriu o mundo", lock.player), "Entre pelo Multiplayer → pAuletOr 3.")
            }
            (LockStatus::Held { lock: a }, LockStatus::Held { lock: b }) if a.session_id == b.session_id => {
                if cfg.notify_join_leave {
                    for p in b.online.iter().filter(|p| !a.online.contains(p) && **p != cfg.player_name) {
                        self.notice(NoticeLevel::Info, &format!("{p} entrou no mundo"), "");
                    }
                }
                if !a.handing_over && b.handing_over {
                    self.notice(NoticeLevel::Warning, &format!("{} vai passar a vez", b.player), "Clique em Assumir agora para continuar no seu PC.");
                }
            }
            (LockStatus::Held { lock }, LockStatus::Free) => {
                self.notice(NoticeLevel::Success, "Mundo salvo", &format!("A sessão de {} terminou e o mundo está na nuvem.", lock.player))
            }
            (_, LockStatus::Stale { lock }) if !matches!(prev, LockStatus::Stale { .. }) => self.notice(
                NoticeLevel::Warning,
                &format!("A sessão de {} parece ter caído", lock.player),
                "Você pode assumir o mundo pelo app.",
            ),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ jogar

    /// Clique em "Jogar": pega a vez, baixa o mundo, sobe o servidor, avisa todo mundo.
    /// `takeover` = sessões velhas que o usuário confirmou assumir.
    pub async fn play(self: &Arc<Self>, takeover: Vec<String>, open_game: bool) -> NmResult<()> {
        let _op = self.op.try_lock().map_err(|_| NmError::Other(anyhow::anyhow!("já tem uma operação em andamento")))?;
        if self.session.lock().await.is_some() {
            return Err(NmError::Other(anyhow::anyhow!("você já está hospedando")));
        }
        self.set_status(|s| s.last_error = None);
        let res = self.play_inner(takeover, open_game).await;
        if let Err(e) = &res {
            let h = e.human();
            self.set_status(|s| {
                s.last_error = Some(h);
                s.phase = Phase::Idle;
                s.sync = None;
            });
        }
        res
    }

    async fn play_inner(self: &Arc<Self>, takeover: Vec<String>, open_game: bool) -> NmResult<()> {
        let cfg = self.config.read().await.clone();
        if !cfg.setup_complete {
            return Err(NmError::NotConfigured("termine o assistente primeiro".into()));
        }
        if !cfg.eula_accepted {
            return Err(NmError::NotConfigured("aceite o EULA do Minecraft nas configurações".into()));
        }
        self.phase(Phase::Preparing);

        // 0) recuperação pendente vem antes de tudo
        if LocalState::load(&self.paths).dirty {
            self.recover().await?;
            self.phase(Phase::Preparing);
        }

        // Este PC pode estar em outra rede Tailscale (pessoal): troca para a do mundo.
        if std::env::var_os("PAULETOR3_TEST_IP").is_none() {
            let st = tailscale::status().await;
            if !tailscale::on_world(&st, cfg.world_tailnet.as_deref()) {
                self.notice(NoticeLevel::Info, "Entrando na rede do mundo", "O Tailscale está sendo trocado para a rede do pAuletOr 3.");
                let key = self.secrets.read().await.tailscale_auth_key.clone();
                let t = tailscale::join_world(key.as_deref(), &cfg.player_name, cfg.world_tailnet.as_deref()).await?;
                if cfg.world_tailnet.as_deref() != Some(t.as_str()) {
                    self.update_config(|c| c.world_tailnet = Some(t.clone())).await?;
                }
            }
        }
        let ip = tailscale::my_ip().await?;
        let remote = self.remote().await?;
        let se = SyncEngine::new(remote.clone(), self.paths.clone());

        // 1) versões fixadas (o dono cria na primeira vez)
        let versions = match Versions::load(&*remote).await? {
            Some(v) => v,
            None if cfg.is_owner => {
                let v = Versions::builtin();
                v.save(&*remote).await?;
                v
            }
            None => return Err(NmError::NotConfigured("o dono ainda não criou o mundo".into())),
        };
        let settings = self.world_settings().await?;

        // 2) a vez
        let lm = self.lock_manager().await?;
        let previous_host = match lm.status().await? {
            LockStatus::Stale { lock } if takeover.contains(&lock.session_id) => Some(lock),
            _ => None,
        };
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let address = cfg.public_address(Some(&ip)).unwrap_or_else(|| ip.clone());
        let mut me = LockInfo::new(&session_id, &cfg.player_name, LockPurpose::Hosting);
        me.tailscale_ip = Some(ip.clone());
        me.address = Some(address.clone());
        let guard = lm.acquire(me, &takeover).await?;

        let started_at = Utc::now();
        // Daqui em diante, qualquer erro precisa soltar a vez (o mundo ainda não mudou).
        let prepared = async {
            // 3) mundo
            let head_before = se.head().await?.map(|h| h.manifest);
            se.pull(self.progress()).await?;

            // 4) Java + servidor conferidos
            let java = java::ensure_java(&self.paths, &versions, self.progress()).await?;
            server::prepare(&self.paths, &versions, self.progress()).await?;
            server::verify_install(&self.paths, &versions)?;
            if !server::eula_accepted(&self.paths) {
                server::accept_eula(&self.paths)?;
            }
            let rcon_password = util::random_token(32);
            server::write_properties(&self.paths, &settings, &ip, &rcon_password)?;
            // Espelho inicial com o servidor ainda parado: assim cada checkpoint só copia o que
            // mudou e a pausa do salvamento automático do jogo fica em milissegundos.
            checkpoint::snapshot_offline(&self.paths).await?;
            Ok::<_, NmError>((head_before, java, rcon_password))
        }
        .await;
        let (head_before, java, rcon_password) = match prepared {
            Ok(x) => x,
            Err(e) => {
                let _ = guard.release().await;
                return Err(e);
            }
        };

        // 5) a partir daqui o mundo local vai mudar: marca como sujo ANTES de subir o servidor
        let mut st = LocalState::load(&self.paths);
        st.dirty = true;
        st.active_session = Some(ActiveSession {
            session_id: session_id.clone(),
            started_at,
            base_manifest: head_before.clone(),
            last_published: None,
            rcon_password: Some(rcon_password.clone()),
            server_ip: Some(ip.clone()),
        });
        st.save(&self.paths)?;

        self.phase(Phase::Starting);
        let heap = crate::heap::effective_heap_mb(cfg.ram_mb);
        let proc = ServerProcess::launch(
            &self.paths,
            LaunchOptions { java, heap_mb: heap, server_ip: ip.clone(), rcon_password },
        )
        .await?;
        let lines = proc.subscribe_lines();
        self.forward_console(lines);
        let ready = proc.wait_ready(proc.subscribe(), Duration::from_secs(600)).await;
        if let Err(e) = ready {
            tracing::error!(error = %e, "servidor não abriu");
            let _ = proc.stop(Duration::from_secs(60)).await;
            // O mundo pode ter sido tocado na inicialização: envia como sessão curta, sem perder nada.
            self.finalize_without_session(&guard, &se, &session_id, started_at, head_before.as_deref(), &versions, &cfg.player_name).await;
            let _ = guard.release().await;
            return Err(e);
        }

        // 6) endereço fixo
        if let (AddressMode::Duckdns, Some(domain)) = (cfg.address_mode, &cfg.duckdns_domain) {
            if let Some(token) = self.secrets.read().await.duckdns_token.clone() {
                if let Err(e) = duckdns::update(domain, &token, &ip).await {
                    tracing::warn!(error = %e, "DuckDNS não atualizou");
                    self.notice(NoticeLevel::Warning, "DuckDNS não atualizou", "Os amigos podem entrar pelo IP mostrado no app.");
                }
            }
        }

        // 7) estatísticas e diário
        let mut stats = Stats::load(&*remote).await.unwrap_or_default();
        if stats.world_born.is_none() {
            stats.world_born = Some(started_at);
        }
        if let Some(prev) = &previous_host {
            self.journal.post(msg::host_crash_detected(&prev.player, (Utc::now() - prev.heartbeat_at).num_minutes())).await;
            self.journal.post(msg::took_over(&cfg.player_name, Some(&prev.player))).await;
        }
        self.journal.post(msg::world_opened(&cfg.player_name, &address, started_at)).await;
        for label in stats.due_anniversaries(Utc::now()) {
            self.journal.post(msg::anniversary(label, stats.world_born.unwrap())).await;
        }
        for (day, d) in stats.due_daily(Utc::now()) {
            self.journal.post(msg::daily(day, &d)).await;
        }
        let _ = stats.save(&*remote).await;

        let session = Arc::new(HostSession {
            session_id: session_id.clone(),
            started_at,
            guard,
            server: proc.clone(),
            log: Mutex::new(SessionLog::new(&session_id, &cfg.player_name, started_at)),
            stats: Mutex::new(stats),
            push_lock: Mutex::new(()),
            last_published: Mutex::new(None),
            base_manifest: head_before,
            versions,
            client_seen: false.into(),
            ending: false.into(),
            tasks: Default::default(),
        });
        *self.session.lock().await = Some(session.clone());
        self.spawn_session_tasks(&session);

        self.set_status(|s| {
            s.phase = Phase::Hosting;
            s.me_hosting = true;
            s.address = Some(address.clone());
            s.session_started_at = Some(started_at);
            s.next_checkpoint_at = Some(Utc::now() + chrono::Duration::minutes(cfg.checkpoint_minutes as i64));
            s.online = vec![];
            s.sync = None;
        });
        self.notice(NoticeLevel::Success, "Mundo aberto!", &format!("Seus amigos já podem entrar em {address}."));

        if open_game {
            if let Some(l) = &cfg.launcher_path {
                if let Err(e) = launcher::open(l) {
                    tracing::warn!(error = %e, "não consegui abrir o launcher");
                }
            }
        }
        Ok(())
    }

    fn forward_console(self: &Arc<Self>, mut lines: tokio::sync::broadcast::Receiver<String>) {
        let events = self.events.clone();
        let file = self.paths.logs().join("servidor-console.log");
        tokio::spawn(async move {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&file).ok();
            loop {
                match lines.recv().await {
                    Ok(line) => {
                        if let Some(f) = f.as_mut() {
                            use std::io::Write;
                            let _ = writeln!(f, "{line}");
                        }
                        events(Event::ServerLog { line });
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }

    fn spawn_session_tasks(self: &Arc<Self>, s: &Arc<HostSession>) {
        let mut tasks = s.tasks.lock().unwrap();
        // eventos do log do servidor
        {
            let me = self.clone();
            let sess = s.clone();
            let mut rx = s.server.subscribe();
            tasks.push(tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => me.on_log_event(&sess, ev).await,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            }));
        }
        // checkpoints
        {
            let me = self.clone();
            let sess = s.clone();
            tasks.push(tokio::spawn(async move {
                loop {
                    let minutes = me.config.read().await.checkpoint_minutes.max(5);
                    tokio::time::sleep(Duration::from_secs(minutes * 60)).await;
                    if sess.ending.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    if let Err(e) = me.checkpoint(&sess).await {
                        tracing::error!(error = %e, "checkpoint falhou");
                        me.notice(NoticeLevel::Warning, "Salvamento automático falhou", "Vou tentar de novo no próximo ciclo.");
                    }
                    me.set_status(|s| s.next_checkpoint_at = Some(Utc::now() + chrono::Duration::minutes(minutes as i64)));
                }
            }));
        }
        // fim de sessão, trava perdida, servidor caído, virada do dia
        {
            let me = self.clone();
            let sess = s.clone();
            tasks.push(tokio::spawn(async move { me.monitor(sess).await }));
        }
    }

    async fn on_log_event(self: &Arc<Self>, s: &Arc<HostSession>, ev: LogEvent) {
        let now = Utc::now();
        match ev {
            LogEvent::Joined(p) => {
                s.log.lock().await.joined(&p, now);
                self.journal.post(msg::joined(&p, now)).await;
                (self.events)(Event::PlayerJoined { name: p.clone() });
                self.publish_online(s).await;
            }
            LogEvent::Left(p) => {
                let played = s.log.lock().await.left(&p, now);
                if let Some(secs) = played {
                    self.journal.post(msg::left(&p, secs)).await;
                    let rec = s.stats.lock().await.check_record(&p, secs);
                    if let Some(prev) = rec {
                        self.journal.post(msg::record(&p, secs, prev)).await;
                    }
                }
                (self.events)(Event::PlayerLeft { name: p.clone() });
                self.publish_online(s).await;
            }
            LogEvent::Death { player, message, .. } => {
                s.stats.lock().await.on_death(&player, now);
                self.journal.post(msg::death(&player, &message)).await;
            }
            ref e @ LogEvent::Advancement { ref player, ref title, ref kind } => {
                s.stats.lock().await.on_advancement(player, title, now);
                let challenge = matches!(kind, crate::logparse::AdvancementKind::Challenge);
                self.journal.post(msg::advancement(player, title, challenge)).await;
                let first = {
                    let mut st = s.stats.lock().await;
                    if e.is_nether_entry() && st.first("nether", player, now) {
                        Some(msg::first_nether(player))
                    } else if e.is_end_entry() && st.first("end", player, now) {
                        Some(msg::first_end(player))
                    } else if e.is_dragon_kill() && st.first("dragon", player, now) {
                        Some(msg::dragon(player))
                    } else {
                        None
                    }
                };
                if let Some(m) = first {
                    self.journal.post(m).await;
                }
            }
            LogEvent::Chat { player, text } => {
                self.journal.post(msg::chat(&player, &text)).await;
            }
            _ => {}
        }
    }

    async fn publish_online(self: &Arc<Self>, s: &Arc<HostSession>) {
        let online = s.log.lock().await.online();
        self.set_status(|st| st.online = online.clone());
        if let Err(e) = s.guard.update(|l| l.online = online).await {
            tracing::warn!(error = %e, "não consegui publicar quem está online");
        }
    }

    async fn monitor(self: Arc<Self>, s: Arc<HostSession>) {
        use std::sync::atomic::Ordering;
        let mut empty_since: Option<tokio::time::Instant> = None;
        let mut warned_friends = false;
        let mut lost = s.guard.lost();
        let mut day = journal::day_sp(Utc::now());
        let started = tokio::time::Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                _ = lost.changed() => {}
            }
            if s.ending.load(Ordering::SeqCst) {
                return;
            }
            if *lost.borrow() {
                self.on_lock_lost(&s).await;
                return;
            }
            if !s.server.running().await {
                tracing::error!("o servidor fechou sozinho");
                self.journal
                    .post(msg::upload_failed(&s.log.lock().await.host, "O servidor fechou sozinho; o mundo foi salvo como estava."))
                    .await;
                let me = self.clone();
                tokio::spawn(async move { let _ = me.end_session(EndReason::UserStop).await; });
                return;
            }
            // virada do dia durante a sessão: resumo do dia anterior
            let today = journal::day_sp(Utc::now());
            if today != day {
                day = today;
                let due = s.stats.lock().await.due_daily(Utc::now());
                for (d, st) in due {
                    self.journal.post(msg::daily(d, &st)).await;
                }
            }
            let grace = Duration::from_secs(self.config.read().await.grace_minutes.max(1) * 60);
            let client = launcher::client_running(Some(s.server.pid));
            if client {
                s.client_seen.store(true, Ordering::SeqCst);
            }
            let players = match s.server.players().await {
                Ok(p) => p,
                Err(_) => s.log.lock().await.online(),
            };
            let friends: Vec<String> = {
                let me_name = self.config.read().await.player_name.clone();
                players.iter().filter(|p| **p != me_name).cloned().collect()
            };
            let seen = s.client_seen.load(Ordering::SeqCst);
            self.set_status(|st| {
                st.host_client_running = client;
                st.friends_still_playing = seen && !client && !friends.is_empty();
                st.online = players.clone();
            });
            if seen && !client && !friends.is_empty() && !warned_friends {
                warned_friends = true;
                self.notice(NoticeLevel::Warning, "Seus amigos ainda estão jogando", "Não desligue o PC: o mundo fecha sozinho quando o último sair.");
            }
            // Ninguém jogando: começa a contar a tolerância.
            let nobody = players.is_empty();
            let should_count = nobody && (seen || started.elapsed() > Duration::from_secs(10 * 60));
            if should_count {
                let since = *empty_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() >= grace {
                    tracing::info!("todos saíram; encerrando a sessão");
                    let me = self.clone();
                    tokio::spawn(async move { let _ = me.end_session(EndReason::Normal).await; });
                    return;
                }
            } else {
                empty_since = None;
            }
        }
    }

    /// Salvamento periódico (com o servidor rodando).
    async fn checkpoint(self: &Arc<Self>, s: &Arc<HostSession>) -> NmResult<()> {
        let Ok(_push) = s.push_lock.try_lock() else {
            tracing::info!("envio anterior ainda em andamento; pulando este checkpoint");
            return Ok(());
        };
        let snap = checkpoint::snapshot_live(&s.server, &self.paths).await?;
        let se = self.sync_engine().await?;
        let expected = s.last_published.lock().await.clone().or_else(|| s.base_manifest.clone());
        let players = s.log.lock().await.everyone();
        let meta = PushMeta {
            kind: ManifestKind::Checkpoint,
            session_id: s.session_id.clone(),
            session_started_at: s.started_at,
            host_player: self.config.read().await.player_name.clone(),
            players,
            versions: s.versions.server_versions(),
            note: None,
        };
        let m = se.push(&self.paths.staging(), meta, Some(&s.guard), expected.as_deref(), true, self.progress()).await?;
        *s.last_published.lock().await = Some(m.id.clone());
        let mut st = LocalState::load(&self.paths);
        if let Some(a) = st.active_session.as_mut() {
            a.last_published = Some(m.id.clone());
        }
        st.save(&self.paths)?;
        let remote = se.remote.clone();
        let _ = s.stats.lock().await.save(&*remote).await;
        self.journal
            .post(msg::checkpoint(&util::human_bytes(m.total_size), &util::human_bytes(snap.mirror.copied_bytes)))
            .await;
        self.set_status(|st| {
            st.last_checkpoint_at = Some(Utc::now());
            st.sync = None;
        });
        tracing::info!(manifest = %m.id, pause_ms = snap.pause_ms, "checkpoint publicado");
        Ok(())
    }

    /// "Passar a vez": avisa no chat com contagem regressiva, fecha, envia e libera.
    pub async fn pass_turn(self: &Arc<Self>) -> NmResult<()> {
        let s = self.session.lock().await.clone().ok_or_else(|| NmError::Other(anyhow::anyhow!("você não está hospedando")))?;
        let host = self.config.read().await.player_name.clone();
        let _ = s.guard.update(|l| l.handing_over = true).await;
        self.journal.post(msg::handing_over(&host)).await;
        for (msg_text, wait) in [
            ("O host vai passar a vez em 30 segundos. Quem quiser continuar: clique em Assumir agora no app.", 20),
            ("Fechando em 10 segundos…", 5),
            ("5…", 5),
        ] {
            let _ = s.server.say(msg_text).await;
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        self.end_session(EndReason::Handover).await
    }

    /// Encerra a sessão: para o servidor, envia o mundo e libera a vez (nessa ordem).
    pub async fn end_session(self: &Arc<Self>, reason: EndReason) -> NmResult<()> {
        use std::sync::atomic::Ordering;
        let Some(s) = self.session.lock().await.clone() else { return Ok(()) };
        if s.ending.swap(true, Ordering::SeqCst) {
            return Ok(()); // já encerrando
        }
        self.phase(Phase::Stopping);
        let _push = s.push_lock.lock().await;
        if reason != EndReason::Handover {
            let _ = s.server.say("O mundo está sendo salvo e fechado. Até a próxima!").await;
        }
        let graceful = s.server.stop(Duration::from_secs(180)).await;
        if !graceful {
            self.notice(NoticeLevel::Error, "O servidor precisou ser fechado à força", "O último salvamento automático está seguro.");
        }
        for t in s.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
        let now = Utc::now();
        let ranking = {
            let mut log = s.log.lock().await;
            log.close(now);
            log.ranking()
        };
        let res = self.finalize(&s, graceful).await;
        match res {
            Ok(()) => {
                let host = self.config.read().await.player_name.clone();
                self.journal.post(msg::world_saved(&host, (now - s.started_at).num_seconds(), &ranking)).await;
                self.maybe_gc(&s).await;
                if let Err(e) = s.guard.release().await {
                    tracing::error!(error = %e, "não consegui liberar a vez; ela expira sozinha em 5 min");
                }
                *self.session.lock().await = None;
                let mut st = LocalState::load(&self.paths);
                st.active_session = None;
                st.save(&self.paths)?;
                self.set_status(|st| {
                    st.phase = Phase::Idle;
                    st.me_hosting = false;
                    st.online = vec![];
                    st.session_started_at = None;
                    st.next_checkpoint_at = None;
                    st.host_client_running = false;
                    st.friends_still_playing = false;
                    st.sync = None;
                });
                self.notice(NoticeLevel::Success, "Mundo salvo na nuvem", "Pode desligar o PC.");
                let _ = self.refresh().await;
                Ok(())
            }
            Err(e) => {
                let host = self.config.read().await.player_name.clone();
                self.journal
                    .post(msg::upload_failed(&host, "O mundo está guardado neste PC; o app tenta de novo ao abrir. Não apague a pasta do app."))
                    .await;
                *self.session.lock().await = None;
                let h = e.human();
                self.set_status(|st| {
                    st.phase = Phase::Idle;
                    st.me_hosting = false;
                    st.last_error = Some(h);
                    st.recovery_pending = true;
                    st.sync = None;
                });
                self.notice(NoticeLevel::Error, "Não consegui enviar o mundo", "Ele está seguro neste PC. Tente de novo em Saúde do sistema → Recuperar.");
                Err(e)
            }
        }
    }

    async fn finalize(self: &Arc<Self>, s: &Arc<HostSession>, graceful: bool) -> NmResult<()> {
        let se = self.sync_engine().await?;
        checkpoint::snapshot_offline(&self.paths).await?;
        let players = s.log.lock().await.everyone();
        let meta = PushMeta {
            kind: if graceful { ManifestKind::Session } else { ManifestKind::Recovered },
            session_id: s.session_id.clone(),
            session_started_at: s.started_at,
            host_player: self.config.read().await.player_name.clone(),
            players,
            versions: s.versions.server_versions(),
            note: (!graceful).then(|| "Servidor fechado à força".to_string()),
        };
        let expected = s.last_published.lock().await.clone().or_else(|| s.base_manifest.clone());
        let m = se.push(&self.paths.staging(), meta, Some(&s.guard), expected.as_deref(), true, self.progress()).await?;
        // estatísticas da sessão (idempotente)
        {
            let log = s.log.lock().await.clone();
            let mut st = s.stats.lock().await;
            st.apply(&log);
            if let Err(e) = st.save(&*se.remote).await {
                tracing::warn!(error = %e, "estatísticas não gravadas");
            }
        }
        let mut st = LocalState::load(&self.paths);
        st.current_manifest = Some(m.id.clone());
        st.dirty = false;
        st.save(&self.paths)?;
        Ok(())
    }

    /// Falha na inicialização: envia o que existir, sem publicar se nada mudou de fato.
    #[allow(clippy::too_many_arguments)]
    async fn finalize_without_session(
        &self,
        guard: &LockGuard,
        se: &SyncEngine,
        session_id: &str,
        started_at: DateTime<Utc>,
        expected: Option<&str>,
        versions: &Versions,
        player: &str,
    ) {
        if !self.paths.world().join("level.dat").exists() {
            let mut st = LocalState::load(&self.paths);
            st.dirty = false;
            st.active_session = None;
            let _ = st.save(&self.paths);
            return;
        }
        let r = async {
            checkpoint::snapshot_offline(&self.paths).await?;
            let meta = PushMeta {
                kind: ManifestKind::Recovered,
                session_id: session_id.into(),
                session_started_at: started_at,
                host_player: player.into(),
                players: vec![],
                versions: versions.server_versions(),
                note: Some("Servidor não abriu".into()),
            };
            let m = se.push(&self.paths.staging(), meta, Some(guard), expected, true, crate::events::quiet()).await?;
            let mut st = LocalState::load(&self.paths);
            st.current_manifest = Some(m.id);
            st.dirty = false;
            st.active_session = None;
            st.save(&self.paths)?;
            Ok::<_, NmError>(())
        }
        .await;
        if let Err(e) = r {
            tracing::error!(error = %e, "não consegui enviar o mundo depois da falha de inicialização");
        }
    }

    async fn on_lock_lost(self: &Arc<Self>, s: &Arc<HostSession>) {
        use std::sync::atomic::Ordering;
        s.ending.store(true, Ordering::SeqCst);
        tracing::error!("outro PC assumiu o mundo enquanto eu hospedava");
        self.notice(NoticeLevel::Error, "Outro PC assumiu o mundo", "Seu progresso fica guardado na linha do tempo, sem sobrescrever ninguém.");
        let _ = s.server.say("Este PC perdeu a vez de hospedar. Salvando e fechando.").await;
        s.server.stop(Duration::from_secs(180)).await;
        let _push = s.push_lock.lock().await;
        let r = async {
            let se = self.sync_engine().await?;
            checkpoint::snapshot_offline(&self.paths).await?;
            let meta = PushMeta {
                kind: ManifestKind::Recovered,
                session_id: s.session_id.clone(),
                session_started_at: s.started_at,
                host_player: self.config.read().await.player_name.clone(),
                players: s.log.lock().await.everyone(),
                versions: s.versions.server_versions(),
                note: Some("Sessão interrompida: outro PC assumiu".into()),
            };
            se.push(&self.paths.staging(), meta, None, None, false, self.progress()).await
        }
        .await;
        if let Err(e) = r {
            tracing::error!(error = %e, "não consegui guardar a sessão interrompida");
        } else {
            let mut st = LocalState::load(&self.paths);
            st.dirty = false;
            st.current_manifest = None;
            st.active_session = None;
            let _ = st.save(&self.paths);
        }
        for t in s.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
        *self.session.lock().await = None;
        self.set_status(|st| {
            st.phase = Phase::Idle;
            st.me_hosting = false;
            st.sync = None;
        });
    }

    async fn maybe_gc(&self, s: &Arc<HostSession>) {
        let marker = self.paths.root.join("last-gc.txt");
        let recent = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|t| t.trim().parse::<DateTime<Utc>>().ok())
            .map(|t| Utc::now() - t < chrono::Duration::hours(20))
            .unwrap_or(false);
        if recent {
            return;
        }
        let Ok(se) = self.sync_engine().await else { return };
        match retention::gc(&se, &s.guard, retention::Policy::default(), false).await {
            Ok(r) => {
                tracing::info!(?r, "coleta de lixo concluída");
                let _ = std::fs::write(&marker, Utc::now().to_rfc3339());
                self.prune_local_cache(&se).await;
            }
            Err(e) => tracing::warn!(error = %e, "coleta de lixo pulada"),
        }
    }

    /// Cache local: mantém só objetos do HEAD (o resto pode ser baixado de novo).
    async fn prune_local_cache(&self, se: &SyncEngine) {
        let Ok(Some(h)) = se.head().await else { return };
        let Ok(m) = se.fetch_manifest(&h.manifest).await else { return };
        let keep = m.object_hashes();
        let cache = self.paths.cache_objects();
        let Ok(rd) = std::fs::read_dir(&cache) else { return };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if crate::hashing::is_object_hash(&n) && !keep.contains(&n) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Para a sessão a pedido (botão Parar, Sair, desligamento do Windows).
    pub async fn stop(self: &Arc<Self>, reason: EndReason) -> NmResult<()> {
        self.end_session(reason).await
    }

    /// Salvamento imediato (botão "Salvar agora" e testes).
    pub async fn force_checkpoint(self: &Arc<Self>) -> NmResult<()> {
        let s = self.session.lock().await.clone().ok_or_else(|| NmError::Other(anyhow::anyhow!("você não está hospedando")))?;
        self.checkpoint(&s).await
    }

    pub async fn is_hosting(&self) -> bool {
        self.session.lock().await.is_some()
    }

    /// Envia um comando para o console do servidor (aba Logs).
    pub async fn console(&self, command: &str) -> NmResult<String> {
        let s = self.session.lock().await.clone().ok_or_else(|| NmError::Other(anyhow::anyhow!("o servidor não está rodando")))?;
        let c = command.trim().trim_start_matches('/');
        if c == "stop" {
            return Err(NmError::Other(anyhow::anyhow!("use o botão Parar para fechar com segurança")));
        }
        s.server.cmd(c).await
    }

    // ------------------------------------------------------------------ amigos assumindo

    /// "Assumir agora": espera a vez ficar livre (o host está salvando) e então joga.
    pub async fn take_turn(self: &Arc<Self>, open_game: bool) -> NmResult<()> {
        self.phase(Phase::WaitingTurn);
        let lm = self.lock_manager().await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
        let previous;
        loop {
            match lm.status().await? {
                LockStatus::Free => {
                    previous = None;
                    break;
                }
                LockStatus::Stale { lock } => {
                    previous = Some(lock);
                    break;
                }
                LockStatus::Held { .. } if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                LockStatus::Held { lock } => {
                    self.phase(Phase::Idle);
                    return Err(NmError::HeldBy(Box::new(lock)));
                }
            }
        }
        if let Some(p) = previous {
            self.phase(Phase::Idle);
            return Err(NmError::StaleLock(Box::new(p)));
        }
        let me = self.config.read().await.player_name.clone();
        let prev_host = self.status().lock.and_then(|l| match l {
            LockStatus::Held { lock } => Some(lock.player),
            _ => None,
        });
        self.play(vec![], open_game).await?;
        self.journal.post(msg::took_over(&me, prev_host.as_deref())).await;
        Ok(())
    }

    // ------------------------------------------------------------------ recuperação

    /// Recupera uma sessão deste PC que caiu (luz, travamento): envia o que existir localmente.
    pub async fn recover(self: &Arc<Self>) -> NmResult<()> {
        let st = LocalState::load(&self.paths);
        if !st.dirty {
            return Ok(());
        }
        self.phase(Phase::Recovering);
        let active = st.active_session.clone();
        let cfg = self.config.read().await.clone();
        let se = self.sync_engine().await?;

        // 1) servidor órfão (o app caiu com ele rodando): para com segurança via RCON
        if let Some(pid) = server::orphan_server_pid(&self.paths) {
            tracing::warn!(pid, "servidor órfão encontrado; parando com segurança");
            if let Some(a) = &active {
                if let (Some(pass), Some(ip)) = (&a.rcon_password, &a.server_ip) {
                    if let Ok(mut r) = crate::rcon::Rcon::connect(&format!("{ip}:{}", crate::RCON_PORT), pass).await {
                        let _ = r.cmd("stop").await;
                    }
                }
            }
            for _ in 0..120 {
                if server::orphan_server_pid(&self.paths).is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if server::orphan_server_pid(&self.paths).is_some() {
                return Err(NmError::Other(anyhow::anyhow!(
                    "um servidor antigo ainda está rodando neste PC; feche-o (Gerenciador de Tarefas → Java) e tente de novo"
                )));
            }
        }

        // 2) a minha trava antiga é minha: pode remover
        let lm = self.lock_manager().await?;
        if let Some(a) = &active {
            lm.remove(&a.session_id).await?;
        }
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let me = LockInfo::new(&session_id, &cfg.player_name, LockPurpose::Maintenance);
        let guard = lm.acquire(me, &[]).await.ok();

        // 3) só publica se o HEAD ainda é o desta sessão (ninguém assumiu depois da queda)
        let head = se.head().await?.map(|h| h.manifest);
        let expected = active.as_ref().and_then(|a| a.last_published.clone().or_else(|| a.base_manifest.clone()));
        let world_ok = level_dat_ok(&self.paths);
        let can_publish = guard.is_some() && head == expected && world_ok;
        checkpoint::snapshot_offline(&self.paths).await?;
        let versions = Versions::load(&*se.remote).await?.unwrap_or_else(Versions::builtin);
        let meta = PushMeta {
            kind: ManifestKind::Recovered,
            session_id: active.as_ref().map(|a| a.session_id.clone()).unwrap_or(session_id),
            session_started_at: active.as_ref().map(|a| a.started_at).unwrap_or_else(Utc::now),
            host_player: cfg.player_name.clone(),
            players: vec![],
            versions: versions.server_versions(),
            note: Some(if can_publish {
                "Recuperado após queda".into()
            } else if !world_ok {
                "Recuperado após queda (level.dat danificado; não publicado)".into()
            } else {
                "Recuperado após queda (outro PC já tinha assumido)".into()
            }),
        };
        let res = se
            .push(&self.paths.staging(), meta, guard.as_ref(), expected.as_deref(), can_publish, self.progress())
            .await;
        match res {
            Ok(m) => {
                let mut st = LocalState::load(&self.paths);
                st.dirty = false;
                st.active_session = None;
                st.current_manifest = if can_publish { Some(m.id) } else { None };
                st.save(&self.paths)?;
                self.journal.post(msg::recovered(&cfg.player_name, can_publish)).await;
                self.notice(
                    NoticeLevel::Success,
                    "Mundo recuperado",
                    if can_publish {
                        "O que estava neste PC na hora da queda foi enviado."
                    } else {
                        "Sua cópia ficou guardada na linha do tempo; o mundo atual não foi sobrescrito."
                    },
                );
                if let Some(g) = guard {
                    let _ = g.release().await;
                }
                self.set_status(|s| {
                    s.recovery_pending = false;
                    s.phase = Phase::Idle;
                    s.sync = None;
                });
                Ok(())
            }
            Err(e) => {
                if let Some(g) = guard {
                    let _ = g.release().await;
                }
                self.phase(Phase::Idle);
                Err(e)
            }
        }
    }

    // ------------------------------------------------------------------ manutenção

    async fn maintenance<T, F, Fut>(self: &Arc<Self>, f: F) -> NmResult<T>
    where
        F: FnOnce(LockGuard, SyncEngine) -> Fut,
        Fut: std::future::Future<Output = (LockGuard, NmResult<T>)>,
    {
        let _op = self.op.try_lock().map_err(|_| NmError::Other(anyhow::anyhow!("já tem uma operação em andamento")))?;
        if self.is_hosting().await {
            return Err(NmError::Other(anyhow::anyhow!("feche a sessão antes")));
        }
        self.phase(Phase::Maintenance);
        let cfg = self.config.read().await.clone();
        let lm = self.lock_manager().await?;
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let guard = match lm.acquire(LockInfo::new(&session_id, &cfg.player_name, LockPurpose::Maintenance), &[]).await {
            Ok(g) => g,
            Err(e) => {
                self.phase(Phase::Idle);
                return Err(e);
            }
        };
        let se = self.sync_engine().await?;
        let (guard, res) = f(guard, se).await;
        let _ = guard.release().await;
        self.phase(Phase::Idle);
        let _ = self.refresh().await;
        res
    }

    /// Restaura uma versão antiga (cria um manifesto novo; nada é apagado).
    pub async fn restore(self: &Arc<Self>, manifest_id: String) -> NmResult<()> {
        let player = self.config.read().await.player_name.clone();
        let me = self.clone();
        self.maintenance(|guard, se| async move {
            let r = async {
                let target = se.fetch_manifest(&manifest_id).await?;
                let m = se.restore(&manifest_id, &guard, &player).await?;
                let label = format!(
                    "a sessão de {} de {} às {}",
                    target.host_player,
                    journal::date_br(target.created_at),
                    journal::hhmm(target.created_at)
                );
                me.journal.post(msg::restored(&player, &label)).await;
                // o mundo local passa a não corresponder ao HEAD: baixa de novo na próxima vez
                let mut st = LocalState::load(&me.paths);
                if !st.dirty {
                    st.current_manifest = None;
                    st.save(&me.paths)?;
                }
                tracing::info!(manifest = %m.id, "versão restaurada");
                Ok(())
            }
            .await;
            (guard, r)
        })
        .await
    }

    /// Coleta de lixo manual (Saúde → Liberar espaço).
    pub async fn gc(self: &Arc<Self>, dry_run: bool) -> NmResult<retention::GcReport> {
        self.maintenance(|guard, se| async move {
            let r = retention::gc(&se, &guard, retention::Policy::default(), dry_run).await;
            (guard, r)
        })
        .await
    }

    /// Importa uma pasta de mundo existente como nova versão (o dono, no assistente).
    pub async fn import_world(self: &Arc<Self>, folder: PathBuf) -> NmResult<()> {
        if !folder.join("level.dat").exists() {
            return Err(NmError::Other(anyhow::anyhow!("essa pasta não tem level.dat; escolha a pasta do mundo")));
        }
        let player = self.config.read().await.player_name.clone();
        let paths = self.paths.clone();
        let progress = self.progress();
        self.maintenance(|guard, se| async move {
            let r = async {
                let info = crate::players::level_info(&folder).map_err(NmError::Other)?;
                let versions = Versions::load(&*se.remote).await?.unwrap_or_else(Versions::builtin);
                if info.data_version > versions.world_data_version {
                    return Err(NmError::VersionMismatch(format!(
                        "o mundo é do Minecraft {} e o servidor é {}; abrir causaria perda de dados",
                        info.version_name, versions.minecraft
                    )));
                }
                // As configurações do servidor passam a ser as do próprio mundo.
                let mut ws = WorldSettings::load(&*se.remote).await?.unwrap_or_default();
                ws.difficulty = info.difficulty.clone();
                ws.hardcore = info.hardcore;
                ws.gamemode = info.game_type.clone();
                if let Some(seed) = info.seed {
                    ws.seed = seed.to_string();
                }
                ws.save(&*se.remote).await?;
                let tmp = paths.tmp().join(format!("importar-{}", uuid::Uuid::new_v4().simple()));
                let world = tmp.join("world");
                let f2 = folder.clone();
                let w2 = world.clone();
                tokio::task::spawn_blocking(move || copy_dir(&f2, &w2)).await.map_err(|e| anyhow::anyhow!(e))??;
                let head = se.head().await?.map(|h| h.manifest);
                if Versions::load(&*se.remote).await?.is_none() {
                    versions.save(&*se.remote).await?;
                }
                let meta = PushMeta {
                    kind: ManifestKind::Import,
                    session_id: uuid::Uuid::new_v4().simple().to_string(),
                    session_started_at: Utc::now(),
                    host_player: player,
                    players: vec![],
                    versions: versions.server_versions(),
                    note: Some(format!("Importado de {}", folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
                };
                let res = se.push(&tmp, meta, Some(&guard), head.as_deref(), true, progress).await;
                let _ = util::remove_dir_all_retry(&tmp);
                res?;
                let mut st = LocalState::load(&paths);
                if !st.dirty {
                    st.current_manifest = None;
                    st.save(&paths)?;
                }
                Ok(())
            }
            .await;
            (guard, r)
        })
        .await
    }

    /// Jogadores do mundo atual (baixa o HEAD se preciso). Só com ninguém hospedando neste PC.
    pub async fn world_players(self: &Arc<Self>) -> NmResult<Vec<crate::players::WorldPlayer>> {
        if self.is_hosting().await {
            return Err(NmError::Other(anyhow::anyhow!("feche a sessão antes")));
        }
        // Uma operação por vez sobre a pasta do mundo (abrir a tela duas vezes não pode montar duas cópias).
        let _op = self.op.lock().await;
        let se = self.sync_engine().await?;
        if !LocalState::load(&self.paths).dirty {
            se.pull(self.progress()).await?;
        }
        let mut nicks: Vec<String> = vec![self.config.read().await.player_name.clone()];
        if let Ok(st) = Stats::load(&*se.remote).await {
            nicks.extend(st.players.keys().cloned());
        }
        let world = self.paths.world();
        let r = tokio::task::spawn_blocking(move || crate::players::scan(&world, &nicks))
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
        self.phase(Phase::Idle);
        Ok(r)
    }

    /// Vincula o progresso de um UUID antigo ao nick (publica uma nova versão; nada é apagado).
    pub async fn migrate_player(self: &Arc<Self>, from_uuid: String, nick: String) -> NmResult<crate::players::Migration> {
        let player = self.config.read().await.player_name.clone();
        let paths = self.paths.clone();
        let progress = self.progress();
        self.maintenance(|guard, se| async move {
            let r = async {
                if LocalState::load(&paths).dirty {
                    return Err(NmError::Integrity("há uma sessão não enviada neste PC; recupere antes".into()));
                }
                se.pull(progress.clone()).await?;
                let world = paths.world();
                let (f, n) = (from_uuid.clone(), nick.clone());
                let mig = tokio::task::spawn_blocking(move || crate::players::migrate(&world, &f, &n))
                    .await
                    .map_err(|e| anyhow::anyhow!(e))??;
                checkpoint::snapshot_offline(&paths).await?;
                let head = se.head().await?.map(|h| h.manifest);
                let versions = Versions::load(&*se.remote).await?.unwrap_or_else(Versions::builtin);
                let meta = PushMeta {
                    kind: ManifestKind::Edit,
                    session_id: uuid::Uuid::new_v4().simple().to_string(),
                    session_started_at: Utc::now(),
                    host_player: player,
                    players: vec![nick.clone()],
                    versions: versions.server_versions(),
                    note: Some(format!("Progresso de {} vinculado ao nick {}", &from_uuid[..8], nick)),
                };
                let m = se.push(&paths.staging(), meta, Some(&guard), head.as_deref(), true, progress).await?;
                let mut st = LocalState::load(&paths);
                st.current_manifest = Some(m.id);
                st.save(&paths)?;
                Ok(mig)
            }
            .await;
            (guard, r)
        })
        .await
    }

    /// Linha do tempo (mais nova primeiro).
    pub async fn timeline(&self) -> NmResult<Vec<ManifestSummary>> {
        self.sync_engine().await?.timeline().await
    }

    pub async fn stats(&self) -> NmResult<Stats> {
        Stats::load(&*self.remote().await?).await
    }
}

fn level_dat_ok(paths: &Paths) -> bool {
    let p = paths.world().join("level.dat");
    match std::fs::File::open(&p) {
        Ok(f) => crate::nbt::read_gzip(f).is_ok(),
        Err(_) => false,
    }
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    for e in walkdir::WalkDir::new(src) {
        let e = e?;
        let rel = e.path().strip_prefix(src)?;
        let out = dst.join(rel);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&out)?;
        } else if e.file_type().is_file() && e.file_name() != "session.lock" {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(e.path(), &out)?;
        }
    }
    Ok(())
}
