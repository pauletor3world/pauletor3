//! Retenção de versões e coleta de lixo (GC).
//!
//! Mantém:
//! - o manifesto do HEAD (sempre);
//! - o manifesto final (o mais recente) de cada uma das últimas 20 sessões;
//! - o manifesto mais recente de cada dia, nos últimos 30 dias.
//!
//! Se o que sobra passar do orçamento de espaço (a nuvem grátis tem limite), as versões mais
//! antigas saem primeiro, até caber; o HEAD fica sempre.
//!
//! O GC apaga os manifestos fora dessa lista e depois os objetos que nenhum manifesto mantido
//! referencia. Só roda com a vez (de quem hospeda ou de manutenção).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use serde::Serialize;

use crate::lease::LeaseGuard;
use crate::manifest::ManifestSummary;
use crate::remote::layout;
use crate::sync::SyncEngine;
use crate::{NmError, NmResult};

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub last_sessions: usize,
    pub daily_days: i64,
    /// Teto do que fica na nuvem (bytes). Acima disso, as versões mais antigas saem.
    pub budget_bytes: u64,
}

impl Default for Policy {
    fn default() -> Self {
        // a coordenação aceita até 4 GB; deixa folga para a próxima sessão subir
        Self { last_sessions: 20, daily_days: 30, budget_bytes: 2_600_000_000 }
    }
}

impl Policy {
    /// Nuvem cheia no meio de um envio: libera bem mais, para o salvamento caber.
    pub fn emergency() -> Self {
        Self { last_sessions: 3, daily_days: 1, budget_bytes: 1_800_000_000 }
    }
}

