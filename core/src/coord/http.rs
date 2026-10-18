//! Cliente da coordenação (Cloudflare Worker + Durable Object). O contrato está em
//! `docs/api.md`; o Worker em `coordinator/`.
//!
//! Autenticação: cada pedido é assinado com a chave do app (Ed25519, a mesma identidade do
//! iroh). Cabeçalhos `x-p3-id` (chave pública hex), `x-p3-ts` (ms) e `x-p3-sig`, assinatura de
//! `p3v1\n{MÉTODO}\n{caminho?query}\n{ts}\n{sha256 do corpo | "-"}`. Não há senha nem conta.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::*;
use crate::{NmError, NmResult};

/// Acesso assinado à API (compartilhado pela coordenação e pela nuvem de blobs).
#[derive(Clone)]
pub struct ApiClient {
    pub base: String,
    pub world_id: String,
    secret: Arc<Zeroizing<[u8; 32]>>,
    pub id: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient").field("base", &self.base).field("world", &self.world_id).finish()
    }
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    lease: Option<LeaseInfo>,
    #[serde(default)]
    best_seq: Option<u64>,
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    host_version: Option<String>,
    #[serde(default)]
    versao_minima: Option<String>,
}

pub enum Body<'a> {
    None,
    /// JSON assinado (o hash do corpo entra na assinatura).
    Json(Vec<u8>),
    /// Bytes de blob (não entram na assinatura; o conteúdo é endereçado por hash e conferido
    /// por quem baixa).
    Blob(&'a [u8]),
}

impl ApiClient {
    pub fn new(base: &str, world_id: &str, secret: [u8; 32]) -> NmResult<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Machenia/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| NmError::Other(e.into()))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            world_id: world_id.to_string(),
            id: crate::p2p::public_hex(&secret),
            secret: Arc::new(Zeroizing::new(secret)),
            http,
        })
    }

    pub fn world_path(&self, rest: &str) -> String {
        format!("/v1/worlds/{}/{}", self.world_id, rest.trim_start_matches('/'))
    }

    fn sign(&self, method: &str, path: &str, ts: u64, body_hash: &str) -> String {
        let msg = format!("p3v1\n{method}\n{path}\n{ts}\n{body_hash}");
        crate::p2p::sign(&self.secret, msg.as_bytes())
    }

    /// Cabeçalhos de autenticação para um GET em `path` sem corpo (usado pelo updater, que
    /// assina o prefixo `/u/:mundo` e repete os cabeçalhos no JSON e no download).
    pub fn auth_headers(&self, path: &str) -> [(&'static str, String); 3] {
        let ts = chrono::Utc::now().timestamp_millis() as u64;
        let sig = self.sign("GET", path, ts, "-");
        [("x-p3-id", self.id.clone()), ("x-p3-ts", ts.to_string()), ("x-p3-sig", sig)]
    }

    /// Faz um pedido assinado. `path` inclui a query.
    pub async fn send(&self, method: reqwest::Method, path: &str, body: Body<'_>, extra: &[(&str, &str)]) -> NmResult<reqwest::Response> {
        let ts = chrono::Utc::now().timestamp_millis() as u64;
        let body_hash = match &body {
            Body::Json(b) => crate::hashing::sha256_bytes(b),
            _ => "-".to_string(),
        };
        let sig = self.sign(method.as_str(), path, ts, &body_hash);
        let mut rb = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .header("x-p3-id", &self.id)
            .header("x-p3-ts", ts.to_string())
            .header("x-p3-sig", sig);
        for (k, v) in extra {
            rb = rb.header(*k, *v);
        }
        rb = match body {
            Body::None => rb,
            Body::Json(b) => rb.header("content-type", "application/json").body(b),
            Body::Blob(b) => rb.header("content-type", "application/octet-stream").body(b.to_vec()),
        };
        let resp = rb.send().await.map_err(|e| NmError::Remote(format!("sem resposta da coordenação: {e}")))?;
        if resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(resp);
        }
        Err(Self::to_error(resp).await)
    }

    async fn to_error(resp: reqwest::Response) -> NmError {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let Ok(e) = serde_json::from_str::<ApiError>(&text) else {
            return NmError::Remote(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
        };
        let msg = e.message.clone().unwrap_or_else(|| e.error.clone());
        match e.error.as_str() {
            "held_by" => match e.lease {
                Some(l) => NmError::HeldBy(Box::new(l)),
                None => NmError::Remote(msg),
            },
            "not_best" => NmError::NotBestCandidate { best_seq: e.best_seq.unwrap_or(0) },
            "lost_lock" => NmError::LostLock,
            "head_moved" => NmError::HeadMoved { current: e.current },
            "forbidden" | "not_member" | "pending_approval" | "bad_signature" | "invite_invalid" => NmError::Forbidden(msg),
            "format_too_new" => NmError::FormatTooNew { found: 0 },
            "app_version_mismatch" => NmError::AppVersionMismatch { host: e.host_version.unwrap_or_default(), mine: crate::APP_VERSION.into() },
            "app_outdated" => NmError::AppOutdated { atual: crate::APP_VERSION.into(), minima: e.versao_minima.unwrap_or_default() },
            "quota" => NmError::CloudFull(msg),
            _ if status.is_server_error() || status.as_u16() == 429 => NmError::Remote(msg),
            _ => NmError::Remote(format!("{status}: {msg}")),
        }
    }

    pub async fn json<T: DeserializeOwned>(&self, method: reqwest::Method, path: &str, body: Option<&impl Serialize>) -> NmResult<T> {
        let b = match body {
            Some(v) => Body::Json(serde_json::to_vec(v)?),
            None => Body::None,
        };
        let resp = self.send(method, path, b, &[]).await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(NmError::Remote(format!("{path}: não encontrado")));
        }
        resp.json::<T>().await.map_err(|e| NmError::Remote(format!("resposta ilegível: {e}")))
    }
}

