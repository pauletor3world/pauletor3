//! Leitura do console do servidor: entradas, saídas, mortes, conquistas, chat e marcos do ciclo de vida.
//!
//! Os modelos de morte vêm do `en_us.json` do server.jar 1.21.11 (ver `death_templates.rs`).
//! Formato das linhas:
//! - vanilla: `[21:03:11] [Server thread/INFO]: Ana joined the game`
//! - com o Fabric: `[21:03:11] [Server thread/INFO] (Minecraft) Ana joined the game`

use std::sync::OnceLock;

use regex::Regex;

use crate::death_templates::{DEATH_TEMPLATES, ADV_ENTER_END, ADV_ENTER_NETHER, ADV_KILL_DRAGON};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvancementKind {
    Task,
    Goal,
    Challenge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    Joined(String),
    Left(String),
    /// Morte: vítima, mensagem completa do jogo, chave do modelo (ex.: `death.attack.mob`).
    Death { player: String, message: String, key: String },
    Advancement { player: String, title: String, kind: AdvancementKind },
    Chat { player: String, text: String },
    /// "Done (12.345s)! For help, type "help""
    Ready,
    /// "Saved the game" (resposta do save-all)
    Saved,
    /// "Stopping server"
    Stopping,
    /// Erro grave de inicialização (porta ocupada etc.)
    Fatal(String),
}

impl LogEvent {
    pub fn is_nether_entry(&self) -> bool {
        matches!(self, LogEvent::Advancement { title, .. } if title == ADV_ENTER_NETHER)
    }
    pub fn is_end_entry(&self) -> bool {
        matches!(self, LogEvent::Advancement { title, .. } if title == ADV_ENTER_END)
    }
    pub fn is_dragon_kill(&self) -> bool {
        matches!(self, LogEvent::Advancement { title, .. } if title == ADV_KILL_DRAGON)
    }
}

const NAME: &str = r"([A-Za-z0-9_]{1,16})";

fn re_line() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[[^\]]*\] \[([^\]]+)/([A-Z]+)\](?:: | \([^)]*\) )(.*)$").unwrap())
}
fn re_join() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(&format!(r"^{NAME} joined the game$")).unwrap())
}
fn re_left() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(&format!(r"^{NAME} left the game$")).unwrap())
}
fn re_done() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^Done \([0-9.,]+s\)! For help").unwrap())
}
fn re_adv() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(&format!(
            r"^{NAME} has (made the advancement|reached the goal|completed the challenge) \[(.+)\]$"
        ))
        .unwrap()
    })
}
fn re_chat() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(&format!(r"^(?:\[Not Secure\] )?<{NAME}> (.*)$")).unwrap())
}

/// Modelos de morte compilados em regex, do mais específico (mais longo) para o mais genérico.
fn death_res() -> &'static Vec<(&'static str, Regex)> {
    static R: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    R.get_or_init(|| {
        let mut v: Vec<(&'static str, &'static str)> = DEATH_TEMPLATES.to_vec();
        v.sort_by_key(|(_, t)| std::cmp::Reverse(t.len()));
        v.into_iter()
            .map(|(k, t)| {
                let mut pat = String::from("^");
                let mut rest = t;
                while let Some(i) = rest.find('%') {
                    pat.push_str(&regex::escape(&rest[..i]));
                    let tail = &rest[i..];
                    let end = tail.find('s').map(|j| j + 1).unwrap_or(tail.len());
                    let spec = &tail[..end];
                    pat.push_str(if spec == "%1$s" { NAME } else { "(.+)" });
                    rest = &tail[end..];
                }
                pat.push_str(&regex::escape(rest));
                pat.push('$');
                (k, Regex::new(&pat).expect("modelo de morte vira regex"))
            })
            .collect()
    })
}

/// Extrai (thread, nível, mensagem) de uma linha de log.
pub fn split_line(line: &str) -> Option<(&str, &str, &str)> {
    let c = re_line().captures(line.trim_end())?;
    Some((c.get(1)?.as_str(), c.get(2)?.as_str(), c.get(3)?.as_str()))
}

