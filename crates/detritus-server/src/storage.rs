use std::{
    io,
    path::{Path, PathBuf},
};

use chrono::NaiveDate;
use serde::Serialize;
use tokio::fs;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SourceKey {
    pub project: String,
    pub source_id: String,
}

impl SourceKey {
    pub fn new(project: String, source_id: String) -> Result<Self, StorageError> {
        validate_component("project", &project)?;
        validate_component("source_id", &source_id)?;
        Ok(Self { project, source_id })
    }

    pub fn canonical(&self) -> String {
        format!("{}/{}", self.project, self.source_id)
    }
}

#[derive(Debug, Clone)]
pub struct StoragePaths {
    data_dir: PathBuf,
}

impl StoragePaths {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub async fn prepare(&self) -> Result<(), StorageError> {
        fs::create_dir_all(self.data_dir.join("logs")).await?;
        fs::create_dir_all(self.data_dir.join("crashes").join("by-hash")).await?;
        fs::create_dir_all(self.data_dir.join("crashes").join("by-source")).await?;
        fs::create_dir_all(self.tmp_dir()).await?;
        Ok(())
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.data_dir.join("tmp")
    }

    pub fn log_file(&self, source: &SourceKey, date: NaiveDate) -> PathBuf {
        self.data_dir
            .join("logs")
            .join(&source.project)
            .join(&source.source_id)
            .join(format!("{date}.ndjson"))
    }

    pub fn blob_path(&self, sha256: &str) -> PathBuf {
        let prefix = &sha256[..2];
        self.data_dir
            .join("crashes")
            .join("by-hash")
            .join(prefix)
            .join(format!("{sha256}.bin"))
    }

    pub fn index_path(&self, source: &SourceKey, timestamp: &str, sha256: &str) -> PathBuf {
        self.data_dir
            .join("crashes")
            .join("by-source")
            .join(&source.project)
            .join(&source.source_id)
            .join(format!("{timestamp}-{sha256}.json"))
    }

    pub fn relative_to_data_dir<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.data_dir).unwrap_or(path)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("invalid storage path component `{name}`: `{value}`")]
    InvalidComponent { name: &'static str, value: String },
    #[error("storage I/O error: {0}")]
    Io(#[from] io::Error),
}

pub fn validate_component(name: &'static str, value: &str) -> Result<(), StorageError> {
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
