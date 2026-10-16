//! Assistentes: primeira configuração do dono e entrada de um amigo pelo convite.

use std::path::PathBuf;
use std::sync::Arc;

use pauletor3_core::config::Config;
use pauletor3_core::events::quiet;
use pauletor3_core::invite::{self, InvitePayload};
use pauletor3_core::manifest::ManifestSummary;
use pauletor3_core::remote::rclone::setup as rclone_setup;
use pauletor3_core::session::Engine;
use pauletor3_core::sync::SyncEngine;
use pauletor3_core::tailscale;
use pauletor3_core::versions::{Versions, WorldSettings};
use pauletor3_core::{firewall, java, paths, server, util};
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
}

#[tauri::command]
pub async fn setup_suggest_dir() -> CmdResult<SetupSuggestion> {
    let suggested = paths::suggest_data_dir();
    let parent = suggested.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| suggested.clone());
    Ok(SetupSuggestion {
        free_space: util::free_space(&parent),
        suggested_dir: suggested.display().to_string(),
        existing_dir: paths::resolve_data_dir().map(|p| p.display().to_string()),
        minecraft_dir: pauletor3_core::config::default_minecraft_dir().map(|p| p.display().to_string()),
        launchers: pauletor3_core::launcher::detect(),
    })
}

#[derive(Deserialize)]
pub struct SetupInit {
    pub data_dir: String,
    pub player_name: String,
    pub eula: bool,
    pub owner: bool,
    pub minecraft_dir: Option<String>,
    pub launcher_path: Option<String>,
}