/// Decide quais manifestos manter. Função pura (testável).
pub fn select_keep(
    manifests: &[ManifestSummary],
    head: Option<&str>,
    policy: Policy,
    now: DateTime<Utc>,
) -> BTreeSet<String> {
    let mut keep = BTreeSet::new();
    if let Some(h) = head {
        keep.insert(h.to_string());
    }

    // Último manifesto de cada sessão, das sessões mais recentes.
    let mut by_session: BTreeMap<&str, &ManifestSummary> = BTreeMap::new();
    for m in manifests {
        let e = by_session.entry(m.session_id.as_str()).or_insert(m);
        if (m.created_at, &m.id) > (e.created_at, &e.id) {
            *e = m;
        }
    }
    let mut finals: Vec<&ManifestSummary> = by_session.into_values().collect();
    finals.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    for m in finals.into_iter().take(policy.last_sessions) {
        keep.insert(m.id.clone());
    }

    // Último manifesto de cada dia (no fuso local), nos últimos N dias.
    let cutoff: NaiveDate = (now.with_timezone(&Local) - Duration::days(policy.daily_days)).date_naive();
    let mut by_day: BTreeMap<NaiveDate, &ManifestSummary> = BTreeMap::new();
    for m in manifests {
        let day = m.created_at.with_timezone(&Local).date_naive();
        if day < cutoff {
            continue;
        }
        let e = by_day.entry(day).or_insert(m);
        if (m.created_at, &m.id) > (e.created_at, &e.id) {
            *e = m;
        }
    }
    for m in by_day.into_values() {
        keep.insert(m.id.clone());
    }
    keep
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct GcReport {
    pub manifests_kept: usize,
    pub manifests_deleted: usize,
    pub objects_kept: usize,
    pub objects_deleted: usize,
    pub bytes_freed: u64,
}

/// Roda o GC. Exige a vez (modo manutenção).
pub async fn gc(engine: &SyncEngine, guard: &LeaseGuard, policy: Policy, dry_run: bool) -> NmResult<GcReport> {
    guard.verify().await?;
    let head = engine.head().await?;
    let ids = engine.list_manifest_ids().await?;
    let mut summaries = Vec::new();
    let mut unreadable = Vec::new();
    for id in &ids {
        match engine.fetch_manifest(id).await {
            Ok(m) => summaries.push(m.summary()),
            Err(e) => {
                tracing::warn!(%id, error = %e, "manifesto ilegível; o GC não vai apagar nada");
                unreadable.push(id.clone());
            }
        }
    }
    if !unreadable.is_empty() {
        // Sem saber o que um manifesto referencia, apagar objetos seria arriscado.
        return Err(NmError::Integrity(format!(
            "{} manifesto(s) ilegível(is); coleta de lixo cancelada por segurança",
            unreadable.len()
        )));
    }
    let keep = select_keep(&summaries, head.as_ref().map(|h| h.manifest.as_str()), policy, Utc::now());
    if let Some(h) = &head {
        if !keep.contains(&h.manifest) || !ids.contains(&h.manifest) {
            return Err(NmError::Integrity("o HEAD aponta para um manifesto que não existe".into()));
        }
    }

    let objects = engine.remote_objects().await?;
    let mut refs: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for id in &keep {
        refs.insert(id.clone(), engine.fetch_manifest(id).await?.object_hashes().into_iter().collect());
    }
    // Orçamento: tira a versão mantida mais antiga (nunca o HEAD) até caber.
    let head_id = head.as_ref().map(|h| h.manifest.clone());
    let mut keep = keep;
    let size_of = |refs: &BTreeMap<String, HashSet<String>>| -> u64 {
        let all: HashSet<&String> = refs.values().flatten().collect();
        all.into_iter().map(|h| objects.get(h).copied().unwrap_or(0)).sum()
    };
    while size_of(&refs) > policy.budget_bytes {
        let oldest = summaries
            .iter()
            .filter(|m| keep.contains(&m.id) && Some(&m.id) != head_id.as_ref())
            .min_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)))
            .map(|m| m.id.clone());
        let Some(o) = oldest else { break };
        keep.remove(&o);
        refs.remove(&o);
    }
    let live: HashSet<String> = refs.into_values().flatten().collect();
    let dead: Vec<(String, u64)> = objects
        .iter()
        .filter(|(h, _)| !live.contains(*h))
        .map(|(h, s)| (h.clone(), *s))
        .collect();
    let dead_manifests: Vec<String> = ids.iter().filter(|i| !keep.contains(*i)).cloned().collect();

    let report = GcReport {
        manifests_kept: keep.len(),
        manifests_deleted: dead_manifests.len(),
        objects_kept: objects.len() - dead.len(),
        objects_deleted: dead.len(),
        bytes_freed: dead.iter().map(|d| d.1).sum(),
    };
    if dry_run {
        return Ok(report);
    }
    // Ordem: manifestos primeiro, objetos depois. Se cair no meio, sobra lixo, nunca falta objeto.
    let names: Vec<String> = dead_manifests.iter().map(|i| format!("{i}.json")).collect();
    engine.remote.delete_many(layout::MANIFESTS, &names).await?;
    guard.verify().await?;
    let names: Vec<String> = dead.into_iter().map(|d| d.0).collect();
    engine.remote.delete_many(layout::OBJECTS, &names).await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestKind;

    fn s(id: &str, session: &str, at: DateTime<Utc>) -> ManifestSummary {
        ManifestSummary {
            id: id.into(),
            kind: ManifestKind::Checkpoint,
            created_at: at,
            session_id: session.into(),
            host_player: "a".into(),
            duration_secs: 0,
            players: vec![],
            total_size: 0,
            file_count: 0,
            restored_from: None,
            note: None,
        }
    }

    #[test]
    fn keeps_final_of_last_sessions_and_daily() {
        let now: DateTime<Utc> = "2026-06-30T12:00:00Z".parse().unwrap();
        let mut ms = vec![];
        // 25 sessões antigas (60+ dias atrás), 3 checkpoints cada
        for i in 0..25 {
            let base = now - Duration::days(100) + Duration::hours(i * 2);
            for c in 0..3 {
                ms.push(s(&format!("old-{i:02}-{c}"), &format!("old{i:02}"), base + Duration::minutes(c * 15)));
            }
        }
        let policy = Policy { last_sessions: 20, daily_days: 30, budget_bytes: u64::MAX };
        let keep = select_keep(&ms, Some("old-00-0"), policy, now);
        // HEAD + finais das 20 sessões mais recentes (old05..old24)
        assert!(keep.contains("old-00-0"));
        assert!(keep.contains("old-24-2"));
        assert!(keep.contains("old-05-2"));
        assert!(!keep.contains("old-04-2"));
        assert!(!keep.contains("old-24-1"), "checkpoints intermediários saem");
        assert_eq!(keep.len(), 21);

        // Um por dia nos últimos 30 dias
        let mut recent = ms.clone();
        for d in 0..40 {
            for c in 0..2 {
                let at = now - Duration::days(d) + Duration::minutes(c);
                recent.push(s(&format!("d{d:02}-{c}"), &format!("day{d:02}x{c}"), at));
            }
        }
        let keep = select_keep(&recent, None, Policy { last_sessions: 0, daily_days: 30, budget_bytes: u64::MAX }, now);
        assert!(keep.contains("d00-1"));
        assert!(!keep.contains("d00-0"));
        assert!(keep.contains("d29-1"));
        assert!(!keep.contains("d35-1"));
    }
}
