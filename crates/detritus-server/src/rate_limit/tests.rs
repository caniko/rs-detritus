use super::*;
use crate::auth::{TestToken, TokenStore};

#[tokio::test]
async fn buckets_are_isolated_and_concurrent_requests_cannot_exceed_burst() {
    let store = TokenStore::for_tests(vec![TestToken {
        id: "first".into(),
        secret_hash: include_str!("../../tests/fixtures/legacy-token.phc")
            .trim()
            .into(),
        project: "tests".into(),
        source_prefix: "tests/".into(),
    }]);
    let token = store.authenticate("secret-token").unwrap();
    let source = SourceKey::new("tests".into(), "one".into()).unwrap();
    let limiter = RateLimiter::new(RateLimitConfig {
        logs_per_minute: 0,
        logs_burst: 2,
        crashes_per_minute: 0,
        crashes_burst: 1,
    });
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let (limiter, token, source) = (limiter.clone(), token.clone(), source.clone());
        tasks.spawn(async move { limiter.check_logs(&token, &source).await.is_ok() });
    }
    let mut allowed = 0;
    while let Some(result) = tasks.join_next().await {
        allowed += usize::from(result.unwrap());
    }
    assert_eq!(allowed, 2);
    assert!(limiter.check_crashes(&token, &source).await.is_ok());
    assert!(limiter.check_crashes(&token, &source).await.is_err());
    assert!(
        limiter
            .check_logs(
                &token,
                &SourceKey::new("tests".into(), "two".into()).unwrap()
            )
            .await
            .is_ok()
    );
    let mut other_token = token.clone();
    other_token.id = "second".into();
    assert!(limiter.check_logs(&other_token, &source).await.is_ok());
}

#[tokio::test]
async fn fractional_refill_accumulates_and_is_capped_at_burst() {
    let store = TokenStore::for_tests(vec![TestToken {
        id: "first".into(),
        secret_hash: include_str!("../../tests/fixtures/legacy-token.phc")
            .trim()
            .into(),
        project: "tests".into(),
        source_prefix: "tests/".into(),
    }]);
    let token = store.authenticate("secret-token").unwrap();
    let source = SourceKey::new("tests".into(), "one".into()).unwrap();
    let limiter = RateLimiter::new(RateLimitConfig {
        logs_per_minute: 1,
        logs_burst: 1,
        ..Default::default()
    });
    limiter.check_logs(&token, &source).await.unwrap();
    let key = RateKey {
        endpoint: "logs",
        token_id: token.id.clone(),
        source: source.canonical(),
    };
    limiter
        .buckets
        .lock()
        .await
        .get_mut(&key)
        .unwrap()
        .last_refill = Instant::now()
        .checked_sub(std::time::Duration::from_secs(24))
        .unwrap();
    assert!(limiter.check_logs(&token, &source).await.is_err());
    limiter
        .buckets
        .lock()
        .await
        .get_mut(&key)
        .unwrap()
        .last_refill = Instant::now()
        .checked_sub(std::time::Duration::from_secs(42))
        .unwrap();
    assert!(limiter.check_logs(&token, &source).await.is_ok());
    limiter
        .buckets
        .lock()
        .await
        .get_mut(&key)
        .unwrap()
        .last_refill = Instant::now()
        .checked_sub(std::time::Duration::from_secs(3600))
        .unwrap();
    assert!(limiter.check_logs(&token, &source).await.is_ok());
    assert!(limiter.check_logs(&token, &source).await.is_err());
    let disabled = RateLimiter::new(RateLimitConfig {
        logs_burst: 0,
        ..Default::default()
    });
    assert!(disabled.check_logs(&token, &source).await.is_err());
}
