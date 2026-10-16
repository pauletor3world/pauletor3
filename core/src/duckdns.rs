//! Endereço fixo via DuckDNS, apontando para o IP do Tailscale do host atual.
//!
//! API (https://www.duckdns.org/spec.jsp):
//! `https://www.duckdns.org/update?domains={sub}&token={token}&ip={ip}[&verbose=true]` → `OK` ou `KO`.
//! A documentação não restringe faixas de IP; aceitar 100.x foi validado na prática (ver docs/decisoes.md).

use std::net::{IpAddr, ToSocketAddrs};

use crate::{NmError, NmResult};

/// "pauletor3.duckdns.org" ou "pauletor3" → "pauletor3".
pub fn subdomain(host: &str) -> String {
    host.trim().trim_end_matches('.').trim_end_matches(".duckdns.org").to_lowercase()
}

pub fn fqdn(host: &str) -> String {
    format!("{}.duckdns.org", subdomain(host))
}

pub fn is_valid_subdomain(s: &str) -> bool {
    let s = subdomain(s);
    !s.is_empty() && s.len() <= 63 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !s.starts_with('-')
}

pub async fn update(host: &str, token: &str, ip: &str) -> NmResult<()> {
    if !is_valid_subdomain(host) {
        return Err(NmError::Other(anyhow::anyhow!("hostname do DuckDNS inválido")));
    }
    let url = reqwest::Url::parse_with_params(
        "https://www.duckdns.org/update",
        &[("domains", subdomain(host).as_str()), ("token", token), ("ip", ip), ("verbose", "true")],
    )
    .map_err(|e| NmError::Other(e.into()))?;
    let body = reqwest::Client::new()
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| NmError::Remote(format!("DuckDNS: {e}")))?
        .text()
        .await
        .map_err(|e| NmError::Remote(format!("DuckDNS: {e}")))?;
    if body.trim_start().starts_with("OK") {
        Ok(())
    } else {
        Err(NmError::Remote("DuckDNS recusou a atualização (token ou domínio errado?)".into()))
    }
}

/// Resolve o nome pelo DNS do sistema (o mesmo que o Minecraft vai usar).
pub async fn resolve(host: &str) -> Option<Vec<IpAddr>> {
    let h = fqdn(host);
    tokio::task::spawn_blocking(move || (h.as_str(), 25565u16).to_socket_addrs().ok().map(|a| a.map(|s| s.ip()).collect()))
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(subdomain("pAuletOr3.duckdns.org"), "pauletor3");
        assert_eq!(fqdn("pauletor3"), "pauletor3.duckdns.org");
        assert!(is_valid_subdomain("nosso-mundo"));
        assert!(!is_valid_subdomain("a b"));
        assert!(!is_valid_subdomain(""));
    }
}
