use std::{
    io,
    path::{Component, Path, PathBuf},
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

    pub(crate) async fn contains_dir(&self, path: &Path) -> io::Result<bool> {
        // Trust the configured root, but never follow a symlink below it.
        let mut dir = self.data_dir.clone();
        for part in path
            .strip_prefix(&dir)
            .map_err(io::Error::other)?
            .components()
        {
            let Component::Normal(name) = part else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid storage path",
                ));
            };
            dir.push(name);
            match fs::symlink_metadata(&dir).await {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid storage directory",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }

    /// Creates the base storage directory tree.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] if creating any storage subdirectory fails.
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
        self.source_logs_dir(source).join(format!("{date}.ndjson"))
    }

    pub(crate) fn source_logs_dir(&self, source: &SourceKey) -> PathBuf {
        self.data_dir
            .join("logs")
            .join(&source.project)
            .join(&source.source_id)
    }

    pub(crate) fn blob_path(&self, sha256: &str) -> PathBuf {
        // Callers generate or validate the 64-char lowercase hex digest.
        let prefix = &sha256[..2];
        self.data_dir
            .join("crashes")
            .join("by-hash")
            .join(prefix)
            .join(format!("{sha256}.bin"))
    }

    pub(crate) fn index_path(&self, source: &SourceKey, timestamp: &str, sha256: &str) -> PathBuf {
        self.source_indexes_dir(source)
            .join(format!("{timestamp}-{sha256}.json"))
    }

    pub(crate) fn source_indexes_dir(&self, source: &SourceKey) -> PathBuf {
        self.indexes_dir()
            .join(&source.project)
            .join(&source.source_id)
    }

    pub(crate) fn indexes_dir(&self) -> PathBuf {
        self.data_dir.join("crashes").join("by-source")
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
    /// Storage path component was not a valid single name of at most 255 bytes.
    #[error("invalid storage path component `{name}`: `{value}`")]
    InvalidComponent { name: &'static str, value: String },
    /// Filesystem operation failed.
    #[error("storage I/O error: {0}")]
    Io(#[from] io::Error),
}

pub(crate) fn validate_component(name: &'static str, value: &str) -> Result<(), StorageError> {
    let invalid = value.len() > 255
        || value.contains(['/', '\\', '\0'])
        || !matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        );
    if invalid {
        Err(StorageError::InvalidComponent {
            name,
            value: value.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn source_key_accepts_ordinary_components() {
        let key = SourceKey::new("detritus".to_owned(), "install-42".to_owned())
            .expect("ordinary components are valid");
        assert_eq!(key.canonical(), "detritus/install-42");
    }

    #[test]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn source_key_rejects_empty_components() {
        assert!(matches!(
            SourceKey::new(String::new(), "ok".to_owned()),
            Err(StorageError::InvalidComponent {
                name: "project",
                ..
            })
        ));
        assert!(matches!(
            SourceKey::new("ok".to_owned(), String::new()),
            Err(StorageError::InvalidComponent {
                name: "source_id",
                ..
            })
        ));
    }

    #[test]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn source_key_rejects_path_traversal_components() {
        // The component becomes a directory name in blob/log paths, so dot and
        // separator sequences must never be accepted.
        for bad in ["..", ".", "../etc", "a/b", "a\\b", "/abs", "p\\..\\q"] {
            assert!(
                matches!(
                    SourceKey::new(bad.to_owned(), "ok".to_owned()),
                    Err(StorageError::InvalidComponent {
                        name: "project",
                        ..
                    })
                ),
                "expected `{bad}` to be rejected as a path component"
            );
        }
    }

    #[test]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn validate_component_accepts_dotted_names_without_separators() {
        // A leading/embedded dot is fine; only `.`/`..`/separators are rejected.
        validate_component("project", "my.app").expect("dots inside a name are allowed");
        validate_component("project", ".hidden").expect("a leading dot is allowed");
    }
}
