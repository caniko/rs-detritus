use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use serde::Deserialize;
use tokio::{fs, task::JoinHandle};

use crate::{
    metrics::{JanitorMetricStats, Metrics},
    storage::StoragePaths,
};

#[derive(Debug, Clone, Copy)]
pub struct RetentionConfig {
    pub logs_ttl_days: u64,
    pub crashes_ttl_days: u64,
    pub janitor_interval: Duration,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            logs_ttl_days: 14,
            crashes_ttl_days: 90,
            janitor_interval: Duration::from_secs(60 * 60),
        }
    }
}

#[derive(Debug, Default)]
pub struct JanitorStats {
    pub logs_deleted: u64,
    pub indexes_deleted: u64,
    pub blobs_deleted: u64,
    pub bytes_freed: u64,
}

pub fn spawn_janitor(
    storage: StoragePaths,
    config: RetentionConfig,
    metrics: Metrics,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(config.janitor_interval);
        loop {
            interval.tick().await;
            let started = Instant::now();
            match run_janitor_cycle(&storage, config).await {
                Ok(stats) => {
                    metrics.observe_janitor("ok", started.elapsed(), &stats.as_metrics());
                    tracing::info!(?stats, "retention janitor cycle complete");
                }
                Err(error) => {
                    metrics.observe_janitor(
                        "error",
                        started.elapsed(),
                        &JanitorMetricStats::default(),
                    );
                    tracing::error!(%error, "retention janitor cycle failed");
                }
            }
        }
    })
}

pub async fn run_janitor_cycle(
    storage: &StoragePaths,
    config: RetentionConfig,
) -> Result<JanitorStats, JanitorError> {
    let mut stats = JanitorStats::default();
    let blob_snapshot = snapshot_blobs(&storage.data_dir().join("crashes").join("by-hash")).await?;
    prune_logs(storage, config.logs_ttl_days, &mut stats).await?;
    let referenced =
        prune_indexes_and_collect_references(storage, config.crashes_ttl_days, &mut stats).await?;
    sweep_blobs(&blob_snapshot, &referenced, &mut stats).await?;
    Ok(stats)
}

async fn prune_logs(
    storage: &StoragePaths,
    ttl_days: u64,
    stats: &mut JanitorStats,
) -> Result<(), JanitorError> {
    let root = storage.data_dir().join("logs");
    let files = collect_files(&root).await?;
    for file in files {
        if file.extension().and_then(|ext| ext.to_str()) != Some("ndjson") {
            continue;
        }
        if is_older_than(&file, ttl_days).await? {
            let len = file_len(&file).await;
            fs::remove_file(&file).await?;
            stats.logs_deleted += 1;
            stats.bytes_freed += len;
        }
    }
    Ok(())
}

async fn prune_indexes_and_collect_references(
    storage: &StoragePaths,
    ttl_days: u64,
    stats: &mut JanitorStats,
) -> Result<HashSet<String>, JanitorError> {
    let root = storage.data_dir().join("crashes").join("by-source");
    let files = collect_files(&root).await?;
    let mut referenced = HashSet::new();
    for file in files {
        if file.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if is_older_than(&file, ttl_days).await? {
            let len = file_len(&file).await;
            fs::remove_file(&file).await?;
            stats.indexes_deleted += 1;
            stats.bytes_freed += len;
            continue;
        }
        let bytes = fs::read(&file).await?;
        if let Ok(index) = serde_json::from_slice::<CrashIndexRef>(&bytes) {
            referenced.insert(index.dump.sha256);
        }
    }
    Ok(referenced)
}

async fn sweep_blobs(
    blob_snapshot: &[PathBuf],
    referenced: &HashSet<String>,
    stats: &mut JanitorStats,
) -> Result<(), JanitorError> {
    for blob in blob_snapshot {
        let Some(file_name) = blob.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(hash) = file_name.strip_suffix(".bin") else {
            continue;
        };
        if referenced.contains(hash) {
            continue;
        }
        let len = file_len(blob).await;
        match fs::remove_file(blob).await {
            Ok(()) => {
                stats.blobs_deleted += 1;
                stats.bytes_freed += len;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn snapshot_blobs(root: &Path) -> Result<Vec<PathBuf>, JanitorError> {
    collect_files(root).await.map(|files| {
        files
            .into_iter()
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("bin"))
            .collect()
    })
}

async fn collect_files(root: &Path) -> Result<Vec<PathBuf>, JanitorError> {
    if !fs::try_exists(root).await? {
        return Ok(Vec::new());
    }
    let mut dirs = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = dirs.pop() {
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let ty = entry.file_type().await?;
            if ty.is_dir() {
                dirs.push(entry.path());
            } else if ty.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

async fn is_older_than(path: &Path, ttl_days: u64) -> Result<bool, JanitorError> {
    let modified = fs::metadata(path).await?.modified()?;
    if ttl_days == 0 {
        return Ok(true);
    }
    let cutoff = SystemTime::now() - Duration::from_secs(ttl_days * 24 * 60 * 60);
    Ok(modified <= cutoff)
}

async fn file_len(path: &Path) -> u64 {
    fs::metadata(path)
        .await
        .map_or(0, |metadata| metadata.len())
}

impl JanitorStats {
    fn as_metrics(&self) -> JanitorMetricStats {
        JanitorMetricStats {
            logs_deleted: self.logs_deleted,
            indexes_deleted: self.indexes_deleted,
            blobs_deleted: self.blobs_deleted,
            bytes_freed: self.bytes_freed,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CrashIndexRef {
    dump: CrashDumpRef,
}

#[derive(Debug, Deserialize)]
struct CrashDumpRef {
    sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum JanitorError {
    #[error("janitor I/O error: {0}")]
    Io(#[from] std::io::Error),
}
