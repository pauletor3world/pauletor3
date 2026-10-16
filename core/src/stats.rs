//! Estatísticas: tempo jogado, sessões, quem mais hospedou, dias jogados, resumo diário e marcos.
//! Guardadas no remote (`stats.json`). Só o dono da vez grava, e cada sessão é aplicada
//! uma única vez (`applied_sessions`), então reenviar não conta em dobro.
//! Dias são contados no horário de Brasília.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::journal::day_sp;
use crate::remote::{layout, read_json, write_json, Remote};
use crate::{NmError, NmResult, FORMAT_VERSION};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayerStats {
    pub seconds: i64,
    pub sessions: u32,
    pub last_seen: Option<DateTime<Utc>>,
    pub days: BTreeSet<NaiveDate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DailyStats {
    pub seconds: BTreeMap<String, i64>,
    pub deaths: BTreeMap<String, u32>,
    pub advancements: Vec<(String, String)>,
}

impl DailyStats {
    pub fn is_empty(&self) -> bool {
        self.seconds.is_empty() && self.deaths.is_empty() && self.advancements.is_empty()
    }
}

/// Primeiras vezes do mundo: (quem, quando).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Firsts {
    pub nether: Option<(String, DateTime<Utc>)>,
    pub end: Option<(String, DateTime<Utc>)>,
    pub dragon: Option<(String, DateTime<Utc>)>,
}

/// Marcos de aniversário (rótulo, dias).
pub const ANNIVERSARIES: &[(&str, i64)] = &[
    ("1 semana", 7),
    ("1 mês", 30),
    ("3 meses", 91),
    ("6 meses", 182),
    ("1 ano", 365),
    ("2 anos", 730),
    ("3 anos", 1095),
    ("5 anos", 1826),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stats {
    pub format: u32,
    pub players: BTreeMap<String, PlayerStats>,
    /// Quantas sessões cada jogador hospedou.
    pub hosts: BTreeMap<String, u32>,
    pub host_seconds: BTreeMap<String, i64>,
    pub days: BTreeSet<NaiveDate>,
    pub sessions: u32,
    /// Ids das sessões já contabilizadas (mantém as últimas 500).
    pub applied_sessions: Vec<String>,
    /// Acontecimentos por dia, para o resumo diário. Guarda 60 dias.
    #[serde(default)]
    pub daily: BTreeMap<NaiveDate, DailyStats>,
    #[serde(default)]
    pub daily_posted: BTreeSet<NaiveDate>,
    #[serde(default)]
    pub firsts: Firsts,
    /// Maior tempo seguido de um jogador numa sessão.
    #[serde(default)]
    pub record: Option<(String, i64)>,
    #[serde(default)]
    pub world_born: Option<DateTime<Utc>>,
    #[serde(default)]
    pub anniversaries: BTreeSet<String>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            players: BTreeMap::new(),
            hosts: BTreeMap::new(),
            host_seconds: BTreeMap::new(),
            days: BTreeSet::new(),
            sessions: 0,
            applied_sessions: vec![],
            daily: BTreeMap::new(),
            daily_posted: BTreeSet::new(),
            firsts: Firsts::default(),
            record: None,
            world_born: None,
            anniversaries: BTreeSet::new(),
        }
    }
}

/// Permanência de um jogador: (entrada, saída).
pub type Interval = (DateTime<Utc>, Option<DateTime<Utc>>);

/// Registro de uma sessão (montado a partir do console do servidor).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionLog {
    pub session_id: String,
    pub host: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    /// jogador → intervalos (entrada, saída)
    pub intervals: BTreeMap<String, Vec<Interval>>,
}

impl SessionLog {
    pub fn new(session_id: &str, host: &str, started: DateTime<Utc>) -> Self {
        Self { session_id: session_id.into(), host: host.into(), started_at: Some(started), ..Default::default() }
    }

    pub fn joined(&mut self, name: &str, at: DateTime<Utc>) {
        let v = self.intervals.entry(name.to_string()).or_default();
        if v.last().map(|(_, e)| e.is_none()).unwrap_or(false) {
            return; // já estava dentro
        }
        v.push((at, None));
    }

    /// Registra a saída e devolve quanto tempo durou essa permanência.
    pub fn left(&mut self, name: &str, at: DateTime<Utc>) -> Option<i64> {
        let last = self.intervals.get_mut(name)?.last_mut()?;
        if last.1.is_none() {
            last.1 = Some(at);
            return Some((at - last.0).num_seconds().max(0));
        }
        None
    }

