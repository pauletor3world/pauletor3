//! Primeira execução: o dono cria o mundo; um amigo entra pelo convite. Nada de contas de
//! terceiros, rede ou portas: só a pasta, o nick e (para o dono) o endereço da coordenação.

use std::path::PathBuf;
use std::sync::Arc;

use pauletor3_core::config::Config;
use pauletor3_core::coord::InviteOptions;
use pauletor3_core::events::quiet;
use pauletor3_core::invite::Invite;
use pauletor3_core::session::Engine;
use pauletor3_core::versions::Versions;
use pauletor3_core::{java, paths, server, util};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::{engine, CmdError, CmdResult};
use crate::AppState;

#[derive(Serialize)]
pub struct SetupSuggestion {
    pub suggested_dir: String,
    pub existing_dir: Option<String>,
    pub minecraft_dir: Option<String>,
    pub launchers: Vec<pauletor3_core::launcher::LauncherCandidate>,
    pub free_space: Option<u64>,
    /// Convite que abriu o app (link `pauletor3://...`), se houver.
    pub pending_invite: Option<String>,
}

#[tauri::command]
pub async fn setup_suggest_dir(state: State<'_, AppState>) -> CmdResult<SetupSuggestion> {
    let suggested = paths::suggest_data_dir();
    let parent = suggested.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| suggested.clone());
    Ok(SetupSuggestion {
        free_space: util::free_space(&parent),
        suggested_dir: suggested.display().to_string(),
        existing_dir: paths::resolve_data_dir().map(|p| p.display().to_string()),
        minecraft_dir: pauletor3_core::config::default_minecraft_dir().map(|p| p.display().to_string()),
        launchers: pauletor3_core::launcher::detect(),
        pending_invite: state.pending_invite.lock().unwrap().clone(),
    })
}

#[derive(Deserialize)]
pub struct SetupInit {
    pub data_dir: String,
    pub eula: bool,
    pub minecraft_dir: Option<String>,
    pub launcher_path: Option<String>,
}

/// Escolhe a pasta de dados, aceita o EULA e guarda onde está o Minecraft.
#[tauri::command]
pub async fn setup_init(app: AppHandle, args: SetupInit) -> CmdResult<()> {
    if !args.eula {
        return Err(CmdError::msg("É preciso aceitar o EULA", "Sem isso o servidor do Minecraft não pode rodar."));
    }
    let dir = PathBuf::from(args.data_dir.trim());
    if !dir.is_absolute() {
        return Err(CmdError::msg("Pasta inválida", "Escolha uma pasta completa, como D:\\pAuletOr3."));
    }
    std::fs::create_dir_all(&dir).map_err(|e| CmdError::msg("Não consegui criar a pasta", e.to_string()))?;
    paths::save_data_dir(&dir)?;
    let e = crate::open_engine(&app, dir)?;
    e.update_config(|c| {
        c.eula_accepted = true;
        if let Some(m) = args.minecraft_dir.filter(|s| !s.is_empty()) {
            c.minecraft_dir = Some(m.into());
        }
        if let Some(l) = args.launcher_path.filter(|s| !s.is_empty()) {
            c.launcher_path = Some(l.into());
        }
    })
    .await?;
    server::accept_eula(&e.paths)?;
    Ok(())
}

