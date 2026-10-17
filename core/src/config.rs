//! Configuração local deste PC (sem segredos: esses ficam em [`crate::secrets`]).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::journal::JournalSettings;
use crate::paths::Paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Nome do jogador (mesmo nick usado no Minecraft).
    pub player_name: String,
    /// Este PC é do dono (convida, aprova membros, muda as configurações do mundo).
    pub is_owner: bool,
    /// Endereço da coordenação (o Worker do dono).
    pub coordinator_url: Option<String>,
    pub world_id: Option<String>,
    pub world_name: String,
    pub minecraft_dir: Option<PathBuf>,
    pub launcher_path: Option<PathBuf>,
    /// Abrir o Minecraft direto pelo app (Java próprio, entra no mundo sozinho) em vez do
    /// launcher. Só no build pessoal (o cliente entra em modo offline).
    pub direct_launch: bool,
    /// Heap manual do servidor em MB (None = automático).
    pub ram_mb: Option<u64>,
    /// Memória do jogo em MB quando aberto direto (None = automático).
    pub client_ram_mb: Option<u64>,
    pub checkpoint_minutes: u64,
    /// Tolerância depois que o último jogador sai, antes de encerrar.
    pub grace_minutes: u64,
    /// Porta do proxy local (o Minecraft entra em `localhost` nesta porta).
    pub proxy_port: u16,
    /// Se o host cair, este PC assume sozinho (se estava jogando).
    pub auto_failover: bool,
    /// Tentar entrar no mundo pelo Minecraft com ninguém hospedando acorda o mundo neste PC.
    pub wake_on_join: bool,
    /// Manter uma cópia atualizada do mundo neste PC em segundo plano.
    pub background_replica: bool,
    pub notify_toasts: bool,
    pub notify_join_leave: bool,
    pub journal: JournalSettings,
    pub eula_accepted: bool,
    pub setup_complete: bool,
    /// Canal de atualização: "estavel" (todos) ou "teste" (versões para testar com os amigos
    /// antes de liberar). Preparado, sem tela: muda-se editando o config.json.
    pub canal_atualizacao: String,
    /// Versão da `.minecraft` para abrir o jogo direto (OptiFine, Fabric...). Vazio = o
    /// Minecraft vanilla do próprio app.
    pub client_version: Option<String>,
    /// Endereço do `latest.json` no lugar do padrão (ex.: o Worker conferindo a licença na
    /// versão comercial). Vazio = padrão do build.
    pub endpoint_atualizacao: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: String::new(),
            is_owner: false,
            coordinator_url: None,
            world_id: None,
            world_name: "pAuletOr 3".into(),
            minecraft_dir: default_minecraft_dir(),
            launcher_path: None,
            direct_launch: crate::OFFLINE_MODE,
            ram_mb: None,
            client_ram_mb: None,
            checkpoint_minutes: 5,
            grace_minutes: 2,
            proxy_port: crate::GAME_PORT,
            auto_failover: true,
            wake_on_join: true,
            background_replica: true,
            notify_toasts: true,
            notify_join_leave: true,
            journal: JournalSettings::default(),
            eula_accepted: false,
            setup_complete: false,
            canal_atualizacao: "estavel".into(),
            client_version: None,
            endpoint_atualizacao: None,
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

    /// Endereço que o jogador usa no Minecraft (sempre o mesmo, em qualquer PC).
    pub fn game_address(&self) -> String {
        if self.proxy_port == 25565 {
            "localhost".into()
        } else {
            format!("localhost:{}", self.proxy_port)
        }
    }

    /// Pronto para usar: assistente concluído e ligado a um mundo da v2. Uma config da v1
    /// (Drive/Tailscale) não tem mundo e volta para o assistente.
    pub fn ready(&self) -> bool {
        self.setup_complete && self.joined()
    }

    /// O botão Jogar abre o Minecraft direto (e não o launcher).
    pub fn launches_directly(&self) -> bool {
        crate::OFFLINE_MODE && self.direct_launch
    }

    /// Ligado a um mundo (coordenação + id).
    pub fn joined(&self) -> bool {
        self.coordinator_url.is_some() && self.world_id.is_some()
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
        let mut c = Config::default();
        assert_eq!(c.game_address(), "localhost");
        c.proxy_port = 25570;
        assert_eq!(c.game_address(), "localhost:25570");
        // config antiga (v1) ainda abre, com padrões no que falta
        let old: Config = serde_json::from_str(r#"{"player_name":"Ana","duckdns_domain":"x"}"#).unwrap();
        assert_eq!(old.player_name, "Ana");
        assert_eq!(old.checkpoint_minutes, 5);
    }
}
