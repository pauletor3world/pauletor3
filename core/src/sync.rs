//! Sincronização atômica e versionada (o coração do sistema).
//!
//! Armazenamento ([`Remote`], a nuvem do mundo):
//! ```text
//! objects/<sha256>        cada arquivo do mundo, guardado uma vez, pelo hash
//! manifests/<id>.json     mapa caminho → objeto de uma versão completa
//! ```
//! O HEAD (qual manifesto é o oficial) mora na coordenação e só muda por CAS, **por último**,
//! depois de todos os objetos e do manifesto confirmados no armazenamento. Um envio
//! interrompido nunca altera o HEAD: o mundo oficial continua íntegro.
//!
//! Os objetos que faltam são buscados primeiro nos PCs dos amigos (P2P, ver
//! [`ObjectSource`]) e só depois na nuvem; o hash é conferido do mesmo jeito.

use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::{SyncPhase, SyncProgress, SyncProgressFn};
use crate::hashing;
use crate::coord::{Coordinator, Head};
use crate::lease::LeaseGuard;
use crate::manifest::{self, FileEntry, HashIndex, Manifest, ManifestKind, ManifestSummary, ServerVersions};
use crate::paths::Paths;
use crate::remote::{layout, ProgressFn, Remote, TransferProgress};
use crate::util;
use crate::{NmError, NmResult, FORMAT_VERSION};

/// Outra fonte de objetos e manifestos (os PCs dos amigos, por P2P).
#[async_trait::async_trait]
pub trait ObjectSource: Send + Sync {
    /// Bytes de um manifesto, se algum amigo tiver.
    async fn fetch_manifest(&self, id: &str) -> Option<Vec<u8>>;
    /// Baixa o que conseguir de `hashes` para `dir/<hash>` e devolve quais chegaram
    /// (o hash ainda é conferido por quem chamou).
    async fn fetch_objects(&self, hashes: &[String], dir: &Path, progress: ProgressFn) -> Vec<String>;
}