#[tauri::command]
pub async fn setup_init(app: AppHandle, args: SetupInit) -> CmdResult<()> {
    if !Config::is_valid_player_name(&args.player_name) {
        return Err(CmdError::msg("Nick inválido", "Use exatamente o nick do Minecraft: 3 a 16 letras, números ou _."));
    }
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
        c.player_name = args.player_name.clone();
        c.eula_accepted = true;
        c.is_owner = args.owner;
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

async fn write_remote(e: &Arc<Engine>, backend: &str, params: Vec<(String, String)>) -> CmdResult<()> {
    let pass = util::random_token(40);
    rclone_setup::create_encrypted_config(e.rclone_bin(), &e.rclone_config_path(), &pass, backend, &params).await?;
    let json = serde_json::to_string(&params).map_err(|err| CmdError::msg("Erro interno", err.to_string()))?;
    e.update_secrets(|s| {
        s.rclone_config_pass = Some(pass);
        s.remote_backend = Some(backend.to_string());
        s.remote_params_json = Some(json);
    })
    .await?;
    // Testa de verdade: lista a pasta do mundo.
    let r = e.remote().await?;
    r.list("").await.map_err(|err| CmdError::msg("A nuvem não respondeu", err.to_string()))?;
    Ok(())
}

/// Abre o navegador para o login do Google (conta dedicada ao mundo) e conecta o remote.
/// Com client ID próprio, usa o escopo mínimo `drive.file`: o app só enxerga o que ele mesmo criou.
#[tauri::command]
pub async fn drive_authorize(state: State<'_, AppState>, client_id: Option<String>, client_secret: Option<String>) -> CmdResult<()> {
    let e = engine(&state)?;
    let id = client_id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let secret = client_secret.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let client = match (&id, &secret) {
        (Some(i), Some(s)) => {
            if !i.ends_with(".apps.googleusercontent.com") {
                return Err(CmdError::msg("Client ID inválido", "Ele termina com .apps.googleusercontent.com"));
            }
            Some((i.as_str(), s.as_str()))
        }
        (None, None) => None,
        _ => return Err(CmdError::msg("Falta uma parte", "Informe o Client ID e o Client secret juntos.")),
    };
    let token = rclone_setup::authorize_drive(e.rclone_bin(), client).await?;
    let mut params = vec![("token".to_string(), token)];
    match client {
        Some((i, s)) => {
            params.push(("scope".into(), "drive.file".into()));
            params.push(("client_id".into(), i.to_string()));
            params.push(("client_secret".into(), s.to_string()));
        }
        None => params.push(("scope".into(), "drive".into())),
    }
    write_remote(&e, "drive", params).await
}

#[derive(Serialize)]
pub struct WorldProbe {
    pub has_world: bool,
    pub head: Option<ManifestSummary>,
    pub settings: Option<WorldSettings>,
    pub versions: Option<Versions>,
}

#[tauri::command]
pub async fn world_probe(state: State<'_, AppState>) -> CmdResult<WorldProbe> {
    let e = engine(&state)?;
    let r = e.remote().await?;
    let se = SyncEngine::new(r.clone(), e.paths.clone());
    let head = match se.head().await? {
        Some(h) => Some(se.fetch_manifest(&h.manifest).await?.summary()),
        None => None,
    };
    Ok(WorldProbe {
        has_world: head.is_some(),
        head,
        settings: WorldSettings::load(&*r).await?,
        versions: Versions::load(&*r).await?,
    })
}

#[tauri::command]
pub async fn import_world(state: State<'_, AppState>, folder: String) -> CmdResult<pauletor3_core::players::LevelInfo> {
    let e = engine(&state)?;
    let f = PathBuf::from(&folder);
    let info = pauletor3_core::players::level_info(&f).map_err(|err| CmdError::msg("Essa pasta não é um mundo", err.to_string()))?;
    e.import_world(f).await?;
    Ok(info)
}

#[tauri::command]
pub async fn create_world(state: State<'_, AppState>, settings: WorldSettings) -> CmdResult<()> {
    let e = engine(&state)?;
    let r = e.remote().await?;
    if Versions::load(&*r).await?.is_none() {
        Versions::builtin().save(&*r).await?;
    }
    e.save_world_settings(settings).await?;
    Ok(())
}

#[tauri::command]
pub async fn tailscale_status() -> CmdResult<tailscale::Status> {
    Ok(tailscale::status().await)
}

pub async fn install_tailscale_inner(e: &Arc<Engine>) -> CmdResult<()> {
    if tailscale::cli_path().is_some() {
        return Ok(());
    }
    let dest = e.paths.downloads().join("tailscale-setup.exe");
    tailscale::download_installer(&dest).await?;
    // O instalador oficial pede permissão de administrador (UAC).
    let ps = format!(
        "$p = Start-Process -FilePath '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        dest.display().to_string().replace('\'', "''")
    );
    let out = util::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .await
        .map_err(|err| CmdError::msg("Não consegui abrir o instalador", err.to_string()))?;
    let _ = std::fs::remove_file(&dest);
    for _ in 0..30 {
        if tailscale::cli_path().is_some() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Err(CmdError::msg(
        "O Tailscale não foi instalado",
        format!("O instalador terminou sem instalar (código {:?}). Tente de novo e aceite a permissão.", out.status.code()),
    ))
}

#[tauri::command]
pub async fn tailscale_install(state: State<'_, AppState>) -> CmdResult<()> {
    install_tailscale_inner(&engine(&state)?).await
}

/// Entra na rede do mundo com a chave (do convite ou digitada pelo dono).
#[tauri::command]
pub async fn tailscale_up(state: State<'_, AppState>, auth_key: Option<String>) -> CmdResult<tailscale::Status> {
    let e = engine(&state)?;
    let key = match auth_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(k) => {
            let k2 = k.clone();
            e.update_secrets(|s| s.tailscale_auth_key = Some(k2)).await?;
            k
        }
        None => e
            .secrets()
            .await
            .tailscale_auth_key
            .clone()
            .ok_or_else(|| CmdError::msg("Falta a chave do Tailscale", "Cole a auth key."))?,
    };
    if !key.starts_with("tskey-") {
        return Err(CmdError::msg("Chave inválida", "A auth key do Tailscale começa com tskey-auth-."));
    }
    let name = e.config.read().await.player_name.clone();
    tailscale::up_with_key(&key, &name).await?;
    for _ in 0..15 {
        let st = tailscale::status().await;
        if st.running() {
            return Ok(st);
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Ok(tailscale::status().await)
}

#[derive(Deserialize)]
pub struct NetworkArgs {
    pub duckdns_domain: Option<String>,
    pub duckdns_token: Option<String>,
    pub discord_webhook: Option<String>,
    pub tailscale_auth_key: Option<String>,
    pub tailscale_key_expires: Option<chrono::NaiveDate>,
}

fn clean(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[tauri::command]
pub async fn save_network(state: State<'_, AppState>, args: NetworkArgs) -> CmdResult<()> {
    let e = engine(&state)?;
    let domain = clean(args.duckdns_domain);
    if let Some(d) = &domain {
        if !pauletor3_core::duckdns::is_valid_subdomain(d) {
            return Err(CmdError::msg("Nome do DuckDNS inválido", "Use só o nome, por exemplo: pauletor3"));
        }
    }
    let webhook = clean(args.discord_webhook);
    if let Some(w) = &webhook {
        if !pauletor3_core::journal::is_valid_webhook(w) {
            return Err(CmdError::msg("Webhook inválido", "A URL começa com https://discord.com/api/webhooks/"));
        }
    }
    let token = clean(args.duckdns_token);
    let key = clean(args.tailscale_auth_key);
    e.update_secrets(|s| {
        if token.is_some() {
            s.duckdns_token = token.clone();
        }
        if webhook.is_some() {
            s.discord_webhook = webhook.clone();
        }
        if key.is_some() {
            s.tailscale_auth_key = key.clone();
        }
    })
    .await?;
    e.update_config(|c| {
        if let Some(d) = &domain {
            c.duckdns_domain = Some(pauletor3_core::duckdns::subdomain(d));
        }
        if args.tailscale_key_expires.is_some() {
            c.tailscale_key_expires = args.tailscale_key_expires;
        }
    })
    .await?;
    Ok(())
}

#[derive(Serialize)]
pub struct DuckTest {
    pub address: String,
    pub resolved: Vec<String>,
    pub expected: String,
}

/// Atualiza o DuckDNS com o IP do Tailscale deste PC e confere a resolução.
#[tauri::command]
pub async fn test_duckdns(state: State<'_, AppState>) -> CmdResult<DuckTest> {
    let e = engine(&state)?;
    let domain = e.config.read().await.duckdns_domain.clone().ok_or_else(|| CmdError::msg("Sem domínio", "Informe o nome no DuckDNS."))?;
    let token = e.secrets().await.duckdns_token.clone().ok_or_else(|| CmdError::msg("Sem token", "Cole o token do DuckDNS."))?;
    let ip = tailscale::my_ip().await?;
    pauletor3_core::duckdns::update(&domain, &token, &ip).await?;
    let mut resolved = vec![];
    for _ in 0..10 {
        resolved = pauletor3_core::duckdns::resolve(&domain).await.unwrap_or_default().iter().map(|i| i.to_string()).collect();
        if resolved.contains(&ip) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    Ok(DuckTest { address: pauletor3_core::duckdns::fqdn(&domain), resolved, expected: ip })
}

#[tauri::command]
pub async fn install_firewall() -> CmdResult<()> {
    firewall::install_rules().await?;
    Ok(())
}

/// Remove as regras criadas pelo app (antes de desinstalar).
#[tauri::command]
pub async fn remove_firewall() -> CmdResult<()> {
    firewall::run_elevated(&firewall::remove_commands()).await?;
    Ok(())
}

#[tauri::command]
pub async fn ensure_serversdat(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    crate::commands::run_fix(app, state, "fix_serversdat".into()).await
}

pub async fn prepare_runtime_inner(e: &Arc<Engine>) -> CmdResult<()> {
    let r = e.remote().await?;
    let v = Versions::load(&*r).await?.unwrap_or_else(Versions::builtin);
    let progress = {
        let e = e.clone();
        let _ = e;
        quiet()
    };
    java::ensure_java(&e.paths, &v, progress.clone()).await?;
    server::prepare(&e.paths, &v, progress).await?;
    server::verify_install(&e.paths, &v)?;
    Ok(())
}

#[tauri::command]
pub async fn prepare_runtime(state: State<'_, AppState>) -> CmdResult<()> {
    prepare_runtime_inner(&engine(&state)?).await
}

#[tauri::command]
pub async fn create_invite(state: State<'_, AppState>, password: String) -> CmdResult<String> {
    let e = engine(&state)?;
    let c = e.config.read().await.clone();
    if !c.is_owner {
        return Err(CmdError::msg("Só o dono gera convites", "Peça um convite ao dono do mundo."));
    }
    let s = e.secrets().await;
    let backend = s.remote_backend.clone().ok_or_else(|| CmdError::msg("Nuvem não conectada", "Conecte o Google Drive primeiro."))?;
    let params: Vec<(String, String)> = s
        .remote_params_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .ok_or_else(|| CmdError::msg("Nuvem não conectada", "Conecte o Google Drive primeiro."))?;
    let payload = InvitePayload {
        world_name: e.status().world_name,
        remote_backend: backend,
        remote_params: params,
        remote_root: c.remote_root.clone(),
        tailscale_auth_key: s.tailscale_auth_key.clone(),
        tailscale_key_expires: c.tailscale_key_expires,
        duckdns_domain: c.duckdns_domain.clone(),
        duckdns_token: s.duckdns_token.clone(),
        discord_webhook: s.discord_webhook.clone(),
        created_by: c.player_name.clone(),
        created_at: chrono::Utc::now(),
    };
    invite::seal(&payload, &password).map_err(|err| CmdError::msg("Não consegui gerar o convite", err.to_string()))
}

#[derive(Serialize)]
pub struct InviteAccepted {
    pub world_name: String,
    pub created_by: String,
    pub has_tailscale_key: bool,
}

#[tauri::command]
pub async fn accept_invite(state: State<'_, AppState>, code: String, password: String) -> CmdResult<InviteAccepted> {
    let e = engine(&state)?;
    let p = invite::open(&code, &password).map_err(|err| CmdError::msg("Convite não abriu", err.to_string()))?;
    // A pasta do mundo no remote precisa estar definida antes do teste de conexão.
    let root = p.remote_root.clone();
    e.update_config(|c| c.remote_root = root).await?;
    write_remote(&e, &p.remote_backend, p.remote_params.clone()).await?;
    let (ts, dd, wh) = (p.tailscale_auth_key.clone(), p.duckdns_token.clone(), p.discord_webhook.clone());
    e.update_secrets(|s| {
        s.tailscale_auth_key = ts;
        s.duckdns_token = dd;
        s.discord_webhook = wh;
    })
    .await?;
    let (root, domain, exp) = (p.remote_root.clone(), p.duckdns_domain.clone(), p.tailscale_key_expires);
    e.update_config(|c| {
        c.remote_root = root;
        c.duckdns_domain = domain;
        c.tailscale_key_expires = exp;
        c.is_owner = false;
    })
    .await?;
    Ok(InviteAccepted { world_name: p.world_name.clone(), created_by: p.created_by.clone(), has_tailscale_key: p.tailscale_auth_key.is_some() })
}

#[tauri::command]
pub async fn finish_setup(state: State<'_, AppState>) -> CmdResult<()> {
    let e = engine(&state)?;
    e.update_config(|c| c.setup_complete = true).await?;
    let _ = e.refresh().await;
    Ok(())
}
