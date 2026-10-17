//! Testes de integração sem Minecraft: vários "PCs" (pastas e motores próprios) no mesmo
//! processo, com a coordenação em memória (mesmas regras do Durable Object), a nuvem numa
//! pasta (cifrada com a chave do mundo) e a rede P2P de verdade (iroh, só local).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use machenia_core::coord::mem::{MemCoordinator, MemHub};
use machenia_core::coord::{rules, Coordinator, LeasePurpose, Presence};
use machenia_core::events::quiet;
use machenia_core::lease::{LeaseConfig, LeaseGuard, LeaseManager};
use machenia_core::manifest::{FileEntry, ManifestKind, ServerVersions};
use machenia_core::mirror;
use machenia_core::p2p::{self, NetMode, Node, PeerObjects};
use machenia_core::paths::Paths;
use machenia_core::remote::{layout, EncryptedRemote, FsRemote, Remote};
use machenia_core::retention::{self, Policy};
use machenia_core::sync::{LocalState, PullOutcome, PushMeta, SyncEngine};
use machenia_core::NmError;

const KEY: [u8; 32] = [42u8; 32];

/// Um "PC": pasta de dados própria e identidade própria na coordenação.
struct Pc {
    _dir: tempfile::TempDir,
    paths: Paths,
    coord: MemCoordinator,
}

impl Pc {
    fn new(hub: &MemHub, id: &str) -> Self {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        paths.ensure().unwrap();
        Self { _dir: d, paths, coord: hub.client(id) }
    }
    fn engine(&self, r: Arc<dyn Remote>) -> SyncEngine {
        SyncEngine::new(r, Arc::new(self.coord.clone()), self.paths.clone())
    }
    async fn acquire(&self, sid: &str, replica_seq: u64) -> Result<LeaseGuard, NmError> {
        LeaseManager::new(Arc::new(self.coord.clone()), LeaseConfig::default())
            .acquire(LeaseManager::new_request(sid, "Ana", LeasePurpose::Hosting, None, replica_seq))
            .await
    }
}

fn hub() -> MemHub {
    let h = MemHub::new("pAuletOr 3", "a", "Ana");
    for (id, nick) in [("b", "Bia"), ("c", "Caio")] {
        h.add_member(id, nick);
    }
    h
}

/// Nuvem numa pasta, cifrada com a chave do mundo (como a de verdade).
fn cloud(dir: &Path, tmp: &Path) -> (FsRemote, Arc<dyn Remote>) {
    let fs = FsRemote::new(dir);
    let r: Arc<dyn Remote> = Arc::new(EncryptedRemote::new(Arc::new(fs.clone()), KEY, tmp.to_path_buf()));
    (fs, r)
}

/// Cria um mundo falso com level.dat válido (NBT gzip), regiões e playerdata.
fn make_world(server: &Path, seed: u8) {
    use machenia_core::nbt::{self, Tag};
    let w = server.join("world");
    std::fs::create_dir_all(w.join("region")).unwrap();
    std::fs::create_dir_all(w.join("playerdata")).unwrap();
    std::fs::create_dir_all(w.join("DIM-1/region")).unwrap();
    let root = Tag::Compound(vec![("Data".into(), Tag::Compound(vec![("LevelName".into(), Tag::String("world".into()))]))]);
    let mut raw = vec![];
    nbt::write(&mut raw, "", &root).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &raw).unwrap();
    std::fs::write(w.join("level.dat"), gz.finish().unwrap()).unwrap();
    for i in 0..6 {
        let data: Vec<u8> = (0..(40_000 + i * 1000)).map(|k| (k as u8).wrapping_mul(seed).wrapping_add(i as u8)).collect();
        std::fs::write(w.join("region").join(format!("r.{i}.0.mca")), data).unwrap();
    }
    std::fs::write(w.join("DIM-1/region/r.0.0.mca"), vec![seed; 9000]).unwrap();
    std::fs::write(w.join("playerdata/ana.dat"), format!("ana-{seed}")).unwrap();
    std::fs::write(w.join("session.lock"), b"x").unwrap();
    std::fs::write(server.join("ops.json"), b"[]").unwrap();
}