/// Estado local da cópia do mundo neste PC.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalState {
    /// Manifesto que a pasta `server/world` representa (quando não está suja).
    pub current_manifest: Option<String>,
    /// A pasta do mundo tem mudanças que ainda não viraram HEAD (servidor rodou/caiu).
    pub dirty: bool,
    pub active_session: Option<ActiveSession>,
    /// HEAD mais novo que este PC tem **inteiro e conferido** no cache (réplica local).
    #[serde(default)]
    pub replica: Option<ReplicaMark>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicaMark {
    pub seq: u64,
    pub manifest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSession {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    /// HEAD no momento em que a sessão começou.
    pub base_manifest: Option<String>,
    /// Último manifesto que esta sessão publicou como HEAD (checkpoint).
    pub last_published: Option<String>,
    /// Senha do RCON desta sessão (para parar com segurança um servidor que ficou órfão
    /// se o app cair). Fica só neste PC, na pasta do usuário.
    #[serde(default)]
    pub rcon_password: Option<String>,
    #[serde(default)]
    pub server_port: Option<u16>,
    /// `session_number` da vez desta sessão.
    #[serde(default)]
    pub session_number: Option<u64>,
}

impl LocalState {
    pub fn load(paths: &Paths) -> Self {
        std::fs::read(paths.state())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        util::write_atomic(&paths.state(), &serde_json::to_vec_pretty(self)?)
    }
}

/// Metadados de um envio.
#[derive(Debug, Clone)]
pub struct PushMeta {
    pub kind: ManifestKind,
    pub session_id: String,
    pub session_started_at: DateTime<Utc>,
    pub host_player: String,
    pub players: Vec<String>,
    pub versions: ServerVersions,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullOutcome {
    /// Não há mundo no remote ainda.
    NoWorld,
    UpToDate,
    Updated,
}

#[derive(Clone)]
pub struct SyncEngine {
    pub remote: Arc<dyn Remote>,
    pub coord: Arc<dyn Coordinator>,
    pub peers: Option<Arc<dyn ObjectSource>>,
    pub paths: Paths,
}

impl SyncEngine {
    pub fn new(remote: Arc<dyn Remote>, coord: Arc<dyn Coordinator>, paths: Paths) -> Self {
        Self { remote, coord, peers: None, paths }
    }

    pub fn with_peers(mut self, peers: Option<Arc<dyn ObjectSource>>) -> Self {
        self.peers = peers;
        self
    }

    // ------------------------------------------------------------------ leitura

    pub async fn head(&self) -> NmResult<Option<Head>> {
        let h = self.coord.head().await?;
        if let Some(h) = &h {
            if h.format > FORMAT_VERSION {
                return Err(NmError::FormatTooNew { found: h.format });
            }
            if !manifest::is_manifest_id(&h.manifest) {
                return Err(NmError::Integrity("HEAD aponta para um id inválido".into()));
            }
        }
        Ok(h)
    }

    /// Lê um manifesto (manifestos são imutáveis, então ficam em cache local).
    pub async fn fetch_manifest(&self, id: &str) -> NmResult<Manifest> {
        if !manifest::is_manifest_id(id) {
            return Err(NmError::Integrity(format!("id de manifesto inválido: {id}")));
        }
        let cache = self.paths.cache_manifests().join(format!("{id}.json"));
        if let Ok(b) = std::fs::read(&cache) {
            if let Ok(m) = serde_json::from_slice::<Manifest>(&b) {
                if m.id == id {
                    m.validate()?;
                    return Ok(m);
                }
            }
        }
        if let Some(p) = &self.peers {
            if let Some(bytes) = p.fetch_manifest(id).await {
                match parse_manifest(id, &bytes) {
                    Ok(m) => {
                        util::write_atomic(&cache, &bytes)?;
                        return Ok(m);
                    }
                    Err(e) => tracing::warn!(%id, error = %e, "manifesto recebido por P2P ignorado"),
                }
            }
        }
        let bytes = self
            .remote
            .read(&layout::manifest(id))
            .await?
            .ok_or_else(|| NmError::Integrity(format!("manifesto {id} não existe na nuvem")))?;
        let m = parse_manifest(id, &bytes)?;
        util::write_atomic(&cache, &bytes)?;
        Ok(m)
    }

    pub async fn list_manifest_ids(&self) -> NmResult<Vec<String>> {
        let mut ids: Vec<String> = self
            .remote
            .list(layout::MANIFESTS)
            .await?
            .into_iter()
            .filter_map(|e| e.name.strip_suffix(".json").map(str::to_string))
            .filter(|id| manifest::is_manifest_id(id))
            .collect();
        ids.sort();
        Ok(ids)
    }

    /// Resumos de todos os manifestos, do mais novo para o mais velho.
    pub async fn timeline(&self) -> NmResult<Vec<ManifestSummary>> {
        let mut out = Vec::new();
        for id in self.list_manifest_ids().await?.into_iter().rev() {
            match self.fetch_manifest(&id).await {
                Ok(m) => out.push(m.summary()),
                Err(e) => tracing::warn!(%id, error = %e, "manifesto ignorado na linha do tempo"),
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ envio

    /// Envia o snapshot em `snapshot_root` (a pasta de staging), cria um manifesto e, se
    /// `publish`, aponta o HEAD para ele. Exige a vez (`guard`) para a cerca de segurança.
    ///
    /// Sem `guard`, o manifesto é gravado solto (aparece na linha do tempo) e o HEAD nunca muda.
    /// `expected_head` é o manifesto que deve ser o HEAD atual; se o HEAD mudou por fora,
    /// o envio é publicado só como manifesto solto (aparece na linha do tempo) e o HEAD fica intacto.
    pub async fn push(
        &self,
        snapshot_root: &Path,
        meta: PushMeta,
        guard: Option<&LeaseGuard>,
        expected_head: Option<&str>,
        publish: bool,
        progress: SyncProgressFn,
    ) -> NmResult<Manifest> {
        // 1) varre e calcula hashes (usando o índice para pular o que não mudou)
        let root = snapshot_root.to_path_buf();
        let index_path = self.paths.hash_index();
        let p2 = progress.clone();
        let files = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let mut index = HashIndex::load(&index_path);
            let files = manifest::scan_files(&root, &mut index, |done, total| {
                p2(SyncProgress { phase: SyncPhase::Scanning, bytes_done: done, bytes_total: total, files_done: 0, files_total: 0 })
            })?;
            index.save(&index_path)?;
            Ok(files)
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))??;

        // 2) garante cada objeto no cache local (hardlink do staging; cópia se não der)
        let cache = self.paths.cache_objects();
        std::fs::create_dir_all(&cache)?;
        {
            let root = snapshot_root.to_path_buf();
            let files = files.clone();
            let cache = cache.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                for (rel, e) in &files {
                    let obj = cache.join(&e.hash);
                    if obj.exists() {
                        continue;
                    }
                    let src = util::join_rel(&root, rel)?;
                    let tmp = cache.join(format!(".{}.tmp-{}", e.hash, uuid::Uuid::new_v4().simple()));
                    if std::fs::hard_link(&src, &tmp).is_err() {
                        std::fs::copy(&src, &tmp)?;
                    }
                    util::replace_file(&tmp, &obj)?;
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
        }

        // 3) o que falta no remote
        let remote_objs: HashMap<String, u64> = self
            .remote
            .list(layout::OBJECTS)
            .await?
            .into_iter()
            .map(|e| (e.name, e.size))
            .collect();
        let mut sizes: HashMap<String, u64> = HashMap::new();
        for e in files.values() {
            sizes.insert(e.hash.clone(), e.size);
        }
        let missing: Vec<String> = sizes
            .iter()
            .filter(|(h, s)| remote_objs.get(*h) != Some(*s))
            .map(|(h, _)| h.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        // 4) confere de novo o hash do que vai subir (o nome do objeto TEM que ser o hash dele)
        {
            let cache = cache.clone();
            let missing = missing.clone();
            let p3 = progress.clone();
            let total: u64 = missing.iter().map(|h| sizes[h]).sum();
            tokio::task::spawn_blocking(move || -> NmResult<()> {
                let mut done = 0;
                for h in &missing {
                    let actual = hashing::sha256_file(&cache.join(h))?;
                    if &actual != h {
                        let _ = std::fs::remove_file(cache.join(h));
                        return Err(NmError::Integrity(format!(
                            "objeto local {h} está corrompido; tente de novo"
                        )));
                    }
                    let sz = std::fs::metadata(cache.join(h))?.len();
                    done += sz;
                    p3(SyncProgress { phase: SyncPhase::Verifying, bytes_done: done, bytes_total: total, files_done: 0, files_total: 0 });
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
        }

        // 5) envio em lote
        if !missing.is_empty() {
            let p4 = progress.clone();
            let pf: ProgressFn = Arc::new(move |t: TransferProgress| {
                p4(SyncProgress {
                    phase: SyncPhase::Uploading,
                    bytes_done: t.bytes_done,
                    bytes_total: t.bytes_total,
                    files_done: t.files_done,
                    files_total: t.files_total,
                })
            });
            self.remote.upload_many(&cache, &missing, layout::OBJECTS, pf).await?;
        }

        // 6) confirma que TODOS os objetos do manifesto estão no remote com o tamanho certo
        progress(SyncProgress { phase: SyncPhase::Publishing, bytes_done: 0, bytes_total: 1, files_done: 0, files_total: 0 });
        let after: HashMap<String, u64> = self
            .remote
            .list(layout::OBJECTS)
            .await?
            .into_iter()
            .map(|e| (e.name, e.size))
            .collect();
        for (h, s) in &sizes {
            if after.get(h) != Some(s) {
                return Err(NmError::Integrity(format!("objeto {h} não chegou inteiro ao remote")));
            }
        }

        // 7) manifesto
        let head = self.head().await?;
        let now = Utc::now();
        let total_size = files.values().map(|e| e.size).sum();
        let m = Manifest {
            format: FORMAT_VERSION,
            id: manifest::new_manifest_id(now, &meta.host_player),
            kind: meta.kind,
            created_at: now,
            parent: head.as_ref().map(|h| h.manifest.clone()),
            session_id: meta.session_id.clone(),
            session_started_at: meta.session_started_at,
            host_player: meta.host_player.clone(),
            host_machine: util::machine_name(),
            duration_secs: (now - meta.session_started_at).num_seconds().max(0),
            players: meta.players.clone(),
            versions: meta.versions.clone(),
            app_version: crate::APP_VERSION.to_string(),
            restored_from: None,
            note: meta.note.clone(),
            total_size,
            files,
        };
        self.write_manifest(&m).await?;

        // 8) HEAD — por último, com CAS na coordenação (vez atual + HEAD esperado)
        if publish {
            let Some(guard) = guard else {
                return Err(NmError::LostLock);
            };
            match guard.publish_head(expected_head, &m.id).await {
                Ok(h) => self.mark_replica(h.seq, &m.id)?,
                Err(NmError::HeadMoved { current }) => {
                    tracing::error!(?current, ?expected_head, "HEAD mudou por fora; manifesto fica solto");
                    return Err(NmError::Integrity(format!(
                        "o HEAD mudou durante a sessão; esta versão ficou salva como {} na linha do tempo",
                        m.id
                    )));
                }
                Err(e) => return Err(e),
            }
        }
        progress(SyncProgress { phase: SyncPhase::Publishing, bytes_done: 1, bytes_total: 1, files_done: 0, files_total: 0 });
        Ok(m)
    }

    async fn write_manifest(&self, m: &Manifest) -> NmResult<()> {
        m.validate()?;
        let bytes = serde_json::to_vec(m)?;
        self.remote.write(&layout::manifest(&m.id), &bytes).await?;
        let back = self
            .remote
            .read(&layout::manifest(&m.id))
            .await?
            .ok_or_else(|| NmError::Integrity("manifesto não apareceu no remote".into()))?;
        if hashing::sha256_bytes(&back) != hashing::sha256_bytes(&bytes) {
            return Err(NmError::Integrity("manifesto gravado não confere".into()));
        }
        util::write_atomic(&self.paths.cache_manifests().join(format!("{}.json", m.id)), &bytes)?;
        Ok(())
    }

    /// Registra que o cache local tem inteira a versão `seq` (só avança).
    fn mark_replica(&self, seq: u64, manifest: &str) -> NmResult<()> {
        let mut st = LocalState::load(&self.paths);
        if st.replica.as_ref().map(|r| r.seq < seq).unwrap_or(true) {
            st.replica = Some(ReplicaMark { seq, manifest: manifest.to_string() });
            st.save(&self.paths)?;
        }
        Ok(())
    }

    /// `seq` do HEAD mais novo que este PC tem inteiro.
    pub fn replica_seq(&self) -> u64 {
        LocalState::load(&self.paths).replica.map(|r| r.seq).unwrap_or(0)
    }

    /// Réplica em segundo plano: traz para o cache tudo o que o HEAD atual precisa, sem tocar
    /// no mundo local. Quando alguém clicar em Jogar, quase tudo já está no disco.
    pub async fn prefetch(&self, progress: SyncProgressFn) -> NmResult<Option<Head>> {
        let Some(head) = self.head().await? else { return Ok(None) };
        if self.replica_seq() >= head.seq {
            return Ok(Some(head));
        }
        let m = self.fetch_manifest(&head.manifest).await?;
        self.fetch_objects(&m, progress).await?;
        self.mark_replica(head.seq, &head.manifest)?;
        Ok(Some(head))
    }

    /// Restaura uma versão antiga: cria um manifesto novo (cópia) e aponta o HEAD para ele.
    /// A história nunca é apagada.
    pub async fn restore(&self, target_id: &str, guard: &LeaseGuard, player: &str) -> NmResult<Manifest> {
        let target = self.fetch_manifest(target_id).await?;
        let remote_objs: HashMap<String, u64> = self
            .remote
            .list(layout::OBJECTS)
            .await?
            .into_iter()
            .map(|e| (e.name, e.size))
            .collect();
        for e in target.files.values() {
            if remote_objs.get(&e.hash) != Some(&e.size) {
                return Err(NmError::Integrity(format!(
                    "a versão {target_id} está incompleta no remote (falta {})",
                    &e.hash[..12]
                )));
            }
        }
        let head = self.head().await?;
        let now = Utc::now();
        let info = guard.info().await;
        let expected = head.as_ref().map(|h| h.manifest.clone());
        let m = Manifest {
            format: FORMAT_VERSION,
            id: manifest::new_manifest_id(now, player),
            kind: ManifestKind::Restore,
            created_at: now,
            parent: head.as_ref().map(|h| h.manifest.clone()),
            session_id: info.session_id.clone(),
            session_started_at: now,
            host_player: player.to_string(),
            host_machine: util::machine_name(),
            duration_secs: 0,
            players: vec![],
            versions: target.versions.clone(),
            app_version: crate::APP_VERSION.to_string(),
            restored_from: Some(target_id.to_string()),
            note: Some(format!("Restauração de {}", target.created_at.format("%d/%m/%Y %H:%M"))),
            total_size: target.total_size,
            files: target.files.clone(),
        };
        self.write_manifest(&m).await?;
        guard.publish_head(expected.as_deref(), &m.id).await?;
        Ok(m)
    }

    // ------------------------------------------------------------------ download

    /// Garante que `server/` contém exatamente o HEAD. Nunca toca num mundo local sujo.
    pub async fn pull(&self, progress: SyncProgressFn) -> NmResult<PullOutcome> {
        let state = LocalState::load(&self.paths);
        if state.dirty {
            return Err(NmError::Integrity(
                "o mundo local tem mudanças não enviadas; a recuperação precisa rodar antes".into(),
            ));
        }
        let Some(head) = self.head().await? else { return Ok(PullOutcome::NoWorld) };
        if state.current_manifest.as_deref() == Some(head.manifest.as_str()) && self.paths.world().join("level.dat").exists() {
            return Ok(PullOutcome::UpToDate);
        }
        let m = self.fetch_manifest(&head.manifest).await?;
        self.materialize(&m, &self.paths.server(), progress).await?;
        let mut st = LocalState::load(&self.paths);
        st.current_manifest = Some(m.id.clone());
        st.dirty = false;
        st.save(&self.paths)?;
        self.mark_replica(head.seq, &head.manifest)?;
        Ok(PullOutcome::Updated)
    }

    /// Baixa o que falta para o cache, confere hashes e monta `target_server_dir` de forma atômica.
    pub async fn materialize(&self, m: &Manifest, target_server_dir: &Path, progress: SyncProgressFn) -> NmResult<()> {
        self.fetch_objects(m, progress.clone()).await?;

        let staging_parent = self.paths.incoming();
        std::fs::create_dir_all(&staging_parent)?;
        let assembly = staging_parent.join(format!("montagem-{}", uuid::Uuid::new_v4().simple()));
        let cache = self.paths.cache_objects();
        let m2 = m.clone();
        let asm = assembly.clone();
        let p2 = progress.clone();
        let assembled = tokio::task::spawn_blocking(move || assemble(&m2, &cache, &asm, p2))
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        if let Err(e) = assembled {
            let _ = util::remove_dir_all_retry(&assembly);
            return Err(e);
        }

        let target = target_server_dir.to_path_buf();
        let asm = assembly.clone();
        let recovered = self.paths.recovered();
        tokio::task::spawn_blocking(move || swap_into(&asm, &target, &recovered))
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
        let _ = util::remove_dir_all_retry(&assembly);

        // O índice de hash já pode conhecer estes arquivos (mtime foi preservado).
        let mut index = HashIndex::load(&self.paths.hash_index());
        for (rel, e) in &m.files {
            index.insert(rel, e.clone());
        }
        index.save(&self.paths.hash_index())?;
        Ok(())
    }

    /// Baixa para o cache os objetos que faltam, conferindo o hash de cada um.
    async fn fetch_objects(&self, m: &Manifest, progress: SyncProgressFn) -> NmResult<()> {
        let cache = self.paths.cache_objects();
        std::fs::create_dir_all(&cache)?;
        let mut needed: BTreeSet<String> = BTreeSet::new();
        for e in m.files.values() {
            let p = cache.join(&e.hash);
            let ok = std::fs::metadata(&p).map(|md| md.len() == e.size).unwrap_or(false);
            if !ok {
                needed.insert(e.hash.clone());
            }
        }
        if needed.is_empty() {
            return Ok(());
        }
        let landing = self.paths.incoming().join(format!("objetos-{}", uuid::Uuid::new_v4().simple()));
        // Primeiro os amigos (P2P): rápido e sem gastar a cota da nuvem.
        if let Some(peers) = &self.peers {
            std::fs::create_dir_all(&landing)?;
            let list: Vec<String> = needed.iter().cloned().collect();
            let sizes: HashMap<String, u64> = m.files.values().map(|e| (e.hash.clone(), e.size)).collect();
            let total: u64 = list.iter().map(|h| sizes.get(h).copied().unwrap_or(0)).sum();
            let p = progress.clone();
            let pf: ProgressFn = Arc::new(move |t: TransferProgress| {
                p(SyncProgress {
                    phase: SyncPhase::Downloading,
                    bytes_done: t.bytes_done,
                    bytes_total: total,
                    files_done: t.files_done,
                    files_total: t.files_total,
                })
            });
            let got = peers.fetch_objects(&list, &landing, pf).await;
            let landing2 = landing.clone();
            let cache2 = cache.clone();
            let good = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
                let mut good = vec![];
                for h in got {
                    let f = landing2.join(&h);
                    match hashing::sha256_file(&f) {
                        Ok(actual) if actual == h => {
                            util::replace_file(&f, &cache2.join(&h))?;
                            good.push(h);
                        }
                        _ => {
                            tracing::warn!(hash = %h, "objeto recebido por P2P com hash errado; descartado");
                            let _ = std::fs::remove_file(&f);
                        }
                    }
                }
                Ok(good)
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
            for h in good {
                needed.remove(&h);
            }
            if needed.is_empty() {
                let _ = util::remove_dir_all_retry(&landing);
                return Ok(());
            }
            tracing::info!(faltam = needed.len(), "buscando na nuvem o que os amigos não tinham");
        }
        let result = async {
            for attempt in 0..2 {
                let list: Vec<String> = needed.iter().cloned().collect();
                let p = progress.clone();
                let pf: ProgressFn = Arc::new(move |t: TransferProgress| {
                    p(SyncProgress {
                        phase: SyncPhase::Downloading,
                        bytes_done: t.bytes_done,
                        bytes_total: t.bytes_total,
                        files_done: t.files_done,
                        files_total: t.files_total,
                    })
                });
                self.remote.download_many(layout::OBJECTS, &list, &landing, pf).await?;
                let landing2 = landing.clone();
                let cache2 = cache.clone();
                let p = progress.clone();
                let bad = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
                    let mut bad = vec![];
                    let n = list.len() as u64;
                    for (i, h) in list.iter().enumerate() {
                        let f = landing2.join(h);
                        match hashing::sha256_file(&f) {
                            Ok(actual) if &actual == h => util::replace_file(&f, &cache2.join(h))?,
                            _ => {
                                let _ = std::fs::remove_file(&f);
                                bad.push(h.clone());
                            }
                        }
                        p(SyncProgress { phase: SyncPhase::Verifying, bytes_done: 0, bytes_total: 0, files_done: i as u64 + 1, files_total: n });
                    }
                    Ok(bad)
                })
                .await
                .map_err(|e| anyhow::anyhow!(e))??;
                if bad.is_empty() {
                    return Ok(());
                }
                tracing::warn!(attempt, count = bad.len(), "objetos baixados com hash errado; baixando de novo");
                needed = bad.into_iter().collect();
            }
            Err(NmError::Integrity(format!(
                "{} arquivo(s) continuam chegando corrompidos; o mundo local não foi tocado",
                needed.len()
            )))
        }
        .await;
        let _ = util::remove_dir_all_retry(&landing);
        result
    }

    /// Objetos presentes no remote (nome → tamanho).
    pub async fn remote_objects(&self) -> NmResult<HashMap<String, u64>> {
        Ok(self.remote.list(layout::OBJECTS).await?.into_iter().map(|e| (e.name, e.size)).collect())
    }

    /// Hash dos arquivos sincronizáveis de uma pasta de servidor (para comparar PCs).
    pub fn fingerprint(server_dir: &Path) -> anyhow::Result<std::collections::BTreeMap<String, FileEntry>> {
        let mut idx = HashIndex::default();
        manifest::scan_files(server_dir, &mut idx, |_, _| {})
    }
}

fn parse_manifest(id: &str, bytes: &[u8]) -> NmResult<Manifest> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| NmError::Integrity(format!("manifesto {id} ilegível: {e}")))?;
    if m.id != id {
        return Err(NmError::Integrity(format!("manifesto {id} tem id {}", m.id)));
    }
    m.validate()?;
    Ok(m)
}

/// Copia cada arquivo do cache para a montagem, conferindo o hash durante a cópia.
fn assemble(m: &Manifest, cache: &Path, out: &Path, progress: SyncProgressFn) -> NmResult<()> {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(out)?;
    for d in manifest::SYNCED_DIRS {
        std::fs::create_dir_all(out.join(d))?;
    }
    let total = m.total_size;
    let mut done = 0u64;
    let n = m.files.len() as u64;
    for (i, (rel, e)) in m.files.iter().enumerate() {
        let dst = util::join_rel(out, rel)?;
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut src = std::fs::File::open(cache.join(&e.hash))
            .map_err(|err| NmError::Integrity(format!("objeto {} ausente no cache: {err}", &e.hash[..12])))?;
        let mut f = std::fs::File::create(&dst)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut written = 0u64;
        loop {
            let k = src.read(&mut buf)?;
            if k == 0 {
                break;
            }
            h.update(&buf[..k]);
            f.write_all(&buf[..k])?;
            written += k as u64;
        }
        f.sync_all()?;
        if written != e.size || hex::encode(h.finalize()) != e.hash {
            let _ = std::fs::remove_file(cache.join(&e.hash));
            return Err(NmError::Integrity(format!("{rel} não confere com o manifesto")));
        }
        if e.mtime > 0 {
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(e.mtime as u64);
            let _ = f.set_modified(t);
        }
        done += e.size;
        progress(SyncProgress { phase: SyncPhase::Assembling, bytes_done: done, bytes_total: total, files_done: i as u64 + 1, files_total: n });
    }
    Ok(())
}

/// Troca as partes sincronizadas de `target` pelas da montagem.
/// A pasta `world` é trocada por renomeação; a antiga só é apagada depois que a nova está no lugar.
fn swap_into(assembly: &Path, target: &Path, recovered: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    for d in manifest::SYNCED_DIRS {
        let new = assembly.join(d);
        let cur = target.join(d);
        let old = target.join(format!("{d}.antigo-{stamp}"));
        if cur.exists() {
            util::rename_dir(&cur, &old)?;
        }
        if let Err(e) = util::rename_dir(&new, &cur) {
            // desfaz
            if old.exists() {
                let _ = util::rename_dir(&old, &cur);
            }
            return Err(e);
        }
        if old.exists() {
            if let Err(e) = util::remove_dir_all_retry(&old) {
                tracing::warn!(error = %e, "não consegui apagar a pasta antiga; movendo para recovered/");
                let _ = std::fs::create_dir_all(recovered);
                let _ = util::rename_dir(&old, &recovered.join(format!("{d}-{stamp}")));
            }
        }
    }
    for f in manifest::SYNCED_FILES {
        let new = assembly.join(f);
        let cur = target.join(f);
        if new.exists() {
            util::replace_file(&new, &cur)?;
        } else if cur.exists() {
            std::fs::remove_file(&cur)?;
        }
    }
    Ok(())
}

/// Ao abrir o app: se uma troca foi interrompida (queda de energia no meio), conserta.
pub fn repair_interrupted_swap(paths: &Paths) -> anyhow::Result<()> {
    let server = paths.server();
    let Ok(rd) = std::fs::read_dir(&server) else { return Ok(()) };
    let mut olds: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("world.antigo-")).unwrap_or(false))
        .collect();
    olds.sort();
    let world = paths.world();
    if !world.exists() {
        if let Some(last) = olds.pop() {
            tracing::warn!("troca do mundo foi interrompida; devolvendo a pasta anterior");
            util::rename_dir(&last, &world)?;
        }
    }
    for o in olds {
        let dest = paths.recovered().join(o.file_name().unwrap());
        let _ = util::rename_dir(&o, &dest);
    }
    let _ = util::remove_dir_all_retry(&paths.incoming());
    Ok(())
}
