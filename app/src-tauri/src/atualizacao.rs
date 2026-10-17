//! Serviço de atualização do app: procura ao abrir e a cada 1 h, baixa em segundo plano assim
//! que encontra, e só instala no momento seguro (a decisão mora em
//! `machenia_core::atualizacao::Maquina`; aqui ficam o plugin updater, os eventos e a ponte
//! com a interface).
//!
//! Fonte das atualizações:
//! - build pessoal: o Worker do dono (`/u/:mundo/latest.json`, só membros, cabeçalhos assinados);
//! - build padrão: o GitHub Releases (ou `endpoint_atualizacao` do config).
//!   Sem endereço, não procura (falha em silêncio).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;

use machenia_core::atualizacao::{abaixo_do_minimo, Efeito, Estado, Maquina, Oferta, Pedido};
use machenia_core::events::{Event, NoticeLevel};
use machenia_core::session::EndReason;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::AppState;

/// Intervalo entre verificações com o app aberto.
const INTERVALO: Duration = Duration::from_secs(60 * 60);
/// Prazo total para passar o host (ou fechar o mundo com a nuvem conferida) antes de instalar.
const PRAZO_TROCA: Duration = Duration::from_secs(90);
/// Build padrão: releases públicas do GitHub (hoje em rascunho; nada é encontrado até publicar).
#[cfg(not(feature = "personal"))]
const ENDPOINT_PADRAO: &str = "https://github.com/pauletor3world/pauletor3/releases/latest/download/latest.json";

pub struct Servico {
    maq: Mutex<Maquina>,
    update: Mutex<Option<Update>>,
    bytes: Mutex<Option<Vec<u8>>>,
    progresso: Mutex<Option<Channel<Progresso>>>,
    /// "Atualizado para a versão X ✓" (mostrado uma vez, depois de reabrir).
    acabou_de_atualizar: Mutex<Option<String>>,
}

