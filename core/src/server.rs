//! Ciclo do servidor: instalação conferida, `server.properties` gerenciado e processo Java.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::{broadcast, Mutex};

use crate::events::SyncProgressFn;
use crate::java;
use crate::logparse::{self, LogEvent};
use crate::paths::Paths;
use crate::rcon::Rcon;
use crate::versions::{Versions, WorldSettings};
use crate::{NmError, NmResult, OFFLINE_MODE, RCON_PORT};

pub const LAUNCHER_JAR: &str = "fabric-server-launch.jar";

/// Baixa e confere o servidor fixado (launcher do Fabric, server.jar oficial, Lithium).
/// Garante que a pasta `mods/` contém **só** o Lithium.
pub async fn prepare(paths: &Paths, v: &Versions, progress: SyncProgressFn) -> NmResult<()> {
    let dir = paths.server();
    std::fs::create_dir_all(dir.join("mods"))?;
    java::download_verified(&v.fabric_launcher, &dir.join(LAUNCHER_JAR), progress.clone()).await?;
    java::download_verified(&v.vanilla_server, &dir.join("server.jar"), progress.clone()).await?;
    let lithium = dir.join("mods").join(&v.lithium.filename);
    java::download_verified(&v.lithium, &lithium, progress).await?;
    enforce_only_lithium(&dir.join("mods"), &v.lithium.filename)?;
    // O launcher do Fabric 1.1.x ignora `serverJar` e procura o jar oficial em
    // `.fabric/server/<versão>-server.jar`; colocamos lá a cópia já conferida (sha1 da Mojang).
    let fabric_jar = fabric_vanilla_jar(paths, v);
    if v.vanilla_server.verify(&fabric_jar).is_err() {
        std::fs::create_dir_all(fabric_jar.parent().unwrap())?;
        std::fs::copy(dir.join("server.jar"), &fabric_jar)?;
    }
    std::fs::write(
        dir.join("fabric-server-launcher.properties"),
        "#Gerenciado pelo pAuletOr 3\nserverJar=server.jar\n",
    )?;
    Ok(())
}

/// Onde o launcher do Fabric procura o server.jar oficial.
pub fn fabric_vanilla_jar(paths: &Paths, v: &Versions) -> PathBuf {
    paths.server().join(".fabric").join("server").join(format!("{}-server.jar", v.minecraft))
}

fn enforce_only_lithium(mods: &Path, keep: &str) -> NmResult<()> {
    for e in std::fs::read_dir(mods)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name != keep && e.path().is_file() {
            tracing::warn!(file = %name, "removendo arquivo estranho da pasta mods");
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

/// Confere que o servidor instalado bate com as versões fixadas. Recusa hospedar se não bater.
pub fn verify_install(paths: &Paths, v: &Versions) -> NmResult<()> {
    let dir = paths.server();
    v.fabric_launcher.verify(&dir.join(LAUNCHER_JAR))?;
    v.vanilla_server.verify(&dir.join("server.jar"))?;
    v.vanilla_server.verify(&fabric_vanilla_jar(paths, v))?;
    v.lithium.verify(&dir.join("mods").join(&v.lithium.filename))?;
    let extras: Vec<String> = std::fs::read_dir(dir.join("mods"))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != &v.lithium.filename)
        .collect();
    if !extras.is_empty() {
        return Err(NmError::VersionMismatch(format!("mods não permitidos: {}", extras.join(", "))));
    }
    Ok(())
}

pub fn accept_eula(paths: &Paths) -> std::io::Result<()> {
    std::fs::create_dir_all(paths.server())?;
    std::fs::write(
        paths.server().join("eula.txt"),
        "#Aceito pelo jogador no assistente do pAuletOr 3 (https://aka.ms/MinecraftEULA)\neula=true\n",
    )
}

pub fn eula_accepted(paths: &Paths) -> bool {
    std::fs::read_to_string(paths.server().join("eula.txt"))
        .map(|s| s.lines().any(|l| l.trim() == "eula=true"))
        .unwrap_or(false)
}

/// Escapa um valor para arquivo `.properties` do Java (ISO-8859-1 com escapes \uXXXX).
pub fn escape_prop(v: &str) -> String {
    let mut out = String::new();
    for (i, c) in v.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if i == 0 => out.push_str("\\ "),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Aquece o servidor sem abrir mundo: o launcher do Fabric baixa as bibliotecas na primeira
/// execução (até 1 min) e o `--initSettings` do Minecraft sai logo depois de carregar. Assim o
/// primeiro "Jogar" (e uma troca de host) não esperam esse download. Idempotente.
pub async fn warm_up(paths: &Paths, java: &Path) -> NmResult<()> {
    let marker = paths.server().join(".fabric").join("aquecido");
    if marker.exists() {
        return Ok(());
    }
    let mut c = crate::util::command(java);
    c.arg("-jar").arg(LAUNCHER_JAR).arg("--initSettings").arg("nogui").current_dir(paths.server()).stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
    let mut child = c.spawn().map_err(|e| NmError::Other(anyhow::anyhow!("não consegui iniciar o Java: {e}")))?;
    match tokio::time::timeout(Duration::from_secs(600), child.wait()).await {
        Ok(Ok(st)) if st.success() => {
            if let Some(p) = marker.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&marker, b"ok")?;
            Ok(())
        }
        Ok(Ok(st)) => Err(NmError::Other(anyhow::anyhow!("o aquecimento do servidor terminou com {st}"))),
        Ok(Err(e)) => Err(NmError::Other(e.into())),
        Err(_) => {
            let _ = child.kill().await;
            Err(NmError::Other(anyhow::anyhow!("o aquecimento do servidor demorou demais")))
        }
    }
}

/// Quem pode entrar (gerado a partir dos membros aprovados do mundo).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct WhitelistEntry {
    pub uuid: String,
    pub name: String,
}

