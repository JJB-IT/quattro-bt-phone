use clap::Parser;

/// Control phone calls through quattro-bt-phoned.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {}

fn main() -> anyhow::Result<()> {
    let _args = Args::parse();
    println!("{}", qbp_proto::socket_path().display());
    Ok(())
}
