use super::*;

#[test]
fn retention_removes_only_expired_directories_and_saturates_extreme_values() {
    let dir = tempfile::tempdir().unwrap();
    let expired = dir.path().join("expired");
    let current = dir.path().join("current");
    fs::create_dir(&expired).unwrap();
    fs::create_dir(&current).unwrap();
    fs::File::open(&expired)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
        .unwrap();
    fs::write(dir.path().join("unrelated-file"), b"keep").unwrap();
    cleanup_sent(dir.path(), u64::MAX).unwrap();
    assert!(expired.exists());
    cleanup_sent(dir.path(), 7).unwrap();
    assert!(!expired.exists());
    assert!(current.exists());
    assert!(dir.path().join("unrelated-file").exists());
    cleanup_sent(dir.path(), 0).unwrap();
    assert!(!current.exists());
    assert_eq!(
        pending_entries(&dir.path().join("missing")).unwrap(),
        Vec::<PathBuf>::new()
    );
    assert!(pending_entries(&dir.path().join("unrelated-file")).is_err());
}

#[test]
fn compression_options_and_crash_url_respect_explicit_paths() {
    let config = ShipConfig::default()
        .with_dump_compression_level(1)
        .with_attachment_compression(false);
    assert_eq!(config.dump_compression_level, 1);
    assert!(!config.attachment_compression);
    assert_eq!(
        crash_url(&"http://localhost/v1/crashes".parse().unwrap()).path(),
        "/v1/crashes"
    );
    assert_eq!(
        crash_url(&"http://localhost/nested".parse().unwrap()).path(),
        "/v1/crashes"
    );
    let opaque: Url = "mailto:user@example.test".parse().unwrap();
    assert_eq!(crash_url(&opaque), opaque);
}

#[tokio::test]
async fn invalid_pending_metadata_aborts_shipping_without_moving_entry() {
    let root = tempfile::tempdir().unwrap();
    let entry = root.path().join("pending/invalid");
    fs::create_dir_all(&entry).unwrap();
    fs::write(entry.join("metadata.json"), b"bad JSON").unwrap();
    assert!(matches!(
        ship_pending_crashes(
            root.path(),
            "http://localhost:1".parse().unwrap(),
            SecretString::from("token")
        )
        .await,
        Err(ShipError::Json(_))
    ));
    assert!(entry.exists());
    assert_eq!(
        pending_entries(&root.path().join("sent")).unwrap(),
        Vec::<PathBuf>::new()
    );
    assert!(matches!(
        read_envelope(&entry.join("missing")),
        Err(ShipError::Io(_))
    ));
}
