//! Checklist de saúde: cada item com status verde/amarelo/vermelho e uma ação de correção.

use serde::Serialize;

use crate::session::Engine;
use crate::versions::Versions;
use crate::{java, server, util};

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
    /// Ação que a interface oferece ("retry_network", "prepare", "recover"...).
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
    let st = engine.status();
    let mut out = vec![];

    // Coordenação
    let backend = engine.backend().await;
    let coord_t = std::time::Instant::now();
    let view = match &backend {
        Ok(b) => b.coord.view().await,
        Err(e) => Err(crate::NmError::NotConfigured(e.to_string())),
    };
    out.push(match &view {
        Ok(v) => ok("coord", "Coordenação do mundo", format!("Acessível ({} ms) · {} membro(s)", coord_t.elapsed().as_millis(), v.members.len())),
        Err(crate::NmError::Forbidden(m)) => bad("coord", "Coordenação do mundo", Level::Error, m.clone(), None),
        Err(e) => bad("coord", "Coordenação do mundo", Level::Error, format!("{e}"), Some(("retry_network", "Tentar de novo"))),
    });

    // Rede P2P
    out.push(match (&st.net.started, st.net.online) {
        (false, _) => bad("p2p", "Conexão entre os PCs (P2P)", Level::Error, "A rede P2P não iniciou.", Some(("retry_network", "Iniciar"))),
        (true, false) => bad("p2p", "Conexão entre os PCs (P2P)", Level::Warn, "Sem contato com os servidores de apoio; conexões podem demorar.", Some(("retry_network", "Tentar de novo"))),
        (true, true) => {
            let how = match (&st.net.path_to_host, st.net.rtt_ms) {
                (Some(p), Some(ms)) => format!("Ligado ao host por conexão {p} ({ms} ms)."),
                (Some(p), None) => format!("Ligado ao host por conexão {p}."),
                _ => "Pronta.".into(),
            };
            if st.net.path_to_host.as_deref() == Some("relay") {
                bad("p2p", "Conexão entre os PCs (P2P)", Level::Warn, format!("{how} A conexão direta não foi possível (provável CGNAT); o jogo funciona, com um pouco mais de ping."), None)
            } else {
                ok("p2p", "Conexão entre os PCs (P2P)", how)
            }
        }
    });

    // Proxy local
    out.push(match (&st.net.proxy_port, &st.net.proxy_error) {
        (Some(p), _) => ok("proxy", "Endereço no Minecraft", format!("{} (porta {p} deste PC)", cfg.game_address())),
        (None, Some(e)) => bad("proxy", "Endereço no Minecraft", Level::Error, e.clone(), Some(("open_settings", "Trocar a porta"))),
        (None, None) => bad("proxy", "Endereço no Minecraft", Level::Warn, "Ainda não iniciado.", Some(("retry_network", "Iniciar"))),
    });

    // Nuvem do mundo
    let versions = match &backend {
        Ok(b) => match Versions::load(&*b.remote).await {
            Ok(v) => {
                out.push(ok("cloud", "Nuvem do mundo", "Acessível (cifrada com a chave do mundo)."));
                v
            }
            Err(e) => {
                out.push(bad("cloud", "Nuvem do mundo", Level::Error, format!("{e}"), Some(("retry_network", "Tentar de novo"))));
                None
            }
        },
        Err(_) => None,
    };

    // Réplica local
    match st.head_seq {
        Some(h) if st.replica_seq >= h => out.push(ok("replica", "Cópia do mundo neste PC", "Em dia: Jogar abre em segundos.")),
        Some(_) if st.replicating => out.push(ok("replica", "Cópia do mundo neste PC", "Atualizando agora...")),
        Some(_) if !cfg.background_replica => out.push(bad("replica", "Cópia do mundo neste PC", Level::Warn, "Desatualizada (a cópia em segundo plano está desligada).", Some(("open_settings", "Ligar")))),
        Some(_) => out.push(bad("replica", "Cópia do mundo neste PC", Level::Warn, "Desatualizada; o app atualiza sozinho quando houver amigos online ou na nuvem.", None)),
        None => out.push(ok("replica", "Cópia do mundo neste PC", "O mundo ainda não foi aberto nenhuma vez.")),
    }

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
        (None, _) => bad("versions", "Versões do servidor", Level::Warn, "O dono ainda não publicou as versões.", None),
        (Some(v), Ok(())) => ok(
            "versions",
            "Versões do servidor",
            format!("Minecraft {} · Fabric {} · Lithium {}", v.minecraft, v.fabric_loader, v.lithium_version),
        ),
        (Some(_), Err(e)) => bad("versions", "Versões do servidor", Level::Warn, format!("{e}"), Some(("prepare", "Baixar e conferir"))),
    });

    // Minecraft aberto direto pelo app
    if cfg.launches_directly() {
        let title = "Minecraft deste PC";
        out.push(match (v.client_meta(), &st.client_prep) {
            (None, _) => bad("client", title, Level::Error, format!("O app não conhece o cliente do Minecraft {}.", v.minecraft), None),
            (Some(_), Some(p)) => ok("client", title, format!("Baixando... {:.0}%", p.percent())),
            (Some(m), None) if crate::client::is_ready(&engine.paths, &m) => ok("client", title, format!("Minecraft {} pronto: Jogar abre o jogo já dentro do mundo.", v.minecraft)),
            (Some(_), None) => bad("client", title, Level::Warn, "Ainda não baixado (baixa sozinho no primeiro Jogar).", Some(("prepare_client", "Baixar agora"))),
        });
    }

    // servers.dat (só importa quando o jogo abre por um launcher)
    if let (Some(mc), false) = (&cfg.minecraft_dir, cfg.launches_directly()) {
        let want = cfg.game_address();
        let cur = crate::serversdat::current_address(mc);
        out.push(match cur {
            Some(c) if c == want => ok("serversdat", "Entrada \"pAuletOr 3\" no Minecraft", c),
            _ => bad("serversdat", "Entrada \"pAuletOr 3\" no Minecraft", Level::Warn, "Falta ou está diferente.", Some(("fix_serversdat", "Corrigir"))),
        });
    }

    // Disco
    let free = util::free_space(&engine.paths.root).unwrap_or(0);
    let world = st.head.as_ref().map(|h| h.total_size).unwrap_or(0);
    let need = world.saturating_mul(3) + 2 * 1024 * 1024 * 1024;
    out.push(if free >= need {
        ok("disk", "Espaço em disco", format!("{} livres", util::human_bytes(free)))
    } else {
        bad("disk", "Espaço em disco", if free >= need / 2 { Level::Warn } else { Level::Error }, format!("Só {} livres; o ideal é {}.", util::human_bytes(free), util::human_bytes(need)), None)
    });

    // Diário no Discord (opcional)
    let j = engine.journal.status();
    if let Some(e) = &j.last_error {
        out.push(bad("discord", "Diário no Discord", Level::Warn, e.clone(), Some(("open_owner", "Configurar"))));
    }

    // Recuperação pendente
    if crate::sync::LocalState::load(&engine.paths).dirty && !engine.is_hosting().await {
        out.push(bad("recovery", "Sessão interrompida", Level::Error, "Há uma sessão deste PC que não foi enviada.", Some(("recover", "Recuperar agora"))));
    }
    out
}
