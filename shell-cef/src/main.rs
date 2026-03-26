mod actions;
mod protocol;
mod server;

use std::path::PathBuf;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "twake-shell-cef", version, about = "Twake Desktop Shell-CEF RPC server")]
struct Cli {
    /// Unix socket path.
    #[arg(long = "sock", env = "TWAKE_SHELL_CEF_SOCK", default_value = "/tmp/twake-shell-cef.sock")]
    sock: PathBuf,

    /// Log level (error, warn, info, debug, trace).
    #[arg(long = "log-level", env = "TWAKE_LOG_LEVEL", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing subscriber.
    let filter = EnvFilter::try_new(&cli.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    info!(sock = ?cli.sock, "Starting shell-cef");

    // Shutdown channel driven by SIGTERM / SIGINT.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("failed to install SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => info!("Received SIGTERM"),
            _ = sigint.recv() => info!("Received SIGINT"),
        }

        let _ = shutdown_tx.send(true);
    });

    server::run(&cli.sock, shutdown_rx).await?;

    Ok(())
}
