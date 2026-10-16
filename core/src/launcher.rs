//! Detecção do launcher (TLauncher / oficial) e do processo do cliente Minecraft.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LauncherCandidate {
    pub kind: &'static str,
    pub label: String,
    pub path: PathBuf,
}

/// Procura launchers nos lugares de instalação conhecidos.
pub fn detect() -> Vec<LauncherCandidate> {
    let mut out = vec![];
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut push = |kind: &'static str, label: &str, p: Option<PathBuf>| {
        if let Some(p) = p {
            if p.is_file() && !out.iter().any(|c: &LauncherCandidate| c.path == p) {
                out.push(LauncherCandidate { kind, label: label.into(), path: p });
            }
        }
    };
    // TLauncher: normalmente dentro da .minecraft, ou na pasta de programas do usuário.
    push("tlauncher", "TLauncher", env("APPDATA").map(|a| a.join(".minecraft").join("TLauncher.exe")));
    push("tlauncher", "TLauncher", env("APPDATA").map(|a| a.join(".tlauncher").join("TLauncher.exe")));
    push("tlauncher", "TLauncher", env("LOCALAPPDATA").map(|a| a.join("Programs").join("TLauncher").join("TLauncher.exe")));
    push("tlauncher", "TLauncher", env("USERPROFILE").map(|a| a.join("Desktop").join("TLauncher.exe")));
    // Launcher oficial (instalador clássico e versão Xbox/Microsoft Store).
    push("official", "Launcher oficial", env("ProgramFiles(x86)").map(|p| p.join("Minecraft Launcher").join("MinecraftLauncher.exe")));
    push("official", "Launcher oficial", env("ProgramFiles").map(|p| p.join("Minecraft Launcher").join("MinecraftLauncher.exe")));
    push("official", "Launcher oficial (Xbox)", Some(PathBuf::from(r"C:\XboxGames\Minecraft Launcher\Content\Minecraft.exe")));
    out
}

/// Abre o launcher configurado.
pub fn open(path: &Path) -> std::io::Result<()> {
    let mut c = std::process::Command::new(path);
    if let Some(dir) = path.parent() {
        c.current_dir(dir);
    }
    c.spawn().map(|_| ())
}

/// Se há um cliente Minecraft rodando (exclui o servidor, pelo PID).
/// O cliente roda como `java`/`javaw` com a classe principal `net.minecraft.client.main.Main`
/// (ou passando `--accessToken`/`--assetsDir`, que só o cliente recebe).
pub fn client_running(exclude_pid: Option<u32>) -> bool {
    let mut s = sysinfo::System::new();
    s.refresh_processes();
    s.processes().iter().any(|(pid, p)| {
        if Some(pid.as_u32()) == exclude_pid {
            return false;
        }
        let name = p.name().to_lowercase();
        if !(name.starts_with("java") || name.starts_with("javaw")) {
            return false;
        }
        let cmd = p.cmd().join(" ");
        is_client_cmdline(&cmd)
    })
}

pub fn is_client_cmdline(cmd: &str) -> bool {
    !cmd.contains("fabric-server-launch.jar")
        && (cmd.contains("net.minecraft.client.main.Main")
            || cmd.contains("--assetsDir")
            || cmd.contains("--accessToken"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn classifies_cmdlines() {
        assert!(super::is_client_cmdline("javaw -Xmx2G -cp x net.minecraft.client.main.Main --username Ana"));
        assert!(super::is_client_cmdline("java ... --assetsDir C:/x --version 1.21.11"));
        assert!(!super::is_client_cmdline("java -Xmx4G -jar fabric-server-launch.jar nogui"));
        assert!(!super::is_client_cmdline("java -jar outra-coisa.jar"));
    }
}
