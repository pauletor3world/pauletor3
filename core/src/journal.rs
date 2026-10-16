//! Diário do mundo no Discord.
//!
//! - Cada acontecimento vira uma [`Entry`] (um embed já renderizado) numa **fila local persistente**
//!   (`journal-queue.json`). Se a internet cair, as mensagens saem depois, na ordem, sem duplicar.
//! - Um trabalhador envia em ordem, respeitando o rate limit do Discord (HTTP 429 `retry_after` e os
//!   cabeçalhos `X-RateLimit-*`), com novas tentativas e espera crescente.
//! - Eventos em rajada (várias conquistas em segundos) com a mesma `group` são juntados num embed só.
//! - Só o app do host publica os eventos da sessão (quem chama decide; ver `session`).
//! - Cada categoria pode ser ligada/desligada nas configurações.
//!
//! Garantia de "sem duplicar": a entrada só sai da fila depois do 2xx do Discord. Se o PC cair
//! exatamente entre o 2xx e a gravação da fila, aquela mensagem pode sair de novo (janela de milissegundos).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use chrono_tz::America::Sao_Paulo;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify};

use crate::util;

// ------------------------------------------------------------------ categorias

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Session,
    Checkpoint,
    Players,
    Deaths,
    Advancements,
    Milestones,
    Chat,
    Daily,
    Problems,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Session,
        Category::Checkpoint,
        Category::Players,
        Category::Deaths,
        Category::Advancements,
        Category::Milestones,
        Category::Chat,
        Category::Daily,
        Category::Problems,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Category::Session => "Sessões (aberto, salvo, passar a vez)",
            Category::Checkpoint => "Salvamentos automáticos",
            Category::Players => "Entradas e saídas",
            Category::Deaths => "Mortes",
            Category::Advancements => "Conquistas",
            Category::Milestones => "Marcos (Nether, End, dragão, recordes, aniversários)",
            Category::Chat => "Mensagens do chat",
            Category::Daily => "Resumo do dia",
            Category::Problems => "Problemas e recuperações",
        }
    }
    pub fn default_on(self) -> bool {
        !matches!(self, Category::Chat)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JournalSettings {
    pub enabled: BTreeMap<Category, bool>,
}

impl Default for JournalSettings {
    fn default() -> Self {
        Self { enabled: Category::ALL.iter().map(|c| (*c, c.default_on())).collect() }
    }
}

impl JournalSettings {
    pub fn is_on(&self, c: Category) -> bool {
        self.enabled.get(&c).copied().unwrap_or_else(|| c.default_on())
    }
}

// ------------------------------------------------------------------ cores e formato

pub mod color {
    pub const OPEN: u32 = 0x3FB27F;
    pub const SAVED: u32 = 0xE8B04B;
    pub const CHECKPOINT: u32 = 0x4B5263;
    pub const JOIN: u32 = 0x5CA9E6;
    pub const LEAVE: u32 = 0x8A93A6;
    pub const DEATH: u32 = 0x9B2C2C;
    pub const ADVANCEMENT: u32 = 0x7C5CD6;
    pub const MILESTONE: u32 = 0xF2C14E;
    pub const CHAT: u32 = 0x36393F;
    pub const DAILY: u32 = 0x2E8B9A;
    pub const PROBLEM: u32 = 0xE5534B;
    pub const RECOVERED: u32 = 0xE08A3C;
    pub const HANDOVER: u32 = 0xB57EDC;
}

/// "21:03" no horário de Brasília.
pub fn hhmm(t: DateTime<Utc>) -> String {
    t.with_timezone(&Sao_Paulo).format("%H:%M").to_string()
}

pub fn date_br(t: DateTime<Utc>) -> String {
    t.with_timezone(&Sao_Paulo).format("%d/%m/%Y").to_string()
}

pub fn day_sp(t: DateTime<Utc>) -> chrono::NaiveDate {
    t.with_timezone(&Sao_Paulo).date_naive()
}

fn esc(s: &str) -> String {
    // Evita que nomes com _ ou * virem formatação no Discord.
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '_' | '*' | '~' | '`' | '|' | '>' | '\\') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

// ------------------------------------------------------------------ entradas

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub category: Category,
    /// Entradas com a mesma `group`, criadas em poucos segundos, viram um embed só.
    pub group: Option<String>,
    /// Título usado quando a entrada é juntada com outras.
    pub group_title: Option<String>,
    pub title: String,
    pub description: String,
    pub color: u32,
    pub fields: Vec<Field>,
}

