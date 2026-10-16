//! Configuração local deste PC (sem segredos: esses ficam em [`crate::secrets`]).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::journal::JournalSettings;
use crate::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AddressMode {
    /// Um nome DuckDNS fixo aponta para o IP do Tailscale do host atual.
    #[default]
    Duckdns,
    /// Fallback: o app de cada jogador reescreve o IP do host no servers.dat.
    DirectIp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// Nome do jogador (mesmo nick usado no Minecraft).
    pub player_name: String,
    /// Este PC é do dono (pode gerar convites e mudar as configurações do mundo).
    #[serde(default)]
    pub is_owner: bool,
    pub minecraft_dir: Option<PathBuf>,
    pub launcher_path: Option<PathBuf>,
    /// Heap manual em MB (None = automático).
    pub ram_mb: Option<u64>,
    pub checkpoint_minutes: u64,
    /// Tolerância depois que o último jogador sai, antes de encerrar.
    pub grace_minutes: u64,
    pub remote_root: String,
    pub duckdns_domain: Option<String>,
    pub address_mode: AddressMode,
    pub tailscale_key_expires: Option<chrono::NaiveDate>,
    /// Rede Tailscale do mundo; o app troca para ela para jogar.
    #[serde(default)]
    pub world_tailnet: Option<String>,
    pub notify_toasts: bool,
    pub notify_join_leave: bool,
    pub journal: JournalSettings,
    pub eula_accepted: bool,
    pub setup_complete: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: String::new(),
            is_owner: false,
            minecraft_dir: default_minecraft_dir(),
            launcher_path: None,
            ram_mb: None,
            checkpoint_minutes: 15,
            grace_minutes: 2,
            remote_root: "pAuletOr3".into(),
            duckdns_domain: None,
            address_mode: AddressMode::Duckdns,
            tailscale_key_expires: None,
            world_tailnet: None,
            notify_toasts: true,
            notify_join_leave: true,
            journal: JournalSettings::default(),
            eula_accepted: false,
            setup_complete: false,
        }
    }
}

pub fn default_minecraft_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(".minecraft"))
}

impl Config {
    pub fn load(paths: &Paths) -> Self {
        std::fs::read(paths.config())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        crate::util::write_atomic(&paths.config(), &serde_json::to_vec_pretty(self)?)
    }

    /// Endereço que os jogadores usam (o hostname DuckDNS, se configurado).
    pub fn public_address(&self, host_ip: Option<&str>) -> Option<String> {
        match (self.address_mode, &self.duckdns_domain, host_ip) {
            (AddressMode::Duckdns, Some(d), _) => Some(crate::duckdns::fqdn(d)),
            (_, _, Some(ip)) => Some(ip.to_string()),
            _ => None,
        }
    }

    pub fn is_valid_player_name(name: &str) -> bool {
        (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_address() {
        assert!(Config::is_valid_player_name("Ana_23"));
        assert!(!Config::is_valid_player_name("Zé"));
        assert!(!Config::is_valid_player_name("ab"));
        let mut c = Config { duckdns_domain: Some("pauletor3".into()), ..Default::default() };
        assert_eq!(c.public_address(Some("100.64.0.1")).unwrap(), "pauletor3.duckdns.org");
        c.address_mode = AddressMode::DirectIp;
        assert_eq!(c.public_address(Some("100.64.0.1")).unwrap(), "100.64.0.1");
    }
}
