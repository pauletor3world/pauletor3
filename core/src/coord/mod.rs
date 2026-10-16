//! Coordenação do mundo: a autoridade sobre **quem hospeda** (lease), **qual é a versão
//! oficial** (HEAD), **quem é membro** e **quem está online**.
//!
//! Na v1 isso era feito com arquivos no Google Drive, que não tem compare-and-swap. Na v2 a
//! autoridade é um Durable Object da Cloudflare (ver `coordinator/`), que serializa todas as
//! operações de um mundo e por isso dá CAS de verdade. O núcleo só conhece o trait
//! [`Coordinator`]; [`mem::MemCoordinator`] é a implementação de referência (testes com rede
//! simulada) e [`http::HttpCoordinator`] fala com o Worker. As duas seguem as mesmas regras,
//! descritas em [`rules`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::NmResult;

pub mod http;
pub mod mem;
pub mod rules;

/// Para que a vez foi pega.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeasePurpose {
    /// Hospedando o mundo.
    Hosting,
    /// Manutenção (restaurar versão, coleta de lixo, importação). Ninguém joga.
    Maintenance,
}

/// A vez de hospedar. Só existe uma por mundo; quem decide é a coordenação.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LeaseInfo {
    pub format: u32,
    /// Número de sessão monotônico: cada `acquire` bem-sucedido incrementa.
    /// O HEAD só aceita publicação com o número da vez atual (cerca de segurança).
    pub session_number: u64,
    pub session_id: String,
    /// Identidade do PC que hospeda (chave pública do iroh, em hex).
    pub holder: String,
    pub player: String,
    pub machine: String,
    /// Endereço P2P do host (JSON de `iroh::EndpointAddr`), para o proxy dos amigos.
    pub endpoint: Option<String>,
    pub app_version: String,
    pub purpose: LeasePurpose,
    pub acquired_at: DateTime<Utc>,
    pub heartbeat_at: DateTime<Utc>,
    /// Último aviso de um membro de que o host responde por P2P (mantém a vez viva se só a
    /// conexão do host com a coordenação caiu).
    #[serde(default)]
    pub vouched_at: Option<DateTime<Utc>>,
    /// O host está passando a vez (os amigos podem assumir assim que ela for liberada).
    #[serde(default)]
    pub handing_over: bool,
    /// Jogadores online agora.
    #[serde(default)]
    pub online: Vec<String>,
}

impl LeaseInfo {
    /// Momento a partir do qual a vez é considerada abandonada.
    pub fn expires_at(&self, ttl: chrono::Duration) -> DateTime<Utc> {
        let last = match self.vouched_at {
            Some(v) if v > self.heartbeat_at => v,
            _ => self.heartbeat_at,
        };
        last + ttl
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeaseStatus {
    /// Ninguém hospedando. `previous` é a última vez que **expirou** sem ser liberada
    /// (o host caiu), para o diário e a interface explicarem.
    Free {
        #[serde(default)]
        previous: Option<LeaseInfo>,
    },
    Held { lease: LeaseInfo },
}

impl LeaseStatus {
    pub fn held(&self) -> Option<&LeaseInfo> {
        match self {
            LeaseStatus::Held { lease } => Some(lease),
            _ => None,
        }
    }
    pub fn is_free(&self) -> bool {
        matches!(self, LeaseStatus::Free { .. })
    }
}

/// Aponta para o manifesto oficial. Só muda com CAS, pela coordenação.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Head {
    pub format: u32,
    pub manifest: String,
    /// Conta as publicações (monotônico).
    pub seq: u64,
    /// `session_number` da vez que publicou.
    pub session_number: u64,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Member,
}

/// O que cada app conta de si a cada poucos segundos.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Presence {
    /// Endereço P2P deste PC (JSON de `iroh::EndpointAddr`).
    pub endpoint: Option<String>,
    /// `seq` do HEAD mais novo que este PC tem **inteiro e conferido** no disco.
    pub replica_seq: u64,
    /// O Minecraft deste PC está conectado ao mundo (pelo proxy local).
    pub playing: bool,
    /// `session_number` do host que este PC está alcançando por P2P agora.
    pub host_reachable: Option<u64>,
    pub app_version: String,
    /// Tipo de conexão com o host ("direta", "relay"), para a tela de Saúde.
    #[serde(default)]
    pub path: Option<String>,
    /// Porta do proxy local deste PC (o host manda o jogo de volta para ela na troca).
    #[serde(default)]
    pub proxy_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Member {
    /// Chave pública do app (hex).
    pub id: String,
    pub nick: String,
    pub role: Role,
    pub approved: bool,
    pub joined_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    /// Presença recente (some depois de ~1 min sem notícias).
    #[serde(default)]
    pub presence: Option<Presence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invite {
    /// Token do convite (só aparece na criação; a coordenação guarda o hash).
    #[serde(default)]
    pub token: Option<String>,
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub uses_left: u32,
    /// Quem entra por este convite já entra aprovado.
    pub preapproved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteOptions {
    pub hours: u32,
    pub uses: u32,
    pub preapproved: bool,
}

impl Default for InviteOptions {
    fn default() -> Self {
        Self { hours: 48, uses: 5, preapproved: true }
    }
}

/// Fotografia do mundo do ponto de vista da coordenação.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldView {
    pub world_id: String,
    pub name: String,
    pub lease: LeaseStatus,
    pub head: Option<Head>,
    pub members: Vec<Member>,
    /// Relógio da coordenação (evita depender do relógio de cada PC).
    pub now: DateTime<Utc>,
    /// O app que pediu é dono?
    pub me: Option<Member>,
}

impl WorldView {
    pub fn online_members(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.presence.is_some())
    }
}

/// Pedido de vez.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquireRequest {
    pub session_id: String,
    pub player: String,
    pub machine: String,
    pub endpoint: Option<String>,
    pub purpose: LeasePurpose,
    /// `seq` do HEAD que este PC tem inteiro (para preferir a réplica mais nova no failover).
    pub replica_seq: u64,
    pub app_version: String,
}

