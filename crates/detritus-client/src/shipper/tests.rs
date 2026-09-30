use super::*;

#[tokio::test]
async fn successive_scans_reuse_one_http1_connection_with_streamed_responses() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let root = tempfile::tempdir().unwrap();
    let entry = root.path().join("pending/first");
    fs::create_dir_all(&entry).unwrap();
    let metadata = CrashMetadata::new(
        detritus_protocol::SourceId {
            project: "tests".into(),
            platform: "linux".into(),
            version: "1".into(),
            install_id: uuid::Uuid::nil(),
        },
        chrono::Utc::now(),
        detritus_protocol::CrashKind::PanicTarball,
        detritus_protocol::BuildInfo {
            git_sha: "test".into(),
            profile: "test".into(),
            target_triple: "test".into(),
        },
        serde_json::json!({}),
    );
    let encoded = serde_json::to_vec(&metadata).unwrap();
    fs::write(entry.join("metadata.json"), &encoded).unwrap();
    fs::write(entry.join("dump.bin"), b"dump").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint: Url = format!("http://{}", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let server = tokio::spawn(async move {
        // A second TCP connection is deliberately never accepted.
        let (socket, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(socket);
        for _ in 0..2 {
            let mut length = None;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':')
                    && key.eq_ignore_ascii_case("content-length")
                {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; length.unwrap()];
            reader.read_exact(&mut body).await.unwrap();
            reader
                .get_mut()
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 8\r\n\r\n")
                .await
                .unwrap();
            // Deliver the body after headers to ensure the client must drain it.
            tokio::time::sleep(Duration::from_millis(20)).await;
            reader.get_mut().write_all(b"accepted").await.unwrap();
        }
    });
    crate::install_default_crypto_provider();
    let client = reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(2))
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    let shipper = CrashShipper::with_client(client, SecretString::from("token"));
    assert_eq!(
        shipper
            .ship_pending(root.path(), endpoint.clone())
            .await
            .unwrap(),
        1
    );
    let second = root.path().join("pending/second");
    fs::create_dir_all(&second).unwrap();
    fs::write(second.join("metadata.json"), encoded).unwrap();
    fs::write(second.join("dump.bin"), b"second dump").unwrap();
    assert_eq!(
        shipper.ship_pending(root.path(), endpoint).await.unwrap(),
        1
    );
    server.await.unwrap();
    assert_eq!(pending_entries(&root.path().join("sent")).unwrap().len(), 2);
}

#[tokio::test]
async fn custom_transport_timeout_keeps_pending_crashes_and_redacts_token() {
    let root = tempfile::tempdir().unwrap();
    let entry = root.path().join("pending/stalled");
    fs::create_dir_all(&entry).unwrap();
    let metadata = CrashMetadata::new(
        detritus_protocol::SourceId {
            project: "tests".into(),
            platform: "linux".into(),
            version: "1".into(),
            install_id: uuid::Uuid::nil(),
        },
        chrono::Utc::now(),
        detritus_protocol::CrashKind::PanicTarball,
        detritus_protocol::BuildInfo {
            git_sha: "test".into(),
            profile: "test".into(),
            target_triple: "test".into(),
        },
        serde_json::json!({}),
    );
    fs::write(
        entry.join("metadata.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    fs::write(entry.join("dump.bin"), b"dump").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    crate::install_default_crypto_provider();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    let shipper = CrashShipper::with_client(client, SecretString::from("private-token"))
        .with_config(ShipConfig::default().with_dump_compression_level(1));
    assert!(!format!("{shipper:?}").contains("private-token"));
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        shipper.ship_pending(
            root.path(),
            format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(ShipError::Http(error)) if error.is_timeout()));
    assert!(entry.exists());
    assert_eq!(
        pending_entries(&root.path().join("sent")).unwrap(),
        Vec::<PathBuf>::new()
    );
    let shipper = CrashShipper::new(SecretString::from("private\ninvalid")).unwrap();
    assert!(matches!(
        shipper
            .ship_pending(root.path(), "http://localhost:1".parse().unwrap())
            .await,
        Err(ShipError::InvalidToken)
    ));
}

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
