//! Integração com o cliente Tailscale instalado no PC (CLI `tailscale`).
//! Flags conferidas no `tailscale up --help` da v1.98: `--auth-key`, `--hostname`, `--unattended`.

use std::net::Ipv4Addr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{NmError, NmResult};

/// Página oficial de download (redireciona para o instalador mais recente).
pub const INSTALLER_URL: &str = "https://pkgs.tailscale.com/stable/tailscale-setup-latest.exe";

pub fn cli_path() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Tailscale").join("tailscale.exe")),
        Some(PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe")),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return Some(c);
        }
    }
    which_in_path("tailscale")
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in ["", ".exe"] {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Status {
    pub installed: bool,
    pub backend_state: String,
    pub ipv4: Option<String>,
    pub hostname: Option<String>,
    pub tailnet: Option<String>,
    pub key_expiry: Option<String>,
}

impl Status {
    pub fn running(&self) -> bool {
        self.backend_state == "Running" && self.ipv4.is_some()
    }
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(rename = "BackendState", default)]
    backend_state: String,
    #[serde(rename = "TailscaleIPs", default)]
    ips: Option<Vec<String>>,
    #[serde(rename = "Self")]
    me: Option<RawSelf>,
    #[serde(rename = "CurrentTailnet")]
    tailnet: Option<RawTailnet>,
}
#[derive(Deserialize)]
struct RawSelf {
    #[serde(rename = "HostName", default)]
    host_name: String,
    #[serde(rename = "KeyExpiry")]
    key_expiry: Option<String>,
}
#[derive(Deserialize)]
struct RawTailnet {
    #[serde(rename = "Name", default)]
    name: String,
}

pub fn parse_status(json: &[u8]) -> Option<Status> {
    let raw: RawStatus = serde_json::from_slice(json).ok()?;
    let ipv4 = raw
        .ips
        .unwrap_or_default()
        .into_iter()
        .find(|ip| ip.parse::<Ipv4Addr>().map(is_tailscale_ip).unwrap_or(false));
    Some(Status {
        installed: true,
        backend_state: raw.backend_state,
        ipv4,
        hostname: raw.me.as_ref().map(|m| m.host_name.clone()),
        key_expiry: raw.me.and_then(|m| m.key_expiry),
        tailnet: raw.tailnet.map(|t| t.name),
    })
}

/// 100.64.0.0/10 (faixa CGNAT usada pelo Tailscale).
pub fn is_tailscale_ip(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (64..=127).contains(&o[1])
}

pub async fn status() -> Status {
    let Some(cli) = cli_path() else { return Status::default() };
    let out = crate::util::command(&cli).args(["status", "--json"]).output().await;
    match out {
        Ok(o) => parse_status(&o.stdout).unwrap_or(Status { installed: true, backend_state: "Unknown".into(), ..Default::default() }),
        Err(_) => Status { installed: true, backend_state: "Unknown".into(), ..Default::default() },
    }
}

/// IP v4 do Tailscale deste PC, ou erro se desconectado.
pub async fn my_ip() -> NmResult<String> {
    // Só para testes de ponta a ponta em máquinas fora da rede (ex.: 127.0.0.1).
    if let Ok(ip) = std::env::var("PAULETOR3_TEST_IP") {
        tracing::warn!(%ip, "usando IP de teste no lugar do Tailscale");
        return Ok(ip);
    }
    let s = status().await;
    if s.running() {
        Ok(s.ipv4.unwrap())
    } else {
        Err(NmError::TailscaleDown)
    }
}

/// Um perfil (conta) do Tailscale neste PC. O Tailscale permite várias contas e alterna entre elas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub tailnet: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub selected: bool,
}

pub fn parse_profiles(json: &[u8]) -> Vec<Profile> {
    serde_json::from_slice(json).unwrap_or_default()
}

pub async fn profiles() -> Vec<Profile> {
    let Some(cli) = cli_path() else { return vec![] };
    match crate::util::command(&cli).args(["switch", "--list", "--json"]).output().await {
        Ok(o) => parse_profiles(&o.stdout),
        Err(_) => vec![],
    }
}

