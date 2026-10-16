//! Testes de integração do modelo de sincronização e da trava, usando o rclone de verdade
//! com o backend `local` (sem Google Drive) e o remote em Rust puro para injetar falhas.
//!
//! O rclone é procurado em `PAULETOR3_RCLONE` ou `D:\DevTools\rclone\rclone.exe`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use pauletor3_core::events::quiet;
use pauletor3_core::lock::{LockConfig, LockGuard, LockInfo, LockManager, LockPurpose, LockState, LockStatus, Liveness};
use pauletor3_core::manifest::{FileEntry, ManifestKind, ServerVersions};
use pauletor3_core::paths::Paths;
use pauletor3_core::remote::{layout, write_json, FsRemote, RcloneRemote, Remote};
use pauletor3_core::retention::{self, Policy};
use pauletor3_core::sync::{LocalState, PullOutcome, PushMeta, SyncEngine};
use pauletor3_core::{mirror, NmError};

fn rclone_bin() -> Option<PathBuf> {
    let p = std::env::var_os("PAULETOR3_RCLONE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\DevTools\rclone\rclone.exe"));
    if p.exists() {
        Some(p)
    } else {
        eprintln!("rclone não encontrado; teste com rclone pulado");
        None
    }
}

struct Dead;
#[async_trait::async_trait]
impl Liveness for Dead {
    async fn alive(&self, _: &LockInfo) -> bool {
        false
    }
}

fn fast_lock() -> LockConfig {
    LockConfig { settle: Duration::from_millis(400), stale_after: Duration::from_secs(300), heartbeat_every: Duration::from_secs(30) }
}

/// Um "PC": pasta de dados própria.
struct Pc {
    _dir: tempfile::TempDir,
    paths: Paths,
}

impl Pc {
    fn new() -> Self {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        paths.ensure().unwrap();
        Self { _dir: d, paths }
    }
    fn engine(&self, r: Arc<dyn Remote>) -> SyncEngine {
        SyncEngine::new(r, self.paths.clone())
    }
}

