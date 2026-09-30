use super::*;

fn config(dir: &Path) -> PanicHookConfig {
    PanicHookConfig {
        endpoint: "http://localhost:4317".parse().unwrap(),
        token: SecretString::from("never-persist-this-secret"),
        source: SourceId {
            project: "tests".into(),
            platform: "linux".into(),
            version: "1".into(),
            install_id: Uuid::nil(),
        },
        spool_dir: dir.to_owned(),
        kind: PanicKind::Minidump,
        build: BuildInfo {
            git_sha: "abc".into(),
            profile: "test".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
        },
        context: json!({"turn": 42}),
        context_files: vec![],
        sent_retention_days: 7,
    }
}

#[test]
fn tarball_contains_readable_panic_backtrace_and_environment() {
    let bytes = panic_tarball("panic ☃").unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
    let mut entries = std::collections::BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().into_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        entries.insert(name, bytes);
    }
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[Path::new("panic.txt")], "panic ☃".as_bytes());
    assert_ne!(entries[Path::new("backtrace.txt")].as_slice(), b"");
    let env: serde_json::Value = serde_json::from_slice(&entries[Path::new("env.json")]).unwrap();
    assert_eq!(env["os"], std::env::consts::OS);
    assert_eq!(env["arch"], std::env::consts::ARCH);
}

#[test]
fn attachments_report_lengths_and_propagate_missing_file_errors() {
    let root = tempfile::tempdir().unwrap();
    let dest = root.path().join("dest");
    fs::create_dir(&dest).unwrap();
    let path = root.path().join("context.json");
    fs::write(&path, b"{\"turn\":42}").unwrap();
    let manifest = copy_context_file(2, &path, &dest).unwrap();
    assert_eq!(manifest.key, "context-2");
    assert_eq!(manifest.len, 11);
    assert_eq!(manifest.filename.as_deref(), Some("context.json"));
    assert_eq!(
        fs::read(dest.join("context.json")).unwrap(),
        b"{\"turn\":42}"
    );
    assert_eq!(
        copy_context_file(0, &root.path().join("missing"), &dest)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn hook_installation_reports_invalid_spool_root_without_replacing_hook() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    fs::write(&file, b"file").unwrap();
    assert!(matches!(
        install_panic_hook(config(&file)),
        Err(PanicHookError::Io(_))
    ));
}

#[cfg(all(feature = "minidump", not(target_os = "android")))]
#[test]
fn native_dump_callback_persists_raw_dump_context_and_token_free_config() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("context.json");
    fs::write(&path, b"{}").unwrap();
    let mut config = config(root.path());
    config.context_files.push(path);
    write_pending_minidump(&config, b"MDMP-test").unwrap();
    let entry = fs::read_dir(root.path().join("pending"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(entry.join("dump.bin")).unwrap(), b"MDMP-test");
    let metadata: CrashMetadata =
        serde_json::from_slice(&fs::read(entry.join("metadata.json")).unwrap()).unwrap();
    assert_eq!(metadata.kind, CrashKind::Minidump);
    assert_eq!(metadata.context, json!({"turn": 42}));
    assert_eq!(metadata.attachments.len(), 1);
    let raw = fs::read_to_string(entry.join("sdk-config.json")).unwrap();
    assert!(!raw.contains("never-persist-this-secret"));
    let stored: StoredUploadConfig = serde_json::from_str(&raw).unwrap();
    assert_eq!(stored.sent_retention_days, 7);
    assert_eq!(stored.endpoint, config.endpoint.as_str());
}
