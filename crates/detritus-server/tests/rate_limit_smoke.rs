//! Rate limiting smoke tests.

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
    RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig, TestToken, TokenStore,
    serve_with_shutdown,
};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tonic::{Code, metadata::MetadataValue};

const INSTALL_ID: &str = "11111111-1111-1111-1111-111111111111";

#[tokio::test]
async fn rate_limit_smoke_rejects_excess_log_batches() {
    let temp = TempDir::new().expect("temp dir");
    let (addr, shutdown, handle) = spawn_server(temp.path()).await;
    let mut client = LogsServiceClient::connect(format!("http://{addr}"))
        .await
        .expect("connect logs client");

    client
        .export(request(0))
        .await
        .expect("first batch allowed");
    let error = client
        .export(request(1))
        .await
        .expect_err("second batch is rate limited");
    assert_eq!(error.code(), Code::ResourceExhausted);

    shutdown.send(()).expect("send shutdown");
    handle
        .await
        .expect("server task")
        .expect("server exits cleanly");
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
        rate_limit: RateLimitConfig {
            logs_per_minute: 1,
            logs_burst: 1,
            crashes_per_minute: 30,
            crashes_burst: 5,
        },
        retention: RetentionConfig::default(),
        schema_registry: SchemaRegistry::empty(),
    };
    let handle = tokio::spawn(serve_with_shutdown(listener, config, async {
        let _ = shutdown_rx.await;
    }));
    (addr, shutdown_tx, handle)
}

fn request(index: usize) -> tonic::Request<ExportLogsServiceRequest> {
    let mut request = tonic::Request::new(ExportLogsServiceRequest {
        resource_logs: vec![resource_logs(index)],
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
        MetadataValue::try_from("Bearer secret-token").expect("auth metadata"),
    );
    request
}

fn resource_logs(index: usize) -> ResourceLogs {
    ResourceLogs {
        resource: Some(Resource {
            attributes: vec![
                string_attr("source.project", "detritus"),
                string_attr("source.platform", "linux"),
                string_attr("source.version", "0.1.0"),
                string_attr("source.install_id", INSTALL_ID),
            ],
            dropped_attributes_count: 0,
            entity_refs: Vec::new(),
        }),
        scope_logs: vec![ScopeLogs {
            scope: None,
            log_records: vec![log_record(index)],
            schema_url: String::new(),
        }],
        schema_url: String::new(),
    }
}

fn log_record(index: usize) -> LogRecord {
    LogRecord {
        time_unix_nano: index as u64,
        observed_time_unix_nano: index as u64,
        severity_number: SeverityNumber::Info as i32,
        severity_text: "INFO".to_owned(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(format!("log {index}"))),
        }),
        attributes: Vec::new(),
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: Vec::new(),
        span_id: Vec::new(),
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

fn test_token_store() -> TokenStore {
    TokenStore::for_tests(vec![TestToken {
        id: "detritus-test".to_owned(),
        secret_hash: Argon2::default()
            .hash_password(
                b"secret-token",
                &SaltString::from_b64("c29tZXNhbHQ").expect("salt"),
            )
            .expect("hash token")
            .to_string(),
        project: "detritus".to_owned(),
        source_prefix: "detritus/".to_owned(),
    }])
}