impl Entry {
    fn new(category: Category, title: impl Into<String>, description: impl Into<String>, color: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            created_at: Utc::now(),
            category,
            group: None,
            group_title: None,
            title: title.into(),
            description: description.into(),
            color,
            fields: vec![],
        }
    }
    fn grouped(mut self, group: &str, title: &str) -> Self {
        self.group = Some(group.into());
        self.group_title = Some(title.into());
        self
    }
    fn field(mut self, name: &str, value: impl Into<String>, inline: bool) -> Self {
        self.fields.push(Field { name: name.into(), value: value.into(), inline });
        self
    }
}

/// Construtores das mensagens (em português, curtas).
pub mod msg {
    use super::*;
    use crate::util::human_duration;

    pub fn world_opened(host: &str, address: &str, at: DateTime<Utc>) -> Entry {
        Entry::new(
            Category::Session,
            "🟢 Mundo aberto",
            format!("**{}** está hospedando desde as {}.", esc(host), hhmm(at)),
            color::OPEN,
        )
        .field("Endereço", format!("`{address}`"), true)
        .field("Como entrar", "Multiplayer → **pAuletOr 3**", true)
    }

    pub fn world_saved(host: &str, duration_secs: i64, per_player: &[(String, i64)]) -> Entry {
        let who = if per_player.is_empty() {
            "Ninguém entrou desta vez.".to_string()
        } else {
            per_player
                .iter()
                .map(|(p, s)| format!("• **{}** — {}", esc(p), human_duration(*s)))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Entry::new(
            Category::Session,
            "💾 Mundo salvo e fechado",
            format!("Sessão de **{}** durou {}.\n\n{}", esc(host), human_duration(duration_secs), who),
            color::SAVED,
        )
    }

    pub fn checkpoint(size: &str, uploaded: &str) -> Entry {
        Entry::new(
            Category::Checkpoint,
            "Salvamento automático",
            format!("Mundo guardado na nuvem ({size}; {uploaded} enviados)."),
            color::CHECKPOINT,
        )
    }

    pub fn handing_over(host: &str) -> Entry {
        Entry::new(
            Category::Session,
            "🔁 Passando o host",
            format!("**{}** precisou sair. O host passa para quem está jogando, sem ninguém cair.", esc(host)),
            color::HANDOVER,
        )
    }

    pub fn took_over(new_host: &str, previous: Option<&str>) -> Entry {
        let d = match previous {
            Some(p) => format!("**{}** assumiu o mundo depois de **{}**.", esc(new_host), esc(p)),
            None => format!("**{}** assumiu o mundo.", esc(new_host)),
        };
        Entry::new(Category::Session, "🔁 Nova vez", d, color::HANDOVER)
    }

    pub fn joined(player: &str, at: DateTime<Utc>) -> Entry {
        Entry::new(Category::Players, "", format!("➕ **{}** entrou às {}", esc(player), hhmm(at)), color::JOIN)
            .grouped("presence", "👥 Movimento")
    }

    pub fn left(player: &str, played_secs: i64) -> Entry {
        Entry::new(
            Category::Players,
            "",
            format!("➖ **{}** saiu depois de {}", esc(player), human_duration(played_secs)),
            color::LEAVE,
        )
        .grouped("presence", "👥 Movimento")
    }

    pub fn death(player: &str, game_message: &str) -> Entry {
        let _ = player;
        Entry::new(Category::Deaths, "", format!("💀 {}", esc(game_message)), color::DEATH).grouped("deaths", "💀 Mortes")
    }

    pub fn advancement(player: &str, title: &str, challenge: bool) -> Entry {
        let icon = if challenge { "🏅" } else { "🏆" };
        Entry::new(Category::Advancements, "", format!("{icon} **{}** — *{}*", esc(player), esc(title)), color::ADVANCEMENT)
            .grouped("adv", "🏆 Conquistas")
    }

    pub fn first_nether(player: &str) -> Entry {
        Entry::new(
            Category::Milestones,
            "🔥 Primeira ida ao Nether!",
            format!("**{}** abriu caminho para o Nether. O mundo nunca mais foi o mesmo.", esc(player)),
            color::MILESTONE,
        )
    }

    pub fn first_end(player: &str) -> Entry {
        Entry::new(
            Category::Milestones,
            "🌌 Chegamos ao End!",
            format!("**{}** foi o primeiro a pisar no End.", esc(player)),
            color::MILESTONE,
        )
    }

    pub fn dragon(player: &str) -> Entry {
        Entry::new(
            Category::Milestones,
            "🐉 Dragão derrotado!",
            format!("**{}** deu o golpe final no Ender Dragon. Lendário.", esc(player)),
            color::MILESTONE,
        )
    }

    pub fn record(player: &str, secs: i64, previous: i64) -> Entry {
        Entry::new(
            Category::Milestones,
            "⏱️ Novo recorde de tempo jogado",
            format!(
                "**{}** jogou {} seguidos (recorde anterior: {}).",
                esc(player),
                human_duration(secs),
                human_duration(previous)
            ),
            color::MILESTONE,
        )
    }

    pub fn anniversary(label: &str, born: DateTime<Utc>) -> Entry {
        Entry::new(
            Category::Milestones,
            format!("🎂 O mundo faz {label}!"),
            format!("Nascido em {}. Que venham muitas outras aventuras.", date_br(born)),
            color::MILESTONE,
        )
    }

    pub fn chat(player: &str, text: &str) -> Entry {
        Entry::new(Category::Chat, "", format!("💬 **{}**: {}", esc(player), esc(text)), color::CHAT).grouped("chat", "💬 Chat")
    }

    pub fn daily(day: chrono::NaiveDate, d: &crate::stats::DailyStats) -> Entry {
        let mut players: Vec<(&String, &i64)> = d.seconds.iter().collect();
        players.sort_by(|a, b| b.1.cmp(a.1));
        let who = players
            .iter()
            .map(|(p, s)| format!("• **{}** — {}", esc(p), human_duration(**s)))
            .collect::<Vec<_>>()
            .join("\n");
        let deaths: u32 = d.deaths.values().sum();
        let worst = d.deaths.iter().max_by_key(|(_, n)| **n).filter(|(_, n)| **n > 0);
        let mut e = Entry::new(
            Category::Daily,
            format!("📅 Resumo de {}", day.format("%d/%m")),
            if who.is_empty() { "Ninguém jogou.".into() } else { who },
            color::DAILY,
        )
        .field("Mortes", if deaths == 0 { "nenhuma 🎉".to_string() } else { deaths.to_string() }, true)
        .field("Conquistas", d.advancements.len().to_string(), true);
        if let Some((p, n)) = worst {
            e = e.field("Mais azarado", format!("{} ({n})", esc(p)), true);
        }
        if !d.advancements.is_empty() {
            let list = d
                .advancements
                .iter()
                .take(10)
                .map(|(p, t)| format!("{} — *{}*", esc(p), esc(t)))
                .collect::<Vec<_>>()
                .join("\n");
            e = e.field("Destaques", list, false);
        }
        e
    }

    pub fn host_crash_detected(host: &str, minutes: i64) -> Entry {
        Entry::new(
            Category::Problems,
            "⚠️ O host parece ter caído",
            format!(
                "O PC de **{}** parou de responder há {minutes} min. O último salvamento está seguro; alguém pode assumir pelo app.",
                esc(host)
            ),
            color::PROBLEM,
        )
    }

    pub fn recovered(host: &str, published: bool) -> Entry {
        let d = if published {
            format!("O PC de **{}** voltou e enviou o mundo como estava na hora da queda.", esc(host))
        } else {
            format!(
                "O PC de **{}** voltou, mas outra pessoa já tinha assumido. A cópia dele ficou guardada na linha do tempo.",
                esc(host)
            )
        };
        Entry::new(Category::Problems, "🩹 Mundo recuperado após queda", d, color::RECOVERED)
    }

    pub fn upload_failed(host: &str, what_to_do: &str) -> Entry {
        Entry::new(
            Category::Problems,
            "⚠️ Falha ao enviar o mundo",
            format!("O envio do PC de **{}** não terminou. {what_to_do}", esc(host)),
            color::PROBLEM,
        )
    }

    pub fn restored(by: &str, target_label: &str) -> Entry {
        Entry::new(
            Category::Problems,
            "⏪ Versão restaurada",
            format!("**{}** restaurou o mundo para {target_label}. Nada foi apagado: a versão anterior continua na linha do tempo.", esc(by)),
            color::HANDOVER,
        )
    }
}

// ------------------------------------------------------------------ fila

/// Junta entradas consecutivas da mesma `group` criadas dentro de `window`.
/// Devolve (ids consumidos, embeds JSON). No máximo 10 embeds por mensagem (limite do Discord).
pub fn build_batch(queue: &[Entry], window: chrono::Duration) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut ids = vec![];
    let mut embeds = vec![];
    let mut i = 0;
    while i < queue.len() && embeds.len() < 10 {
        let first = &queue[i];
        let mut j = i + 1;
        if let Some(g) = &first.group {
            while j < queue.len()
                && queue[j].group.as_ref() == Some(g)
                && queue[j].created_at - first.created_at <= window
                && j - i < 25
            {
                j += 1;
            }
        }
        let slice = &queue[i..j];
        ids.extend(slice.iter().map(|e| e.id.clone()));
        embeds.push(render(slice));
        i = j;
        // Uma mensagem por "assunto" quando o próximo não é agrupável com o atual: mantém a leitura limpa.
        if embeds.len() >= 3 {
            break;
        }
    }
    (ids, embeds)
}