/// Cria um mundo novo na coordenação. Só quem tem a chave de administração do Worker
/// (o dono, que publicou o Worker) consegue — é a "licença" do build pessoal (ver
/// [`crate::license`]).
pub async fn create_world(base: &str, secret: [u8; 32], admin_token: &str, name: &str, nick: &str) -> NmResult<String> {
    let api = ApiClient::new(base, "-", secret)?;
    #[derive(Serialize)]
    struct Req<'a> {
        name: &'a str,
        nick: &'a str,
    }
    #[derive(Deserialize)]
    struct Resp {
        world_id: String,
    }
    let body = serde_json::to_vec(&Req { name, nick })?;
    let resp = api.send(reqwest::Method::POST, "/v1/worlds", Body::Json(body), &[("x-p3-admin", admin_token)]).await?;
    let r: Resp = resp.json().await.map_err(|e| NmError::Remote(format!("resposta ilegível: {e}")))?;
    Ok(r.world_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinResult {
    pub world_id: String,
    pub name: String,
    pub approved: bool,
    pub owner_nick: String,
}

/// Entra num mundo com o token do convite (não precisa ser membro ainda).
pub async fn redeem_invite(base: &str, secret: [u8; 32], token: &str, nick: &str) -> NmResult<JoinResult> {
    let api = ApiClient::new(base, "-", secret)?;
    #[derive(Serialize)]
    struct Req<'a> {
        nick: &'a str,
    }
    api.json(reqwest::Method::POST, &format!("/v1/join/{token}"), Some(&Req { nick })).await
}

/// A coordenação de verdade.
#[derive(Clone, Debug)]
pub struct HttpCoordinator {
    pub api: ApiClient,
}

impl HttpCoordinator {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }
}

#[derive(Serialize)]
struct HeartbeatReq<'a> {
    session_number: u64,
    online: &'a [String],
    handing_over: bool,
}

#[derive(Serialize)]
struct ReleaseReq {
    session_number: u64,
    handoff: bool,
}

#[derive(Serialize)]
struct HeadReq<'a> {
    session_number: u64,
    expected: Option<&'a str>,
    manifest: &'a str,
}

