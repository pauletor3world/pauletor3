//! "Copiar diagnóstico": relatório anonimizado para pedir ajuda.
//! Remove tokens, chaves, webhooks, e-mails, IPs do Tailscale e o nome do usuário do Windows.

use std::sync::OnceLock;

use regex::Regex;

use crate::health::Check;
use crate::session::Engine;

fn patterns() -> &'static Vec<(Regex, &'static str)> {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            (Regex::new(r"https://(?:discord|discordapp)\.com/api/webhooks/\S+").unwrap(), "[webhook]"),
            (Regex::new(r"tskey-[A-Za-z0-9_-]+").unwrap(), "[tailscale-key]"),
            (Regex::new(r#""(access_token|refresh_token|token|password|rcon\.password)"\s*:\s*"[^"]*""#).unwrap(), "\"$1\":\"[oculto]\""),
            (Regex::new(r"(?i)(token|password|senha)=\S+").unwrap(), "$1=[oculto]"),
            (Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap(), "[email]"),
            (Regex::new(r"\b100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.\d{1,3}\.\d{1,3}\b").unwrap(), "100.x.x.x"),
            (Regex::new(r"P3.[A-Za-z0-9_-]{20,}").unwrap(), "[convite]"),
        ]
    })
}

pub fn scrub(text: &str) -> String {
    let mut s = text.to_string();
    for (re, rep) in patterns() {
        s = re.replace_all(&s, *rep).into_owned();
    }
    if let Ok(user) = std::env::var("USERNAME") {
        if user.len() >= 3 {
            s = s.replace(&user, "[usuario]");
        }
    }
    s
}

fn tail(path: &std::path::Path, lines: usize) -> String {
    let Ok(t) = std::fs::read_to_string(path) else { return String::new() };
    let v: Vec<&str> = t.lines().collect();
    v[v.len().saturating_sub(lines)..].join("\n")
}

fn newest_log(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("machencraft") || n.starts_with("pauletor3")).unwrap_or(false))
        .collect();
    v.sort();
    v.pop()
}

pub async fn report(engine: &Engine, checks: &[Check]) -> String {
    let st = engine.status();
    let cfg = engine.config.read().await.clone();
    let mut r = String::new();
    r.push_str("# Diagnóstico do Machen Craft\n\n");
    r.push_str(&format!("- App: {}\n", crate::APP_VERSION));
    r.push_str(&format!("- Windows: {}\n", sysinfo::System::long_os_version().unwrap_or_default()));
    r.push_str(&format!("- RAM: {} MB · heap: {} MB\n", crate::heap::total_ram_mb(), crate::heap::effective_heap_mb(cfg.ram_mb)));
    r.push_str(&format!("- Fase: {:?} · hospedando: {} · recuperação pendente: {}\n", st.phase, st.me_hosting, st.recovery_pending));
    r.push_str(&format!("- Checkpoint: {} min · tolerância: {} min · proxy: {}\n", cfg.checkpoint_minutes, cfg.grace_minutes, cfg.game_address()));
    r.push_str(&format!(
        "- Rede P2P: iniciada={} online={} · até o host: {} ({} ms) · coordenação ok={}\n",
        st.net.started,
        st.net.online,
        st.net.path_to_host.as_deref().unwrap_or("-"),
        st.net.rtt_ms.map(|m| m.to_string()).unwrap_or_else(|| "-".into()),
        st.coord_ok
    ));
    r.push_str(&format!("- Réplica: versão {} de {} · build: {}\n", st.replica_seq, st.head_seq.unwrap_or(0), if crate::OFFLINE_MODE { "pessoal" } else { "padrão" }));
    if let Some(h) = &st.head {
        r.push_str(&format!("- HEAD: {} ({:?}, {} arquivos, {})\n", h.id, h.kind, h.file_count, crate::util::human_bytes(h.total_size)));
    }
    if let Some(e) = &st.last_error {
        r.push_str(&format!("- Último erro: {} — {}\n", e.title, e.detail));
    }
    r.push_str("\n## Saúde\n\n");
    for c in checks {
        let icon = match c.level {
            crate::health::Level::Ok => "🟢",
            crate::health::Level::Warn => "🟡",
            crate::health::Level::Error => "🔴",
        };
        r.push_str(&format!("{icon} {} — {}\n", c.title, c.detail));
    }
    r.push_str("\n## Log do app (fim)\n\n```\n");
    if let Some(p) = newest_log(&engine.paths.logs()) {
        r.push_str(&tail(&p, 80));
    }
    r.push_str("\n```\n\n## Console do servidor (fim)\n\n```\n");
    r.push_str(&tail(&engine.paths.logs().join("servidor-console.log"), 60));
    r.push_str("\n```\n");
    scrub(&r)
}

#[cfg(test)]
mod tests {
    #[test]
    fn scrubs_secrets() {
        let t = "webhook https://discord.com/api/webhooks/123/abcDEF key tskey-auth-kXYZ-123 ip 100.101.5.6 \
                 email ana@gmail.com {\"refresh_token\":\"1//abc\"} rcon.password=xyz P3.AAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let s = super::scrub(t);
        for secret in ["abcDEF", "kXYZ", "100.101.5.6", "ana@gmail.com", "1//abc", "xyz", "P3.AAAA"] {
            assert!(!s.contains(secret), "{secret} vazou: {s}");
        }
        assert!(s.contains("100.x.x.x"));
    }
}
