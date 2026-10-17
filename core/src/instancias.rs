//! Versões do Minecraft instaladas na `.minecraft` (OptiFine, Fabric, Quilt), para abrir o jogo
//! direto pelo app com a versão que cada um usa, como nos launchers.
//!
//! O app instala e confere o cliente vanilla do mundo (sha1 da Mojang). Uma versão da
//! `.minecraft` com `inheritsFrom` igual à versão do mundo é aplicada por cima dele: as
//! bibliotecas dela entram antes no classpath (substituindo as de mesmo grupo:artefato), a
//! classe principal e os argumentos extras são os dela, e a pasta do jogo passa a ser a
//! `.minecraft` (mods, shaders, pacotes de recursos e opções de quem já joga).
//! Forge/NeoForge não entram (inicialização própria, que o app não monta).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::client::{Arg, Rule};
use crate::{NmError, NmResult};

/// Uma versão que dá para escolher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientVersion {
    pub id: String,
    /// Nome para a tela, ex.: "OptiFine HD U J7 (1.21.11)".
    pub label: String,
    /// "vanilla", "optifine", "fabric" ou "quilt".
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChildJson {
    id: String,
    #[serde(default)]
    inherits_from: Option<String>,
    #[serde(default)]
    main_class: Option<String>,
    #[serde(default)]
    libraries: Vec<ChildLib>,
    #[serde(default)]
    arguments: Option<ChildArgs>,
    /// Formato antigo (uma linha só); usado por OptiFine de versões velhas.
    #[serde(default)]
    minecraft_arguments: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ChildArgs {
    #[serde(default)]
    game: Vec<Arg>,
    #[serde(default)]
    jvm: Vec<Arg>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChildLib {
    name: String,
    /// Base Maven (Fabric): o arquivo fica em `url + caminho`.
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    downloads: Option<ChildDownloads>,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChildDownloads {
    #[serde(default)]
    artifact: Option<ChildArtifact>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChildArtifact {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// O que uma versão da `.minecraft` muda no lançamento.
#[derive(Debug, Clone)]
pub struct Overlay {
    pub id: String,
    pub main_class: String,
    /// Bibliotecas da versão (caminhos absolutos), antes das do vanilla.
    pub libraries: Vec<PathBuf>,
    /// `grupo:artefato` das bibliotecas acima (as do vanilla com o mesmo nome saem).
    pub replaces: BTreeSet<String>,
    pub jvm: Vec<Arg>,
    pub game: Vec<Arg>,
    /// Jar do jogo próprio da versão (OptiFine), se existir.
    pub jar: Option<PathBuf>,
    /// Pasta do jogo: a `.minecraft`.
    pub game_dir: PathBuf,
}

fn kind_of(c: &ChildJson) -> Option<&'static str> {
    let id = c.id.to_lowercase();
    let main = c.main_class.clone().unwrap_or_default().to_lowercase();
    if id.contains("forge") || main.contains("forge") || main.contains("bootstraplauncher") || main.contains("modlauncher") {
        return None;
    }
    if id.contains("optifine") || c.libraries.iter().any(|l| l.name.to_lowercase().starts_with("optifine:")) {
        return Some("optifine");
    }
    if main.contains("fabricmc") || id.contains("fabric") {
        return Some("fabric");
    }
    if main.contains("quiltmc") || id.contains("quilt") {
        return Some("quilt");
    }
    Some("outra")
}

fn label_for(c: &ChildJson, kind: &str, minecraft: &str) -> String {
    match kind {
        "optifine" => {
            let rest = c.id.split_once("OptiFine").map(|(_, r)| r.trim_start_matches(['_', '-'])).unwrap_or(&c.id);
            format!("OptiFine {} ({minecraft})", rest.replace('_', " "))
        }
        "fabric" => format!("Fabric ({minecraft})"),
        "quilt" => format!("Quilt ({minecraft})"),
        _ => c.id.clone(),
    }
}

fn read_child(mc: &Path, id: &str) -> Option<ChildJson> {
    let p = mc.join("versions").join(id).join(format!("{id}.json"));
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// Versões da `.minecraft` que rodam no mundo (`inheritsFrom` = versão do mundo). A primeira
/// da lista é sempre o vanilla do próprio app.
pub fn list(mc: Option<&Path>, minecraft: &str) -> Vec<ClientVersion> {
    let mut out = vec![ClientVersion { id: String::new(), label: format!("Minecraft {minecraft} (padrão do app)"), kind: "vanilla".into() }];
    let Some(mc) = mc else { return out };
    let Ok(dirs) = std::fs::read_dir(mc.join("versions")) else { return out };
    let mut found: Vec<ClientVersion> = dirs
        .flatten()
        .filter_map(|d| d.file_name().into_string().ok())
        .filter_map(|id| read_child(mc, &id))
        .filter(|c| c.inherits_from.as_deref() == Some(minecraft))
        .filter_map(|c| {
            let kind = kind_of(&c)?;
            Some(ClientVersion { label: label_for(&c, kind, minecraft), id: c.id.clone(), kind: kind.into() })
        })
        .collect();
    found.sort_by(|a, b| a.label.cmp(&b.label));
    out.extend(found);
    out
}

/// `grupo:artefato:versão[:classificador]` → caminho Maven relativo.
pub fn maven_path(name: &str) -> Option<String> {
    let mut p = name.split('@').next()?.split(':');
    let (g, a, v) = (p.next()?, p.next()?, p.next()?);
    let classifier = p.next();
    if g.is_empty() || a.is_empty() || v.is_empty() || name.contains("..") {
        return None;
    }
    let file = match classifier {
        Some(c) => format!("{a}-{v}-{c}.jar"),
        None => format!("{a}-{v}.jar"),
    };
    Some(format!("{}/{a}/{v}/{file}", g.replace('.', "/")))
}

fn group_artifact(name: &str) -> String {
    let mut p = name.split(':');
    format!("{}:{}", p.next().unwrap_or(""), p.next().unwrap_or(""))
}

/// Monta o que a versão `id` muda no lançamento. Bibliotecas que faltam e têm endereço (Fabric)
/// são baixadas para a `.minecraft/libraries`; sem endereço, pede para abrir uma vez no launcher.
pub async fn overlay(mc: &Path, id: &str, minecraft: &str) -> NmResult<Overlay> {
    let c = read_child(mc, id).ok_or_else(|| NmError::VersionMismatch(format!("a versão {id} não está mais na .minecraft")))?;
    if c.inherits_from.as_deref() != Some(minecraft) {
        return Err(NmError::VersionMismatch(format!("a versão {id} não é do Minecraft {minecraft}, a do mundo")));
    }
    if kind_of(&c).is_none() {
        return Err(NmError::VersionMismatch(format!("a versão {id} (Forge/NeoForge) não abre pelo app; escolha outra em Configurações")));
    }
    let main_class = c.main_class.clone().ok_or_else(|| NmError::VersionMismatch(format!("a versão {id} não diz a classe principal")))?;
    let libs_root = mc.join("libraries");
    let none = std::collections::BTreeMap::new();
    let mut libraries = vec![];
    let mut replaces = BTreeSet::new();
    let http = reqwest::Client::builder().user_agent(concat!("MachenCraft/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| NmError::Other(e.into()))?;
    for l in &c.libraries {
        if !crate::client::rules_allow(&l.rules, &none) || l.name.contains(":natives-") {
            continue;
        }
        let art = l.downloads.as_ref().and_then(|d| d.artifact.as_ref());
        let rel = art.and_then(|a| a.path.clone()).or_else(|| maven_path(&l.name));
        let Some(rel) = rel else { continue };
        let path = crate::util::join_rel(&libs_root, &rel).map_err(|e| NmError::Integrity(format!("caminho de biblioteca inválido: {e}")))?;
        if !path.is_file() {
            let url = art.and_then(|a| a.url.clone()).filter(|u| !u.is_empty()).or_else(|| l.url.as_ref().map(|base| format!("{}/{rel}", base.trim_end_matches('/'))));
            let Some(url) = url else {
                return Err(NmError::VersionMismatch(format!("falta a biblioteca {} da versão {id}; abra essa versão uma vez no launcher dela", l.name)));
            };
            tracing::info!(lib = %l.name, "baixando biblioteca da versão escolhida");
            let bytes = http
                .get(&url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| NmError::Remote(format!("biblioteca {}: {e}", l.name)))?
                .bytes()
                .await
                .map_err(|e| NmError::Remote(format!("biblioteca {}: {e}", l.name)))?;
            if let Some(d) = path.parent() {
                std::fs::create_dir_all(d)?;
            }
            crate::util::write_atomic(&path, &bytes)?;
        }
        replaces.insert(group_artifact(&l.name));
        libraries.push(path);
    }
    let mut args = c.arguments.clone().unwrap_or_default();
    if let Some(line) = &c.minecraft_arguments {
        // formato antigo: só o que não é do vanilla (ex.: --tweakClass)
        let mut it = line.split_whitespace().peekable();
        while let Some(t) = it.next() {
            if t == "--tweakClass" {
                if let Some(v) = it.next() {
                    args.game.push(Arg::Plain(t.into()));
                    args.game.push(Arg::Plain(v.into()));
                }
            }
        }
    }
    let jar = mc.join("versions").join(id).join(format!("{id}.jar"));
    let jar = std::fs::metadata(&jar).ok().filter(|m| m.len() > 0).map(|_| jar);
    Ok(Overlay { id: c.id, main_class, libraries, replaces, jvm: args.jvm, game: args.game, jar, game_dir: mc.to_path_buf() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(mc: &Path, id: &str, json: serde_json::Value) {
        let d = mc.join("versions").join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("{id}.json")), serde_json::to_vec(&json).unwrap()).unwrap();
    }

    #[test]
    fn maven_paths() {
        assert_eq!(maven_path("net.fabricmc:fabric-loader:0.17.2").as_deref(), Some("net/fabricmc/fabric-loader/0.17.2/fabric-loader-0.17.2.jar"));
        assert_eq!(maven_path("optifine:OptiFine:1.21.11_HD_U_J7").as_deref(), Some("optifine/OptiFine/1.21.11_HD_U_J7/OptiFine-1.21.11_HD_U_J7.jar"));
        assert_eq!(maven_path("a:b:c:natives-windows").as_deref(), Some("a/b/c/b-c-natives-windows.jar"));
        assert_eq!(maven_path("ruim"), None);
    }

    #[test]
    fn lists_only_versions_of_the_world() {
        let d = tempfile::tempdir().unwrap();
        let mc = d.path();
        write(mc, "1.21.11-OptiFine_HD_U_J7", serde_json::json!({"id": "1.21.11-OptiFine_HD_U_J7", "inheritsFrom": "1.21.11", "mainClass": "net.minecraft.launchwrapper.Launch",
            "libraries": [{"name": "optifine:OptiFine:1.21.11_HD_U_J7"}], "arguments": {"game": ["--tweakClass", "optifine.OptiFineTweaker"]}}));
        write(mc, "fabric-loader-0.17.2-1.21.11", serde_json::json!({"id": "fabric-loader-0.17.2-1.21.11", "inheritsFrom": "1.21.11", "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient"}));
        write(mc, "1.20.1-forge-47.2.0", serde_json::json!({"id": "1.20.1-forge-47.2.0", "inheritsFrom": "1.20.1", "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher"}));
        write(mc, "neoforge-21.11.1", serde_json::json!({"id": "neoforge-21.11.1", "inheritsFrom": "1.21.11", "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher"}));
        write(mc, "fabric-loader-0.17.2-1.21.10", serde_json::json!({"id": "fabric-loader-0.17.2-1.21.10", "inheritsFrom": "1.21.10", "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient"}));
        let l = list(Some(mc), "1.21.11");
        let ids: Vec<&str> = l.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, vec!["", "fabric-loader-0.17.2-1.21.11", "1.21.11-OptiFine_HD_U_J7"]);
        assert_eq!(l[2].kind, "optifine");
        assert_eq!(l[2].label, "OptiFine HD U J7 (1.21.11)");
    }

    #[tokio::test]
    async fn overlay_uses_local_libraries_and_extra_args() {
        let d = tempfile::tempdir().unwrap();
        let mc = d.path();
        let id = "1.21.11-OptiFine_HD_U_J7";
        write(mc, id, serde_json::json!({"id": id, "inheritsFrom": "1.21.11", "mainClass": "net.minecraft.launchwrapper.Launch",
            "libraries": [{"name": "optifine:OptiFine:1.21.11_HD_U_J7"}, {"name": "optifine:launchwrapper-of:2.3"}],
            "arguments": {"game": ["--tweakClass", "optifine.OptiFineTweaker"]}}));
        for rel in ["optifine/OptiFine/1.21.11_HD_U_J7/OptiFine-1.21.11_HD_U_J7.jar", "optifine/launchwrapper-of/2.3/launchwrapper-of-2.3.jar"] {
            let p = mc.join("libraries").join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"jar").unwrap();
        }
        let o = overlay(mc, id, "1.21.11").await.unwrap();
        assert_eq!(o.main_class, "net.minecraft.launchwrapper.Launch");
        assert_eq!(o.libraries.len(), 2);
        assert!(o.replaces.contains("optifine:launchwrapper-of"));
        assert_eq!(o.game_dir, mc);
        assert!(matches!(&o.game[0], Arg::Plain(s) if s == "--tweakClass"));
        // biblioteca faltando sem endereço: explica o que fazer
        std::fs::remove_file(mc.join("libraries/optifine/launchwrapper-of/2.3/launchwrapper-of-2.3.jar")).unwrap();
        let e = overlay(mc, id, "1.21.11").await.unwrap_err();
        assert!(e.to_string().contains("abra essa versão uma vez"));
        // versão de outro Minecraft é recusada
        assert!(overlay(mc, id, "1.21.10").await.is_err());
    }
}