fn render(slice: &[Entry]) -> serde_json::Value {
    let first = &slice[0];
    let (title, description) = if slice.len() == 1 && !first.title.is_empty() {
        (first.title.clone(), first.description.clone())
    } else if slice.len() == 1 {
        (String::new(), first.description.clone())
    } else {
        let title = first.group_title.clone().unwrap_or_default();
        let body = slice.iter().map(|e| e.description.as_str()).collect::<Vec<_>>().join("\n");
        (title, body)
    };
    let mut e = serde_json::json!({
        "description": truncate(&description, 4000),
        "color": first.color,
        "timestamp": slice.last().unwrap().created_at.to_rfc3339(),
        "footer": { "text": format!("{} · horário de Brasília", hhmm(slice.last().unwrap().created_at)) },
    });
    if !title.is_empty() {
        e["title"] = serde_json::Value::String(truncate(&title, 250));
    }
    if slice.len() == 1 && !first.fields.is_empty() {
        e["fields"] = serde_json::Value::Array(
            first
                .fields
                .iter()
                .take(25)
                .map(|f| serde_json::json!({"name": truncate(&f.name, 250), "value": truncate(&f.value, 1000), "inline": f.inline}))
                .collect(),
        );
    }
    e
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct QueueFile {
    entries: Vec<Entry>,
}

const MAX_QUEUE: usize = 1000;

/// Diário: fila persistente + trabalhador que envia para o webhook.
pub struct Journal {
    path: PathBuf,
    queue: Mutex<Vec<Entry>>,
    notify: Notify,
    pub settings: std::sync::RwLock<JournalSettings>,
    webhook: std::sync::RwLock<Option<String>>,
    world_name: std::sync::RwLock<String>,
    /// Janela de agrupamento (rajadas).
    pub burst_window: Duration,
    status: std::sync::RwLock<JournalStatus>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct JournalStatus {
    pub pending: usize,
    pub last_error: Option<String>,
    pub last_sent_at: Option<DateTime<Utc>>,
}

pub fn is_valid_webhook(url: &str) -> bool {
    (url.starts_with("https://discord.com/api/webhooks/") || url.starts_with("https://discordapp.com/api/webhooks/"))
        && url.len() < 300
        && !url.contains(char::is_whitespace)
}

impl Journal {
    pub fn open(path: PathBuf, settings: JournalSettings, webhook: Option<String>, world_name: &str) -> Arc<Self> {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<QueueFile>(&b).ok())
            .map(|q| q.entries)
            .unwrap_or_default();
        Arc::new(Self {
            path,
            queue: Mutex::new(entries),
            notify: Notify::new(),
            settings: std::sync::RwLock::new(settings),
            webhook: std::sync::RwLock::new(webhook.filter(|w| is_valid_webhook(w))),
            world_name: std::sync::RwLock::new(world_name.to_string()),
            burst_window: Duration::from_secs(8),
            status: Default::default(),
        })
    }

    pub fn set_webhook(&self, w: Option<String>) {
        *self.webhook.write().unwrap() = w.filter(|w| is_valid_webhook(w));
        self.notify.notify_one();
    }

    pub fn set_settings(&self, s: JournalSettings) {
        *self.settings.write().unwrap() = s;
    }

    pub fn set_world_name(&self, n: &str) {
        *self.world_name.write().unwrap() = n.to_string();
    }

    pub fn status(&self) -> JournalStatus {
        self.status.read().unwrap().clone()
    }

    /// Enfileira (se a categoria estiver ligada e houver webhook configurado).
    pub async fn post(&self, e: Entry) {
        if !self.settings.read().unwrap().is_on(e.category) || self.webhook.read().unwrap().is_none() {
            return;
        }
        let mut q = self.queue.lock().await;
        q.push(e);
        if q.len() > MAX_QUEUE {
            // Descarta primeiro o que é menos importante.
            if let Some(pos) = q.iter().position(|x| matches!(x.category, Category::Chat | Category::Checkpoint | Category::Players)) {
                q.remove(pos);
            } else {
                q.remove(0);
            }
        }
        self.persist(&q);
        self.status.write().unwrap().pending = q.len();
        drop(q);
        self.notify.notify_one();
    }

    fn persist(&self, q: &[Entry]) {
        let file = QueueFile { entries: q.to_vec() };
        if let Err(e) = serde_json::to_vec(&file).map_err(anyhow::Error::from).and_then(|b| util::write_atomic(&self.path, &b)) {
            tracing::warn!(error = %e, "não consegui gravar a fila do diário");
        }
    }

    pub async fn pending(&self) -> Vec<Entry> {
        self.queue.lock().await.clone()
    }

    /// Laço do trabalhador. Roda para sempre (chame com `tokio::spawn`).
    pub async fn run(self: Arc<Self>) {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().expect("cliente http");
        let mut backoff = Duration::from_secs(2);
        loop {
            let snapshot = self.queue.lock().await.clone();
            let webhook = self.webhook.read().unwrap().clone();
            let Some(url) = webhook.filter(|_| !snapshot.is_empty()) else {
                tokio::select! {
                    _ = self.notify.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                }
                continue;
            };
            // Espera a rajada terminar antes de mandar um grupo.
            let first = &snapshot[0];
            if first.group.is_some() {
                let age = (Utc::now() - first.created_at).to_std().unwrap_or_default();
                if age < self.burst_window {
                    tokio::time::sleep(self.burst_window - age).await;
                    continue;
                }
            }
            let window = chrono::Duration::from_std(self.burst_window).unwrap_or(chrono::Duration::seconds(8));
            let (ids, embeds) = build_batch(&snapshot, window);
            let body = serde_json::json!({
                "username": self.world_name.read().unwrap().clone(),
                "allowed_mentions": { "parse": [] },
                "embeds": embeds,
            });
            let res = client.post(format!("{url}?wait=true")).json(&body).send().await;
            match res {
                Ok(r) if r.status().is_success() => {
                    let remaining = header_f64(&r, "x-ratelimit-remaining");
                    let reset_after = header_f64(&r, "x-ratelimit-reset-after");
                    let mut q = self.queue.lock().await;
                    q.retain(|e| !ids.contains(&e.id));
                    self.persist(&q);
                    {
                        let mut st = self.status.write().unwrap();
                        st.pending = q.len();
                        st.last_error = None;
                        st.last_sent_at = Some(Utc::now());
                    }
                    drop(q);
                    backoff = Duration::from_secs(2);
                    let pause = match (remaining, reset_after) {
                        (Some(r), Some(a)) if r < 1.0 => Duration::from_secs_f64(a.clamp(0.0, 60.0)),
                        _ => Duration::from_millis(1100),
                    };
                    tokio::time::sleep(pause).await;
                }
                Ok(r) if r.status().as_u16() == 429 => {
                    let header_wait = header_f64(&r, "retry-after");
                    let body: serde_json::Value = r.json().await.unwrap_or_default();
                    let wait = body["retry_after"].as_f64().or(header_wait).unwrap_or(5.0);
                    tracing::info!(wait, "Discord pediu para esperar (rate limit)");
                    tokio::time::sleep(Duration::from_secs_f64(wait.clamp(0.5, 600.0))).await;
                }
                Ok(r) if r.status().as_u16() == 400 => {
                    // Mensagem malformada: descartar para não travar a fila para sempre.
                    let text = r.text().await.unwrap_or_default();
                    tracing::error!(%text, "Discord recusou um embed; descartando");
                    let mut q = self.queue.lock().await;
                    q.retain(|e| !ids.contains(&e.id));
                    self.persist(&q);
                    self.status.write().unwrap().last_error = Some(format!("embed recusado: {text}"));
                }
                Ok(r) => {
                    let code = r.status().as_u16();
                    let msg = if matches!(code, 401 | 403 | 404) {
                        "o webhook do Discord não existe mais ou foi trocado; cole um novo nas configurações".to_string()
                    } else {
                        format!("Discord respondeu {code}")
                    };
                    tracing::warn!(code, "diário: falha ao enviar");
                    self.status.write().unwrap().last_error = Some(msg);
                    let wait = if matches!(code, 401 | 403 | 404) { Duration::from_secs(600) } else { backoff };
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = self.notify.notified() => {}
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(300));
                }
                Err(e) => {
                    tracing::info!(error = %e, "diário: sem internet; tento depois");
                    self.status.write().unwrap().last_error = Some("sem conexão; as mensagens saem quando a internet voltar".into());
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(300));
                }
            }
        }
    }
}