impl Default for Servico {
    fn default() -> Self {
        Self {
            maq: Mutex::new(Maquina::nova(machenia_core::APP_VERSION)),
            update: Mutex::new(None),
            bytes: Mutex::new(None),
            progresso: Mutex::new(None),
            acabou_de_atualizar: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Progresso {
    pub baixado: u64,
    pub total: Option<u64>,
}

/// O que a interface mostra.
#[derive(Debug, Clone, Serialize)]
pub struct Visao {
    #[serde(flatten)]
    pub maq: Maquina,
    pub obrigatoria: bool,
    pub sou_host: bool,
    /// Abaixo da versão mínima do grupo (Jogar e hospedar bloqueados).
    pub bloqueado: bool,
    pub versao_minima: Option<String>,
    pub acabou_de_atualizar: Option<String>,
}

fn servico(app: &AppHandle) -> &Servico {
    &app.state::<AppState>().inner().atualizacao
}

fn visao(app: &AppHandle) -> Visao {
    let s = servico(app);
    let maq = s.maq.lock().unwrap().clone();
    let st = app.state::<AppState>().engine().map(|e| e.status());
    let bloqueado = st.as_ref().map(|s| s.desatualizado).unwrap_or(false);
    Visao {
        obrigatoria: maq.obrigatoria() || bloqueado,
        sou_host: st.as_ref().map(|s| s.me_hosting).unwrap_or(false),
        bloqueado,
        versao_minima: st.and_then(|s| s.versao_minima),
        acabou_de_atualizar: s.acabou_de_atualizar.lock().unwrap().clone(),
        maq,
    }
}

fn emitir(app: &AppHandle) {
    let _ = app.emit("p3://atualizacao", visao(app));
}

// ------------------------------------------------------------------ início

/// Marcador gravado antes de instalar; ao reabrir na versão nova, vira o aviso "Atualizado ✓".
fn marcador(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("atualizacao-pendente.txt"))
}

pub fn iniciar(app: AppHandle) {
    if let Some(p) = marcador(&app) {
        if let Ok(v) = std::fs::read_to_string(&p) {
            let v = v.trim().to_string();
            if v == machenia_core::APP_VERSION {
                tracing::info!(versao = %v, "app atualizado");
                *servico(&app).acabou_de_atualizar.lock().unwrap() = Some(v);
            } else {
                tracing::warn!(esperada = %v, atual = machenia_core::APP_VERSION, "a atualização não chegou a ser instalada");
            }
            let _ = std::fs::remove_file(&p);
        }
    }
    tauri::async_runtime::spawn(async move {
        // deixa o app abrir (rede, réplica) antes da primeira verificação
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            verificar(&app).await;
            tokio::time::sleep(INTERVALO).await;
        }
    });
}

// ------------------------------------------------------------------ verificação

async fn endereco(app: &AppHandle) -> Result<Option<(tauri::Url, Vec<(&'static str, String)>)>, String> {
    let Some(engine) = app.state::<AppState>().engine() else { return Ok(None) };
    if !engine.config.read().await.ready() {
        return Ok(None);
    }
    #[cfg(feature = "personal")]
    let (url, headers) = match engine.updater_endpoint().await {
        Ok(x) => x,
        Err(_) => return Ok(None), // ainda sem mundo: nada a procurar
    };
    #[cfg(not(feature = "personal"))]
    let (url, headers) = match engine.config.read().await.endpoint_atualizacao.clone().filter(|e| !e.trim().is_empty()) {
        Some(e) => (e, Vec::new()),
        None => (ENDPOINT_PADRAO.to_string(), Vec::new()),
    };
    let url = url.parse().map_err(|e| format!("endereço de atualização inválido: {e}"))?;
    Ok(Some((url, headers)))
}

async fn buscar(app: &AppHandle) -> Result<Option<(Update, Oferta)>, String> {
    let Some((url, headers)) = endereco(app).await? else { return Ok(None) };
    let mut b = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?.timeout(Duration::from_secs(60));
    for (k, v) in headers {
        b = b.header(k, v).map_err(|e| e.to_string())?;
    }
    let hook = app.clone();
    b = b.on_before_exit(move || antes_de_sair(&hook));
    let found = b.build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let desatualizado = app.state::<AppState>().engine().map(|e| e.status().desatualizado).unwrap_or(false);
    Ok(found.map(|u| {
        let minima = u.raw_json.get("versao_minima").and_then(|v| v.as_str()).unwrap_or("");
        let obrigatoria = u.raw_json.get("obrigatoria").and_then(|v| v.as_bool()).unwrap_or(false)
            || desatualizado
            || abaixo_do_minimo(machenia_core::APP_VERSION, minima);
        let o = Oferta { versao: u.version.clone(), notas: u.body.clone().filter(|n| !n.trim().is_empty()), obrigatoria };
        (u, o)
    }))
}

/// Procura atualização. Falha (sem internet, servidor fora) só vai para o log.
pub async fn verificar(app: &AppHandle) {
    let r = buscar(app).await;
    let efeito = {
        let s = servico(app);
        let mut maq = s.maq.lock().unwrap();
        match r {
            Err(e) => {
                tracing::info!(error = %e, "não consegui procurar atualização; tento de novo depois");
                maq.verificou(Err(e))
            }
            Ok(None) => {
                let e = maq.verificou(Ok(None));
                if maq.estado == Estado::SemAtualizacao {
                    *s.update.lock().unwrap() = None;
                    *s.bytes.lock().unwrap() = None;
                }
                e
            }
            Ok(Some((u, o))) => {
                let nova = maq.oferta.as_ref().map(|a| a.versao != o.versao).unwrap_or(true);
                let e = maq.verificou(Ok(Some(o)));
                if nova && e == Efeito::Baixar {
                    tracing::info!(versao = %u.version, "atualização encontrada");
                    *s.update.lock().unwrap() = Some(u);
                    *s.bytes.lock().unwrap() = None;
                }
                e
            }
        }
    };
    emitir(app);
    executar(app, efeito).await;
}

// ------------------------------------------------------------------ efeitos

/// Executa um efeito e os que vierem em seguida (baixar → instalar, passar host → instalar).
async fn executar(app: &AppHandle, mut efeito: Efeito) {
    loop {
        efeito = match efeito {
            Efeito::Nada | Efeito::EsperarMomento => break,
            Efeito::Baixar => baixar(app).await,
            Efeito::PassarHost => passar_host(app).await,
            Efeito::Instalar => instalar(app).await,
            Efeito::Avisar { titulo, corpo } => {
                avisar(app, &titulo, &corpo);
                Efeito::Nada
            }
        };
        emitir(app);
    }
    emitir(app);
}

fn disparar(app: &AppHandle, efeito: Efeito) {
    if efeito == Efeito::Nada {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { executar(&app, efeito).await });
}

async fn momento(app: &AppHandle) -> machenia_core::atualizacao::Momento {
    match app.state::<AppState>().engine() {
        Some(e) => e.momento().await,
        None => Default::default(),
    }
}

async fn baixar(app: &AppHandle) -> Efeito {
    let Some(u) = servico(app).update.lock().unwrap().clone() else {
        let m = momento(app).await;
        return servico(app).maq.lock().unwrap().baixou(Err("nada para baixar".into()), m);
    };
    let mut baixado: u64 = 0;
    let mut ultimo = std::time::Instant::now() - Duration::from_secs(1);
    let app2 = app.clone();
    let r = u
        .download(
            |n, total| {
                baixado += n as u64;
                servico(&app2).maq.lock().unwrap().progresso(baixado, total);
                if ultimo.elapsed() >= Duration::from_millis(200) || total == Some(baixado) {
                    ultimo = std::time::Instant::now();
                    if let Some(c) = servico(&app2).progresso.lock().unwrap().as_ref() {
                        let _ = c.send(Progresso { baixado, total });
                    }
                }
            },
            || {},
        )
        .await;
    let m = momento(app).await;
    let s = servico(app);
    match r {
        Ok(bytes) => {
            tracing::info!(versao = %u.version, tamanho = bytes.len(), "atualização baixada e conferida");
            *s.bytes.lock().unwrap() = Some(bytes);
            s.maq.lock().unwrap().baixou(Ok(()), m)
        }
        Err(e) => {
            // inclui assinatura errada: o plugin recusa o arquivo e nada é instalado
            tracing::warn!(error = %e, "download da atualização falhou");
            s.maq.lock().unwrap().baixou(Err(e.to_string()), m)
        }
    }
}

async fn passar_host(app: &AppHandle) -> Efeito {
    let r = match app.state::<AppState>().engine() {
        Some(e) => {
            tracing::info!("passando o host (ou fechando o mundo) para atualizar");
            e.encerrar_para_atualizar(PRAZO_TROCA).await.map_err(|e| e.to_string())
        }
        None => Ok(machenia_core::atualizacao::FimSeguro::NadaAberto),
    };
    match &r {
        Ok(fim) => tracing::info!(?fim, "mundo em estado seguro para atualizar"),
        Err(e) => tracing::warn!(error = %e, "atualização cancelada: o estado final do mundo não se confirmou"),
    }
    let m = momento(app).await;
    servico(app).maq.lock().unwrap().host_passado(r, m)
}

async fn instalar(app: &AppHandle) -> Efeito {
    let s = servico(app);
    // última conferência, já com o mundo parado
    let m = momento(app).await;
    if !m.seguro() {
        tracing::warn!(?m, "o mundo deixou de estar seguro; a instalação espera");
        s.maq.lock().unwrap().voltar_a_esperar();
        return Efeito::EsperarMomento;
    }
    let (Some(u), Some(bytes)) = (s.update.lock().unwrap().clone(), s.bytes.lock().unwrap().clone()) else {
        return s.maq.lock().unwrap().instalacao_falhou();
    };
    if let Some(p) = marcador(app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(&p, &u.version);
    }
    tracing::info!(versao = %u.version, "instalando a atualização");
    let st = app.state::<AppState>();
    st.quitting.store(true, Ordering::SeqCst);
    // no Windows, `install` abre o instalador e encerra o processo (antes roda `antes_de_sair`)
    let r = tauri::async_runtime::spawn_blocking(move || u.install(bytes)).await;
    match r {
        Ok(Ok(())) => {
            app.restart();
        }
        Ok(Err(e)) => tracing::error!(error = %e, "instalação falhou"),
        Err(e) => tracing::error!(error = %e, "instalação interrompida"),
    }
    st.quitting.store(false, Ordering::SeqCst);
    if let Some(p) = marcador(app) {
        let _ = std::fs::remove_file(p);
    }
    s.maq.lock().unwrap().instalacao_falhou()
}

/// Gancho do plugin logo antes de o processo sair para instalar: última garantia de que o
/// servidor local está parado e o mundo salvo. Roda numa thread de bloqueio (fora do async).
fn antes_de_sair(app: &AppHandle) {
    if let Some(engine) = app.state::<AppState>().engine() {
        let hosting = tauri::async_runtime::block_on(engine.is_hosting());
        if hosting {
            tracing::error!("instalação pedida com a sessão aberta; salvando e fechando antes");
            let _ = tauri::async_runtime::block_on(engine.stop(EndReason::UserStop));
        }
    }
    tracing::info!("saindo para instalar a atualização");
}

fn avisar(app: &AppHandle, titulo: &str, corpo: &str) {
    let _ = app.emit("p3://notice", &Event::Notice { level: NoticeLevel::Warning, title: titulo.into(), body: corpo.into() });
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if !focused {
        let _ = app.notification().builder().title(titulo).body(corpo).show();
    }
}

// ------------------------------------------------------------------ ganchos do app

/// Mudou o status do mundo: confere se a espera acabou e a versão mínima.
pub fn status_mudou(app: &AppHandle) {
    let s = servico(app);
    let bloqueado = app.state::<AppState>().engine().map(|e| e.status().desatualizado).unwrap_or(false);
    let (esperando, mudou_obrigatoria) = {
        let mut maq = s.maq.lock().unwrap();
        let antes = maq.obrigatoria();
        if bloqueado && !antes {
            maq.definir_obrigatoria(true);
        }
        (maq.estado == Estado::AguardandoMomentoSeguro && !maq.passando_host, maq.obrigatoria() != antes)
    };
    if mudou_obrigatoria {
        emitir(app);
    }
    if esperando {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let m = momento(&app).await;
            let e = servico(&app).maq.lock().unwrap().momento_mudou(m);
            if e != Efeito::Nada {
                executar(&app, e).await;
            }
        });
    }
}

