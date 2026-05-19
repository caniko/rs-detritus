use std::{collections::HashMap, sync::Arc, time::Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{auth::TokenContext, storage::SourceKey};

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_logs_per_minute")]
    pub logs_per_minute: u32,
    #[serde(default = "default_logs_burst")]
    pub logs_burst: u32,
    #[serde(default = "default_crashes_per_minute")]
    pub crashes_per_minute: u32,
    #[serde(default = "default_crashes_burst")]
    pub crashes_burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            logs_per_minute: default_logs_per_minute(),
            logs_burst: default_logs_burst(),
            crashes_per_minute: default_crashes_per_minute(),
            crashes_burst: default_crashes_burst(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<RateKey, Bucket>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub async fn check_logs(
        &self,
        token: &TokenContext,
        source: &SourceKey,
    ) -> Result<(), RateLimitError> {
        self.check(
            "logs",
            token,
            source,
            self.config.logs_per_minute,
            self.config.logs_burst,
        )
        .await
    }

    pub async fn check_crashes(
        &self,
        token: &TokenContext,
        source: &SourceKey,
    ) -> Result<(), RateLimitError> {
        self.check(
            "crashes",
            token,
            source,
            self.config.crashes_per_minute,
            self.config.crashes_burst,
        )
        .await
    }

    async fn check(
        &self,
        endpoint: &'static str,
        token: &TokenContext,
        source: &SourceKey,
        per_minute: u32,
        burst: u32,
    ) -> Result<(), RateLimitError> {
        let mut buckets = self.buckets.lock().await;
        let key = RateKey {
            endpoint,
            token_id: token.id.clone(),
            source: source.canonical(),
        };
        let now = Instant::now();
        let bucket = buckets.entry(key).or_insert_with(|| Bucket {
            tokens: f64::from(burst),
            last_refill: now,
        });
        let refill_per_second = f64::from(per_minute) / 60.0;
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_per_second).min(f64::from(burst));
        bucket.last_refill = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            Err(RateLimitError)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RateKey {
    endpoint: &'static str,
    token_id: String,
    source: String,
}

#[derive(Debug, Clone)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("rate limit exceeded")]
pub struct RateLimitError;

fn default_logs_per_minute() -> u32 {
    1_000
}

fn default_logs_burst() -> u32 {
    200
}

fn default_crashes_per_minute() -> u32 {
    30
}

fn default_crashes_burst() -> u32 {
    5
}
