mod audio;
mod backend;
mod bluez;
mod config;
mod dbus;
mod notify;
mod pbap;
mod server;
mod store;
mod telephony;
mod vcard;

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tokio::signal::unix::{SignalKind, signal};

/// Bluetooth hands-free phone daemon for Omarchy.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Simulate a phone instead of talking to D-Bus (for UI development).
    #[arg(long)]
    mock: bool,
    /// Config file [default: $XDG_CONFIG_HOME/quattro-bt-phone/config.toml]
    #[arg(long)]
    config: Option<PathBuf>,
    /// Socket path [default: $XDG_RUNTIME_DIR/quattro-bt-phone.sock]
    #[arg(long)]
    socket: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,zbus=warn".into()),
        )
        .init();
    let args = Args::parse();
    let config_path = args.config.unwrap_or_else(config::Config::default_path);
    let config = config::Config::load(&config_path)?;
    let socket = args.socket.unwrap_or_else(qbp_proto::socket_path);

    let listener = server::bind(&socket)?;
    tracing::info!(socket = %socket.display(), mock = args.mock, "listening");

    let (handle, jobs, state) = backend::Handle::new();
    let backend = async {
        if args.mock {
            backend::mock::run(jobs, state, config.auto_record).await;
            Ok(())
        } else {
            backend::real::run(jobs, state, config, config_path).await
        }
    };

    let mut term = signal(SignalKind::terminate())?;
    let result = tokio::select! {
        r = backend => r.context("backend stopped"),
        r = server::serve(listener, handle) => r.context("socket server stopped"),
        _ = tokio::signal::ctrl_c() => Ok(()),
        _ = term.recv() => Ok(()),
    };
    let _ = std::fs::remove_file(&socket);
    result
}
