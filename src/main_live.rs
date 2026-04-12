use anyhow::Result;
use clap::Parser;

use gex_strategy::broker::ibkr::IbkrBroker;
use gex_strategy::config::strategy::StrategyConfig;
use gex_strategy::config::tickers::Ticker;
use gex_strategy::live::auth::{set_basic_auth, ServerConfig};
use gex_strategy::live::runner::run_live;
use gex_strategy::live::set_verbose;

#[derive(Parser)]
#[command(name = "live", about = "Run GEX strategy live trading")]
struct Args {
    /// HTTP/HTTPS server port
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Enable verbose debug logging (per-bar updates, recovery details, etc.)
    #[arg(long)]
    debug: bool,

    /// Basic auth credentials (user:password). Omit for no auth.
    #[arg(long, env = "DASHBOARD_AUTH")]
    auth: Option<String>,

    /// Path to TLS certificate PEM file (enables HTTPS)
    #[arg(long, env = "TLS_CERT")]
    tls_cert: Option<String>,

    /// Path to TLS private key PEM file
    #[arg(long, env = "TLS_KEY")]
    tls_key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args = Args::parse();
    set_verbose(args.debug);

    if let Some(cred) = args.auth {
        anyhow::ensure!(cred.contains(':'), "--auth must be user:password");
        set_basic_auth(cred);
    }

    let server_cfg = ServerConfig {
        port: args.port,
        tls_cert: args.tls_cert,
        tls_key: args.tls_key,
    };

    let tickers = Ticker::STRATEGY.to_vec();
    let config = StrategyConfig::default();
    let broker = IbkrBroker::new();

    println!("[live] Tickers: {:?}", tickers);
    println!("[live] Port: {}", server_cfg.port);

    run_live(&tickers, &config, broker, server_cfg).await
}
