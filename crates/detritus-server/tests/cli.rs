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
            .build()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Ok(response) = client.get(format!("http://{addr}/healthz")).send().await {
                assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
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
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("openmetrics")
        );
        assert!(response.text().await.unwrap().contains("detritus_"));
        let status = Command::new("kill")
            .args(["-INT", &daemon.0.id().to_string()])
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
