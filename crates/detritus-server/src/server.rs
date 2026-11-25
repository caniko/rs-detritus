use std::{future::Future, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use detritus_protocol::otlp::logs::LogsServiceServer;
use tokio::{net::TcpListener, sync::Mutex};
use tonic::codec::CompressionEncoding;
use tonic::service::Routes;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    decompression::RequestDecompressionLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::TraceLayer,
};

use crate::{
    auth::{AuthState, TokenStore, auth_middleware},
    crashes::crashes_handler,
    janitor::{RetentionConfig, spawn_janitor},
    logs::{LogWriterPool, LogsHandler},
    metrics::Metrics,
    purge::{PURGE_SOURCE_PATH, purge_source},
    rate_limit::{RateLimitConfig, RateLimiter},
    schemas::SchemaRegistry,
    storage::{StorageError, StoragePaths},
};

/// Errors returned while starting or running a Detritus server.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// Binding the listener, inspecting its address, or the running server failed.
    #[error("server I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Preparing the on-disk storage layout failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Registering the Prometheus metrics collectors failed.
    #[error("metrics initialization failed: {0}")]
    Metrics(#[from] prometheus::Error),
}

/// Runtime configuration for an embedded Detritus server.
#[derive(Debug, Clone)]
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
    pub(crate) writers: LogWriterPool,
    pub(crate) maintenance: Arc<Mutex<()>>,
}

/// Runs a Detritus server until the process receives Ctrl-C or, on Unix, SIGTERM.
///
/// # Errors
///
/// Returns an error if binding the listener, preparing storage, initializing
/// metrics, or serving requests fails.
pub async fn serve(config: ServerConfig) -> Result<(), ServerError> {
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
) -> Result<(), ServerError> {
    let storage = StoragePaths::new(config.data_dir);
    storage.prepare().await?;
    let metrics = Metrics::new()?;
    let rate_limiter = RateLimiter::new(config.rate_limit);
    let writers = LogWriterPool::new(storage.clone());
    // ponytail: one storage lock delays publication during a full sweep;
    // use finer-grained locking and reference tracking if sweeps get slow.
    let maintenance = Arc::new(Mutex::new(()));
    let janitor = spawn_janitor(
        storage.clone(),
        config.retention,
        metrics.clone(),
        maintenance.clone(),
    );
    let app = app(
        AppState {
            storage,
            max_dump_bytes: config.max_dump_bytes,
            rate_limiter,
            metrics,
            schema_registry: config.schema_registry,
            writers: writers.clone(),
            maintenance: maintenance.clone(),
        },
        config.token_store,
    );
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "observability server listening");

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
    janitor.abort();
    let _guard = maintenance.lock().await;
    writers.shutdown().await;
    result?;
    Ok(())
}

fn app(state: AppState, token_store: TokenStore) -> Router {
    let grpc = Routes::new(
        LogsServiceServer::new(LogsHandler::new(
            state.writers.clone(),
            state.rate_limiter.clone(),
            state.metrics.clone(),
            state.schema_registry.clone(),
            state.maintenance.clone(),
        ))
        .accept_compressed(CompressionEncoding::Gzip)
        .accept_compressed(CompressionEncoding::Zstd)
        .send_compressed(CompressionEncoding::Gzip)
        .send_compressed(CompressionEncoding::Zstd),
    )
    .into_axum_router();
    let auth_state = AuthState {
        token_store,
        metrics: state.metrics.clone(),
    };
    let http = Router::new()
        .route("/v1/crashes", post(crashes_handler))
        .route(
            PURGE_SOURCE_PATH,
            post(purge_source).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/healthz", get(healthz))
        .route("/metrics", get(render_metrics))
        .with_state(state);

    http.merge(grpc)
        .layer(middleware::from_fn_with_state(auth_state, auth_middleware))
        .layer(
            ServiceBuilder::new()
                .layer(SetSensitiveRequestHeadersLayer::new([
                    header::AUTHORIZATION,
                ]))
                // Set before tracing, propagate after it so generated IDs appear
                // in both spans and responses, including authentication failures.
                // https://docs.rs/tower-http/0.7.1/tower_http/request_id/index.html
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(TraceLayer::new_for_http().make_span_with(
                    |request: &axum::http::Request<axum::body::Body>| {
                        tracing::info_span!("http_request",
                            method = %request.method(), uri = %request.uri(),
                            request_id = request.headers().get("x-request-id")
                                .and_then(|value| value.to_str().ok()).unwrap_or("")
                        )
                    },
                ))
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(RequestDecompressionLayer::new())
                .layer(RequestBodyLimitLayer::new(150 * 1024 * 1024))
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
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(error) = result {
                            tracing::error!(%error, "failed to install ctrl-c handler");
                        }
                    }
                    _ = terminate.recv() => {}
                }
                return;
            }
            Err(error) => tracing::error!(%error, "failed to install SIGTERM handler"),
        }
    }
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install ctrl-c handler");
    }
}
