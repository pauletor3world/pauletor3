//! As regras da coordenação, como funções puras. O Durable Object (`coordinator/src/rules.ts`)
//! implementa exatamente as mesmas regras; os testes dos dois lados usam os mesmos casos.

use chrono::{DateTime, Duration, Utc};

use super::{LeaseInfo, Presence};

/// Sem heartbeat (nem aviso de membro) por este tempo, a vez expira.
pub const LEASE_TTL_SECS: i64 = 45;
/// Frequência do heartbeat do host.
pub const HEARTBEAT_SECS: u64 = 10;
/// Depois que a vez expira, durante este prazo só o membro com a réplica mais nova
/// (entre os que estavam jogando) pode assumir.
pub const FAILOVER_PREFERENCE_SECS: i64 = 20;
/// Presença mais velha que isto não conta (o app manda a cada 5–30 s).
pub const PRESENCE_FRESH_SECS: i64 = 60;

pub fn ttl() -> Duration {
    Duration::seconds(LEASE_TTL_SECS)
}

#[derive(Debug, Clone, PartialEq)]
pub enum AcquireDecision {
    /// Pode pegar. `previous` = vez anterior que expirou sem ser liberada (o host caiu).
    Grant { previous: Option<LeaseInfo> },
    Held(LeaseInfo),
    /// Outro membro tem réplica mais nova; tente de novo em alguns segundos.
    NotBest { best_seq: u64 },
}

/// Presença de um membro, com o momento em que chegou.
#[derive(Debug, Clone)]
pub struct SeenPresence<'a> {
    pub member: &'a str,
    pub presence: &'a Presence,
    pub at: DateTime<Utc>,
}

pub fn is_expired(lease: &LeaseInfo, now: DateTime<Utc>) -> bool {
    now >= lease.expires_at(ttl())
}

pub fn decide_acquire(
    now: DateTime<Utc>,
    lease: Option<&LeaseInfo>,
    requester: &str,
    replica_seq: u64,
    presences: &[SeenPresence<'_>],
) -> AcquireDecision {
    let Some(l) = lease else {
        return AcquireDecision::Grant { previous: None };
    };
    if l.holder == requester {
        // O próprio PC (app reiniciado depois de uma queda): pode retomar.
        return AcquireDecision::Grant { previous: Some(l.clone()) };
    }
    if !is_expired(l, now) {
        return AcquireDecision::Held(l.clone());
    }
    let expired_at = l.expires_at(ttl());
    if now < expired_at + Duration::seconds(FAILOVER_PREFERENCE_SECS) {
        let best = presences
            .iter()
            .filter(|p| p.member != l.holder && p.presence.playing)
            .filter(|p| now - p.at <= Duration::seconds(PRESENCE_FRESH_SECS))
            .map(|p| p.presence.replica_seq)
            .max()
            .unwrap_or(0);
        if replica_seq < best {
            return AcquireDecision::NotBest { best_seq: best };
        }
    }
    AcquireDecision::Grant { previous: Some(l.clone()) }
}

/// Estado da vez visto de fora (expirada = livre, com a anterior para explicar).
pub fn status(now: DateTime<Utc>, lease: Option<&LeaseInfo>) -> super::LeaseStatus {
    match lease {
        None => super::LeaseStatus::Free { previous: None },
        Some(l) if is_expired(l, now) => super::LeaseStatus::Free { previous: Some(l.clone()) },
        Some(l) => super::LeaseStatus::Held { lease: l.clone() },
    }
}

/// Um nick de Minecraft válido (3–16, letras, números e `_`).
pub fn valid_nick(n: &str) -> bool {
    (3..=16).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coord::LeasePurpose;

    fn lease(holder: &str, hb: DateTime<Utc>) -> LeaseInfo {
        LeaseInfo {
            format: 1,
            session_number: 7,
            session_id: "s".into(),
            holder: holder.into(),
            player: "Ana".into(),
            machine: "pc".into(),
            endpoint: None,
            app_version: "x".into(),
            purpose: LeasePurpose::Hosting,
            acquired_at: hb,
            heartbeat_at: hb,
            vouched_at: None,
            handing_over: false,
            online: vec![],
        }
    }

    fn pres(seq: u64, playing: bool) -> Presence {
        Presence { replica_seq: seq, playing, ..Default::default() }
    }

    #[test]
    fn free_and_held() {
        let now = Utc::now();
        assert_eq!(decide_acquire(now, None, "b", 0, &[]), AcquireDecision::Grant { previous: None });
        let l = lease("a", now - Duration::seconds(10));
        assert!(matches!(decide_acquire(now, Some(&l), "b", 0, &[]), AcquireDecision::Held(_)));
        // o próprio dono pode retomar
        assert!(matches!(decide_acquire(now, Some(&l), "a", 0, &[]), AcquireDecision::Grant { .. }));
    }

    #[test]
    fn vouch_keeps_lease_alive() {
        let now = Utc::now();
        let mut l = lease("a", now - Duration::seconds(120));
        assert!(is_expired(&l, now));
        l.vouched_at = Some(now - Duration::seconds(5));
        assert!(!is_expired(&l, now));
        assert!(matches!(decide_acquire(now, Some(&l), "b", 0, &[]), AcquireDecision::Held(_)));
    }

    #[test]
    fn failover_prefers_newest_replica_then_opens() {
        let now = Utc::now();
        // expirou há 5 s
        let l = lease("a", now - Duration::seconds(LEASE_TTL_SECS + 5));
        let p_new = pres(10, true);
        let p_old = pres(8, true);
        let seen = vec![
            SeenPresence { member: "b", presence: &p_new, at: now },
            SeenPresence { member: "c", presence: &p_old, at: now },
        ];
        assert_eq!(decide_acquire(now, Some(&l), "c", 8, &seen), AcquireDecision::NotBest { best_seq: 10 });
        assert!(matches!(decide_acquire(now, Some(&l), "b", 10, &seen), AcquireDecision::Grant { previous: Some(_) }));
        // passado o prazo de preferência, qualquer um
        let later = now + Duration::seconds(FAILOVER_PREFERENCE_SECS + 1);
        assert!(matches!(decide_acquire(later, Some(&l), "c", 8, &seen), AcquireDecision::Grant { .. }));
    }

    #[test]
    fn stale_presence_and_dead_host_do_not_count() {
        let now = Utc::now();
        let l = lease("a", now - Duration::seconds(LEASE_TTL_SECS + 1));
        let host = pres(99, true);
        let stale = pres(50, true);
        let idle = pres(40, false);
        let seen = vec![
            SeenPresence { member: "a", presence: &host, at: now },
            SeenPresence { member: "b", presence: &stale, at: now - Duration::seconds(PRESENCE_FRESH_SECS + 1) },
            SeenPresence { member: "d", presence: &idle, at: now },
        ];
        assert!(matches!(decide_acquire(now, Some(&l), "c", 1, &seen), AcquireDecision::Grant { .. }));
    }

    #[test]
    fn nicks() {
        assert!(valid_nick("Ana_23"));
        assert!(!valid_nick("Zé"));
        assert!(!valid_nick("ab"));
    }
}
