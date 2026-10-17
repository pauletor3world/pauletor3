//! Minecraft aberto direto pelo app, sem launcher de terceiros.
//!
//! A instalação do cliente sai dos metadados oficiais da Mojang (piston-meta). O JSON da versão
//! é fixado por sha1 em `versions.json`, e ele traz o sha1 de todo o resto (client.jar,
//! bibliotecas, índice de assets, configuração de log): nada roda sem conferir. Arquivos que já
//! existem na `.minecraft` de outro launcher são reaproveitados quando o hash bate, então quem já
//! joga não baixa de novo os ~450 MB de assets.
//!
//! O comando segue as regras do próprio JSON (argumentos por sistema, bibliotecas nativas por
//! arquitetura) e entra no mundo direto pelo Quick Play (`--quickPlayMultiplayer`).
//! Só é usado no build pessoal: o cliente entra em modo offline, com o nick do app.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Deserialize;

use crate::events::{SyncPhase, SyncProgress, SyncProgressFn};
use crate::paths::Paths;
use crate::versions::PinnedFile;
use crate::{NmError, NmResult};

const RESOURCES: &str = "https://resources.download.minecraft.net";
/// Downloads simultâneos (a maioria dos assets tem poucos KB).
const PARALLEL: usize = 16;
/// Marca de instalação completa e conferida (guarda o sha1 do JSON da versão).
const READY_MARK: &str = ".pronto";

// ------------------------------------------------------------------ JSON da versão (Mojang)

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMeta {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub main_class: String,
    pub asset_index: AssetIndexRef,
    pub downloads: Downloads,
    pub libraries: Vec<Library>,
    pub arguments: Arguments,
    #[serde(default)]
    pub logging: Option<Logging>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    pub client: Artifact,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: LibDownloads,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Arg>,
    #[serde(default)]
    pub jvm: Vec<Arg>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    Ruled { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: Artifact,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct AssetIndex {
    objects: BTreeMap<String, AssetObject>,
}

#[derive(Debug, Clone, Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

// ------------------------------------------------------------------ regras

/// Recursos ligados no lançamento (`features` das regras do JSON).
type Features = BTreeMap<&'static str, bool>;

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

fn rule_matches(r: &Rule, features: &Features) -> bool {
    if let Some(os) = &r.os {
        if os.name.as_deref().is_some_and(|n| n != os_name()) {
            return false;
        }
        if os.arch.as_deref().is_some_and(|a| a != std::env::consts::ARCH) {
            return false;
        }
    }
    r.features.iter().all(|(k, v)| features.get(k.as_str()).copied().unwrap_or(false) == *v)
}

/// Mesma semântica do launcher oficial: sem regras, vale; com regras, vale a última que casar.
/// As regras de uma biblioteca/argumento valem neste PC? (usado também pelas versões da `.minecraft`)
pub fn rules_allow(rules: &[Rule], features: &BTreeMap<&'static str, bool>) -> bool {
    allowed(rules, features)
}

fn allowed(rules: &[Rule], features: &Features) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut ok = false;
    for r in rules {
        if rule_matches(r, features) {
            ok = r.action == RuleAction::Allow;
        }
    }
    ok
}

/// O JSON libera as nativas de Windows de todas as arquiteturas (x64, arm64, x86) só pela regra
/// de sistema; baixamos só a deste PC.
fn native_for_this_arch(name: &str) -> bool {
    let Some(classifier) = name.splitn(4, ':').nth(3) else { return true };
    if !classifier.starts_with("natives-windows") {
        return true;
    }
    let want = match std::env::consts::ARCH {
        "aarch64" => "natives-windows-arm64",
        "x86" => "natives-windows-x86",
        _ => "natives-windows",
    };
    classifier == want
}

impl VersionMeta {
    /// Bibliotecas que este PC usa, na ordem do JSON (sem repetir caminho).
    pub fn libraries_here(&self) -> Vec<&Artifact> {
        let none = Features::new();
        let mut seen = BTreeSet::new();
        self.libraries
            .iter()
            .filter(|l| allowed(&l.rules, &none) && native_for_this_arch(&l.name))
            .filter_map(|l| l.downloads.artifact.as_ref())
            .filter(|a| a.path.as_ref().is_some_and(|p| seen.insert(p.clone())))
            .collect()
    }
}

// ------------------------------------------------------------------ pastas

/// Pastas do cliente sob `<dados>/client` (tudo do app; a `.minecraft` só é lida).
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn new(paths: &Paths) -> Self {
        Self { root: paths.client() }
    }
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.root.join("versions").join(id)
    }
    pub fn natives(&self, id: &str) -> PathBuf {
        self.root.join("natives").join(id)
    }
    /// Pasta do jogo (`--gameDir`): opções, capturas de tela, pacotes de recursos.
    pub fn game(&self) -> PathBuf {
        self.root.join("game")
    }
}

