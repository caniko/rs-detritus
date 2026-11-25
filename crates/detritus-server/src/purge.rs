use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    time::Instant,
};

use axum::{
    Extension, Json,
    extract::{FromRequest, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tokio::fs;
use tracing::Instrument;

use crate::{
    auth::{TokenContext, endpoint_label, error_response},
    janitor::{JanitorError, JanitorStats, collect_files, is_crash_index, read_index_hashes},
    metrics::Metrics,
    server::AppState,
    storage::SourceKey,
};

pub(crate) const PURGE_SOURCE_PATH: &str = "/v1/admin/purge-source";

#[derive(Deserialize)]
struct PurgeRequest {
    project: String,
    install_id: String,
}

pub(crate) async fn purge_source(
    State(state): State<AppState>,
    Extension(token): Extension<TokenContext>,
    request: Request,
) -> Response {
    let started = Instant::now();
    let mut stats = JanitorStats::default();
    if !token.can_purge {
        let status = StatusCode::FORBIDDEN;
        record_purge(&state.metrics, Some(&token), None, status, started, &stats);
        return error_response(status, "token is not permitted to purge");
    }
    let body = Json::<PurgeRequest>::from_request(request, &state)
        .await
        .ok()
        .map(|Json(body)| body);
    let target = body
        .as_ref()
        .map(|body| (body.project.as_str(), body.install_id.as_str()));
    let source = body
        .as_ref()
        .and_then(|body| SourceKey::new(body.project.clone(), body.install_id.clone()).ok());
    let source = match source {
        Some(source) if token.permits(&source) => source,
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
    let (task_state, task_token, task_source) = (state.clone(), token.clone(), source.clone());
    // A disconnected caller must not release the lock while the old writer
    // is still draining or while deletion is incomplete.
    tokio::spawn(
        async move {
            let _guard = guard;
            let result = delete_source(&task_state, &task_source, &mut stats).await;
            task_state.rate_limiter.remove_source(&task_source).await;
            let status = if result.is_ok() {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            record_purge(
                &task_state.metrics,
                Some(&task_token),
                Some((&task_source.project, &task_source.source_id)),
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
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        record_purge(
            &state.metrics,
            Some(&token),
            Some((&source.project, &source.source_id)),
            status,
            started,
            &JanitorStats::default(),
        );
        error_response(status, "source purge failed")
    })
}

async fn delete_source(
    state: &AppState,
    source: &SourceKey,
    stats: &mut JanitorStats,
) -> Result<(), JanitorError> {
    state.writers.close_source(source).await;
    state.metrics.remove_writer_queue_depth(&source.canonical());
    let storage = &state.storage;
    let indexes_dir = storage.source_indexes_dir(source);
    let indexes = collect_files(storage, &indexes_dir, None).await?;
    let mut candidates = HashSet::new();
    for index in indexes.iter().filter(|file| is_crash_index(file)) {
        candidates.extend(read_index_hashes(index).await?);
    }
    if !candidates.is_empty() {
        let survivors = collect_files(storage, &storage.indexes_dir(), Some(&indexes_dir)).await?;
        for index in survivors.iter().filter(|file| is_crash_index(file)) {
            for hash in read_index_hashes(index).await? {
                candidates.remove(&hash);
            }
        }
    }
    let logs_dir = storage.source_logs_dir(source);
    let logs = collect_files(storage, &logs_dir, None).await?;
    let log_bytes = tree_bytes(&logs).await?;
    let index_bytes = tree_bytes(&indexes).await?;
    let mut blobs = Vec::new();
    for hash in candidates {
        let path = storage.blob_path(&hash);
        let shard = path.parent().expect("blob path includes its shard");
        if !storage.contains_dir(shard).await? {
            continue;
        }
        match fs::symlink_metadata(&path).await {
            Ok(meta) if meta.is_file() => blobs.push((path, meta.len())),
            Ok(_) => {
                return Err(io::Error::other(format!("invalid blob: {}", path.display())).into());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    // Finish all reads before any deletion so a failed scan can be retried.
    if remove_source_dir(&logs_dir).await? {
        stats.logs_deleted = logs.len() as u64;
        stats.bytes_freed += log_bytes;
    }
    if remove_source_dir(&indexes_dir).await? {
        stats.indexes_deleted = indexes.len() as u64;
        stats.bytes_freed += index_bytes;
    }
    for (path, len) in blobs {
        match fs::remove_file(path).await {
            Ok(()) => {
                stats.blobs_deleted += 1;
                stats.bytes_freed += len;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn tree_bytes(files: &[PathBuf]) -> io::Result<u64> {
    let mut bytes = 0;
    for file in files {
        bytes += fs::metadata(file).await?.len();
    }
    Ok(bytes)
}

async fn remove_source_dir(path: &Path) -> io::Result<bool> {
    match fs::remove_dir_all(path).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
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
