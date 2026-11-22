use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

/// Daily log files (7 kept) plus stderr. Level via `KAVOX_LOG`, default `info`.
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let _ = std::fs::create_dir_all(dir);
    let filter = || EnvFilter::try_from_env("KAVOX_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr = fmt::layer().with_writer(std::io::stderr);
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("kavox")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir);
    match appender {
        Ok(file) => {
            let (writer, guard) = tracing_appender::non_blocking(file);
            let file_layer = fmt::layer().with_writer(writer).with_ansi(false);
            let _ = tracing_subscriber::registry()
                .with(filter())
                .with(stderr)
                .with(file_layer)
                .try_init();
            Some(guard)
        }
        Err(e) => {
            let _ = tracing_subscriber::registry()
                .with(filter())
                .with(stderr)
                .try_init();
            tracing::warn!("logging to stderr only: {e}");
            None
        }
    }
}
