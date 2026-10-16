//! Coordenação em memória: a implementação de referência das regras, usada nos testes com
//! rede simulada (vários "PCs" no mesmo processo, cada um com a sua identidade). Permite
//! simular a coordenação fora do ar e controlar o relógio.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

use super::rules::{self, AcquireDecision, SeenPresence};
use super::*;
use crate::{NmError, FORMAT_VERSION};

#[derive(Debug, Default)]
struct MemWorld {
    name: String,
    lease: Option<LeaseInfo>,
    session_counter: u64,
    head: Option<Head>,
    members: BTreeMap<String, Member>,
    presence: BTreeMap<String, (Presence, DateTime<Utc>)>,
    invites: BTreeMap<String, (String, Invite)>, // token -> (id, invite)
    events: Vec<DiaryEvent>,
    private: PrivateSettings,
}

/// Relógio controlável (os testes avançam o tempo sem dormir).
#[derive(Clone, Default)]
pub struct TestClock(Arc<Mutex<Duration>>);

impl TestClock {
    pub fn now(&self) -> DateTime<Utc> {
        Utc::now() + *self.0.lock().unwrap()
    }
    pub fn advance(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
}

/// O "servidor" compartilhado. Cada PC simulado pega um cliente com [`MemHub::client`].
#[derive(Clone, Default)]
pub struct MemHub {
    world: Arc<Mutex<MemWorld>>,
    pub clock: TestClock,
    /// Coordenação fora do ar para todo mundo.
    pub down: Arc<AtomicBool>,
}

impl MemHub {
    pub fn new(name: &str, owner_id: &str, owner_nick: &str) -> Self {
        let hub = Self::default();
        {
            let mut w = hub.world.lock().unwrap();
            w.name = name.into();
            w.members.insert(
                owner_id.into(),
                Member {
                    id: owner_id.into(),
                    nick: owner_nick.into(),
                    role: Role::Owner,
                    approved: true,
                    joined_at: Utc::now(),
                    last_seen: None,
                    presence: None,
                },
            );
        }
        hub
    }

    pub fn client(&self, me: &str) -> MemCoordinator {
        MemCoordinator { hub: self.clone(), me: me.into(), offline: Arc::new(AtomicBool::new(false)) }
    }

    /// Adiciona um membro aprovado direto (atalho de teste; o caminho real é o convite).
    pub fn add_member(&self, id: &str, nick: &str) {
        self.world.lock().unwrap().members.insert(
            id.into(),
            Member { id: id.into(), nick: nick.into(), role: Role::Member, approved: true, joined_at: Utc::now(), last_seen: None, presence: None },
        );
    }

    /// Entra por convite (o que o Worker faz em `POST /v1/invites/:token/redeem`).
    pub fn redeem(&self, token: &str, id: &str, nick: &str) -> NmResult<()> {
        let now = self.clock.now();
        let mut w = self.world.lock().unwrap();
        let (_, inv) = w.invites.get_mut(token).ok_or_else(|| NmError::Forbidden("convite inválido".into()))?;
        if inv.expires_at < now || inv.uses_left == 0 {
            return Err(NmError::Forbidden("convite expirado".into()));
        }
        inv.uses_left -= 1;
        let approved = inv.preapproved;
        w.members.entry(id.into()).or_insert(Member {
            id: id.into(),
            nick: nick.into(),
            role: Role::Member,
            approved,
            joined_at: now,
            last_seen: None,
            presence: None,
        });
        Ok(())
    }
}

#[derive(Clone)]
pub struct MemCoordinator {
    hub: MemHub,
    me: String,
    /// Só este PC sem acesso à coordenação (partição).
    pub offline: Arc<AtomicBool>,
}

impl MemCoordinator {
    fn guard(&self) -> NmResult<std::sync::MutexGuard<'_, MemWorld>> {
        if self.hub.down.load(Ordering::SeqCst) || self.offline.load(Ordering::SeqCst) {
            return Err(NmError::Remote("coordenação fora do ar (simulado)".into()));
        }
        Ok(self.hub.world.lock().unwrap())
    }

