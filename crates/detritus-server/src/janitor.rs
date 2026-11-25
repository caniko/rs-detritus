use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use serde::{Deserialize, Serialize};
use tokio::{fs, sync::Mutex, task::JoinHandle};

use crate::{
    metrics::{JanitorMetricStats, Metrics},
    storage::StoragePaths,
};

/// Retention policy for logs, crash indexes, and unreferenced crash blobs.
#[derive(Debug, Clone, Copy)]
pub struct RetentionConfig {
    /// Number of days to keep NDJSON log files.
    pub logs_ttl_days: u64,
    /// Number of days to keep crash indexes.
    pub crashes_ttl_days: u64,
    /// Delay between background janitor cycles.
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

/// Files and bytes removed by one janitor cycle.
#[derive(Debug, Default, Serialize)]
pub struct JanitorStats {
    /// NDJSON log files removed.
    pub logs_deleted: u64,
    /// Crash index JSON files removed.
    pub indexes_deleted: u64,
    /// Unreferenced crash blobs removed.
    pub blobs_deleted: u64,
    /// Total bytes freed from removed files.
    pub bytes_freed: u64,
}

pub(crate) fn spawn_janitor(
    storage: StoragePaths,
    config: RetentionConfig,
    metrics: Metrics,
    maintenance: Arc<Mutex<()>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(config.janitor_interval);
        loop {
            interval.tick().await;
            let started = Instant::now();
            let mut stats = JanitorStats::default();
            let result = {
                let _guard = maintenance.lock().await;
                run_cycle(&storage, config, &mut stats).await
            };
            let label = if result.is_ok() { "ok" } else { "error" };
            metrics.observe_janitor(label, started.elapsed(), &stats.as_metrics());
            match result {
                Ok(()) => tracing::info!(?stats, "retention janitor cycle complete"),
                // Already reported with its path while collecting references.
                Err(JanitorError::CorruptIndex { .. }) => {}
                Err(error) => {
                    tracing::error!(%error, "retention janitor cycle failed");
                }
            }
        }
    })
}

/// Runs one retention pass over the storage tree.
pub async fn run_janitor_cycle(
    storage: &StoragePaths,
    config: RetentionConfig,
) -> Result<JanitorStats, JanitorError> {
    let mut stats = JanitorStats::default();
    run_cycle(storage, config, &mut stats).await?;
    Ok(stats)
}

async fn run_cycle(
    storage: &StoragePaths,
    config: RetentionConfig,
    stats: &mut JanitorStats,
) -> Result<(), JanitorError> {
    prune_logs(storage, config.logs_ttl_days, stats).await?;
    // Snapshot candidates before scanning indexes, never the other way around.
    let snapshot =
        collect_files(storage, &storage.data_dir().join("crashes/by-hash"), None).await?;
    let referenced = collect_index_references(storage, config.crashes_ttl_days, stats).await?;
    sweep_blobs(&snapshot, &referenced, stats).await
}

async fn prune_logs(
    storage: &StoragePaths,
    ttl_days: u64,
    stats: &mut JanitorStats,
) -> Result<(), JanitorError> {
    let root = storage.data_dir().join("logs");
    let files = collect_files(storage, &root, None).await?;
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

async fn collect_index_references(
    storage: &StoragePaths,
    ttl_days: u64,
    stats: &mut JanitorStats,
) -> Result<HashSet<String>, JanitorError> {
    let files = collect_files(storage, &storage.indexes_dir(), None).await?;
    let mut referenced = HashSet::new();
    let mut corrupt = None;
    for file in files.into_iter().filter(|file| is_crash_index(file)) {
        if is_older_than(&file, ttl_days).await? {
            let len = file_len(&file).await;
            fs::remove_file(&file).await?;
            stats.indexes_deleted += 1;
            stats.bytes_freed += len;
            continue;
        }
        match read_index_hashes(&file).await {
            Ok(hashes) => referenced.extend(hashes),
            Err(error @ JanitorError::CorruptIndex { .. }) => {
                tracing::error!(path = %file.display(), %error, "skipping blob sweep");
                corrupt.get_or_insert(error);
            }
            Err(error) => return Err(error),
        }
    }
    corrupt.map_or(Ok(referenced), Err)
}

pub(crate) fn is_crash_index(file: &Path) -> bool {
    file.extension().and_then(|ext| ext.to_str()) == Some("json")
}

pub(crate) async fn read_index_hashes(
    file: &Path,
) -> Result<impl Iterator<Item = String> + use<>, JanitorError> {
    let bytes = fs::read(file).await?;
    let corrupt = |reason| JanitorError::CorruptIndex {
        path: file.to_path_buf(),
        reason,
    };
    let index: CrashIndexRef =
        serde_json::from_slice(&bytes).map_err(|error| corrupt(error.to_string()))?;
    // The purge also uses these digests to construct blob paths.
    if !std::iter::once(&index.dump)
        .chain(&index.attachments)
        .all(|blob| {
            blob.sha256.len() == 64
                && blob
                    .sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
    {
        return Err(corrupt("invalid SHA-256 digest".to_owned()));
    }
    Ok(std::iter::once(index.dump)
        .chain(index.attachments)
        .map(|blob| blob.sha256))
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
        let len = match fs::metadata(blob).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
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

pub(crate) async fn collect_files(
    storage: &StoragePaths,
    root: &Path,
    exclude: Option<&Path>,
) -> io::Result<Vec<PathBuf>> {
    if !storage.contains_dir(root).await? {
        return Ok(Vec::new());
    }
    let mut dirs = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = dirs.pop() {
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if exclude == Some(path.as_path()) {
                continue;
            }
            let ty = entry.file_type().await?;
            if ty.is_dir() {
                dirs.push(path);
            } else if ty.is_file() {
                files.push(path);
            } else if ty.is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected symlink in storage tree: {}", path.display()),
                ));
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
    #[serde(default)]
    attachments: Vec<CrashDumpRef>,
}

#[derive(Debug, Deserialize)]
struct CrashDumpRef {
    sha256: String,
}

#[doc(hidden)]
/// Hidden error type returned by the retention test hook.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JanitorError {
    /// Filesystem operation failed during retention cleanup.
    #[error("janitor I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A surviving crash index could not be parsed safely.
    #[error("corrupt crash index {}: {reason}", path.display())]
    CorruptIndex {
        /// Path of the index.
        path: PathBuf,
        /// Why the index could not be used.
        reason: String,
    },
}
