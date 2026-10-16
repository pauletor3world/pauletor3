//! Checklist de saúde: cada item com status verde/amarelo/vermelho e uma ação de correção.

use serde::Serialize;

use crate::config::AddressMode;
use crate::session::Engine;
use crate::versions::Versions;
use crate::{duckdns, firewall, java, server, tailscale, util};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub title: &'static str,
    pub level: Level,
    pub detail: String,
    /// Ação que a interface oferece ("fix_firewall", "install_tailscale"...).
    pub fix: Option<&'static str>,
    pub fix_label: Option<&'static str>,
}

fn ok(id: &'static str, title: &'static str, detail: impl Into<String>) -> Check {
    Check { id, title, level: Level::Ok, detail: detail.into(), fix: None, fix_label: None }
}
fn bad(id: &'static str, title: &'static str, level: Level, detail: impl Into<String>, fix: Option<(&'static str, &'static str)>) -> Check {
    Check { id, title, level, detail: detail.into(), fix: fix.map(|f| f.0), fix_label: fix.map(|f| f.1) }
}

pub async fn run(engine: &Engine) -> Vec<Check> {
    let cfg = engine.config.read().await.clone();
    let mut out = vec![];

    // Tailscale
    let ts = tailscale::status().await;
    out.push(if !ts.installed {
        bad("tailscale", "Tailscale", Level::Error, "Não está instalado.", Some(("install_tailscale", "Instalar")))
    } else if ts.running() {
        ok("tailscale", "Tailscale", format!("Conectado ({})", ts.ipv4.clone().unwrap_or_default()))
    } else {
        bad("tailscale", "Tailscale", Level::Error, format!("Desconectado ({}).", ts.backend_state), Some(("connect_tailscale", "Conectar")))
    });
    if let Some(exp) = cfg.tailscale_key_expires {
        let days = (exp - chrono::Local::now().date_naive()).num_days();
        if days <= 14 {
            out.push(bad(
                "tailscale_key",
                "Chave de convite do Tailscale",
                if days < 0 { Level::Error } else { Level::Warn },
                if days < 0 {
                    "Expirou: novos amigos não conseguem entrar. Gere outra no painel do Tailscale.".to_string()
                } else {
                    format!("Expira em {days} dia(s). Gere outra no painel do Tailscale e um novo convite.")
                },
                Some(("open_tailscale_keys", "Como gerar")),
            ));
        }
    }

    // Remote
    let remote = engine.remote().await;
    let (remote_check, versions) = match remote {
        Err(e) => (bad("remote", "Nuvem (Google Drive)", Level::Error, e.to_string(), Some(("reconnect_remote", "Conectar"))), None),
        Ok(r) => match Versions::load(&*r).await {
            Ok(v) => (ok("remote", "Nuvem (Google Drive)", "Acessível."), v),
            Err(e) => (bad("remote", "Nuvem (Google Drive)", Level::Error, e.to_string(), Some(("reconnect_remote", "Reconectar"))), None),
        },
    };
    out.push(remote_check);

    // Java + versões
    let v = versions.clone().unwrap_or_else(Versions::builtin);
    match java::find_java(&engine.paths, &v) {
        Some(j) => {
            let ver = java::java_version(&j).await.unwrap_or_else(|| "?".into());
            out.push(ok("java", "Java 21", ver));
        }
        None => out.push(bad("java", "Java 21", Level::Warn, "Ainda não baixado (baixa sozinho ao jogar).", Some(("prepare", "Baixar agora")))),
    }
    out.push(match (&versions, server::verify_install(&engine.paths, &v)) {
        (None, _) => bad("versions", "Versões do servidor", Level::Warn, "O dono ainda não publicou as versões (acontece no primeiro Jogar).", None),
        (Some(v), Ok(())) => ok(
            "versions",
            "Versões do servidor",
            format!("Minecraft {} · Fabric {} · Lithium {}", v.minecraft, v.fabric_loader, v.lithium_version),
        ),
        (Some(_), Err(e)) => bad("versions", "Versões do servidor", Level::Warn, format!("{e}"), Some(("prepare", "Baixar e conferir"))),
    });

    // Endereço
    match (cfg.address_mode, &cfg.duckdns_domain) {
        (AddressMode::Duckdns, Some(d)) => {
            let ips = duckdns::resolve(d).await.unwrap_or_default();
            let holder_ip = match engine.status().lock {
                Some(crate::lock::LockStatus::Held { lock }) => lock.tailscale_ip,
                _ => None,
            };
            out.push(match (ips.first(), holder_ip) {
                (None, _) => bad("duckdns", "Endereço fixo (DuckDNS)", Level::Warn, format!("{} não resolve agora.", duckdns::fqdn(d)), Some(("use_direct_ip", "Usar IP direto"))),
                (Some(ip), Some(h)) if ip.to_string() != h => bad(
                    "duckdns",
                    "Endereço fixo (DuckDNS)",
                    Level::Warn,
                    format!("Aponta para {ip}, mas o host está em {h} (o DNS pode levar 1 min para atualizar)."),
                    None,
                ),
                (Some(ip), _) => ok("duckdns", "Endereço fixo (DuckDNS)", format!("{} → {ip}", duckdns::fqdn(d))),
            });
        }
        (AddressMode::Duckdns, None) => out.push(bad("duckdns", "Endereço fixo (DuckDNS)", Level::Warn, "Não configurado.", Some(("open_settings", "Configurar")))),
        (AddressMode::DirectIp, _) => out.push(ok("duckdns", "Endereço", "Modo IP direto (o app atualiza a entrada no Minecraft).")),
    }

    // servers.dat
    if let Some(mc) = &cfg.minecraft_dir {
        let want = cfg.public_address(None);
        let cur = crate::serversdat::current_address(mc);
        out.push(match (want, cur) {
            (Some(w), Some(c)) if w == c => ok("serversdat", "Entrada \"pAuletOr 3\" no Minecraft", c),
            (Some(_), _) => bad("serversdat", "Entrada \"pAuletOr 3\" no Minecraft", Level::Warn, "Falta ou está desatualizada.", Some(("fix_serversdat", "Corrigir"))),
            (None, Some(c)) => ok("serversdat", "Entrada \"pAuletOr 3\" no Minecraft", c),
            (None, None) => bad("serversdat", "Entrada \"pAuletOr 3\" no Minecraft", Level::Warn, "Ainda não criada.", None),
        });
    }

    // Disco
    let free = util::free_space(&engine.paths.root).unwrap_or(0);
    let world = engine.status().head.map(|h| h.total_size).unwrap_or(0);
    let need = world.saturating_mul(3) + 2 * 1024 * 1024 * 1024;
    out.push(if free >= need {
        ok("disk", "Espaço em disco", format!("{} livres", util::human_bytes(free)))
    } else {
        bad("disk", "Espaço em disco", if free >= need / 2 { Level::Warn } else { Level::Error }, format!("Só {} livres; o ideal é {}.", util::human_bytes(free), util::human_bytes(need)), None)
    });

    // Firewall
    out.push(if firewall::rules_present().await {
        ok("firewall", "Firewall do Windows", "Porta liberada só para o Tailscale; RCON bloqueado.")
    } else {
        bad("firewall", "Firewall do Windows", Level::Warn, "Regras não criadas (necessárias para hospedar).", Some(("fix_firewall", "Corrigir (pede administrador)")))
    });

    // Diário
    let j = engine.journal.status();
    if engine.secrets().await.discord_webhook.is_some() {
        out.push(match &j.last_error {
            None => ok("discord", "Diário no Discord", if j.pending > 0 { format!("{} mensagem(ns) na fila", j.pending) } else { "Em dia.".into() }),
            Some(e) => bad("discord", "Diário no Discord", Level::Warn, e.clone(), Some(("open_settings", "Configurar"))),
        });
    }

    // Recuperação pendente
    if crate::sync::LocalState::load(&engine.paths).dirty && !engine.is_hosting().await {
        out.push(bad("recovery", "Sessão interrompida", Level::Error, "Há uma sessão deste PC que não foi enviada.", Some(("recover", "Recuperar agora"))));
    }
    out
}
