//! Manifestos: o mapa caminho → objeto de uma versão completa do mundo.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::hashing;
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    /// Fim normal de sessão.
    Session,
    /// Salvamento periódico durante a partida.
    Checkpoint,
    /// Enviado depois de uma queda (energia, travamento).
    Recovered,
    /// Restauração de uma versão antiga (cópia, a história nunca é apagada).
    Restore,
    /// Mundo importado de uma pasta existente.
    Import,
    /// Ajuste feito pelo app com ninguém jogando (ex.: vincular progresso de jogador).
    Edit,
}

impl ManifestKind {
    pub fn label(self) -> &'static str {
        match self {
            ManifestKind::Session => "Sessão",
            ManifestKind::Checkpoint => "Salvamento automático",
            ManifestKind::Recovered => "Recuperado após queda",
            ManifestKind::Restore => "Restauração",
            ManifestKind::Import => "Importação",
            ManifestKind::Edit => "Ajuste de jogadores",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub hash: String,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerVersions {
    pub minecraft: String,
    pub fabric_loader: String,
    pub lithium: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub id: String,
    pub kind: ManifestKind,
    pub created_at: DateTime<Utc>,
    /// Manifesto que era o HEAD quando este foi criado.
    pub parent: Option<String>,
    pub session_id: String,
    pub session_started_at: DateTime<Utc>,
    pub host_player: String,
    pub host_machine: String,
    pub duration_secs: i64,
    #[serde(default)]
    pub players: Vec<String>,
    #[serde(default)]
    pub versions: ServerVersions,
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub total_size: u64,
    pub files: BTreeMap<String, FileEntry>,
}

impl Manifest {
    pub fn object_hashes(&self) -> BTreeSet<String> {
        self.files.values().map(|f| f.hash.clone()).collect()
    }

    pub fn summary(&self) -> ManifestSummary {
        ManifestSummary {
            id: self.id.clone(),
            kind: self.kind,
            created_at: self.created_at,
            session_id: self.session_id.clone(),
            host_player: self.host_player.clone(),
            duration_secs: self.duration_secs,
            players: self.players.clone(),
            total_size: self.total_size,
            file_count: self.files.len(),
            restored_from: self.restored_from.clone(),
            note: self.note.clone(),
        }
    }

    /// Validação estrutural: formato conhecido, caminhos seguros, hashes válidos, tamanho coerente.
    pub fn validate(&self) -> crate::NmResult<()> {
        if self.format > crate::FORMAT_VERSION {
            return Err(crate::NmError::FormatTooNew { found: self.format });
        }
        let mut total = 0u64;
        for (p, e) in &self.files {
            util::join_rel(Path::new("x"), p).map_err(|e| crate::NmError::Integrity(e.to_string()))?;
            if !hashing::is_object_hash(&e.hash) {
                return Err(crate::NmError::Integrity(format!("hash inválido para {p}")));
            }
            total += e.size;
        }
        if total != self.total_size {
            return Err(crate::NmError::Integrity("tamanho total do manifesto não confere".into()));
        }
        Ok(())
    }
}

/// O que a linha do tempo mostra (sem o mapa de arquivos).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSummary {
    pub id: String,
    pub kind: ManifestKind,
    pub created_at: DateTime<Utc>,
    pub session_id: String,
    pub host_player: String,
    pub duration_secs: i64,
    pub players: Vec<String>,
    pub total_size: u64,
    pub file_count: usize,
    pub restored_from: Option<String>,
    pub note: Option<String>,
}

/// Gera um id ordenável: `20261015T221000Z-vitor-1a2b3c`.
pub fn new_manifest_id(at: DateTime<Utc>, player: &str) -> String {
    let slug: String = player
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(16)
        .collect::<String>()
        .to_lowercase();
    let slug = if slug.is_empty() { "host".to_string() } else { slug };
    let rnd = &uuid::Uuid::new_v4().simple().to_string()[..6];
    format!("{}-{slug}-{rnd}", at.format("%Y%m%dT%H%M%SZ"))
}

pub fn is_manifest_id(s: &str) -> bool {
    !s.is_empty() && s.len() < 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// O que entra no mundo sincronizado, relativo à pasta do servidor.
pub const SYNCED_DIRS: &[&str] = &["world"];
pub const SYNCED_FILES: &[&str] = &[
    "ops.json",
    "whitelist.json",
    "banned-players.json",
    "banned-ips.json",
    "usercache.json",
];

/// Arquivos que nunca entram (travas do servidor, temporários).
pub fn is_excluded(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name == "session.lock"
        || name.ends_with(".tmp")
        || name.starts_with(".nm-tmp")
        || name.ends_with("_old.tmp")
}

/// Lista os arquivos sincronizáveis sob `root` (pasta do servidor ou do staging).
pub fn list_synced_files(root: &Path) -> anyhow::Result<Vec<(String, PathBuf, std::fs::Metadata)>> {
    let mut out = Vec::new();
    for d in SYNCED_DIRS {
        let base = root.join(d);
        if !base.exists() {
            continue;
        }
        for ent in walkdir::WalkDir::new(&base).follow_links(false) {
            let ent = ent?;
            if !ent.file_type().is_file() {
                continue;
            }
            let rel = util::rel_slash(root, ent.path())?;
            if is_excluded(&rel) {
                continue;
            }
            let meta = ent.metadata()?;
            out.push((rel, ent.path().to_path_buf(), meta));
        }
    }
    for f in SYNCED_FILES {
        let p = root.join(f);
        if let Ok(meta) = std::fs::metadata(&p) {
            if meta.is_file() {
                out.push((f.to_string(), p, meta));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Índice local (caminho, tamanho, mtime) → hash, para não recalcular hash de região que não mudou.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HashIndex {
    entries: HashMap<String, FileEntry>,
}

impl HashIndex {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        util::write_atomic(path, &serde_json::to_vec(self)?)
    }
    pub fn lookup(&self, rel: &str, size: u64, mtime: i64) -> Option<&str> {
        self.entries
            .get(rel)
            .filter(|e| e.size == size && e.mtime == mtime && mtime != 0)
            .map(|e| e.hash.as_str())
    }
    pub fn insert(&mut self, rel: &str, e: FileEntry) {
        self.entries.insert(rel.to_string(), e);
    }
    pub fn retain_paths(&mut self, keep: &BTreeSet<String>) {
        self.entries.retain(|k, _| keep.contains(k));
    }
}

/// Calcula o mapa de arquivos de `root`, usando o índice para pular arquivos inalterados.
/// `progress(done, total)` é chamado a cada arquivo.
pub fn scan_files(
    root: &Path,
    index: &mut HashIndex,
    mut progress: impl FnMut(u64, u64),
) -> anyhow::Result<BTreeMap<String, FileEntry>> {
    let files = list_synced_files(root)?;
    let total_bytes: u64 = files.iter().map(|f| f.2.len()).sum();
    let mut done = 0u64;
    let mut out = BTreeMap::new();
    for (rel, path, meta) in files {
        let size = meta.len();
        let mtime = util::mtime_ns(&meta);
        let hash = match index.lookup(&rel, size, mtime) {
            Some(h) => h.to_string(),
            None => {
                let h = hashing::sha256_file(&path).with_context(|| format!("lendo {}", path.display()))?;
                // Se o arquivo mudou enquanto líamos, o tamanho não bate: recusa.
                let after = std::fs::metadata(&path)?;
                if after.len() != size || util::mtime_ns(&after) != mtime {
                    anyhow::bail!("{rel} mudou durante a leitura; tente de novo");
                }
                index.insert(&rel, FileEntry { hash: h.clone(), size, mtime });
                h
            }
        };
        done += size;
        progress(done, total_bytes);
        out.insert(rel, FileEntry { hash, size, mtime });
    }
    let keep: BTreeSet<String> = out.keys().cloned().collect();
    index.retain_paths(&keep);
    Ok(out)
}

/// Diferença entre dois mapas de arquivos.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

pub fn diff(old: &BTreeMap<String, FileEntry>, new: &BTreeMap<String, FileEntry>) -> Diff {
    let mut d = Diff::default();
    for (k, v) in new {
        match old.get(k) {
            None => d.added.push(k.clone()),
            Some(o) if o.hash != v.hash => d.changed.push(k.clone()),
            _ => {}
        }
    }
    for k in old.keys() {
        if !new.contains_key(k) {
            d.removed.push(k.clone());
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(h: char, size: u64) -> FileEntry {
        FileEntry { hash: std::iter::repeat_n(h, 64).collect(), size, mtime: 1 }
    }

    #[test]
    fn diff_detects_changes() {
        let mut a = BTreeMap::new();
        a.insert("world/a".into(), entry('a', 1));
        a.insert("world/b".into(), entry('b', 1));
        let mut b = BTreeMap::new();
        b.insert("world/a".into(), entry('a', 1));
        b.insert("world/b".into(), entry('c', 1));
        b.insert("world/n".into(), entry('d', 1));
        let mut a2 = a.clone();
        a2.insert("world/gone".into(), entry('e', 1));
        let d = diff(&a2, &b);
        assert_eq!(d.added, vec!["world/n"]);
        assert_eq!(d.changed, vec!["world/b"]);
        assert_eq!(d.removed, vec!["world/gone"]);
    }

    #[test]
    fn scan_uses_index_and_excludes_lock() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("world/region")).unwrap();
        std::fs::write(root.join("world/level.dat"), b"level").unwrap();
        std::fs::write(root.join("world/region/r.0.0.mca"), b"region").unwrap();
        std::fs::write(root.join("world/session.lock"), b"lock").unwrap();
        std::fs::write(root.join("ops.json"), b"[]").unwrap();
        std::fs::write(root.join("server.properties"), b"nao entra").unwrap();
        let mut idx = HashIndex::default();
        let files = scan_files(root, &mut idx, |_, _| {}).unwrap();
        let keys: Vec<_> = files.keys().cloned().collect();
        assert_eq!(keys, vec!["ops.json", "world/level.dat", "world/region/r.0.0.mca"]);
        assert_eq!(files["world/level.dat"].hash, hashing::sha256_bytes(b"level"));
        // Segunda varredura usa o índice (mesmo resultado).
        let again = scan_files(root, &mut idx, |_, _| {}).unwrap();
        assert_eq!(files, again);
    }

    #[test]
    fn validate_rejects_bad_paths() {
        let mut files = BTreeMap::new();
        files.insert("../evil".to_string(), entry('a', 3));
        let m = Manifest {
            format: 1,
            id: "x".into(),
            kind: ManifestKind::Session,
            created_at: Utc::now(),
            parent: None,
            session_id: "s".into(),
            session_started_at: Utc::now(),
            host_player: "a".into(),
            host_machine: "b".into(),
            duration_secs: 0,
            players: vec![],
            versions: Default::default(),
            app_version: "0".into(),
            restored_from: None,
            note: None,
            total_size: 3,
            files,
        };
        assert!(m.validate().is_err());
        let mut ok = m.clone();
        ok.files = BTreeMap::from([("world/a".to_string(), entry('a', 3))]);
        assert!(ok.validate().is_ok());
        let mut newer = ok.clone();
        newer.format = 99;
        assert!(matches!(newer.validate(), Err(crate::NmError::FormatTooNew { found: 99 })));
    }

    #[test]
    fn ids_sort_by_time() {
        let a = new_manifest_id("2026-01-01T00:00:00Z".parse().unwrap(), "Zé Ninguém");
        let b = new_manifest_id("2026-01-02T00:00:00Z".parse().unwrap(), "abc");
        assert!(a < b);
        assert!(is_manifest_id(&a));
        assert!(a.contains("-zningum-"), "{a}");
        assert!(!is_manifest_id("../x"));
    }
}
