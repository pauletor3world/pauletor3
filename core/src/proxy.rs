//! Proxy local: a "mágica" do endereço fixo.
//!
//! O app de cada jogador escuta em `127.0.0.1:<porta>` e o Minecraft sempre entra em
//! `localhost`. O proxy lê o handshake do protocolo do jogo e decide:
//! - **alguém hospeda neste PC** → encaminha para o servidor local;
//! - **um amigo hospeda** → encaminha pelo túnel P2P (iroh) até o PC dele;
//! - **ninguém hospeda** → responde a lista de servidores com um MOTD explicando o estado e,
//!   se for uma tentativa de entrar, acorda o mundo neste PC e pede para entrar de novo.
//!
//! Como o endereço nunca muda, o host pode trocar sem o jogador mexer em nada, e nenhuma
//! porta é aberta no roteador nem na rede local (o proxy só escuta em 127.0.0.1).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use iroh::EndpointAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::p2p::Node;

/// Para onde o tráfego do jogo vai agora.
#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    /// Este PC hospeda: servidor em 127.0.0.1:porta.
    Local(u16),
    /// Um amigo hospeda.
    Remote(EndpointAddr),
    /// Ninguém (ou trocando de host). `motd` aparece na lista de servidores; `message` é o
    /// aviso ao tentar entrar; `wake` = uma tentativa de entrar acorda o mundo neste PC.
    Waiting { motd: String, message: String, wake: bool },
}

impl Route {
    pub fn idle() -> Self {
        Route::Waiting {
            motd: "§aMundo livre §7— entre para acordar o mundo".into(),
            message: "Acordando o mundo no seu PC...\n\nEntre de novo em alguns segundos.".into(),
            wake: true,
        }
    }
}

pub type WakeFn = Arc<dyn Fn() + Send + Sync>;
/// Chamado quando o túnel até o host falha (o host pode ter caído).
pub type HostUnreachableFn = Arc<dyn Fn() + Send + Sync>;

pub struct ProxyHandle {
    pub addr: SocketAddr,
    route: watch::Sender<Route>,
    /// Conexões de jogo (login) abertas agora, através deste proxy.
    playing: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl ProxyHandle {
    pub fn set_route(&self, r: Route) {
        self.route.send_if_modified(|cur| {
            if *cur != r {
                *cur = r;
                true
            } else {
                false
            }
        });
    }

    pub fn route(&self) -> Route {
        self.route.borrow().clone()
    }

    /// O Minecraft deste PC está dentro do mundo agora.
    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst) > 0
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Liga o proxy em `127.0.0.1:port` (0 = porta livre qualquer, para testes).
pub async fn start(port: u16, node: Option<Arc<Node>>, on_wake: WakeFn, on_unreachable: HostUnreachableFn) -> anyhow::Result<ProxyHandle> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("a porta {port} já está em uso neste PC (outro servidor de Minecraft aberto?)"))?;
    let addr = listener.local_addr()?;
    let (route, rx) = watch::channel(Route::idle());
    let playing = Arc::new(AtomicUsize::new(0));
    let pl = playing.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else { continue };
            let _ = sock.set_nodelay(true);
            let r = rx.borrow().clone();
            let node = node.clone();
            let wake = on_wake.clone();
            let unreachable = on_unreachable.clone();
            let pl = pl.clone();
            tokio::spawn(async move {
                if let Err(e) = handle(sock, r, node, wake, unreachable, pl).await {
                    tracing::debug!(error = %e, "conexão do proxy terminou com erro");
                }
            });
        }
    });
    Ok(ProxyHandle { addr, route, playing, task })
}

