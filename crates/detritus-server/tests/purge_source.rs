//! Source purge HTTP and storage contract.

use std::{path::Path, time::Duration};

use detritus_protocol::{
    GRPC_VERSION_KEY, PROTOCOL_VERSION,
    otlp::{
        common::{AnyValue, KeyValue, any_value},
        logs::{ExportLogsServiceRequest, LogRecord, LogsServiceClient, ResourceLogs, ScopeLogs},
        resource::Resource,
    },
};
use detritus_server::{
    RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig, TokenStore, serve_with_shutdown,
};
use reqwest::{Response, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::{fs, net::TcpListener, sync::oneshot, task::JoinHandle};

const PROJECT: &str = "detritus";
const A: &str = "install-a";
const B: &str = "install-b";
const BEARER: &str = "secret-token";

#[tokio::test]
async fn purge_requires_bearer_capability_and_source_scope() {
    let ingest = TestServer::start(false, "detritus/").await;
    let ingest_log = ingest
        .root
        .path()
        .join(format!("logs/{PROJECT}/{A}/kept.ndjson"));
    write(&ingest_log, b"kept\n").await;
    assert_eq!(
        ingest.purge(PROJECT, A, Some(BEARER)).await.status(),
        StatusCode::FORBIDDEN
    );
    let admin = TestServer::start(true, "detritus/install-").await;
    let admin_log = admin
        .root
        .path()
        .join(format!("logs/{PROJECT}/{A}/kept.ndjson"));
    let outside_log = admin
        .root
        .path()
        .join(format!("logs/{PROJECT}/outside-prefix/kept.ndjson"));
    write(&admin_log, b"kept\n").await;
    write(&outside_log, b"kept\n").await;
    for token in [None, Some("wrong-token")] {
        assert_eq!(
            admin.purge(PROJECT, A, token).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    for (project, source) in [("foreign", A), (PROJECT, "outside-prefix")] {
        assert_eq!(
            admin.purge(project, source, Some(BEARER)).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    let too_long = format!("install-{}", "é".repeat(124));
    for bad in ["..", ".", "/", "\\", "", too_long.as_str()] {
        for (project, source) in [(PROJECT, bad), (bad, A)] {
            assert_eq!(
                admin.purge(project, source, Some(BEARER)).await.status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
    for body in ["{", r#"{"project":"detritus"}"#] {
        for (server, status) in [
            (&admin, StatusCode::BAD_REQUEST),
            (&ingest, StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                server
                    .client
                    .post(format!("{}/v1/admin/purge-source", server.url))
                    .bearer_auth(BEARER)
                    .header("content-type", "application/json")
                    .body(body)
                    .send()
                    .await
                    .expect("purge request")
                    .status(),
                status
            );
        }
    }
    for log in [ingest_log, admin_log, outside_log] {
        assert_eq!(fs::read(log).await.unwrap(), b"kept\n");
    }
    ingest.stop().await;
    admin.stop().await;
}

#[tokio::test]
async fn purge_counts_only_target_files_and_preserves_shared_blobs() {
    let server = TestServer::start(true, "detritus/").await;
    let root = server.root.path();
    let [
        shared_dump,
        shared_attachment,
        private_dump,
        private_attachment,
        orphan,
    ] = ["11", "22", "33", "44", "55"].map(|byte| byte.repeat(32));
    let (shared_dump, shared_attachment, private_dump, private_attachment) = (
        shared_dump.as_str(),
        shared_attachment.as_str(),
        private_dump.as_str(),
        private_attachment.as_str(),
    );
    let blob = |hash: &str| root.join(format!("crashes/by-hash/{}/{hash}.bin", &hash[..2]));
    let indexes = |source| root.join(format!("crashes/by-source/{PROJECT}/{source}"));
    let logs = |source| root.join(format!("logs/{PROJECT}/{source}"));
    let index = |dump: &str, attachment: &str| {
        serde_json::to_vec(&json!({
            "dump": {"sha256": dump},
            "attachments": [{"sha256": attachment}],
        }))
        .expect("index JSON")
    };
    let kept_index = index(shared_dump, shared_attachment);
    let first = index(shared_dump, private_attachment);
    let second = index(private_dump, shared_attachment);
    write(&indexes(B).join("kept.json"), &kept_index).await;
    write(&logs(B).join("2026-05-20.ndjson"), b"kept\n").await;
    write(&indexes(A).join("one.json"), &first).await;
    write(&indexes(A).join("two.json"), &second).await;
    write(&logs(A).join("2026-05-19.ndjson"), b"one\n").await;
    write(&logs(A).join("2026-05-20.ndjson"), b"two\n").await;
    for hash in [
        shared_dump,
        shared_attachment,
        private_dump,
        private_attachment,
    ] {
        write(&blob(hash), hash.as_bytes()).await;
    }
    write(&blob(&orphan), b"orphan").await;

    let bytes = first.len() + second.len() + 8 + private_dump.len() + private_attachment.len();
    assert_eq!(
        server.purge_ok(A).await,
        json!({"logs_deleted": 2, "indexes_deleted": 2, "blobs_deleted": 2, "bytes_freed": bytes})
    );
    for deleted in [
        logs(A),
        indexes(A),
        blob(private_dump),
        blob(private_attachment),
    ] {
        assert!(!deleted.exists(), "still present: {}", deleted.display());
    }
    assert_eq!(
        fs::read(indexes(B).join("kept.json")).await.unwrap(),
        kept_index
    );
    assert_eq!(
        fs::read(logs(B).join("2026-05-20.ndjson")).await.unwrap(),
        b"kept\n"
    );
    for hash in [shared_dump, shared_attachment] {
        assert_eq!(fs::read(blob(hash)).await.unwrap(), hash.as_bytes());
    }

    let zeros =
        json!({"logs_deleted": 0, "indexes_deleted": 0, "blobs_deleted": 0, "bytes_freed": 0});
    assert_eq!(server.purge_ok(A).await, zeros);
    assert_eq!(server.purge_ok("unknown").await, zeros);
    assert_eq!(fs::read(blob(&orphan)).await.unwrap(), b"orphan");
    let metrics = server.metrics().await;
    assert!(metrics.contains("detritus_purge_total{project=\"detritus\",result=\"ok\"} 3"));
    assert!(
        metrics.contains("detritus_requests_total{endpoint=\"purge_source\",status=\"200\"} 3")
    );
    server.stop().await;
}

#[tokio::test]
async fn purge_can_retry_after_a_corrupt_surviving_index_is_repaired() {
    let server = TestServer::start(true, "detritus/").await;
    let root = server.root.path();
    let hash = "00".repeat(32);
    let blob = root.join(format!("crashes/by-hash/00/{hash}.bin"));
    let target = root.join(format!("crashes/by-source/{PROJECT}/{A}/one.json"));
    let log = root.join(format!("logs/{PROJECT}/{A}/one.ndjson"));
    let corrupt = root.join(format!("crashes/by-source/another-project/{B}/broken.json"));
    write(&blob, b"possibly referenced").await;
    write(
        &target,
        json!({"dump":{"sha256":hash}}).to_string().as_bytes(),
    )
    .await;
    write(&corrupt, b"{").await;
    write(&log, b"one\n").await;
    let logs_only = root.join(format!("logs/{PROJECT}/{B}/two.ndjson"));
    write(&logs_only, b"two\n").await;
    assert_eq!(
        server.purge_ok(B).await,
        json!({"logs_deleted": 1, "indexes_deleted": 0, "blobs_deleted": 0, "bytes_freed": 4})
    );
    assert_eq!(
        server.purge(PROJECT, A, Some(BEARER)).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(target.exists());
    assert!(log.exists());
    assert!(blob.exists());
    assert!(
        server
            .metrics()
            .await
            .contains("detritus_purge_total{project=\"detritus\",result=\"error\"} 1")
    );
    fs::remove_file(corrupt).await.unwrap();
    let stats = server.purge_ok(A).await;
    assert_eq!(stats["indexes_deleted"], 1);
    assert_eq!(stats["blobs_deleted"], 1);
    assert!(!target.exists());
    assert!(!blob.exists());
    server.stop().await;
}

#[tokio::test]
async fn purge_removes_a_crash_uploaded_over_http_with_an_attachment() {
    const SOURCE: &str = "11111111-1111-1111-1111-111111111111";
    let server = TestServer::start(true, "detritus/").await;
    let root = server.root.path();
    fs::remove_dir(root.join("tmp")).await.unwrap();
    let metadata = json!({
        "schema_version": PROTOCOL_VERSION,
        "source": {"project": PROJECT, "platform": "linux", "version": "0.1.0", "install_id": SOURCE},
        "timestamp": "2026-05-19T00:00:00Z",
        "kind": "PanicTarball",
        "build": {"git_sha": "test", "profile": "dev", "target_triple": "x86_64-unknown-linux-gnu"},
        "context": null,
        "attachments": [{"key": "log", "content_type": "application/octet-stream", "len": 10}],
    });
    let body = format!(
        "--upload\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{metadata}\r\n\
         --upload\r\nContent-Disposition: form-data; name=\"dump\"\r\n\r\ndump\r\n\
         --upload\r\nContent-Disposition: form-data; name=\"attach:log\"\r\n\r\nattachment\r\n\
         --upload--\r\n"
    );
    let response = server
        .client
        .post(format!("{}/v1/crashes", server.url))
        .bearer_auth(BEARER)
        .header("content-type", "multipart/form-data; boundary=upload")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "{}",
        response.text().await.unwrap()
    );
    assert!(
        fs::read_dir(root.join("tmp"))
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap()
            .is_none()
    );
    let indexes = root.join(format!("crashes/by-source/{PROJECT}/{SOURCE}"));
    let index = fs::read_dir(&indexes)
        .await
        .unwrap()
        .next_entry()
        .await
        .unwrap()
        .unwrap();
    let index_bytes = index.metadata().await.unwrap().len();
    let blobs = [b"dump".as_slice(), b"attachment"].map(|bytes| {
        let hash = hex::encode(Sha256::digest(bytes));
        let path = root.join(format!("crashes/by-hash/{}/{hash}.bin", &hash[..2]));
        assert!(path.exists());
        path
    });
    assert_eq!(
        server.purge_ok(SOURCE).await,
        json!({"logs_deleted": 0, "indexes_deleted": 1, "blobs_deleted": 2, "bytes_freed": index_bytes + 14})
    );
    assert!(!indexes.exists());
    assert!(blobs.iter().all(|path| !path.exists()));
    server.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn purge_refuses_a_symlinked_project_directory() {
    let server = TestServer::start(true, "detritus/").await;
    let outside = TempDir::new().unwrap();
    let kept = outside.path().join(format!("{A}/kept.ndjson"));
    write(&kept, b"outside\n").await;
    fs::symlink(
        outside.path(),
        server.root.path().join(format!("logs/{PROJECT}")),
    )
    .await
    .unwrap();
    assert_eq!(
        server.purge(PROJECT, A, Some(BEARER)).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(fs::read(&kept).await.unwrap(), b"outside\n");
    server.stop().await;
}

#[tokio::test]
async fn purge_closes_the_writer_and_audits_before_it_reopens() {
    const SOURCE: &str = "audit-source";
    let audit = tempfile::NamedTempFile::new().expect("audit file");
    let writer = audit.reopen().expect("audit writer");
    tracing_subscriber::fmt()
        .json()
        .with_writer(writer)
        .try_init()
        .expect("audit subscriber");
    let server = TestServer::start(true, "detritus/").await;
    let logs = server.root.path().join(format!("logs/{PROJECT}/{SOURCE}"));
    let mut client = LogsServiceClient::connect(server.url.clone())
        .await
        .expect("OTLP connect");
    client
        .export(log_request(SOURCE, 1))
        .await
        .expect("first export");
    assert!(
        server
            .metrics()
            .await
            .contains(&format!("source_id=\"{PROJECT}/{SOURCE}\""))
    );
    client
        .export(log_request(SOURCE, 1))
        .await
        .expect("second export");
    assert_eq!(
        client
            .export(log_request(SOURCE, 1))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::ResourceExhausted
    );
    let stats = server.purge_ok(SOURCE).await;
    assert!(!server.metrics().await.contains(SOURCE));
    assert_eq!(stats["logs_deleted"], 1);
    assert!(stats["bytes_freed"].as_u64().unwrap() > 0);
    assert!(!logs.exists());
    let events = fs::read_to_string(audit.path()).await.unwrap();
    let events = events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|entry| entry["fields"]["source_id"] == SOURCE)
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["target"], "detritus::audit");
    assert_eq!(
        events[0]["fields"],
        json!({
            "event": "purge_source",
            "token_id": "purge-test",
            "project": PROJECT,
            "source_id": SOURCE,
            "logs_deleted": 1,
            "indexes_deleted": 0,
            "blobs_deleted": 0,
            "bytes_freed": stats["bytes_freed"],
            "result": "ok",
        })
    );

    client
        .export(log_request(SOURCE, 2))
        .await
        .expect("reopened source gets a fresh rate-limit bucket");
    drop(client);
    // Keep the directory alive after graceful shutdown flushes the reopened writer.
    let TestServer {
        root,
        shutdown,
        task,
        ..
    } = server;
    shutdown.send(()).unwrap();
    task.await.unwrap().unwrap();
    let mut entries = fs::read_dir(&logs).await.expect("reopened log directory");
    let file = entries
        .next_entry()
        .await
        .unwrap()
        .expect("reopened log file");
    let data = fs::read_to_string(file.path()).await.unwrap();
    let line: Value = serde_json::from_str(&data).expect("exactly one NDJSON record");
    assert_eq!(line["time_unix_nano"], 2);
    assert!(entries.next_entry().await.unwrap().is_none());
    drop(root);
}

fn log_request(source: &str, time_unix_nano: u64) -> tonic::Request<ExportLogsServiceRequest> {
    let attributes = [
        ("source.project", PROJECT),
        ("source.platform", "linux"),
        ("source.version", "0.1.0"),
        ("source.install_id", source),
    ]
    .into_iter()
    .map(|(key, value)| KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.to_owned())),
        }),
    })
    .collect();
    let mut request = tonic::Request::new(ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes,
                ..Resource::default()
            }),
            scope_logs: vec![ScopeLogs {
                log_records: vec![LogRecord {
                    time_unix_nano,
                    ..LogRecord::default()
                }],
                ..ScopeLogs::default()
            }],
            ..ResourceLogs::default()
        }],
    });
    request.metadata_mut().insert(
        GRPC_VERSION_KEY,
        PROTOCOL_VERSION.to_string().parse().unwrap(),
    );
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {BEARER}").parse().unwrap());
    request
}

