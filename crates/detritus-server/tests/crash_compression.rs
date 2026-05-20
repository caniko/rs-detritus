//! Integration tests for client-side zstd compression of crash dumps.
//!
//! These tests verify that:
//!  1. A compressible dump uploaded with `Content-Encoding: zstd` is stored
//!     smaller than the original source bytes.
//!  2. The per-crash index records `content_encoding: "zstd"` on the dump
//!     pointer.
//!  3. Re-uploading the same source bytes (which compress to the same output)
//!     returns `dedup: true`.

use std::{net::SocketAddr, path::Path, sync::Once};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use chrono::Utc;
use detritus_protocol::{
    BuildInfo, CrashEnvelope, CrashKind, CrashMetadata, PROTOCOL_VERSION, SourceId,
    multipart::{DEFAULT_BOUNDARY, EnvelopeEncodings, PartEncoding},
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

const INSTALL_ID: &str = "22222222-2222-2222-2222-222222222222";

/// Builds a highly compressible payload: 10 KB of repeated ASCII text.
fn compressible_payload() -> Vec<u8> {
    "The quick brown fox jumps over the lazy dog. "
        .repeat(228)
        .into_bytes()
}

#[tokio::test]
async fn compressed_dump_is_stored_smaller_index_records_encoding_and_dedup_works() {
    install_default_crypto_provider();

    let temp = TempDir::new().expect("temp dir");
    let (addr, shutdown, handle) = spawn_server(temp.path()).await;

    let source_bytes = compressible_payload();
    let source_len = source_bytes.len();

    // Compress with zstd level 19, matching the client default.
    let compressed = zstd::encode_all(source_bytes.as_slice(), 19).expect("zstd compress");
    let compressed_len = compressed.len();
    assert!(
        compressed_len < source_len,
        "zstd should shrink a compressible payload: {source_len} → {compressed_len}"
    );

    // The hash covers the *compressed* bytes.
    let expected_sha256 = hex::encode(Sha256::digest(&compressed));

    // First upload.
    let first = post_compressed_dump(addr, compressed.clone()).await;
    assert_eq!(first.status(), reqwest::StatusCode::CREATED);
    let first_body: serde_json::Value = first.json().await.expect("json response");
    assert_eq!(
        first_body["id"], expected_sha256,
        "id matches sha256 of compressed bytes"
    );
    assert_eq!(first_body["dedup"], false, "first upload is not a dedup");

    // Verify the blob exists and is smaller than the original source.
    let blob_path = temp
        .path()
        .join("crashes")
        .join("by-hash")
        .join(&expected_sha256[..2])
        .join(format!("{expected_sha256}.bin"));
    assert!(blob_path.exists(), "blob exists at content-addressed path");
    let stored_len = tokio::fs::metadata(&blob_path)
        .await
        .expect("blob metadata")
        .len();
    assert!(
        stored_len < source_len as u64,
        "stored blob ({stored_len} bytes) must be smaller than original source ({source_len} bytes)"
    );

    // Verify the index records `content_encoding: "zstd"`.
    let index_dir = temp
        .path()
        .join("crashes")
        .join("by-source")
        .join("detritus")
        .join(INSTALL_ID);
    let mut entries = tokio::fs::read_dir(&index_dir).await.expect("index dir");
    let mut index_count = 0;
    while let Some(entry) = entries.next_entry().await.expect("index entry") {
        let raw = tokio::fs::read(entry.path()).await.expect("read index");
        let parsed: serde_json::Value = serde_json::from_slice(&raw).expect("index json");
        assert_eq!(
            parsed["dump"]["content_encoding"], "zstd",
            "dump index must record content_encoding = zstd"
        );
        assert_eq!(parsed["dump"]["sha256"], expected_sha256);
        index_count += 1;
    }
    assert_eq!(index_count, 1, "exactly one index entry after first upload");

    // Second upload of identical source bytes → same compressed output → dedup.
    let second = post_compressed_dump(addr, compressed).await;
    assert_eq!(second.status(), reqwest::StatusCode::CREATED);
    let second_body: serde_json::Value = second.json().await.expect("json response");
    assert_eq!(
        second_body["dedup"], true,
        "re-uploading same compressed bytes returns dedup=true"
    );
    assert_eq!(second_body["id"], expected_sha256);

    shutdown.send(()).expect("shutdown");
    handle
        .await
        .expect("server task")
        .expect("server exits cleanly");
}

/// Sends a crash envelope whose `dump` part has `Content-Encoding: zstd`.
async fn post_compressed_dump(addr: SocketAddr, compressed_dump: Vec<u8>) -> reqwest::Response {
    let envelope = CrashEnvelope {
        metadata: metadata(),
        dump: compressed_dump,
        attachments: Vec::new(),
    };
    let encodings = EnvelopeEncodings {
        dump: PartEncoding {
            content_encoding: Some("zstd".to_owned()),
        },
        attachments: Vec::new(),
    };
    let mut body = Vec::new();
    envelope
        .write_to_with_boundary_and_encodings(&mut body, DEFAULT_BOUNDARY, &encodings)
        .await
        .expect("multipart encodes");
    reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("client")
        .post(format!("http://{addr}/v1/crashes"))
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={DEFAULT_BOUNDARY}"),
        )
        .header(AUTHORIZATION, "Bearer secret-token")
        .body(body)
        .send()
        .await
        .expect("send request")
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

fn metadata() -> CrashMetadata {
    CrashMetadata {
        schema_version: PROTOCOL_VERSION,
        source: SourceId {
            project: "detritus".to_owned(),
            platform: "linux".to_owned(),
            version: "0.1.0".to_owned(),
            install_id: Uuid::parse_str(INSTALL_ID).expect("uuid"),
        },
        timestamp: Utc::now(),
        kind: CrashKind::PanicTarball,
        build: BuildInfo {
            git_sha: "abcdef0".to_owned(),
            profile: "test".to_owned(),
            target_triple: "x86_64-unknown-linux-gnu".to_owned(),
        },
        panic_text: None,
        context: json!({ "test": "crash_compression" }),
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

fn install_default_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}
