//! Log ingestion smoke tests.

use std::{net::SocketAddr, path::Path};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use chrono::Utc;
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
    RateLimitConfig, RetentionConfig, ServerConfig, TestToken, TokenStore, serve_with_shutdown,
};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tonic::{Code, metadata::MetadataValue};

const INSTALL_ID: &str = "11111111-1111-1111-1111-111111111111";

#[tokio::test]
async fn logs_smoke_writes_one_ndjson_line_per_record() {
    let temp = TempDir::new().expect("temp dir");
    let (addr, shutdown, handle) = spawn_server(temp.path()).await;

    let mut client = LogsServiceClient::connect(format!("http://{addr}"))
        .await
        .expect("connect logs client");
    let no_auth = client
        .export(versioned_request(resource_logs(9_000, 1), None))
        .await
        .expect_err("missing auth rejected");
    assert!(matches!(
        no_auth.code(),
        Code::Unauthenticated | Code::Unknown | Code::Internal
    ));

    let wrong_project = client
        .export(versioned_request(
            resource_logs_for_project("rs-modde", 9_001, 1),
            Some("secret-token"),
        ))
        .await
        .expect_err("wrong project rejected");
    assert_eq!(wrong_project.code(), Code::PermissionDenied);

    let mut request = tonic::Request::new(ExportLogsServiceRequest {
        resource_logs: vec![resource_logs(0, 1_000)],
    });
    request.metadata_mut().insert(
        GRPC_VERSION_KEY,
        PROTOCOL_VERSION
            .to_string()
            .parse()
            .expect("metadata value"),
    );
    add_auth(&mut request, "secret-token");
    client.export(request).await.expect("export logs");

    let mut second = tonic::Request::new(ExportLogsServiceRequest {
        resource_logs: vec![resource_logs(1_000, 1)],
    });
    second.metadata_mut().insert(
        GRPC_VERSION_KEY,
        PROTOCOL_VERSION
            .to_string()
            .parse()
            .expect("metadata value"),
    );
    add_auth(&mut second, "secret-token");
    client.export(second).await.expect("second export appends");

    let mut mismatch = tonic::Request::new(ExportLogsServiceRequest {
        resource_logs: vec![resource_logs(2_000, 1)],
    });
    mismatch.metadata_mut().insert(
        GRPC_VERSION_KEY,
        (PROTOCOL_VERSION + 1)
            .to_string()
            .parse()
            .expect("metadata value"),
    );
    add_auth(&mut mismatch, "secret-token");
    let error = client
        .export(mismatch)
        .await
        .expect_err("version mismatch is rejected");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    assert!(error.message().contains("does not match"));

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
    assert_eq!(lines.len(), 1_001);
    assert!(lines.iter().any(|line| line.contains("log 0")));
    assert!(lines.iter().any(|line| line.contains("log 1000")));
    for line in lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        assert!(parsed.get("severity_text").is_some());
    }
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
    };
    let handle = tokio::spawn(serve_with_shutdown(listener, config, async {
        let _ = shutdown_rx.await;
    }));
    (addr, shutdown_tx, handle)
}

fn resource_logs(start: usize, count: usize) -> ResourceLogs {
    resource_logs_for_project("detritus", start, count)
}

fn resource_logs_for_project(project: &str, start: usize, count: usize) -> ResourceLogs {
    ResourceLogs {
        resource: Some(Resource {
            attributes: vec![
                string_attr("source.project", project),
                string_attr("source.platform", "linux"),
                string_attr("source.version", "0.1.0"),
                string_attr("source.install_id", INSTALL_ID),
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
    token: Option<&str>,
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
    if let Some(token) = token {
        add_auth(&mut request, token);
    }
    request
}

fn add_auth(request: &mut tonic::Request<ExportLogsServiceRequest>, token: &str) {
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("auth metadata"),
    );
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

fn log_record(index: usize) -> LogRecord {
    LogRecord {
        time_unix_nano: index as u64,
        observed_time_unix_nano: index as u64,
        severity_number: SeverityNumber::Info as i32,
        severity_text: "INFO".to_owned(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(format!("log {index}"))),
        }),
        attributes: vec![string_attr("message", &format!("log {index}"))],
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: vec![1; 16],
        span_id: vec![2; 8],
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
