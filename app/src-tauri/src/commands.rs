//! Comandos chamados pela interface (invoke).

use std::sync::Arc;

use machencraft_core::config::Config;
use machencraft_core::coord::{DiaryEvent, Invite, PrivateSettings};
use machencraft_core::error::HumanError;
use machencraft_core::health::{self, Check};
use machencraft_core::journal::{Category, JournalSettings, JournalStatus};
use machencraft_core::launcher::{self, LauncherCandidate};
use machencraft_core::manifest::ManifestSummary;
use machencraft_core::players::{Migration, WorldPlayer};
use machencraft_core::retention::GcReport;
use machencraft_core::session::{EndReason, Engine, Status};
use machencraft_core::stats::Stats;
use machencraft_core::versions::WorldSettings;
use machencraft_core::{serversdat, NmError};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::AppState;

/// Erro que a interface recebe: sempre com título humano.
#[derive(Debug, Serialize)]
pub struct CmdError(pub Box<HumanError>);

impl From<NmError> for CmdError {
    fn from(e: NmError) -> Self {
        tracing::warn!(error = %e, "comando falhou");
        CmdError(Box::new(e.human()))
    }
}

impl From<anyhow::Error> for CmdError {
    fn from(e: anyhow::Error) -> Self {
        CmdError::from(NmError::Other(e))
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::from(NmError::from(e))
    }
}

impl CmdError {
    pub fn msg(title: &str, detail: impl Into<String>) -> Self {
        CmdError(Box::new(HumanError { code: "other", title: title.into(), detail: detail.into(), lease: None }))
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

pub fn engine(state: &State<'_, AppState>) -> CmdResult<Arc<Engine>> {
    state.engine().ok_or_else(|| CmdError::msg("Falta terminar a configuração", "Abra o assistente."))
}

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub configured: bool,
    pub has_engine: bool,
    pub data_dir: Option<String>,
    pub is_owner: bool,
    pub start_minimized: bool,
}

#[tauri::command]
pub async fn app_ready(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if !state.start_minimized {
        crate::show_main(&app);
    }
    Ok(())
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let e = state.engine();
    let (configured, owner, dir) = match &e {
        Some(e) => {
            let c = e.config.read().await;
            (c.ready(), c.is_owner, Some(e.paths.root.display().to_string()))
        }
        None => (false, false, None),
    };
    Ok(AppInfo {
        version: machencraft_core::APP_VERSION.into(),
        configured,
        has_engine: e.is_some(),
        data_dir: dir,
        is_owner: owner,
        start_minimized: state.start_minimized,
    })
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> CmdResult<Status> {
    Ok(engine(&state)?.status())
}

#[tauri::command]
pub async fn refresh(state: State<'_, AppState>) -> CmdResult<Status> {
    let e = engine(&state)?;
    e.refresh().await?;
    Ok(e.status())
}

#[tauri::command]
pub async fn play(state: State<'_, AppState>, open_game: bool) -> CmdResult<()> {
    let e = engine(&state)?;
    e.play(open_game).await?;
    Ok(())
}

/// Abre o Minecraft (com a entrada "pAuletOr 3" pronta) sem mexer no mundo.
#[tauri::command]
pub async fn open_game(state: State<'_, AppState>) -> CmdResult<()> {
    engine(&state)?.open_game().await;
    Ok(())
}

#[tauri::command]
pub async fn take_turn(state: State<'_, AppState>, open_game: bool) -> CmdResult<()> {
    Ok(engine(&state)?.take_turn(open_game).await?)
}

#[tauri::command]
pub async fn pass_turn(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(engine(&state)?.pass_turn().await?)
}

#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(engine(&state)?.stop(EndReason::UserStop).await?)
}

#[tauri::command]
pub async fn save_now(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(engine(&state)?.force_checkpoint().await?)
}

#[tauri::command]
pub async fn recover(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(engine(&state)?.recover().await?)
}

#[tauri::command]
pub async fn timeline(state: State<'_, AppState>) -> CmdResult<Vec<ManifestSummary>> {
    Ok(engine(&state)?.timeline().await?)
}

#[tauri::command]
pub async fn restore(state: State<'_, AppState>, manifest_id: String) -> CmdResult<()> {
    Ok(engine(&state)?.restore(manifest_id).await?)
}

#[tauri::command]
pub async fn stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    Ok(engine(&state)?.stats().await?)
}