fn meta(kind: ManifestKind, session: &str) -> PushMeta {
    PushMeta {
        kind,
        session_id: session.into(),
        session_started_at: Utc::now(),
        host_player: "Ana".into(),
        players: vec!["Ana".into()],
        versions: ServerVersions { minecraft: "1.21.11".into(), fabric_loader: "0.19.5".into(), lithium: "0.21.4".into() },
        note: None,
    }
}

async fn host_push(pc: &Pc, se: &SyncEngine, guard: &LeaseGuard, kind: ManifestKind, expected: Option<&str>) -> Result<String, NmError> {
    mirror::mirror(&pc.paths.server(), &pc.paths.staging()).unwrap();
    se.push(&pc.paths.staging(), meta(kind, "sessao"), Some(guard), expected, true, quiet()).await.map(|m| m.id)
}

fn fingerprint(server: &Path) -> BTreeMap<String, FileEntry> {
    let mut f = SyncEngine::fingerprint(server).unwrap();
    for v in f.values_mut() {
        v.mtime = 0;
    }
    f.remove("world/session.lock");
    f
}

/// PC A cria o mundo e envia; PC B baixa e recebe exatamente o mesmo (hashes iguais).
/// Depois B joga, envia, e A recebe as mudanças (só os objetos novos trafegam).
#[tokio::test]
async fn roundtrip_between_two_pcs() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let b = Pc::new(&h, "b");
    let (fs, ra) = cloud(nuvem.path(), &a.paths.tmp());
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());

    make_world(&a.paths.server(), 3);
    let g = a.acquire("a1", 0).await.unwrap();
    let sa = a.engine(ra.clone());
    let m1 = host_push(&a, &sa, &g, ManifestKind::Session, None).await.unwrap();
    g.release().await.unwrap();
    assert_eq!(sa.head().await.unwrap().unwrap().manifest, m1);
    assert_eq!(sa.replica_seq(), 1);
    assert!(!sa.fetch_manifest(&m1).await.unwrap().files.contains_key("world/session.lock"));
    // a nuvem só tem bytes cifrados
    let any = fs.list(layout::OBJECTS).await.unwrap()[0].name.clone();
    let raw = fs.read(&format!("objects/{any}")).await.unwrap().unwrap();
    assert_eq!(&raw[..4], b"P3E1");

    let sb = b.engine(rb.clone());
    assert_eq!(sb.pull(quiet()).await.unwrap(), PullOutcome::Updated);
    assert_eq!(fingerprint(&a.paths.server()), fingerprint(&b.paths.server()), "mundo idêntico no segundo PC");
    assert_eq!(sb.pull(quiet()).await.unwrap(), PullOutcome::UpToDate);

    // B joga: muda uma região e cria outra
    std::fs::write(b.paths.world().join("region/r.0.0.mca"), vec![7u8; 41_000]).unwrap();
    std::fs::write(b.paths.world().join("region/r.9.9.mca"), vec![8u8; 5_000]).unwrap();
    std::fs::remove_file(b.paths.world().join("region/r.5.0.mca")).unwrap();
    let before = rb.list(layout::OBJECTS).await.unwrap().len();
    let g = b.acquire("b1", sb.replica_seq()).await.unwrap();
    let m2 = host_push(&b, &sb, &g, ManifestKind::Session, Some(&m1)).await.unwrap();
    g.release().await.unwrap();
    assert_eq!(rb.list(layout::OBJECTS).await.unwrap().len() - before, 2, "só os 2 arquivos novos foram enviados");

    // A já tem o mundo: só a diferença é trocada (nada é remontado do zero)
    assert_eq!(sa.pull(quiet()).await.unwrap(), PullOutcome::Updated);
    assert_eq!(fingerprint(&a.paths.server()), fingerprint(&b.paths.server()));
    assert!(!a.paths.world().join("region/r.5.0.mca").exists(), "arquivo apagado no outro PC some aqui");
    assert!(a.paths.world().join("session.lock").exists(), "o resto da pasta do mundo ficou onde estava");
    assert_eq!(LocalState::load(&a.paths).current_manifest.as_deref(), Some(m2.as_str()));
}

