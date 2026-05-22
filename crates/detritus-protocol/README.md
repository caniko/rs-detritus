# detritus-protocol

[![Crates.io](https://img.shields.io/crates/v/detritus-protocol.svg)](https://crates.io/crates/detritus-protocol)
[![Documentation](https://docs.rs/detritus-protocol/badge.svg)](https://docs.rs/detritus-protocol)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/LICENSE)

`detritus-protocol` contains the shared wire types for Detritus telemetry and crash ingestion.
It is the contract used by `detritus-client` when producing payloads and by `detritus-server`
when validating and storing them.
The crate covers source identity, crash metadata, crash envelopes, multipart crash helpers,
and a curated OpenTelemetry Protocol logs facade.
Version 0.1.0 intentionally covers OTLP logs only; traces and metrics are not part of this
published surface yet.

## Quick start

Use the protocol version when checking compatibility and construct crash metadata with the
current schema version:

```rust
use chrono::Utc;
use detritus_protocol::{BuildInfo, CrashKind, CrashMetadata, PROTOCOL_VERSION, SourceId};
use serde_json::json;
use uuid::Uuid;

let source = SourceId {
    project: "detritus".to_owned(),
    platform: "linux".to_owned(),
    version: "0.1.0".to_owned(),
    install_id: Uuid::nil(),
};

let metadata = CrashMetadata::new(
    source,
    Utc::now(),
    CrashKind::PanicTarball,
    BuildInfo {
        git_sha: "unknown".to_owned(),
        profile: "release".to_owned(),
        target_triple: "x86_64-unknown-linux-gnu".to_owned(),
    },
    json!({"tick": 42}),
);

assert_eq!(metadata.schema_version, PROTOCOL_VERSION);
```

With the default `multipart` feature enabled, `CrashEnvelope` can also encode and decode
multipart crash uploads through async readers and writers.

## Examples

- [`crash_envelope`](examples/crash_envelope.rs) - construct a crash envelope and serialize it as a multipart upload.

Run with:

```sh
cargo run --example crash_envelope -p detritus-protocol
```

## Feature flags

| Feature     | Default | Effect                                                                                  |
| ----------- | ------- | --------------------------------------------------------------------------------------- |
| `multipart` | yes     | Enables `CrashEnvelope` multipart read/write helpers and `multipart::DEFAULT_BOUNDARY`. |

The base schema types and OTLP log facade are always available.
Features are additive: disabling default features removes only optional multipart helpers.

## Compatibility

- Detritus protocol version: `PROTOCOL_VERSION == 1`.
- Detritus crate version: v0.1.0.
- OTLP scope: logs export requests, responses, records, resources, and service stubs.
- MSRV: Rust 1.88.
- Edition: Rust 2024.

## Public surface

The stable v0.1.0 surface is the root constants, crash schema types, `SourceId`, the curated
`otlp::{common, resource, logs}` facade, and feature-gated multipart helpers.
Generated protobuf package modules are intentionally hidden behind the curated facade.
Downstream code should prefer root re-exports such as `detritus_protocol::CrashEnvelope` and
`detritus_protocol::SourceId`.

## Related crates

- [detritus-client](https://crates.io/crates/detritus-client) - client SDK for tracing and crash capture.
- [detritus-protocol](https://crates.io/crates/detritus-protocol) - shared wire types and OTLP log facade.
- [detritus-server](https://crates.io/crates/detritus-server) - receiver binary and embeddable server.

## Documentation

- [API docs](https://docs.rs/detritus-protocol)
- [Workspace](https://codeberg.org/caniko/rs-detritus)
- [Documentation book](https://caniko.codeberg.page/rs-detritus/)
- [Architecture](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/docs/src/concepts/architecture.md)
- [Storage layout](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/docs/src/concepts/storage.md)

## License

Licensed under the [Apache License, Version 2.0](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/LICENSE).