struct PlayingGuard(Arc<AtomicUsize>);
impl Drop for PlayingGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn handle(
    mut sock: TcpStream,
    route: Route,
    node: Option<Arc<Node>>,
    wake: WakeFn,
    unreachable: HostUnreachableFn,
    playing: Arc<AtomicUsize>,
) -> anyhow::Result<()> {
    let (raw, hs) = tokio::time::timeout(Duration::from_secs(10), read_handshake(&mut sock))
        .await
        .context("handshake demorou")??;
    let _guard = if hs.next_state != 1 {
        playing.fetch_add(1, Ordering::SeqCst);
        Some(PlayingGuard(playing.clone()))
    } else {
        None
    };
    match route {
        Route::Local(port) => {
            let mut up = TcpStream::connect(("127.0.0.1", port)).await.context("servidor local não respondeu")?;
            let _ = up.set_nodelay(true);
            up.write_all(&raw).await?;
            let _ = tokio::io::copy_bidirectional(&mut sock, &mut up).await;
        }
        Route::Remote(addr) => {
            let Some(node) = node else { bail!("sem rede P2P") };
            match node.open_game_stream(&addr).await {
                Ok((mut send, recv)) => {
                    send.write_all(&raw).await?;
                    let mut quic = tokio::io::join(recv, send);
                    let _ = tokio::io::copy_bidirectional(&mut sock, &mut quic).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "não consegui abrir o túnel até o host");
                    unreachable();
                    let motd = "§eNão consegui falar com o host §7— tentando de novo".to_string();
                    reply_waiting(&mut sock, &hs, &motd, "Não consegui falar com o PC de quem está hospedando.\n\nO app está tentando de novo; se ele caiu, outro amigo assume.").await?;
                }
            }
        }
        Route::Waiting { motd, message, wake: do_wake } => {
            if hs.next_state != 1 && do_wake {
                wake();
            }
            reply_waiting(&mut sock, &hs, &motd, &message).await?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ protocolo do jogo

#[derive(Debug, Clone, PartialEq)]
pub struct Handshake {
    pub protocol: i32,
    pub address: String,
    pub port: u16,
    /// 1 = lista de servidores (status), 2 = entrar (login), 3 = transferência.
    pub next_state: i32,
}

async fn read_varint<R: AsyncReadExt + Unpin>(r: &mut R, raw: &mut Vec<u8>) -> anyhow::Result<i32> {
    let mut val: i32 = 0;
    for i in 0..5 {
        let b = r.read_u8().await?;
        raw.push(b);
        val |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(val);
        }
    }
    bail!("VarInt longo demais")
}

fn parse_varint(buf: &[u8], pos: &mut usize) -> anyhow::Result<i32> {
    let mut val: i32 = 0;
    for i in 0..5 {
        let b = *buf.get(*pos).context("pacote curto")?;
        *pos += 1;
        val |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(val);
        }
    }
    bail!("VarInt longo demais")
}

fn write_varint(out: &mut Vec<u8>, mut v: i32) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v = ((v as u32) >> 7) as i32;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

fn packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = Vec::new();
    write_varint(&mut inner, id);
    inner.extend_from_slice(body);
    let mut out = Vec::new();
    write_varint(&mut out, inner.len() as i32);
    out.extend(inner);
    out
}

