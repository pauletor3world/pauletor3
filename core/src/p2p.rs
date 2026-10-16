//! Rede entre os PCs do grupo, embutida no app (iroh: QUIC P2P com furo de NAT e relays
//! públicos como reserva). Ninguém instala VPN nem abre porta no roteador.
//!
//! Dois protocolos (ALPN), ambos aceitos **só de membros do mundo**:
//! - [`ALPN_MC`]: túnel do Minecraft. Cada conexão TCP do jogo vira um stream bidirecional
//!   até o host, que encaminha para o servidor em `127.0.0.1`.
//! - [`ALPN_SYNC`]: réplica. Serve manifestos e objetos do cache local (pedidos por hash, e
//!   quem recebe confere o hash), e responde `ping` (para medir e para avisar que o host vive).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, PublicKey, RelayMode, SecretKey};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{watch, Mutex};

use crate::paths::Paths;
use crate::remote::{ProgressFn, TransferProgress};
use crate::sync::ObjectSource;

pub const ALPN_MC: &[u8] = b"pauletor3/mc/1";
pub const ALPN_SYNC: &[u8] = b"pauletor3/sync/1";

/// Versão do cabeçalho do túnel do jogo.
const MC_TUNNEL_V1: u8 = 1;
const MAX_REQUEST: u32 = 4096;
/// Pedidos de objetos em paralelo para um mesmo amigo.
const PARALLEL: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetMode {
    /// Relays e descoberta públicos do iroh (n0).
    Public,
    /// Só endereços diretos, sem relay nem descoberta (testes na mesma máquina).
    LocalOnly,
}

pub fn id_hex(id: &EndpointId) -> String {
    hex::encode(id.as_bytes())
}

pub fn parse_id(hex_id: &str) -> anyhow::Result<EndpointId> {
    let b = hex::decode(hex_id).context("id inválido")?;
    let arr: [u8; 32] = b.try_into().map_err(|_| anyhow!("id com tamanho errado"))?;
    PublicKey::from_bytes(&arr).map_err(|e| anyhow!("id inválido: {e}"))
}

pub fn parse_addr(json: &str) -> anyhow::Result<EndpointAddr> {
    Ok(serde_json::from_str(json)?)
}

/// Chave secreta do app (identidade). Guardada nos segredos (DPAPI).
pub fn new_secret() -> [u8; 32] {
    SecretKey::generate().to_bytes()
}

pub fn public_hex(secret: &[u8; 32]) -> String {
    id_hex(&SecretKey::from_bytes(secret).public())
}

/// Assina uma mensagem com a chave do app (autenticação na coordenação).
pub fn sign(secret: &[u8; 32], msg: &[u8]) -> String {
    hex::encode(SecretKey::from_bytes(secret).sign(msg).to_bytes())
}

struct Shared {
    members: RwLock<HashSet<EndpointId>>,
    /// Porta do servidor local quando este PC hospeda.
    game_port: watch::Sender<Option<u16>>,
    cache_objects: PathBuf,
    cache_manifests: PathBuf,
    /// Conexões de jogo de amigos passando pelo túnel até este PC agora.
    active_tunnels: std::sync::atomic::AtomicUsize,
}

impl Shared {
    fn allowed(&self, id: &EndpointId) -> bool {
        self.members.read().unwrap().contains(id)
    }
}

/// O nó P2P deste PC.
pub struct Node {
    pub endpoint: Endpoint,
    router: Router,
    shared: Arc<Shared>,
    conns: Mutex<HashMap<(EndpointId, &'static [u8]), Connection>>,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("id", &self.endpoint.id().fmt_short().to_string()).finish()
    }
}

impl Node {
    pub async fn start(secret: &[u8; 32], mode: NetMode, paths: &Paths) -> anyhow::Result<Arc<Self>> {
        let sk = SecretKey::from_bytes(secret);
        let builder = match mode {
            NetMode::Public => Endpoint::builder(presets::N0),
            NetMode::LocalOnly => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .clear_ip_transports()
                .bind_addr("127.0.0.1:0")
                .map_err(|e| anyhow!("{e}"))?,
        };
        let endpoint = builder
            .secret_key(sk)
            .alpns(vec![ALPN_MC.to_vec(), ALPN_SYNC.to_vec()])
            .bind()
            .await
            .map_err(|e| anyhow!("não consegui abrir a rede P2P: {e}"))?;
        let (game_port, _) = watch::channel(None);
        let shared = Arc::new(Shared {
            members: Default::default(),
            game_port,
            cache_objects: paths.cache_objects(),
            cache_manifests: paths.cache_manifests(),
            active_tunnels: Default::default(),
        });
        let router = Router::builder(endpoint.clone())
            .accept(ALPN_MC, McHandler(shared.clone()))
            .accept(ALPN_SYNC, SyncHandler(shared.clone()))
            .spawn();
        Ok(Arc::new(Self { endpoint, router, shared, conns: Mutex::new(HashMap::new()) }))
    }

