use std::{io, path::Path, time::Instant};

use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tokio::fs;
use tracing::Instrument;

use crate::{
    auth::{TokenContext, endpoint_label, error_response},
    janitor::{JanitorError, JanitorStats, sweep_unreferenced_blobs},
    metrics::Metrics,
    server::AppState,
    storage::{SourceKey, StoragePaths},
};

pub(crate) const PURGE_SOURCE_PATH: &str = "/v1/admin/purge-source";

#[derive(Deserialize)]
pub(crate) struct PurgeRequest {
    project: String,
    install_id: String,
}

pub(crate) async fn purge_source(
    State(state): State<AppState>,
    Extension(token): Extension<TokenContext>,
    body: Result<Json<PurgeRequest>, JsonRejection>,
) -> Response {
    let started = Instant::now();
    let body = body.ok().map(|Json(body)| body);
    let target = body
        .as_ref()
        .map(|body| (body.project.as_str(), body.install_id.as_str()));
    let source = body
        .as_ref()
        .and_then(|body| SourceKey::new(body.project.clone(), body.install_id.clone()).ok());
    let mut stats = JanitorStats::default();
    let source = match source {
        Some(source) if token.can_purge && token.permits(&source) => source,
        denied => {
            let (status, message) = if denied.is_some() {
                (
                    StatusCode::FORBIDDEN,
                    "token is not permitted to purge this source",
                )
            } else {
                (StatusCode::BAD_REQUEST, "invalid purge request")
            };
            record_purge(
                &state.metrics,
                Some(&token),
                target,
                status,
                started,
                &stats,
            );
            return error_response(status, message);
        }
    };

    let guard = state.maintenance.clone().lock_owned().await;
    // A disconnected caller must not release the lock while the old writer
    // is still draining or while deletion is incomplete.
    tokio::spawn(
        async move {
            let _guard = guard;
            let result = delete_source(&state, &source, &mut stats).await;
            let status = if result.is_ok() {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            record_purge(
                &state.metrics,
                Some(&token),
                Some((&source.project, &source.source_id)),
                status,
                started,
                &stats,
            );
            if let Err(error) = result {
                tracing::error!(%error, "source purge failed");
                return error_response(status, "source purge failed");
            }
            Json(stats).into_response()
        }
        .in_current_span(),
    )
    .await
    .unwrap_or_else(|error| {
        tracing::error!(%error, "source purge task failed");
        error_response(StatusCode::INTERNAL_SERVER_ERROR, "source purge failed")
    })
}

async fn delete_source(
    state: &AppState,
    source: &SourceKey,
    stats: &mut JanitorStats,
) -> Result<(), JanitorError> {
    state.writers.close_source(source).await;
    let storage = &state.storage;
    let logs = remove_source_dir(
        storage,
        &storage.source_logs_dir(source),
        &mut stats.logs_deleted,
        &mut stats.bytes_freed,
    )
    .await?;
    let indexes = remove_source_dir(
        storage,
        &storage.source_indexes_dir(source),
        &mut stats.indexes_deleted,
        &mut stats.bytes_freed,
    )
    .await?;
    // A no-op should not report unrelated orphan removals. The periodic janitor
    // still handles orphans left by an interrupted purge.
    if logs || indexes {
        sweep_unreferenced_blobs(storage, None, stats).await?;
    }
    Ok(())
}

async fn remove_source_dir(
    storage: &StoragePaths,
    path: &Path,
    files: &mut u64,
    bytes: &mut u64,
) -> io::Result<bool> {
    if !storage.contains_dir(path).await? {
        return Ok(false);
    }
    remove_tree(path, files, bytes).await?;
    Ok(true)
}

async fn remove_tree(path: &Path, files: &mut u64, bytes: &mut u64) -> io::Result<()> {
    let mut entries = match fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    while let Some(entry) = entries.next_entry().await? {
        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.is_dir() {
            Box::pin(remove_tree(&entry.path(), files, bytes)).await?;
        } else {
            match fs::remove_file(entry.path()).await {
                Ok(()) => {
                    *files += 1;
                    *bytes += metadata.len();
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    match fs::remove_dir(path).await {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(crate) fn record_purge(
    metrics: &Metrics,
    token: Option<&TokenContext>,
    target: Option<(&str, &str)>,
    status: StatusCode,
    started: Instant,
    stats: &JanitorStats,
) {
    let result = if status.is_success() { "ok" } else { "error" };
    let (project, source_id) = target.unwrap_or_default();
    tracing::info!(
        target: "detritus::audit",
        event = "purge_source",
        token_id = token.map_or("", |token| token.id.as_str()),
        project,
        source_id,
        logs_deleted = stats.logs_deleted,
        indexes_deleted = stats.indexes_deleted,
        blobs_deleted = stats.blobs_deleted,
        bytes_freed = stats.bytes_freed,
        result,
    );
    // Use configured projects, not unbounded request values, as metric labels.
    metrics.observe_purge(
        token.map_or("unknown", |token| token.project.as_str()),
        result,
    );
    metrics.observe_request(
        endpoint_label(PURGE_SOURCE_PATH),
        status.as_str(),
        started.elapsed(),
    );
}
