//! Orquestração: o que acontece quando alguém clica em Jogar, durante a partida, quando o host
//! cai e no fim.
//!
//! [`Engine`] é o objeto único que a interface (Tauri) e a CLI usam. Além das operações
//! (jogar, passar a vez, restaurar...), ele mantém um **laço de fundo** enquanto o app está
//! aberto: conta à coordenação que este PC está online, aponta o proxy local para o host certo,
//! mantém a réplica do mundo em dia e, se o host cair com este PC jogando, assume sozinho.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use crate::config::Config;
use crate::coord::http::{ApiClient, HttpCoordinator};
use crate::coord::{Coordinator, DiaryEvent, Invite as CoordInvite, InviteOptions, LeaseInfo, LeasePurpose, LeaseStatus, Presence, PrivateSettings, Role, WorldView};
use crate::error::HumanError;
use crate::events::{Event, EventSink, NoticeLevel, SyncProgress, SyncProgressFn};
use crate::journal::{self, msg, Journal};
use crate::lease::{LeaseConfig, LeaseGuard, LeaseManager};
use crate::logparse::LogEvent;
use crate::manifest::{ManifestKind, ManifestSummary};
use crate::p2p::{self, NetMode, Node, PeerObjects};
use crate::paths::Paths;
use crate::proxy::{self, ProxyHandle, Route};
use crate::remote::{CloudRemote, EncryptedRemote, Remote};
use crate::secrets::Secrets;
use crate::server::{LaunchOptions, ServerProcess, WhitelistEntry};
use crate::stats::{SessionLog, Stats};
use crate::sync::{ActiveSession, LocalState, ObjectSource, PushMeta, SyncEngine};
use crate::versions::{Versions, WorldSettings};
use crate::{checkpoint, java, launcher, license, retention, server, serversdat, util};
use crate::{NmError, NmResult, SERVER_PORT};

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

