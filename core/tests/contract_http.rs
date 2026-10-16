//! Contrato entre o app (Rust) e a coordenação (Worker): os mesmos fluxos dos testes em
//! memória, agora contra o Worker de verdade rodando localmente (`wrangler dev`).
//!
//! Roda só com `P3_COORD_URL` e `P3_ADMIN` definidos, por exemplo:
//! ```text
//! cd coordinator && npx wrangler dev --local --port 8787     (ADMIN_TOKEN em .dev.vars)
//! P3_COORD_URL=http://127.0.0.1:8787 P3_ADMIN=dev-admin-local cargo test -p pauletor3-core --test contract_http
//! ```

use std::sync::Arc;

use chrono::Utc;
use pauletor3_core::coord::http::{self, ApiClient, HttpCoordinator};
use pauletor3_core::coord::{Coordinator, InviteOptions, LeasePurpose, LeaseStatus, PrivateSettings, Presence, Role};
use pauletor3_core::events::quiet;
use pauletor3_core::invite::Invite;
use pauletor3_core::lease::{LeaseConfig, LeaseManager};
use pauletor3_core::manifest::{ManifestKind, ServerVersions};
use pauletor3_core::p2p;
use pauletor3_core::paths::Paths;
use pauletor3_core::remote::{layout, CloudRemote, EncryptedRemote, Remote};
use pauletor3_core::sync::{PullOutcome, PushMeta, SyncEngine};
use pauletor3_core::NmError;

fn env() -> Option<(String, String)> {
    Some((std::env::var("P3_COORD_URL").ok()?, std::env::var("P3_ADMIN").ok()?))
}

