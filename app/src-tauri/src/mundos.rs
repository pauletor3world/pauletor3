//! Vários mundos no mesmo PC. Cada mundo tem a sua pasta de dados (config, chave, cache e
//! servidor); o Java e o Minecraft ficam na pasta do primeiro. Um mundo fica aberto por vez:
//! o `localhost` do Minecraft aponta para ele, e trocar fecha a rede de um e abre a do outro.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use machenia_core::config::Config;
use machenia_core::events::{Event, EventSink};
use machenia_core::invite::Invite;
use machenia_core::paths::{self, Paths};
use machenia_core::secrets::Secrets;
use machenia_core::session::{Engine, Phase};
use machenia_core::server;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{engine, CmdError, CmdResult};
use crate::AppState;

#[derive(Serialize)]
pub struct MundoView {
    pub dir: String,
    pub name: String,
    pub active: bool,
    pub owner: bool,
    /// Assistente concluído e ligado a um mundo (os outros ficam de fora da lista).
    pub ready: bool,
    pub player_name: String,
    /// O primeiro mundo deste PC (guarda o Java e o Minecraft dos outros; não sai da lista).
    pub first: bool,
}

fn worlds() -> CmdResult<paths::Worlds> {
    paths::worlds().ok_or_else(|| CmdError::msg("Falta terminar a configuração", "Abra o assistente."))
}

/// Os mundos deste PC, o aberto primeiro.
#[tauri::command]
pub async fn worlds_list(state: State<'_, AppState>) -> CmdResult<Vec<MundoView>> {
    let w = worlds()?;
    let current = state.engine();
    let mut out = vec![];
    for dir in &w.all {
        let active = current.as_ref().is_some_and(|e| &e.paths.root == dir);
        let c = match (&current, active) {
            (Some(e), true) => e.config.read().await.clone(),
            _ => Config::load(&Paths::new(dir)),
        };
        if !active && !c.ready() {
            continue;
        }
        out.push(MundoView {
            dir: dir.display().to_string(),
            name: c.world_name.clone(),
            active,
            owner: c.is_owner,
            ready: c.ready(),
            player_name: c.player_name.clone(),
            first: dir == &w.home,
        });
    }
    out.sort_by_key(|m| !m.active);
    Ok(out)
}

/// Dá para fechar este mundo agora sem perder nada nem derrubar ninguém?
async fn check_can_leave(e: &Engine) -> CmdResult<()> {
    let st = e.status();
    if e.is_hosting().await || st.me_hosting {
        return Err(CmdError::msg("O mundo está aberto neste PC", "Clique em Parar (ou passe a vez) antes de trocar de mundo."));
    }
    if !matches!(st.phase, Phase::Idle | Phase::WaitingTurn) {
        return Err(CmdError::msg("Ainda trabalhando neste mundo", "Espere terminar o que está em andamento e tente de novo."));
    }
    if st.playing {
        return Err(CmdError::msg("Você está jogando neste mundo", "Saia do mundo no Minecraft antes de trocar: a troca derrubaria a sua conexão."));
    }
    if st.replicating || st.sync.is_some() {
        return Err(CmdError::msg("Atualizando a cópia deste mundo", "Espere a cópia terminar (alguns segundos) e tente de novo."));
    }
    if st.client_prep.is_some() || st.client_starting {
        return Err(CmdError::msg("Preparando o Minecraft", "Espere o Minecraft terminar de baixar e tente de novo."));
    }
    Ok(())
}

/// Fecha o mundo aberto (rede, proxy, tarefas de fundo) e abre o de `dir`.
pub async fn switch_to(app: &AppHandle, dir: PathBuf) -> CmdResult<Arc<Engine>> {
    let state = app.state::<AppState>();
    if let Some(old) = state.engine() {
        if old.paths.root == dir {
            return Ok(old);
        }
        check_can_leave(&old).await?;
    }
    paths::add_world(&dir, true)?;
    let old = state.engine.write().unwrap().take();
    crate::stop_background(app);
    if let Some(old) = old {
        old.stop_network().await;
        tracing::info!(de = %old.paths.root.display(), para = %dir.display(), "trocando de mundo");
    }
    state.console.lock().unwrap().clear();
    let e = crate::open_engine(app, dir)?;
    let _ = app.emit("p3://world-changed", e.paths.root.display().to_string());
    let _ = app.emit("p3://status", &e.status());
    Ok(e)
}

