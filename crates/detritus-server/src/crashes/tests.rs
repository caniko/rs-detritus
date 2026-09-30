use super::*;
use crate::{
    auth::{TestToken, TokenStore},
    metrics::Metrics,
    rate_limit::{RateLimitConfig, RateLimiter},
    schemas::SchemaRegistry,
};

async fn setup(root: &std::path::Path) -> (AppState, TokenContext, Vec<u8>) {
    let storage = StoragePaths::new(root.to_owned());
    storage.prepare().await.unwrap();
    let token = TokenStore::for_tests(vec![TestToken {
        id: "test".into(),
        secret_hash: include_str!("../../tests/fixtures/legacy-token.phc")
            .trim()
            .into(),
        project: "tests".into(),
        source_prefix: "tests/".into(),
    }])
    .authenticate("secret-token")
    .unwrap();
    let metadata = CrashMetadata::new(
        detritus_protocol::SourceId {
            project: "tests".into(),
            platform: "linux".into(),
            version: "1".into(),
            install_id: Uuid::nil(),
        },
        chrono::Utc::now(),
        detritus_protocol::CrashKind::Minidump,
        detritus_protocol::BuildInfo {
            git_sha: "abc".into(),
            profile: "test".into(),
            target_triple: "test".into(),
        },
        json!({}),
    );
    (
        AppState {
            storage,
            max_dump_bytes: 100,
            rate_limiter: RateLimiter::new(RateLimitConfig {
                crashes_burst: 100,
                ..Default::default()
            }),
            metrics: Metrics::new().unwrap(),
            schema_registry: SchemaRegistry::empty(),
        },
        token,
        serde_json::to_vec(&metadata).unwrap(),
    )
}

fn body(parts: &[(Option<&str>, &[u8])]) -> Body {
    let mut bytes = Vec::new();
    for (name, value) in parts {
        bytes.extend_from_slice(b"--test\r\nContent-Disposition: form-data");
        if let Some(name) = name {
            bytes.extend_from_slice(format!("; name=\"{name}\"").as_bytes());
        }
        bytes.extend_from_slice(b"\r\n\r\n");
        bytes.extend_from_slice(value);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"--test--\r\n");
    Body::from(bytes)
}

fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "multipart/form-data; boundary=test".parse().unwrap(),
    );
    headers
}

#[tokio::test]
async fn malformed_parts_sizes_and_headers_return_structured_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (state, token, metadata) = setup(dir.path()).await;
    let oversize_metadata = vec![b' '; 65537];
    let oversize_dump = vec![0; 101];
    for (parts, status) in [
        (vec![], StatusCode::BAD_REQUEST),
        (
            vec![(Some("dump"), b"dump".as_slice())],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![(Some("metadata"), b"bad".as_slice())],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![(Some("metadata"), metadata.as_slice())],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![
                (Some("metadata"), metadata.as_slice()),
                (Some("metadata"), metadata.as_slice()),
            ],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![
                (Some("metadata"), metadata.as_slice()),
                (Some("dump"), b"dump"),
                (Some("dump"), b"dump"),
            ],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![
                (Some("metadata"), metadata.as_slice()),
                (Some("unknown"), b"dump"),
            ],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![(Some("metadata"), metadata.as_slice()), (None, b"dump")],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![(Some("metadata"), oversize_metadata.as_slice())],
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            vec![
                (Some("metadata"), metadata.as_slice()),
                (Some("dump"), oversize_dump.as_slice()),
            ],
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            vec![
                (Some("metadata"), metadata.as_slice()),
                (Some("attach:context"), oversize_dump.as_slice()),
            ],
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let error = crashes_handler(
            State(state.clone()),
            Extension(token.clone()),
            headers(),
            body(&parts),
        )
        .await
        .unwrap_err();
        let response = error.into_response();
        assert_eq!(response.status(), status);
        let value: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["error"]["code"], status.as_u16());
        assert!(value["error"]["message"].is_string());
    }
    for header in [
        None,
        Some("text/plain".parse().unwrap()),
        Some(axum::http::HeaderValue::from_bytes(&[255]).unwrap()),
    ] {
        let mut headers = HeaderMap::new();
        if let Some(header) = header {
            headers.insert(axum::http::header::CONTENT_TYPE, header);
        }
        assert_eq!(
            crashes_inner(&state, &token, headers, Body::empty())
                .await
                .unwrap_err()
                .status_code(),
            StatusCode::BAD_REQUEST
        );
    }
    assert!(
        fs::read_dir(state.storage.tmp_dir())
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn tenant_protocol_path_and_rate_limits_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, token, bytes) = setup(dir.path()).await;
    let original: CrashMetadata = serde_json::from_slice(&bytes).unwrap();
    for (project, version, status) in [
        ("tests", 999, StatusCode::PRECONDITION_FAILED),
        ("other", PROTOCOL_VERSION, StatusCode::FORBIDDEN),
        ("../escape", PROTOCOL_VERSION, StatusCode::BAD_REQUEST),
    ] {
        let mut metadata = original.clone();
        metadata.source.project = project.into();
        metadata.schema_version = version;
        let bytes = serde_json::to_vec(&metadata).unwrap();
        assert_eq!(
            crashes_inner(
                &state,
                &token,
                headers(),
                body(&[(Some("metadata"), &bytes), (Some("dump"), b"dump")])
            )
            .await
            .unwrap_err()
            .status_code(),
            status
        );
    }
    state.rate_limiter = RateLimiter::new(RateLimitConfig {
        crashes_burst: 0,
        ..Default::default()
    });
    assert_eq!(
        crashes_inner(
            &state,
            &token,
            headers(),
            body(&[(Some("metadata"), &bytes), (Some("dump"), b"dump")])
        )
        .await
        .unwrap_err()
        .status_code(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn concurrent_blob_finalization_has_one_winner_and_cleans_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let storage = StoragePaths::new(dir.path().to_owned());
    storage.prepare().await.unwrap();
    let hash = hex::encode(Sha256::digest(b"same bytes"));
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..16 {
        let path = storage.tmp_dir().join(format!("{index}.tmp"));
        fs::write(&path, b"same bytes").await.unwrap();
        let storage = storage.clone();
        let hash = hash.clone();
        tasks.spawn(async move { finalize_blob(&storage, path, &hash).await });
    }
    let mut winners = 0;
    while let Some(result) = tasks.join_next().await {
        winners += usize::from(!result.unwrap().unwrap().1);
    }
    assert_eq!(winners, 1);
    assert_eq!(
        fs::read(storage.blob_path(&hash)).await.unwrap(),
        b"same bytes"
    );
    assert!(
        fs::read_dir(storage.tmp_dir())
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap()
            .is_none()
    );
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, b"file").await.unwrap();
    let error = finalize_blob(
        &StoragePaths::new(blocked),
        dir.path().join("missing"),
        &hash,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
}
