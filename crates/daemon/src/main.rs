use clap::Parser;

/// Bluetooth hands-free phone daemon for Omarchy.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Simulate a phone instead of talking to D-Bus (for UI development).
    #[arg(long)]
    mock: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    tracing::info!(mock = args.mock, socket = %qbp_proto::socket_path().display(), "quattro-bt-phoned starting");
    Ok(())
}
