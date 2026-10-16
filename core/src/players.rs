//! Jogadores do mundo: identidade, progresso e migração de UUID.
//!
//! O servidor roda em modo offline, em que o UUID de cada jogador é calculado a partir do nick
//! (`UUID v3 de MD5("OfflinePlayer:<nick>")`). Mundos que vieram do single-player ou de servidores
//! com login guardam o progresso em UUIDs "online" (v4); sem migrar, esses jogadores entrariam zerados.
//! [`migrate`] copia inventário/posição (`playerdata`), conquistas (`advancements`) e estatísticas
//! (`stats`) do UUID antigo para o UUID offline do nick, guardando uma cópia do que existia antes.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::nbt::{self, Tag};

/// UUID offline de um nick (o mesmo cálculo do servidor Minecraft).
pub fn offline_uuid(nick: &str) -> String {
    use md5::{Digest, Md5};
    let mut h: [u8; 16] = Md5::digest(format!("OfflinePlayer:{nick}").as_bytes()).into();
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(h).hyphenated().to_string()
}

pub fn uuid_version(u: &str) -> Option<usize> {
    uuid::Uuid::parse_str(u).ok().map(|u| u.get_version_num())
}

fn uuid_from_ints(v: &[i32]) -> Option<String> {
    if v.len() != 4 {
        return None;
    }
    let mut b = [0u8; 16];
    for (i, x) in v.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    Some(uuid::Uuid::from_bytes(b).hyphenated().to_string())
}

