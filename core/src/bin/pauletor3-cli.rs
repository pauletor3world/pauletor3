//! pauletor3-cli — a mesma lógica do app, pela linha de comando (testes e emergências).
//!
//! Pasta de dados: `--home` ou `PAULETOR3_HOME` (padrão: a mesma do app).
//! `--local-remote <pasta>` usa uma pasta local (via rclone `:local:`) em vez do Google Drive.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use pauletor3_core::events::{Event, EventSink};
use pauletor3_core::paths::{self, Paths};
use pauletor3_core::remote::RcloneRemote;
use pauletor3_core::session::{EndReason, Engine};
use pauletor3_core::{health, util};

#[derive(Parser)]
#[command(name = "pauletor3-cli", version, about = "pAuletOr 3 pela linha de comando")]
struct Cli {
    /// Pasta de dados do app.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Usa uma pasta local como "nuvem" (testes).
    #[arg(long, global = true)]
    local_remote: Option<PathBuf>,
    /// Caminho do rclone.exe.
    #[arg(long, global = true)]
    rclone: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra quem está hospedando, o HEAD e o estado local.
    Status,
    /// Hospeda o mundo e fica rodando até a sessão acabar (Ctrl+C encerra com segurança).
    Play {
        /// Assumir uma sessão abandonada (id da trava).
        #[arg(long)]
        takeover: Option<String>,
        /// Encerra depois de N segundos (testes).
        #[arg(long)]
        stop_after: Option<u64>,
        /// Força um salvamento automático depois de N segundos (testes).
        #[arg(long)]
        checkpoint_after: Option<u64>,
    },
    /// Envia o que ficou de uma sessão que caiu neste PC.
    Recover,
    /// Lista as versões do mundo.
    Timeline,
    /// Restaura uma versão (cria uma nova; nada é apagado).
    Restore { manifest: String },
    /// Coleta de lixo no remote.
    Gc {
        #[arg(long)]
        dry_run: bool,
    },
    /// Baixa o HEAD para a pasta local (sem hospedar).
    Pull,
    /// Checklist de saúde.
    Health,
    /// Relatório anonimizado.
    Diagnose,
    /// Estatísticas.
    Stats,
    /// Lê/define a configuração local (nome do jogador, dono, EULA...).
    Setup {
        #[arg(long)]
        player: Option<String>,
        #[arg(long)]
        owner: bool,
        #[arg(long)]
        accept_eula: bool,
        #[arg(long)]
        checkpoint_minutes: Option<u64>,
        #[arg(long)]
        grace_minutes: Option<u64>,
        #[arg(long)]
        direct_ip: bool,
    },
    /// Importa uma pasta de mundo como nova versão (dono).
    Import { folder: PathBuf },
    /// Lista os jogadores do mundo (UUIDs, último acesso, progresso).
    Players,
    /// Vincula o progresso de um UUID antigo a um nick.
    MigratePlayer { uuid: String, nick: String },
    /// Imprime o hash de cada arquivo sincronizado de uma pasta de servidor (para comparar PCs).
    Fingerprint { server_dir: PathBuf },
}