/// Passar o host: a vez fica "vencida agora" e marcada; quem está jogando assume na hora, e a
/// réplica mais nova tem a preferência.
#[tokio::test]
async fn planned_handoff_goes_to_newest_replica() {
    let h = hub();
    let a = Pc::new(&h, "a");
    let b = Pc::new(&h, "b");
    let c = Pc::new(&h, "c");
    let ga = a.acquire("sa", 3).await.unwrap();
    b.coord.presence(&Presence { replica_seq: 3, playing: true, ..Default::default() }).await.unwrap();
    c.coord.presence(&Presence { replica_seq: 2, playing: true, ..Default::default() }).await.unwrap();
    assert!(matches!(c.acquire("sc", 2).await, Err(NmError::HeldBy(_))));

    ga.release_handoff().await.unwrap();
    let v = b.coord.view().await.unwrap();
    match &v.lease {
        machenia_core::coord::LeaseStatus::Free { previous: Some(p) } => {
            assert!(p.handing_over);
            assert_eq!(p.holder, "a");
        }
        other => panic!("esperava a vez livre com o host anterior: {other:?}"),
    }
    assert!(matches!(c.acquire("sc", 2).await, Err(NmError::NotBestCandidate { best_seq: 3 })));
    let gb = b.acquire("sb", 3).await.unwrap();
    assert!(gb.session_number() > ga.session_number());
}

/// Envio interrompido no meio: o HEAD não muda e o mundo oficial continua íntegro.
#[tokio::test]
async fn interrupted_upload_never_moves_head() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let (fs, r) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 1);
    let se = a.engine(r.clone());
    let g = a.acquire("s", 0).await.unwrap();
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();

    make_world(&a.paths.server(), 99);
    fs.fail_after(2);
    let err = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap_err();
    assert!(matches!(err, NmError::Remote(_)), "{err:?}");
    assert_eq!(se.head().await.unwrap().unwrap().manifest, m1, "HEAD intacto");
    assert_eq!(se.list_manifest_ids().await.unwrap(), vec![m1.clone()], "nenhum manifesto parcial");

    let b = Pc::new(&h, "b");
    fs.heal();
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
    let sb = b.engine(rb);
    sb.pull(quiet()).await.unwrap();
    let m = sb.fetch_manifest(&m1).await.unwrap();
    for (rel, e) in &m.files {
        assert_eq!(machenia_core::hashing::sha256_file(&b.paths.server().join(rel)).unwrap(), e.hash);
    }

    let m2 = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap();
    assert_eq!(se.head().await.unwrap().unwrap().manifest, m2);
    g.release().await.unwrap();
}

/// Objeto corrompido na nuvem: o download é recusado e o mundo local bom não é tocado.
#[tokio::test]
async fn corrupted_object_aborts_without_touching_local_world() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let (fs, r) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 4);
    let se = a.engine(r.clone());
    let g = a.acquire("s", 0).await.unwrap();
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();

    let b = Pc::new(&h, "b");
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
    let sb = b.engine(rb);
    sb.pull(quiet()).await.unwrap();
    let good = fingerprint(&b.paths.server());

    std::fs::write(a.paths.world().join("region/r.1.0.mca"), vec![42u8; 33_000]).unwrap();
    let m2 = host_push(&a, &se, &g, ManifestKind::Session, Some(&m1)).await.unwrap();
    let hash = se.fetch_manifest(&m2).await.unwrap().files["world/region/r.1.0.mca"].hash.clone();
    // alguém adultera o objeto na nuvem (cifrado: nem abre com a chave)
    fs.write(&format!("objects/{hash}"), b"estragado").await.unwrap();

    let err = sb.pull(quiet()).await.unwrap_err();
    assert!(matches!(err, NmError::Integrity(_)), "{err:?}");
    assert_eq!(fingerprint(&b.paths.server()), good, "mundo local intocado");
    assert_eq!(LocalState::load(&b.paths).current_manifest.as_deref(), Some(m1.as_str()));
    g.release().await.unwrap();
}

