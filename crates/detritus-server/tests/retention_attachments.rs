//! Retention must preserve every blob referenced by a live crash index.

use std::path::{Path, PathBuf};

use detritus_server::{RetentionConfig, StoragePaths, run_janitor_cycle};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[tokio::test]
async fn live_index_preserves_dump_and_attachment_blobs() {
    let temp = TempDir::new().expect("storage");
    let storage = StoragePaths::new(temp.path().to_path_buf());
    storage.prepare().await.expect("prepare");
    let (dump_hash, dump_path) = blob(temp.path(), b"dump").await;
    let (attachment_hash, attachment_path) = blob(temp.path(), b"context").await;
    let (_, orphan_path) = blob(temp.path(), b"orphan").await;
    let index_dir = temp.path().join("crashes/by-source/project/source");
    tokio::fs::create_dir_all(&index_dir)
        .await
        .expect("indexes");
    tokio::fs::write(
        index_dir.join("live.json"),
        serde_json::to_vec(&json!({
            "dump": { "sha256": dump_hash },
            "attachments": [{ "sha256": attachment_hash }]
        }))
        .expect("index JSON"),
    )
    .await
    .expect("write index");

    let stats = run_janitor_cycle(&storage, RetentionConfig::default())
        .await
        .expect("retention pass");

    assert_eq!(stats.indexes_deleted, 0);
    assert_eq!(stats.blobs_deleted, 1);
    assert_eq!(stats.bytes_freed, 6);
    assert_eq!(tokio::fs::read(dump_path).await.expect("dump"), b"dump");
    assert_eq!(
        tokio::fs::read(attachment_path).await.expect("attachment"),
        b"context"
    );
    assert!(!orphan_path.exists());
}

async fn blob(root: &Path, bytes: &[u8]) -> (String, PathBuf) {
    let hash = hex::encode(Sha256::digest(bytes));
    let path = root
        .join("crashes/by-hash")
        .join(&hash[..2])
        .join(format!("{hash}.bin"));
    tokio::fs::create_dir_all(path.parent().expect("blob directory"))
        .await
        .expect("mkdir");
    tokio::fs::write(&path, bytes).await.expect("write blob");
    (hash, path)
}
