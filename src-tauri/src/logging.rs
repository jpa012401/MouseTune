//! File + stderr logging. Logs go to `~/Library/Logs/com.johnpaulalabe.mousetune/`.

use tauri::{AppHandle, Manager};
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Must be kept alive for the lifetime of the app, or buffered log lines are lost.
pub struct LogGuard(#[allow(dead_code)] WorkerGuard);

pub fn init(app: &AppHandle) -> Result<LogGuard, Box<dyn std::error::Error>> {
    let dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&dir)?;

    let file = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("mousetune")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&dir)?;
    let (writer, guard) = tracing_appender::non_blocking(file);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,mousetune_lib=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init()?;

    Ok(LogGuard(guard))
}
