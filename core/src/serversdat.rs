//! Entrada "pAuletOr 3" no `servers.dat` do Minecraft (lista de servidores do Multiplayer).
//!
//! Formato (NBT sem compressão): raiz `{ servers: [ { name: "...", ip: "...", icon?: "...",
//! acceptTextures?: byte, hidden?: byte }, ... ] }`. Só mexemos na nossa entrada; tudo o mais
//! (inclusive campos que não conhecemos) é preservado byte a byte.
//! Sempre com o Minecraft fechado e com backup do arquivo antes.

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::nbt::{self, Tag};
use crate::util;

pub const ENTRY_NAME: &str = "pAuletOr 3";

pub fn servers_dat(minecraft_dir: &Path) -> PathBuf {
    minecraft_dir.join("servers.dat")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Updated,
    Unchanged,
}

/// Lê o endereço atual da nossa entrada (se existir).
pub fn current_address(minecraft_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(servers_dat(minecraft_dir)).ok()?;
    let (_, root) = nbt::read(&bytes[..]).ok()?;
    match root.get("servers")? {
        Tag::List(_, v) => v
            .iter()
            .find(|s| s.get("name").and_then(Tag::as_str) == Some(ENTRY_NAME))
            .and_then(|s| s.get("ip").and_then(Tag::as_str).map(str::to_string)),
        _ => None,
    }
}

/// Garante a entrada "pAuletOr 3" com `address`. Faz backup antes de gravar.
/// O chamador precisa garantir que o Minecraft está fechado.
pub fn ensure_entry(minecraft_dir: &Path, address: &str, backups: &Path) -> anyhow::Result<Change> {
    let file = servers_dat(minecraft_dir);
    let (name, mut root) = match std::fs::read(&file) {
        Ok(bytes) => nbt::read(&bytes[..]).map_err(|e| anyhow::anyhow!("servers.dat ilegível ({e}); não vou mexer nele"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), Tag::Compound(vec![])),
        Err(e) => return Err(e.into()),
    };
    if !matches!(root, Tag::Compound(_)) {
        anyhow::bail!("servers.dat com formato inesperado; não vou mexer nele");
    }
    if root.get("servers").is_none() {
        root.set("servers", Tag::List(10, vec![]));
    }
    let change;
    {
        let Some(Tag::List(_, list)) = root.get_mut("servers") else {
            anyhow::bail!("servers.dat com formato inesperado; não vou mexer nele");
        };
        if let Some(entry) = list.iter_mut().find(|s| s.get("name").and_then(Tag::as_str) == Some(ENTRY_NAME)) {
            if entry.get("ip").and_then(Tag::as_str) == Some(address) {
                change = Change::Unchanged;
            } else {
                entry.set("ip", Tag::String(address.to_string()));
                change = Change::Updated;
            }
        } else {
            // Nossa entrada vai para o topo da lista.
            list.insert(
                0,
                Tag::Compound(vec![
                    ("name".into(), Tag::String(ENTRY_NAME.into())),
                    ("ip".into(), Tag::String(address.to_string())),
                    ("acceptTextures".into(), Tag::Byte(0)),
                    ("hidden".into(), Tag::Byte(0)),
                ]),
            );
            change = Change::Added;
        }
    }
    if change == Change::Unchanged {
        return Ok(change);
    }
    if file.exists() {
        std::fs::create_dir_all(backups)?;
        let b = backups.join(format!(
            "servers.dat.{}-{}.bak",
            Utc::now().format("%Y%m%d-%H%M%S%.3f"),
            &uuid::Uuid::new_v4().simple().to_string()[..6]
        ));
        std::fs::copy(&file, &b)?;
    }
    let mut out = vec![];
    nbt::write(&mut out, &name, &root)?;
    // confere que relê antes de gravar
    nbt::read(&out[..])?;
    util::write_atomic(&file, &out)?;
    Ok(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_updates_and_preserves_other_servers() {
        let mc = tempfile::tempdir().unwrap();
        let bk = tempfile::tempdir().unwrap();
        // servers.dat existente com outro servidor e um campo desconhecido
        let root = Tag::Compound(vec![(
            "servers".into(),
            Tag::List(
                10,
                vec![Tag::Compound(vec![
                    ("ip".into(), Tag::String("mc.hypixel.net".into())),
                    ("name".into(), Tag::String("Hypixel".into())),
                    ("icon".into(), Tag::String("iVBORw0KGgo=".into())),
                    ("campoNovo".into(), Tag::Long(42)),
                ])],
            ),
        )]);
        let mut buf = vec![];
        nbt::write(&mut buf, "", &root).unwrap();
        std::fs::write(servers_dat(mc.path()), &buf).unwrap();

        assert_eq!(ensure_entry(mc.path(), "pauletor3.duckdns.org", bk.path()).unwrap(), Change::Added);
        assert_eq!(current_address(mc.path()).as_deref(), Some("pauletor3.duckdns.org"));
        assert_eq!(ensure_entry(mc.path(), "pauletor3.duckdns.org", bk.path()).unwrap(), Change::Unchanged);
        assert_eq!(ensure_entry(mc.path(), "100.64.1.2", bk.path()).unwrap(), Change::Updated);
        assert_eq!(std::fs::read_dir(bk.path()).unwrap().count(), 2, "backup antes de cada gravação");

        let (_, back) = nbt::read(&std::fs::read(servers_dat(mc.path())).unwrap()[..]).unwrap();
        let Some(Tag::List(_, list)) = back.get("servers") else { panic!() };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].get("name").unwrap().as_str(), Some("pAuletOr 3"));
        assert_eq!(list[1].get("campoNovo"), Some(&Tag::Long(42)), "campo desconhecido preservado");
        assert_eq!(list[1].get("icon").unwrap().as_str(), Some("iVBORw0KGgo="));
    }

    #[test]
    fn creates_file_when_missing_and_refuses_garbage() {
        let mc = tempfile::tempdir().unwrap();
        let bk = tempfile::tempdir().unwrap();
        assert_eq!(ensure_entry(mc.path(), "x.duckdns.org", bk.path()).unwrap(), Change::Added);
        std::fs::write(servers_dat(mc.path()), b"lixo").unwrap();
        assert!(ensure_entry(mc.path(), "x.duckdns.org", bk.path()).is_err());
        assert_eq!(std::fs::read(servers_dat(mc.path())).unwrap(), b"lixo", "arquivo estranho intocado");
    }
}