/// Cria um mundo falso com level.dat válido (NBT gzip), regiões e playerdata.
fn make_world(server: &Path, seed: u8) {
    use pauletor3_core::nbt::{self, Tag};
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

async fn host_push(pc: &Pc, se: &SyncEngine, guard: &LockGuard, kind: ManifestKind, expected: Option<&str>) -> Result<String, NmError> {
    mirror::mirror(&pc.paths.server(), &pc.paths.staging()).unwrap();
    se.push(&pc.paths.staging(), meta(kind, "sessao"), Some(guard), expected, true, quiet()).await.map(|m| m.id)
}

fn fingerprint(server: &Path) -> BTreeMap<String, FileEntry> {
    let mut f = SyncEngine::fingerprint(server).unwrap();
    for v in f.values_mut() {
        v.mtime = 0;
    }
    f
}

async fn acquire(r: Arc<dyn Remote>, id: &str) -> LockGuard {
    LockManager::new(r, Arc::new(Dead), fast_lock())
        .acquire(LockInfo::new(id, "Ana", LockPurpose::Hosting), &[])
        .await
        .unwrap()
}

/// PC A cria o mundo e envia; PC B baixa e recebe exatamente o mesmo (hashes iguais).
/// Depois B joga, envia, e A recebe as mudanças (só os objetos novos trafegam).
#[tokio::test]
async fn roundtrip_between_two_pcs_with_rclone() {
    let Some(bin) = rclone_bin() else { return };
    let remote_dir = tempfile::tempdir().unwrap();
    let a = Pc::new();
    let b = Pc::new();
    let ra: Arc<dyn Remote> = Arc::new(RcloneRemote::local(bin.clone(), remote_dir.path(), a.paths.tmp()));
    let rb: Arc<dyn Remote> = Arc::new(RcloneRemote::local(bin.clone(), remote_dir.path(), b.paths.tmp()));

    make_world(&a.paths.server(), 3);
    let g = acquire(ra.clone(), "a1").await;
    let sa = a.engine(ra.clone());
    let m1 = host_push(&a, &sa, &g, ManifestKind::Session, None).await.unwrap();
    g.release().await.unwrap();
    assert_eq!(sa.head().await.unwrap().unwrap().manifest, m1);
    // session.lock nunca vai para a nuvem
    assert!(!sa.fetch_manifest(&m1).await.unwrap().files.contains_key("world/session.lock"));

    let sb = b.engine(rb.clone());
    assert_eq!(sb.pull(quiet()).await.unwrap(), PullOutcome::Updated);
    let mut fa = fingerprint(&a.paths.server());
    fa.remove("world/session.lock");
    assert_eq!(fa, fingerprint(&b.paths.server()), "mundo idêntico no segundo PC");
    assert_eq!(sb.pull(quiet()).await.unwrap(), PullOutcome::UpToDate);

    // B joga: muda uma região e cria outra
    std::fs::write(b.paths.world().join("region/r.0.0.mca"), vec![7u8; 41_000]).unwrap();
    std::fs::write(b.paths.world().join("region/r.9.9.mca"), vec![8u8; 5_000]).unwrap();
    let before = rb.list(layout::OBJECTS).await.unwrap().len();
    let g = acquire(rb.clone(), "b1").await;
    let m2 = host_push(&b, &sb, &g, ManifestKind::Session, Some(&m1)).await.unwrap();
    g.release().await.unwrap();
    let after = rb.list(layout::OBJECTS).await.unwrap().len();
    assert_eq!(after - before, 2, "só os 2 arquivos novos foram enviados");

    assert_eq!(sa.pull(quiet()).await.unwrap(), PullOutcome::Updated);
    assert_eq!(fingerprint(&a.paths.server()), fingerprint(&b.paths.server()));
    assert_eq!(LocalState::load(&a.paths).current_manifest.as_deref(), Some(m2.as_str()));
}

/// Envio interrompido no meio: o HEAD não muda e o mundo oficial continua íntegro.
#[tokio::test]
async fn interrupted_upload_never_moves_head() {
    let remote_dir = tempfile::tempdir().unwrap();
    let fs = FsRemote::new(remote_dir.path());
    let r: Arc<dyn Remote> = Arc::new(fs.clone());
    let a = Pc::new();
    make_world(&a.paths.server(), 1);
    let se = a.engine(r.clone());
    let g = acquire(r.clone(), "s").await;
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();

    // muda tudo e derruba a "internet" depois de 2 objetos
    make_world(&a.paths.server(), 99);
    fs.fail_after(2);
    let err = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap_err();
    assert!(matches!(err, NmError::Remote(_)), "{err:?}");
    assert_eq!(se.head().await.unwrap().unwrap().manifest, m1, "HEAD intacto");
    assert_eq!(se.list_manifest_ids().await.unwrap(), vec![m1.clone()], "nenhum manifesto parcial");

    // outro PC baixa e recebe a versão boa, íntegra
    let b = Pc::new();
    fs.heal();
    let sb = b.engine(r.clone());
    sb.pull(quiet()).await.unwrap();
    let m = sb.fetch_manifest(&m1).await.unwrap();
    for (rel, e) in &m.files {
        let h = pauletor3_core::hashing::sha256_file(&b.paths.server().join(rel)).unwrap();
        assert_eq!(h, e.hash);
    }

    // a internet volta: o reenvio aproveita o que já subiu e publica
    let m2 = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap();
    assert_eq!(se.head().await.unwrap().unwrap().manifest, m2);
    g.release().await.unwrap();
}

/// Objeto corrompido no remote: o download é recusado e o mundo local bom não é tocado.
#[tokio::test]
async fn corrupted_object_aborts_without_touching_local_world() {
    let remote_dir = tempfile::tempdir().unwrap();
    let r: Arc<dyn Remote> = Arc::new(FsRemote::new(remote_dir.path()));
    let a = Pc::new();
    make_world(&a.paths.server(), 4);
    let se = a.engine(r.clone());
    let g = acquire(r.clone(), "s").await;
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();

    let b = Pc::new();
    let sb = b.engine(r.clone());
    sb.pull(quiet()).await.unwrap();
    let good = fingerprint(&b.paths.server());

    // A envia versão nova; um objeto dela chega corrompido no remote
    std::fs::write(a.paths.world().join("region/r.1.0.mca"), vec![42u8; 33_000]).unwrap();
    let m2 = host_push(&a, &se, &g, ManifestKind::Session, Some(&m1)).await.unwrap();
    let h = se.fetch_manifest(&m2).await.unwrap().files["world/region/r.1.0.mca"].hash.clone();
    std::fs::write(remote_dir.path().join("objects").join(&h), b"estragado").unwrap();

    let err = sb.pull(quiet()).await.unwrap_err();
    assert!(matches!(err, NmError::Integrity(_)), "{err:?}");
    assert_eq!(fingerprint(&b.paths.server()), good, "mundo local intocado");
    assert_eq!(LocalState::load(&b.paths).current_manifest.as_deref(), Some(m1.as_str()));
    g.release().await.unwrap();
}

/// Queda do host: a trava fica velha (heartbeat antigo + porta morta) e só é assumida com confirmação.
#[tokio::test]
async fn host_crash_stale_lock_and_takeover_with_rclone() {
    let Some(bin) = rclone_bin() else { return };
    let remote_dir = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let r: Arc<dyn Remote> = Arc::new(RcloneRemote::local(bin, remote_dir.path(), tmp.path().to_path_buf()));
    let mut crashed = LockInfo::new("caiu", "Ana", LockPurpose::Hosting);
    crashed.state = LockState::Active;
    crashed.tailscale_ip = Some("127.0.0.1".into());
    crashed.heartbeat_at = Utc::now() - chrono::Duration::minutes(7);
    write_json(&*r, &layout::lock("caiu"), &crashed).await.unwrap();

    let lm = LockManager::new(r.clone(), Arc::new(Dead), fast_lock());
    assert!(matches!(lm.status().await.unwrap(), LockStatus::Stale { .. }));
    let err = lm.acquire(LockInfo::new("bia", "Bia", LockPurpose::Hosting), &[]).await.unwrap_err();
    assert!(matches!(err, NmError::StaleLock(ref l) if l.player == "Ana"));
    let g = lm.acquire(LockInfo::new("bia", "Bia", LockPurpose::Hosting), &["caiu".into()]).await.unwrap();
    assert!(matches!(lm.status().await.unwrap(), LockStatus::Held { lock } if lock.player == "Bia"));
    g.release().await.unwrap();
}

/// Dois PCs clicando em Jogar ao mesmo tempo (via rclone): só um hospeda.
#[tokio::test]
async fn two_hosts_at_once_with_rclone() {
    let Some(bin) = rclone_bin() else { return };
    for round in 0..3 {
        let remote_dir = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mut tasks = vec![];
        for i in 0..2 {
            let r: Arc<dyn Remote> = Arc::new(RcloneRemote::local(bin.clone(), remote_dir.path(), tmp.path().join(format!("t{i}"))));
            tasks.push(tokio::spawn(async move {
                LockManager::new(r, Arc::new(Dead), fast_lock())
                    .acquire(LockInfo::new(&format!("pc{i}"), &format!("P{i}"), LockPurpose::Hosting), &[])
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

/// Checkpoints durante a partida encadeiam o HEAD; um HEAD mudado por fora bloqueia a publicação.
#[tokio::test]
async fn checkpoints_chain_and_foreign_head_is_never_overwritten() {
    let remote_dir = tempfile::tempdir().unwrap();
    let r: Arc<dyn Remote> = Arc::new(FsRemote::new(remote_dir.path()));
    let a = Pc::new();
    make_world(&a.paths.server(), 5);
    let se = a.engine(r.clone());
    let g = acquire(r.clone(), "s").await;
    let base = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();
    let mut last = base.clone();
    for i in 0..3u8 {
        std::fs::write(a.paths.world().join("playerdata/ana.dat"), format!("progresso {i}")).unwrap();
        let id = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&last)).await.unwrap();
        assert_eq!(se.fetch_manifest(&id).await.unwrap().parent.as_deref(), Some(last.as_str()));
        last = id;
    }
    // alguém (bug, outro app) mexe no HEAD por fora
    write_json(&*r, layout::HEAD, &pauletor3_core::sync::Head {
        format: 1,
        manifest: base.clone(),
        seq: 99,
        updated_at: Utc::now(),
        updated_by: "intruso".into(),
    })
    .await
    .unwrap();
    std::fs::write(a.paths.world().join("playerdata/ana.dat"), b"mais progresso").unwrap();
    let err = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&last)).await.unwrap_err();
    assert!(matches!(err, NmError::Integrity(_)));
    assert_eq!(se.head().await.unwrap().unwrap().manifest, base, "não sobrescreve o HEAD alheio");
    assert_eq!(se.list_manifest_ids().await.unwrap().len(), 5, "o progresso ficou salvo como manifesto solto");
    g.release().await.unwrap();
}

/// Perder a trava no meio da sessão impede gravar o HEAD.
#[tokio::test]
async fn lost_lock_blocks_head() {
    let remote_dir = tempfile::tempdir().unwrap();
    let r: Arc<dyn Remote> = Arc::new(FsRemote::new(remote_dir.path()));
    let a = Pc::new();
    make_world(&a.paths.server(), 6);
    let se = a.engine(r.clone());
    let g = acquire(r.clone(), "s").await;
    let m1 = host_push(&a, &se, &g, ManifestKind::Session, None).await.unwrap();
    r.delete(&layout::lock("s")).await.unwrap(); // alguém assumiu
    std::fs::write(a.paths.world().join("playerdata/ana.dat"), b"x").unwrap();
    let err = host_push(&a, &se, &g, ManifestKind::Checkpoint, Some(&m1)).await.unwrap_err();
    assert!(matches!(err, NmError::LostLock), "{err:?}");
    assert_eq!(se.head().await.unwrap().unwrap().manifest, m1);
}

/// Restaurar cria um manifesto novo apontando para os mesmos objetos; a história fica.
#[tokio::test]
async fn restore_creates_new_version() {
    let Some(bin) = rclone_bin() else { return };
    let remote_dir = tempfile::tempdir().unwrap();
    let a = Pc::new();
    let r: Arc<dyn Remote> = Arc::new(RcloneRemote::local(bin, remote_dir.path(), a.paths.tmp()));
    make_world(&a.paths.server(), 7);
    let se = a.engine(r.clone());
    let g = acquire(r.clone(), "s").await;
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

    let b = Pc::new();
    b.engine(r.clone()).pull(quiet()).await.unwrap();
    assert_eq!(fingerprint(&b.paths.server()), v1);
}

/// GC: apaga manifestos fora da retenção e objetos órfãos, nunca algo que um manifesto mantido usa.
#[tokio::test]
async fn gc_keeps_what_is_referenced() {
    let Some(bin) = rclone_bin() else { return };
    let remote_dir = tempfile::tempdir().unwrap();
    let a = Pc::new();
    let r: Arc<dyn Remote> = Arc::new(RcloneRemote::local(bin, remote_dir.path(), a.paths.tmp()));
    make_world(&a.paths.server(), 8);
    let se = a.engine(r.clone());
    let g = acquire(r.clone(), "s").await;
    let mut last: Option<String> = None;
    for i in 0..4u8 {
        std::fs::write(a.paths.world().join("region/r.5.0.mca"), vec![i; 20_000 + i as usize]).unwrap();
        let mut m = meta(ManifestKind::Session, &format!("sessao{i}"));
        m.session_started_at = Utc::now();
        mirror::mirror(&a.paths.server(), &a.paths.staging()).unwrap();
        let id = se.push(&a.paths.staging(), m, Some(&g), last.as_deref(), true, quiet()).await.unwrap().id;
        last = Some(id);
    }
    // lixo: objeto órfão de um envio interrompido
    std::fs::write(remote_dir.path().join("objects").join("f".repeat(64)), b"orfao").unwrap();

    let policy = Policy { last_sessions: 2, daily_days: 0 };
    let dry = retention::gc(&se, &g, policy, true).await.unwrap();
    assert_eq!(dry.manifests_deleted, 2);
    let rep = retention::gc(&se, &g, policy, false).await.unwrap();
    assert_eq!(rep.manifests_deleted, 2);
    assert!(rep.objects_deleted >= 3, "{rep:?}"); // 2 versões antigas de r.5.0 + órfão
    g.release().await.unwrap();

    // todo manifesto restante continua baixável e íntegro
    for id in se.list_manifest_ids().await.unwrap() {
        let b = Pc::new();
        let m = se.fetch_manifest(&id).await.unwrap();
        b.engine(r.clone()).materialize(&m, &b.paths.server(), quiet()).await.unwrap();
    }
}
