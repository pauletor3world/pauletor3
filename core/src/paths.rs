use std::path::{Path, PathBuf};

/// Layout das pastas locais do app. Tudo de um mundo fica sob `root` (escolhida no assistente;
/// o padrão é o disco com mais espaço livre). O que não é de mundo nenhum (Java, Minecraft
/// aberto direto, downloads) fica em `shared`, para os outros mundos não baixarem de novo.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub shared: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self { shared: root.clone(), root }
    }

    /// Mais um mundo: dados em `root`, Java e Minecraft em `shared` (a pasta do primeiro).
    pub fn for_world(root: impl Into<PathBuf>, shared: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), shared: shared.into() }
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
        self.shared.join("runtime")
    }
    pub fn downloads(&self) -> PathBuf {
        self.shared.join("downloads")
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
        self.shared.join("client")
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
            self.shared.clone(),
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

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Location {
    /// Pasta do mundo aberto agora.
    data_dir: PathBuf,
    /// Pasta do primeiro mundo: guarda o que é de todos (Java, Minecraft) e, em `mundos\`, as
    /// pastas dos outros. Vazio = `data_dir` (um mundo só, como até a 2.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    home: Option<PathBuf>,
    /// Todos os mundos deste PC (pastas de dados), na ordem em que entraram.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    worlds: Vec<PathBuf>,
}

impl Location {
    fn home(&self) -> PathBuf {
        self.home.clone().unwrap_or_else(|| self.data_dir.clone())
    }

    /// Lista completa, sempre com o primeiro mundo e o aberto.
    fn all(&self) -> Vec<PathBuf> {
        let mut v = self.worlds.clone();
        for d in [self.data_dir.clone(), self.home()] {
            if !v.contains(&d) {
                v.insert(0, d);
            }
        }
        v
    }
}

fn read_location_full(f: &Path) -> Option<Location> {
    serde_json::from_slice(&std::fs::read(f).ok()?).ok()
}

fn read_location(f: &Path) -> Option<PathBuf> {
    read_location_full(f).map(|l| l.data_dir)
}

fn write_location(loc: &Location) -> anyhow::Result<()> {
    let f = location_file().ok_or_else(|| anyhow::anyhow!("APPDATA não definido"))?;
    crate::util::write_atomic(&f, &serde_json::to_vec_pretty(loc)?)
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

/// Grava a pasta do mundo aberto. Os outros mundos da lista continuam nela.
pub fn save_data_dir(dir: &Path) -> anyhow::Result<()> {
    let mut loc = location_file().and_then(|f| read_location_full(&f)).unwrap_or_default();
    loc.data_dir = dir.to_path_buf();
    write_location(&loc)
}

// ------------------------------------------------------------------ vários mundos

/// As pastas de dados dos mundos deste PC. Um fica aberto por vez (o `localhost` do Minecraft
/// aponta para ele); trocar fecha a rede de um e abre a do outro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worlds {
    /// Pasta do primeiro mundo (o Java e o Minecraft de todos ficam nela).
    pub home: PathBuf,
    pub active: PathBuf,
    pub all: Vec<PathBuf>,
}

impl Worlds {
    /// As pastas do mundo `dir`, com o que é compartilhado na do primeiro.
    pub fn paths(&self, dir: &Path) -> Paths {
        Paths::for_world(dir, &self.home)
    }
}

/// Os mundos deste PC (nada se o assistente ainda não rodou). Chame depois de
/// [`resolve_data_dir`], que migra os nomes antigos.
pub fn worlds() -> Option<Worlds> {
    if let Some(h) = std::env::var_os("MACHENIA_HOME") {
        let h = PathBuf::from(h);
        return Some(Worlds { home: h.clone(), active: h.clone(), all: vec![h] });
    }
    let loc = read_location_full(&location_file()?)?;
    Some(Worlds { home: loc.home(), active: loc.data_dir.clone(), all: loc.all() })
}

/// Pasta nova (que ainda não existe) para mais um mundo: `<primeiro>\mundos\mundo-N`.
pub fn new_world_dir() -> anyhow::Result<PathBuf> {
    let w = worlds().ok_or_else(|| anyhow::anyhow!("termine o assistente antes de adicionar outro mundo"))?;
    let base = w.home.join("mundos");
    (2..1000)
        .map(|n| base.join(format!("mundo-{n}")))
        .find(|d| !d.exists() && !w.all.contains(d))
        .ok_or_else(|| anyhow::anyhow!("mundos demais nesta pasta"))
}

fn edit_location(f: impl FnOnce(&mut Location) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let file = location_file().ok_or_else(|| anyhow::anyhow!("APPDATA não definido"))?;
    let mut loc = read_location_full(&file).ok_or_else(|| anyhow::anyhow!("termine o assistente antes de mexer nos mundos"))?;
    // a partir de agora a lista e o primeiro mundo ficam explícitos
    loc.home = Some(loc.home());
    loc.worlds = loc.all();
    f(&mut loc)?;
    write_location(&loc)
}

/// Põe `dir` na lista (se faltar) e, com `activate`, deixa ele como o mundo aberto.
pub fn add_world(dir: &Path, activate: bool) -> anyhow::Result<()> {
    edit_location(|loc| {
        if !loc.worlds.iter().any(|d| d == dir) {
            loc.worlds.push(dir.to_path_buf());
        }
        if activate {
            loc.data_dir = dir.to_path_buf();
        }
        Ok(())
    })
}

/// Tira `dir` da lista (os arquivos ficam no disco). O mundo aberto e o primeiro não saem.
pub fn remove_world(dir: &Path) -> anyhow::Result<()> {
    edit_location(|loc| {
        if dir == loc.data_dir {
            anyhow::bail!("troque para outro mundo antes de tirar este da lista");
        }
        if dir == loc.home() {
            anyhow::bail!("o primeiro mundo guarda o Java e o Minecraft dos outros e não sai da lista");
        }
        loc.worlds.retain(|d| d != dir);
        Ok(())
    })
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
    fn mundos_compartilham_java_e_minecraft() {
        let p = Paths::for_world("D:/M/mundos/mundo-2", "D:/M");
        assert_eq!(p.world(), PathBuf::from("D:/M/mundos/mundo-2/server/world"));
        assert_eq!(p.config(), PathBuf::from("D:/M/mundos/mundo-2/config.json"));
        assert_eq!(p.client(), PathBuf::from("D:/M/client"));
        assert_eq!(p.runtime(), PathBuf::from("D:/M/runtime"));
        // um mundo só: tudo na mesma pasta, como antes
        assert_eq!(Paths::new("D:/M").client(), PathBuf::from("D:/M/client"));
        // location.json da 2.2 (só data_dir) abre com um mundo
        let old: Location = serde_json::from_str(r#"{"data_dir":"D:/M"}"#).unwrap();
        assert_eq!(old.all(), vec![PathBuf::from("D:/M")]);
        assert_eq!(old.home(), PathBuf::from("D:/M"));
        let two = Location { data_dir: "D:/M/mundos/mundo-2".into(), home: Some("D:/M".into()), worlds: vec!["D:/M".into(), "D:/M/mundos/mundo-2".into()] };
        assert_eq!(two.all().len(), 2);
        // lista perdida: o primeiro e o aberto voltam sozinhos
        let lost = Location { worlds: vec![], ..two };
        assert_eq!(lost.all(), vec![PathBuf::from("D:/M"), PathBuf::from("D:/M/mundos/mundo-2")]);
    }

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