#[derive(Serialize)]
struct MemberReq {
    approved: Option<bool>,
    remove: bool,
}

#[derive(Deserialize)]
struct Ok_ {}

#[async_trait::async_trait]
impl Coordinator for HttpCoordinator {
    fn me(&self) -> String {
        self.api.id.clone()
    }

    async fn view(&self) -> NmResult<WorldView> {
        self.api.json(reqwest::Method::GET, &self.api.world_path("view"), None::<&()>).await
    }

    async fn acquire(&self, req: AcquireRequest) -> NmResult<LeaseInfo> {
        self.api.json(reqwest::Method::POST, &self.api.world_path("lease/acquire"), Some(&req)).await
    }

    async fn heartbeat(&self, session_number: u64, online: &[String], handing_over: bool) -> NmResult<LeaseInfo> {
        let r = HeartbeatReq { session_number, online, handing_over };
        self.api.json(reqwest::Method::POST, &self.api.world_path("lease/heartbeat"), Some(&r)).await
    }

    async fn release(&self, session_number: u64, handoff: bool) -> NmResult<()> {
        let _: Ok_ = self
            .api
            .json(reqwest::Method::POST, &self.api.world_path("lease/release"), Some(&ReleaseReq { session_number, handoff }))
            .await?;
        Ok(())
    }

    async fn head(&self) -> NmResult<Option<Head>> {
        self.api.json(reqwest::Method::GET, &self.api.world_path("head"), None::<&()>).await
    }

    async fn publish_head(&self, session_number: u64, expected: Option<&str>, manifest: &str) -> NmResult<Head> {
        let r = HeadReq { session_number, expected, manifest };
        self.api.json(reqwest::Method::POST, &self.api.world_path("head"), Some(&r)).await
    }

    async fn presence(&self, p: &Presence) -> NmResult<WorldView> {
        self.api.json(reqwest::Method::POST, &self.api.world_path("presence"), Some(p)).await
    }

    async fn post_events(&self, events: &[DiaryEvent]) -> NmResult<()> {
        let _: Ok_ = self.api.json(reqwest::Method::POST, &self.api.world_path("events"), Some(&events)).await?;
        Ok(())
    }

    async fn events(&self, limit: usize) -> NmResult<Vec<DiaryEvent>> {
        self.api.json(reqwest::Method::GET, &self.api.world_path(&format!("events?limit={limit}")), None::<&()>).await
    }

    async fn create_invite(&self, opts: &InviteOptions) -> NmResult<Invite> {
        self.api.json(reqwest::Method::POST, &self.api.world_path("invites"), Some(opts)).await
    }

    async fn invites(&self) -> NmResult<Vec<Invite>> {
        self.api.json(reqwest::Method::GET, &self.api.world_path("invites"), None::<&()>).await
    }

    async fn revoke_invite(&self, id: &str) -> NmResult<()> {
        let _: Ok_ = self.api.json(reqwest::Method::DELETE, &self.api.world_path(&format!("invites/{id}")), None::<&()>).await?;
        Ok(())
    }

    async fn set_member(&self, id: &str, approved: Option<bool>, remove: bool) -> NmResult<()> {
        let _: Ok_ = self
            .api
            .json(reqwest::Method::POST, &self.api.world_path(&format!("members/{id}")), Some(&MemberReq { approved, remove }))
            .await?;
        Ok(())
    }

    async fn private_settings(&self) -> NmResult<PrivateSettings> {
        self.api.json(reqwest::Method::GET, &self.api.world_path("private"), None::<&()>).await
    }

    async fn set_private_settings(&self, s: &PrivateSettings) -> NmResult<()> {
        let _: Ok_ = self.api.json(reqwest::Method::PUT, &self.api.world_path("private"), Some(s)).await?;
        Ok(())
    }

    async fn call(&self, to: &str) -> NmResult<()> {
        #[derive(Serialize)]
        struct Req<'a> {
            to: &'a str,
        }
        let _: Ok_ = self.api.json(reqwest::Method::POST, &self.api.world_path("call"), Some(&Req { to })).await?;
        Ok(())
    }
}
