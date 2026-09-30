use super::*;

const LEGACY_HASH: &str = include_str!("../../tests/fixtures/legacy-token.phc");

fn entry(hash: &str) -> TestToken {
    TestToken {
        id: "test".into(),
        secret_hash: hash.trim().into(),
        project: "tests".into(),
        source_prefix: "tests/allowed".into(),
    }
}

fn token_config(hash: &str) -> String {
    format!(
        "[[token]]\nid = 'test'\nsecret = '{}'\nproject = 'tests'\nsource_prefix = 'tests/allowed'\n",
        hash.trim()
    )
}

#[test]
fn legacy_hash_authenticates_only_correct_secret_and_debug_redacts_hash() {
    let test_token = entry(LEGACY_HASH);
    assert!(!format!("{test_token:?}").contains(LEGACY_HASH.trim()));
    let store = TokenStore::for_tests(vec![entry("invalid hash"), test_token]);
    let token = store
        .authenticate("secret-token")
        .expect("Argon2 0.5 hash remains valid");
    assert!(store.authenticate("wrong-token").is_none());
    assert!(!format!("{store:?}").contains(LEGACY_HASH.trim()));
    assert!(!format!("{token:?}").contains(LEGACY_HASH.trim()));
    assert!(token.permits(&SourceKey::new("tests".into(), "allowed-install".into()).unwrap()));
    assert!(!token.permits(&SourceKey::new("tests".into(), "other".into()).unwrap()));
    assert!(!token.permits(&SourceKey::new("other".into(), "allowed-install".into()).unwrap()));
    let mut extensions = axum::http::Extensions::new();
    assert_eq!(
        token_from_extensions(&extensions).unwrap_err().code(),
        tonic::Code::Unauthenticated
    );
    extensions.insert(token);
    assert_eq!(token_from_extensions(&extensions).unwrap().id, "test");
}

#[tokio::test]
async fn token_loaders_reject_missing_malformed_empty_and_invalid_hash_configs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tokens.toml");
    assert!(matches!(
        TokenStore::load(&path).await,
        Err(AuthConfigError::Io(_))
    ));
    assert!(matches!(
        load_security_config(&path).await,
        Err(AuthConfigError::Io(_))
    ));
    for (text, expected) in [
        ("bad[", "toml"),
        ("", "empty"),
        (&token_config("not-phc"), "hash"),
    ] {
        fs::write(&path, text).await.unwrap();
        for error in [
            TokenStore::load(&path).await.unwrap_err(),
            load_security_config(&path).await.unwrap_err(),
        ] {
            assert!(match (expected, error) {
                ("toml", AuthConfigError::Toml(_)) | ("empty", AuthConfigError::NoTokens) => true,
                ("hash", AuthConfigError::InvalidHash { id, .. }) => id == "test",
                _ => false,
            });
        }
    }
    fs::write(&path, token_config(LEGACY_HASH)).await.unwrap();
    assert!(
        TokenStore::load(&path)
            .await
            .unwrap()
            .authenticate("secret-token")
            .is_some()
    );
    let text = format!(
        "{}\n[rate_limit]\nlogs_per_minute = 60\nlogs_burst = 2\n",
        token_config(LEGACY_HASH)
    );
    fs::write(&path, text).await.unwrap();
    let security = load_security_config(&path).await.unwrap();
    assert_eq!(security.rate_limit.logs_per_minute, 60);
    assert_eq!(security.rate_limit.logs_burst, 2);
    assert_eq!(
        security.rate_limit.crashes_burst,
        RateLimitConfig::default().crashes_burst
    );
}

#[tokio::test]
async fn middleware_rejects_bad_headers_and_inserts_context_on_success() {
    use axum::{Extension, Router, http::HeaderValue, middleware, routing::get};
    use tower::ServiceExt;
    let app = Router::new()
        .route(
            "/private",
            get(|Extension(token): axum::Extension<TokenContext>| async move { token.id }),
        )
        .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
        .route("/metrics", get(|| async { "metrics" }))
        .layer(middleware::from_fn_with_state(
            AuthState {
                token_store: TokenStore::for_tests(vec![entry(LEGACY_HASH)]),
                metrics: Metrics::new().unwrap(),
            },
            auth_middleware,
        ));
    for (header, status, message) in [
        (None, StatusCode::UNAUTHORIZED, "missing bearer token"),
        (
            Some(HeaderValue::from_static("Basic token")),
            StatusCode::UNAUTHORIZED,
            "must use Bearer",
        ),
        (
            Some(HeaderValue::from_bytes(&[0xff]).unwrap()),
            StatusCode::UNAUTHORIZED,
            "not UTF-8",
        ),
        (
            Some(HeaderValue::from_static("Bearer bad")),
            StatusCode::UNAUTHORIZED,
            "invalid bearer token",
        ),
        (
            Some(HeaderValue::from_static("Bearer secret-token")),
            StatusCode::OK,
            "test",
        ),
    ] {
        let mut request = Request::builder()
            .uri("/private")
            .body(Body::empty())
            .unwrap();
        if let Some(header) = header {
            request.headers_mut().insert(header::AUTHORIZATION, header);
        }
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&bytes).unwrap().contains(message));
    }
    for path in ["/healthz", "/metrics"] {
        assert!(
            app.clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }
}
