use std::{
    io,
    path::{Path, PathBuf},
};

use chrono::NaiveDate;
use serde::Serialize;
use tokio::fs;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct SourceKey {
    pub(crate) project: String,
    pub(crate) source_id: String,
}

impl SourceKey {
    pub(crate) fn new(project: String, source_id: String) -> Result<Self, StorageError> {
        validate_component("project", &project)?;
        validate_component("source_id", &source_id)?;
        Ok(Self { project, source_id })
    }

    pub(crate) fn canonical(&self) -> String {
        format!("{}/{}", self.project, self.source_id)
    }
}

#[doc(hidden)]
/// Hidden storage path helper used by integration tests.
#[derive(Debug, Clone)]
pub struct StoragePaths {
    data_dir: PathBuf,
}

impl StoragePaths {
    /// Creates storage paths rooted at `data_dir`.
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    /// Returns the storage root directory.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Creates the base storage directory tree.
    pub async fn prepare(&self) -> Result<(), StorageError> {
        fs::create_dir_all(self.data_dir.join("logs")).await?;
        fs::create_dir_all(self.data_dir.join("crashes").join("by-hash")).await?;
        fs::create_dir_all(self.data_dir.join("crashes").join("by-source")).await?;
        fs::create_dir_all(self.tmp_dir()).await?;
        Ok(())
    }

    pub(crate) fn tmp_dir(&self) -> PathBuf {
        self.data_dir.join("tmp")
    }

    pub(crate) fn log_file(&self, source: &SourceKey, date: NaiveDate) -> PathBuf {
        self.data_dir
            .join("logs")
            .join(&source.project)
            .join(&source.source_id)
            .join(format!("{date}.ndjson"))
    }

    pub(crate) fn blob_path(&self, sha256: &str) -> PathBuf {
        // Invariant: `sha256` is a 64-char lowercase hex digest produced by
        // `hex::encode(Sha256::finalize(..))` at the single call site, so the
        // two-char shard prefix is always in range and on a char boundary. A
        // future caller passing an untrusted/short hash must validate it first.
        let prefix = &sha256[..2];
        self.data_dir
            .join("crashes")
            .join("by-hash")
            .join(prefix)
            .join(format!("{sha256}.bin"))
    }

    pub(crate) fn index_path(&self, source: &SourceKey, timestamp: &str, sha256: &str) -> PathBuf {
        self.data_dir
            .join("crashes")
            .join("by-source")
            .join(&source.project)
            .join(&source.source_id)
            .join(format!("{timestamp}-{sha256}.json"))
    }

    pub(crate) fn relative_to_data_dir<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.data_dir).unwrap_or(path)
    }
}

#[doc(hidden)]
/// Hidden storage error type used by storage-layout test hooks.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// Storage path component was empty or contained path separators.
    #[error("invalid storage path component `{name}`: `{value}`")]
    InvalidComponent { name: &'static str, value: String },
    /// Filesystem operation failed.
    #[error("storage I/O error: {0}")]
    Io(#[from] io::Error),
}

pub(crate) fn validate_component(name: &'static str, value: &str) -> Result<(), StorageError> {
    let invalid = value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\');
    if invalid {
        Err(StorageError::InvalidComponent {
            name,
            value: value.to_owned(),
        })
    } else {
        Ok(())
    }
}
