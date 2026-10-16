//! Espelho rápido do mundo para a pasta de staging.
//!
//! Roda durante o `save-off`, então precisa ser rápido: só copia arquivos cujo tamanho
//! ou mtime mudaram. Cada cópia vai para um temporário e é renomeada, então um arquivo
//! já existente no staging nunca é modificado no lugar (isso permite que o cache de
//! objetos use hardlinks para ele com segurança).

use std::collections::BTreeSet;
use std::path::Path;

use crate::manifest;
use crate::util;

#[derive(Debug, Default, Clone, Copy)]
pub struct MirrorStats {
    pub copied: u64,
    pub copied_bytes: u64,
    pub removed: u64,
    pub unchanged: u64,
}

pub fn mirror(src_root: &Path, dst_root: &Path) -> anyhow::Result<MirrorStats> {
    let mut st = MirrorStats::default();
    std::fs::create_dir_all(dst_root)?;
    let src = manifest::list_synced_files(src_root)?;
    let mut keep = BTreeSet::new();
    for (rel, path, meta) in &src {
        keep.insert(rel.clone());
        let dst = util::join_rel(dst_root, rel)?;
        let same = std::fs::metadata(&dst)
            .map(|d| d.len() == meta.len() && util::mtime_ns(&d) == util::mtime_ns(meta))
            .unwrap_or(false);
        if same {
            st.unchanged += 1;
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = dst.with_file_name(format!(
            ".nm-tmp-{}",
            uuid::Uuid::new_v4().simple()
        ));
        // std::fs::copy no Windows usa CopyFileExW, que preserva a data de modificação.
        std::fs::copy(path, &tmp)?;
        set_mtime_like(&tmp, meta)?;
        util::replace_file(&tmp, &dst)?;
        st.copied += 1;
        st.copied_bytes += meta.len();
    }
    for (rel, path, _) in manifest::list_synced_files(dst_root)? {
        if !keep.contains(&rel) {
            std::fs::remove_file(&path)?;
            st.removed += 1;
        }
    }
    Ok(st)
}

/// Garante que o mtime do destino é igual ao da origem (em outros sistemas `copy` não preserva).
fn set_mtime_like(p: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    if let Ok(m) = meta.modified() {
        let f = std::fs::OpenOptions::new().write(true).open(p)?;
        f.set_modified(m)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_changes_and_deletions() {
        let s = tempfile::tempdir().unwrap();
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(s.path().join("world/region")).unwrap();
        std::fs::write(s.path().join("world/level.dat"), b"1").unwrap();
        std::fs::write(s.path().join("world/region/a.mca"), b"a").unwrap();
        std::fs::write(s.path().join("world/session.lock"), b"x").unwrap();
        let st = mirror(s.path(), d.path()).unwrap();
        assert_eq!(st.copied, 2);
        assert!(!d.path().join("world/session.lock").exists());

        let st = mirror(s.path(), d.path()).unwrap();
        assert_eq!(st.copied, 0);
        assert_eq!(st.unchanged, 2);

        std::fs::remove_file(s.path().join("world/region/a.mca")).unwrap();
        std::fs::write(s.path().join("world/level.dat"), b"22").unwrap();
        let st = mirror(s.path(), d.path()).unwrap();
        assert_eq!((st.copied, st.removed), (1, 1));
        assert_eq!(std::fs::read(d.path().join("world/level.dat")).unwrap(), b"22");
        assert!(!d.path().join("world/region/a.mca").exists());
    }
}