#[tauri::command]
pub async fn world_switch(app: AppHandle, dir: String) -> CmdResult<()> {
    let dir = PathBuf::from(dir);
    if !worlds()?.all.contains(&dir) {
        return Err(CmdError::msg("Mundo não encontrado", "Esse mundo não está mais na lista deste PC."));
    }
    if !Config::load(&Paths::new(&dir)).ready() {
        return Err(CmdError::msg("Esse mundo não terminou de ser adicionado", "Tire da lista e adicione de novo."));
    }
    switch_to(&app, dir).await?;
    Ok(())
}

/// Tira um mundo da lista deste PC. Os arquivos ficam no disco (e o mundo segue com o grupo).
#[tauri::command]
pub async fn world_remove(dir: String) -> CmdResult<()> {
    paths::remove_world(Path::new(&dir)).map_err(|e| CmdError::msg("Não deu para tirar da lista", e.to_string()))?;
    Ok(())
}

#[derive(Serialize)]
pub struct AddDefaults {
    pub coordinator_url: Option<String>,
    /// Já existe uma chave de administração guardada neste PC para essa coordenação.
    pub has_admin_token: bool,
    pub player_name: String,
}

/// Uma chave de administração guardada em algum mundo deste PC (o dono cria vários).
async fn saved_admin(current: Option<&Arc<Engine>>, url: &str) -> Option<String> {
    if let Some(e) = current {
        let same = e.config.read().await.coordinator_url.as_deref() == Some(url);
        if let (true, Some(t)) = (same, e.secrets().await.admin_token.clone()) {
            return Some(t);
        }
    }
    for dir in paths::worlds()?.all {
        let p = Paths::new(&dir);
        if Config::load(&p).coordinator_url.as_deref() != Some(url) {
            continue;
        }
        if let Some(t) = Secrets::load(&p.secrets()).ok().and_then(|s| s.admin_token.clone()) {
            return Some(t);
        }
    }
    None
}

/// O que já dá para preencher no "Adicionar mundo" (endereço da coordenação, nick).
#[tauri::command]
pub async fn world_add_defaults(state: State<'_, AppState>) -> CmdResult<AddDefaults> {
    let e = state.engine();
    let c = match &e {
        Some(e) => e.config.read().await.clone(),
        None => Config::default(),
    };
    let has = match &c.coordinator_url {
        Some(u) => saved_admin(e.as_ref(), u).await.is_some(),
        None => false,
    };
    Ok(AddDefaults { coordinator_url: c.coordinator_url.clone(), has_admin_token: has, player_name: c.player_name.clone() })
}

/// Monta a pasta do mundo novo com as preferências deste PC e um motor sem rede para criar ou
/// entrar. Só o progresso do envio chega na interface.
async fn new_world_engine(app: &AppHandle) -> CmdResult<(PathBuf, Arc<Engine>)> {
    let state = app.state::<AppState>();
    let current = engine(&state)?;
    check_can_leave(&current).await?;
    let w = worlds()?;
    let dir = paths::new_world_dir()?;
    let p = w.paths(&dir);
    p.ensure()?;
    let cfg = current.config.read().await.for_new_world();
    cfg.save(&p)?;
    if cfg.eula_accepted {
        server::accept_eula(&p)?;
    }
    let app2 = app.clone();
    let sink: EventSink = Arc::new(move |e: Event| {
        if let Event::Sync(s) = e {
            let _ = app2.emit("p3://sync", s);
        }
    });
    let e = Engine::open(p, sink)?;
    Ok((dir, e))
}