    pub fn id_hex(&self) -> String {
        id_hex(&self.endpoint.id())
    }

    /// Endereço atual (JSON), publicado na presença e no lease.
    pub fn addr_json(&self) -> String {
        serde_json::to_string(&self.endpoint.addr()).unwrap_or_default()
    }

    /// Espera estar alcançável (relay conectado) por até `timeout`.
    pub async fn wait_online(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, self.endpoint.online()).await.is_ok()
    }

    /// Quem pode se conectar (ids hex dos membros aprovados).
    pub fn set_members<'a>(&self, ids: impl IntoIterator<Item = &'a str>) {
        let set: HashSet<EndpointId> = ids.into_iter().filter_map(|h| parse_id(h).ok()).collect();
        *self.shared.members.write().unwrap() = set;
    }

    /// Amigos jogando agora através do túnel até este PC (host).
    pub fn remote_players(&self) -> usize {
        self.shared.active_tunnels.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Liga/desliga o encaminhamento do túnel para o servidor local.
    pub fn set_hosting(&self, port: Option<u16>) {
        self.shared.game_port.send_replace(port);
    }

    async fn connection(&self, addr: &EndpointAddr, alpn: &'static [u8]) -> anyhow::Result<Connection> {
        let key = (addr.id, alpn);
        {
            let conns = self.conns.lock().await;
            if let Some(c) = conns.get(&key) {
                if c.close_reason().is_none() {
                    return Ok(c.clone());
                }
            }
        }
        let c = tokio::time::timeout(Duration::from_secs(20), self.endpoint.connect(addr.clone(), alpn))
            .await
            .map_err(|_| anyhow!("tempo esgotado conectando com {}", addr.id.fmt_short()))?
            .map_err(|e| anyhow!("não conectou com {}: {e}", addr.id.fmt_short()))?;
        self.conns.lock().await.insert(key, c.clone());
        Ok(c)
    }

    /// Abre um túnel de jogo até o host (um stream por conexão TCP do Minecraft).
    pub async fn open_game_stream(&self, host: &EndpointAddr) -> anyhow::Result<(SendStream, RecvStream)> {
        let conn = self.connection(host, ALPN_MC).await?;
        let (mut send, recv) = conn.open_bi().await.map_err(|e| anyhow!("{e}"))?;
        send.write_all(&[MC_TUNNEL_V1]).await?;
        Ok((send, recv))
    }

    /// Ida e volta até um amigo (também confirma que ele está vivo).
    pub async fn ping(&self, peer: &EndpointAddr) -> anyhow::Result<Duration> {
        let t = std::time::Instant::now();
        let conn = self.connection(peer, ALPN_SYNC).await?;
        let (status, _) = request(&conn, &SyncRequest::Ping).await?;
        if status != STATUS_OK {
            bail!("ping recusado");
        }
        Ok(t.elapsed())
    }

    /// "direta" ou "relay" para a conexão de jogo/sync com um amigo, se houver.
    pub async fn path_kind(&self, peer: &EndpointId) -> Option<&'static str> {
        let conns = self.conns.lock().await;
        let c = conns
            .iter()
            .find(|((id, _), c)| id == peer && c.close_reason().is_none())
            .map(|(_, c)| c.clone())?;
        let paths = c.paths();
        let selected = paths.iter().find(|p| p.is_selected())?;
        Some(if selected.is_relay() { "relay" } else { "direta" })
    }

    pub async fn shutdown(&self) {
        let _ = self.router.shutdown().await;
    }
}

// ------------------------------------------------------------------ túnel do jogo (host)

#[derive(Clone)]
struct McHandler(Arc<Shared>);

impl std::fmt::Debug for McHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("McHandler")
    }
}

impl ProtocolHandler for McHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let peer = conn.remote_id();
        if !self.0.allowed(&peer) {
            tracing::warn!(peer = %peer.fmt_short(), "conexão de jogo recusada: não é membro");
            conn.close(1u32.into(), b"not a member");
            return Ok(());
        }
        loop {
            let (send, mut recv) = match conn.accept_bi().await {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            let shared = self.0.clone();
            tokio::spawn(async move {
                let mut hdr = [0u8; 1];
                if recv.read_exact(&mut hdr).await.is_err() || hdr[0] != MC_TUNNEL_V1 {
                    return;
                }
                let Some(port) = *shared.game_port.borrow() else {
                    tracing::info!("pedido de jogo, mas este PC não está hospedando");
                    return;
                };
                match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                    Ok(mut tcp) => {
                        let _ = tcp.set_nodelay(true);
                        shared.active_tunnels.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let mut quic = tokio::io::join(recv, send);
                        let _ = tokio::io::copy_bidirectional(&mut tcp, &mut quic).await;
                        shared.active_tunnels.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    Err(e) => tracing::warn!(error = %e, "servidor local não aceitou a conexão"),
                }
            });
        }
    }
}

