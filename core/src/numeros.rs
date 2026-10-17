//! "Nosso Mundo em números": as estatísticas que o próprio Minecraft grava por jogador
//! (`world/stats/<uuid>.json`), somadas e com o nick de cada um (`usercache.json`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Numeros {
    pub blocos_minerados: u64,
    /// Em metros (andando, correndo, nadando, voando, de barco, a cavalo...).
    pub distancia_m: u64,
    pub mortes: u64,
    pub mobs_derrotados: u64,
    pub horas_jogadas: f64,
    pub pulos: u64,
    pub por_jogador: Vec<NumerosJogador>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct NumerosJogador {
    pub nick: String,
    pub blocos_minerados: u64,
    pub distancia_m: u64,
    pub mortes: u64,
    pub mobs_derrotados: u64,
    pub horas_jogadas: f64,
}

fn nicks(server: &Path) -> BTreeMap<String, String> {
    #[derive(serde::Deserialize)]
    struct E {
        uuid: String,
        name: String,
    }
    std::fs::read(server.join("usercache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<E>>(&b).ok())
        .map(|v| v.into_iter().map(|e| (e.uuid, e.name)).collect())
        .unwrap_or_default()
}

fn sum(v: Option<&serde_json::Value>) -> u64 {
    v.and_then(|o| o.as_object()).map(|o| o.values().filter_map(|x| x.as_u64()).sum()).unwrap_or(0)
}

/// Lê a pasta do servidor (`server/`, com `world/stats` e `usercache.json`).
pub fn ler(server: &Path) -> Numeros {
    let names = nicks(server);
    let mut out = Numeros::default();
    let Ok(dir) = std::fs::read_dir(server.join("world").join("stats")) else { return out };
    for e in dir.flatten() {
        let p = e.path();
        let Some(uuid) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        let Some(v) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else { continue };
        let s = &v["stats"];
        let custom = s.get("minecraft:custom").and_then(|c| c.as_object()).cloned().unwrap_or_default();
        let c = |k: &str| custom.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let cm: u64 = custom.iter().filter(|(k, _)| k.ends_with("_one_cm")).filter_map(|(_, x)| x.as_u64()).sum();
        let j = NumerosJogador {
            nick: names.get(&uuid).cloned().unwrap_or_else(|| uuid.chars().take(8).collect()),
            blocos_minerados: sum(s.get("minecraft:mined")),
            distancia_m: cm / 100,
            mortes: c("minecraft:deaths"),
            mobs_derrotados: c("minecraft:mob_kills"),
            horas_jogadas: c("minecraft:play_time") as f64 / 20.0 / 3600.0,
        };
        out.blocos_minerados += j.blocos_minerados;
        out.distancia_m += j.distancia_m;
        out.mortes += j.mortes;
        out.mobs_derrotados += j.mobs_derrotados;
        out.horas_jogadas += j.horas_jogadas;
        out.pulos += c("minecraft:jump");
        out.por_jogador.push(j);
    }
    out.por_jogador.sort_by(|a, b| b.horas_jogadas.total_cmp(&a.horas_jogadas));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soma_as_estatisticas_do_vanilla() {
        let d = tempfile::tempdir().unwrap();
        let stats = d.path().join("world").join("stats");
        std::fs::create_dir_all(&stats).unwrap();
        std::fs::write(d.path().join("usercache.json"), r#"[{"uuid":"u1","name":"Ana","expiresOn":"x"}]"#).unwrap();
        std::fs::write(
            stats.join("u1.json"),
            r#"{"stats":{"minecraft:mined":{"minecraft:stone":100,"minecraft:dirt":20},"minecraft:custom":{"minecraft:walk_one_cm":150000,"minecraft:sprint_one_cm":50000,"minecraft:deaths":3,"minecraft:mob_kills":7,"minecraft:play_time":144000,"minecraft:jump":9}},"DataVersion":4671}"#,
        )
        .unwrap();
        std::fs::write(stats.join("u2.json"), r#"{"stats":{"minecraft:custom":{"minecraft:deaths":1}}}"#).unwrap();
        let n = ler(d.path());
        assert_eq!(n.blocos_minerados, 120);
        assert_eq!(n.distancia_m, 2000);
        assert_eq!(n.mortes, 4);
        assert_eq!(n.mobs_derrotados, 7);
        assert!((n.horas_jogadas - 2.0).abs() < 1e-9);
        assert_eq!(n.por_jogador[0].nick, "Ana");
        assert_eq!(n.por_jogador.len(), 2);
    }
}