/// Dois PCs clicando em Jogar ao mesmo tempo: só um hospeda (20 rodadas).
#[tokio::test]
async fn two_hosts_at_once_never() {
    for round in 0..20 {
        let h = hub();
        let mut tasks = vec![];
        for id in ["a", "b", "c"] {
            let c = h.client(id);
            tasks.push(tokio::spawn(async move {
                LeaseManager::new(Arc::new(c), LeaseConfig::default())
                    .acquire(LeaseManager::new_request(&format!("s-{id}"), id, LeasePurpose::Hosting, None, 0))
                    .await
            }));
        }
        let mut ok = 0;
        for t in tasks {
            if t.await.unwrap().is_ok() {
                ok += 1;
            }
        }
        assert_eq!(ok, 1, "rodada {round}: exatamente um host");
    }
}

/// O host cai: a vez expira, quem estava jogando com a réplica mais nova tem a preferência,
/// e o host antigo (se voltar) não consegue publicar por cima.
#[tokio::test]
async fn crash_failover_prefers_newest_replica_and_fences_old_host() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let (_, ra) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 9);
    let sa = a.engine(ra);
    let ga = a.acquire("sa", 0).await.unwrap();
    let m1 = host_push(&a, &sa, &ga, ManifestKind::Checkpoint, None).await.unwrap();
    let m2 = {
        std::fs::write(a.paths.world().join("playerdata/ana.dat"), b"progresso").unwrap();
        host_push(&a, &sa, &ga, ManifestKind::Checkpoint, Some(&m1)).await.unwrap()
    };
    // b e c estão jogando; b tem a réplica da versão 2, c ficou na 1
    let b = Pc::new(&h, "b");
    let c = Pc::new(&h, "c");
    b.coord.presence(&Presence { replica_seq: 2, playing: true, ..Default::default() }).await.unwrap();
    c.coord.presence(&Presence { replica_seq: 1, playing: true, ..Default::default() }).await.unwrap();

    // queda de energia do host: nada de heartbeat
    ga.abandon().await;
    h.clock.advance(chrono::Duration::seconds(rules::LEASE_TTL_SECS + 2));
    b.coord.presence(&Presence { replica_seq: 2, playing: true, ..Default::default() }).await.unwrap();
    c.coord.presence(&Presence { replica_seq: 1, playing: true, ..Default::default() }).await.unwrap();

    assert!(matches!(c.acquire("sc", 1).await, Err(NmError::NotBestCandidate { best_seq: 2 })));
    let gb = b.acquire("sb", 2).await.unwrap();
    assert!(gb.session_number() > ga.session_number());

    // b continua do HEAD e publica
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
    let sb = b.engine(rb);
    sb.pull(quiet()).await.unwrap();
    std::fs::write(b.paths.world().join("playerdata/ana.dat"), b"continuou no b").unwrap();
    let m3 = host_push(&b, &sb, &gb, ManifestKind::Checkpoint, Some(&m2)).await.unwrap();

    // a volta à vida e tenta publicar o que tinha: barrado, sem tocar no HEAD
    std::fs::write(a.paths.world().join("playerdata/ana.dat"), b"progresso velho do a").unwrap();
    let err = host_push(&a, &sa, &ga, ManifestKind::Checkpoint, Some(&m2)).await.unwrap_err();
    assert!(matches!(err, NmError::LostLock), "{err:?}");
    assert_eq!(sa.head().await.unwrap().unwrap().manifest, m3);
}

/// Réplica velha nunca vence a nova: mesmo com a vez livre, publicar exige partir do HEAD.
#[tokio::test]
async fn stale_replica_cannot_overwrite_newer_world() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let b = Pc::new(&h, "b");
    let (_, ra) = cloud(nuvem.path(), &a.paths.tmp());
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
    make_world(&a.paths.server(), 2);
    let sa = a.engine(ra);
    let sb = b.engine(rb);
    let g = a.acquire("s1", 0).await.unwrap();
    let m1 = host_push(&a, &sa, &g, ManifestKind::Session, None).await.unwrap();
    g.release().await.unwrap();
    sb.pull(quiet()).await.unwrap(); // b fica com a versão 1

    let g = a.acquire("s2", 1).await.unwrap();
    std::fs::write(a.paths.world().join("playerdata/ana.dat"), b"novo").unwrap();
    let m2 = host_push(&a, &sa, &g, ManifestKind::Session, Some(&m1)).await.unwrap();
    g.release().await.unwrap();

    // b tenta publicar a partir da cópia velha (bug ou app antigo): vira manifesto solto
    let g = b.acquire("s3", 1).await.unwrap();
    std::fs::write(b.paths.world().join("playerdata/ana.dat"), b"velho").unwrap();
    let err = host_push(&b, &sb, &g, ManifestKind::Session, Some(&m1)).await.unwrap_err();
    assert!(matches!(err, NmError::Integrity(_)), "{err:?}");
    assert_eq!(sb.head().await.unwrap().unwrap().manifest, m2, "o mundo novo continua oficial");
    g.release().await.unwrap();
}

