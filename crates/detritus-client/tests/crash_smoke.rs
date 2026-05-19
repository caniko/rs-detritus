//! Client panic-hook and crash-shipping smoke tests.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use detritus::{
    BuildInfo, PanicHookConfig, PanicKind, SourceId, install_panic_hook, ship_pending_crashes,
};
use detritus_server::{
    RateLimitConfig, RetentionConfig, ServerConfig, TestToken, TokenStore, serve_with_shutdown,
};
use secrecy::SecretString;
use serde_json::json;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

const INSTALL_ID: &str = "11111111-1111-1111-1111-111111111111";

#[tokio::test(flavor = "multi_thread")]
async fn panic_hook_spools_chains_and_ships_next_launch() {
    let server_data = TempDir::new().expect("server data");
    let spool = TempDir::new().expect("spool dir");
    let context_file = spool.path().join("context.json");
    std::fs::write(&context_file, br#"{"tick":42}"#).expect("write context file");
    let previous_hook_ran = Arc::new(AtomicBool::new(false));
    let previous_hook_ran_for_hook = Arc::clone(&previous_hook_ran);
    std::panic::set_hook(Box::new(move |_| {
        previous_hook_ran_for_hook.store(true, Ordering::SeqCst);
    }));

    install_panic_hook(config(
        "http://127.0.0.1:1".parse().expect("offline endpoint"),
        spool.path().to_path_buf(),
        context_file,
    ))
    .expect("install hook");

    let result = std::thread::spawn(|| panic!("panic hook smoke")).join();
    assert!(result.is_err());
    assert!(previous_hook_ran.load(Ordering::SeqCst));

    let pending = entries(spool.path().join("pending"));
    assert_eq!(pending.len(), 1);
    assert!(pending[0].join("metadata.json").exists());
    assert!(pending[0].join("dump.bin").exists());
    assert!(pending[0].join("context.json").exists());

    let (addr, shutdown, handle) = spawn_server(server_data.path()).await;
    let shipped = ship_pending_crashes(
        spool.path(),
        format!("http://{addr}").parse().expect("server endpoint"),
        SecretString::from("secret-token"),
    )
    .await
    .expect("ship pending");
    assert_eq!(shipped, 1);
    assert!(entries(spool.path().join("pending")).is_empty());
    assert_eq!(entries(spool.path().join("sent")).len(), 1);

    shutdown.send(()).expect("send shutdown");
    handle
        .await
        .expect("server task")
        .expect("server exits cleanly");

    let index_dir = server_data
        .path()
        .join("crashes")
        .join("by-source")
        .join("detritus")
        .join(INSTALL_ID);
    assert_eq!(entries(index_dir).len(), 1);
}

async fn spawn_server(
    data_dir: &Path,
) -> (
    SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let config = ServerConfig {
        bind: addr,
        data_dir: data_dir.to_path_buf(),
        max_dump_bytes: 100 * 1024 * 1024,
        token_store: test_token_store(),
        rate_limit: RateLimitConfig::default(),
        retention: RetentionConfig::default(),
    };
    let handle = tokio::spawn(serve_with_shutdown(listener, config, async {
        let _ = shutdown_rx.await;
    }));
    (addr, shutdown_tx, handle)
}

fn config(endpoint: url::Url, spool_dir: PathBuf, context_file: PathBuf) -> PanicHookConfig {
    PanicHookConfig {
        endpoint,
        token: SecretString::from("secret-token"),
        source: source(),
        spool_dir,
        kind: PanicKind::PanicTarball,
        build: BuildInfo {
            git_sha: "test".to_owned(),
            profile: "test".to_owned(),
            target_triple: std::env::consts::ARCH.to_owned(),
        },
        context: json!({"test": "panic_hook_spools_chains_and_ships_next_launch"}),
        context_files: vec![context_file],
        sent_retention_days: 90,
    }
}

fn source() -> SourceId {
    SourceId {
        project: "detritus".to_owned(),
        platform: "linux".to_owned(),
        version: "0.1.0".to_owned(),
        install_id: Uuid::parse_str(INSTALL_ID).expect("install id"),
    }
}

fn entries(path: PathBuf) -> Vec<PathBuf> {
    let mut entries = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

fn test_token_store() -> TokenStore {
    TokenStore::for_tests(vec![TestToken {
        id: "detritus-test".to_owned(),
        secret_hash: test_hash(),
        project: "detritus".to_owned(),
        source_prefix: "detritus/".to_owned(),
    }])
}

fn test_hash() -> String {
    Argon2::default()
        .hash_password(
            b"secret-token",
            &SaltString::from_b64("c29tZXNhbHQ").expect("salt"),
        )
        .expect("hash token")
        .to_string()
}