// ------------------------------------------------------------------ réplica (servir)

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum SyncRequest {
    Ping,
    Manifest { id: String },
    Object { hash: String },
}

const STATUS_OK: u8 = 0;
const STATUS_MISSING: u8 = 1;

#[derive(Clone)]
struct SyncHandler(Arc<Shared>);

impl std::fmt::Debug for SyncHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyncHandler")
    }
}

impl ProtocolHandler for SyncHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let peer = conn.remote_id();
        if !self.0.allowed(&peer) {
            tracing::warn!(peer = %peer.fmt_short(), "réplica recusada: não é membro");
            conn.close(1u32.into(), b"not a member");
            return Ok(());
        }
        loop {
            let (send, recv) = match conn.accept_bi().await {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };
            let shared = self.0.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_one(&shared, send, recv).await {
                    tracing::debug!(error = %e, "pedido de réplica falhou");
                }
            });
        }
    }
}

async fn serve_one(shared: &Shared, mut send: SendStream, mut recv: RecvStream) -> anyhow::Result<()> {
    let len = recv.read_u32().await?;
    if len > MAX_REQUEST {
        bail!("pedido grande demais");
    }
    let mut buf = vec![0u8; len as usize];
    recv.read_exact(&mut buf).await?;
    let req: SyncRequest = serde_json::from_slice(&buf)?;
    let file = match &req {
        SyncRequest::Ping => {
            send.write_u8(STATUS_OK).await?;
            send.write_u64(0).await?;
            send.finish()?;
            return Ok(());
        }
        SyncRequest::Manifest { id } if crate::manifest::is_manifest_id(id) => shared.cache_manifests.join(format!("{id}.json")),
        SyncRequest::Object { hash } if crate::hashing::is_object_hash(hash) => shared.cache_objects.join(hash),
        _ => bail!("pedido inválido"),
    };
    match tokio::fs::File::open(&file).await {
        Ok(mut f) => {
            let size = f.metadata().await?.len();
            send.write_u8(STATUS_OK).await?;
            send.write_u64(size).await?;
            tokio::io::copy(&mut f, &mut send).await?;
        }
        Err(_) => {
            send.write_u8(STATUS_MISSING).await?;
            send.write_u64(0).await?;
        }
    }
    send.finish()?;
    Ok(())
}

async fn request(conn: &Connection, req: &SyncRequest) -> anyhow::Result<(u8, RecvStream)> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| anyhow!("{e}"))?;
    let body = serde_json::to_vec(req)?;
    send.write_u32(body.len() as u32).await?;
    send.write_all(&body).await?;
    send.finish()?;
    let status = recv.read_u8().await?;
    Ok((status, recv))
}

// ------------------------------------------------------------------ réplica (buscar)

/// Busca manifestos e objetos nos amigos online (o host primeiro).
pub struct PeerObjects {
    pub node: Arc<Node>,
    pub peers: Vec<EndpointAddr>,
}

impl PeerObjects {
    async fn fetch_to(&self, conn: &Connection, req: &SyncRequest, dest: &Path) -> anyhow::Result<u64> {
        let (status, mut recv) = request(conn, req).await?;
        if status != STATUS_OK {
            bail!("não tem");
        }
        let size = recv.read_u64().await?;
        let tmp = dest.with_extension(format!("p2p-{}", uuid::Uuid::new_v4().simple()));
        let mut f = tokio::fs::File::create(&tmp).await?;
        let n = tokio::io::copy(&mut (&mut recv).take(size), &mut f).await?;
        f.sync_all().await?;
        drop(f);
        if n != size {
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("transferência incompleta");
        }
        tokio::fs::rename(&tmp, dest).await?;
        Ok(n)
    }
}

#[async_trait::async_trait]
impl ObjectSource for PeerObjects {
    async fn fetch_manifest(&self, id: &str) -> Option<Vec<u8>> {
        for p in &self.peers {
            let Ok(conn) = self.node.connection(p, ALPN_SYNC).await else { continue };
            let Ok((STATUS_OK, mut recv)) = request(&conn, &SyncRequest::Manifest { id: id.to_string() }).await else { continue };
            let Ok(size) = recv.read_u64().await else { continue };
            if size > 64 << 20 {
                continue;
            }
            let mut buf = vec![0u8; size as usize];
            if recv.read_exact(&mut buf).await.is_ok() {
                return Some(buf);
            }
        }
        None
    }