/// Coordenação fora do ar: nada é publicado; quando volta, o envio continua de onde parou.
#[tokio::test]
async fn coordinator_outage_and_partition() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let (_, r) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 5);
    let se = a.engine(r);
    let g = a.acquire("s", 0).await.unwrap();
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();

    // só este PC perde a coordenação (partição)
    a.coord.set_offline(true);
    std::fs::write(a.paths.world().join("playerdata/ana.dat"), b"durante a queda").unwrap();
    let err = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap_err();
    assert!(matches!(err, NmError::Remote(_)), "{err:?}");
    // os outros continuam vendo o HEAD bom e não conseguem hospedar (a vez segue com a)
    let b = Pc::new(&h, "b");
    assert_eq!(b.coord.head().await.unwrap().unwrap().manifest, m1);
    assert!(matches!(b.acquire("sb", 1).await, Err(NmError::HeldBy(_))));

    // coordenação volta
    a.coord.set_offline(false);
    let m2 = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap();
    assert_eq!(se.head().await.unwrap().unwrap().manifest, m2);

    // queda geral: ninguém publica nada
    h.down.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(b.acquire("sb", 2).await, Err(NmError::Remote(_))));
    h.down.store(false, std::sync::atomic::Ordering::SeqCst);
    g.release().await.unwrap();
}

/// Réplica por P2P: B copia o mundo direto do cache de A, com a nuvem fora do ar.
#[tokio::test]
async fn p2p_replica_works_without_cloud() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let b = Pc::new(&h, "b");
    let (fs, ra) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 6);
    let sa = a.engine(ra);
    let g = a.acquire("s", 0).await.unwrap();
    let m1 = host_push(&a, &sa, &g, ManifestKind::Session, None).await.unwrap();

    let na = Node::start(&p2p::new_secret(), NetMode::LocalOnly, &a.paths).await.unwrap();
    let nb = Node::start(&p2p::new_secret(), NetMode::LocalOnly, &b.paths).await.unwrap();
    na.set_members([nb.id_hex().as_str()]);
    nb.set_members([na.id_hex().as_str()]);

    // a nuvem "cai" para downloads: tudo tem que vir de A
    fs.fail_after(0);
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
    let peers: Arc<dyn machenia_core::sync::ObjectSource> = Arc::new(PeerObjects { node: nb.clone(), peers: vec![na.endpoint.addr()] });
    let sb = b.engine(rb).with_peers(Some(peers));
    let head = sb.prefetch(quiet()).await.unwrap().unwrap();
    assert_eq!(head.manifest, m1);
    assert_eq!(sb.replica_seq(), head.seq, "réplica em dia sem a nuvem");
    sb.pull(quiet()).await.unwrap();
    assert_eq!(fingerprint(&a.paths.server()), fingerprint(&b.paths.server()));
    g.release().await.unwrap();
}

/// Restaurar cria um manifesto novo apontando para os mesmos objetos; a história fica.
#[tokio::test]
async fn restore_creates_new_version() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let (_, r) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 7);
    let se = a.engine(r.clone());
    let g = a.acquire("s", 0).await.unwrap();
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();
    let v1 = fingerprint(&a.paths.server());
    std::fs::write(a.paths.world().join("region/r.2.0.mca"), vec![1u8; 100]).unwrap();
    let m2 = host_push(&a, &se, &g, ManifestKind::Session, Some(&m1)).await.unwrap();
    let restored = se.restore(&m1, &g, "Ana").await.unwrap();
    g.release().await.unwrap();
    assert_eq!(restored.restored_from.as_deref(), Some(m1.as_str()));
    assert_eq!(restored.parent.as_deref(), Some(m2.as_str()));
    assert_eq!(se.head().await.unwrap().unwrap().manifest, restored.id);
    assert_eq!(se.list_manifest_ids().await.unwrap().len(), 3, "nada apagado");

    let b = Pc::new(&h, "b");
    let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
    b.engine(rb).pull(quiet()).await.unwrap();
    assert_eq!(fingerprint(&b.paths.server()), v1);
}

