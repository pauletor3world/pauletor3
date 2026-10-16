//! Versões fixadas do servidor. `versions.json` no remote é a fonte da verdade para todos os PCs.
//! O app se recusa a hospedar se qualquer arquivo baixado não bater com o hash fixado.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::hashing;
use crate::manifest::ServerVersions;
use crate::remote::{layout, read_json, write_json, Remote};
use crate::{NmError, NmResult, FORMAT_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedFile {
    pub url: String,
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    pub size: Option<u64>,
}

impl PinnedFile {
    /// Confere o arquivo contra todos os hashes fixados (pelo menos um tem que existir).
    pub fn verify(&self, path: &Path) -> NmResult<()> {
        let mut checked = false;
        if let Some(size) = self.size {
            let actual = std::fs::metadata(path)?.len();
            if actual != size {
                return Err(NmError::VersionMismatch(format!("{}: tamanho {actual}, esperado {size}", self.filename)));
            }
        }
        type Hasher = fn(&Path) -> std::io::Result<String>;
        let checks: [(&Option<String>, Hasher, &str); 3] = [
            (&self.sha512, hashing::sha512_file, "sha512"),
            (&self.sha256, hashing::sha256_file, "sha256"),
            (&self.sha1, hashing::sha1_file, "sha1"),
        ];
        for (expected, f, name) in checks {
            if let Some(exp) = expected {
                let actual = f(path)?;
                if !actual.eq_ignore_ascii_case(exp) {
                    return Err(NmError::VersionMismatch(format!("{}: {name} não confere", self.filename)));
                }
                checked = true;
            }
        }
        if !checked {
            return Err(NmError::VersionMismatch(format!("{}: nenhum hash fixado", self.filename)));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Versions {
    pub format: u32,
    pub minecraft: String,
    /// DataVersion dos mundos desta versão (mundos mais novos não podem ser abertos).
    #[serde(default = "default_data_version")]
    pub world_data_version: i32,
    pub fabric_loader: String,
    pub fabric_installer: String,
    /// Launcher do servidor Fabric (meta.fabricmc.net); sha256 calculado ao fixar a versão.
    pub fabric_launcher: PinnedFile,
    /// server.jar oficial (piston-data.mojang.com); sha1 vem do manifesto de versões da Mojang.
    pub vanilla_server: PinnedFile,
    pub lithium_version: String,
    /// Lithium (Modrinth); sha512 vem da API do Modrinth.
    pub lithium: PinnedFile,
    pub java_major: u32,
    pub java_release: String,
    /// JRE Temurin para Windows x64 (Adoptium); sha256 vem da API do Adoptium.
    pub java_windows_x64: PinnedFile,
}

fn default_data_version() -> i32 {
    4671
}

impl Versions {
    /// Padrão embutido no app, conferido nas APIs oficiais em 15/10/2026 (ver docs/decisoes.md).
    pub fn builtin() -> Self {
        serde_json::from_str(include_str!("../versions.default.json")).expect("versions.default.json válido")
    }

    pub fn server_versions(&self) -> ServerVersions {
        ServerVersions {
            minecraft: self.minecraft.clone(),
            fabric_loader: self.fabric_loader.clone(),
            lithium: self.lithium_version.clone(),
        }
    }

    pub async fn load(remote: &dyn Remote) -> NmResult<Option<Self>> {
        let v: Option<Self> = read_json(remote, layout::VERSIONS).await?;
        if let Some(v) = &v {
            if v.format > FORMAT_VERSION {
                return Err(NmError::FormatTooNew { found: v.format });
            }
        }
        Ok(v)
    }

    pub async fn save(&self, remote: &dyn Remote) -> NmResult<()> {
        write_json(remote, layout::VERSIONS, self).await
    }
}

/// Configurações do mundo, iguais em todo PC (ficam no remote).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldSettings {
    pub format: u32,
    pub world_name: String,
    pub motd: String,
    pub difficulty: String,
    pub gamemode: String,
    pub view_distance: u32,
    pub simulation_distance: u32,
    pub max_players: u32,
    pub pvp: bool,
    pub hardcore: bool,
    #[serde(default)]
    pub seed: String,
    #[serde(default = "default_spawn_protection")]
    pub spawn_protection: u32,
    #[serde(default)]
    pub allow_flight: bool,
}

fn default_spawn_protection() -> u32 {
    0
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            world_name: "pAuletOr 3".into(),
            motd: "pAuletOr 3".into(),
            difficulty: "normal".into(),
            gamemode: "survival".into(),
            view_distance: 10,
            simulation_distance: 10,
            max_players: 8,
            pvp: true,
            hardcore: false,
            seed: String::new(),
            spawn_protection: 0,
            allow_flight: false,
        }
    }
}

impl WorldSettings {
    pub async fn load(remote: &dyn Remote) -> NmResult<Option<Self>> {
        read_json(remote, layout::WORLD_SETTINGS).await
    }
    pub async fn save(&self, remote: &dyn Remote) -> NmResult<()> {
        self.validate()?;
        write_json(remote, layout::WORLD_SETTINGS, self).await
    }
    pub fn validate(&self) -> NmResult<()> {
        let ok = ["peaceful", "easy", "normal", "hard"].contains(&self.difficulty.as_str())
            && ["survival", "creative", "adventure", "spectator"].contains(&self.gamemode.as_str())
            && (3..=32).contains(&self.view_distance)
            && (3..=32).contains(&self.simulation_distance)
            && (1..=100).contains(&self.max_players)
            && !self.motd.contains('\n');
        if ok {
            Ok(())
        } else {
            Err(NmError::Other(anyhow::anyhow!("configurações do mundo inválidas")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_parses_and_is_pinned() {
        let v = Versions::builtin();
        assert_eq!(v.minecraft, "1.21.11");
        assert_eq!(v.java_major, 21);
        assert!(v.fabric_launcher.sha256.is_some());
        assert!(v.vanilla_server.sha1.is_some());
        assert!(v.lithium.sha512.is_some());
        assert!(v.java_windows_x64.sha256.is_some());
    }

    #[test]
    fn verify_detects_tamper() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.jar");
        std::fs::write(&p, b"abc").unwrap();
        let mut pf = PinnedFile {
            url: "x".into(),
            filename: "f.jar".into(),
            sha1: None,
            sha256: Some(hashing::sha256_bytes(b"abc")),
            sha512: None,
            size: Some(3),
        };
        pf.verify(&p).unwrap();
        std::fs::write(&p, b"abd").unwrap();
        assert!(pf.verify(&p).is_err());
        pf.sha256 = None;
        assert!(pf.verify(&p).is_err(), "sem hash não passa");
    }
}