/// Evento do diário do mundo (linha do tempo dentro do app).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiaryEvent {
    pub at: DateTime<Utc>,
    /// "opened", "joined", "left", "death", "advancement", "host_changed", "saved", ...
    pub kind: String,
    pub player: Option<String>,
    pub text: String,
}

/// A autoridade do mundo. Todas as operações são atômicas do lado da coordenação.
#[async_trait::async_trait]
pub trait Coordinator: Send + Sync {
    /// Identidade deste app (chave pública, hex).
    fn me(&self) -> String;

    async fn view(&self) -> NmResult<WorldView>;

    /// Pega a vez. Erros: `HeldBy` (alguém hospeda), `NotBestCandidate` (no failover, outro
    /// membro tem réplica mais nova e ainda está no prazo de preferência).
    async fn acquire(&self, req: AcquireRequest) -> NmResult<LeaseInfo>;

    /// Mantém a vez viva. `LostLock` se ela não é mais nossa.
    async fn heartbeat(&self, session_number: u64, online: &[String], handing_over: bool) -> NmResult<LeaseInfo>;

    /// Libera a vez (só depois do HEAD final). Com `handoff`, a vez fica "vencida agora" com
    /// `handing_over`: quem estava jogando assume pelo caminho do failover.
    async fn release(&self, session_number: u64, handoff: bool) -> NmResult<()>;

    async fn head(&self) -> NmResult<Option<Head>>;

    /// Publica um HEAD novo com CAS: exige a vez atual (`session_number`) e que o HEAD atual
    /// seja `expected`. Os objetos e o manifesto já precisam estar no armazenamento.
    async fn publish_head(&self, session_number: u64, expected: Option<&str>, manifest: &str) -> NmResult<Head>;

    /// Conta que este PC está online (e o que ele tem) e recebe a fotografia do mundo na
    /// mesma ida e volta (economiza pedidos: o plano grátis tem limite diário).
    async fn presence(&self, p: &Presence) -> NmResult<WorldView>;

    /// Diário do mundo.
    async fn post_events(&self, events: &[DiaryEvent]) -> NmResult<()>;
    async fn events(&self, limit: usize) -> NmResult<Vec<DiaryEvent>>;

    // ---- só o dono
    async fn create_invite(&self, opts: &InviteOptions) -> NmResult<Invite>;
    async fn invites(&self) -> NmResult<Vec<Invite>>;
    async fn revoke_invite(&self, id: &str) -> NmResult<()>;
    async fn set_member(&self, id: &str, approved: Option<bool>, remove: bool) -> NmResult<()>;
    /// Configurações privadas do mundo (ex.: webhook do Discord). Membros aprovados leem.
    async fn private_settings(&self) -> NmResult<PrivateSettings>;
    async fn set_private_settings(&self, s: &PrivateSettings) -> NmResult<()>;
}

/// Configurações guardadas na coordenação, visíveis só para membros aprovados.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PrivateSettings {
    pub discord_webhook: Option<String>,
}
