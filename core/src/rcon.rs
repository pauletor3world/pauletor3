//! Cliente RCON (protocolo Source RCON usado pelo servidor Minecraft).
//! Pacote: i32 tamanho (LE) | i32 id | i32 tipo | corpo ASCII | 0x00 0x00

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const AUTH: i32 = 3;
const EXEC: i32 = 2;
const MAX_PAYLOAD: usize = 4096 * 4;

pub struct Rcon {
    stream: TcpStream,
    next_id: i32,
    timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum RconError {
    #[error("rcon: conexão: {0}")]
    Io(#[from] std::io::Error),
    #[error("rcon: senha recusada")]
    Auth,
    #[error("rcon: tempo esgotado")]
    Timeout,
    #[error("rcon: resposta inválida")]
    Protocol,
}

impl Rcon {
    pub async fn connect(addr: &str, password: &str) -> Result<Self, RconError> {
        let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .map_err(|_| RconError::Timeout)??;
        stream.set_nodelay(true)?;
        let mut r = Self { stream, next_id: 1, timeout: Duration::from_secs(60) };
        let id = r.send(AUTH, password).await?;
        // O servidor pode mandar um pacote vazio de resposta antes da resposta de auth.
        loop {
            let (rid, typ, _) = r.recv().await?;
            if rid == -1 {
                return Err(RconError::Auth);
            }
            if typ == 2 && rid == id {
                return Ok(r);
            }
        }
    }

    async fn send(&mut self, typ: i32, body: &str) -> Result<i32, RconError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let body = body.as_bytes();
        let len = 4 + 4 + body.len() + 2;
        let mut buf = Vec::with_capacity(len + 4);
        buf.extend_from_slice(&(len as i32).to_le_bytes());
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&typ.to_le_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(&[0, 0]);
        self.stream.write_all(&buf).await?;
        Ok(id)
    }

    async fn recv(&mut self) -> Result<(i32, i32, String), RconError> {
        let fut = async {
            let len = self.stream.read_i32_le().await? as usize;
            if !(10..=MAX_PAYLOAD + 10).contains(&len) {
                return Err(RconError::Protocol);
            }
            let mut buf = vec![0u8; len];
            self.stream.read_exact(&mut buf).await?;
            let id = i32::from_le_bytes(buf[0..4].try_into().unwrap());
            let typ = i32::from_le_bytes(buf[4..8].try_into().unwrap());
            let body = String::from_utf8_lossy(&buf[8..len - 2]).into_owned();
            Ok((id, typ, body))
        };
        tokio::time::timeout(self.timeout, fut).await.map_err(|_| RconError::Timeout)?
    }

    /// Executa um comando e devolve a resposta.
    ///
    /// O RCON do servidor vanilla lê **um pacote por leitura** do socket e fecha a conexão se
    /// receber dois pacotes grudados (o tamanho não bate). Por isso nada de "pacote sentinela":
    /// um comando, uma resposta, sempre em sequência.
    pub async fn cmd(&mut self, command: &str) -> Result<String, RconError> {
        let id = self.send(EXEC, command).await?;
        loop {
            let (rid, _, body) = self.recv().await?;
            if rid == id {
                return Ok(body);
            }
            if rid == -1 {
                return Err(RconError::Auth);
            }
        }
    }
}

/// Lê a resposta de `list`: "There are 2 of a max of 8 players online: Ana, Bia".
pub fn parse_list(resp: &str) -> Option<(u32, Vec<String>)> {
    let re = regex::Regex::new(r"There are (\d+) of a max(?: of)? (\d+) players online:?\s*(.*)").ok()?;
    let c = re.captures(resp.trim())?;
    let n: u32 = c.get(1)?.as_str().parse().ok()?;
    let names: Vec<String> = c
        .get(3)
        .map(|m| m.as_str())
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some((n, names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list() {
        assert_eq!(parse_list("There are 0 of a max of 8 players online: "), Some((0, vec![])));
        assert_eq!(
            parse_list("There are 2 of a max of 8 players online: Ana, Bia"),
            Some((2, vec!["Ana".into(), "Bia".into()]))
        );
        assert_eq!(parse_list("lixo"), None);
    }

    /// Servidor RCON falso para testar o protocolo de ponta a ponta.
    #[tokio::test]
    async fn talks_protocol() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            // Igual ao RconClient do vanilla: lê até 1460 bytes de uma vez e exige que seja
            // exatamente UM pacote; se vierem dois grudados, fecha a conexão.
            let mut buf = vec![0u8; 1460];
            loop {
                let n = match s.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                if n < 14 {
                    return;
                }
                let len = i32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
                if len != n - 4 {
                    return; // dois pacotes juntos: o vanilla desconecta
                }
                let id = i32::from_le_bytes(buf[4..8].try_into().unwrap());
                let typ = i32::from_le_bytes(buf[8..12].try_into().unwrap());
                let body = String::from_utf8_lossy(&buf[12..n - 2]).to_string();
                let (rid, reply) = match typ {
                    3 if body == "certa" => (id, String::new()),
                    3 => (-1, String::new()),
                    _ if body == "list" => (id, "There are 1 of a max of 8 players online: Ana".into()),
                    _ => (id, format!("ok {body}")),
                };
                let rtyp: i32 = if typ == 3 { 2 } else { 0 };
                let mut out = vec![];
                out.extend_from_slice(&((10 + reply.len()) as i32).to_le_bytes());
                out.extend_from_slice(&rid.to_le_bytes());
                out.extend_from_slice(&rtyp.to_le_bytes());
                out.extend_from_slice(reply.as_bytes());
                out.extend_from_slice(&[0, 0]);
                s.write_all(&out).await.unwrap();
            }
        });
        let mut r = Rcon::connect(&addr, "certa").await.unwrap();
        let resp = r.cmd("list").await.unwrap();
        assert_eq!(parse_list(&resp).unwrap().1, vec!["Ana"]);
        // vários comandos seguidos na mesma conexão (o checkpoint faz save-off/save-all/save-on)
        for c in ["save-off", "save-all flush", "save-on"] {
            assert_eq!(r.cmd(c).await.unwrap(), format!("ok {c}"));
        }
    }
}