fn version_id(pinned: &PinnedFile) -> NmResult<String> {
    let id = pinned.filename.strip_suffix(".json").unwrap_or(&pinned.filename);
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return Err(NmError::VersionMismatch(format!("versão do cliente inválida: {}", pinned.filename)));
    }
    Ok(id.to_string())
}

fn is_sha1(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ------------------------------------------------------------------ downloads

#[derive(Debug, Clone)]
struct Job {
    url: String,
    dest: PathBuf,
    sha1: String,
    size: u64,
    /// Mesmo arquivo na `.minecraft` de outro launcher (reaproveitado se o hash bater).
    reuse: Option<PathBuf>,
    /// Endereçado por conteúdo (assets): pode ser link físico em vez de cópia.
    immutable: bool,
}

impl Job {
    /// `quick`: só tamanho (instalação já conferida antes); senão, tamanho e sha1.
    fn ok_at(&self, p: &Path, quick: bool) -> bool {
        match std::fs::metadata(p) {
            Ok(m) if m.is_file() && m.len() == self.size => quick || crate::hashing::sha1_file(p).is_ok_and(|h| h.eq_ignore_ascii_case(&self.sha1)),
            _ => false,
        }
    }

    /// Traz da `.minecraft` se lá existir conferido.
    fn try_reuse(&self) -> bool {
        let Some(src) = &self.reuse else { return false };
        if !self.ok_at(src, false) {
            return false;
        }
        if let Some(d) = self.dest.parent() {
            if std::fs::create_dir_all(d).is_err() {
                return false;
            }
        }
        let _ = std::fs::remove_file(&self.dest);
        (self.immutable && std::fs::hard_link(src, &self.dest).is_ok()) || std::fs::copy(src, &self.dest).is_ok()
    }
}

fn http() -> NmResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("MachenCraft/", env!("CARGO_PKG_VERSION"), " (github.com/pauletor3world)"))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| NmError::Other(e.into()))
}

/// Progresso agregado de muitos downloads, mandado no máximo a cada 150 ms.
struct Meter {
    done: AtomicU64,
    files: AtomicU64,
    total: u64,
    files_total: u64,
    last: Mutex<Instant>,
    progress: SyncProgressFn,
}

impl Meter {
    fn report(&self, force: bool) {
        {
            let mut last = self.last.lock().unwrap();
            if !force && last.elapsed() < Duration::from_millis(150) {
                return;
            }
            *last = Instant::now();
        }
        (self.progress)(SyncProgress {
            phase: SyncPhase::Downloading,
            bytes_done: self.done.load(Ordering::Relaxed).min(self.total),
            bytes_total: self.total,
            files_done: self.files.load(Ordering::Relaxed),
            files_total: self.files_total,
        });
    }
}

