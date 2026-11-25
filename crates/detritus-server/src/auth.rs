use std::{path::Path, sync::Arc};

use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use detritus_protocol::schema::{SchemaError, SchemaKind};
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::fs;

use crate::{
    janitor::JanitorStats,
    metrics::Metrics,
    purge::{PURGE_SOURCE_PATH, record_purge},
    rate_limit::RateLimitConfig,
    schemas::{ProjectSchemaEntry, SchemaOptions, SchemaRegistry},
    storage::SourceKey,
};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct TokenContext {
    pub(crate) id: String,
    pub(crate) project: String,
    pub(crate) source_prefix: String,
    pub(crate) can_purge: bool,
    secret_hash: String,
}

// Manual Debug so the Argon2 secret hash never reaches logs or panic output
// (the store is reachable via TokenStore/ServerConfig Debug). C-DEBUG.
impl std::fmt::Debug for TokenContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenContext")
            .field("id", &self.id)
            .field("project", &self.project)
            .field("source_prefix", &self.source_prefix)
            .field("can_purge", &self.can_purge)
            .field("secret_hash", &"<redacted>")
            .finish()
    }
}

impl TokenContext {
    pub(crate) fn permits(&self, source: &SourceKey) -> bool {
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

/// In-memory bearer-token store used by the server.
#[derive(Debug, Clone)]
pub struct TokenStore {
    tokens: Arc<Vec<TokenContext>>,
}

impl TokenStore {
    /// Loads a token store from a TOML token configuration file.
    ///
    /// # Errors
    ///
    /// Returns [`AuthConfigError::Io`] if the file cannot be read,
    /// [`AuthConfigError::Toml`] if it is not valid TOML,
    /// [`AuthConfigError::InvalidHash`] if a token's Argon2 hash is malformed,
    /// or [`AuthConfigError::NoTokens`] if the file declares no tokens.
    pub async fn load(path: &Path) -> Result<Self, AuthConfigError> {
        let raw = fs::read_to_string(path).await?;
        let config: TokensConfig = toml::from_str(&raw)?;
        Self::from_entries(config.token)
    }

    /// Creates a token store from pre-hashed entries for tests and harnesses.
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
                        can_purge: false,
                    })
                    .collect(),
            ),
        }
    }

    pub(crate) fn authenticate(&self, presented: &str) -> Option<TokenContext> {
        self.tokens
            .iter()
            .find(|token| token.verify(presented))
            .cloned()
    }
}

/// Security configuration loaded from the token configuration file.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Bearer-token store used for request authentication.
    pub token_store: TokenStore,
    /// Per-token and per-source rate limit configuration.
    pub rate_limit: RateLimitConfig,
    /// Per-tenant JSON Schema registry.
    pub schema_registry: SchemaRegistry,
}

/// Loads token and rate-limit configuration from a TOML file.
///
/// Schema paths in `[[schema]]` entries are resolved relative to the tokens
/// config file's parent directory, not relative to the current working
/// directory.  This prevents deployment breakage when `systemd` (or similar)
/// starts the daemon from `/`.
///
/// # Errors
///
/// Returns [`AuthConfigError::Io`] if the file cannot be read,
/// [`AuthConfigError::Toml`] if it is not valid TOML,
/// [`AuthConfigError::SchemaProjectMismatch`] if a `[[schema]]` entry names a
/// project with no matching `[[token]]`, [`AuthConfigError::InvalidHash`] or
/// [`AuthConfigError::NoTokens`] from token loading, or
/// [`AuthConfigError::Schema`] if a referenced schema file fails to load.
pub async fn load_security_config(path: &Path) -> Result<SecurityConfig, AuthConfigError> {
    let raw = fs::read_to_string(path).await?;
    let config: TokensConfig = toml::from_str(&raw)?;

    // Collect the set of project names declared in [[token]] entries so we can
    // validate that every [[schema]] entry refers to a known project.
    let known_projects: std::collections::HashSet<&str> =
        config.token.iter().map(|t| t.project.as_str()).collect();

    // Determine the directory containing the tokens file so schema paths can
    // be resolved relative to it rather than relative to CWD.
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));

    // Build the list of resolved schema entries, checking project membership.
    let mut schema_entries: Vec<ProjectSchemaEntry> = Vec::new();
    for entry in &config.schema {
        if !known_projects.contains(entry.project.as_str()) {
            return Err(AuthConfigError::SchemaProjectMismatch {
                project: entry.project.clone(),
                kind: entry.kind,
            });
        }
        schema_entries.push(ProjectSchemaEntry {
            project: entry.project.clone(),
            kind: entry.kind,
            // Resolve the path relative to the tokens config's parent dir.
            path: config_dir.join(&entry.path),
        });
    }

    let token_store = TokenStore::from_entries(config.token)?;
    let mut schema_options = config.schema_validation;
    for resource in &mut schema_options.resources {
        resource.path = config_dir.join(&resource.path);
    }
    let schema_registry =
        SchemaRegistry::load_with_options(&schema_entries, &schema_options).await?;
    Ok(SecurityConfig {
        token_store,
        rate_limit: config.rate_limit.unwrap_or_default(),
        schema_registry,
    })
}