/// Manda uma mensagem de teste direto (fora da fila), para o assistente conferir o webhook.
pub async fn send_test(url: &str, world: &str) -> Result<(), String> {
    if !is_valid_webhook(url) {
        return Err("isso não parece um webhook do Discord (https://discord.com/api/webhooks/...)".into());
    }
    let e = Entry::new(
        Category::Session,
        "👋 O diário do mundo está ligado",
        format!("A partir de agora, a história do **{}** aparece aqui.", esc(world)),
        color::OPEN,
    );
    let body = serde_json::json!({
        "username": world,
        "allowed_mentions": { "parse": [] },
        "embeds": [render(std::slice::from_ref(&e))],
    });
    let r = reqwest::Client::new()
        .post(format!("{url}?wait=true"))
        .timeout(Duration::from_secs(15))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("sem conexão com o Discord: {e}"))?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("o Discord recusou ({}); confira se o webhook não foi apagado", r.status()))
    }
}

fn header_f64(r: &reqwest::Response, name: &str) -> Option<f64> {
    r.headers().get(name)?.to_str().ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(e: Entry, secs: i64) -> Entry {
        let mut e = e;
        e.created_at = "2026-10-15T23:00:00Z".parse::<DateTime<Utc>>().unwrap() + chrono::Duration::seconds(secs);
        e
    }

    #[test]
    fn groups_bursts() {
        let q = vec![
            at(msg::advancement("Ana", "Stone Age", false), 0),
            at(msg::advancement("Ana", "Getting an Upgrade", false), 2),
            at(msg::advancement("Bia", "Acquire Hardware", false), 5),
            at(msg::death("Ana", "Ana was slain by Zombie"), 6),
            at(msg::advancement("Ana", "Hot Stuff", false), 30),
        ];
        let (ids, embeds) = build_batch(&q, chrono::Duration::seconds(8));
        assert_eq!(embeds.len(), 3, "{embeds:#?}");
        assert_eq!(ids.len(), 5);
        assert_eq!(embeds[0]["title"], "🏆 Conquistas");
        assert_eq!(embeds[0]["description"].as_str().unwrap().lines().count(), 3);
        assert!(embeds[1]["description"].as_str().unwrap().contains("slain"));
        // horário de Brasília (UTC-3): 23:00Z → 20:00
        assert!(embeds[0]["footer"]["text"].as_str().unwrap().starts_with("20:00"));
    }

    #[test]
    fn escapes_markdown_and_formats() {
        let e = msg::joined("Ana_23", "2026-10-15T23:05:00Z".parse().unwrap());
        assert!(e.description.contains("Ana\\_23"));
        assert!(e.description.contains("20:05"));
        let s = msg::world_saved("Ana", 5400, &[("Ana".into(), 5400), ("Bia".into(), 1800)]);
        assert!(s.description.contains("1 h 30 min"));
    }

    #[test]
    fn categories_default() {
        let s = JournalSettings::default();
        assert!(s.is_on(Category::Deaths));
        assert!(!s.is_on(Category::Chat));
    }

    #[tokio::test]
    async fn queue_persists_and_respects_categories() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("q.json");
        let j = Journal::open(p.clone(), JournalSettings::default(), Some("https://discord.com/api/webhooks/1/x".into()), "pAuletOr 3");
        j.post(msg::chat("Ana", "oi")).await; // chat desligado por padrão
        j.post(msg::joined("Ana", Utc::now())).await;
        j.post(msg::death("Ana", "Ana drowned")).await;
        drop(j);
        let j2 = Journal::open(p, JournalSettings::default(), Some("https://discord.com/api/webhooks/1/x".into()), "pAuletOr 3");
        let pending = j2.pending().await;
        assert_eq!(pending.len(), 2, "sobrevive ao fechamento do app");
        assert_eq!(pending[0].category, Category::Players);
    }

    /// Webhook falso: confere ordem, ausência de duplicatas e respeito ao 429.
    #[tokio::test]
    async fn sends_in_order_with_rate_limit() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::<String>::new()));
        let rec = received.clone();
        tokio::spawn(async move {
            let mut n = 0;
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let mut total = 0;
                loop {
                    let k = s.read(&mut buf[total..]).await.unwrap();
                    total += k;
                    let text = String::from_utf8_lossy(&buf[..total]).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len: usize = text
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse().unwrap()))
                            .unwrap_or(0);
                        if total >= h + 4 + len {
                            break;
                        }
                    }
                    if k == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&buf[..total]).to_string();
                n += 1;
                let resp = if n == 1 {
                    "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: 20\r\nconnection: close\r\n\r\n{\"retry_after\": 0.6}".to_string()
                } else {
                    rec.lock().await.push(text.split("\r\n\r\n").nth(1).unwrap_or("").to_string());
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}".to_string()
                };
                s.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(d.path().join("q.json"), JournalSettings::default(), None, "pAuletOr 3");
        // Endereço local para o teste (o validador só aceita discord.com, então forçamos por dentro).
        *j.webhook.write().unwrap() = Some(format!("http://127.0.0.1:{port}/api/webhooks/1/x"));
        j.post(msg::world_opened("Ana", "pauletor3.duckdns.org", Utc::now())).await;
        j.post(msg::first_nether("Ana")).await;
        let worker = tokio::spawn(j.clone().run());
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if j.pending().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("fila esvazia");
        worker.abort();
        let got = received.lock().await.clone();
        let all = got.join("\n");
        let a = all.find("Mundo aberto").unwrap();
        let b = all.find("Nether").unwrap();
        assert!(a < b, "ordem preservada");
        assert_eq!(all.matches("Mundo aberto").count(), 1, "sem duplicar após 429");
    }
}