fn check_nick(nick: &str) -> CmdResult<()> {
    if !Config::is_valid_player_name(nick) {
        return Err(CmdError::msg("Nick inválido", "Use exatamente o nick do Minecraft: 3 a 16 letras, números ou _."));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct InvitePreview {
    pub world_id: String,
    pub host: String,
}

/// Confere um convite colado (sem gastar o uso).
#[tauri::command]
pub async fn invite_preview(invite: String) -> CmdResult<InvitePreview> {
    let inv = Invite::parse(&invite).map_err(|_| CmdError::msg("Convite inválido", "Copie o link inteiro (com a parte depois do #) ou o código P3J1."))?;
    Ok(InvitePreview { world_id: inv.world_id().to_string(), host: inv.base.clone() })
}

#[derive(Deserialize)]
pub struct CreateWorldArgs {
    pub coordinator_url: String,
    pub admin_token: String,
    pub name: String,
    pub nick: String,
    /// Pasta de um mundo existente para importar (opcional).
    pub folder: Option<String>,
}

/// O dono cria o mundo (e importa uma pasta, se escolheu).
#[tauri::command]
pub async fn create_world(state: State<'_, AppState>, args: CreateWorldArgs) -> CmdResult<()> {
    check_nick(&args.nick)?;
    let url = args.coordinator_url.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost")) {
        return Err(CmdError::msg("Endereço inválido", "Cole o endereço que o script de publicação mostrou (começa com https://)."));
    }
    let e = engine(&state)?;
    let folder = args.folder.filter(|f| !f.trim().is_empty()).map(PathBuf::from);
    if let Some(f) = &folder {
        if !f.join("level.dat").exists() {
            return Err(CmdError::msg("Essa pasta não é um mundo", "Escolha a pasta que tem o arquivo level.dat dentro."));
        }
    }
    e.create_world(&url, args.admin_token.trim(), args.name.trim(), &args.nick, folder).await?;
    e.start_network().await?;
    Ok(())
}

/// Um amigo entra pelo convite.
#[tauri::command]
pub async fn join_world(state: State<'_, AppState>, invite: String, nick: String) -> CmdResult<pauletor3_core::coord::http::JoinResult> {
    check_nick(&nick)?;
    let e = engine(&state)?;
    let r = e.join_world(&invite, &nick).await?;
    *state.pending_invite.lock().unwrap() = None;
    e.start_network().await?;
    Ok(r)
}

/// Lê o nível de uma pasta de mundo (para mostrar antes de importar).
#[tauri::command]
pub async fn probe_world_folder(folder: String) -> CmdResult<pauletor3_core::players::LevelInfo> {
    let f = PathBuf::from(folder);
    if !f.join("level.dat").exists() {
        return Err(CmdError::msg("Essa pasta não é um mundo", "Escolha a pasta que tem o arquivo level.dat dentro."));
    }
    let info = pauletor3_core::players::level_info(&f)?;
    let v = Versions::builtin();
    if info.data_version > v.world_data_version {
        return Err(CmdError::msg(
            "Mundo de uma versão mais nova",
            format!("O mundo é do Minecraft {} e o servidor é {}; abrir causaria perda de dados.", info.version_name, v.minecraft),
        ));
    }
    Ok(info)
}

/// Link de convite novo (dono).
#[tauri::command]
pub async fn make_invite(state: State<'_, AppState>, hours: Option<u32>, uses: Option<u32>, approval: Option<bool>) -> CmdResult<String> {
    let e = engine(&state)?;
    let opts = InviteOptions { hours: hours.unwrap_or(48), uses: uses.unwrap_or(5), preapproved: !approval.unwrap_or(false) };
    Ok(e.create_invite(opts).await?.1)
}

#[tauri::command]
pub async fn ensure_serversdat(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    crate::commands::run_fix(app, state, "fix_serversdat".into()).await
}

pub async fn prepare_runtime_inner(e: &Arc<Engine>) -> CmdResult<()> {
    let v = match e.backend().await {
        Ok(b) => Versions::load(&*b.remote).await?.unwrap_or_else(Versions::builtin),
        Err(_) => Versions::builtin(),
    };
    java::ensure_java(&e.paths, &v, quiet()).await?;
    server::prepare(&e.paths, &v, quiet()).await?;
    server::verify_install(&e.paths, &v)?;
    Ok(())
}

/// Baixa Java e servidor com antecedência (para o primeiro Jogar ser rápido).
#[tauri::command]
pub async fn prepare_runtime(state: State<'_, AppState>) -> CmdResult<()> {
    prepare_runtime_inner(&engine(&state)?).await
}

#[tauri::command]
pub async fn finish_setup(state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    e.update_config(|c| c.setup_complete = true).await?;
    // a entrada "pAuletOr 3" no Minecraft (localhost) já pode existir desde agora
    let cfg = e.config.read().await.clone();
    if let Some(mc) = &cfg.minecraft_dir {
        if !pauletor3_core::launcher::client_running(None) {
            let _ = pauletor3_core::serversdat::ensure_entry(mc, &cfg.game_address(), &e.paths.backups());
        }
    }
    let _ = e.refresh().await;
    Ok(())
}
