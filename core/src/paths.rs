use std::path::{Path, PathBuf};

/// Layout das pastas locais do app. Tudo fica sob `root` (escolhida no assistente;
/// o padrão é o disco com mais espaço livre).
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }
    pub fn secrets(&self) -> PathBuf {
        self.root.join("secrets.bin")
    }
    pub fn state(&self) -> PathBuf {
        self.root.join("state.json")
    }
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads")
    }
    /// Pasta de trabalho do servidor (cwd do processo Java).
    pub fn server(&self) -> PathBuf {
        self.root.join("server")
    }
    pub fn world(&self) -> PathBuf {
        self.server().join("world")
    }
    /// Espelho consistente do mundo, tirado durante o `save-off`. O envio sai daqui.
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }
    pub fn cache_objects(&self) -> PathBuf {
        self.root.join("cache").join("objects")
    }
    pub fn cache_manifests(&self) -> PathBuf {
        self.root.join("cache").join("manifests")
    }
    pub fn hash_index(&self) -> PathBuf {
        self.root.join("cache").join("hash-index.json")
    }
    /// Onde um download é montado antes da troca atômica.
    pub fn incoming(&self) -> PathBuf {
        self.root.join("incoming")
    }
    /// Cópias de segurança feitas antes de qualquer operação que substitua o mundo local.
    pub fn recovered(&self) -> PathBuf {
        self.root.join("recovered")
    }
    /// Instalação do cliente Minecraft aberto direto pelo app (ver [`crate::client`]).
    pub fn client(&self) -> PathBuf {
        self.root.join("client")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [
            self.root.clone(),
            self.runtime(),
            self.downloads(),
            self.server(),
            self.cache_objects(),
            self.cache_manifests(),
            self.recovered(),
            self.logs(),
            self.backups(),
            self.tmp(),
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

/// Nome das pastas do app (perfil do usuário e pasta de dados sugerida).
pub const DIR_NAME: &str = "Machenia";
/// Nomes antigos: "pAuletOr3" (até a 2.0.3) e "MachenCraft" (até a 2.1.x). Migrados sozinhos.
const LEGACY_DIR_NAMES: [&str; 2] = ["MachenCraft", "pAuletOr3"];

/// Arquivo minúsculo em `%APPDATA%\Machenia\location.json` que diz onde está a pasta de dados.
/// Fica sempre no perfil do usuário; os dados pesados ficam onde ele escolher.
pub fn location_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(DIR_NAME).join("location.json"))
}

fn legacy_location_files() -> Vec<PathBuf> {
    let Some(a) = std::env::var_os("APPDATA") else { return Vec::new() };
    LEGACY_DIR_NAMES.iter().map(|n| PathBuf::from(&a).join(n).join("location.json")).collect()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Location {
    data_dir: PathBuf,
}

fn read_location(f: &Path) -> Option<PathBuf> {
    let loc: Location = serde_json::from_slice(&std::fs::read(f).ok()?).ok()?;
    Some(loc.data_dir)
}

/// Pasta de dados: `MACHENIA_HOME`, senão o que estiver em `location.json`.
/// Na primeira abertura depois de uma troca de nome, adota o `location.json` antigo e
/// renomeia a pasta (`MachenCraft` ou `pAuletOr3`) para `Machenia` (se nada estiver usando a pasta).
pub fn resolve_data_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("MACHENIA_HOME") {
        return Some(PathBuf::from(h));
    }
    let (dir, legacy) = match read_location(&location_file()?) {
        Some(d) => (d, None),
        None => legacy_location_files().into_iter().find_map(|f| read_location(&f).map(|d| (d, Some(f))))?,
    };
    let migrated = migrate_legacy_root(&dir);
    let saved = (legacy.is_some() || migrated != dir) && save_data_dir(&migrated).is_ok();
    if let (true, Some(old)) = (saved, legacy) {
        let _ = std::fs::remove_file(&old);
        if let Some(parent) = old.parent() {
            let _ = std::fs::remove_dir(parent); // só some se estiver vazia
        }
    }
    Some(migrated)
}

/// `...\MachenCraft` (ou `...\pAuletOr3`) vira `...\Machenia`. Um rename na mesma unidade é
/// instantâneo e atômico; se algo estiver com arquivos abertos lá dentro (ou o destino já
/// existir), fica tudo como está e tenta de novo na próxima abertura.
fn migrate_legacy_root(dir: &Path) -> PathBuf {
    let legacy = dir.file_name().and_then(|n| n.to_str()).is_some_and(|n| LEGACY_DIR_NAMES.contains(&n));
    if !legacy {
        return dir.to_path_buf();
    }
    let new = dir.with_file_name(DIR_NAME);
    if dir.is_dir() && !new.exists() && std::fs::rename(dir, &new).is_ok() {
        return new;
    }
    dir.to_path_buf()
}

pub fn save_data_dir(dir: &Path) -> anyhow::Result<()> {
    let f = location_file().ok_or_else(|| anyhow::anyhow!("APPDATA não definido"))?;
    crate::util::write_atomic(&f, &serde_json::to_vec_pretty(&Location { data_dir: dir.to_path_buf() })?)
}

/// Sugere uma pasta de dados: o disco fixo com mais espaço livre.
pub fn suggest_data_dir() -> PathBuf {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let best = disks
        .iter()
        .filter(|d| !d.is_removable())
        .max_by_key(|d| d.available_space())
        .map(|d| d.mount_point().to_path_buf());
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    match (best, local) {
        (Some(mp), Some(local)) if local.starts_with(&mp) => local.join(DIR_NAME),
        (Some(mp), _) => mp.join(DIR_NAME),
        (None, Some(local)) => local.join(DIR_NAME),
        (None, None) => PathBuf::from(DIR_NAME),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migra_a_pasta_antiga() {
        let t = tempfile::tempdir().unwrap();
        let old = t.path().join("pAuletOr3");
        std::fs::create_dir_all(old.join("server")).unwrap();
        let got = migrate_legacy_root(&old);
        assert_eq!(got, t.path().join("Machenia"));
        assert!(got.join("server").is_dir() && !old.exists());
        // o nome da 2.1 também
        std::fs::remove_dir_all(&got).unwrap();
        let mc = t.path().join("MachenCraft");
        std::fs::create_dir_all(&mc).unwrap();
        assert_eq!(migrate_legacy_root(&mc), t.path().join("Machenia"));
        // outro nome qualquer fica como está
        let other = t.path().join("Jogos");
        assert_eq!(migrate_legacy_root(&other), other);
        // destino já existe: não mexe
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(migrate_legacy_root(&old), old);
    }
}
