//! Núcleo do pAuletOr 3.
//!
//! Tudo que importa para a integridade do mundo mora aqui: o modelo de sincronização
//! endereçado por conteúdo ([`sync`]), a trava de host ([`lock`]), o ciclo do servidor
//! ([`server`]) e a orquestração de sessão ([`session`]). A interface (Tauri) e a CLI são
//! apenas clientes deste crate.

pub mod checkpoint;
pub mod config;
pub mod diagnostics;
pub mod death_templates;
pub mod journal;
pub mod duckdns;
pub mod error;
pub mod events;
pub mod firewall;
pub mod hashing;
pub mod health;
pub mod heap;
pub mod invite;
pub mod java;
pub mod launcher;
pub mod lock;
pub mod logging;
pub mod logparse;
pub mod manifest;
pub mod mirror;
pub mod nbt;
pub mod paths;
pub mod players;
pub mod rcon;
pub mod remote;
pub mod retention;
pub mod secrets;
pub mod server;
pub mod serversdat;
pub mod session;
pub mod shutdown;
pub mod stats;
pub mod sync;
pub mod tailscale;
pub mod util;
pub mod versions;

pub use error::{NmError, NmResult};

/// Versão do formato dos arquivos no remote (manifesto, HEAD, trava, stats).
/// Um app que encontre um formato maior do que este se recusa a hospedar.
pub const FORMAT_VERSION: u32 = 1;

/// Versão do app (vem do Cargo).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Porta padrão do jogo.
pub const GAME_PORT: u16 = 25565;

/// Porta do RCON. Fica atrás de uma regra de bloqueio no firewall (ver [`firewall`]).
pub const RCON_PORT: u16 = 25575;
