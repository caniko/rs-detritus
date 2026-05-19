use std::{
    fs, io,
    path::{Path, PathBuf},
};

use fs2::FileExt;
use prost::Message;
use uuid::Uuid;

use detritus_protocol::otlp::logs::ExportLogsServiceRequest;

pub(crate) fn ensure_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

pub(crate) fn write_log_batch(
    dir: &Path,
    request: &ExportLogsServiceRequest,
) -> io::Result<PathBuf> {
    ensure_dir(dir)?;
    let path = dir.join(format!("log-{}.protobuf", Uuid::new_v4()));
    let mut bytes = Vec::with_capacity(request.encoded_len());
    request
        .encode(&mut bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(&path, bytes)?;
    Ok(path)
}

pub(crate) fn read_log_batch(path: &Path) -> io::Result<ExportLogsServiceRequest> {
    let bytes = fs::read(path)?;
    ExportLogsServiceRequest::decode(bytes.as_slice())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

pub(crate) fn pending_log_batches(dir: &Path) -> io::Result<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(entries) => {
            let mut paths = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "protobuf"))
                .collect::<Vec<_>>();
            paths.sort();
            Ok(paths)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub(crate) struct SpoolLock {
    file: fs::File,
}

impl SpoolLock {
    pub(crate) fn acquire(dir: &Path) -> io::Result<Self> {
        ensure_dir(dir)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(dir.join(".lock"))?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }
}

impl Drop for SpoolLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}
