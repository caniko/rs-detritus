//! Command-line entry point for the `detritusd` receiver.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use clap::{Parser, ValueEnum};
use detritus_server::{RetentionConfig, ServerConfig, load_security_config, serve};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Parser)]
#[command(about = "Detritus ingestion server")]
struct Cli {
    /// Socket address to bind. Port 4317 is the OTLP/gRPC convention.
    #[arg(long, default_value = "127.0.0.1:4317")]
    bind: SocketAddr,
    /// Storage root for logs, crash blobs, crash indexes, and temp files.
    #[arg(long, default_value = "./data/")]
    data_dir: PathBuf,
    /// Format for the server's own stderr logs.
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,
    /// Maximum bytes accepted for each dump or attachment part.
    #[arg(long, default_value_t = 100 * 1024 * 1024)]
    max_dump_bytes: u64,
    /// TOML file containing Argon2-hashed bearer-token entries.
    #[arg(long)]
    tokens_config: PathBuf,
    /// NDJSON retention in days.
    #[arg(long, default_value_t = 14)]
    logs_ttl_days: u64,
    /// Crash index retention in days.
    #[arg(long, default_value_t = 90)]
    crashes_ttl_days: u64,
    /// Janitor interval in seconds.
    #[arg(long, default_value_t = 60 * 60)]
    janitor_interval_secs: u64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogFormat {
    Json,
    Pretty,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("first and only CryptoProvider install, at startup before any TLS use");

    let cli = Cli::parse();
    init_tracing(cli.log_format);

    let data_dir = absolute_path(&cli.data_dir)?;
    let tokens_config = absolute_path(&cli.tokens_config)?;
    tracing::info!(path = %tokens_config.display(), "loading token config");
    let security = load_security_config(&tokens_config).await?;
    let config = ServerConfig {
        bind: cli.bind,
        data_dir,
        max_dump_bytes: cli.max_dump_bytes,
        token_store: security.token_store,
        rate_limit: security.rate_limit,
        schema_registry: security.schema_registry,
        retention: RetentionConfig {
            logs_ttl_days: cli.logs_ttl_days,
            crashes_ttl_days: cli.crashes_ttl_days,
            janitor_interval: std::time::Duration::from_secs(cli.janitor_interval_secs),
        },
    };

    serve(config).await
}

fn init_tracing(log_format: LogFormat) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match log_format {
        LogFormat::Json => fmt().with_env_filter(env_filter).json().init(),
        LogFormat::Pretty => fmt().with_env_filter(env_filter).init(),
    }
}

fn absolute_path(path: &Path) -> std::io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
