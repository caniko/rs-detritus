//! Exercise real CLI configuration, HTTP health/metrics, and signal shutdown.
#![cfg(unix)]

use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn cli_serves_health_metrics_and_shuts_down_in_both_log_formats() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();
    for format in ["json", "pretty"] {
        let dir = tempfile::tempdir().unwrap();
        let tokens = format!(
            "[[token]]\nid = 'test'\nsecret = '{}'\nproject = 'tests'\nsource_prefix = 'tests/'\n",
            include_str!("fixtures/legacy-token.phc").trim()
        );
        std::fs::write(dir.path().join("tokens.toml"), tokens).unwrap();
        // Reserve an available port then relinquish it immediately before launch.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let data_dir = if format == "json" {
            dir.path().join("data")
        } else {
            "data".into()
        };
        let mut daemon = Daemon(
            Command::new(env!("CARGO_BIN_EXE_detritusd"))
                .current_dir(dir.path())
                .args([
                    "--bind",
                    &addr.to_string(),
                    "--tokens-config",
                    "tokens.toml",
                    "--log-format",
                    format,
                    "--data-dir",
                ])
                .arg(data_dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .no_zstd()
            .build()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Ok(response) = client.get(format!("http://{addr}/healthz")).send().await {
                assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
                uuid::Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap())
                    .unwrap();
                break;
            }
            assert!(
                daemon.0.try_wait().unwrap().is_none(),
                "daemon exited during startup"
            );
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "startup timed out"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let response = client
            .get(format!("http://{addr}/metrics"))
            .header("x-request-id", "diagnostic-123")
            .header("accept-encoding", "zstd")
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(response.headers()["x-request-id"], "diagnostic-123");
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("openmetrics")
        );
        // Auto decoding is disabled to prove actual zstd response negotiation.
        assert_eq!(response.headers()["content-encoding"], "zstd");
        let bytes = response.bytes().await.unwrap();
        let text = String::from_utf8(zstd::stream::decode_all(bytes.as_ref()).unwrap()).unwrap();
        assert!(text.contains("detritus_"));
        let status = Command::new("kill")
            .args([
                if format == "json" { "-INT" } else { "-TERM" },
                &daemon.0.id().to_string(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        let started = Instant::now();
        loop {
            if let Some(status) = daemon.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "shutdown timed out"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(dir.path().join("data/logs").is_dir());
    }
}

#[test]
fn cli_hashes_stdin_tokens_with_unique_salts_without_echoing_secrets() {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    use std::io::Write;
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_detritusd"))
            .arg("hash-token")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"  new-secret-token  \r\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stderr, Vec::<u8>::new());
        let hash = String::from_utf8(output.stdout).unwrap();
        assert!(!hash.contains("new-secret-token"));
        let parsed = PasswordHash::new(hash.trim()).unwrap();
        Argon2::default()
            .verify_password(b"  new-secret-token  ", &parsed)
            .unwrap();
        assert!(
            Argon2::default()
                .verify_password(b"wrong", &parsed)
                .is_err()
        );
        hashes.push(hash);
    }
    assert_ne!(hashes[0], hashes[1]);
    let output = Command::new(env!("CARGO_BIN_EXE_detritusd"))
        .arg("hash-token")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("empty"));
}

#[test]
fn cli_reports_invalid_arguments_and_missing_security_config() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_detritusd"))
        .args(["--bind", "invalid", "--tokens-config", "missing.toml"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid"));
    let output = Command::new(env!("CARGO_BIN_EXE_detritusd"))
        .current_dir(dir.path())
        .args(["--tokens-config", "missing.toml"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No such file"));
}