#[tokio::test]
async fn full_contract_against_local_worker() {
    let Some((base, admin)) = env() else {
        eprintln!("P3_COORD_URL/P3_ADMIN não definidos; pulando o teste de contrato");
        return;
    };
    let owner_key = p2p::new_secret();
    let friend_key = p2p::new_secret();
    let world_key = [7u8; 32];

    // dono cria o mundo
    assert!(matches!(http::create_world(&base, owner_key, "errada", "Teste", "mchado").await, Err(NmError::Forbidden(_))));
    let world = http::create_world(&base, owner_key, &admin, "Mundo de Teste", "mchado").await.unwrap();
    let owner = HttpCoordinator::new(ApiClient::new(&base, &world, owner_key).unwrap());
    let v = owner.view().await.unwrap();
    assert_eq!(v.name, "Mundo de Teste");
    assert_eq!(v.me.as_ref().unwrap().role, Role::Owner);

    // convite → link → amigo entra
    let inv = owner.create_invite(&InviteOptions { hours: 1, uses: 1, preapproved: true }).await.unwrap();
    let link = Invite::new(&base, inv.token.as_deref().unwrap(), &world_key).link();
    let parsed = Invite::parse(&link).unwrap();
    assert_eq!(parsed.world_id(), world);
    let j = http::redeem_invite(&parsed.base, friend_key, &parsed.token, "Sara_1").await.unwrap();
    assert!(j.approved);
    assert_eq!(j.owner_nick, "mchado");
    let friend = HttpCoordinator::new(ApiClient::new(&base, &world, friend_key).unwrap());
    assert!(matches!(friend.create_invite(&InviteOptions::default()).await, Err(NmError::Forbidden(_))));

    // presença devolve a fotografia
    let v = friend.presence(&Presence { replica_seq: 0, playing: true, app_version: "t".into(), ..Default::default() }).await.unwrap();
    assert_eq!(v.members.len(), 2);
    assert!(v.members.iter().any(|m| m.nick == "Sara_1" && m.presence.as_ref().map(|p| p.playing) == Some(true)));

    // configurações privadas (webhook)
    assert!(owner.set_private_settings(&PrivateSettings { discord_webhook: Some("https://exemplo.com/x".into()) }).await.is_err());
    owner
        .set_private_settings(&PrivateSettings { discord_webhook: Some("https://discord.com/api/webhooks/123/abc-DEF".into()) })
        .await
        .unwrap();
    assert!(friend.private_settings().await.unwrap().discord_webhook.is_some());

    // vez + sincronização pela nuvem de verdade (cifrada)
    let da = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    let pa = Paths::new(da.path());
    let pb = Paths::new(db.path());
    pa.ensure().unwrap();
    pb.ensure().unwrap();
    let ra: Arc<dyn Remote> = Arc::new(EncryptedRemote::new(Arc::new(CloudRemote::new(owner.api.clone())), world_key, pa.tmp()));
    let rb: Arc<dyn Remote> = Arc::new(EncryptedRemote::new(Arc::new(CloudRemote::new(friend.api.clone())), world_key, pb.tmp()));
    let sa = SyncEngine::new(ra.clone(), Arc::new(owner.clone()), pa.clone());
    let sb = SyncEngine::new(rb.clone(), Arc::new(friend.clone()), pb.clone());

    let world_dir = pa.staging().join("world").join("region");
    std::fs::create_dir_all(&world_dir).unwrap();
    // uma região maior que um pedaço do armazenamento (1,9 MB) e outras pequenas
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(world_dir.join("r.0.0.mca"), &big).unwrap();
    std::fs::write(world_dir.join("r.1.0.mca"), vec![9u8; 50_000]).unwrap();
    std::fs::write(pa.staging().join("world").join("level.dat"), b"nivel").unwrap();

    let lm_a = LeaseManager::new(Arc::new(owner.clone()), LeaseConfig::default());
    let lm_b = LeaseManager::new(Arc::new(friend.clone()), LeaseConfig::default());
    let ga = lm_a.acquire(LeaseManager::new_request("s1", "mchado", LeasePurpose::Hosting, Some("{}".into()), 0)).await.unwrap();
    assert!(matches!(
        lm_b.acquire(LeaseManager::new_request("s2", "Sara_1", LeasePurpose::Hosting, None, 0)).await,
        Err(NmError::HeldBy(ref l)) if l.player == "mchado"
    ));
    let meta = PushMeta {
        kind: ManifestKind::Session,
        session_id: "s1".into(),
        session_started_at: Utc::now(),
        host_player: "mchado".into(),
        players: vec![],
        versions: ServerVersions { minecraft: "1.21.11".into(), fabric_loader: "0.19.5".into(), lithium: "0.21.4".into() },
        note: None,
    };
    let m1 = sa.push(&pa.staging(), meta.clone(), Some(&ga), None, true, quiet()).await.unwrap();
    assert_eq!(sa.head().await.unwrap().unwrap().manifest, m1.id);
    // HEAD só com CAS: publicar de novo a partir de "nada" falha
    assert!(matches!(ga.publish_head(None, &m1.id).await, Err(NmError::HeadMoved { .. })));
    ga.update(Some(vec!["mchado".into()]), None).await.unwrap();
    assert!(matches!(friend.view().await.unwrap().lease, LeaseStatus::Held { ref lease } if lease.online == vec!["mchado".to_string()]));
    ga.release().await.unwrap();

    assert_eq!(sb.pull(quiet()).await.unwrap(), PullOutcome::Updated);
    assert_eq!(std::fs::read(pb.world().join("region").join("r.0.0.mca")).unwrap(), big, "arquivo grande íntegro");
    assert_eq!(rb.list(layout::OBJECTS).await.unwrap().len(), 3);

    // a vez velha não publica mais
    let gb = lm_b.acquire(LeaseManager::new_request("s2", "Sara_1", LeasePurpose::Hosting, None, 1)).await.unwrap();
    assert!(gb.session_number() > ga.session_number());
    assert!(matches!(ga.publish_head(Some(&m1.id), &m1.id).await, Err(NmError::LostLock)));
    gb.release().await.unwrap();

    // diário
    owner
        .post_events(&[pauletor3_core::coord::DiaryEvent { at: Utc::now(), kind: "opened".into(), player: Some("mchado".into()), text: "abriu".into() }])
        .await
        .unwrap();
    assert_eq!(friend.events(10).await.unwrap()[0].text, "abriu");

    // remoção de membro corta o acesso na hora
    owner.set_member(&friend.me(), None, true).await.unwrap();
    assert!(matches!(friend.view().await, Err(NmError::Forbidden(_))));
    assert!(matches!(rb.read("versions.json").await, Err(NmError::Forbidden(_))));
}
