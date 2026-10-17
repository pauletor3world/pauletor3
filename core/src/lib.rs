//! Núcleo do Machen Craft.
//!
//! Tudo que importa para a integridade do mundo mora aqui: o modelo de sincronização
//! endereçado por conteúdo ([`sync`]), a trava de host ([`lock`]), o ciclo do servidor
//! ([`server`]) e a orquestração de sessão ([`session`]). A interface (Tauri) e a CLI são
//! apenas clientes deste crate.

pub mod checkpoint;
pub mod client;
pub mod config;
pub mod coord;
pub mod diagnostics;
pub mod death_templates;
pub mod journal;
pub mod error;
pub mod events;
pub mod hashing;
pub mod health;
pub mod heap;
pub mod invite;
pub mod java;
pub mod launcher;
pub mod lease;
pub mod license;
pub mod logging;
pub mod logparse;
pub mod manifest;
pub mod mirror;
pub mod nbt;
pub mod p2p;
pub mod paths;
pub mod players;
pub mod proxy;
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
pub mod util;
pub mod versions;

pub use error::{NmError, NmResult};

/// Versão do formato dos dados compartilhados (manifesto, HEAD, lease, stats).
/// Um app que encontre um formato maior do que este se recusa a hospedar.
pub const FORMAT_VERSION: u32 = 1;

/// Versão do app (vem do Cargo).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Porta do proxy local: o Minecraft sempre entra em `localhost` (porta padrão do jogo).
pub const GAME_PORT: u16 = 25565;

/// Porta interna do servidor (só em 127.0.0.1; ninguém de fora alcança).
pub const SERVER_PORT: u16 = 25566;

/// Porta do RCON (só em 127.0.0.1, senha aleatória por sessão).
pub const RCON_PORT: u16 = 25575;

/// Modo offline do servidor (`online-mode=false`): só existe no build pessoal
/// (`--features personal`), usado pelo grupo dos autores. O build padrão exige conta original.
pub const OFFLINE_MODE: bool = cfg!(feature = "personal");