/// Pre-hashed token entry used by embedded tests and local harnesses.
#[derive(Clone)]
pub struct TestToken {
    /// Token identifier used in logs and rate-limit keys.
    pub id: String,
    /// Argon2 encoded password hash for the bearer token.
    pub secret_hash: String,
    /// Project this token may write.
    pub project: String,
    /// Canonical source prefix this token may write.
    pub source_prefix: String,
}

// Manual Debug so the Argon2 secret hash is not printed. C-DEBUG.
impl std::fmt::Debug for TestToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestToken")
            .field("id", &self.id)
            .field("project", &self.project)
            .field("source_prefix", &self.source_prefix)
            .field("secret_hash", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct TokensConfig {
    #[serde(default)]
    token: Vec<TokenEntry>,
    rate_limit: Option<RateLimitConfig>,
    /// Optional per-tenant schema declarations.  Existing tokens.toml files
    /// without a `[[schema]]` table will deserialise this as an empty `Vec`,
    /// producing a [`SchemaRegistry::empty()`] registry.
    #[serde(default)]
    schema: Vec<SchemaEntry>,
    #[serde(default)]
    schema_validation: SchemaOptions,
}

#[derive(Debug, Deserialize)]
struct TokenEntry {
    id: String,
    secret: String,
    project: String,
    source_prefix: String,
    #[serde(default)]
    pub(crate) can_purge: bool,
}

/// One `[[schema]]` entry from a tokens config file.
#[derive(Debug, Deserialize)]
struct SchemaEntry {
    /// Project this schema applies to; must match a `[[token]].project`.
    project: String,
    /// Payload kind (`crash_metadata` or `log_attributes`).
    kind: SchemaKind,
    /// Path to the JSON Schema file, resolved relative to the tokens config.
    path: std::path::PathBuf,
}

/// Errors returned while loading security configuration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthConfigError {
    /// Token configuration file could not be read.
    #[error("token config I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Token configuration file is not valid TOML.
    #[error("token config TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    /// Token configuration did not contain any token entries.
    #[error("token config contains no tokens")]
    NoTokens,
    /// Token entry contained an invalid Argon2 hash.
    #[error("token `{id}` has an invalid Argon2 hash: {message}")]
    InvalidHash {
        /// Token identifier from the configuration file.
        id: String,
        /// Hash parser error message.
        message: String,
    },
    /// A `[[schema]]` entry references a project not declared in `[[token]]`.
    #[error(
        "schema entry for project `{project}` / kind `{kind:?}` does not match any token project"
    )]
    SchemaProjectMismatch {
        /// The project name from the `[[schema]]` entry.
        project: String,
        /// The schema kind from the entry.
        kind: SchemaKind,
    },
    /// A schema file could not be read or parsed.
    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),
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
                can_purge: token.can_purge,
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
pub(crate) struct AuthState {
    pub token_store: TokenStore,
    pub metrics: Metrics,
}

pub(crate) async fn auth_middleware(
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
    let context = match authenticate_request(&state.token_store, request.headers()) {
        Ok(context) => context,
        Err(message) => {
            let status = StatusCode::UNAUTHORIZED;
            if path == PURGE_SOURCE_PATH && request.method() == Method::POST {
                record_purge(
                    &state.metrics,
                    None,
                    None,
                    status,
                    started,
                    &JanitorStats::default(),
                );
            } else {
                state
                    .metrics
                    .observe_request(endpoint, status.as_str(), started.elapsed());
            }
            return error_response(status, message);
        }
    };

    request.extensions_mut().insert(context);
    next.run(request).await
}

fn authenticate_request(
    store: &TokenStore,
    headers: &HeaderMap,
) -> Result<TokenContext, &'static str> {
    let token = headers
        .get(header::AUTHORIZATION)
        .ok_or("missing bearer token")?
        .to_str()
        .map_err(|_| "authorization header is not UTF-8")?
        .strip_prefix("Bearer ")
        .ok_or("authorization header must use Bearer")?;
    store.authenticate(token).ok_or("invalid bearer token")
}

#[allow(clippy::result_large_err)]
pub(crate) fn token_from_extensions(
    extensions: &axum::http::Extensions,
) -> Result<TokenContext, tonic::Status> {
    extensions
        .get::<TokenContext>()
        .cloned()
        .ok_or_else(|| tonic::Status::unauthenticated("missing token context"))
}

pub(crate) fn endpoint_label(path: &str) -> &'static str {
    if path == "/v1/crashes" {
        "crashes"
    } else if path == PURGE_SOURCE_PATH {
        "purge_source"
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

pub(crate) fn error_response(status: StatusCode, message: &str) -> Response {
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
