use super::*;
use detritus_protocol::otlp::logs::{LogRecord, ResourceLogs, ScopeLogs};

#[test]
fn persisted_batches_roundtrip_and_ignore_unrelated_files() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("queue");
    assert_eq!(pending_log_batches(&dir).unwrap(), Vec::<PathBuf>::new());
    let request = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            scope_logs: vec![ScopeLogs {
                log_records: vec![LogRecord {
                    time_unix_nano: 42,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    let a = write_log_batch(&dir, &request).unwrap();
    let b = write_log_batch(&dir, &request).unwrap();
    fs::write(dir.join("metadata.json"), b"{}").unwrap();
    let paths = pending_log_batches(&dir).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths[0] < paths[1]);
    assert_ne!(a, b);
    assert_eq!(read_log_batch(&a).unwrap(), request);
    let lock = SpoolLock::acquire(&dir).unwrap();
    let competitor = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(".lock"))
        .unwrap();
    assert!(FileExt::try_lock_exclusive(&competitor).is_err());
    drop(lock);
    FileExt::try_lock_exclusive(&competitor).unwrap();
}

#[test]
fn malformed_or_missing_batches_and_non_directory_roots_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        read_log_batch(&dir.path().join("missing"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    let file = dir.path().join("bad.protobuf");
    fs::write(&file, [0xff]).unwrap();
    assert_eq!(
        read_log_batch(&file).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert!(pending_log_batches(&file).is_err());
    assert!(write_log_batch(&file, &ExportLogsServiceRequest::default()).is_err());
    assert!(SpoolLock::acquire(&file).is_err());
}
