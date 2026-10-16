//! pauletor3-cli — a mesma lógica do app, pela linha de comando (testes e emergências).
//!
//! Pasta de dados: `--home` ou `PAULETOR3_HOME` (padrão: a mesma do app). Com pastas
//! diferentes, dá para simular vários "PCs" na mesma máquina.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use pauletor3_core::coord::InviteOptions;
use pauletor3_core::events::{Event, EventSink};
use pauletor3_core::paths::{self, Paths};
use pauletor3_core::session::{EndReason, Engine};
use pauletor3_core::{health, util};

#[derive(Parser)]
#[command(name = "pauletor3-cli", version, about = "pAuletOr 3 pela linha de comando")]
struct Cli {
    /// Pasta de dados do app.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra o estado do mundo (quem hospeda, membros, HEAD, réplica).
    Status,
    /// Baixa/confere o Minecraft e abre o jogo direto, entrando no mundo (build pessoal).
    Jogo {
        /// Só baixa e confere, sem abrir.
        #[arg(long)]
        so_baixar: bool,
    },
    /// Dono: cria o mundo na coordenação (e importa uma pasta, se dada).
    CreateWorld {
        #[arg(long)]
        coord: String,
        #[arg(long)]
        admin: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        nick: String,
        #[arg(long)]
        import: Option<PathBuf>,
    },
    /// Amigo: entra no mundo pelo link (ou código) do convite.
    Join {
        invite: String,
        #[arg(long)]
        nick: String,
    },
    /// Dono: cria um link de convite.
    Invite {
        #[arg(long, default_value_t = 48)]
        hours: u32,
        #[arg(long, default_value_t = 5)]
        uses: u32,
        /// Quem entrar precisa da sua aprovação.
        #[arg(long)]
        approval: bool,
    },
    /// Lista os membros.
    Members,
    /// Dono: aprova um membro (id ou nick).
    Approve { who: String },
    /// Dono: remove um membro (id ou nick).
    Remove { who: String },
    /// Liga a rede e fica online como o app (presença, proxy, réplica e failover automático).
    /// Com `--play`, também hospeda.
    Up {
        #[arg(long)]
        play: bool,
        /// Encerra (com segurança) depois de N segundos.
        #[arg(long)]
        stop_after: Option<u64>,
        /// Força um salvamento automático depois de N segundos (testes).
        #[arg(long)]
        checkpoint_after: Option<u64>,
        /// Simula queda de energia depois de N segundos hospedando (testes de failover).
        #[arg(long)]
        crash_after: Option<u64>,
        /// Encerra (com segurança) N segundos depois de virar host (testes de failover).
        #[arg(long)]
        stop_after_hosting: Option<u64>,
    },
    /// Hospeda até a sessão acabar (atalho para `up --play`).
    Play {
        #[arg(long)]
        stop_after: Option<u64>,
        #[arg(long)]
        checkpoint_after: Option<u64>,
    },
    /// Envia o que ficou de uma sessão que caiu neste PC.
    Recover,
    /// Lista as versões do mundo.
    Timeline,
    /// Diário do mundo.
    Diary,
    /// Restaura uma versão (cria uma nova; nada é apagado).
    Restore { manifest: String },
    /// Coleta de lixo na nuvem.
    Gc {
        #[arg(long)]
        dry_run: bool,
    },
    /// Baixa o HEAD para a pasta local (sem hospedar).
    Pull,
    /// Baixa e confere Java e servidor (o app faz isso no fim do assistente).
    Prepare,
    /// Checklist de saúde.
    Health,
    /// Relatório anonimizado.
    Diagnose,
    /// Estatísticas.
    Stats,
    /// Lê/define a configuração local.
    Setup {
        #[arg(long)]
        player: Option<String>,
        #[arg(long)]
        accept_eula: bool,
        #[arg(long)]
        checkpoint_minutes: Option<u64>,
        #[arg(long)]
        grace_minutes: Option<u64>,
        #[arg(long)]
        proxy_port: Option<u16>,
        #[arg(long)]
        auto_failover: Option<bool>,
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

fn e(err: pauletor3_core::NmError) -> anyhow::Error {
    anyhow::anyhow!("{err}")
}

async fn member_id(engine: &Engine, who: &str) -> anyhow::Result<String> {
    let v = engine.view().await.ok_or_else(|| anyhow::anyhow!("sem fotografia do mundo"))?;
    v.members
        .iter()
        .find(|m| m.id == who || m.nick.eq_ignore_ascii_case(who))
        .map(|m| m.id.clone())
        .ok_or_else(|| anyhow::anyhow!("membro não encontrado: {who}"))
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

    let engine = Engine::open(p.clone(), printer)?;
    tokio::spawn(engine.journal.clone().run());

    match cli.cmd {
        Cmd::Jogo { so_baixar } => {
            let en = engine.clone();
            let tick = tokio::spawn(async move {
                loop {
                    if let Some(p) = en.status().client_prep {
                        eprint!("\r{:<22} {:>5.1}%  {}/{} arquivos   ", p.phase.label(), p.percent(), p.files_done, p.files_total);
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            });
            let r = engine.client_now(!so_baixar).await;
            tick.abort();
            r.map_err(e)?;
            println!("\n{}", if so_baixar { "Minecraft pronto" } else { "Minecraft aberto" });
        }
        Cmd::Status => {
            engine.refresh().await.map_err(e)?;
            println!("{}", serde_json::to_string_pretty(&engine.status())?);
        }
        Cmd::CreateWorld { coord, admin, name, nick, import } => {
            engine.create_world(&coord, &admin, &name, &nick, import).await.map_err(e)?;
            println!("mundo criado: {}", engine.config.read().await.world_id.clone().unwrap_or_default());
        }
        Cmd::Join { invite, nick } => {
            let r = engine.join_world(&invite, &nick).await.map_err(e)?;
            println!(
                "você entrou no mundo {} de {}{}",
                r.name,
                r.owner_nick,
                if r.approved { "" } else { " (aguardando aprovação do dono)" }
            );
        }
        Cmd::Invite { hours, uses, approval } => {
            let (_, link) = engine.create_invite(InviteOptions { hours, uses, preapproved: !approval }).await.map_err(e)?;
            println!("{link}");
        }
        Cmd::Members => {
            engine.refresh().await.map_err(e)?;
            for m in engine.status().members {
                println!(
                    "{}  {:<16} {:<7} {}{}{}",
                    &m.id[..12],
                    m.nick,
                    format!("{:?}", m.role).to_lowercase(),
                    if m.approved { "" } else { "pendente " },
                    if m.online { "online " } else { "" },
                    if m.hosting { "hospedando" } else { "" }
                );
            }
        }
        Cmd::Approve { who } => {
            engine.refresh().await.map_err(e)?;
            let id = member_id(&engine, &who).await?;
            engine.set_member(&id, Some(true), false).await.map_err(e)?;
            println!("aprovado");
        }
        Cmd::Remove { who } => {
            engine.refresh().await.map_err(e)?;
            let id = member_id(&engine, &who).await?;
            engine.set_member(&id, None, true).await.map_err(e)?;
            println!("removido");
        }
        Cmd::Up { play, stop_after, checkpoint_after, crash_after, stop_after_hosting } => {
            run_up(&engine, play, stop_after, checkpoint_after, crash_after, stop_after_hosting).await?;
        }
        Cmd::Play { stop_after, checkpoint_after } => {
            run_up(&engine, true, stop_after, checkpoint_after, None, None).await?;
        }
        Cmd::Recover => {
            engine.recover().await.map_err(e)?;
            println!("ok");
        }
        Cmd::Timeline => {
            for m in engine.timeline().await.map_err(e)? {
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
        Cmd::Diary => {
            for ev in engine.diary(100).await.map_err(e)? {
                println!("{}  {:<12} {}", ev.at.with_timezone(&chrono::Local).format("%d/%m %H:%M"), ev.kind, ev.text);
            }
        }
        Cmd::Restore { manifest } => {
            engine.restore(manifest).await.map_err(e)?;
            println!("restaurado");
        }
        Cmd::Gc { dry_run } => {
            let r = engine.gc(dry_run).await.map_err(e)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Cmd::Pull => {
            let b = engine.backend().await.map_err(e)?;
            let se = pauletor3_core::sync::SyncEngine::new(b.remote, b.coord, p.clone());
            let o = se.pull(Arc::new(|_| {})).await.map_err(e)?;
            println!("{o:?}");
        }
        Cmd::Prepare => {
            let v = match engine.backend().await {
                Ok(b) => pauletor3_core::versions::Versions::load(&*b.remote).await.map_err(e)?.unwrap_or_else(pauletor3_core::versions::Versions::builtin),
                Err(_) => pauletor3_core::versions::Versions::builtin(),
            };
            let java = pauletor3_core::java::ensure_java(&p, &v, pauletor3_core::events::quiet()).await.map_err(e)?;
            pauletor3_core::server::prepare(&p, &v, pauletor3_core::events::quiet()).await.map_err(e)?;
            pauletor3_core::server::verify_install(&p, &v).map_err(e)?;
            pauletor3_core::server::warm_up(&p, &java).await.map_err(e)?;
            println!("Java e servidor prontos");
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
            println!("{}", serde_json::to_string_pretty(&engine.stats().await.map_err(e)?)?);
        }
        Cmd::Setup { player, accept_eula, checkpoint_minutes, grace_minutes, proxy_port, auto_failover } => {
            let c = engine
                .update_config(|c| {
                    if let Some(p) = player {
                        c.player_name = p;
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
                    if let Some(pp) = proxy_port {
                        c.proxy_port = pp;
                    }
                    if let Some(a) = auto_failover {
                        c.auto_failover = a;
                    }
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&c)?);
        }
        Cmd::Import { folder } => {
            engine.import_world(folder).await.map_err(e)?;
            println!("\nimportado; HEAD = {:?}", engine.timeline().await.map_err(e)?.first().map(|m| m.id.clone()));
        }
        Cmd::Players => {
            for p in engine.world_players().await.map_err(e)? {
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
            let m = engine.migrate_player(uuid, nick).await.map_err(e)?;
            println!("{}", serde_json::to_string_pretty(&m)?);
        }
        Cmd::Fingerprint { server_dir } => {
            for (k, v) in pauletor3_core::sync::SyncEngine::fingerprint(&server_dir)? {
                println!("{}  {}", v.hash, k);
            }
        }
    }
    // dá um instante para o diário esvaziar a fila
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(())
}

/// Fica online como o app. Imprime mudanças de estado (para os testes de ponta a ponta lerem).
async fn run_up(
    engine: &Arc<Engine>,
    play: bool,
    stop_after: Option<u64>,
    checkpoint_after: Option<u64>,
    crash_after: Option<u64>,
    stop_after_hosting: Option<u64>,
) -> anyhow::Result<()> {
    engine.start_network().await.map_err(e)?;
    let _ = engine.refresh().await;
    if let Some(a) = engine.proxy_addr().await {
        println!("proxy local em {a}");
    }
    if play {
        engine.play_with(pauletor3_core::session::PlayTrigger::Button, false).await.map_err(e)?;
        println!("\nhospedando");
    }
    let started = tokio::time::Instant::now();
    let mut checkpointed = false;
    let mut last_line = String::new();
    let mut hosting_since: Option<tokio::time::Instant> = None;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nencerrando com segurança…");
                engine.stop(EndReason::UserStop).await.map_err(e)?;
                break;
            }
        }
        let st = engine.status();
        let host = st.lease.as_ref().and_then(|l| l.held().map(|l| l.player.clone()));
        let line = format!(
            "fase={:?} hospedando={} host={} replica={}/{} jogando={} failover={}",
            st.phase,
            st.me_hosting,
            host.unwrap_or_else(|| "-".into()),
            st.replica_seq,
            st.head_seq.unwrap_or(0),
            st.playing,
            st.failover.clone().unwrap_or_default()
        );
        if line != last_line {
            let epoch = chrono::Utc::now().timestamp_millis();
            println!("[{:>5}s] [t={epoch}] {line}", started.elapsed().as_secs());
            last_line = line;
        }
        if st.me_hosting && hosting_since.is_none() {
            hosting_since = Some(tokio::time::Instant::now());
        }
        if let (Some(s), Some(since)) = (stop_after_hosting, hosting_since) {
            if since.elapsed() >= Duration::from_secs(s) {
                engine.stop(EndReason::UserStop).await.map_err(e)?;
                break;
            }
        }
        if let Some(s) = checkpoint_after {
            if !checkpointed && st.me_hosting && started.elapsed() >= Duration::from_secs(s) {
                checkpointed = true;
                match engine.force_checkpoint().await {
                    Ok(()) => println!("checkpoint publicado"),
                    Err(err) => eprintln!("checkpoint falhou: {err}"),
                }
            }
        }
        if let Some(s) = crash_after {
            if st.me_hosting && started.elapsed() >= Duration::from_secs(s) {
                println!("SIMULANDO QUEDA DE ENERGIA t={}", chrono::Utc::now().timestamp_millis());
                engine.simulate_crash().await;
                std::process::exit(9);
            }
        }
        if let Some(s) = stop_after {
            if started.elapsed() >= Duration::from_secs(s) {
                engine.stop(EndReason::UserStop).await.map_err(e)?;
                engine.wait_successor(Duration::from_secs(90)).await;
                break;
            }
        }
    }
    println!("encerrado; HEAD = {:?}", engine.status().head.map(|h| h.id));
    engine.stop_network().await;
    Ok(())
}