#[tauri::command]
pub async fn health(state: State<'_, AppState>) -> CmdResult<Vec<Check>> {
    let e = engine(&state)?;
    let _ = e.refresh().await;
    Ok(health::run(&e).await)
}

/// Ação de correção de um item do checklist.
#[tauri::command]
pub async fn run_fix(_app: AppHandle, state: State<'_, AppState>, fix: String) -> CmdResult<String> {
    let e = engine(&state)?;
    match fix.as_str() {
        "fix_serversdat" => {
            let cfg = e.config.read().await.clone();
            let mc = cfg.minecraft_dir.clone().ok_or_else(|| CmdError::msg("Pasta .minecraft não definida", "Defina nas Configurações."))?;
            if launcher::client_running(None) {
                return Err(CmdError::msg("Feche o Minecraft primeiro", "A lista de servidores só pode ser alterada com o jogo fechado."));
            }
            let addr = cfg.game_address();
            serversdat::ensure_entry(&mc, &addr, &e.paths.backups())?;
            Ok(format!("Entrada \"pAuletOr 3\" aponta para {addr}."))
        }
        "retry_network" => {
            e.start_network().await?;
            e.refresh().await?;
            Ok("Conexão restabelecida.".into())
        }
        "recover" => {
            e.recover().await?;
            Ok("Sessão recuperada.".into())
        }
        "prepare" => {
            crate::setup::prepare_runtime_inner(&e).await?;
            Ok("Java e servidor prontos.".into())
        }
        "prepare_client" => {
            e.prefetch_client().await;
            Ok("Baixando o Minecraft; o progresso aparece no Início.".into())
        }
        "open_settings" | "open_owner" => Ok(String::new()),
        other => Err(CmdError::msg("Ação desconhecida", other)),
    }
}

#[tauri::command]
pub async fn diagnostics(state: State<'_, AppState>) -> CmdResult<String> {
    let e = engine(&state)?;
    let _ = e.refresh().await;
    let checks = health::run(&e).await;
    Ok(machencraft_core::diagnostics::report(&e, &checks).await)
}