pub fn parse_line(line: &str) -> Option<LogEvent> {
    if line.contains("**** FAILED TO BIND TO PORT!") {
        return Some(LogEvent::Fatal("A porta do jogo já está em uso por outro programa".into()));
    }
    if line.contains("You need to agree to the EULA") {
        return Some(LogEvent::Fatal("O EULA do Minecraft não foi aceito".into()));
    }
    let (thread, level, msg) = split_line(line)?;
    // Só mensagens da thread principal do servidor contam (chat de jogadores não engana:
    // ele aparece como "<nome> texto").
    if thread != "Server thread" || level != "INFO" {
        return None;
    }
    if let Some(c) = re_chat().captures(msg) {
        return Some(LogEvent::Chat { player: c[1].to_string(), text: c[2].to_string() });
    }
    if let Some(c) = re_join().captures(msg) {
        return Some(LogEvent::Joined(c[1].to_string()));
    }
    if let Some(c) = re_left().captures(msg) {
        return Some(LogEvent::Left(c[1].to_string()));
    }
    if let Some(c) = re_adv().captures(msg) {
        let kind = match &c[2] {
            "reached the goal" => AdvancementKind::Goal,
            "completed the challenge" => AdvancementKind::Challenge,
            _ => AdvancementKind::Task,
        };
        return Some(LogEvent::Advancement { player: c[1].to_string(), title: c[3].to_string(), kind });
    }
    if re_done().is_match(msg) {
        return Some(LogEvent::Ready);
    }
    if msg == "Stopping server" {
        return Some(LogEvent::Stopping);
    }
    if msg == "Saved the game" {
        return Some(LogEvent::Saved);
    }
    for (key, re) in death_res() {
        if let Some(c) = re.captures(msg) {
            return Some(LogEvent::Death { player: c[1].to_string(), message: msg.to_string(), key: key.to_string() });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const VANILLA: &str = "[21:03:11] [Server thread/INFO]: ";
    const FABRIC: &str = "[21:03:11] [Server thread/INFO] (Minecraft) ";

    fn both(msg: &str) -> [Option<LogEvent>; 2] {
        [parse_line(&format!("{VANILLA}{msg}")), parse_line(&format!("{FABRIC}{msg}"))]
    }

    #[test]
    fn lifecycle_and_presence() {
        for e in both("Ana_23 joined the game") {
            assert_eq!(e, Some(LogEvent::Joined("Ana_23".into())));
        }
        for e in both("Ana_23 left the game") {
            assert_eq!(e, Some(LogEvent::Left("Ana_23".into())));
        }
        for e in both("Done (6.123s)! For help, type \"help\"") {
            assert_eq!(e, Some(LogEvent::Ready));
        }
        for e in both("Saved the game") {
            assert_eq!(e, Some(LogEvent::Saved));
        }
        for e in both("Stopping server") {
            assert_eq!(e, Some(LogEvent::Stopping));
        }
        assert!(matches!(
            parse_line("[21:00:00] [Server thread/WARN]: **** FAILED TO BIND TO PORT!"),
            Some(LogEvent::Fatal(_))
        ));
    }

    #[test]
    fn chat_does_not_fool_parser() {
        for e in both("<Bia> Ana joined the game") {
            assert_eq!(e, Some(LogEvent::Chat { player: "Bia".into(), text: "Ana joined the game".into() }));
        }
        for e in both("[Not Secure] <Bia> Ana was slain by Zombie") {
            assert!(matches!(e, Some(LogEvent::Chat { .. })));
        }
        // outras threads nunca contam
        assert_eq!(parse_line("[21:03:11] [Async Chat Thread - #0/INFO]: Ana joined the game"), None);
    }

    #[test]
    fn advancements() {
        for e in both("Ana has made the advancement [We Need to Go Deeper]") {
            let e = e.unwrap();
            assert!(e.is_nether_entry());
            assert!(matches!(e, LogEvent::Advancement { kind: AdvancementKind::Task, .. }));
        }
        assert!(both("Bia has completed the challenge [How Did We Get Here?]")[0]
            .as_ref()
            .is_some_and(|e| matches!(e, LogEvent::Advancement { kind: AdvancementKind::Challenge, .. })));
        assert!(both("Bia has reached the goal [Free the End]")[1].as_ref().unwrap().is_dragon_kill());
        assert!(both("Bia has made the advancement [The End?]")[0].as_ref().unwrap().is_end_entry());
    }

    /// Cada modelo real de morte da 1.21.11, preenchido, tem que virar Death com a vítima certa.
    #[test]
    fn every_real_death_template_parses() {
        assert!(DEATH_TEMPLATES.len() >= 100);
        for (key, tpl) in DEATH_TEMPLATES {
            let msg = tpl.replace("%1$s", "Ana_23").replace("%2$s", "Zombie").replace("%3$s", "Diamond Sword");
            for e in both(&msg) {
                match e {
                    Some(LogEvent::Death { player, message, .. }) => {
                        assert_eq!(player, "Ana_23", "{key}");
                        assert_eq!(message, msg);
                    }
                    other => panic!("{key}: {msg:?} -> {other:?}"),
                }
            }
        }
    }

    #[test]
    fn specific_death_samples() {
        let cases = [
            ("Ana was slain by Zombie", "death.attack.mob"),
            ("Ana fell from a high place", "death.fell.accident.generic"),
            ("Ana tried to swim in lava", "death.attack.lava"),
            ("Ana was shot by Skeleton using [Bow of Doom]", "death.attack.arrow.item"),
            ("Ana drowned", "death.attack.drown"),
            ("Ana hit the ground too hard while trying to escape Creeper", "death.attack.fall.player"),
        ];
        for (msg, key) in cases {
            match both(msg)[1].clone() {
                Some(LogEvent::Death { key: k, .. }) => assert_eq!(k, key, "{msg}"),
                other => panic!("{msg}: {other:?}"),
            }
        }
        // Mensagem do servidor qualquer não vira morte
        assert_eq!(both("Preparing spawn area: 83%")[0], None);
        assert_eq!(both("Ana lost connection: Disconnected")[0], None);
    }
}
