use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};

/// Escreve um arquivo de forma atômica: grava num temporário ao lado e renomeia.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path.parent().context("caminho sem pasta pai")?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("arquivo"),
        uuid::Uuid::new_v4().simple()
    ));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    replace_file(&tmp, path)?;
    Ok(())
}

/// Renomeia `from` para `to`, substituindo `to` se existir.
pub fn replace_file(from: &Path, to: &Path) -> anyhow::Result<()> {
    // No Windows, std::fs::rename usa MoveFileExW com MOVEFILE_REPLACE_EXISTING.
    // Repetimos algumas vezes porque antivírus/indexador às vezes seguram o arquivo por um instante.
    let mut last = None;
    for attempt in 0..8 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap()).with_context(|| format!("não consegui substituir {}", to.display()))
}

/// Renomeia uma pasta, com novas tentativas (Windows segura handles por instantes).
pub fn rename_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    let mut last = None;
    for attempt in 0..20 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap())
        .with_context(|| format!("não consegui renomear {} para {}", from.display(), to.display()))
}

pub fn remove_dir_all_retry(p: &Path) -> anyhow::Result<()> {
    if !p.exists() {
        return Ok(());
    }
    let mut last = None;
    for attempt in 0..10 {
        match std::fs::remove_dir_all(p) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap()).with_context(|| format!("não consegui apagar {}", p.display()))
}

/// Caminho relativo com `/`, independente do sistema.
pub fn rel_slash(root: &Path, p: &Path) -> anyhow::Result<String> {
    let rel = p.strip_prefix(root)?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

/// Converte um caminho relativo com `/` num `PathBuf` sob `root`, recusando `..` e absolutos.
pub fn join_rel(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let mut out = root.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') || part.contains('\\') {
            anyhow::bail!("caminho inválido no manifesto: {rel}");
        }
        out.push(part);
    }
    Ok(out)
}

/// Data de modificação em nanossegundos desde a época Unix (o NTFS tem resolução de 100 ns;
/// segundos não bastam, porque uma região pode ser regravada no mesmo segundo com o mesmo tamanho).
pub fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i]).replace('.', ",")
    }
}

pub fn human_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    match (h, m) {
        (0, 0) => "menos de 1 min".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Nome da máquina (para exibir quem está hospedando).
pub fn machine_name() -> String {
    hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "PC".into())
}

/// Espaço livre no volume que contém `p`.
pub fn free_space(p: &Path) -> Option<u64> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let canon = std::fs::canonicalize(p).ok().unwrap_or_else(|| p.to_path_buf());
    let s = canon.to_string_lossy().to_lowercase().replace("\\\\?\\", "");
    disks
        .iter()
        .filter(|d| s.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// `tokio::process::Command` que não abre janela de console no Windows.
pub fn command(bin: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut c = tokio::process::Command::new(bin);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.kill_on_drop(false);
    c
}

/// Mesmo que [`command`], mas síncrono.
pub fn std_command(bin: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Senha aleatória alfanumérica.
pub fn random_token(len: usize) -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    rand::thread_rng().sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

/// Copia uma pasta inteira (para quando `rename` não serve, como entre discos diferentes).
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir_all(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_rel_rejects_traversal() {
        let root = Path::new("/r");
        assert!(join_rel(root, "../x").is_err());
        assert!(join_rel(root, "a/../../x").is_err());
        assert!(join_rel(root, "C:/x").is_err());
        assert!(join_rel(root, "a\\b").is_err());
        assert!(join_rel(root, "").is_err());
        assert_eq!(join_rel(root, "world/region/r.0.0.mca").unwrap(), root.join("world").join("region").join("r.0.0.mca"));
    }

    #[test]
    fn write_atomic_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.json");
        write_atomic(&p, b"1").unwrap();
        write_atomic(&p, b"2").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"2");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn humanizes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1,5 KB");
        assert_eq!(human_duration(3720), "1 h 2 min");
        assert_eq!(human_duration(30), "menos de 1 min");
    }
}