fn default_rclone() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let candidates = [
        exe.as_ref().and_then(|e| e.parent().map(|d| d.join("rclone.exe"))),
        exe.as_ref().and_then(|e| e.parent().map(|d| d.join("rclone-x86_64-pc-windows-msvc.exe"))),
        Some(PathBuf::from(r"D:\DevTools\rclone\rclone.exe")),
    ];
    candidates.into_iter().flatten().find(|p| p.exists()).unwrap_or_else(|| PathBuf::from("rclone"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let home = cli
        .home
        .clone()
        .or_else(paths::resolve_data_dir)
        .ok_or_else(|| anyhow::anyhow!("pasta de dados não definida; use --home"))?;
    let p = Paths::new(&home);
    p.ensure()?;
    let _log = pauletor3_core::logging::init(&p.logs(), true)?;
    let rclone = cli.rclone.clone().unwrap_or_else(default_rclone);

    let printer: EventSink = Arc::new(|e: Event| match e {
        Event::Sync(s) => eprint!("\r{:<22} {:>5.1}%   ", s.phase.label(), s.percent()),
        Event::Notice { title, body, .. } => eprintln!("\n» {title} {body}"),
        Event::PlayerJoined { name } => eprintln!("\n+ {name}"),
        Event::PlayerLeft { name } => eprintln!("\n- {name}"),
        Event::ServerLog { line } => {
            if std::env::var_os("PAULETOR3_CONSOLE").is_some() {
                eprintln!("  | {line}")
            }
        }
        Event::StatusChanged => {}
    });

    let engine = match &cli.local_remote {
        Some(dir) => {
            std::fs::create_dir_all(dir)?;
            let r = RcloneRemote::local(rclone.clone(), dir, p.tmp());
            Engine::with_remote(p.clone(), Arc::new(r), printer)?
        }
        None => Engine::open(p.clone(), rclone.clone(), printer)?,
    };
    tokio::spawn(engine.journal.clone().run());

    match cli.cmd {
        Cmd::Status => {
            engine.refresh().await?;
            println!("{}", serde_json::to_string_pretty(&engine.status())?);
        }
        Cmd::Setup { player, owner, accept_eula, checkpoint_minutes, grace_minutes, direct_ip } => {
            let c = engine
                .update_config(|c| {
                    if let Some(p) = player {
                        c.player_name = p;
                    }
                    if owner {
                        c.is_owner = true;
                    }
                    if accept_eula {
                        c.eula_accepted = true;
                    }
                    if let Some(m) = checkpoint_minutes {
                        c.checkpoint_minutes = m;
                    }
                    if let Some(m) = grace_minutes {
                        c.grace_minutes = m;
                    }
                    if direct_ip {
                        c.address_mode = pauletor3_core::config::AddressMode::DirectIp;
                    }
                    c.setup_complete = !c.player_name.is_empty();
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&c)?);
        }
        Cmd::Play { takeover, stop_after, checkpoint_after } => {
            engine.play(takeover.into_iter().collect(), false).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("\nhospedando em {}", engine.status().address.unwrap_or_default());
            if let Some(secs) = checkpoint_after {
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                match engine.force_checkpoint().await {
                    Ok(()) => println!("\ncheckpoint publicado"),
                    Err(e) => {
                        eprintln!("\ncheckpoint falhou: {e}; encerrando a sessão com segurança");
                        engine.stop(EndReason::UserStop).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                        anyhow::bail!("checkpoint falhou: {e}");
                    }
                }
            }
            let wait = async {
                loop {
                    if !engine.is_hosting().await {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            };
            tokio::select! {
                _ = wait => {}
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\nencerrando com segurança…");
                    engine.stop(EndReason::UserStop).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                _ = async {
                    match stop_after {
                        Some(s) => tokio::time::sleep(std::time::Duration::from_secs(s)).await,
                        None => std::future::pending().await,
                    }
                } => {
                    engine.stop(EndReason::UserStop).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                }
            }
            println!("\nsessão encerrada; HEAD = {:?}", engine.status().head.map(|h| h.id));
        }
        Cmd::Recover => {
            engine.recover().await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("ok");
        }
        Cmd::Timeline => {
            for m in engine.timeline().await? {
                println!(
                    "{}  {:<22} {:<12} {:>10}  {}",
                    m.id,
                    m.kind.label(),
                    m.host_player,
                    util::human_bytes(m.total_size),
                    m.note.unwrap_or_default()
                );
            }
        }
        Cmd::Restore { manifest } => {
            engine.restore(manifest).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("restaurado");
        }
        Cmd::Gc { dry_run } => {
            let r = engine.gc(dry_run).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Cmd::Pull => {
            let se = pauletor3_core::sync::SyncEngine::new(engine.remote().await?, p.clone());
            let o = se.pull(Arc::new(|_| {})).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{o:?}");
        }
        Cmd::Health => {
            let _ = engine.refresh().await;
            for c in health::run(&engine).await {
                println!("{:?}\t{}\t{}", c.level, c.title, c.detail);
            }
        }
        Cmd::Diagnose => {
            let _ = engine.refresh().await;
            let checks = health::run(&engine).await;
            println!("{}", pauletor3_core::diagnostics::report(&engine, &checks).await);
        }
        Cmd::Stats => {
            println!("{}", serde_json::to_string_pretty(&engine.stats().await?)?);
        }
        Cmd::Import { folder } => {
            engine.import_world(folder).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("
importado; HEAD = {:?}", engine.timeline().await?.first().map(|m| m.id.clone()));
        }
        Cmd::Players => {
            for p in engine.world_players().await.map_err(|e| anyhow::anyhow!("{e}"))? {
                println!(
                    "{}  v{}  {:<16}  xp {:>3}  itens {:>3}  {:<22} {}{}",
                    p.uuid,
                    p.uuid_version,
                    p.last_played.map(|t| t.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string()).unwrap_or_default(),
                    p.xp_level,
                    p.items,
                    p.dimension.trim_start_matches("minecraft:"),
                    p.nick.map(|n| format!("nick {n}")).unwrap_or_default(),
                    if p.singleplayer_owner { "  (dono do single-player)" } else { "" }
                );
            }
        }
        Cmd::MigratePlayer { uuid, nick } => {
            let m = engine.migrate_player(uuid, nick).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{}", serde_json::to_string_pretty(&m)?);
        }
        Cmd::Fingerprint { server_dir } => {
            for (k, v) in pauletor3_core::sync::SyncEngine::fingerprint(&server_dir)? {
                println!("{}  {}", v.hash, k);
            }
        }
    }
    // dá um instante para o diário esvaziar a fila
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    Ok(())
}
