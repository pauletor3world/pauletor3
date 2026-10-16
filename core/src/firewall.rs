//! Regras do Firewall do Windows.
//!
//! - Libera a porta do jogo (TCP 25565) **somente** para a faixa do Tailscale (100.64.0.0/10).
//! - Bloqueia a porta do RCON (TCP 25575) para qualquer origem remota. Regras de bloqueio
//!   têm precedência sobre as de liberação no Windows, então nem uma liberação ampla do
//!   `java.exe` (aquela janela "Permitir acesso") expõe o RCON.
//!
//! Criar regras exige administrador. Para não pedir UAC a cada sessão, as regras são criadas
//! uma vez (assistente ou "Corrigir" no checklist) e removidas na desinstalação.
//! Elas são inofensivas fora de sessão: só valem para tráfego vindo do Tailscale.

use crate::{GAME_PORT, RCON_PORT};

pub const RULE_GAME: &str = "pAuletOr 3 - Minecraft (somente Tailscale)";
pub const RULE_RCON: &str = "pAuletOr 3 - bloqueio do RCON";
pub const TAILSCALE_RANGE: &str = "100.64.0.0/10";

/// Comandos netsh para criar as regras (primeiro apaga versões antigas).
pub fn add_commands() -> Vec<String> {
    vec![
        format!("netsh advfirewall firewall delete rule name=\"{RULE_GAME}\""),
        format!("netsh advfirewall firewall delete rule name=\"{RULE_RCON}\""),
        format!(
            "netsh advfirewall firewall add rule name=\"{RULE_GAME}\" dir=in action=allow protocol=TCP localport={GAME_PORT} remoteip={TAILSCALE_RANGE} profile=any"
        ),
        format!(
            "netsh advfirewall firewall add rule name=\"{RULE_RCON}\" dir=in action=block protocol=TCP localport={RCON_PORT} remoteip=any profile=any"
        ),
    ]
}

pub fn remove_commands() -> Vec<String> {
    vec![
        format!("netsh advfirewall firewall delete rule name=\"{RULE_GAME}\""),
        format!("netsh advfirewall firewall delete rule name=\"{RULE_RCON}\""),
    ]
}

/// Confere se as duas regras existem (não precisa de administrador).
pub async fn rules_present() -> bool {
    if !cfg!(windows) {
        return true;
    }
    for name in [RULE_GAME, RULE_RCON] {
        let out = crate::util::command("netsh")
            .args(["advfirewall", "firewall", "show", "rule"])
            .arg(format!("name={name}"))
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => {}
            _ => return false,
        }
    }
    true
}

/// Executa os comandos com elevação (uma única janela do UAC).
pub async fn run_elevated(commands: &[String]) -> anyhow::Result<()> {
    let script = commands.join(" & ");
    // Start-Process -Verb RunAs abre o UAC; -Wait espera terminar.
    let ps = format!(
        "$p = Start-Process -FilePath cmd.exe -ArgumentList '/c {}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
        script.replace('\'', "''")
    );
    let out = crate::util::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "o firewall não foi configurado (permissão de administrador negada?) {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub async fn install_rules() -> anyhow::Result<()> {
    run_elevated(&add_commands()).await?;
    if !rules_present().await {
        anyhow::bail!("as regras do firewall não aparecem depois da criação");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rules_are_scoped() {
        let c = super::add_commands().join("\n");
        assert!(c.contains("remoteip=100.64.0.0/10"));
        assert!(c.contains("action=block protocol=TCP localport=25575"));
    }
}
