//! Native crash capture runs in a disposable subprocess because handlers are global.

#[cfg(target_os = "linux")]
fn main() {
    use detritus::{BuildInfo, PanicHookConfig, PanicKind, SourceId, install_panic_hook};
    use std::{
        process::Command,
        time::{Duration, Instant},
    };

    if let Some(spool) = std::env::var_os("DETRITUS_NATIVE_SPOOL") {
        // The reporter re-executes this binary with the same config. It exits
        // inside install_panic_hook after handling the parent's native crash.
        install_panic_hook(PanicHookConfig {
            endpoint: "http://127.0.0.1:1".parse().unwrap(),
            token: secrecy::SecretString::from("secret-token"),
            source: SourceId {
                project: "native-test".into(),
                platform: "linux".into(),
                version: "1".into(),
                install_id: uuid::Uuid::nil(),
            },
            spool_dir: spool.into(),
            kind: PanicKind::Minidump,
            build: BuildInfo {
                git_sha: "test".into(),
                profile: "test".into(),
                target_triple: "x86_64-unknown-linux-gnu".into(),
            },
            context: serde_json::json!({"native": true}),
            context_files: vec![],
            sent_retention_days: 7,
        })
        .unwrap();
        std::process::abort();
    }

    let spool = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env("DETRITUS_NATIVE_SPOOL", spool.path())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success(), "native abort must terminate the process");
            break;
        }
        if started.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("native crash capture timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    loop {
        let entries = std::fs::read_dir(spool.path().join("pending")).unwrap();
        let entry = entries
            .filter_map(Result::ok)
            .find(|entry| entry.path().join("sdk-config.json").exists());
        if let Some(entry) = entry {
            let metadata: detritus_protocol::CrashMetadata =
                serde_json::from_slice(&std::fs::read(entry.path().join("metadata.json")).unwrap())
                    .unwrap();
            assert_eq!(metadata.kind, detritus_protocol::CrashKind::Minidump);
            assert_eq!(metadata.context, serde_json::json!({"native": true}));
            let dump = std::fs::read(entry.path().join("dump.bin")).unwrap();
            assert!(dump.starts_with(b"MDMP"), "must contain an actual minidump");
            assert!(dump.len() > 32);
            assert!(
                !std::fs::read_to_string(entry.path().join("sdk-config.json"))
                    .unwrap()
                    .contains("secret-token")
            );
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "minidump was not spooled"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