    async fn fetch_objects(&self, hashes: &[String], dir: &Path, progress: ProgressFn) -> Vec<String> {
        use futures_util::stream::{self, StreamExt};
        let mut remaining: Vec<String> = hashes.to_vec();
        let mut got: Vec<String> = vec![];
        let bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let files = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let total_files = hashes.len() as u64;
        for p in &self.peers {
            if remaining.is_empty() {
                break;
            }
            let conn = match self.node.connection(p, ALPN_SYNC).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::info!(peer = %p.id.fmt_short(), error = %e, "amigo indisponível para a réplica");
                    continue;
                }
            };
            let results: Vec<(String, bool)> = stream::iter(remaining.clone())
                .map(|h| {
                    let conn = conn.clone();
                    let dest = dir.join(&h);
                    let bytes = bytes.clone();
                    let files = files.clone();
                    let progress = progress.clone();
                    async move {
                        let ok = match self.fetch_to(&conn, &SyncRequest::Object { hash: h.clone() }, &dest).await {
                            Ok(n) => {
                                let b = bytes.fetch_add(n, std::sync::atomic::Ordering::SeqCst) + n;
                                let f = files.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                                progress(TransferProgress { bytes_done: b, bytes_total: 0, files_done: f, files_total: total_files });
                                true
                            }
                            Err(_) => false,
                        };
                        (h, ok)
                    }
                })
                .buffer_unordered(PARALLEL)
                .collect()
                .await;
            remaining.clear();
            for (h, ok) in results {
                if ok {
                    got.push(h);
                } else {
                    remaining.push(h);
                }
            }
        }
        got
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn node(dir: &Path) -> (Arc<Node>, Paths) {
        let paths = Paths::new(dir);
        paths.ensure().unwrap();
        let n = Node::start(&new_secret(), NetMode::LocalOnly, &paths).await.unwrap();
        (n, paths)
    }

    #[tokio::test]
    async fn objects_and_game_tunnel_between_members() {
        let da = tempfile::tempdir().unwrap();
        let db = tempfile::tempdir().unwrap();
        let (a, pa) = node(da.path()).await;
        let (b, _pb) = node(db.path()).await;
        a.set_members([b.id_hex().as_str()]);
        b.set_members([a.id_hex().as_str()]);

        // objeto no cache de A
        let data = b"conteudo do mundo".repeat(10_000);
        let hash = crate::hashing::sha256_bytes(&data);
        std::fs::write(pa.cache_objects().join(&hash), &data).unwrap();

        let src = PeerObjects { node: b.clone(), peers: vec![a.endpoint.addr()] };
        let out = tempfile::tempdir().unwrap();
        let missing = "0".repeat(64);
        let got = src.fetch_objects(&[hash.clone(), missing], out.path(), crate::remote::no_progress()).await;
        assert_eq!(got, vec![hash.clone()]);
        assert_eq!(std::fs::read(out.path().join(&hash)).unwrap(), data);
        assert!(b.ping(&a.endpoint.addr()).await.is_ok());

        // túnel: A "hospeda" um servidor de eco
        let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = echo.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = echo.accept().await.unwrap();
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        a.set_hosting(Some(port));
        let (mut send, mut recv) = b.open_game_stream(&a.endpoint.addr()).await.unwrap();
        send.write_all(b"ola minecraft").await.unwrap();
        let mut buf = [0u8; 13];
        recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ola minecraft");
        assert!(b.path_kind(&a.endpoint.id()).await.is_some());
    }

    #[tokio::test]
    async fn strangers_are_refused() {
        let da = tempfile::tempdir().unwrap();
        let dc = tempfile::tempdir().unwrap();
        let (a, pa) = node(da.path()).await;
        let (c, _) = node(dc.path()).await;
        let data = b"segredo".to_vec();
        let hash = crate::hashing::sha256_bytes(&data);
        std::fs::write(pa.cache_objects().join(&hash), &data).unwrap();
        // c não é membro de a
        let src = PeerObjects { node: c.clone(), peers: vec![a.endpoint.addr()] };
        let out = tempfile::tempdir().unwrap();
        let got = src.fetch_objects(&[hash], out.path(), crate::remote::no_progress()).await;
        assert!(got.is_empty());
        assert!(c.ping(&a.endpoint.addr()).await.is_err());
    }
}