/// Lê o primeiro pacote (handshake). Devolve os bytes crus (para repassar ao servidor) e o conteúdo.
pub async fn read_handshake<R: AsyncReadExt + Unpin>(r: &mut R) -> anyhow::Result<(Vec<u8>, Handshake)> {
    let mut raw = Vec::new();
    let len = read_varint(r, &mut raw).await?;
    if !(1..=1024).contains(&len) {
        bail!("handshake com tamanho estranho ({len}); cliente antigo?");
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    raw.extend_from_slice(&body);
    let mut pos = 0;
    let id = parse_varint(&body, &mut pos)?;
    if id != 0 {
        bail!("primeiro pacote não é handshake");
    }
    let protocol = parse_varint(&body, &mut pos)?;
    let slen = parse_varint(&body, &mut pos)? as usize;
    let address = String::from_utf8_lossy(body.get(pos..pos + slen).context("endereço curto")?).into_owned();
    pos += slen;
    let port = u16::from_be_bytes([*body.get(pos).context("curto")?, *body.get(pos + 1).context("curto")?]);
    pos += 2;
    let next_state = parse_varint(&body, &mut pos)?;
    Ok((raw, Handshake { protocol, address, port, next_state }))
}

fn status_json(protocol: i32, motd: &str) -> String {
    serde_json::json!({
        "version": { "name": "pAuletOr 3", "protocol": protocol },
        "players": { "max": 0, "online": 0 },
        "description": { "text": motd },
    })
    .to_string()
}

async fn reply_waiting(sock: &mut TcpStream, hs: &Handshake, motd: &str, message: &str) -> anyhow::Result<()> {
    match hs.next_state {
        1 => {
            // status request (0x00) → resposta; depois ping (0x01) → pong
            let mut tmp = Vec::new();
            let len = read_varint(sock, &mut tmp).await?;
            let mut body = vec![0u8; len.clamp(0, 64) as usize];
            sock.read_exact(&mut body).await?;
            let mut out = Vec::new();
            write_string(&mut out, &status_json(hs.protocol, motd));
            sock.write_all(&packet(0x00, &out)).await?;
            let mut tmp = Vec::new();
            if let Ok(Ok(len)) = tokio::time::timeout(Duration::from_secs(5), read_varint(sock, &mut tmp)).await {
                let mut body = vec![0u8; len.clamp(0, 64) as usize];
                sock.read_exact(&mut body).await?;
                // body = id 0x01 + i64; devolve igual
                sock.write_all(&packet(0x01, body.get(1..).unwrap_or_default())).await?;
            }
        }
        _ => {
            // login: "Disconnect (login)" (0x00) com componente de texto em JSON
            let mut out = Vec::new();
            write_string(&mut out, &serde_json::json!({ "text": message }).to_string());
            sock.write_all(&packet(0x00, &out)).await?;
        }
    }
    sock.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake_bytes(next: i32) -> Vec<u8> {
        let mut b = Vec::new();
        write_varint(&mut b, 774);
        write_string(&mut b, "localhost");
        b.extend_from_slice(&25565u16.to_be_bytes());
        write_varint(&mut b, next);
        packet(0x00, &b)
    }

    async fn read_packet(s: &mut TcpStream) -> (i32, Vec<u8>) {
        let mut raw = Vec::new();
        let len = read_varint(s, &mut raw).await.unwrap();
        let mut body = vec![0u8; len as usize];
        s.read_exact(&mut body).await.unwrap();
        let mut pos = 0;
        let id = parse_varint(&body, &mut pos).unwrap();
        (id, body[pos..].to_vec())
    }

    #[tokio::test]
    async fn handshake_roundtrip() {
        let bytes = handshake_bytes(2);
        let (raw, hs) = read_handshake(&mut bytes.as_slice()).await.unwrap();
        assert_eq!(raw, bytes);
        assert_eq!(hs, Handshake { protocol: 774, address: "localhost".into(), port: 25565, next_state: 2 });
    }

    #[tokio::test]
    async fn waiting_status_and_wake_on_login() {
        let woke = Arc::new(AtomicUsize::new(0));
        let w = woke.clone();
        let p = start(0, None, Arc::new(move || { w.fetch_add(1, Ordering::SeqCst); }), Arc::new(|| {})).await.unwrap();

        // lista de servidores: não acorda, mostra o MOTD e responde o ping
        let mut s = TcpStream::connect(p.addr).await.unwrap();
        s.write_all(&handshake_bytes(1)).await.unwrap();
        s.write_all(&packet(0x00, &[])).await.unwrap();
        let (id, body) = read_packet(&mut s).await;
        assert_eq!(id, 0);
        let json = String::from_utf8_lossy(&body);
        assert!(json.contains("Mundo livre"), "{json}");
        assert!(json.contains("774"), "devolve o protocolo do cliente");
        s.write_all(&packet(0x01, &42i64.to_be_bytes())).await.unwrap();
        let (id, body) = read_packet(&mut s).await;
        assert_eq!((id, body), (1, 42i64.to_be_bytes().to_vec()));
        assert_eq!(woke.load(Ordering::SeqCst), 0);

        // tentar entrar: acorda e recebe a mensagem
        let mut s = TcpStream::connect(p.addr).await.unwrap();
        s.write_all(&handshake_bytes(2)).await.unwrap();
        let (id, body) = read_packet(&mut s).await;
        assert_eq!(id, 0);
        assert!(String::from_utf8_lossy(&body).contains("Acordando"));
        assert_eq!(woke.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn forwards_to_local_server_and_counts_players() {
        let srv = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = srv.local_addr().unwrap().port();
        let p = start(0, None, Arc::new(|| {}), Arc::new(|| {})).await.unwrap();
        p.set_route(Route::Local(port));
        let mut c = TcpStream::connect(p.addr).await.unwrap();
        c.write_all(&handshake_bytes(2)).await.unwrap();
        c.write_all(b"login...").await.unwrap();
        let (mut up, _) = srv.accept().await.unwrap();
        let (_, hs) = read_handshake(&mut up).await.unwrap();
        assert_eq!(hs.next_state, 2);
        let mut buf = [0u8; 8];
        up.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"login...");
        assert!(p.playing());
        drop(c);
        drop(up);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!p.playing());
    }
}
