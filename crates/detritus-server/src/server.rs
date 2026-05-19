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
    storage::StoragePaths,
};

#[derive(Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub max_dump_bytes: u64,
    pub token_store: TokenStore,
    pub rate_limit: RateLimitConfig,
    pub retention: RetentionConfig,
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) storage: StoragePaths,
    pub(crate) max_dump_bytes: u64,
    pub(crate) rate_limiter: RateLimiter,
    pub(crate) metrics: Metrics,
}

pub async fn serve(config: ServerConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(config.bind).await?;
    serve_with_shutdown(listener, config, shutdown_signal()).await
}

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
) -> Router {
    let state = AppState {
        storage,
        max_dump_bytes,
        rate_limiter,
        metrics: metrics.clone(),
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