/// O app está fechando. Com "Atualizar ao fechar" e a atualização pronta, faz o fluxo seguro e
/// instala (o processo termina lá). Devolve se ainda é preciso seguir com o fechamento normal.
pub async fn ao_fechar(app: &AppHandle) {
    let m = momento(app).await;
    let e = servico(app).maq.lock().unwrap().fechando(m);
    if e != Efeito::Nada {
        tracing::info!("atualizando ao fechar");
        executar(app, e).await;
    }
}

// ------------------------------------------------------------------ comandos

#[tauri::command]
pub fn atualizacao_estado(app: AppHandle) -> Visao {
    visao(&app)
}

/// A interface assina o progresso do download (canal do Tauri).
#[tauri::command]
pub fn atualizacao_progresso(app: AppHandle, canal: Channel<Progresso>) {
    let (baixado, total) = {
        let m = servico(&app).maq.lock().unwrap();
        (m.baixado, m.total)
    };
    let _ = canal.send(Progresso { baixado, total });
    *servico(&app).progresso.lock().unwrap() = Some(canal);
}

#[tauri::command]
pub async fn atualizacao_atualizar(app: AppHandle, modo: Pedido) -> Visao {
    let m = momento(&app).await;
    let e = servico(&app).maq.lock().unwrap().pedir(modo, m);
    emitir(&app);
    disparar(&app, e);
    visao(&app)
}

#[tauri::command]
pub fn atualizacao_cancelar(app: AppHandle) -> Visao {
    servico(&app).maq.lock().unwrap().cancelar();
    emitir(&app);
    visao(&app)
}

#[tauri::command]
pub async fn atualizacao_verificar(app: AppHandle) -> Visao {
    verificar(&app).await;
    visao(&app)
}

#[tauri::command]
pub fn atualizacao_visto(app: AppHandle) {
    *servico(&app).acabou_de_atualizar.lock().unwrap() = None;
    emitir(&app);
}