/// Um membro, do jeito que a interface mostra.
#[derive(Debug, Clone, Serialize)]
pub struct MemberView {
    pub id: String,
    pub nick: String,
    pub role: Role,
    pub approved: bool,
    pub online: bool,
    pub playing: bool,
    pub hosting: bool,
    pub is_me: bool,
    pub last_seen: Option<DateTime<Utc>>,
    /// "direta" / "relay" (como este membro alcança o host).
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NetStatus {
    pub started: bool,
    /// Conectado à rede P2P (relay doméstico alcançável).
    pub online: bool,
    pub id_short: Option<String>,
    /// Como este PC alcança o host agora ("direta" / "relay").
    pub path_to_host: Option<String>,
    /// Ida e volta até o host (ms).
    pub rtt_ms: Option<u64>,
    pub proxy_port: Option<u16>,
    pub proxy_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub configured: bool,
    pub phase: Phase,
    pub player_name: String,
    pub world_name: String,
    pub is_owner: bool,
    pub lease: Option<LeaseStatus>,
    pub me_hosting: bool,
    pub head: Option<ManifestSummary>,
    pub head_seq: Option<u64>,
    pub replica_seq: u64,
    pub replicating: bool,
    pub online: Vec<String>,
    pub members: Vec<MemberView>,
    pub sync: Option<SyncProgress>,
    /// Endereço que o jogador usa no Minecraft (sempre o mesmo).
    pub address: String,
    /// O Minecraft deste PC está dentro do mundo agora.
    pub playing: bool,
    pub host_client_running: bool,
    pub friends_still_playing: bool,
    pub recovery_pending: bool,
    /// Troca automática de host em andamento (mensagem para a interface).
    pub failover: Option<String>,
    pub last_error: Option<HumanError>,
    pub session_started_at: Option<DateTime<Utc>>,
    pub next_checkpoint_at: Option<DateTime<Utc>>,
    pub last_checkpoint_at: Option<DateTime<Utc>>,
    /// A coordenação respondeu na última consulta.
    pub coord_ok: bool,
    pub net: NetStatus,
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

/// Por que um "Jogar" começou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayTrigger {
    /// Botão Jogar no app.
    Button,
    /// Tentativa de entrar pelo Minecraft com o mundo livre.
    Wake,
    /// O host caiu e este PC estava jogando.
    Failover,
}

/// Uma sessão hospedada neste PC.
struct HostSession {
    session_id: String,
    started_at: DateTime<Utc>,
    guard: LeaseGuard,
    server: Arc<ServerProcess>,
    log: Mutex<SessionLog>,
    stats: Mutex<Stats>,
    /// Garante um envio por vez.
    push_lock: Mutex<()>,
    last_published: Mutex<Option<String>>,
    base_manifest: Option<String>,
    versions: Versions,
    client_seen: AtomicBool,
    ending: AtomicBool,
    tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

/// Coordenação + nuvem prontas para uso.
#[derive(Clone)]
pub struct Backend {
    pub coord: Arc<dyn Coordinator>,
    pub remote: Arc<dyn Remote>,
}

struct Net {
    node: Arc<Node>,
    proxy: Option<ProxyHandle>,
}

pub struct Engine {
    pub paths: Paths,
    pub config: RwLock<Config>,
    secrets: RwLock<Secrets>,
    backend_override: Option<Backend>,
    net_mode: NetMode,
    events: EventSink,
    pub journal: Arc<Journal>,
    status: std::sync::RwLock<Status>,
    session: Mutex<Option<Arc<HostSession>>>,
    /// Serializa operações grandes (jogar, restaurar, recuperar...).
    op: Mutex<()>,
    pub lease_cfg: std::sync::RwLock<LeaseConfig>,
    world_settings: RwLock<Option<WorldSettings>>,
    net: RwLock<Option<Net>>,
    view: RwLock<Option<WorldView>>,
    /// Pedido de acordar o mundo vindo do proxy (alguém tentou entrar).
    wake_requested: AtomicBool,
    /// Último momento em que o Minecraft deste PC estava dentro do mundo (para o failover).
    last_playing: std::sync::Mutex<Option<tokio::time::Instant>>,
    /// Host alcançável por P2P agora (`session_number`).
    host_reachable: std::sync::Mutex<Option<u64>>,
    replicating: AtomicBool,
    background: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Engine {
    /// Abre o motor com a config e os segredos deste PC (coordenação e nuvem de verdade).
    pub fn open(paths: Paths, events: EventSink) -> anyhow::Result<Arc<Self>> {
        Self::build(paths, events, None, NetMode::Public)
    }

    /// Para testes: coordenação e nuvem dadas, rede P2P só local.
    pub fn with_backend(paths: Paths, backend: Backend, events: EventSink) -> anyhow::Result<Arc<Self>> {
        Self::build(paths, events, Some(backend), NetMode::LocalOnly)
    }

    fn build(paths: Paths, events: EventSink, backend: Option<Backend>, net_mode: NetMode) -> anyhow::Result<Arc<Self>> {
        paths.ensure()?;
        let config = Config::load(&paths);
        let mut secrets = Secrets::load(&paths.secrets()).unwrap_or_else(|e| {
            tracing::error!(error = %e, "segredos ilegíveis; será preciso entrar no mundo de novo");
            Secrets::default()
        });
        if secrets.identity().is_none() {
            secrets.identity_key = Some(hex::encode(p2p::new_secret()));
            secrets.save(&paths.secrets())?;
        }
        let journal = Journal::open(paths.root.join("journal-queue.json"), config.journal.clone(), None, &config.world_name);
        let status = Status {
            configured: config.ready(),
            phase: Phase::Idle,
            player_name: config.player_name.clone(),
            world_name: config.world_name.clone(),
            is_owner: config.is_owner,
            lease: None,
            me_hosting: false,
            head: None,
            head_seq: None,
            replica_seq: LocalState::load(&paths).replica.map(|r| r.seq).unwrap_or(0),
            replicating: false,
            online: vec![],
            members: vec![],
            sync: None,
            address: config.game_address(),
            playing: false,
            host_client_running: false,
            friends_still_playing: false,
            recovery_pending: LocalState::load(&paths).dirty,
            failover: None,
            last_error: None,
            session_started_at: None,
            next_checkpoint_at: None,
            last_checkpoint_at: None,
            coord_ok: false,
            net: NetStatus::default(),
            refreshed_at: None,
        };
        let _ = crate::sync::repair_interrupted_swap(&paths);
        Ok(Arc::new(Self {
            paths,
            config: RwLock::new(config),
            secrets: RwLock::new(secrets),
            backend_override: backend,
            net_mode,
            events,
            journal,
            status: std::sync::RwLock::new(status),
            session: Mutex::new(None),
            op: Mutex::new(()),
            lease_cfg: std::sync::RwLock::new(LeaseConfig::default()),
            world_settings: RwLock::new(None),
            net: RwLock::new(None),
            view: RwLock::new(None),
            wake_requested: AtomicBool::new(false),
            last_playing: std::sync::Mutex::new(None),
            host_reachable: std::sync::Mutex::new(None),
            replicating: AtomicBool::new(false),
            background: std::sync::Mutex::new(None),
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
            s.configured = c2.ready();
            s.player_name = c2.player_name.clone();
            s.world_name = c2.world_name.clone();
            s.is_owner = c2.is_owner;
            s.address = c2.game_address();
        });
        Ok(c2)
    }

    /// Identidade deste app (chave pública, hex).
    pub async fn my_id(&self) -> String {
        let s = self.secrets.read().await;
        p2p::public_hex(&s.identity().expect("identidade criada no build"))
    }

    /// Coordenação + nuvem (cifrada com a chave do mundo).
    pub async fn backend(&self) -> NmResult<Backend> {
        if let Some(b) = &self.backend_override {
            return Ok(b.clone());
        }
        let cfg = self.config.read().await;
        let (Some(url), Some(world)) = (cfg.coordinator_url.clone(), cfg.world_id.clone()) else {
            return Err(NmError::NotConfigured("este PC ainda não entrou em nenhum mundo".into()));
        };
        drop(cfg);
        let s = self.secrets.read().await;
        let id = s.identity().ok_or_else(|| NmError::NotConfigured("identidade do app ausente".into()))?;
        let key = s.world().ok_or_else(|| NmError::NotConfigured("chave do mundo ausente; entre de novo pelo convite".into()))?;
        drop(s);
        let api = ApiClient::new(&url, &world, id)?;
        let coord: Arc<dyn Coordinator> = Arc::new(HttpCoordinator::new(api.clone()));
        let remote: Arc<dyn Remote> = Arc::new(EncryptedRemote::new(Arc::new(CloudRemote::new(api)), key, self.paths.tmp()));
        Ok(Backend { coord, remote })
    }

    /// Endereço das atualizações do app no Worker do dono, com os cabeçalhos assinados.
    pub async fn updater_endpoint(&self) -> NmResult<(String, Vec<(&'static str, String)>)> {
        let cfg = self.config.read().await;
        let (Some(url), Some(world)) = (cfg.coordinator_url.clone(), cfg.world_id.clone()) else {
            return Err(NmError::NotConfigured("este PC ainda não entrou em nenhum mundo".into()));
        };
        drop(cfg);
        let id = self.secrets.read().await.identity().ok_or_else(|| NmError::NotConfigured("identidade ausente".into()))?;
        let api = ApiClient::new(&url, &world, id)?;
        let headers = api.auth_headers(&format!("/u/{world}")).to_vec();
        Ok((format!("{}/u/{world}/latest.json", url.trim_end_matches('/')), headers))
    }

    async fn sync_engine(&self) -> NmResult<SyncEngine> {
        let b = self.backend().await?;
        let peers = self.peer_source(true).await;
        Ok(SyncEngine::new(b.remote, b.coord, self.paths.clone()).with_peers(peers))
    }

    async fn lease_manager(&self) -> NmResult<LeaseManager> {
        Ok(LeaseManager::new(self.backend().await?.coord, self.lease_cfg.read().unwrap().clone()))
    }

    pub async fn node(&self) -> Option<Arc<Node>> {
        self.net.read().await.as_ref().map(|n| n.node.clone())
    }

    async fn set_route(&self, r: Route) {
        if let Some(Net { proxy: Some(p), .. }) = self.net.read().await.as_ref() {
            p.set_route(r);
        }
    }

    pub async fn proxy_addr(&self) -> Option<std::net::SocketAddr> {
        self.net.read().await.as_ref().and_then(|n| n.proxy.as_ref().map(|p| p.addr))
    }

    /// Amigos online (pela última fotografia da coordenação) como fonte de objetos P2P,
    /// o host primeiro e depois quem tem a réplica mais nova.
    async fn peer_source(&self, exclude_me: bool) -> Option<Arc<dyn ObjectSource>> {
        let node = self.node().await?;
        let view = self.view.read().await.clone()?;
        let me = node.id_hex();
        let host = view.lease.held().map(|l| l.holder.clone());
        let mut cands: Vec<(bool, u64, iroh::EndpointAddr)> = view
            .members
            .iter()
            .filter(|m| m.approved && !(exclude_me && m.id == me))
            .filter_map(|m| {
                let p = m.presence.as_ref()?;
                let addr = p2p::parse_addr(p.endpoint.as_deref()?).ok()?;
                Some((Some(&m.id) == host.as_ref(), p.replica_seq, addr))
            })
            .collect();
        cands.sort_by_key(|c| std::cmp::Reverse((c.0, c.1)));
        if cands.is_empty() {
            return None;
        }
        Some(Arc::new(PeerObjects { node, peers: cands.into_iter().map(|c| c.2).collect() }))
    }

    pub async fn world_settings(&self) -> NmResult<WorldSettings> {
        if let Some(w) = self.world_settings.read().await.clone() {
            return Ok(w);
        }
        let r = self.backend().await?.remote;
        let w = WorldSettings::load(&*r).await?.unwrap_or_default();
        *self.world_settings.write().await = Some(w.clone());
        Ok(w)
    }

    /// Muda as configurações do mundo (só com ninguém hospedando).
    pub async fn save_world_settings(&self, w: WorldSettings) -> NmResult<()> {
        let lm = self.lease_manager().await?;
        if !lm.status().await?.is_free() {
            return Err(NmError::Other(anyhow::anyhow!("só dá para mudar com ninguém hospedando")));
        }
        let r = self.backend().await?.remote;
        w.save(&*r).await?;
        self.journal.set_world_name(&w.world_name);
        *self.world_settings.write().await = Some(w.clone());
        let _ = self.update_config(|c| c.world_name = w.world_name.clone()).await;
        Ok(())
    }

    // ------------------------------------------------------------------ mundo: criar e entrar

    /// O dono cria o mundo na coordenação dele. Se `folder` vier, importa esse mundo.
    pub async fn create_world(self: &Arc<Self>, coordinator_url: &str, admin_token: &str, name: &str, nick: &str, folder: Option<PathBuf>) -> NmResult<()> {
        if !Config::is_valid_player_name(nick) {
            return Err(NmError::Other(anyhow::anyhow!("nick inválido (3 a 16 letras, números ou _)")));
        }
        let id = self.secrets.read().await.identity().expect("identidade");
        let lic = license::AdminTokenLicense { admin_token: Some(admin_token.to_string()) };
        let cred = license::License::create_world_credential(&lic).unwrap_or_default();
        let world_id = crate::coord::http::create_world(coordinator_url, id, &cred, name, nick).await?;
        let key = p2p::new_secret();
        self.update_secrets(|s| {
            s.world_key = Some(hex::encode(key));
            s.admin_token = Some(admin_token.to_string());
        })
        .await?;
        self.update_config(|c| {
            c.coordinator_url = Some(coordinator_url.trim_end_matches('/').to_string());
            c.world_id = Some(world_id.clone());
            c.world_name = name.to_string();
            c.player_name = nick.to_string();
            c.is_owner = true;
            c.setup_complete = true;
        })
        .await?;
        // versões e configurações do mundo
        let b = self.backend().await?;
        if Versions::load(&*b.remote).await?.is_none() {
            Versions::builtin().save(&*b.remote).await?;
        }
        let ws = WorldSettings { world_name: name.to_string(), ..Default::default() };
        if WorldSettings::load(&*b.remote).await?.is_none() {
            ws.save(&*b.remote).await?;
        }
        if let Some(f) = folder {
            let was_local = f == self.paths.world();
            self.import_world(f).await?;
            if was_local {
                // o mundo que a v1 deixou aqui virou a primeira versão do mundo novo
                let mut st = LocalState::load(&self.paths);
                st.dirty = false;
                st.active_session = None;
                st.current_manifest = None;
                st.save(&self.paths)?;
                self.set_status(|s| s.recovery_pending = false);
            }
        }
        let _ = self.refresh().await;
        Ok(())
    }

    /// Um amigo entra pelo convite (link ou código). Sem conta, sem senha.
    pub async fn join_world(self: &Arc<Self>, invite: &str, nick: &str) -> NmResult<crate::coord::http::JoinResult> {
        if !Config::is_valid_player_name(nick) {
            return Err(NmError::Other(anyhow::anyhow!("nick inválido (3 a 16 letras, números ou _)")));
        }
        let inv = crate::invite::Invite::parse(invite).map_err(|e| NmError::Other(e.into()))?;
        let id = self.secrets.read().await.identity().expect("identidade");
        let r = crate::coord::http::redeem_invite(&inv.base, id, &inv.token, nick).await?;
        let key = inv.world_key().map_err(|e| NmError::Other(e.into()))?;
        self.update_secrets(|s| s.world_key = Some(hex::encode(*key))).await?;
        self.update_config(|c| {
            c.coordinator_url = Some(inv.base.clone());
            c.world_id = Some(r.world_id.clone());
            c.world_name = r.name.clone();
            c.player_name = nick.to_string();
            c.is_owner = false;
            c.setup_complete = true;
        })
        .await?;
        let _ = self.refresh().await;
        Ok(r)
    }

    /// Link de convite novo (só o dono).
    pub async fn create_invite(&self, opts: InviteOptions) -> NmResult<(CoordInvite, String)> {
        let b = self.backend().await?;
        let inv = b.coord.create_invite(&opts).await?;
        let token = inv.token.clone().ok_or_else(|| NmError::Remote("convite sem token".into()))?;
        let key = self.secrets.read().await.world().ok_or_else(|| NmError::NotConfigured("chave do mundo ausente".into()))?;
        let url = self.config.read().await.coordinator_url.clone().unwrap_or_default();
        let link = crate::invite::Invite::new(&url, &token, &key).link();
        Ok((inv, link))
    }

    pub async fn invites(&self) -> NmResult<Vec<CoordInvite>> {
        self.backend().await?.coord.invites().await
    }

    pub async fn revoke_invite(&self, id: &str) -> NmResult<()> {
        self.backend().await?.coord.revoke_invite(id).await
    }

    pub async fn set_member(self: &Arc<Self>, id: &str, approved: Option<bool>, remove: bool) -> NmResult<()> {
        self.backend().await?.coord.set_member(id, approved, remove).await?;
        let _ = self.refresh().await;
        Ok(())
    }

    pub async fn private_settings(&self) -> NmResult<PrivateSettings> {
        self.backend().await?.coord.private_settings().await
    }

    pub async fn set_private_settings(&self, s: PrivateSettings) -> NmResult<()> {
        if let Some(w) = &s.discord_webhook {
            if !w.is_empty() && !journal::is_valid_webhook(w) {
                return Err(NmError::Other(anyhow::anyhow!("isso não parece um webhook do Discord")));
            }
        }
        self.backend().await?.coord.set_private_settings(&s).await?;
        if self.is_hosting().await {
            self.journal.set_webhook(s.discord_webhook.clone());
        }
        Ok(())
    }

    /// Diário do mundo (dentro do app), do mais novo para o mais velho.
    pub async fn diary(&self, limit: usize) -> NmResult<Vec<DiaryEvent>> {
        self.backend().await?.coord.events(limit).await
    }

    fn diary_post(self: &Arc<Self>, kind: &str, player: Option<&str>, text: String) {
        let me = self.clone();
        let ev = DiaryEvent { at: Utc::now(), kind: kind.into(), player: player.map(str::to_string), text };
        tokio::spawn(async move {
            if let Ok(b) = me.backend().await {
                if let Err(e) = b.coord.post_events(&[ev]).await {
                    tracing::debug!(error = %e, "evento do diário não enviado");
                }
            }
        });
    }

    // ------------------------------------------------------------------ rede e laço de fundo

    /// Liga a rede P2P e o proxy local e começa o laço de fundo. Chamado ao abrir o app
    /// (depois que o PC entrou num mundo). Idempotente.
    pub async fn start_network(self: &Arc<Self>) -> NmResult<()> {
        if self.net.read().await.is_some() {
            return Ok(());
        }
        let secret = self.secrets.read().await.identity().expect("identidade");
        let node = Node::start(&secret, self.net_mode, &self.paths).await.map_err(|e| NmError::P2p(format!("{e:#}")))?;
        let port = if self.backend_override.is_some() { 0 } else { self.config.read().await.proxy_port };
        let wake_me = Arc::downgrade(self);
        let unreachable_me = Arc::downgrade(self);
        let proxy = match proxy::start(
            port,
            Some(node.clone()),
            Arc::new(move || {
                if let Some(me) = wake_me.upgrade() {
                    me.wake_requested.store(true, Ordering::SeqCst);
                }
            }),
            Arc::new(move || {
                if let Some(me) = unreachable_me.upgrade() {
                    *me.host_reachable.lock().unwrap() = None;
                }
            }),
        )
        .await
        {
            Ok(p) => {
                let a = p.addr;
                self.set_status(|s| {
                    s.net.proxy_port = Some(a.port());
                    s.net.proxy_error = None;
                });
                Some(p)
            }
            Err(e) => {
                tracing::error!(error = %e, "proxy local não abriu");
                self.set_status(|s| s.net.proxy_error = Some(format!("{e:#}")));
                None
            }
        };
        let id_short = node.endpoint.id().fmt_short().to_string();
        self.set_status(|s| {
            s.net.started = true;
            s.net.id_short = Some(id_short);
        });
        *self.net.write().await = Some(Net { node, proxy });
        let me = self.clone();
        let h = tokio::spawn(async move { me.background_loop().await });
        *self.background.lock().unwrap() = Some(h);
        Ok(())
    }

    /// Para o laço de fundo e a rede (ao sair do app).
    pub async fn stop_network(&self) {
        if let Some(h) = self.background.lock().unwrap().take() {
            h.abort();
        }
        if let Some(n) = self.net.write().await.take() {
            n.node.shutdown().await;
        }
    }

    async fn background_loop(self: Arc<Self>) {
        let mut tick: u64 = 0;
        loop {
            tick += 1;
            if let Err(e) = self.tick(tick).await {
                tracing::debug!(error = %e, "ciclo de fundo com erro");
            }
            let busy = self.is_hosting().await
                || self.status().playing
                || matches!(self.status().lease, Some(LeaseStatus::Held { .. }))
                || self.status().failover.is_some();
            let wait = if busy { 4 } else { 20 };
            for _ in 0..(wait * 4) {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if self.wake_requested.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
    }

    /// Um ciclo do laço de fundo. Uma única ida à coordenação: a presença leva o que este PC
    /// sabe e volta com a fotografia do mundo.
    async fn tick(self: &Arc<Self>, n: u64) -> NmResult<()> {
        let cfg = self.config.read().await.clone();
        if !cfg.ready() {
            return Ok(());
        }
        let Some(node) = self.node().await else { return Ok(()) };
        let me = node.id_hex();
        let hosting = self.is_hosting().await;

        // 1) o Minecraft deste PC está jogando?
        // `PAULETOR3_PRETEND_PLAYING=1`: só para o teste automático de failover (sem cliente de jogo).
        let pretend = std::env::var_os("PAULETOR3_PRETEND_PLAYING").is_some();
        let playing = pretend || self.net.read().await.as_ref().and_then(|n| n.proxy.as_ref().map(|p| p.playing())).unwrap_or(false);
        if playing {
            *self.last_playing.lock().unwrap() = Some(tokio::time::Instant::now());
        }
        let recently_playing = self
            .last_playing
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < Duration::from_secs(150))
            .unwrap_or(false);

        // 2) o host (pela última fotografia) responde por P2P? O aviso mantém a vez viva se só
        //    a internet do host com a nuvem caiu.
        let prev_view = self.view.read().await.clone();
        let mut path = None;
        let mut rtt = None;
        match prev_view.as_ref().and_then(|v| v.lease.held().cloned()) {
            Some(l) if l.holder != me && l.purpose == LeasePurpose::Hosting => {
                let reach = match l.endpoint.as_deref().map(p2p::parse_addr) {
                    Some(Ok(addr)) if playing || n.is_multiple_of(3) || self.host_reachable.lock().unwrap().is_none() => match node.ping(&addr).await {
                        Ok(d) => {
                            rtt = Some(d.as_millis() as u64);
                            true
                        }
                        Err(_) => false,
                    },
                    Some(Ok(_)) => self.host_reachable.lock().unwrap().is_some(),
                    _ => false,
                };
                *self.host_reachable.lock().unwrap() = reach.then_some(l.session_number);
                if let Ok(id) = p2p::parse_id(&l.holder) {
                    path = node.path_kind(&id).await.map(str::to_string);
                }
            }
            _ => *self.host_reachable.lock().unwrap() = None,
        }
        let online = node.wait_online(Duration::from_millis(1)).await;
        self.set_status(|s| {
            s.playing = playing;
            s.net.online = online || self.net_mode == NetMode::LocalOnly;
            s.net.path_to_host = path.clone();
            s.net.rtt_ms = rtt;
        });

        // 3) presença → fotografia do mundo
        let presence = Presence {
            endpoint: Some(node.addr_json()),
            replica_seq: LocalState::load(&self.paths).replica.map(|r| r.seq).unwrap_or(0),
            playing,
            host_reachable: *self.host_reachable.lock().unwrap(),
            app_version: crate::APP_VERSION.to_string(),
            path,
        };
        let view = match async { self.backend().await?.coord.presence(&presence).await }.await {
            Ok(v) => v,
            Err(e) => {
                self.set_status(|s| s.coord_ok = false);
                if !hosting {
                    self.set_route(Route::Waiting {
                        motd: "§cSem conexão com a nuvem do mundo §7— confira a internet".into(),
                        message: "O app não está conseguindo falar com a nuvem do mundo.\n\nConfira a internet e tente de novo.".into(),
                        wake: false,
                    })
                    .await;
                }
                return Err(e);
            }
        };
        self.apply_view(view.clone()).await?;

        // 4) quem pode falar comigo por P2P
        node.set_members(view.members.iter().filter(|m| m.approved && m.id != me).map(|m| m.id.as_str()));

        // 6) rota do proxy
        if hosting {
            self.set_route(Route::Local(SERVER_PORT)).await;
        } else {
            match &view.lease {
                LeaseStatus::Held { lease } if lease.holder != me && lease.purpose == LeasePurpose::Hosting => match lease.endpoint.as_deref().map(p2p::parse_addr) {
                    Some(Ok(addr)) => self.set_route(Route::Remote(addr)).await,
                    _ => self.set_route(waiting(&format!("§e{} está abrindo o mundo...", lease.player), "O mundo está abrindo. Entre de novo em alguns segundos.", false)).await,
                },
                LeaseStatus::Held { lease } if lease.purpose == LeasePurpose::Maintenance => {
                    self.set_route(waiting(&format!("§e{} está mexendo no mundo §7(manutenção)", lease.player), "Manutenção do mundo em andamento. Tente de novo em alguns minutos.", false)).await
                }
                LeaseStatus::Held { .. } => {}
                LeaseStatus::Free { previous: Some(prev) } if prev.purpose == LeasePurpose::Hosting && recently_playing => {
                    self.set_route(waiting("§eTrocando de host... §7não feche o jogo", "Trocando de host...\n\nEntre de novo em alguns segundos: o mundo continua de onde parou.", false)).await
                }
                LeaseStatus::Free { .. } => {
                    let phase = self.status().phase;
                    if matches!(phase, Phase::Preparing | Phase::Starting) {
                        let pct = self.status().sync.map(|s| s.percent()).unwrap_or(0.0);
                        self.set_route(waiting(&format!("§eAcordando o mundo no seu PC... §f{pct:.0}%"), "Acordando o mundo...\n\nEntre de novo em alguns segundos.", false)).await;
                    } else {
                        self.set_route(Route::Waiting {
                            motd: if cfg.wake_on_join { "§aMundo livre §7— entre para acordar o mundo".into() } else { "§aMundo livre §7— abra pelo app".into() },
                            message: "Acordando o mundo no seu PC...\n\nEntre de novo em alguns segundos.".into(),
                            wake: cfg.wake_on_join,
                        })
                        .await;
                    }
                }
            }
        }

        // 7) failover automático e acordar
        let free = view.lease.is_free();
        let idle = self.status().phase == Phase::Idle && !hosting;
        if free && idle {
            let host_fell = matches!(&view.lease, LeaseStatus::Free { previous: Some(p) } if p.holder != me && p.purpose == LeasePurpose::Hosting);
            let wake = self.wake_requested.swap(false, Ordering::SeqCst);
            let trigger = if host_fell && recently_playing && cfg.auto_failover {
                Some(PlayTrigger::Failover)
            } else if wake && cfg.wake_on_join {
                Some(PlayTrigger::Wake)
            } else {
                None
            };
            if let Some(t) = trigger {
                let me2 = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = me2.play_with(t, false).await {
                        tracing::warn!(error = %e, trigger = ?t, "não assumi o mundo");
                    }
                });
                return Ok(());
            }
        } else {
            self.wake_requested.store(false, Ordering::SeqCst);
        }

        // 8) réplica em segundo plano (sem hospedar, com o mundo publicado mais novo que o meu)
        if cfg.background_replica && !hosting && idle {
            if let Some(h) = &view.head {
                if h.seq > presence.replica_seq && !self.replicating.swap(true, Ordering::SeqCst) {
                    let me2 = self.clone();
                    tokio::spawn(async move {
                        me2.set_status(|s| s.replicating = true);
                        let r = async {
                            let se = me2.sync_engine().await?;
                            se.prefetch(crate::events::quiet()).await
                        }
                        .await;
                        if let Err(e) = r {
                            tracing::info!(error = %e, "réplica em segundo plano adiada");
                        }
                        let seq = LocalState::load(&me2.paths).replica.map(|r| r.seq).unwrap_or(0);
                        me2.set_status(|s| {
                            s.replicating = false;
                            s.replica_seq = seq;
                        });
                        me2.replicating.store(false, Ordering::SeqCst);
                    });
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ estado

    /// Relê o estado do mundo na coordenação. Barato.
    pub async fn refresh(self: &Arc<Self>) -> NmResult<()> {
        let view = match async { self.backend().await?.coord.view().await }.await {
            Ok(v) => v,
            Err(e) => {
                self.set_status(|s| s.coord_ok = false);
                return Err(e);
            }
        };
        self.apply_view(view).await
    }

    /// Atualiza o status da interface a partir de uma fotografia da coordenação.
    async fn apply_view(self: &Arc<Self>, view: WorldView) -> NmResult<()> {
        let b = self.backend().await?;
        let head_summary = match &view.head {
            Some(h) => {
                let se = SyncEngine::new(b.remote.clone(), b.coord.clone(), self.paths.clone()).with_peers(self.peer_source(true).await);
                se.fetch_manifest(&h.manifest).await.ok().map(|m| m.summary())
            }
            None => None,
        };
        if self.world_settings.read().await.is_none() {
            if let Ok(Some(w)) = WorldSettings::load(&*b.remote).await {
                self.journal.set_world_name(&w.world_name);
                *self.world_settings.write().await = Some(w);
            }
        }
        let me_hosting = self.session.lock().await.is_some();
        let prev = self.view.read().await.clone();
        let cfg = self.config.read().await.clone();
        let me = b.coord.me();
        if !me_hosting && cfg.notify_toasts {
            self.toast_changes(prev.as_ref().map(|v| &v.lease), &view.lease, &cfg, &me);
        }
        let host = view.lease.held().map(|l| l.holder.clone());
        let members: Vec<MemberView> = view
            .members
            .iter()
            .map(|m| MemberView {
                id: m.id.clone(),
                nick: m.nick.clone(),
                role: m.role,
                approved: m.approved,
                online: m.presence.is_some(),
                playing: m.presence.as_ref().map(|p| p.playing).unwrap_or(false),
                hosting: Some(&m.id) == host.as_ref(),
                is_me: m.id == me,
                last_seen: m.last_seen,
                path: m.presence.as_ref().and_then(|p| p.path.clone()),
            })
            .collect();
        let online = match &view.lease {
            LeaseStatus::Held { lease } => lease.online.clone(),
            _ => vec![],
        };
        let failover = match &view.lease {
            LeaseStatus::Free { previous: Some(p) } if p.purpose == LeasePurpose::Hosting && p.holder != me => {
                Some(format!("{} saiu de repente. Trocando de host...", p.player))
            }
            _ => None,
        };
        let is_owner = view.me.as_ref().map(|m| m.role == Role::Owner).unwrap_or(cfg.is_owner);
        if view.name != cfg.world_name && !view.name.is_empty() {
            let n = view.name.clone();
            let _ = self.update_config(|c| c.world_name = n).await;
        }
        self.set_status(|s| {
            if !me_hosting {
                s.online = online;
            }
            s.lease = Some(view.lease.clone());
            s.head = head_summary;
            s.head_seq = view.head.as_ref().map(|h| h.seq);
            s.members = members;
            s.failover = if me_hosting { None } else { failover };
            s.me_hosting = me_hosting;
            s.is_owner = is_owner;
            s.coord_ok = true;
            s.refreshed_at = Some(Utc::now());
            s.recovery_pending = LocalState::load(&self.paths).dirty;
            s.replica_seq = LocalState::load(&self.paths).replica.map(|r| r.seq).unwrap_or(0);
        });
        *self.view.write().await = Some(view);
        Ok(())
    }

    fn toast_changes(&self, prev: Option<&LeaseStatus>, now: &LeaseStatus, cfg: &Config, me: &str) {
        let Some(prev) = prev else { return };
        // o que este próprio PC fez já tem aviso próprio
        if now.held().map(|l| l.holder == me).unwrap_or(false) {
            return;
        }
        match (prev, now) {
            (LeaseStatus::Free { .. }, LeaseStatus::Held { lease }) if lease.purpose == LeasePurpose::Hosting => {
                self.notice(NoticeLevel::Info, &format!("{} abriu o mundo", lease.player), "Clique em Jogar para entrar.")
            }
            (LeaseStatus::Held { lease: a }, LeaseStatus::Held { lease: b }) if a.session_number == b.session_number => {
                if cfg.notify_join_leave {
                    for p in b.online.iter().filter(|p| !a.online.contains(p) && **p != cfg.player_name) {
                        self.notice(NoticeLevel::Info, &format!("{p} entrou no mundo"), "");
                    }
                }
                if !a.handing_over && b.handing_over {
                    self.notice(NoticeLevel::Warning, &format!("{} vai passar a vez", b.player), "Clique em Assumir agora para continuar no seu PC.");
                }
            }
            (LeaseStatus::Held { lease: a }, LeaseStatus::Held { lease: b }) if a.purpose == LeasePurpose::Hosting => {
                self.notice(NoticeLevel::Info, &format!("{} assumiu o mundo", b.player), "Entre de novo pelo Minecraft: o endereço é o mesmo.")
            }
            (LeaseStatus::Held { lease }, LeaseStatus::Free { previous: None }) if lease.purpose == LeasePurpose::Hosting => {
                self.notice(NoticeLevel::Success, "Mundo salvo", &format!("A sessão de {} terminou e o mundo está guardado.", lease.player))
            }
            (LeaseStatus::Held { lease }, LeaseStatus::Free { previous: Some(_) }) if lease.purpose == LeasePurpose::Hosting => self.notice(
                NoticeLevel::Warning,
                &format!("A sessão de {} caiu", lease.player),
                "Quem estava jogando assume sozinho. Se ninguém estava, o mundo está guardado no último salvamento.",
            ),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ jogar

    /// Clique em "Jogar". Se alguém já hospeda, só abre o jogo (o proxy leva até o host);
    /// se o mundo está livre, acorda o servidor neste PC.
    pub async fn play(self: &Arc<Self>, open_game: bool) -> NmResult<()> {
        let _ = self.refresh().await;
        let held_by_other = {
            let me = self.my_id().await;
            matches!(self.status().lease, Some(LeaseStatus::Held { ref lease }) if lease.holder != me)
        };
        if held_by_other {
            self.open_game().await;
            return Ok(());
        }
        self.play_with(PlayTrigger::Button, open_game).await
    }

    pub async fn play_with(self: &Arc<Self>, trigger: PlayTrigger, open_game: bool) -> NmResult<()> {
        let _op = self.op.try_lock().map_err(|_| NmError::Other(anyhow::anyhow!("já tem uma operação em andamento")))?;
        if self.session.lock().await.is_some() {
            if open_game {
                self.open_game().await;
            }
            return Ok(());
        }
        self.set_status(|s| s.last_error = None);
        if trigger == PlayTrigger::Failover {
            self.set_status(|s| s.failover = Some("Trocando de host: este PC está assumindo o mundo...".into()));
        }
        let res = self.play_inner(trigger, open_game).await;
        if let Err(e) = &res {
            let quiet = trigger != PlayTrigger::Button && matches!(e, NmError::HeldBy(_) | NmError::NotBestCandidate { .. });
            let h = e.human();
            self.set_status(|s| {
                if !quiet {
                    s.last_error = Some(h);
                }
                s.phase = Phase::Idle;
                s.sync = None;
                s.failover = None;
            });
        }
        res
    }

    async fn play_inner(self: &Arc<Self>, trigger: PlayTrigger, open_game: bool) -> NmResult<()> {
        let cfg = self.config.read().await.clone();
        if !cfg.setup_complete {
            return Err(NmError::NotConfigured("termine a configuração primeiro".into()));
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
        let b = self.backend().await?;
        let se = self.sync_engine().await?;

        // 1) versões fixadas (o dono cria na primeira vez)
        let versions = match Versions::load(&*b.remote).await? {
            Some(v) => v,
            None if cfg.is_owner => {
                let v = Versions::builtin();
                v.save(&*b.remote).await?;
                v
            }
            None => return Err(NmError::NotConfigured("o dono ainda não criou o mundo".into())),
        };
        let settings = self.world_settings().await?;

        // 2) a vez
        let lm = self.lease_manager().await?;
        let previous_host = match lm.status().await? {
            LeaseStatus::Free { previous: Some(p) } if p.purpose == LeasePurpose::Hosting && p.holder != b.coord.me() => Some(p),
            _ => None,
        };
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let endpoint = self.node().await.map(|n| n.addr_json());
        let req = LeaseManager::new_request(&session_id, &cfg.player_name, LeasePurpose::Hosting, endpoint, se.replica_seq());
        let guard = if trigger == PlayTrigger::Failover {
            lm.acquire_patiently(req, Duration::from_secs(40)).await?
        } else {
            lm.acquire(req).await?
        };
        let session_number = guard.session_number();

        let started_at = Utc::now();
        // Daqui em diante, qualquer erro precisa soltar a vez (o mundo ainda não mudou).
        let prepared = async {
            // 3) mundo (amigos primeiro, nuvem depois)
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
            let whitelist = self.whitelist().await;
            server::write_whitelist(&self.paths, &whitelist)?;
            server::write_properties(&self.paths, &settings, SERVER_PORT, &rcon_password, !whitelist.is_empty())?;
            // Espelho inicial com o servidor ainda parado: cada checkpoint só copia o que mudou.
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
            server_port: Some(SERVER_PORT),
            session_number: Some(session_number),
        });
        st.save(&self.paths)?;

        self.phase(Phase::Starting);
        let heap = crate::heap::effective_heap_mb(cfg.ram_mb);
        let proc = ServerProcess::launch(&self.paths, LaunchOptions { java, heap_mb: heap, rcon_password }).await?;
        let lines = proc.subscribe_lines();
        self.forward_console(lines);
        let ready = proc.wait_ready(proc.subscribe(), Duration::from_secs(600)).await;
        if let Err(e) = ready {
            tracing::error!(error = %e, "servidor não abriu");
            let _ = proc.stop(Duration::from_secs(60)).await;
            self.finalize_without_session(&guard, &se, &session_id, started_at, head_before.as_deref(), &versions, &cfg.player_name).await;
            let _ = guard.release().await;
            return Err(e);
        }

        // 6) o túnel e o proxy passam a levar todo mundo para este servidor
        if let Some(n) = self.node().await {
            n.set_hosting(Some(SERVER_PORT));
        }
        self.set_route(Route::Local(SERVER_PORT)).await;

        // 7) estatísticas e diário
        if let Ok(p) = b.coord.private_settings().await {
            self.journal.set_webhook(p.discord_webhook);
        }
        let mut stats = Stats::load(&*b.remote).await.unwrap_or_default();
        if stats.world_born.is_none() {
            stats.world_born = Some(started_at);
        }
        if let Some(prev) = &previous_host {
            self.journal.post(msg::host_crash_detected(&prev.player, (Utc::now() - prev.heartbeat_at).num_minutes())).await;
            self.journal.post(msg::took_over(&cfg.player_name, Some(&prev.player))).await;
            self.diary_post("host_changed", Some(&cfg.player_name), format!("{} caiu; {} assumiu o mundo", prev.player, cfg.player_name));
        }
        self.journal.post(msg::world_opened(&cfg.player_name, &cfg.game_address(), started_at)).await;
        self.diary_post("opened", Some(&cfg.player_name), format!("{} abriu o mundo", cfg.player_name));
        for label in stats.due_anniversaries(Utc::now()) {
            self.journal.post(msg::anniversary(label, stats.world_born.unwrap())).await;
        }
        for (day, d) in stats.due_daily(Utc::now()) {
            self.journal.post(msg::daily(day, &d)).await;
        }
        let _ = stats.save(&*b.remote).await;

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
            s.session_started_at = Some(started_at);
            s.next_checkpoint_at = Some(Utc::now() + chrono::Duration::minutes(cfg.checkpoint_minutes as i64));
            s.online = vec![];
            s.sync = None;
            s.failover = None;
        });
        match trigger {
            PlayTrigger::Failover => self.notice(NoticeLevel::Success, "Você assumiu o mundo", "Entre de novo pelo Minecraft: o mundo continua de onde parou."),
            PlayTrigger::Wake => self.notice(NoticeLevel::Success, "Mundo acordado!", "Entre de novo pelo Minecraft."),
            PlayTrigger::Button => self.notice(NoticeLevel::Success, "Mundo aberto!", "Seus amigos já podem entrar."),
        }
        if open_game {
            self.open_game().await;
        }
        Ok(())
    }

    /// Whitelist a partir dos membros aprovados (quando o UUID de cada um é conhecido).
    async fn whitelist(&self) -> Vec<WhitelistEntry> {
        let nick = self.config.read().await.player_name.clone();
        let id = license::identity(&nick);
        let view = self.view.read().await.clone();
        let mut nicks: Vec<String> = view.map(|v| v.members.into_iter().filter(|m| m.approved).map(|m| m.nick).collect()).unwrap_or_default();
        if !nicks.contains(&nick) {
            nicks.push(nick);
        }
        let mut out = vec![];
        for n in nicks {
            match id.uuid_for(&n) {
                Some(uuid) => out.push(WhitelistEntry { uuid, name: n }),
                // sem UUID de alguém, a whitelist não pode ser aplicada sem barrar essa pessoa
                None => return vec![],
            }
        }
        out
    }

    /// Garante a entrada "pAuletOr 3" no Minecraft e abre o launcher.
    pub async fn open_game(&self) {
        let cfg = self.config.read().await.clone();
        if let Some(mc) = &cfg.minecraft_dir {
            if !launcher::client_running(None) {
                if let Err(e) = serversdat::ensure_entry(mc, &cfg.game_address(), &self.paths.backups()) {
                    tracing::warn!(error = %e, "não consegui criar a entrada no servers.dat");
                }
            }
        }
        if launcher::client_running(None) {
            return;
        }
        if let Some(l) = &cfg.launcher_path {
            if let Err(e) = launcher::open(l) {
                tracing::warn!(error = %e, "não consegui abrir o launcher");
            }
        }
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
                    let minutes = me.config.read().await.checkpoint_minutes.max(1);
                    tokio::time::sleep(Duration::from_secs(minutes * 60)).await;
                    if sess.ending.load(Ordering::SeqCst) {
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
        // fim de sessão, vez perdida, isolamento, servidor caído, virada do dia
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
                self.diary_post("joined", Some(&p), format!("{p} entrou"));
                (self.events)(Event::PlayerJoined { name: p.clone() });
                self.publish_online(s).await;
            }
            LogEvent::Left(p) => {
                let played = s.log.lock().await.left(&p, now);
                if let Some(secs) = played {
                    self.journal.post(msg::left(&p, secs)).await;
                    self.diary_post("left", Some(&p), format!("{p} saiu depois de {}", util::human_duration(secs)));
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
                self.diary_post("death", Some(&player), message.clone());
            }
            ref e @ LogEvent::Advancement { ref player, ref title, ref kind } => {
                s.stats.lock().await.on_advancement(player, title, now);
                let challenge = matches!(kind, crate::logparse::AdvancementKind::Challenge);
                self.journal.post(msg::advancement(player, title, challenge)).await;
                self.diary_post("advancement", Some(player), format!("{player} conseguiu [{title}]"));
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
        if let Err(e) = s.guard.update(Some(online), None).await {
            tracing::warn!(error = %e, "não consegui publicar quem está online");
        }
    }

    async fn monitor(self: Arc<Self>, s: Arc<HostSession>) {
        let mut empty_since: Option<tokio::time::Instant> = None;
        let mut warned_friends = false;
        let mut warned_isolated = false;
        let mut lost = s.guard.lost();
        let mut isolated = s.guard.isolated();
        let mut day = journal::day_sp(Utc::now());
        let started = tokio::time::Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                _ = lost.changed() => {}
                _ = isolated.changed() => {}
            }
            if s.ending.load(Ordering::SeqCst) {
                return;
            }
            if *lost.borrow() {
                self.on_lock_lost(&s, "Outro PC assumiu o mundo").await;
                return;
            }
            // Sem contato com a coordenação: se nenhum amigo está conectado pelo túnel, este PC
            // não pode provar que ainda é o host — salva e fecha (nunca dois hosts). Com amigos
            // conectados, eles avisam a coordenação que o host vive, e a sessão continua.
            if *isolated.borrow() {
                let remote_players = match self.node().await {
                    Some(n) => n.remote_players(),
                    None => 0,
                };
                if remote_players == 0 {
                    self.on_lock_lost(&s, "Sem internet: o mundo foi salvo neste PC e será enviado quando a conexão voltar").await;
                    return;
                }
                if !warned_isolated {
                    warned_isolated = true;
                    self.notice(NoticeLevel::Warning, "Sem contato com a nuvem do mundo", "Seus amigos conectados continuam jogando. O app tenta reconectar sozinho.");
                }
            } else {
                warned_isolated = false;
            }
            if !s.server.running().await {
                tracing::error!("o servidor fechou sozinho");
                self.journal
                    .post(msg::upload_failed(&s.log.lock().await.host, "O servidor fechou sozinho; o mundo foi salvo como estava."))
                    .await;
                let me = self.clone();
                tokio::spawn(async move {
                    let _ = me.end_session(EndReason::UserStop).await;
                });
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
            let client = launcher::client_running(Some(s.server.pid)) || self.status().playing;
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
                self.notice(NoticeLevel::Warning, "Seus amigos ainda estão jogando", "Não desligue o PC: o mundo fecha sozinho quando o último sair. Ou use Passar a vez.");
            }
            // Ninguém jogando: começa a contar a tolerância.
            let nobody = players.is_empty();
            let should_count = nobody && (seen || started.elapsed() > Duration::from_secs(10 * 60));
            if should_count {
                let since = *empty_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() >= grace {
                    tracing::info!("todos saíram; encerrando a sessão");
                    let me = self.clone();
                    tokio::spawn(async move {
                        let _ = me.end_session(EndReason::Normal).await;
                    });
                    return;
                }
            } else {
                empty_since = None;
            }
        }
    }

    /// Salvamento periódico (com o servidor rodando): envia para a nuvem e publica o HEAD.
    /// Os amigos online copiam por P2P em seguida (réplica em segundo plano).
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
        let _ = s.stats.lock().await.save(&*se.remote).await;
        self.journal
            .post(msg::checkpoint(&util::human_bytes(m.total_size), &util::human_bytes(snap.mirror.copied_bytes)))
            .await;
        self.set_status(|st| {
            st.last_checkpoint_at = Some(Utc::now());
            st.sync = None;
            st.replica_seq = LocalState::load(&self.paths).replica.map(|r| r.seq).unwrap_or(0);
        });
        tracing::info!(manifest = %m.id, pause_ms = snap.pause_ms, "checkpoint publicado");
        Ok(())
    }

    /// "Passar a vez": avisa no chat com contagem regressiva, fecha, envia e libera.
    pub async fn pass_turn(self: &Arc<Self>) -> NmResult<()> {
        let s = self.session.lock().await.clone().ok_or_else(|| NmError::Other(anyhow::anyhow!("você não está hospedando")))?;
        let host = self.config.read().await.player_name.clone();
        let _ = s.guard.update(None, Some(true)).await;
        self.journal.post(msg::handing_over(&host)).await;
        self.diary_post("handover", Some(&host), format!("{host} passou a vez"));
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
        if let Some(n) = self.node().await {
            n.set_hosting(None);
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
                self.diary_post("saved", Some(&host), format!("Mundo salvo depois de {}", util::human_duration((now - s.started_at).num_seconds())));
                self.maybe_gc(&s).await;
                if let Err(e) = s.guard.release().await {
                    tracing::error!(error = %e, "não consegui liberar a vez; ela expira sozinha em 1 min");
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
                self.set_route(Route::idle()).await;
                self.notice(NoticeLevel::Success, "Mundo salvo", "Pode desligar o PC.");
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
        guard: &LeaseGuard,
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

    /// A vez não é mais nossa (ou não dá para provar que é): salva e fecha sem publicar.
    /// O que este PC tinha vira um manifesto solto na linha do tempo (nada é perdido nem
    /// sobrescreve o mundo de quem assumiu). Sem internet, fica sujo e a recuperação envia depois.
    async fn on_lock_lost(self: &Arc<Self>, s: &Arc<HostSession>, why: &str) {
        s.ending.store(true, Ordering::SeqCst);
        tracing::error!(%why, "encerrando a sessão sem publicar");
        self.notice(NoticeLevel::Error, why, "Seu progresso fica guardado na linha do tempo, sem sobrescrever ninguém.");
        let _ = s.server.say("Este PC deixou de ser o host. Salvando e fechando; entre de novo em alguns segundos.").await;
        s.server.stop(Duration::from_secs(180)).await;
        if let Some(n) = self.node().await {
            n.set_hosting(None);
        }
        s.guard.abandon().await;
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
                note: Some(format!("Sessão interrompida: {why}")),
            };
            se.push(&self.paths.staging(), meta, None, None, false, self.progress()).await
        }
        .await;
        if let Err(e) = r {
            tracing::error!(error = %e, "não consegui guardar a sessão interrompida; a recuperação envia depois");
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
            st.recovery_pending = LocalState::load(&self.paths).dirty;
        });
        self.set_route(Route::idle()).await;
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

    /// Simula a queda do host (testes): para o heartbeat e mata o servidor, sem salvar nem
    /// liberar a vez — exatamente o que um PC que desliga de repente deixa para trás.
    #[doc(hidden)]
    pub async fn simulate_crash(&self) {
        let Some(s) = self.session.lock().await.take() else { return };
        s.ending.store(true, Ordering::SeqCst);
        s.guard.abandon().await;
        for t in s.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
        s.server.kill().await;
        if let Some(n) = self.node().await {
            n.set_hosting(None);
        }
        self.stop_network().await;
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

    /// "Assumir agora": espera a vez ficar livre (o host está salvando) e então hospeda aqui.
    pub async fn take_turn(self: &Arc<Self>, open_game: bool) -> NmResult<()> {
        self.phase(Phase::WaitingTurn);
        let lm = self.lease_manager().await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
        let prev_host;
        loop {
            match lm.status().await? {
                LeaseStatus::Free { .. } => {
                    prev_host = self.status().lease.and_then(|l| l.held().map(|l| l.player.clone()));
                    break;
                }
                LeaseStatus::Held { .. } if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                LeaseStatus::Held { lease } => {
                    self.phase(Phase::Idle);
                    return Err(NmError::HeldBy(Box::new(lease)));
                }
            }
        }
        self.phase(Phase::Idle);
        let me = self.config.read().await.player_name.clone();
        self.play_with(PlayTrigger::Button, open_game).await?;
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
        let r = self.recover_inner(st).await;
        if r.is_err() {
            self.phase(Phase::Idle);
        }
        r
    }

    async fn recover_inner(self: &Arc<Self>, st: LocalState) -> NmResult<()> {
        self.phase(Phase::Recovering);
        let active = st.active_session.clone();
        let cfg = self.config.read().await.clone();
        let se = self.sync_engine().await?;

        // 1) servidor órfão (o app caiu com ele rodando): para com segurança via RCON
        if let Some(pid) = server::orphan_server_pid(&self.paths) {
            tracing::warn!(pid, "servidor órfão encontrado; parando com segurança");
            if let Some(pass) = active.as_ref().and_then(|a| a.rcon_password.clone()) {
                if let Ok(mut r) = crate::rcon::Rcon::connect(&format!("127.0.0.1:{}", crate::RCON_PORT), &pass).await {
                    let _ = r.cmd("stop").await;
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

        // 2) a vez em modo manutenção (se a minha vez antiga ainda existe, a coordenação deixa
        //    este PC retomá-la; se outro PC assumiu, não publicamos)
        let lm = self.lease_manager().await?;
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let req = LeaseManager::new_request(&session_id, &cfg.player_name, LeasePurpose::Maintenance, None, se.replica_seq());
        let guard = lm.acquire(req).await.ok();

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
        F: FnOnce(LeaseGuard, SyncEngine) -> Fut,
        Fut: std::future::Future<Output = (LeaseGuard, NmResult<T>)>,
    {
        let _op = self.op.try_lock().map_err(|_| NmError::Other(anyhow::anyhow!("já tem uma operação em andamento")))?;
        if self.is_hosting().await {
            return Err(NmError::Other(anyhow::anyhow!("feche a sessão antes")));
        }
        self.phase(Phase::Maintenance);
        let cfg = self.config.read().await.clone();
        let lm = self.lease_manager().await?;
        let se = self.sync_engine().await?;
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let req = LeaseManager::new_request(&session_id, &cfg.player_name, LeasePurpose::Maintenance, None, se.replica_seq());
        let guard = match lm.acquire(req).await {
            Ok(g) => g,
            Err(e) => {
                self.phase(Phase::Idle);
                return Err(e);
            }
        };
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
                me.diary_post("restored", Some(&player), format!("{player} restaurou {label}"));
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

    /// Importa uma pasta de mundo existente como nova versão (o dono, ao criar o mundo).
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
        let _op = self.op.lock().await;
        let se = self.sync_engine().await?;
        if !LocalState::load(&self.paths).dirty {
            se.pull(self.progress()).await?;
        }
        let mut nicks: Vec<String> = vec![self.config.read().await.player_name.clone()];
        if let Some(v) = self.view.read().await.as_ref() {
            nicks.extend(v.members.iter().map(|m| m.nick.clone()));
        }
        if let Ok(st) = Stats::load(&*se.remote).await {
            nicks.extend(st.players.keys().cloned());
        }
        nicks.sort();
        nicks.dedup();
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
        Stats::load(&*self.backend().await?.remote).await
    }

    /// Fotografia mais recente da coordenação.
    pub async fn view(&self) -> Option<WorldView> {
        self.view.read().await.clone()
    }

    /// Vez atual (para a CLI e testes).
    pub async fn lease(&self) -> NmResult<Option<LeaseInfo>> {
        Ok(self.lease_manager().await?.status().await?.held().cloned())
    }
}

fn waiting(motd: &str, message: &str, wake: bool) -> Route {
    Route::Waiting { motd: motd.into(), message: message.into(), wake }
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