async fn fetch_once(http: &reqwest::Client, job: &Job, meter: &Meter, got: &mut u64) -> NmResult<()> {
    use sha1::{Digest, Sha1};
    use tokio::io::AsyncWriteExt;
    let name = job.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let fail = |e: &dyn std::fmt::Display| NmError::Remote(format!("baixando {name}: {e}"));
    if let Some(d) = job.dest.parent() {
        tokio::fs::create_dir_all(d).await?;
    }
    let tmp = job.dest.with_file_name(format!("{name}.baixando"));
    let resp = http.get(&job.url).send().await.and_then(|r| r.error_for_status()).map_err(|e| fail(&e))?;
    let mut f = tokio::fs::File::create(&tmp).await?;
    let mut h = Sha1::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| fail(&e))?;
        h.update(&chunk);
        f.write_all(&chunk).await?;
        *got += chunk.len() as u64;
        meter.done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        meter.report(false);
    }
    f.flush().await?;
    drop(f);
    let actual = hex::encode(h.finalize());
    if *got != job.size || !actual.eq_ignore_ascii_case(&job.sha1) {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(NmError::VersionMismatch(format!("{name}: o arquivo baixado não confere com o hash da Mojang")));
    }
    crate::util::replace_file(&tmp, &job.dest)?;
    Ok(())
}

/// Baixa com até 4 tentativas (a rede de casa cai; o CDN da Mojang às vezes recusa uma conexão).
async fn fetch(http: &reqwest::Client, job: &Job, meter: &Meter) -> NmResult<()> {
    let mut attempt = 0;
    loop {
        let mut got = 0;
        match fetch_once(http, job, meter, &mut got).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                meter.done.fetch_sub(got, Ordering::Relaxed);
                attempt += 1;
                if attempt >= 4 || matches!(e, NmError::VersionMismatch(_)) && attempt >= 2 {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(500 << attempt)).await;
            }
        }
    }
}