    /// Fecha intervalos abertos (fim da sessão).
    pub fn close(&mut self, at: DateTime<Utc>) {
        self.ended_at = Some(at);
        for v in self.intervals.values_mut() {
            for i in v.iter_mut() {
                if i.1.is_none() {
                    i.1 = Some(at);
                }
            }
        }
    }

    pub fn online(&self) -> Vec<String> {
        self.intervals
            .iter()
            .filter(|(_, v)| v.last().map(|(_, e)| e.is_none()).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn everyone(&self) -> Vec<String> {
        self.intervals.keys().cloned().collect()
    }

    pub fn seconds_per_player(&self) -> HashMap<String, i64> {
        let end = self.ended_at.unwrap_or_else(Utc::now);
        self.intervals
            .iter()
            .map(|(k, v)| {
                let s: i64 = v.iter().map(|(a, b)| (b.unwrap_or(end) - *a).num_seconds().max(0)).sum();
                (k.clone(), s)
            })
            .collect()
    }

    /// (jogador, segundos) do mais ao menos jogado.
    pub fn ranking(&self) -> Vec<(String, i64)> {
        let mut v: Vec<_> = self.seconds_per_player().into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

impl Stats {
    /// Aplica uma sessão (idempotente).
    pub fn apply(&mut self, log: &SessionLog) {
        if self.applied_sessions.contains(&log.session_id) {
            return;
        }
        let end = log.ended_at.unwrap_or_else(Utc::now);
        let mut secs: Vec<_> = log.seconds_per_player().into_iter().collect();
        secs.sort();
        for (name, s) in secs {
            let p = self.players.entry(name.clone()).or_default();
            p.seconds += s;
            p.sessions += 1;
            if let Some(iv) = log.intervals.get(&name) {
                for (a, b) in iv {
                    let last = b.unwrap_or(end);
                    let day = day_sp(*a);
                    p.days.insert(day);
                    p.last_seen = Some(p.last_seen.map_or(last, |x| x.max(last)));
                    self.days.insert(day);
                    *self.daily.entry(day).or_default().seconds.entry(name.clone()).or_default() +=
                        (last - *a).num_seconds().max(0);
                }
            }
        }
        if let (Some(s), Some(e)) = (log.started_at, log.ended_at) {
            *self.host_seconds.entry(log.host.clone()).or_default() += (e - s).num_seconds().max(0);
        }
        *self.hosts.entry(log.host.clone()).or_default() += 1;
        self.sessions += 1;
        self.prune();
        self.applied_sessions.push(log.session_id.clone());
        let n = self.applied_sessions.len();
        if n > 500 {
            self.applied_sessions.drain(0..n - 500);
        }
    }

    fn prune(&mut self) {
        while self.daily.len() > 60 {
            let k = *self.daily.keys().next().unwrap();
            self.daily.remove(&k);
        }
        while self.daily_posted.len() > 120 {
            let k = *self.daily_posted.iter().next().unwrap();
            self.daily_posted.remove(&k);
        }
    }

    pub fn on_death(&mut self, player: &str, at: DateTime<Utc>) {
        *self.daily.entry(day_sp(at)).or_default().deaths.entry(player.to_string()).or_default() += 1;
    }

    pub fn on_advancement(&mut self, player: &str, title: &str, at: DateTime<Utc>) {
        self.daily.entry(day_sp(at)).or_default().advancements.push((player.to_string(), title.to_string()));
    }

    /// Registra uma primeira vez do mundo (`nether`, `end`, `dragon`). `true` se foi mesmo a primeira.
    pub fn first(&mut self, which: &str, player: &str, at: DateTime<Utc>) -> bool {
        let slot = match which {
            "nether" => &mut self.firsts.nether,
            "end" => &mut self.firsts.end,
            "dragon" => &mut self.firsts.dragon,
            _ => return false,
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some((player.to_string(), at));
        true
    }

    /// Confere recorde de tempo seguido. Devolve o recorde anterior se foi batido.
    /// Só vale a partir de 30 minutos, para não virar spam no começo do mundo.
    pub fn check_record(&mut self, player: &str, secs: i64) -> Option<i64> {
        let prev = self.record.as_ref().map(|r| r.1).unwrap_or(0);
        if secs > prev && secs >= 1800 {
            self.record = Some((player.to_string(), secs));
            Some(prev)
        } else {
            None
        }
    }

    /// Aniversário que acabou de chegar (só o mais recente, se vários venceram juntos).
    pub fn due_anniversaries(&mut self, now: DateTime<Utc>) -> Vec<&'static str> {
        let Some(born) = self.world_born else { return vec![] };
        let days = (now - born).num_days();
        let mut due = vec![];
        for (label, d) in ANNIVERSARIES {
            if days >= *d && !self.anniversaries.contains(*label) {
                self.anniversaries.insert(label.to_string());
                due.push(*label);
            }
        }
        due.into_iter().last().into_iter().collect()
    }

    /// Dias encerrados, com atividade, cujo resumo ainda não saiu (até 7 dias de atraso).
    pub fn due_daily(&mut self, now: DateTime<Utc>) -> Vec<(NaiveDate, DailyStats)> {
        let today = day_sp(now);
        let mut out = vec![];
        for (day, d) in &self.daily {
            if *day < today && !self.daily_posted.contains(day) && !d.is_empty() {
                out.push((*day, d.clone()));
            }
        }
        for (day, _) in &out {
            self.daily_posted.insert(*day);
        }
        out.retain(|(d, _)| (today - *d).num_days() <= 7);
        out
    }

    pub async fn load(remote: &dyn Remote) -> NmResult<Self> {
        let s: Option<Stats> = read_json(remote, layout::STATS).await?;
        let s = s.unwrap_or_default();
        if s.format > FORMAT_VERSION {
            return Err(NmError::FormatTooNew { found: s.format });
        }
        Ok(s)
    }

    /// Grava (só com a vez em mãos).
    pub async fn save(&self, remote: &dyn Remote) -> NmResult<()> {
        write_json(remote, layout::STATS, self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_log_and_idempotent_apply() {
        let t0: DateTime<Utc> = "2026-10-10T20:00:00Z".parse().unwrap();
        let mut log = SessionLog::new("s1", "Ana", t0);
        log.joined("Ana", t0);
        log.joined("Bia", t0 + chrono::Duration::minutes(10));
        log.joined("Bia", t0 + chrono::Duration::minutes(11)); // duplicado ignorado
        assert_eq!(log.left("Bia", t0 + chrono::Duration::minutes(40)), Some(1800));
        assert_eq!(log.left("Bia", t0 + chrono::Duration::minutes(41)), None);
        assert_eq!(log.online(), vec!["Ana"]);
        log.close(t0 + chrono::Duration::minutes(60));
        assert_eq!(log.ranking(), vec![("Ana".to_string(), 3600), ("Bia".to_string(), 1800)]);

        let mut st = Stats::default();
        st.apply(&log);
        st.apply(&log);
        assert_eq!(st.sessions, 1);
        assert_eq!(st.players["Ana"].seconds, 3600);
        assert_eq!(st.hosts["Ana"], 1);
        assert_eq!(st.host_seconds["Ana"], 3600);
        assert_eq!(st.days.len(), 1);
        assert_eq!(st.daily[&day_sp(t0)].seconds["Bia"], 1800);
    }

    #[test]
    fn milestones_and_daily() {
        let t0: DateTime<Utc> = "2026-10-10T20:00:00Z".parse().unwrap();
        let mut st = Stats::default();
        assert!(st.first("nether", "Ana", t0));
        assert!(!st.first("nether", "Bia", t0));
        assert_eq!(st.check_record("Ana", 600), None, "curto demais");
        assert_eq!(st.check_record("Ana", 3600), Some(0));
        assert_eq!(st.check_record("Bia", 3000), None);
        st.world_born = Some(t0);
        assert!(st.due_anniversaries(t0 + chrono::Duration::days(3)).is_empty());
        assert_eq!(st.due_anniversaries(t0 + chrono::Duration::days(40)), vec!["1 mês"]);
        assert!(st.due_anniversaries(t0 + chrono::Duration::days(41)).is_empty());
        st.on_death("Ana", t0);
        st.on_advancement("Ana", "Stone Age", t0);
        assert!(st.due_daily(t0).is_empty(), "o dia ainda não acabou");
        let due = st.due_daily(t0 + chrono::Duration::days(1));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1.deaths["Ana"], 1);
        assert!(st.due_daily(t0 + chrono::Duration::days(1)).is_empty(), "uma vez só");
    }
}
