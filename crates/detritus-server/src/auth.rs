use std::{path::Path, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::fs;

use crate::{metrics::Metrics, rate_limit::RateLimitConfig, storage::SourceKey};

#[derive(Debug, Clone)]
pub struct TokenContext {
    pub id: String,
    pub project: String,
    pub source_prefix: String,
    secret_hash: String,
}

impl TokenContext {
    pub fn permits(&self, source: &SourceKey) -> bool {
        source
            .project
            .as_bytes()
            .ct_eq(self.project.as_bytes())
            .into()
            && source.canonical().starts_with(&self.source_prefix)
    }

    fn verify(&self, presented: &str) -> bool {
        let Ok(hash) = PasswordHash::new(&self.secret_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(presented.as_bytes(), &hash)
            .is_ok()
    }
}

#[derive(Debug, Clone)]
pub struct TokenStore {
    tokens: Arc<Vec<TokenContext>>,
}

impl TokenStore {
    pub async fn load(path: &Path) -> Result<Self, AuthConfigError> {
        let raw = fs::read_to_string(path).await?;
        let config: TokensConfig = toml::from_str(&raw)?;
        let mut tokens = Vec::with_capacity(config.token.len());
        for token in config.token {
            PasswordHash::new(&token.secret).map_err(|error| AuthConfigError::InvalidHash {
                id: token.id.clone(),
                message: error.to_string(),
            })?;
            tokens.push(TokenContext {
                id: token.id,
                secret_hash: token.secret,
                project: token.project,
                source_prefix: token.source_prefix,
            });
        }
        if tokens.is_empty() {
            return Err(AuthConfigError::NoTokens);
        }
        Ok(Self {
            tokens: Arc::new(tokens),
        })
    }

    pub fn for_tests(tokens: Vec<TestToken>) -> Self {
        Self {
            tokens: Arc::new(
                tokens
                    .into_iter()
                    .map(|token| TokenContext {
                        id: token.id,
                        secret_hash: token.secret_hash,
                        project: token.project,
                        source_prefix: token.source_prefix,
                    })
                    .collect(),
            ),
        }
    }

    pub fn authenticate(&self, presented: &str) -> Option<TokenContext> {
        self.tokens
            .iter()
            .find(|token| token.verify(presented))
            .cloned()
    }
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub token_store: TokenStore,
    pub rate_limit: RateLimitConfig,
}

pub async fn load_security_config(path: &Path) -> Result<SecurityConfig, AuthConfigError> {
    let raw = fs::read_to_string(path).await?;
    let config: TokensConfig = toml::from_str(&raw)?;
    let token_store = TokenStore::from_entries(config.token)?;
    Ok(SecurityConfig {
        token_store,
        rate_limit: config.rate_limit.unwrap_or_default(),
    })
}

#[derive(Debug, Clone)]
pub struct TestToken {
    pub id: String,
    pub secret_hash: String,
    pub project: String,
    pub source_prefix: String,
}

#[derive(Debug, Deserialize)]
struct TokensConfig {
    #[serde(default)]
    token: Vec<TokenEntry>,
    rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Deserialize)]
struct TokenEntry {
    id: String,
    secret: String,
    project: String,
    source_prefix: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthConfigError {
    #[error("token config I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("token config TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("token config contains no tokens")]
    NoTokens,
    #[error("token `{id}` has an invalid Argon2 hash: {message}")]
    InvalidHash { id: String, message: String },
}

impl TokenStore {
    fn from_entries(entries: Vec<TokenEntry>) -> Result<Self, AuthConfigError> {
        let mut tokens = Vec::with_capacity(entries.len());
        for token in entries {
            PasswordHash::new(&token.secret).map_err(|error| AuthConfigError::InvalidHash {
                id: token.id.clone(),
                message: error.to_string(),
            })?;
            tokens.push(TokenContext {
                id: token.id,
                secret_hash: token.secret,
                project: token.project,
                source_prefix: token.source_prefix,
            });
        }
        if tokens.is_empty() {
            return Err(AuthConfigError::NoTokens);
        }
        Ok(Self {
            tokens: Arc::new(tokens),
        })
    }
}

#[derive(Clone)]
pub struct AuthState {
    pub token_store: TokenStore,
    pub metrics: Metrics,
}

pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if path == "/healthz" || path == "/metrics" {
        return next.run(request).await;
    }

    let endpoint = endpoint_label(path);
    let started = std::time::Instant::now();
    let Some(header_value) = request.headers().get(header::AUTHORIZATION) else {
        state
            .metrics
            .observe_request(endpoint, "401", started.elapsed());
        return auth_error(StatusCode::UNAUTHORIZED, "missing bearer token");
    };
    let Ok(header_value) = header_value.to_str() else {
        state
            .metrics
            .observe_request(endpoint, "401", started.elapsed());
        return auth_error(
            StatusCode::UNAUTHORIZED,
            "authorization header is not UTF-8",
        );
    };
    let Some(token) = header_value.strip_prefix("Bearer ") else {
        state
            .metrics
            .observe_request(endpoint, "401", started.elapsed());
        return auth_error(
            StatusCode::UNAUTHORIZED,
            "authorization header must use Bearer",
        );
    };
    let Some(context) = state.token_store.authenticate(token) else {
        state
            .metrics
            .observe_request(endpoint, "401", started.elapsed());
        return auth_error(StatusCode::UNAUTHORIZED, "invalid bearer token");
    };

    request.extensions_mut().insert(context);
    next.run(request).await
}

pub fn token_from_extensions(
    extensions: &axum::http::Extensions,
) -> Result<TokenContext, tonic::Status> {
    extensions
        .get::<TokenContext>()
        .cloned()
        .ok_or_else(|| tonic::Status::unauthenticated("missing token context"))
}

pub fn endpoint_label(path: &str) -> &'static str {
    if path == "/v1/crashes" {
        "crashes"
    } else if path == "/opentelemetry.proto.collector.logs.v1.LogsService/Export" {
        "logs"
    } else if path == "/metrics" {
        "metrics"
    } else if path == "/healthz" {
        "healthz"
    } else {
        "other"
    }
}

fn auth_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": message,
            }
        })),
    )
        .into_response()
}
