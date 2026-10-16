//! Logs estruturados com rotação diária (mantém 14 arquivos).

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Inicia o log em arquivo JSON (rotação diária) e, se `console`, também no terminal.
/// Guarde o `WorkerGuard` enquanto o app estiver vivo.
pub fn init(dir: &Path, console: bool) -> anyhow::Result<WorkerGuard> {
    std::fs::create_dir_all(dir)?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("pauletor3")
        .filename_suffix("log")
        .max_log_files(14)
        .build(dir)?;
    let (nb, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("PAULETOR3_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = fmt::layer().json().with_writer(nb).with_target(true);
    let console_layer = console.then(|| fmt::layer().compact().with_target(false).with_writer(std::io::stderr));
    let _ = tracing_subscriber::registry().with(filter).with(file_layer).with(console_layer).try_init();
    Ok(guard)
}
