//! JRE Temurin 21 baixado pelo próprio app (não depende de Java instalado no sistema).

use std::path::{Path, PathBuf};

use futures_util::StreamExt;

use crate::events::{SyncPhase, SyncProgress, SyncProgressFn};
use crate::paths::Paths;
use crate::versions::{PinnedFile, Versions};
use crate::{NmError, NmResult};

/// Baixa `pinned` para `dest` (via temporário) e confere o hash. Reaproveita se já existir e conferir.
pub async fn download_verified(pinned: &PinnedFile, dest: &Path, progress: SyncProgressFn) -> NmResult<()> {
    if dest.exists() && pinned.verify(dest).is_ok() {
        return Ok(());
    }
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = dest.with_extension("baixando");
    let client = reqwest::Client::builder()
        .user_agent(concat!("MachenCraft/", env!("CARGO_PKG_VERSION"), " (github.com/pauletor3world)"))
        .build()
        .map_err(|e| NmError::Other(e.into()))?;
    let resp = client
        .get(&pinned.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| NmError::Remote(format!("baixando {}: {e}", pinned.filename)))?;
    let total = resp.content_length().or(pinned.size).unwrap_or(0);
    let mut f = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    let mut done = 0u64;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| NmError::Remote(format!("baixando {}: {e}", pinned.filename)))?;
        f.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(SyncProgress { phase: SyncPhase::Downloading, bytes_done: done, bytes_total: total, files_done: 0, files_total: 1 });
    }
    f.sync_all().await?;
    drop(f);
    let tmp2 = tmp.clone();
    let p2 = pinned.clone();
    let check = tokio::task::spawn_blocking(move || p2.verify(&tmp2)).await.map_err(|e| anyhow::anyhow!(e))?;
    if let Err(e) = check {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    crate::util::replace_file(&tmp, dest)?;
    Ok(())
}

pub fn java_dir(paths: &Paths, v: &Versions) -> PathBuf {
    let safe: String = v.java_release.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    paths.runtime().join(format!("java-{}", safe))
}

/// Caminho do `java.exe` se já instalado.
pub fn find_java(paths: &Paths, v: &Versions) -> Option<PathBuf> {
    let root = java_dir(paths, v);
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let direct = root.join("bin").join(exe);
    if direct.exists() {
        return Some(direct);
    }
    // O zip da Adoptium tem uma pasta de topo (ex.: jdk-21.0.12.1+1-jre).
    std::fs::read_dir(&root).ok()?.filter_map(|e| e.ok()).map(|e| e.path().join("bin").join(exe)).find(|p| p.exists())
}

/// Garante o Java instalado, baixando e conferindo o checksum se precisar.
pub async fn ensure_java(paths: &Paths, v: &Versions, progress: SyncProgressFn) -> NmResult<PathBuf> {
    if let Some(j) = find_java(paths, v) {
        return Ok(j);
    }
    if !cfg!(windows) {
        // Fora do Windows (CI/testes), usa o Java do sistema se houver.
        return Ok(PathBuf::from("java"));
    }
    let zip_path = paths.downloads().join(&v.java_windows_x64.filename);
    download_verified(&v.java_windows_x64, &zip_path, progress).await?;
    let dest = java_dir(paths, v);
    let tmp = dest.with_extension("extraindo");
    let _ = crate::util::remove_dir_all_retry(&tmp);
    let zp = zip_path.clone();
    let t2 = tmp.clone();
    tokio::task::spawn_blocking(move || extract_zip(&zp, &t2)).await.map_err(|e| anyhow::anyhow!(e))??;
    let _ = crate::util::remove_dir_all_retry(&dest);
    crate::util::rename_dir(&tmp, &dest)?;
    let _ = std::fs::remove_file(&zip_path);
    find_java(paths, v).ok_or_else(|| NmError::Other(anyhow::anyhow!("java.exe não encontrado no pacote")))
}

fn extract_zip(zip_path: &Path, out: &Path) -> anyhow::Result<()> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;
    for i in 0..z.len() {
        let mut ent = z.by_index(i)?;
        let Some(rel) = ent.enclosed_name() else { anyhow::bail!("zip com caminho inseguro") };
        let p = out.join(rel);
        if ent.is_dir() {
            std::fs::create_dir_all(&p)?;
        } else {
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            let mut w = std::fs::File::create(&p)?;
            std::io::copy(&mut ent, &mut w)?;
        }
    }
    Ok(())
}

/// Versão do Java (primeira linha de `java -version`), para o checklist de saúde.
pub async fn java_version(java: &Path) -> Option<String> {
    let out = crate::util::command(java).arg("-version").output().await.ok()?;
    let s = String::from_utf8_lossy(&out.stderr).into_owned();
    s.lines().next().map(|l| l.trim().to_string())
}