/// Grava o `whitelist.json` gerenciado (só membros aprovados do mundo).
pub fn write_whitelist(paths: &Paths, entries: &[WhitelistEntry]) -> NmResult<()> {
    let file = paths.server().join("whitelist.json");
    crate::util::write_atomic(&file, &serde_json::to_vec_pretty(entries)?)?;
    Ok(())
}

/// Gera o `server.properties` gerenciado. Chaves que não controlamos e que já existiam são mantidas.
///
/// O servidor escuta **só em 127.0.0.1**: quem conversa com ele é o proxy do app (que recebe
/// os amigos pelo túnel P2P). Nada fica exposto na rede local nem na internet, e por isso não
/// há regra de firewall nem porta aberta.
pub fn write_properties(paths: &Paths, s: &WorldSettings, port: u16, rcon_password: &str, whitelist: bool) -> NmResult<()> {
    s.validate()?;
    let file = paths.server().join("server.properties");
    let mut props: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(old) = std::fs::read_to_string(&file) {
        for line in old.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') || l.starts_with('!') {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                props.insert(k.trim().to_string(), v.to_string());
            }
        }
    }
    let managed: Vec<(&str, String)> = vec![
        // O build padrão exige conta original; o modo offline só existe no build `personal`.
        ("online-mode", (!OFFLINE_MODE).to_string()),
        ("enforce-secure-profile", (!OFFLINE_MODE).to_string()),
        ("white-list", whitelist.to_string()),
        ("enforce-whitelist", whitelist.to_string()),
        ("server-ip", "127.0.0.1".into()),
        ("server-port", port.to_string()),
        ("enable-rcon", "true".into()),
        ("rcon.port", RCON_PORT.to_string()),
        ("rcon.password", escape_prop(rcon_password)),
        ("broadcast-rcon-to-ops", "false".into()),
        ("enable-query", "false".into()),
        ("enable-status", "true".into()),
        ("level-name", "world".into()),
        ("level-seed", escape_prop(&s.seed)),
        ("motd", escape_prop(&s.motd)),
        ("difficulty", s.difficulty.clone()),
        ("gamemode", s.gamemode.clone()),
        ("hardcore", s.hardcore.to_string()),
        ("pvp", s.pvp.to_string()),
        ("view-distance", s.view_distance.to_string()),
        ("simulation-distance", s.simulation_distance.to_string()),
        ("max-players", s.max_players.to_string()),
        ("spawn-protection", s.spawn_protection.to_string()),
        ("allow-flight", s.allow_flight.to_string()),
        // Gravação síncrona de chunks: mais segura contra queda de energia.
        ("sync-chunk-writes", "true".into()),
        ("prevent-proxy-connections", "false".into()),
    ];
    for (k, v) in managed {
        props.insert(k.to_string(), v);
    }
    let mut out = String::from("#Gerenciado pelo pAuletOr 3. Alterações manuais nas chaves gerenciadas são sobrescritas.\n");
    for (k, v) in props {
        out.push_str(&format!("{k}={v}\n"));
    }
    crate::util::write_atomic(&file, out.as_bytes())?;
    Ok(())
}

