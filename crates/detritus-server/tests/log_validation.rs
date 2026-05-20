//! Schema-validation integration tests for the OTLP logs Export RPC.

use std::{net::SocketAddr, path::Path};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use detritus_protocol::{
    GRPC_VERSION_KEY, PROTOCOL_VERSION,
    otlp::{
        common::{AnyValue, KeyValue, any_value},
        logs::{
            ExportLogsServiceRequest, LogRecord, LogsServiceClient, ResourceLogs, ScopeLogs,
            SeverityNumber,
        },
        resource::Resource,
    },
};
use detritus_server::{
    ProjectSchemaEntry, RateLimitConfig, RetentionConfig, SchemaKind, SchemaRegistry, ServerConfig,
    TestToken, TokenStore, serve_with_shutdown,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tonic::{Code, metadata::MetadataValue};

const INSTALL_ID: &str = "33333333-3333-3333-3333-333333333333";
const PROJECT: &str = "detritus";

#[tokio::test]
async fn export_ok_when_no_schema_registered() {
    let temp = TempDir::new().expect("temp dir");
    let (addr, shutdown, handle) = spawn_server(temp.path(), SchemaRegistry::empty()).await;

    let mut client = LogsServiceClient::connect(format!("http://{addr}"))
        .await
        .expect("connect");
    client
        .export(versioned_request(resource_logs(0, 3), "secret-token"))
        .await
        .expect("export should succeed when no schema is registered");

    shutdown.send(()).expect("shutdown");
    handle.await.expect("server task").expect("server ok");
}

#[tokio::test]
async fn export_ok_when_schema_satisfied() {
    let temp = TempDir::new().expect("temp dir");
    let schema_path = temp.path().join("log.schema.json");
    std::fs::write(&schema_path, permissive_schema().to_string()).expect("write schema");
    let registry = SchemaRegistry::load(&[ProjectSchemaEntry {
        project: PROJECT.to_owned(),
        kind: SchemaKind::LogAttributes,
        path: schema_path,
    }])
    .await
    .expect("load schema");

    let (addr, shutdown, handle) = spawn_server(temp.path(), registry).await;
    let mut client = LogsServiceClient::connect(format!("http://{addr}"))
        .await
        .expect("connect");
    client
        .export(versioned_request(resource_logs(0, 3), "secret-token"))
        .await
        .expect("export should succeed when payload satisfies registered schema");

    shutdown.send(()).expect("shutdown");
    handle.await.expect("server task").expect("server ok");
}

#[tokio::test]
async fn export_rejected_when_schema_violated_and_no_ndjson_written() {
    let temp = TempDir::new().expect("temp dir");
    let schema_path = temp.path().join("log.schema.json");
    std::fs::write(&schema_path, strict_schema().to_string()).expect("write schema");
    let registry = SchemaRegistry::load(&[ProjectSchemaEntry {
        project: PROJECT.to_owned(),
        kind: SchemaKind::LogAttributes,
        path: schema_path,
    }])
    .await
    .expect("load schema");

    let (addr, shutdown, handle) = spawn_server(temp.path(), registry).await;
    let mut client = LogsServiceClient::connect(format!("http://{addr}"))
        .await
        .expect("connect");

    // The strict schema requires resource.required_tag to be a string. Our
    // fixture resource attributes don't include it.
    let err = client
        .export(versioned_request(resource_logs(0, 1), "secret-token"))
        .await
        .expect_err("violating payload must be rejected");
    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("log attribute validation failed"),
        "error message should mention validation; got: {}",
        err.message()
    );

    let logs_dir = temp.path().join("logs");
    let count = ndjson_count(&logs_dir).await;
    assert_eq!(
        count, 0,
        "no NDJSON files should be written for a rejected batch"
    );

    shutdown.send(()).expect("shutdown");
    handle.await.expect("server task").expect("server ok");
}

fn permissive_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "resource": { "type": "object" },
            "scopes": { "type": "array" }
        }
    })
}

fn strict_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["resource"],
        "properties": {
            "resource": {
                "type": "object",
                "required": ["required_tag"],
                "properties": { "required_tag": { "type": "string" } }
            }
        }
    })
}

async fn ndjson_count(dir: &Path) -> usize {
    let mut count = 0;
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return 0;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            count += Box::pin(ndjson_count(&path)).await;
        } else if path.extension().and_then(|s| s.to_str()) == Some("ndjson") {
            count += 1;
        }
    }
    count
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
        project: PROJECT.to_owned(),
        source_prefix: format!("{PROJECT}/"),
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

fn resource_logs(start: usize, count: usize) -> ResourceLogs {
    ResourceLogs {
        resource: Some(Resource {
            attributes: vec![
                string_attr("source.project", PROJECT),
                string_attr("source.platform", "linux"),
                string_attr("source.version", "0.1.0"),
                string_attr("source.install_id", INSTALL_ID),
                string_attr("env", "test"),
            ],
            dropped_attributes_count: 0,
            entity_refs: Vec::new(),
        }),
        scope_logs: vec![ScopeLogs {
            scope: None,
            log_records: (start..start + count).map(log_record).collect(),
            schema_url: String::new(),
        }],
        schema_url: String::new(),
    }
}

fn versioned_request(
    resource_logs: ResourceLogs,
    token: &str,
) -> tonic::Request<ExportLogsServiceRequest> {
    let mut request = tonic::Request::new(ExportLogsServiceRequest {
        resource_logs: vec![resource_logs],
    });
    request.metadata_mut().insert(
        GRPC_VERSION_KEY,
        PROTOCOL_VERSION
            .to_string()
            .parse()
            .expect("metadata value"),
    );
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("auth metadata"),
    );
    request
}

fn log_record(index: usize) -> LogRecord {
    LogRecord {
        time_unix_nano: index as u64,
        observed_time_unix_nano: index as u64,
        severity_number: SeverityNumber::Info as i32,
        severity_text: "INFO".to_owned(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(format!(
                "validation test log {index}"
            ))),
        }),
        attributes: vec![string_attr("message", &format!("log {index}"))],
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: vec![3; 16],
        span_id: vec![4; 8],
        event_name: String::new(),
    }
}

fn string_attr(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.to_owned())),
        }),
    }
}
