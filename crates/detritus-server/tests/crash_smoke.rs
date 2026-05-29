//! Crash ingestion smoke tests.

use std::{net::SocketAddr, path::Path, sync::Once};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use chrono::Utc;
use detritus_protocol::{
    BuildInfo, CrashEnvelope, CrashKind, CrashMetadata, PROTOCOL_VERSION, SourceId,
    multipart::DEFAULT_BOUNDARY,
};
use detritus_server::{
    RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig, TestToken, TokenStore,
    serve_with_shutdown,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

const INSTALL_ID: &str = "11111111-1111-1111-1111-111111111111";

#[tokio::test]
async fn crash_smoke_writes_blob_and_source_index_with_dedup() {
    install_default_crypto_provider();

    let temp = TempDir::new().expect("temp dir");
    let (addr, shutdown, handle) = spawn_server(temp.path()).await;
    let dump = deterministic_dump();
    let sha256 = hex::encode(Sha256::digest(&dump));

    let no_auth = post_crash_response(
        addr,
        deterministic_dump(),
        PROTOCOL_VERSION,
        None,
        "detritus",
    )
    .await;
    assert_eq!(no_auth.status(), reqwest::StatusCode::UNAUTHORIZED);

    let wrong_project = post_crash_response(
        addr,
        deterministic_dump(),
        PROTOCOL_VERSION,
        Some("secret-token"),
        "rs-modde",
    )
    .await;
    assert_eq!(wrong_project.status(), reqwest::StatusCode::FORBIDDEN);

    let first = post_crash(addr, dump.clone(), PROTOCOL_VERSION).await;
    assert_eq!(first["id"], sha256);
    assert_eq!(first["dedup"], false);

    let blob = temp
        .path()
        .join("crashes")
        .join("by-hash")
        .join(&sha256[..2])
        .join(format!("{sha256}.bin"));
    assert!(blob.exists(), "blob exists at content-addressed path");
    let blob_count_before = count_blobs(temp.path()).await;
    assert_eq!(blob_count_before, 1);

    let second = post_crash(addr, dump, PROTOCOL_VERSION).await;
    assert_eq!(second["id"], sha256);
    assert_eq!(second["dedup"], true);
    assert_eq!(count_blobs(temp.path()).await, blob_count_before);

    let index_dir = temp
        .path()
        .join("crashes")
        .join("by-source")
        .join("detritus")
        .join(INSTALL_ID);
    let mut entries = tokio::fs::read_dir(index_dir).await.expect("index dir");
    let mut index_count = 0;
    while let Some(entry) = entries.next_entry().await.expect("index entry") {
        index_count += 1;
        let parsed: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(entry.path()).await.expect("read index"))
                .expect("index json parses");
        assert_eq!(parsed["metadata"]["schema_version"], PROTOCOL_VERSION);
        assert_eq!(parsed["dump"]["sha256"], sha256);
    }
    assert_eq!(index_count, 2);

    let mismatch = post_crash_response(
        addr,
        deterministic_dump(),
        PROTOCOL_VERSION + 1,
        Some("secret-token"),
        "detritus",
    )
    .await;
    assert_eq!(mismatch.status(), reqwest::StatusCode::PRECONDITION_FAILED);
    let error: serde_json::Value = mismatch.json().await.expect("structured error");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not match")
    );

    let metrics = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("client")
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .expect("get metrics");
    assert_eq!(metrics.status(), reqwest::StatusCode::OK);
    let content_type = metrics
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .expect("content type")
        .to_str()
        .expect("content type string")
        .to_owned();
    assert!(content_type.contains("application/openmetrics-text"));
    let metrics_body = metrics.text().await.expect("metrics body");
    for metric in [
        "detritus_requests_total",
        "detritus_request_duration_seconds",
        "detritus_bytes_ingested_total",
        "detritus_dedup_hits_total",
        "detritus_writer_queue_depth",
        "detritus_janitor_cycles_total",
        "detritus_janitor_blobs_freed_total",
        "detritus_janitor_bytes_freed_total",
        "detritus_janitor_cycle_duration_seconds",
    ] {
        assert!(metrics_body.contains(metric), "missing metric {metric}");
    }

    shutdown.send(()).expect("send shutdown");
    handle
        .await
        .expect("server task")
        .expect("server exits cleanly");
}

async fn post_crash(addr: SocketAddr, dump: Vec<u8>, schema_version: u32) -> serde_json::Value {
    let response =
        post_crash_response(addr, dump, schema_version, Some("secret-token"), "detritus").await;
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response.json().await.expect("crash response json")
}

async fn post_crash_response(
    addr: SocketAddr,
    dump: Vec<u8>,
    schema_version: u32,
    token: Option<&str>,
    project: &str,
) -> reqwest::Response {
    let envelope = CrashEnvelope {
        metadata: metadata(schema_version, project),
        dump,
        attachments: Vec::new(),
    };
    let mut body = Vec::new();
    envelope
        .write_to(&mut body)
        .await
        .expect("multipart encodes");
    let builder = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("client")
        .post(format!("http://{addr}/v1/crashes"))
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={DEFAULT_BOUNDARY}"),
        );
    let builder = if let Some(token) = token {
        builder.header(AUTHORIZATION, format!("Bearer {token}"))
    } else {
        builder
    };
    builder.body(body).send().await.expect("post crash")
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

fn metadata(schema_version: u32, project: &str) -> CrashMetadata {
    CrashMetadata {
        schema_version,
        source: SourceId {
            project: project.to_owned(),
            platform: "linux".to_owned(),
            version: "0.1.0".to_owned(),
            install_id: Uuid::parse_str(INSTALL_ID).expect("uuid"),
        },
        timestamp: Utc::now(),
        kind: CrashKind::Minidump,
        build: BuildInfo {
            git_sha: "abcdef0".to_owned(),
            profile: "release".to_owned(),
            target_triple: "x86_64-unknown-linux-gnu".to_owned(),
        },
        panic_text: None,
        context: json!({ "test": "crash_smoke" }),
        attachments: Vec::new(),
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

fn deterministic_dump() -> Vec<u8> {
    (0..1024 * 1024).map(|index| (index % 251) as u8).collect()
}

async fn count_blobs(data_dir: &Path) -> usize {
    let hash_root = data_dir.join("crashes").join("by-hash");
    let mut prefixes = tokio::fs::read_dir(hash_root).await.expect("hash root");
    let mut count = 0;
    while let Some(prefix) = prefixes.next_entry().await.expect("prefix") {
        let mut blobs = tokio::fs::read_dir(prefix.path())
            .await
            .expect("prefix dir");
        while blobs.next_entry().await.expect("blob").is_some() {
            count += 1;
        }
    }
    count
}

fn install_default_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}
