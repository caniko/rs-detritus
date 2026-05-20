//! Embed a Detritus receiver and shut it down after a short delay.
//!
//! Run with:
//!
//!     cargo run --example embed_server -p detritus-server

use std::time::Duration;

use detritus_server::{
    RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig, TestToken, TokenStore,
    serve_with_shutdown,
};
use tokio::{net::TcpListener, sync::oneshot};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let bind = listener.local_addr()?;
    let data_dir = tempfile::tempdir()?;
    let config = ServerConfig {
        bind,
        data_dir: data_dir.path().to_path_buf(),
        max_dump_bytes: 100 * 1024 * 1024,
        token_store: TokenStore::for_tests(Vec::<TestToken>::new()),
        rate_limit: RateLimitConfig::default(),
        retention: RetentionConfig::default(),
        schema_registry: SchemaRegistry::empty(),
    };

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = shutdown_tx.send(());
    });

    println!("serving Detritus example receiver on {bind}");
    serve_with_shutdown(listener, config, async move {
        let _ = shutdown_rx.await;
    })
    .await?;
    Ok(())
}