    fn member<'a>(&self, w: &'a MemWorld) -> NmResult<&'a Member> {
        match w.members.get(&self.me) {
            Some(m) if m.approved => Ok(m),
            Some(_) => Err(NmError::Forbidden("aguardando aprovação do dono".into())),
            None => Err(NmError::Forbidden("este PC não é membro do mundo".into())),
        }
    }

    fn owner(&self, w: &MemWorld) -> NmResult<()> {
        if self.member(w)?.role != Role::Owner {
            return Err(NmError::Forbidden("só o dono pode fazer isso".into()));
        }
        Ok(())
    }

    pub fn set_offline(&self, v: bool) {
        self.offline.store(v, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl Coordinator for MemCoordinator {
    fn me(&self) -> String {
        self.me.clone()
    }

    async fn view(&self) -> NmResult<WorldView> {
        let now = self.hub.clock.now();
        let w = self.guard()?;
        let me = w.members.get(&self.me).cloned();
        let members = w
            .members
            .values()
            .map(|m| {
                let mut m = m.clone();
                m.presence = w
                    .presence
                    .get(&m.id)
                    .filter(|(_, at)| now - *at <= Duration::seconds(rules::PRESENCE_FRESH_SECS))
                    .map(|(p, _)| p.clone());
                m.last_seen = w.presence.get(&m.id).map(|(_, at)| *at);
                m
            })
            .collect();
        Ok(WorldView {
            world_id: "mem".into(),
            name: w.name.clone(),
            lease: rules::status(now, w.lease.as_ref()),
            head: w.head.clone(),
            members,
            now,
            me,
        })
    }

    async fn acquire(&self, req: AcquireRequest) -> NmResult<LeaseInfo> {
        let now = self.hub.clock.now();
        let mut w = self.guard()?;
        self.member(&w)?;
        let seen: Vec<SeenPresence> = w
            .presence
            .iter()
            .map(|(id, (p, at))| SeenPresence { member: id, presence: p, at: *at })
            .collect();
        match rules::decide_acquire(now, w.lease.as_ref(), &self.me, req.replica_seq, &seen) {
            AcquireDecision::Held(l) => Err(NmError::HeldBy(Box::new(l))),
            AcquireDecision::NotBest { best_seq } => Err(NmError::NotBestCandidate { best_seq }),
            AcquireDecision::Grant { .. } => {
                w.session_counter += 1;
                let l = LeaseInfo {
                    format: FORMAT_VERSION,
                    session_number: w.session_counter,
                    session_id: req.session_id,
                    holder: self.me.clone(),
                    player: req.player,
                    machine: req.machine,
                    endpoint: req.endpoint,
                    app_version: req.app_version,
                    purpose: req.purpose,
                    acquired_at: now,
                    heartbeat_at: now,
                    vouched_at: None,
                    handing_over: false,
                    online: vec![],
                };
                w.lease = Some(l.clone());
                Ok(l)
            }
        }
    }

    async fn heartbeat(&self, session_number: u64, online: &[String], handing_over: bool) -> NmResult<LeaseInfo> {
        let now = self.hub.clock.now();
        let mut w = self.guard()?;
        match w.lease.as_mut() {
            Some(l) if l.holder == self.me && l.session_number == session_number => {
                l.heartbeat_at = now;
                l.online = online.to_vec();
                l.handing_over = handing_over;
                Ok(l.clone())
            }
            _ => Err(NmError::LostLock),
        }
    }

    async fn release(&self, session_number: u64) -> NmResult<()> {
        let mut w = self.guard()?;
        if matches!(&w.lease, Some(l) if l.holder == self.me && l.session_number == session_number) {
            w.lease = None;
        }
        Ok(())
    }

    async fn head(&self) -> NmResult<Option<Head>> {
        let w = self.guard()?;
        self.member(&w)?;
        Ok(w.head.clone())
    }

    async fn publish_head(&self, session_number: u64, expected: Option<&str>, manifest: &str) -> NmResult<Head> {
        let now = self.hub.clock.now();
        let mut w = self.guard()?;
        match &w.lease {
            Some(l) if l.holder == self.me && l.session_number == session_number => {}
            _ => return Err(NmError::LostLock),
        }
        let current = w.head.as_ref().map(|h| h.manifest.as_str());
        if current != expected {
            return Err(NmError::HeadMoved { current: current.map(str::to_string) });
        }
        let h = Head {
            format: FORMAT_VERSION,
            manifest: manifest.into(),
            seq: w.head.as_ref().map(|h| h.seq + 1).unwrap_or(1),
            session_number,
            updated_at: now,
            updated_by: self.me.clone(),
        };
        w.head = Some(h.clone());
        Ok(h)
    }

    async fn presence(&self, p: &Presence) -> NmResult<WorldView> {
        let now = self.hub.clock.now();
        {
            let mut w = self.guard()?;
            self.member(&w)?;
            if let (Some(sn), Some(l)) = (p.host_reachable, w.lease.as_mut()) {
                if l.session_number == sn && l.holder != self.me {
                    l.vouched_at = Some(now);
                }
            }
            w.presence.insert(self.me.clone(), (p.clone(), now));
        }
        self.view().await
    }

    async fn post_events(&self, events: &[DiaryEvent]) -> NmResult<()> {
        let mut w = self.guard()?;
        self.member(&w)?;
        w.events.extend_from_slice(events);
        Ok(())
    }

    async fn events(&self, limit: usize) -> NmResult<Vec<DiaryEvent>> {
        let w = self.guard()?;
        self.member(&w)?;
        Ok(w.events.iter().rev().take(limit).cloned().collect())
    }

    async fn create_invite(&self, opts: &InviteOptions) -> NmResult<Invite> {
        let now = self.hub.clock.now();
        let mut w = self.guard()?;
        self.owner(&w)?;
        let token = crate::util::random_token(24);
        let inv = Invite {
            token: Some(token.clone()),
            id: crate::util::random_token(8),
            created_at: now,
            expires_at: now + Duration::hours(opts.hours as i64),
            uses_left: opts.uses,
            preapproved: opts.preapproved,
        };
        let mut stored = inv.clone();
        stored.token = None;
        w.invites.insert(token, (inv.id.clone(), stored));
        Ok(inv)
    }

    async fn invites(&self) -> NmResult<Vec<Invite>> {
        let w = self.guard()?;
        self.owner(&w)?;
        Ok(w.invites.values().map(|(_, i)| i.clone()).collect())
    }

    async fn revoke_invite(&self, id: &str) -> NmResult<()> {
        let mut w = self.guard()?;
        self.owner(&w)?;
        w.invites.retain(|_, (iid, _)| iid != id);
        Ok(())
    }

    async fn set_member(&self, id: &str, approved: Option<bool>, remove: bool) -> NmResult<()> {
        let mut w = self.guard()?;
        self.owner(&w)?;
        if id == self.me {
            return Err(NmError::Forbidden("o dono não pode se remover".into()));
        }
        if remove {
            w.members.remove(id);
            w.presence.remove(id);
        } else if let (Some(a), Some(m)) = (approved, w.members.get_mut(id)) {
            m.approved = a;
        }
        Ok(())
    }

    async fn private_settings(&self) -> NmResult<PrivateSettings> {
        let w = self.guard()?;
        self.member(&w)?;
        Ok(w.private.clone())
    }

    async fn set_private_settings(&self, s: &PrivateSettings) -> NmResult<()> {
        let mut w = self.guard()?;
        self.owner(&w)?;
        w.private = s.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str, seq: u64) -> AcquireRequest {
        AcquireRequest {
            session_id: id.into(),
            player: id.into(),
            machine: "pc".into(),
            endpoint: None,
            purpose: LeasePurpose::Hosting,
            replica_seq: seq,
            app_version: "t".into(),
        }
    }

    #[tokio::test]
    async fn lease_and_head_cas() {
        let hub = MemHub::new("Mundo", "a", "Ana");
        hub.add_member("b", "Bia");
        let a = hub.client("a");
        let b = hub.client("b");
        let la = a.acquire(req("s1", 0)).await.unwrap();
        assert!(matches!(b.acquire(req("s2", 0)).await, Err(NmError::HeldBy(_))));
        // só quem tem a vez publica, e só a partir do HEAD esperado
        assert!(matches!(b.publish_head(la.session_number, None, "m1").await, Err(NmError::LostLock)));
        let h = a.publish_head(la.session_number, None, "m1").await.unwrap();
        assert_eq!(h.seq, 1);
        assert!(matches!(a.publish_head(la.session_number, None, "m2").await, Err(NmError::HeadMoved { .. })));
        a.release(la.session_number).await.unwrap();
        let lb = b.acquire(req("s2", 1)).await.unwrap();
        assert!(lb.session_number > la.session_number, "session_number monotônico");
        // a vez velha não publica mais
        assert!(matches!(a.publish_head(la.session_number, Some("m1"), "m3").await, Err(NmError::LostLock)));
    }

    #[tokio::test]
    async fn expiry_failover_and_lost_lock() {
        let hub = MemHub::new("Mundo", "a", "Ana");
        hub.add_member("b", "Bia");
        let a = hub.client("a");
        let b = hub.client("b");
        let la = a.acquire(req("s1", 0)).await.unwrap();
        hub.clock.advance(Duration::seconds(rules::LEASE_TTL_SECS + 1));
        assert!(matches!(b.view().await.unwrap().lease, LeaseStatus::Free { previous: Some(_) }));
        let lb = b.acquire(req("s2", 0)).await.unwrap();
        assert!(matches!(a.heartbeat(la.session_number, &[], false).await, Err(NmError::LostLock)));
        b.heartbeat(lb.session_number, &["Bia".into()], false).await.unwrap();
    }

    #[tokio::test]
    async fn invites_and_permissions() {
        let hub = MemHub::new("Mundo", "a", "Ana");
        let a = hub.client("a");
        let c = hub.client("c");
        assert!(matches!(c.view().await.unwrap().me, None));
        assert!(matches!(c.head().await, Err(NmError::Forbidden(_))));
        let inv = a.create_invite(&InviteOptions { hours: 1, uses: 1, preapproved: false }).await.unwrap();
        let token = inv.token.unwrap();
        hub.redeem(&token, "c", "Caio").unwrap();
        assert!(hub.redeem(&token, "d", "Duda").is_err(), "uso único");
        assert!(matches!(c.head().await, Err(NmError::Forbidden(_))), "ainda não aprovado");
        assert!(matches!(c.create_invite(&InviteOptions::default()).await, Err(NmError::Forbidden(_))));
        a.set_member("c", Some(true), false).await.unwrap();
        c.head().await.unwrap();
    }
}
