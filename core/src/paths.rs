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

/// Arquivo minúsculo em `%APPDATA%\pAuletOr3\location.json` que diz onde está a pasta de dados.
/// Fica sempre no perfil do usuário; os dados pesados ficam onde ele escolher.
pub fn location_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("pAuletOr3").join("location.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Location {
    data_dir: PathBuf,
}

/// Pasta de dados: `PAULETOR3_HOME`, senão o que estiver em `location.json`.
pub fn resolve_data_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("PAULETOR3_HOME") {
        return Some(PathBuf::from(h));
    }
    let f = location_file()?;
    let loc: Location = serde_json::from_slice(&std::fs::read(f).ok()?).ok()?;
    Some(loc.data_dir)
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
        (Some(mp), Some(local)) if local.starts_with(&mp) => local.join("pAuletOr3"),
        (Some(mp), _) => mp.join("pAuletOr3"),
        (None, Some(local)) => local.join("pAuletOr3"),
        (None, None) => PathBuf::from("pAuletOr3"),
    }
}
