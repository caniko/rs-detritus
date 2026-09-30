//! Client tracing layer smoke tests.

use std::{net::SocketAddr, path::Path, time::Duration};

use chrono::Utc;
use detritus::{CompressionEncoding, Layer, SourceId};
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
    for compression in [
        None,
        Some(CompressionEncoding::Gzip),
        Some(CompressionEncoding::Zstd),
    ] {
        exports_tracing_events(compression).await;
    }
}

async fn exports_tracing_events(compression: Option<CompressionEncoding>) {
    let temp = TempDir::new().expect("temp dir");
    let queue = TempDir::new().expect("queue dir");
    let (addr, shutdown, handle) = spawn_server(temp.path()).await;
    let mut builder = Layer::builder()
        .endpoint(format!("http://{addr}").parse().expect("endpoint"))
        .token(SecretString::from("secret-token"))
        .source(source())
        .queue_dir(queue.path().to_path_buf())
        .batch_size(10)
        .flush_interval(Duration::from_secs(60));
    if let Some(encoding) = compression {
        builder = builder.compression(encoding);
    }
    let layer = builder.build().expect("build layer");
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
    assert!(
        std::fs::read_dir(queue.path()).unwrap().all(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_none_or(|ext| ext != "protobuf")
        }),
        "successful compressed exports must not be spooled"
    );
}

#[tokio::test]
async fn tls_private_ca_exports_share_one_connection_across_flushes() {
    use detritus::{Certificate, ClientTlsConfig};
    use std::sync::Arc;
    use tokio_rustls::{
        TlsAcceptor,
        rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    };
    const CERT: &[u8] = include_bytes!("fixtures/tls/localhost-cert.pem");
    const KEY: &[u8] = include_bytes!("fixtures/tls/localhost-key.pem");
    detritus::install_default_crypto_provider();
    let data = TempDir::new().unwrap();
    let queue = TempDir::new().unwrap();
    let (addr, shutdown, handle) = spawn_server(data.path()).await;
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from_pem_slice(CERT).unwrap()],
            PrivateKeyDer::from_pem_slice(KEY).unwrap(),
        )
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tls_addr = listener.local_addr().unwrap();
    let proxy = tokio::spawn(async move {
        // Accept exactly one connection: recreating a channel on each flush
        // would stall the second request and fail the test.
        let (socket, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(socket).await.unwrap();
        let mut upstream = tokio::net::TcpStream::connect(addr).await.unwrap();
        tokio::io::copy_bidirectional(&mut tls, &mut upstream)
            .await
            .unwrap();
    });
    let layer = Layer::builder()
        .endpoint(
            format!("https://localhost:{}", tls_addr.port())
                .parse()
                .unwrap(),
        )
        .tls_config(ClientTlsConfig::new().ca_certificate(Certificate::from_pem(CERT)))
        .compression(CompressionEncoding::Zstd)
        .token(SecretString::from("secret-token"))
        .source(source())
        .queue_dir(queue.path().to_owned())
        .flush_interval(Duration::from_secs(60))
        .build()
        .unwrap();
    // Synchronize past the interval's initial tick before emitting records.
    layer.flush().await.unwrap();
    for index in 0..2 {
        tracing::subscriber::with_default(Registry::default().with(layer.clone()), || {
            tracing::info!(index, "private CA export");
        });
        layer.flush().await.unwrap();
    }
    drop(layer);
    tokio::time::timeout(Duration::from_secs(5), proxy)
        .await
        .unwrap()
        .unwrap();
    shutdown.send(()).unwrap();
    handle.await.unwrap().unwrap();
    let text = tokio::fs::read_to_string(
        data.path()
            .join("logs/detritus")
            .join(INSTALL_ID)
            .join(format!("{}.ndjson", Utc::now().date_naive())),
    )
    .await
    .unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.lines().all(|line| line.contains("private CA export")));
}

#[tokio::test]
async fn offline_batches_replay_once_and_invalid_tokens_remain_spooled() {
    let data = TempDir::new().unwrap();
    let queue = TempDir::new().unwrap();
    let offline = Layer::builder()
        .endpoint("http://127.0.0.1:1".parse().unwrap())
        .token(SecretString::from("secret-token"))
        .source(source())
        .queue_dir(queue.path().to_owned())
        .flush_interval(Duration::from_secs(60))
        .build()
        .unwrap();
    offline.flush().await.unwrap();
    tracing::subscriber::with_default(Registry::default().with(offline.clone()), || {
        tracing::info!("replay me exactly once");
    });
    assert!(offline.flush().await.is_err());
    drop(offline);
    let corrupt = queue.path().join("corrupt.protobuf");
    std::fs::write(&corrupt, [255]).unwrap();
    let (addr, shutdown, handle) = spawn_server(data.path()).await;
    let online = Layer::builder()
        .endpoint(format!("http://{addr}").parse().unwrap())
        .token(SecretString::from("secret-token"))
        .source(source())
        .queue_dir(queue.path().to_owned())
        .sample_rate(0.0)
        .flush_interval(Duration::from_secs(60))
        .build()
        .unwrap();
    online.flush().await.unwrap();
    tracing::subscriber::with_default(Registry::default().with(online.clone()), || {
        tracing::info!("sampled out");
    });
    online.flush().await.unwrap();
    drop(online);
    let pending = std::fs::read_dir(queue.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "protobuf"))
        .collect::<Vec<_>>();
    assert_eq!(pending, vec![corrupt]);
    let bad_queue = TempDir::new().unwrap();
    let bad = Layer::builder()
        .endpoint(format!("http://{addr}").parse().unwrap())
        .token(SecretString::from("sensitive\ninvalid"))
        .source(source())
        .queue_dir(bad_queue.path().to_owned())
        .flush_interval(Duration::from_secs(60))
        .build()
        .unwrap();
    bad.flush().await.unwrap();
    tracing::subscriber::with_default(Registry::default().with(bad.clone()), || {
        tracing::info!("invalid header");
    });
    let error = bad.flush().await.unwrap_err();
    assert!(!error.to_string().contains("sensitive"));
    assert!(std::fs::read_dir(bad_queue.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "protobuf")
    }));
    drop(bad);
    shutdown.send(()).unwrap();
    handle.await.unwrap().unwrap();
    let text = tokio::fs::read_to_string(
        data.path()
            .join("logs/detritus")
            .join(INSTALL_ID)
            .join(format!("{}.ndjson", Utc::now().date_naive())),
    )
    .await
    .unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("replay me exactly once"));
}

async fn spawn_server(
    data_dir: &Path,
) -> (
    SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), detritus_server::ServerError>>,
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
    include_str!("fixtures/legacy-token.phc").trim().to_owned()
}