async fn run_cli(args: &[String]) -> NmResult<()> {
    let cli = cli_path().ok_or_else(|| NmError::NotConfigured("Tailscale não instalado".into()))?;
    let out = crate::util::command(&cli).args(args).output().await?;
    if out.status.success() {
        Ok(())
    } else {
        Err(NmError::Other(anyhow::anyhow!(
            "tailscale {} falhou: {}",
            args.first().map(String::as_str).unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

async fn wait_running() -> Status {
    for _ in 0..30 {
        let st = status().await;
        if st.running() {
            return st;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    status().await
}

/// Garante que este PC está na rede do mundo, sem mexer nas outras contas do Tailscale:
/// já está nela → nada; existe um perfil dela → `tailscale switch`; senão → `tailscale login`
/// com a auth key (perfil novo, os outros continuam). Devolve o nome da rede do mundo.
pub async fn join_world(auth_key: Option<&str>, hostname: &str, world_tailnet: Option<&str>) -> NmResult<String> {
    let st = status().await;
    if let (Some(w), true) = (world_tailnet, st.running()) {
        if st.tailnet.as_deref() == Some(w) {
            return Ok(w.to_string());
        }
    }
    if let Some(w) = world_tailnet {
        if let Some(p) = profiles().await.into_iter().find(|p| p.tailnet == w) {
            tracing::info!(tailnet = w, "trocando o Tailscale para a rede do mundo");
            run_cli(&["switch".into(), p.id.clone()]).await?;
            let st = wait_running().await;
            if st.running() && st.tailnet.as_deref() == Some(w) {
                return Ok(w.to_string());
            }
        }
    }
    let key = auth_key.ok_or_else(|| NmError::NotConfigured("falta a chave de convite do Tailscale".into()))?;
    run_cli(&[
        "login".into(),
        format!("--auth-key={key}"),
        format!("--hostname={}", sanitize_hostname(hostname)),
        "--nickname=pAuletOr 3".into(),
        "--unattended".into(),
        "--timeout=60s".into(),
    ])
    .await?;
    let st = wait_running().await;
    match (st.running(), st.tailnet) {
        (true, Some(t)) => Ok(t),
        _ => Err(NmError::TailscaleDown),
    }
}

/// Se este PC está na rede do mundo (quando ela é conhecida).
pub fn on_world(st: &Status, world_tailnet: Option<&str>) -> bool {
    st.running() && world_tailnet.map(|w| st.tailnet.as_deref() == Some(w)).unwrap_or(true)
}

pub fn sanitize_hostname(s: &str) -> String {
    let h: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let h = h.trim_matches('-').to_string();
    format!("p3-{}", if h.is_empty() { "jogador".into() } else { h.chars().take(40).collect::<String>() })
}

/// Baixa o instalador oficial para `dest` (a execução fica com a interface, com consentimento).
pub async fn download_installer(dest: &std::path::Path) -> NmResult<()> {
    let bytes = reqwest::Client::new()
        .get(INSTALLER_URL)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| NmError::Remote(format!("baixando Tailscale: {e}")))?
        .bytes()
        .await
        .map_err(|e| NmError::Remote(format!("baixando Tailscale: {e}")))?;
    crate::util::write_atomic(dest, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_json() {
        let j = br#"{"BackendState":"Running","TailscaleIPs":["100.101.102.103","fd7a:115c::1"],
            "Self":{"HostName":"pc-ana","KeyExpiry":"2027-01-01T00:00:00Z"},"CurrentTailnet":{"Name":"ana@gmail.com"}}"#;
        let s = parse_status(j).unwrap();
        assert!(s.running());
        assert_eq!(s.ipv4.as_deref(), Some("100.101.102.103"));
        let j = br#"{"BackendState":"NeedsLogin","TailscaleIPs":null,"Self":null}"#;
        assert!(!parse_status(j).unwrap().running());
        assert!(is_tailscale_ip("100.64.0.1".parse().unwrap()));
        assert!(!is_tailscale_ip("100.128.0.1".parse().unwrap()));
        assert_eq!(sanitize_hostname("Zé da Silva"), "p3-z--da-silva");
        let ps = parse_profiles(br#"[{"id":"ed6f","nickname":"Eu","tailnet":"eu.github","account":"Eu@github","selected":true}]"#);
        assert_eq!(ps[0].tailnet, "eu.github");
        let mut st = parse_status(br#"{"BackendState":"Running","TailscaleIPs":["100.64.0.2"],"CurrentTailnet":{"Name":"eu.github"}}"#).unwrap();
        assert!(on_world(&st, None));
        assert!(!on_world(&st, Some("pauletor3world.github")));
        st.tailnet = Some("pauletor3world.github".into());
        assert!(on_world(&st, Some("pauletor3world.github")));
    }
}