struct TestServer {
    root: TempDir,
    url: String,
    client: reqwest::Client,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<Result<(), detritus_server::ServerError>>,
}

impl TestServer {
    async fn start(can_purge: bool, source_prefix: &str) -> Self {
        static CRYPTO: std::sync::Once = std::sync::Once::new();
        CRYPTO.call_once(|| {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        });
        let root = TempDir::new().expect("storage");
        let tokens = root.path().join("tokens.toml");
        let hash = include_str!("fixtures/legacy-token.phc").trim();
        let capability = if can_purge { "can_purge = true" } else { "" };
        let config = format!(
            "[[token]]\nid = 'purge-test'\nsecret = '{hash}'\nproject = '{PROJECT}'\nsource_prefix = '{source_prefix}'\n{capability}\n",
        );
        write(&tokens, config.as_bytes()).await;
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let bind = listener.local_addr().expect("address");
        let (shutdown, rx) = oneshot::channel();
        let task = tokio::spawn(serve_with_shutdown(
            listener,
            ServerConfig {
                bind,
                data_dir: root.path().to_path_buf(),
                max_dump_bytes: 1024 * 1024,
                token_store: TokenStore::load(&tokens).await.expect("tokens"),
                rate_limit: RateLimitConfig {
                    logs_burst: 2,
                    logs_per_minute: 0,
                    ..RateLimitConfig::default()
                },
                retention: RetentionConfig::default(),
                schema_registry: SchemaRegistry::empty(),
            },
            async {
                let _ = rx.await;
            },
        ));
        let server = Self {
            root,
            url: format!("http://{bind}"),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("client"),
            shutdown,
            task,
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            // Wait for the initial janitor pass before populating fixtures.
            loop {
                if server
                    .metrics()
                    .await
                    .contains("detritus_janitor_cycles_total 1")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("initial janitor pass");
        server
    }

    async fn purge(&self, project: &str, install_id: &str, token: Option<&str>) -> Response {
        let request = self
            .client
            .post(format!("{}/v1/admin/purge-source", self.url))
            .json(&json!({"project": project, "install_id": install_id}));
        let request = match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        request.send().await.expect("purge request")
    }

    async fn purge_ok(&self, install_id: &str) -> Value {
        let response = self.purge(PROJECT, install_id, Some(BEARER)).await;
        assert_eq!(response.status(), StatusCode::OK);
        response.json().await.expect("purge response")
    }

    async fn metrics(&self) -> String {
        self.client
            .get(format!("{}/metrics", self.url))
            .send()
            .await
            .expect("metrics")
            .text()
            .await
            .expect("metrics text")
    }

    async fn stop(self) {
        self.shutdown.send(()).expect("send shutdown");
        self.task
            .await
            .expect("server task")
            .expect("server shutdown");
    }
}

async fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("parent"))
        .await
        .expect("mkdir");
    fs::write(path, bytes).await.expect("write");
}