/// Processo do servidor em execução.
pub struct ServerProcess {
    child: Mutex<Child>,
    pub pid: u32,
    rcon_addr: String,
    rcon_password: String,
    rcon: Mutex<Option<Rcon>>,
    events: broadcast::Sender<LogEvent>,
    lines: broadcast::Sender<String>,
}

pub struct LaunchOptions {
    pub java: PathBuf,
    pub heap_mb: u64,
    pub rcon_password: String,
}

impl ServerProcess {
    pub async fn launch(paths: &Paths, opts: LaunchOptions) -> NmResult<Arc<Self>> {
        let dir = paths.server();
        let mut c = crate::util::command(&opts.java);
        c.args(crate::heap::jvm_args(opts.heap_mb))
            .arg("-jar")
            .arg(LAUNCHER_JAR)
            .arg("nogui")
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| NmError::Other(anyhow::anyhow!("não consegui iniciar o Java: {e}")))?;
        let pid = child.id().unwrap_or(0);
        std::fs::write(paths.server().join("pauletor3.pid"), pid.to_string())?;
        let (events, _) = broadcast::channel(256);
        let (lines, _) = broadcast::channel(1024);
        for stream in [child.stdout.take().map(Either::Out), child.stderr.take().map(Either::Err)].into_iter().flatten() {
            let ev = events.clone();
            let ln = lines.clone();
            tokio::spawn(async move {
                let reader: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match stream {
                    Either::Out(s) => Box::new(s),
                    Either::Err(s) => Box::new(s),
                };
                // Lê bytes e converte com tolerância: o launcher do Fabric imprime no código de
                // página do Windows, e um único byte não-UTF-8 não pode derrubar a leitura.
                let mut r = BufReader::new(reader);
                let mut buf = Vec::with_capacity(512);
                loop {
                    buf.clear();
                    match r.read_until(b'\n', &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                    if let Some(e) = logparse::parse_line(&line) {
                        let _ = ev.send(e);
                    }
                    let _ = ln.send(line);
                }
            });
        }
        Ok(Arc::new(Self {
            child: Mutex::new(child),
            pid,
            rcon_addr: format!("127.0.0.1:{RCON_PORT}"),
            rcon_password: opts.rcon_password,
            rcon: Mutex::new(None),
            events,
            lines,
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_lines(&self) -> broadcast::Receiver<String> {
        self.lines.subscribe()
    }

    /// Espera o "Done" do servidor (ou erro fatal / processo morrer).
    pub async fn wait_ready(&self, mut rx: broadcast::Receiver<LogEvent>, timeout: Duration) -> NmResult<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if !self.running().await {
                return Err(NmError::Other(anyhow::anyhow!("o servidor fechou durante a inicialização (veja os logs)")));
            }
            match tokio::time::timeout_at(deadline.min(tokio::time::Instant::now() + Duration::from_secs(2)), rx.recv()).await {
                Ok(Ok(LogEvent::Ready)) => return Ok(()),
                Ok(Ok(LogEvent::Fatal(m))) => return Err(NmError::Other(anyhow::anyhow!(m))),
                Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    return Err(NmError::Other(anyhow::anyhow!("o servidor fechou")))
                }
                Err(_) if tokio::time::Instant::now() >= deadline => {
                    return Err(NmError::Other(anyhow::anyhow!("o servidor demorou demais para abrir")))
                }
                Err(_) => {}
            }
        }
    }

    pub async fn running(&self) -> bool {
        matches!(self.child.lock().await.try_wait(), Ok(None))
    }

    /// Executa um comando via RCON (reconecta se preciso).
    pub async fn cmd(&self, command: &str) -> NmResult<String> {
        let mut guard = self.rcon.lock().await;
        for attempt in 0..3 {
            if guard.is_none() {
                match Rcon::connect(&self.rcon_addr, &self.rcon_password).await {
                    Ok(r) => *guard = Some(r),
                    Err(e) if attempt < 2 => {
                        tracing::debug!(error = %e, "rcon ainda não disponível");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    Err(e) => return Err(NmError::Other(e.into())),
                }
            }
            match guard.as_mut().unwrap().cmd(command).await {
                Ok(s) => return Ok(s),
                Err(e) => {
                    tracing::warn!(error = %e, "rcon caiu; reconectando");
                    *guard = None;
                }
            }
        }
        Err(NmError::Other(anyhow::anyhow!("rcon indisponível")))
    }

    pub async fn players(&self) -> NmResult<Vec<String>> {
        let r = self.cmd("list").await?;
        crate::rcon::parse_list(&r)
            .map(|(_, n)| n)
            .ok_or_else(|| NmError::Other(anyhow::anyhow!("resposta de list inesperada: {r}")))
    }

    /// Mensagem no chat do jogo (para avisos de "passar a vez").
    pub async fn say(&self, text: &str) -> NmResult<()> {
        let json = serde_json::json!(["", {"text": "[pAuletOr 3] ", "color": "gold"}, {"text": text, "color": "yellow"}]);
        self.cmd(&format!("tellraw @a {json}")).await.map(|_| ())
    }

    /// Parada graciosa via `stop`. Mata o processo só como último recurso.
    /// Devolve `true` se parou sozinho.
    pub async fn stop(&self, timeout: Duration) -> bool {
        let sent = self.cmd("stop").await.is_ok();
        if !sent {
            // Sem RCON: tenta pelo console.
            let mut child = self.child.lock().await;
            if let Some(stdin) = child.stdin.as_mut() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(b"stop\n").await;
                let _ = stdin.flush().await;
            }
        }
        *self.rcon.lock().await = None;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut child = self.child.lock().await;
                if let Ok(Some(_)) = child.try_wait() {
                    return true;
                }
                if tokio::time::Instant::now() >= deadline {
                    tracing::error!("o servidor não parou a tempo; encerrando o processo à força");
                    let _ = child.kill().await;
                    return false;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn wait_exit(&self) {
        loop {
            if !self.running().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Mata o processo sem salvar (só para simular queda de energia nos testes).
    pub async fn kill(&self) {
        let _ = self.child.lock().await.kill().await;
    }
}

enum Either {
    Out(tokio::process::ChildStdout),
    Err(tokio::process::ChildStderr),
}

/// Se o app caiu com o servidor rodando, o processo pode ter ficado órfão.
pub fn orphan_server_pid(paths: &Paths) -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(paths.server().join("pauletor3.pid")).ok()?.trim().parse().ok()?;
    let mut s = sysinfo::System::new();
    s.refresh_processes();
    let p = s.process(sysinfo::Pid::from_u32(pid))?;
    let name = p.name().to_lowercase();
    (name.starts_with("java")).then_some(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_properties() {
        assert_eq!(escape_prop("pAuletOr 3"), "pAuletOr 3");
        assert_eq!(escape_prop("a=b:c"), "a\\=b\\:c");
        assert_eq!(escape_prop("Ação"), "A\\u00e7\\u00e3o");
        assert_eq!(escape_prop(" x"), "\\ x");
    }

    #[test]
    fn properties_are_managed_and_preserve_unknown() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        std::fs::create_dir_all(paths.server()).unwrap();
        std::fs::write(paths.server().join("server.properties"), "online-mode=true\nfoo=bar\n").unwrap();
        write_properties(&paths, &WorldSettings::default(), 25566, "senha", true).unwrap();
        let s = std::fs::read_to_string(paths.server().join("server.properties")).unwrap();
        assert!(s.contains(&format!("online-mode={}\n", !OFFLINE_MODE)));
        assert!(s.contains("white-list=true\n"));
        assert!(s.contains("server-ip=127.0.0.1\n"), "só localhost");
        assert!(s.contains("server-port=25566\n"));
        assert!(s.contains("enable-rcon=true\n"));
        assert!(s.contains("foo=bar\n"));
    }

    #[test]
    fn only_lithium_survives() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("lithium.jar"), b"x").unwrap();
        std::fs::write(d.path().join("hack.jar"), b"x").unwrap();
        enforce_only_lithium(d.path(), "lithium.jar").unwrap();
        assert!(d.path().join("lithium.jar").exists());
        assert!(!d.path().join("hack.jar").exists());
    }
}