/// Deu errado no meio: a pasta nova some (nada nela é do grupo ainda).
fn discard(dir: &Path, e: Arc<Engine>) {
    drop(e);
    if let Err(err) = std::fs::remove_dir_all(dir) {
        tracing::warn!(error = %err, dir = %dir.display(), "pasta do mundo novo ficou para trás");
    }
}

/// Deu certo: o mundo entra na lista, vira o aberto e o servidor dele é preparado em segundo plano.
async fn adopt(app: &AppHandle, dir: PathBuf, temp: Arc<Engine>) -> CmdResult<()> {
    drop(temp);
    paths::add_world(&dir, false)?;
    let e = switch_to(app, dir).await?;
    tauri::async_runtime::spawn(async move {
        if let Err(err) = crate::setup::prepare_runtime_inner(&e).await {
            tracing::warn!(error = ?err.0, "preparo do servidor do mundo novo falhou; tenta de novo no Jogar");
        }
    });
    Ok(())
}

#[derive(Deserialize)]
pub struct AddCreateArgs {
    /// Vazio = a mesma coordenação do mundo aberto.
    pub coordinator_url: Option<String>,
    /// Vazio = a chave já guardada neste PC para essa coordenação.
    pub admin_token: Option<String>,
    pub name: String,
    pub nick: String,
    pub folder: Option<String>,
}

/// O dono cria mais um mundo (novo ou importado de uma pasta) e passa a usar ele.
#[tauri::command]
pub async fn world_add_create(app: AppHandle, args: AddCreateArgs) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let current = engine(&state)?;
    let url = match args.coordinator_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(u) => u.trim_end_matches('/').to_string(),
        None => current.config.read().await.coordinator_url.clone().unwrap_or_default(),
    };
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost")) {
        return Err(CmdError::msg("Endereço inválido", "Cole o endereço que o script de publicação mostrou (começa com https://)."));
    }
    let token = match args.admin_token.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t.to_string(),
        None => saved_admin(Some(&current), &url)
            .await
            .ok_or_else(|| CmdError::msg("Falta a chave de administração", "Cole a chave que o script publicar-coordenacao.ps1 mostrou."))?,
    };
    if args.name.trim().is_empty() {
        return Err(CmdError::msg("Falta o nome do mundo", "Escolha um nome para o grupo reconhecer o mundo."));
    }
    let folder = args.folder.filter(|f| !f.trim().is_empty()).map(PathBuf::from);
    if let Some(f) = &folder {
        if !f.join("level.dat").exists() {
            return Err(CmdError::msg("Essa pasta não é um mundo", "Escolha a pasta que tem o arquivo level.dat dentro."));
        }
    }
    let (dir, e) = new_world_engine(&app).await?;
    if let Err(err) = e.create_world(&url, &token, args.name.trim(), &args.nick, folder).await {
        discard(&dir, e);
        return Err(err.into());
    }
    adopt(&app, dir, e).await
}

/// Entra em mais um mundo pelo convite e passa a usar ele.
#[tauri::command]
pub async fn world_add_join(app: AppHandle, invite: String, nick: String) -> CmdResult<machenia_core::coord::http::JoinResult> {
    let inv = Invite::parse(&invite).map_err(|_| CmdError::msg("Convite inválido", "Copie o link inteiro (com a parte depois do #) ou o código P3J1."))?;
    for dir in worlds()?.all {
        let c = Config::load(&Paths::new(&dir));
        if c.ready() && c.world_id.as_deref() == Some(inv.world_id()) {
            return Err(CmdError::msg("Esse mundo já está neste PC", format!("É o {}: escolha ele na lista de mundos.", c.world_name)));
        }
    }
    let (dir, e) = new_world_engine(&app).await?;
    let r = match e.join_world(&invite, &nick).await {
        Ok(r) => r,
        Err(err) => {
            discard(&dir, e);
            return Err(err.into());
        }
    };
    *app.state::<AppState>().pending_invite.lock().unwrap() = None;
    adopt(&app, dir, e).await?;
    Ok(r)
}
