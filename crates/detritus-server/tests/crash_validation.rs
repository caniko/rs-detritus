//! Schema-validation integration tests for the crash ingestion endpoint.
//!
//! These tests verify that:
//! - A crash whose metadata violates a registered schema is rejected with HTTP
//!   422 and no bytes are written to disk.
//! - A crash whose metadata satisfies the registered schema is accepted with
//!   HTTP 201.
//! - A crash for a tenant with no registered schema is accepted regardless of
//!   its content (accept-by-default).

use std::{net::SocketAddr, path::Path};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use chrono::Utc;
use detritus_protocol::{
    BuildInfo, CrashEnvelope, CrashKind, CrashMetadata, PROTOCOL_VERSION, SourceId,
    multipart::DEFAULT_BOUNDARY,
};
use detritus_server::{
    ProjectSchemaEntry, RateLimitConfig, RetentionConfig, SchemaKind, SchemaRegistry, ServerConfig,
    TestToken, TokenStore, serve_with_shutdown,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

const INSTALL_ID: &str = "22222222-2222-2222-2222-222222222222";

// ──────────────────────────────────────────────────────────────────────────────
// Test: rejection with a registered schema
// ──────────────────────────────────────────────────────────────────────────────

/// A schema that requires a top-level `"context"` property to have a string
/// field called `"required_tag"`.
fn strict_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["context"],
        "properties": {
            "context": {
                "type": "object",
                "required": ["required_tag"],
                "properties": {
                    "required_tag": { "type": "string" }
                }
            }
        }
    })
}

#[tokio::test]
async fn crash_rejected_when_schema_violated_and_no_blob_written() {
    let temp = TempDir::new().expect("temp dir");
    let schema_path = temp.path().join("crash.schema.json");
    std::fs::write(&schema_path, strict_schema().to_string()).expect("write schema");
    let registry = SchemaRegistry::load(&[ProjectSchemaEntry {
        project: "detritus".to_owned(),
        kind: SchemaKind::CrashMetadata,
        path: schema_path,
    }])
    .await
    .expect("load schema");

    let (addr, shutdown, handle) = spawn_server(temp.path(), registry).await;

    // ── 1. Send a crash that is MISSING the required `context.required_tag`. ──
    let invalid_response = post_crash_raw(
        addr,
        // context does not contain `required_tag`
        json!({ "unrelated": "data" }),
        Some("secret-token"),
    )
    .await;
    assert_eq!(
        invalid_response.status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        "metadata violating the registered schema must be rejected with 422"
    );
    let body: serde_json::Value = invalid_response.json().await.expect("error body json");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("schema validation failed"),
        "error message should mention schema validation; got: {}",
        body["error"]["message"]
    );

    // ── 2. Assert nothing was written to the blobs directory. ─────────────────
    let blobs_dir = temp.path().join("crashes").join("by-hash");
    let blob_count = count_blobs_if_exists(&blobs_dir).await;
    assert_eq!(
        blob_count, 0,
        "no blobs should be written for a rejected crash"
    );

    // ── 3. A crash that SATISFIES the schema is accepted. ─────────────────────
    let valid_response = post_crash_raw(
        addr,
        json!({ "required_tag": "present" }),
        Some("secret-token"),
    )
    .await;
    assert_eq!(
        valid_response.status(),
        reqwest::StatusCode::CREATED,
        "metadata satisfying the registered schema must be accepted"
    );

    shutdown.send(()).expect("shutdown");
    handle.await.expect("task").expect("server");
}

// ──────────────────────────────────────────────────────────────────────────────
// Test: accept-by-default when no schema is registered
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn crash_accepted_when_no_schema_registered() {
    let temp = TempDir::new().expect("temp dir");
    // Empty registry: no schema for any tenant.
    let (addr, shutdown, handle) = spawn_server(temp.path(), SchemaRegistry::empty()).await;

    let response = post_crash_raw(
        addr,
        json!({ "arbitrary": "content" }),
        Some("secret-token"),
    )
    .await;
    assert_eq!(
        response.status(),
        reqwest::StatusCode::CREATED,
        "crashes must be accepted when no schema is registered for the tenant"
    );

    shutdown.send(()).expect("shutdown");
    handle.await.expect("task").expect("server");
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

async fn post_crash_raw(
    addr: SocketAddr,
    context: serde_json::Value,
    token: Option<&str>,
) -> reqwest::Response {
    let metadata = CrashMetadata {
        schema_version: PROTOCOL_VERSION,
        source: SourceId {
            project: "detritus".to_owned(),
            platform: "linux".to_owned(),
            version: "0.1.0".to_owned(),
            install_id: Uuid::parse_str(INSTALL_ID).expect("uuid"),
        },
        timestamp: Utc::now(),
        kind: CrashKind::Minidump,
        build: BuildInfo {
            git_sha: "abc".to_owned(),
            profile: "release".to_owned(),
            target_triple: "x86_64-unknown-linux-gnu".to_owned(),
        },
        panic_text: None,
        context,
        attachments: Vec::new(),
    };
    let envelope = CrashEnvelope {
        metadata,
        dump: vec![0u8; 64],
        attachments: Vec::new(),
    };
    let mut body = Vec::new();
    envelope
        .write_to(&mut body)
        .await
        .expect("multipart encode");

    let builder = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("client")
        .post(format!("http://{addr}/v1/crashes"))
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={DEFAULT_BOUNDARY}"),
        );
    let builder = if let Some(t) = token {
        builder.header(AUTHORIZATION, format!("Bearer {t}"))
    } else {
        builder
    };
    builder.body(body).send().await.expect("post crash")
}

async fn spawn_server(
    data_dir: &Path,
    schema_registry: SchemaRegistry,
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
        schema_registry,
    };
    let handle = tokio::spawn(serve_with_shutdown(listener, config, async {
        let _ = shutdown_rx.await;
    }));
    (addr, shutdown_tx, handle)
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

/// Counts blob files in `blobs_dir`, returning 0 if the directory does not
/// exist (i.e. when the server never wrote any blob).
async fn count_blobs_if_exists(blobs_dir: &Path) -> usize {
    if !blobs_dir.exists() {
        return 0;
    }
    let Ok(mut prefixes) = tokio::fs::read_dir(blobs_dir).await else {
        return 0;
    };
    let mut count = 0;
    while let Some(prefix) = prefixes.next_entry().await.expect("prefix entry") {
        let mut blobs = tokio::fs::read_dir(prefix.path())
            .await
            .expect("prefix dir");
        while blobs.next_entry().await.expect("blob entry").is_some() {
            count += 1;
        }
    }
    count
}