/// GC: apaga manifestos fora da retenção e objetos órfãos, nunca algo que um manifesto mantido usa.
#[tokio::test]
async fn gc_keeps_what_is_referenced() {
    let h = hub();
    let nuvem = tempfile::tempdir().unwrap();
    let a = Pc::new(&h, "a");
    let (fs, r) = cloud(nuvem.path(), &a.paths.tmp());
    make_world(&a.paths.server(), 8);
    let se = a.engine(r.clone());
    let g = a.acquire("s", 0).await.unwrap();
    let mut last: Option<String> = None;
    for i in 0..4u8 {
        std::fs::write(a.paths.world().join("region/r.5.0.mca"), vec![i; 20_000 + i as usize]).unwrap();
        mirror::mirror(&a.paths.server(), &a.paths.staging()).unwrap();
        let id = se.push(&a.paths.staging(), meta(ManifestKind::Session, &format!("sessao{i}")), Some(&g), last.as_deref(), true, quiet()).await.unwrap().id;
        last = Some(id);
    }
    // lixo: objeto órfão de um envio interrompido
    fs.write(&format!("objects/{}", "f".repeat(64)), b"orfao").await.unwrap();

    let policy = Policy { last_sessions: 2, daily_days: 0 };
    assert_eq!(retention::gc(&se, &g, policy, true).await.unwrap().manifests_deleted, 2);
    let rep = retention::gc(&se, &g, policy, false).await.unwrap();
    assert_eq!(rep.manifests_deleted, 2);
    assert!(rep.objects_deleted >= 3, "{rep:?}");
    g.release().await.unwrap();

    for id in se.list_manifest_ids().await.unwrap() {
        let b = Pc::new(&h, "b");
        let (_, rb) = cloud(nuvem.path(), &b.paths.tmp());
        let m = se.fetch_manifest(&id).await.unwrap();
        b.engine(rb).materialize(&m, &b.paths.server(), quiet()).await.unwrap();
    }
}

/// Troca planejada só entre a mesma versão do app; a versão mínima chega na fotografia.
#[tokio::test]
async fn planned_handoff_requires_same_app_version_and_min_version_reaches_view() {
    let h = hub();
    let a = Pc::new(&h, "a");
    let b = Pc::new(&h, "b");
    let ga = a.acquire("sa", 3).await.unwrap();
    b.coord.presence(&Presence { replica_seq: 3, playing: true, ..Default::default() }).await.unwrap();
    ga.release_handoff().await.unwrap();

    let mut old = LeaseManager::new_request("sb", "Bia", LeasePurpose::Hosting, None, 3);
    old.app_version = "0.0.1".into();
    assert!(matches!(b.coord.acquire(old).await, Err(NmError::AppVersionMismatch { .. })));
    assert!(b.acquire("sb", 3).await.is_ok(), "mesma versão assume");

    assert_eq!(b.coord.view().await.unwrap().versao_minima, None);
    h.client("b").set_versao_minima(Some("99.0.0"));
    assert_eq!(b.coord.view().await.unwrap().versao_minima.as_deref(), Some("99.0.0"));
    assert!(machenia_core::atualizacao::abaixo_do_minimo(machenia_core::APP_VERSION, "99.0.0"));
}

/// "Chamar pro jogo": o chamado chega uma vez só, na presença de quem foi chamado.
#[tokio::test]
async fn call_member_arrives_once_on_presence() {
    let h = hub();
    let a = Pc::new(&h, "a");
    let b = Pc::new(&h, "b");
    b.coord.call("a").await.unwrap();
    let v = a.coord.presence(&Presence::default()).await.unwrap();
    assert_eq!(v.chamado.as_ref().map(|c| c.de.as_str()), Some("Bia"));
    assert!(a.coord.presence(&Presence::default()).await.unwrap().chamado.is_none());
    assert!(b.coord.call("ninguem").await.is_err());
}
