use std::{path::Path, time::SystemTime};

use detritus_server::{
    janitor::{RetentionConfig, run_janitor_cycle},
    storage::StoragePaths,
};
use filetime::FileTime;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[tokio::test]
async fn janitor_smoke_prunes_expired_logs_indexes_and_unreferenced_blobs() {
    let temp = TempDir::new().expect("temp dir");
    let storage = StoragePaths::new(temp.path().to_path_buf());
    storage.prepare().await.expect("prepare storage");

    let old_log = temp
        .path()
        .join("logs")
        .join("detritus")
        .join("old-source")
        .join("2026-01-01.ndjson");
    write_file(&old_log, b"old\n").await;
    set_old_mtime(&old_log);

    let recent_log = temp
        .path()
        .join("logs")
        .join("detritus")
        .join("recent-source")
        .join("2026-05-19.ndjson");
    write_file(&recent_log, b"recent\n").await;

    let old_blob = write_blob(temp.path(), b"old dump").await;
    let live_blob = write_blob(temp.path(), b"live dump").await;
    let old_index = write_index(temp.path(), "old-source", &old_blob).await;
    let live_index = write_index(temp.path(), "recent-source", &live_blob).await;
    set_old_mtime(&old_index);

    let stats = run_janitor_cycle(
        &storage,
        RetentionConfig {
            logs_ttl_days: 14,
            crashes_ttl_days: 14,
            janitor_interval: std::time::Duration::from_secs(60),
        },
    )
    .await
    .expect("janitor runs");

    assert_eq!(stats.logs_deleted, 1);
    assert_eq!(stats.indexes_deleted, 1);
    assert_eq!(stats.blobs_deleted, 1);
    assert!(!old_log.exists());
    assert!(recent_log.exists());
    assert!(!old_index.exists());
    assert!(live_index.exists());
    assert!(!blob_path(temp.path(), &old_blob).exists());
    assert!(blob_path(temp.path(), &live_blob).exists());
}

#[tokio::test]
async fn janitor_ttl_zero_wipes_all_ndjson() {
    let temp = TempDir::new().expect("temp dir");
    let storage = StoragePaths::new(temp.path().to_path_buf());
    storage.prepare().await.expect("prepare storage");
    let first = temp.path().join("logs/a/one/2026-05-19.ndjson");
    let second = temp.path().join("logs/a/two/2026-05-20.ndjson");
    write_file(&first, b"one\n").await;
    write_file(&second, b"two\n").await;

    run_janitor_cycle(
        &storage,
        RetentionConfig {
            logs_ttl_days: 0,
            crashes_ttl_days: 90,
            janitor_interval: std::time::Duration::from_secs(1),
        },
    )
    .await
    .expect("janitor runs");

    assert!(!first.exists());
    assert!(!second.exists());
}

async fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.expect("parent");
    }
    tokio::fs::write(path, bytes).await.expect("write");
}

async fn write_blob(root: &Path, bytes: &[u8]) -> String {
    let hash = hex::encode(Sha256::digest(bytes));
    let path = blob_path(root, &hash);
    write_file(&path, bytes).await;
    hash
}

async fn write_index(root: &Path, source: &str, hash: &str) -> std::path::PathBuf {
    let path = root
        .join("crashes")
        .join("by-source")
        .join("detritus")
        .join(source)
        .join(format!("20260519T000000Z-{hash}.json"));
    let body = serde_json::to_vec(&json!({
        "metadata": {},
        "dump": { "sha256": hash },
        "attachments": []
    }))
    .expect("json");
    write_file(&path, &body).await;
    path
}

fn blob_path(root: &Path, hash: &str) -> std::path::PathBuf {
    root.join("crashes")
        .join("by-hash")
        .join(&hash[..2])
        .join(format!("{hash}.bin"))
}

fn set_old_mtime(path: &Path) {
    let old = FileTime::from_system_time(
        SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 60 * 60),
    );
    filetime::set_file_mtime(path, old).expect("set mtime");
}