fn read_gz(p: &Path) -> Option<Tag> {
    nbt::read_gzip(std::fs::File::open(p).ok()?).ok().map(|(_, t)| t)
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelInfo {
    pub level_name: String,
    pub data_version: i32,
    pub version_name: String,
    pub difficulty: String,
    pub hardcore: bool,
    pub game_type: String,
    pub seed: Option<i64>,
    /// Dono do single-player (tag `Player` dentro do level.dat).
    pub singleplayer_uuid: Option<String>,
}

pub fn level_info(world: &Path) -> anyhow::Result<LevelInfo> {
    let root = read_gz(&world.join("level.dat")).ok_or_else(|| anyhow::anyhow!("level.dat ilegível"))?;
    let d = root.get("Data").ok_or_else(|| anyhow::anyhow!("level.dat sem Data"))?;
    let int = |t: Option<&Tag>| match t {
        Some(Tag::Int(v)) => Some(*v),
        Some(Tag::Byte(v)) => Some(*v as i32),
        _ => None,
    };
    let version = d.get("Version");
    let difficulty = match int(d.get("Difficulty")) {
        Some(0) => "peaceful",
        Some(1) => "easy",
        Some(3) => "hard",
        _ => "normal",
    };
    let game_type = match int(d.get("GameType")) {
        Some(1) => "creative",
        Some(2) => "adventure",
        Some(3) => "spectator",
        _ => "survival",
    };
    let seed = d.get("WorldGenSettings").and_then(|w| match w.get("seed") {
        Some(Tag::Long(s)) => Some(*s),
        _ => None,
    });
    let singleplayer_uuid = d.get("Player").and_then(|p| match p.get("UUID") {
        Some(Tag::IntArray(v)) => uuid_from_ints(v),
        _ => None,
    });
    Ok(LevelInfo {
        level_name: d.get("LevelName").and_then(Tag::as_str).unwrap_or("").to_string(),
        data_version: int(d.get("DataVersion")).unwrap_or(0),
        version_name: version.and_then(|v| v.get("Name")).and_then(Tag::as_str).unwrap_or("?").to_string(),
        difficulty: difficulty.into(),
        hardcore: int(d.get("hardcore")).unwrap_or(0) != 0,
        game_type: game_type.into(),
        seed,
        singleplayer_uuid,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldPlayer {
    pub uuid: String,
    /// 3 = offline (nick), 4 = conta com login (Microsoft/TLauncher).
    pub uuid_version: usize,
    pub last_played: Option<DateTime<Utc>>,
    pub xp_level: i32,
    pub items: usize,
    pub dimension: String,
    pub pos: Option<(i64, i64, i64)>,
    pub has_advancements: bool,
    pub has_stats: bool,
    /// Nick conhecido que gera este UUID offline (se algum dos nomes dados bater).
    pub nick: Option<String>,
    /// É o dono do single-player (tag Player do level.dat).
    pub singleplayer_owner: bool,
}

/// Lista os jogadores do mundo. `known_nicks` ajuda a reconhecer UUIDs offline.
pub fn scan(world: &Path, known_nicks: &[String]) -> anyhow::Result<Vec<WorldPlayer>> {
    let sp = level_info(world).ok().and_then(|l| l.singleplayer_uuid);
    let mut out = vec![];
    let dir = world.join("playerdata");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(out) };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".dat") else { continue };
        let Some(ver) = uuid_version(id) else { continue };
        let tag = read_gz(&e.path());
        let last = e.metadata().ok().and_then(|m| m.modified().ok()).map(DateTime::<Utc>::from);
        let (xp, items, dim, pos) = match &tag {
            Some(t) => (
                match t.get("XpLevel") {
                    Some(Tag::Int(x)) => *x,
                    _ => 0,
                },
                match t.get("Inventory") {
                    Some(Tag::List(_, v)) => v.len(),
                    _ => 0,
                },
                t.get("Dimension").and_then(Tag::as_str).unwrap_or("minecraft:overworld").to_string(),
                match t.get("Pos") {
                    Some(Tag::List(_, v)) if v.len() == 3 => {
                        let f = |t: &Tag| if let Tag::Double(d) = t { *d as i64 } else { 0 };
                        Some((f(&v[0]), f(&v[1]), f(&v[2])))
                    }
                    _ => None,
                },
            ),
            None => (0, 0, "?".into(), None),
        };
        let nick = known_nicks.iter().find(|n| offline_uuid(n) == id).cloned();
        out.push(WorldPlayer {
            uuid: id.to_string(),
            uuid_version: ver,
            last_played: last,
            xp_level: xp,
            items,
            dimension: dim,
            pos,
            has_advancements: world.join("advancements").join(format!("{id}.json")).exists(),
            has_stats: world.join("stats").join(format!("{id}.json")).exists(),
            nick,
            singleplayer_owner: sp.as_deref() == Some(id),
        });
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.last_played));
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct Migration {
    pub from: String,
    pub to: String,
    pub nick: String,
    pub copied: Vec<String>,
    pub backed_up: Vec<String>,
}

/// Copia o progresso de `from_uuid` para o UUID offline de `nick`.
/// O que já existia no destino é guardado como `<arquivo>.antes-<data>`; o original fica intacto.
pub fn migrate(world: &Path, from_uuid: &str, nick: &str) -> anyhow::Result<Migration> {
    if !crate::config::Config::is_valid_player_name(nick) {
        anyhow::bail!("nick inválido (3 a 16 letras, números ou _)");
    }
    let from = uuid::Uuid::parse_str(from_uuid).map_err(|_| anyhow::anyhow!("UUID inválido"))?.hyphenated().to_string();
    let to = offline_uuid(nick);
    if from == to {
        anyhow::bail!("esse progresso já é do nick {nick}");
    }
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let mut m = Migration { from: from.clone(), to: to.clone(), nick: nick.into(), copied: vec![], backed_up: vec![] };
    let pairs: [(&str, &str); 3] = [("playerdata", "dat"), ("advancements", "json"), ("stats", "json")];
    let src_player = world.join("playerdata").join(format!("{from}.dat"));
    if !src_player.exists() {
        // Dono do single-player sem arquivo próprio: o progresso está no level.dat.
        let li = level_info(world)?;
        if li.singleplayer_uuid.as_deref() != Some(from.as_str()) {
            anyhow::bail!("não achei o progresso desse UUID no mundo");
        }
        let root = read_gz(&world.join("level.dat")).unwrap();
        let player = root.get("Data").and_then(|d| d.get("Player")).cloned().unwrap();
        std::fs::create_dir_all(world.join("playerdata"))?;
        write_gz(&src_player, &player)?;
    }
    for (dir, ext) in pairs {
        let src = world.join(dir).join(format!("{from}.{ext}"));
        if !src.exists() {
            continue;
        }
        let dst = world.join(dir).join(format!("{to}.{ext}"));
        if dst.exists() {
            let bak: PathBuf = world.join(dir).join(format!("{to}.{ext}.antes-{stamp}"));
            std::fs::copy(&dst, &bak)?;
            m.backed_up.push(format!("{dir}/{}", bak.file_name().unwrap().to_string_lossy()));
        }
        std::fs::copy(&src, &dst)?;
        m.copied.push(format!("{dir}/{to}.{ext}"));
    }
    // O UUID salvo dentro do playerdata precisa ser o novo.
    let dst_player = world.join("playerdata").join(format!("{to}.dat"));
    if let Some(mut t) = read_gz(&dst_player) {
        let u = uuid::Uuid::parse_str(&to)?.as_bytes().to_owned();
        let ints: Vec<i32> = (0..4).map(|i| i32::from_be_bytes(u[i * 4..i * 4 + 4].try_into().unwrap())).collect();
        t.set("UUID", Tag::IntArray(ints));
        write_gz(&dst_player, &t)?;
    }
    Ok(m)
}

fn write_gz(p: &Path, t: &Tag) -> anyhow::Result<()> {
    let mut raw = vec![];
    nbt::write(&mut raw, "", t)?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &raw)?;
    crate::util::write_atomic(p, &gz.finish()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_minecraft() {
        // Valores conferidos com o cálculo do servidor (UUID.nameUUIDFromBytes).
        assert_eq!(offline_uuid("mchado"), "cb31b2e8-67fb-355d-b68a-1701810226f3");
        assert_eq!(offline_uuid("Tester"), "f3d28cb0-7225-3cb1-baeb-2dadd2be89ae");
        assert_eq!(uuid_version(&offline_uuid("x")), Some(3));
    }

    fn player_tag(uuid: &str, xp: i32) -> Tag {
        let u = uuid::Uuid::parse_str(uuid).unwrap();
        let b = u.as_bytes();
        let ints: Vec<i32> = (0..4).map(|i| i32::from_be_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())).collect();
        Tag::Compound(vec![
            ("UUID".into(), Tag::IntArray(ints)),
            ("XpLevel".into(), Tag::Int(xp)),
            ("Inventory".into(), Tag::List(10, vec![Tag::Compound(vec![("id".into(), Tag::String("minecraft:diamond".into()))])])),
            ("Pos".into(), Tag::List(6, vec![Tag::Double(1.0), Tag::Double(64.0), Tag::Double(-3.0)])),
        ])
    }

    #[test]
    fn migrates_online_uuid_to_nick() {
        let d = tempfile::tempdir().unwrap();
        let w = d.path();
        let online = "d43491f3-7b84-4720-8911-4f0a74b269db";
        for dir in ["playerdata", "advancements", "stats"] {
            std::fs::create_dir_all(w.join(dir)).unwrap();
        }
        write_gz(&w.join(format!("playerdata/{online}.dat")), &player_tag(online, 30)).unwrap();
        std::fs::write(w.join(format!("advancements/{online}.json")), b"{\"a\":1}").unwrap();
        std::fs::write(w.join(format!("stats/{online}.json")), b"{\"s\":1}").unwrap();
        // já existe um arquivo do nick (de outra época): vira backup
        let target = offline_uuid("mchado");
        write_gz(&w.join(format!("playerdata/{target}.dat")), &player_tag(&target, 2)).unwrap();
        let level = Tag::Compound(vec![("Data".into(), Tag::Compound(vec![("DataVersion".into(), Tag::Int(4671))]))]);
        write_gz(&w.join("level.dat"), &level).unwrap();

        let ps = scan(w, &["mchado".into()]).unwrap();
        assert_eq!(ps.len(), 2);
        assert!(ps.iter().any(|p| p.uuid == online && p.uuid_version == 4 && p.xp_level == 30));
        assert!(ps.iter().any(|p| p.nick.as_deref() == Some("mchado")));

        let m = migrate(w, online, "mchado").unwrap();
        assert_eq!(m.copied.len(), 3);
        assert_eq!(m.backed_up.len(), 1);
        let t = read_gz(&w.join(format!("playerdata/{target}.dat"))).unwrap();
        assert_eq!(t.get("XpLevel"), Some(&Tag::Int(30)));
        let Some(Tag::IntArray(ints)) = t.get("UUID") else { panic!() };
        assert_eq!(uuid_from_ints(ints).unwrap(), target, "UUID interno atualizado");
        assert!(w.join(format!("playerdata/{online}.dat")).exists(), "original intacto");
        assert!(migrate(w, online, "Zé").is_err());
    }
}
