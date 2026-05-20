//! Client tracing layer smoke tests.

use std::{net::SocketAddr, path::Path, time::Duration};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use chrono::Utc;
use detritus::{Layer, SourceId};
use detritus_server::{
    RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig, TestToken, TokenStore,
    serve_with_shutdown,
};
use secrecy::SecretString;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tracing_subscriber::{Registry, layer::SubscriberExt};
use uuid::Uuid;

const INSTALL_ID: &str = "11111111-1111-1111-1111-111111111111";

#[tokio::test(flavor = "multi_thread")]
async fn layer_smoke_exports_tracing_events_to_server() {
    let temp = TempDir::new().expect("temp dir");
    let queue = TempDir::new().expect("queue dir");
    let (addr, shutdown, handle) = spawn_server(temp.path()).await;
    let layer = Layer::builder()
        .endpoint(format!("http://{addr}").parse().expect("endpoint"))
        .token(SecretString::from("secret-token"))
        .source(source())
        .queue_dir(queue.path().to_path_buf())
        .batch_size(10)
        .flush_interval(Duration::from_secs(60))
        .build()
        .expect("build layer");
    let subscriber = Registry::default().with(layer.clone());

    tracing::subscriber::with_default(subscriber, || {
        for index in 0..50 {
            tracing::info!(index, "client sdk smoke log");
        }
    });
    layer.flush().await.expect("flush layer");

    shutdown.send(()).expect("send shutdown");
    handle
        .await
        .expect("server task")
        .expect("server exits cleanly");

    let today = Utc::now().date_naive();
    let file = temp
        .path()
        .join("logs")
        .join("detritus")
        .join(INSTALL_ID)
        .join(format!("{today}.ndjson"));
    let content = tokio::fs::read_to_string(file).await.expect("read ndjson");
    let lines = content.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 50);
    assert!(
        lines
            .iter()
            .all(|line| line.contains("client sdk smoke log"))
    );
}

async fn spawn_server(
    data_dir: &Path,
) -> (
    SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let config = ServerConfig {
        bind: addr,
        data_dir: data_dir.to_path_buf(),
        max_dump_bytes: 100 * 1024 * 1024,
        token_store: test_token_store(),
        rate_limit: RateLimitConfig::default(),
        retention: RetentionConfig::default(),
        schema_registry: SchemaRegistry::empty(),
    };
    let handle = tokio::spawn(serve_with_shutdown(listener, config, async {
        let _ = shutdown_rx.await;
    }));
    (addr, shutdown_tx, handle)
}

fn source() -> SourceId {
    SourceId {
        project: "detritus".to_owned(),
        platform: "linux".to_owned(),
        version: "0.1.0".to_owned(),
        install_id: Uuid::parse_str(INSTALL_ID).expect("install id"),
    }
}

fn test_token_store() -> TokenStore {
    TokenStore::for_tests(vec![TestToken {
        id: "detritus-test".to_owned(),
        secret_hash: test_hash(),
        project: "detritus".to_owned(),
        source_prefix: "detritus/".to_owned(),
    }])
}

fn test_hash() -> String {
    Argon2::default()
        .hash_password(
            b"secret-token",
            &SaltString::from_b64("c29tZXNhbHQ").expect("salt"),
        )
        .expect("hash token")
        .to_string()
}