/// Garante cada arquivo: confere o que já existe, reaproveita da `.minecraft`, baixa o resto.
async fn run_jobs(http: &reqwest::Client, jobs: Vec<Job>, quick: bool, progress: SyncProgressFn) -> NmResult<()> {
    let missing: Vec<Job> = tokio::task::spawn_blocking(move || {
        jobs.into_iter().filter(|j| !j.ok_at(&j.dest, quick)).filter(|j| !j.try_reuse()).collect::<Vec<_>>()
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    if missing.is_empty() {
        return Ok(());
    }
    tracing::info!(files = missing.len(), "baixando arquivos do Minecraft");
    let meter = Arc::new(Meter {
        done: AtomicU64::new(0),
        files: AtomicU64::new(0),
        total: missing.iter().map(|j| j.size).sum(),
        files_total: missing.len() as u64,
        last: Mutex::new(Instant::now() - Duration::from_secs(1)),
        progress,
    });
    meter.report(true);
    let results: Vec<NmResult<()>> = futures_util::stream::iter(missing)
        .map(|job| {
            let meter = meter.clone();
            async move {
                let r = fetch(http, &job, &meter).await;
                meter.files.fetch_add(1, Ordering::Relaxed);
                meter.report(false);
                r
            }
        })
        .buffer_unordered(PARALLEL)
        .collect()
        .await;
    meter.report(true);
    results.into_iter().collect()
}

// ------------------------------------------------------------------ instalação

/// Cliente instalado e conferido, pronto para montar o comando.
#[derive(Debug, Clone)]
pub struct Installed {
    pub layout: Layout,
    pub meta: VersionMeta,
}

fn read_meta(path: &Path) -> NmResult<VersionMeta> {
    serde_json::from_slice(&std::fs::read(path)?).map_err(|e| NmError::VersionMismatch(format!("JSON da versão ilegível: {e}")))
}

fn jobs_for(meta: &VersionMeta, index: &AssetIndex, layout: &Layout, mc: Option<&Path>) -> NmResult<Vec<Job>> {
    let reuse = |rel: &Path| mc.map(|m| m.join(rel));
    let mut jobs = vec![];
    let c = &meta.downloads.client;
    let jar_rel = Path::new("versions").join(&meta.id).join(format!("{}.jar", meta.id));
    jobs.push(Job {
        url: c.url.clone(),
        dest: layout.version_dir(&meta.id).join(format!("{}.jar", meta.id)),
        sha1: c.sha1.clone(),
        size: c.size,
        reuse: reuse(&jar_rel),
        immutable: false,
    });
    for a in meta.libraries_here() {
        let rel = a.path.as_deref().unwrap_or_default();
        jobs.push(Job {
            url: a.url.clone(),
            dest: crate::util::join_rel(&layout.libraries(), rel).map_err(|e| NmError::VersionMismatch(e.to_string()))?,
            sha1: a.sha1.clone(),
            size: a.size,
            reuse: mc.and_then(|m| crate::util::join_rel(&m.join("libraries"), rel).ok()),
            immutable: false,
        });
    }
    if let Some(l) = meta.logging.as_ref().and_then(|l| l.client.as_ref()) {
        let id = log_config_name(l)?;
        jobs.push(Job {
            url: l.file.url.clone(),
            dest: layout.assets().join("log_configs").join(&id),
            sha1: l.file.sha1.clone(),
            size: l.file.size,
            reuse: reuse(&Path::new("assets").join("log_configs").join(&id)),
            immutable: false,
        });
    }
    let mut seen = BTreeSet::new();
    for o in index.objects.values() {
        let h = o.hash.to_ascii_lowercase();
        if !is_sha1(&h) {
            return Err(NmError::VersionMismatch(format!("hash de asset inválido: {}", o.hash)));
        }
        if !seen.insert(h.clone()) {
            continue;
        }
        let rel = Path::new("objects").join(&h[..2]).join(&h);
        jobs.push(Job {
            url: format!("{RESOURCES}/{}/{h}", &h[..2]),
            dest: layout.assets().join(&rel),
            sha1: h.clone(),
            size: o.size,
            reuse: reuse(&Path::new("assets").join(&rel)),
            immutable: true,
        });
    }
    Ok(jobs)
}

fn log_config_name(l: &LoggingClient) -> NmResult<String> {
    let id = l.file.id.clone().unwrap_or_else(|| "client.xml".into());
    if id.contains(['/', '\\']) || id.contains("..") {
        return Err(NmError::VersionMismatch(format!("configuração de log inválida: {id}")));
    }
    Ok(id)
}

/// Se a instalação já foi conferida inteira para este JSON (só olha a marca).
pub fn is_ready(paths: &Paths, pinned: &PinnedFile) -> bool {
    let Ok(id) = version_id(pinned) else { return false };
    let mark = Layout::new(paths).version_dir(&id).join(READY_MARK);
    match (std::fs::read_to_string(mark), &pinned.sha1) {
        (Ok(m), Some(s)) => m.trim().eq_ignore_ascii_case(s),
        _ => false,
    }
}

/// Garante o cliente instalado e conferido. Na primeira vez confere tudo pelo sha1 (e baixa o
/// que faltar); depois, só confere o tamanho de cada arquivo, que é instantâneo.
pub async fn ensure(paths: &Paths, pinned: &PinnedFile, minecraft_dir: Option<&Path>, progress: SyncProgressFn) -> NmResult<Installed> {
    if pinned.sha1.is_none() {
        return Err(NmError::VersionMismatch("JSON do cliente sem sha1 fixado".into()));
    }
    let id = version_id(pinned)?;
    let layout = Layout::new(paths);
    let vdir = layout.version_dir(&id);
    let json = vdir.join(format!("{id}.json"));
    crate::java::download_verified(pinned, &json, progress.clone()).await?;
    let meta = read_meta(&json)?;
    if meta.id != id {
        return Err(NmError::VersionMismatch(format!("JSON da versão {} em vez de {id}", meta.id)));
    }
    let quick = is_ready(paths, pinned);
    let http = http()?;

    // índice de assets (o sha1 dele vem do JSON da versão)
    let ai = &meta.asset_index;
    if !is_sha1(&ai.sha1) || ai.id.contains(['/', '\\']) || ai.id.contains("..") {
        return Err(NmError::VersionMismatch("índice de assets inválido".into()));
    }
    let index_rel = Path::new("assets").join("indexes").join(format!("{}.json", ai.id));
    let index_job = Job {
        url: ai.url.clone(),
        dest: layout.root.join(&index_rel),
        sha1: ai.sha1.clone(),
        size: ai.size,
        reuse: minecraft_dir.map(|m| m.join(&index_rel)),
        immutable: false,
    };
    let index_path = index_job.dest.clone();
    run_jobs(&http, vec![index_job], false, progress.clone()).await?;
    let index: AssetIndex =
        serde_json::from_slice(&std::fs::read(&index_path)?).map_err(|e| NmError::VersionMismatch(format!("índice de assets ilegível: {e}")))?;

    let jobs = jobs_for(&meta, &index, &layout, minecraft_dir)?;
    run_jobs(&http, jobs, quick, progress).await?;
    std::fs::create_dir_all(layout.natives(&id))?;
    if !quick {
        crate::util::write_atomic(&vdir.join(READY_MARK), pinned.sha1.as_deref().unwrap_or_default().as_bytes())?;
    }
    Ok(Installed { layout, meta })
}

// ------------------------------------------------------------------ lançamento

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub nick: String,
    /// UUID do jogador (com ou sem hífens).
    pub uuid: String,
    pub heap_mb: u64,
    /// Endereço para entrar direto (Quick Play), como `localhost` ou `localhost:25570`.
    pub quick_play: Option<String>,
}

fn substitute(token: &str, vars: &BTreeMap<&str, String>) -> Option<String> {
    let mut out = String::with_capacity(token.len());
    let mut rest = token;
    while let Some(start) = rest.find("${") {
        let end = rest[start..].find('}')? + start;
        out.push_str(&rest[..start]);
        out.push_str(vars.get(&rest[start + 2..end])?);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Expande uma lista de argumentos do JSON. Um valor sem dado (ex.: `--xuid ${auth_xuid}` sem
/// conta Microsoft) sai junto com a sua opção, em vez de ir vazio para o jogo.
fn expand(args: &[Arg], features: &Features, vars: &BTreeMap<&str, String>) -> Vec<String> {
    let mut raw: Vec<Option<String>> = vec![];
    for a in args {
        match a {
            Arg::Plain(s) => raw.push(substitute(s, vars)),
            Arg::Ruled { rules, value } if allowed(rules, features) => match value {
                ArgValue::One(s) => raw.push(substitute(s, vars)),
                ArgValue::Many(v) => raw.extend(v.iter().map(|s| substitute(s, vars))),
            },
            Arg::Ruled { .. } => {}
        }
    }
    let mut out = vec![];
    let mut i = 0;
    while i < raw.len() {
        match &raw[i] {
            Some(s) if s.starts_with("--") && matches!(raw.get(i + 1), Some(None)) => i += 2,
            Some(s) => {
                out.push(s.clone());
                i += 1;
            }
            None => i += 1,
        }
    }
    out
}

impl Installed {
    pub fn client_jar(&self) -> PathBuf {
        self.layout.version_dir(&self.meta.id).join(format!("{}.jar", self.meta.id))
    }

    pub fn classpath(&self) -> String {
        self.classpath_with(None)
    }

    /// Classpath com uma versão da `.minecraft` por cima: as bibliotecas dela primeiro (as do
    /// vanilla com o mesmo grupo:artefato saem) e o jar dela, se tiver.
    pub fn classpath_with(&self, overlay: Option<&crate::instancias::Overlay>) -> String {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let libs = self.layout.libraries();
        let mut parts: Vec<String> = overlay.map(|o| o.libraries.iter().map(|p| p.display().to_string()).collect()).unwrap_or_default();
        let none = Features::new();
        let mut seen = BTreeSet::new();
        for l in &self.meta.libraries {
            if !allowed(&l.rules, &none) || !native_for_this_arch(&l.name) {
                continue;
            }
            if let Some(o) = overlay {
                let mut p = l.name.split(':');
                let ga = format!("{}:{}", p.next().unwrap_or(""), p.next().unwrap_or(""));
                if o.replaces.contains(&ga) {
                    continue;
                }
            }
            let Some(rel) = l.downloads.artifact.as_ref().and_then(|a| a.path.clone()) else { continue };
            if !seen.insert(rel.clone()) {
                continue;
            }
            if let Ok(p) = crate::util::join_rel(&libs, &rel) {
                parts.push(p.display().to_string());
            }
        }
        let jar = overlay.and_then(|o| o.jar.clone()).unwrap_or_else(|| self.client_jar());
        parts.push(jar.display().to_string());
        parts.join(sep)
    }

    /// Argumentos para o `java` (tudo depois do executável).
    pub fn command_line(&self, o: &LaunchOptions) -> NmResult<Vec<String>> {
        self.command_line_with(o, None)
    }

    /// Argumentos com uma versão da `.minecraft` (OptiFine, Fabric) por cima do vanilla.
    pub fn command_line_with(&self, o: &LaunchOptions, overlay: Option<&crate::instancias::Overlay>) -> NmResult<Vec<String>> {
        let id = overlay.map(|v| &v.id).unwrap_or(&self.meta.id);
        let game_dir = overlay.map(|v| v.game_dir.clone()).unwrap_or_else(|| self.layout.game());
        let mut vars: BTreeMap<&str, String> = BTreeMap::new();
        vars.insert("auth_player_name", o.nick.clone());
        vars.insert("version_name", id.clone());
        vars.insert("game_directory", game_dir.display().to_string());
        vars.insert("assets_root", self.layout.assets().display().to_string());
        vars.insert("assets_index_name", self.meta.asset_index.id.clone());
        vars.insert("auth_uuid", o.uuid.replace('-', ""));
        // modo offline: o servidor do build pessoal não confere token
        vars.insert("auth_access_token", "0".into());
        vars.insert("version_type", if self.meta.kind.is_empty() { "release".into() } else { self.meta.kind.clone() });
        vars.insert("natives_directory", self.layout.natives(&self.meta.id).display().to_string());
        vars.insert("library_directory", self.layout.libraries().display().to_string());
        vars.insert("classpath_separator", if cfg!(windows) { ";" } else { ":" }.to_string());
        vars.insert("launcher_name", "MachenCraft".into());
        vars.insert("launcher_version", crate::APP_VERSION.into());
        vars.insert("classpath", self.classpath_with(overlay));
        if let Some(q) = &o.quick_play {
            vars.insert("quickPlayMultiplayer", q.clone());
        }
        let mut features = Features::new();
        features.insert("is_quick_play_multiplayer", o.quick_play.is_some());

        let mut out = crate::heap::client_jvm_args(o.heap_mb);
        out.extend(expand(&self.meta.arguments.jvm, &features, &vars));
        if let Some(v) = overlay {
            out.extend(expand(&v.jvm, &features, &vars));
        }
        if let Some(l) = self.meta.logging.as_ref().and_then(|l| l.client.as_ref()) {
            let cfg = self.layout.assets().join("log_configs").join(log_config_name(l)?);
            out.push(l.argument.replace("${path}", &cfg.display().to_string()));
        }
        out.push(overlay.map(|v| v.main_class.clone()).unwrap_or_else(|| self.meta.main_class.clone()));
        out.extend(expand(&self.meta.arguments.game, &features, &vars));
        if let Some(v) = overlay {
            out.extend(expand(&v.game, &features, &vars));
        }
        Ok(out)
    }

    /// Prepara a pasta do jogo. Na primeira vez traz as opções (teclas, gráficos, volume) da
    /// `.minecraft`; sem isso, começa em português.
    pub fn prepare_game_dir(&self, minecraft_dir: Option<&Path>) -> std::io::Result<()> {
        let game = self.layout.game();
        std::fs::create_dir_all(&game)?;
        let opts = game.join("options.txt");
        if !opts.exists() {
            match minecraft_dir.map(|m| m.join("options.txt")).filter(|p| p.is_file()) {
                Some(src) => {
                    std::fs::copy(src, &opts)?;
                }
                None => std::fs::write(&opts, "lang:pt_br\n")?,
            }
        }
        Ok(())
    }
}

/// `javaw` ao lado do `java` (sem janela de console), se existir.
pub fn javaw(java: &Path) -> PathBuf {
    let w = java.with_file_name(if cfg!(windows) { "javaw.exe" } else { "java" });
    if w.exists() {
        w
    } else {
        java.to_path_buf()
    }
}

/// Abre o jogo. A saída vai para `log` (o jogo segue aberto mesmo se o app fechar).
pub fn spawn(inst: &Installed, java: &Path, args: &[String], log: &Path) -> std::io::Result<std::process::Child> {
    if let Some(d) = log.parent() {
        std::fs::create_dir_all(d)?;
    }
    let out = std::fs::File::create(log)?;
    let err = out.try_clone()?;
    crate::util::std_command(java)
        .args(args)
        .current_dir(inst.layout.game())
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> VersionMeta {
        serde_json::from_value(serde_json::json!({
            "id": "1.21.11",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": {"id": "29", "sha1": "e81d9e2536723a11eb68e11c5642708926b8be5f", "size": 1, "url": "https://x/29.json"},
            "downloads": {"client": {"sha1": "ba2df812c2d12e0219c489c4cd9a5e1f0760f5bd", "size": 1, "url": "https://x/client.jar"}},
            "javaVersion": {"component": "java-runtime-delta", "majorVersion": 21},
            "libraries": [
                {"name": "com.mojang:brigadier:1.3.10", "downloads": {"artifact": {"path": "com/mojang/brigadier/1.3.10/brigadier-1.3.10.jar", "sha1": "a", "size": 1, "url": "u"}}},
                {"name": "ca.weblite:java-objc-bridge:1.1", "rules": [{"action": "allow", "os": {"name": "osx"}}],
                 "downloads": {"artifact": {"path": "ca/objc.jar", "sha1": "a", "size": 1, "url": "u"}}},
                {"name": "org.lwjgl:lwjgl:3.3.3:natives-windows", "rules": [{"action": "allow", "os": {"name": "windows"}}],
                 "downloads": {"artifact": {"path": "org/lwjgl/lwjgl-natives-windows.jar", "sha1": "a", "size": 1, "url": "u"}}},
                {"name": "org.lwjgl:lwjgl:3.3.3:natives-windows-arm64", "rules": [{"action": "allow", "os": {"name": "windows"}}],
                 "downloads": {"artifact": {"path": "org/lwjgl/lwjgl-natives-windows-arm64.jar", "sha1": "a", "size": 1, "url": "u"}}}
            ],
            "arguments": {
                "game": ["--username", "${auth_player_name}", "--uuid", "${auth_uuid}", "--accessToken", "${auth_access_token}",
                         "--clientId", "${clientid}", "--xuid", "${auth_xuid}", "--gameDir", "${game_directory}",
                         {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"},
                         {"rules": [{"action": "allow", "features": {"is_quick_play_multiplayer": true}}], "value": ["--quickPlayMultiplayer", "${quickPlayMultiplayer}"]}],
                "jvm": [{"rules": [{"action": "allow", "os": {"name": "osx"}}], "value": ["-XstartOnFirstThread"]},
                        "-Djava.library.path=${natives_directory}", "-cp", "${classpath}"]
            },
            "logging": {"client": {"argument": "-Dlog4j.configurationFile=${path}",
                        "file": {"id": "client-1.21.2.xml", "sha1": "39384bd14c0606d812afec88d8aff595b2587dd9", "size": 1, "url": "u"}}}
        }))
        .unwrap()
    }

    fn installed() -> Installed {
        Installed { layout: Layout { root: PathBuf::from("/d/client") }, meta: meta() }
    }

    #[test]
    fn libraries_follow_rules_and_arch() {
        let m = meta();
        let libs: Vec<&str> = m.libraries_here().iter().map(|a| a.path.as_deref().unwrap()).collect();
        assert!(libs.contains(&"com/mojang/brigadier/1.3.10/brigadier-1.3.10.jar"));
        assert!(!libs.contains(&"ca/objc.jar"), "biblioteca só de macOS");
        assert!(!libs.contains(&"org/lwjgl/lwjgl-natives-windows-arm64.jar"), "nativa de outra arquitetura");
        if cfg!(all(windows, target_arch = "x86_64")) {
            assert!(libs.contains(&"org/lwjgl/lwjgl-natives-windows.jar"));
        }
        if !cfg!(windows) {
            assert!(!libs.contains(&"org/lwjgl/lwjgl-natives-windows.jar"));
        }
    }

    #[test]
    fn command_line_quick_play_and_offline() {
        let i = installed();
        let o = LaunchOptions { nick: "mchado".into(), uuid: "cb31b2e8-67fb-355d-b68a-1701810226f3".into(), heap_mb: 3072, quick_play: Some("localhost".into()) };
        let args = i.command_line(&o).unwrap();
        let s = args.join(" ");
        let main = args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        assert!(args[..main].contains(&"-Xmx3072M".to_string()), "memória antes da classe principal");
        assert!(args[..main].iter().any(|a| a.starts_with("-Dlog4j.configurationFile=") && a.ends_with("client-1.21.2.xml")));
        assert!(s.contains("--username mchado"));
        assert!(s.contains("--uuid cb31b2e867fb355db68a1701810226f3"));
        assert!(s.contains("--quickPlayMultiplayer localhost"));
        assert!(!s.contains("--xuid") && !s.contains("--clientId"), "opções sem valor saem inteiras");
        assert!(!s.contains("--demo") && !s.contains("${"));
        assert!(!s.contains("-XstartOnFirstThread") || cfg!(target_os = "macos"));
        let cp = &args[args.iter().position(|a| a == "-cp").unwrap() + 1];
        assert!(cp.ends_with("1.21.11.jar") && cp.contains("brigadier-1.3.10.jar"));

        let no_qp = i.command_line(&LaunchOptions { quick_play: None, ..o }).unwrap().join(" ");
        assert!(!no_qp.contains("quickPlay"));
    }

    #[test]
    fn substitute_handles_missing_and_multiple() {
        let mut v = BTreeMap::new();
        v.insert("a", "1".to_string());
        v.insert("b", "2".to_string());
        assert_eq!(substitute("x${a}y${b}", &v).as_deref(), Some("x1y2"));
        assert_eq!(substitute("${nada}", &v), None);
        assert_eq!(substitute("sem", &v).as_deref(), Some("sem"));
    }

    #[test]
    fn jobs_reject_bad_paths_and_dedupe_assets() {
        let mut m = meta();
        let idx: AssetIndex = serde_json::from_value(serde_json::json!({"objects": {
            "a.png": {"hash": "aabbccddeeff00112233445566778899aabbccdd", "size": 3},
            "b.png": {"hash": "aabbccddeeff00112233445566778899aabbccdd", "size": 3}
        }}))
        .unwrap();
        let layout = Layout { root: PathBuf::from("/d/client") };
        let jobs = jobs_for(&m, &idx, &layout, Some(Path::new("/mc"))).unwrap();
        let assets: Vec<_> = jobs.iter().filter(|j| j.immutable).collect();
        assert_eq!(assets.len(), 1);
        assert!(assets[0].url.ends_with("/aa/aabbccddeeff00112233445566778899aabbccdd"));
        assert_eq!(assets[0].reuse.as_deref(), Some(Path::new("/mc/assets/objects/aa/aabbccddeeff00112233445566778899aabbccdd")));
        m.libraries[0].downloads.artifact.as_mut().unwrap().path = Some("../../fora.jar".into());
        assert!(jobs_for(&m, &idx, &layout, None).is_err());
    }

    #[tokio::test]
    async fn reuses_verified_file_from_other_launcher() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("mc").join("obj");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"abc").unwrap();
        let job = Job {
            url: "http://127.0.0.1:9/nunca".into(),
            dest: d.path().join("client").join("obj"),
            sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
            size: 3,
            reuse: Some(src.clone()),
            immutable: true,
        };
        run_jobs(&http().unwrap(), vec![job.clone()], false, crate::events::quiet()).await.unwrap();
        assert_eq!(std::fs::read(&job.dest).unwrap(), b"abc");
        // arquivo adulterado na .minecraft não é aproveitado (e o download falha)
        std::fs::remove_file(&job.dest).unwrap();
        std::fs::write(&src, b"abd").unwrap();
        assert!(run_jobs(&http().unwrap(), vec![job.clone()], false, crate::events::quiet()).await.is_err());
        assert!(!job.dest.exists());
    }
}