/// Configurações sem segredos (só diz se cada segredo existe).
#[derive(Serialize)]
pub struct SettingsView {
    pub config: Config,
    pub has_admin_token: bool,
    pub data_dir: String,
    pub build: &'static str,
    pub auto_heap_mb: u64,
    pub total_ram_mb: u64,
    /// Este build sabe abrir o Minecraft direto (build pessoal).
    pub direct_launch_available: bool,
    pub auto_client_heap_mb: u64,
    pub journal_categories: Vec<(Category, String, bool)>,
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<SettingsView> {
    let e = engine(&state)?;
    let c = e.config.read().await.clone();
    let s = e.secrets().await;
    let total = machencraft_core::heap::total_ram_mb();
    Ok(SettingsView {
        journal_categories: Category::ALL.iter().map(|k| (*k, k.label().to_string(), c.journal.is_on(*k))).collect(),
        config: c,
        has_admin_token: s.admin_token.is_some(),
        data_dir: e.paths.root.display().to_string(),
        build: if machencraft_core::OFFLINE_MODE { "pessoal" } else { "padrão" },
        auto_heap_mb: machencraft_core::heap::auto_heap_mb(total),
        total_ram_mb: total,
        direct_launch_available: machencraft_core::OFFLINE_MODE,
        auto_client_heap_mb: machencraft_core::heap::auto_client_heap_mb(total, None),
    })
}

#[derive(Deserialize)]
pub struct SettingsPatch {
    pub player_name: Option<String>,
    pub minecraft_dir: Option<String>,
    pub launcher_path: Option<String>,
    pub direct_launch: Option<bool>,
    /// Versão da .minecraft para abrir o jogo ("" = vanilla do app).
    pub client_version: Option<String>,
    /// 0 = automático
    pub ram_mb: Option<u64>,
    /// 0 = automático
    pub client_ram_mb: Option<u64>,
    pub checkpoint_minutes: Option<u64>,
    pub grace_minutes: Option<u64>,
    pub notify_toasts: Option<bool>,
    pub notify_join_leave: Option<bool>,
    pub proxy_port: Option<u16>,
    pub auto_failover: Option<bool>,
    pub wake_on_join: Option<bool>,
    pub background_replica: Option<bool>,
    pub journal: Option<Vec<(Category, bool)>>,
}

/// Versões do Minecraft que dá para abrir direto: o vanilla do app e as da .minecraft que são
/// da versão do mundo (OptiFine, Fabric, Quilt).
#[tauri::command]
pub async fn client_versions(state: State<'_, AppState>) -> CmdResult<Vec<machencraft_core::instancias::ClientVersion>> {
    let e = engine(&state)?;
    let mc = e.config.read().await.minecraft_dir.clone();
    let minecraft = e.world_minecraft_version().await;
    Ok(machencraft_core::instancias::list(mc.as_deref(), &minecraft))
}

/// "Restaurar como cópia": a versão vira um mundo de single-player.
#[tauri::command]
pub async fn restore_copy(state: State<'_, AppState>, manifest_id: String) -> CmdResult<String> {
    Ok(engine(&state)?.restore_as_copy(manifest_id).await?)
}

/// "Nosso Mundo em números" (estatísticas do próprio Minecraft).
#[tauri::command]
pub async fn world_numbers(state: State<'_, AppState>) -> CmdResult<machencraft_core::numeros::Numeros> {
    Ok(engine(&state)?.numeros())
}

/// "Chamar pro jogo" um membro do grupo.
#[tauri::command]
pub async fn call_member(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(engine(&state)?.chamar(&id).await?)
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, patch: SettingsPatch) -> CmdResult<()> {
    let e = engine(&state)?;
    if let Some(n) = &patch.player_name {
        if !Config::is_valid_player_name(n) {
            return Err(CmdError::msg("Nick inválido", "Use de 3 a 16 letras, números ou _ (igual ao do Minecraft)."));
        }
        if e.is_hosting().await {
            return Err(CmdError::msg("Feche a sessão antes", "O nick não pode mudar com o mundo aberto."));
        }
    }
    e.update_config(|c| {
        if let Some(v) = patch.player_name {
            c.player_name = v;
        }
        if let Some(v) = patch.minecraft_dir {
            c.minecraft_dir = (!v.is_empty()).then(|| v.into());
        }
        if let Some(v) = patch.launcher_path {
            c.launcher_path = (!v.is_empty()).then(|| v.into());
        }
        if let Some(v) = patch.client_version.clone() {
            c.client_version = (!v.is_empty()).then_some(v);
        }
        if let Some(v) = patch.direct_launch {
            c.direct_launch = v;
        }
        if let Some(v) = patch.ram_mb {
            c.ram_mb = (v > 0).then_some(v);
        }
        if let Some(v) = patch.client_ram_mb {
            c.client_ram_mb = (v > 0).then_some(v);
        }
        if let Some(v) = patch.checkpoint_minutes {
            c.checkpoint_minutes = v.clamp(1, 60);
        }
        if let Some(v) = patch.grace_minutes {
            c.grace_minutes = v.clamp(1, 30);
        }
        if let Some(v) = patch.notify_toasts {
            c.notify_toasts = v;
        }
        if let Some(v) = patch.notify_join_leave {
            c.notify_join_leave = v;
        }
        if let Some(v) = patch.proxy_port {
            c.proxy_port = v.max(1024);
        }
        if let Some(v) = patch.auto_failover {
            c.auto_failover = v;
        }
        if let Some(v) = patch.wake_on_join {
            c.wake_on_join = v;
        }
        if let Some(v) = patch.background_replica {
            c.background_replica = v;
        }
        if let Some(v) = patch.journal {
            let mut j = JournalSettings::default();
            for (k, on) in v {
                j.enabled.insert(k, on);
            }
            c.journal = j;
        }
    })
    .await?;
    if patch.direct_launch == Some(true) {
        e.prefetch_client().await;
    }
    Ok(())
}

/// Configurações privadas do mundo (guardadas na coordenação; só membros leem).
#[tauri::command]
pub async fn get_private(state: State<'_, AppState>) -> CmdResult<PrivateSettings> {
    Ok(engine(&state)?.private_settings().await?)
}

/// Webhook do Discord do diário (dono). Vazio = sem Discord.
#[tauri::command]
pub async fn set_webhook(state: State<'_, AppState>, url: String) -> CmdResult<()> {
    let v = url.trim().to_string();
    let w = (!v.is_empty()).then_some(v);
    Ok(engine(&state)?.set_private_settings(PrivateSettings { discord_webhook: w }).await?)
}

#[tauri::command]
pub async fn invites(state: State<'_, AppState>) -> CmdResult<Vec<Invite>> {
    Ok(engine(&state)?.invites().await?)
}

#[tauri::command]
pub async fn revoke_invite(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(engine(&state)?.revoke_invite(&id).await?)
}

/// Dono: aprova (`approved`) ou remove (`remove`) um membro.
#[tauri::command]
pub async fn set_member(state: State<'_, AppState>, id: String, approved: Option<bool>, remove: bool) -> CmdResult<()> {
    Ok(engine(&state)?.set_member(&id, approved, remove).await?)
}

#[tauri::command]
pub async fn diary(state: State<'_, AppState>, limit: Option<usize>) -> CmdResult<Vec<DiaryEvent>> {
    Ok(engine(&state)?.diary(limit.unwrap_or(200)).await?)
}

#[tauri::command]
pub async fn get_world_settings(state: State<'_, AppState>) -> CmdResult<WorldSettings> {
    Ok(engine(&state)?.world_settings().await?)
}

#[tauri::command]
pub async fn save_world_settings(state: State<'_, AppState>, settings: WorldSettings) -> CmdResult<()> {
    Ok(engine(&state)?.save_world_settings(settings).await?)
}

#[tauri::command]
pub async fn console_cmd(state: State<'_, AppState>, command: String) -> CmdResult<String> {
    Ok(engine(&state)?.console(&command).await?)
}

#[tauri::command]
pub async fn console_history(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    Ok(state.console.lock().unwrap().iter().cloned().collect())
}

/// Fim do console da última sessão hospedada neste PC (para o Console vazio).
#[tauri::command]
pub async fn last_session_log(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let e = engine(&state)?;
    let text = std::fs::read(e.paths.logs().join("servidor-console.log")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    Ok(lines[lines.len().saturating_sub(crate::CONSOLE_LINES)..].to_vec())
}

#[tauri::command]
pub async fn detect_launchers() -> CmdResult<Vec<LauncherCandidate>> {
    Ok(launcher::detect())
}

#[tauri::command]
pub async fn open_launcher(state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    let p = e
        .config
        .read()
        .await
        .launcher_path
        .clone()
        .ok_or_else(|| CmdError::msg("Launcher não configurado", "Escolha o launcher nas Configurações."))?;
    launcher::open(&p).map_err(|err| CmdError::msg("Não consegui abrir o launcher", err.to_string()))
}

#[tauri::command]
pub async fn world_players(state: State<'_, AppState>) -> CmdResult<Vec<WorldPlayer>> {
    Ok(engine(&state)?.world_players().await?)
}

#[tauri::command]
pub async fn migrate_player(state: State<'_, AppState>, uuid: String, nick: String) -> CmdResult<Migration> {
    Ok(engine(&state)?.migrate_player(uuid, nick).await?)
}

#[tauri::command]
pub async fn gc(state: State<'_, AppState>, dry_run: bool) -> CmdResult<GcReport> {
    Ok(engine(&state)?.gc(dry_run).await?)
}

#[tauri::command]
pub async fn journal_status(state: State<'_, AppState>) -> CmdResult<JournalStatus> {
    Ok(engine(&state)?.journal.status())
}

#[tauri::command]
pub async fn test_webhook(state: State<'_, AppState>, url: Option<String>) -> CmdResult<()> {
    let e = engine(&state)?;
    let url = match url.filter(|u| !u.trim().is_empty()) {
        Some(u) => u.trim().to_string(),
        None => e.private_settings().await?.discord_webhook.ok_or_else(|| CmdError::msg("Sem webhook", "Cole a URL do webhook primeiro."))?,
    };
    let world = e.status().world_name;
    machencraft_core::journal::send_test(&url, &world).await.map_err(|m| CmdError::msg("O teste não chegou ao Discord", m))
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> CmdResult<()> {
    crate::quit(&app);
    Ok(())
}

#[tauri::command]
pub async fn hide_window(app: AppHandle) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    Ok(())
}

