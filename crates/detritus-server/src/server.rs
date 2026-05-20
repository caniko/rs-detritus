use std::{future::Future, net::SocketAddr, path::PathBuf};

use axum::{
    Router,
    http::{StatusCode, header},
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use detritus_protocol::otlp::logs::LogsServiceServer;
use tokio::net::TcpListener;
use tonic::service::Routes;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, decompression::RequestDecompressionLayer,
    limit::RequestBodyLimitLayer, trace::TraceLayer,
};

use crate::{
    auth::{AuthState, TokenStore, auth_middleware},
    crashes::crashes_handler,
    janitor::{RetentionConfig, spawn_janitor},
    logs::{LogWriterPool, LogsHandler},
    metrics::Metrics,
    rate_limit::{RateLimitConfig, RateLimiter},
    schemas::SchemaRegistry,
    storage::StoragePaths,
};

/// Runtime configuration for an embedded Detritus server.
#[derive(Clone)]
pub struct ServerConfig {
    /// Socket address to bind when using [`serve`].
    pub bind: SocketAddr,
    /// Root directory for logs, crash blobs, indexes, and temporary files.
    pub data_dir: PathBuf,
    /// Maximum accepted crash dump or attachment size in bytes.
    pub max_dump_bytes: u64,
    /// Bearer-token store used by HTTP and gRPC authentication.
    pub token_store: TokenStore,
    /// Per-source and per-token rate limit configuration.
    pub rate_limit: RateLimitConfig,
    /// Retention policy for logs, crash indexes, and unreferenced blobs.
    pub retention: RetentionConfig,
    /// Tenant JSON Schema registry; use [`SchemaRegistry::empty`] to disable
    /// schema validation for all tenants.
    pub schema_registry: SchemaRegistry,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) storage: StoragePaths,
    pub(crate) max_dump_bytes: u64,
    pub(crate) rate_limiter: RateLimiter,
    pub(crate) metrics: Metrics,
    pub(crate) schema_registry: SchemaRegistry,
}

/// Runs a Detritus server until the process receives Ctrl-C.
///
/// # Errors
///
/// Returns an error if binding the listener, preparing storage, initializing
/// metrics, or serving requests fails.
pub async fn serve(config: ServerConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(config.bind).await?;
    serve_with_shutdown(listener, config, shutdown_signal()).await
}

/// Runs a Detritus server on an existing listener until `shutdown` resolves.
///
/// # Errors
///
/// Returns an error if storage preparation, metrics initialization, listener
/// inspection, or the HTTP/gRPC server fails.
///
/// # Examples
///
/// ```no_run
/// use detritus_server::{
///     RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig, TestToken, TokenStore,
///     serve_with_shutdown,
/// };
/// use tokio::net::TcpListener;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let listener = TcpListener::bind("127.0.0.1:0").await?;
/// let bind = listener.local_addr()?;
/// let config = ServerConfig {
///     bind,
///     data_dir: std::env::temp_dir().join("detritus"),
///     max_dump_bytes: 100 * 1024 * 1024,
///     token_store: TokenStore::for_tests(Vec::<TestToken>::new()),
///     rate_limit: RateLimitConfig::default(),
///     retention: RetentionConfig::default(),
///     schema_registry: SchemaRegistry::empty(),
/// };
///
/// serve_with_shutdown(listener, config, async {}).await?;
/// # Ok(())
/// # }
/// ```
pub async fn serve_with_shutdown(
    listener: TcpListener,
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let storage = StoragePaths::new(config.data_dir);
    storage.prepare().await?;
    let metrics = Metrics::new()?;
    let rate_limiter = RateLimiter::new(config.rate_limit);
    let writers = LogWriterPool::new(storage.clone());
    let janitor = spawn_janitor(storage.clone(), config.retention, metrics.clone());
    let app = app(
        storage,
        writers.clone(),
        config.max_dump_bytes,
        config.token_store,
        rate_limiter,
        metrics,
        config.schema_registry,
    );
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "observability server listening");

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
    janitor.abort();
    writers.shutdown().await;
    result?;
    Ok(())
}

fn app(
    storage: StoragePaths,
    writers: LogWriterPool,
    max_dump_bytes: u64,
    token_store: TokenStore,
    rate_limiter: RateLimiter,
    metrics: Metrics,
    schema_registry: SchemaRegistry,
) -> Router {
    let state = AppState {
        storage,
        max_dump_bytes,
        rate_limiter,
        metrics: metrics.clone(),
        schema_registry,
    };
    let grpc = Routes::new(LogsServiceServer::new(LogsHandler::new(
        writers,
        state.rate_limiter.clone(),
        state.metrics.clone(),
    )))
    .into_axum_router();
    let http = Router::new()
        .route("/v1/crashes", post(crashes_handler))
        .route("/healthz", get(healthz))
        .route("/metrics", get(render_metrics))
        .with_state(state);
    let auth_state = AuthState {
        token_store,
        metrics,
    };

    http.merge(grpc)
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(RequestBodyLimitLayer::new(150 * 1024 * 1024))
                .layer(RequestDecompressionLayer::new())
                .layer(CompressionLayer::new()),
        )
}

async fn healthz() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn render_metrics(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    match state.metrics.render() {
        Ok(body) => (
            [(
                header::CONTENT_TYPE,
                "application/openmetrics-text; version=1.0.0",
            )],
            body,
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to render metrics: {error}"),
        )
            .into_response(),
    }
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install ctrl-c handler");
    }
}
