# detritus-server

[![Crates.io](https://img.shields.io/crates/v/detritus-server.svg)](https://crates.io/crates/detritus-server)
[![Documentation](https://docs.rs/detritus-server/badge.svg)](https://docs.rs/detritus-server)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/LICENSE)

`detritus-server` is the Detritus telemetry and crash ingestion receiver.
The package installs the `detritusd` binary for operators and exposes a small library surface for
embedding the receiver in tests, local tooling, or custom orchestration.
The server accepts OTLP/gRPC log exports and HTTP/2 multipart crash uploads, writes append-only
NDJSON logs, stores crash blobs by content hash, and maintains source indexes for crash metadata.

## Quick start

Install and run the receiver binary:

```sh
cargo install detritus-server
detritusd --bind 127.0.0.1:4317 --data-dir ./detritus-data --tokens-config ./tokens.toml
```

Embed the server when a test or harness needs its own listener:

```rust,no_run
use detritus_server::{
    RateLimitConfig, RetentionConfig, ServerConfig, TestToken, TokenStore, serve_with_shutdown,
};
use tokio::net::TcpListener;

# async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
let listener = TcpListener::bind("127.0.0.1:0").await?;
let bind = listener.local_addr()?;
let config = ServerConfig {
    bind,
    data_dir: std::env::temp_dir().join("detritus"),
    max_dump_bytes: 100 * 1024 * 1024,
    token_store: TokenStore::for_tests(Vec::<TestToken>::new()),
    rate_limit: RateLimitConfig::default(),
    retention: RetentionConfig::default(),
};

serve_with_shutdown(listener, config, async {}).await?;
# Ok(())
# }
```

Production deployments should load real token configuration with `load_security_config` rather
than constructing a test token store.

## Examples

- [`embed_server`](examples/embed_server.rs) - bind an embedded receiver on a kernel-assigned local port and shut it down.

Run with:

```sh
cargo run --example embed_server -p detritus-server
```

## Feature flags

This crate currently has no optional Cargo features.
The default package includes both the library and the `detritusd` binary.
Compression, decompression, HTTP/2, Prometheus metrics, authentication, rate limiting, and retention
janitor support are part of the v0.1.0 receiver.

## Authentication

Every ingestion request except `/healthz` and `/metrics` requires a bearer token.
Token files are TOML documents loaded by `load_security_config`.
Tokens store Argon2 encoded hashes, a project name, and a canonical source prefix.
The server checks tokens in constant time where it compares source components.

## Storage

Logs are written as daily NDJSON files under the configured data directory.
Crash dumps and attachments are content-addressed by SHA-256, so duplicate uploads can share blobs.
Crash metadata is stored as source-indexed JSON that points at those blobs.
See the storage layout documentation for the exact on-disk paths.

## Operations

`detritusd` exposes `/healthz` for liveness and `/metrics` in OpenMetrics text format.
The retention janitor deletes old logs, expired crash indexes, and unreferenced blobs according to
`RetentionConfig`.
See `docs/src/deployment/operations.md` in the workspace for deployment notes, token-file format, and runtime
expectations.

## Compatibility

- Detritus protocol version: v0.1.0 / `PROTOCOL_VERSION == 1`.
- Client compatibility: `detritus-client` v0.1.0.
- MSRV: Rust 1.88.
- Edition: Rust 2024.
- Binary: `detritusd`.

## Related crates

- [detritus-client](https://crates.io/crates/detritus-client) - client SDK for tracing and crash capture.
- [detritus-protocol](https://crates.io/crates/detritus-protocol) - shared wire types and OTLP log facade.
- [detritus-server](https://crates.io/crates/detritus-server) - this receiver binary and embeddable server.

## Documentation

- [API docs](https://docs.rs/detritus-server)
- [Workspace](https://codeberg.org/caniko/rs-detritus)
- [Documentation book](https://caniko.codeberg.page/rs-detritus/)
- [Architecture](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/docs/src/concepts/architecture.md)
- [Operations](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/docs/src/deployment/operations.md)
- [Storage layout](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/docs/src/concepts/storage.md)

## License

Licensed under the [Apache License, Version 2.0](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/LICENSE).
