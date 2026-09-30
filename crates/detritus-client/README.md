# detritus-client

[![Crates.io](https://img.shields.io/crates/v/detritus-client.svg)](https://crates.io/crates/detritus-client)
[![Documentation](https://docs.rs/detritus-client/badge.svg)](https://docs.rs/detritus-client)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/caniko/rs-detritus/blob/trunk/LICENSE)

`detritus-client` is the Rust SDK for sending tracing events and panic artifacts to a Detritus
receiver.
It provides a `tracing-subscriber` layer for OTLP log export, a process-wide panic hook that
spools crash artifacts without doing network I/O in the hook, and an offline shipper that uploads
pending crash entries on a later launch.
The public API is intentionally small so application code can install Detritus without depending
on receiver internals.

## Quick start

Install the tracing layer on a Tokio runtime:

```rust,no_run
use std::{path::PathBuf, time::Duration};
use detritus::{Layer, SourceId};
use secrecy::SecretString;
use tracing_subscriber::{Registry, layer::SubscriberExt};
use url::Url;
use uuid::Uuid;

let source = SourceId {
    project: "detritus".to_owned(),
    platform: "linux".to_owned(),
    version: "0.1.0".to_owned(),
    install_id: Uuid::nil(),
};

let layer = Layer::builder()
    .endpoint(Url::parse("http://127.0.0.1:4317").unwrap())
    .token(SecretString::from("secret-token"))
    .source(source)
    .queue_dir(PathBuf::from("observability-spool/logs"))
    .flush_interval(Duration::from_secs(5))
    .build()
    .unwrap();

let subscriber = Registry::default().with(layer);
tracing::subscriber::set_global_default(subscriber).unwrap();
```

Install crash capture separately if you want panic artifacts:

```rust,no_run
use detritus::{BuildInfo, PanicHookConfig, PanicKind, SourceId, install_panic_hook};
use secrecy::SecretString;
use serde_json::json;
use url::Url;
use uuid::Uuid;

let source = SourceId {
    project: "detritus".to_owned(),
    platform: "linux".to_owned(),
    version: "0.1.0".to_owned(),
    install_id: Uuid::nil(),
};

install_panic_hook(PanicHookConfig {
    endpoint: Url::parse("http://127.0.0.1:4317").unwrap(),
    token: SecretString::from("secret-token"),
    source,
    spool_dir: "observability-spool/crashes".into(),
    kind: PanicKind::PanicTarball,
    build: BuildInfo {
        git_sha: "unknown".to_owned(),
        profile: "release".to_owned(),
        target_triple: "x86_64-unknown-linux-gnu".to_owned(),
    },
    context: json!({}),
    context_files: Vec::new(),
    sent_retention_days: 90,
}).unwrap();
```

## Examples

- [`install_layer`](examples/install_layer.rs) - install the tracing layer and emit example events.

Run with:

```sh
cargo run --example install_layer -p detritus-client
```

## Feature flags

| Feature    | Default | Effect                                                                             |
| ---------- | ------- | ---------------------------------------------------------------------------------- |
| `minidump` | yes     | Enables native minidump support on non-Android targets through `minidumper-child`. |

Disabling default features keeps the tracing layer, panic tarball capture, and offline shipper.
Native minidumps are not used on Android; use `PanicKind::PanicTarball` there.

## Offline spooling

The tracing layer writes failed OTLP batches into the configured queue directory.
The panic hook writes crash entries under `pending/` and never performs network I/O while handling
a panic.
Call `ship_pending_crashes` during process startup to upload crash entries and move successful
uploads into `sent/`.
Each spool directory uses a filesystem lock to avoid concurrent scans by two processes.

## Transport options

The log exporter reuses its HTTP/2 connection across batches and offline replay.
HTTPS endpoints use system trust roots. Use `LayerBuilder::tls_config` with
`ClientTlsConfig`, `Certificate`, and `Identity` (re-exported by `detritus`) for
a private CA or mutual TLS.

Use `.compression(CompressionEncoding::Gzip)` or
`.compression(CompressionEncoding::Zstd)` on the layer builder to compress log
messages. Requests are uncompressed by default; upgraded receivers accept both
algorithms and negotiate compressed responses.

`CrashShipper::new(token)` creates a reusable uploader with connect/read/total
timeouts of 10/30/60 seconds. Its `ship_pending` and `ship_using_stored_config`
methods share the connection pool across scans. `CrashShipper::with_client`
accepts a Reqwest client configured with custom timeouts, TLS, or proxy settings.
See [Offline Shipping](https://detritus.tartanoglu.com/docs/guides/offline-shipping.html)
for examples.

Native minidump reporters re-execute the application with its original arguments
so command-line configuration is available in the reporter process too.

## Compatibility

- Detritus protocol version: `PROTOCOL_VERSION == 1`.
- Receiver compatibility: `detritus-server` 0.1 and 0.2 with default uncompressed logs;
  opt-in log compression requires a 0.2 receiver.
- Public OTLP types use Tonic/Prost 0.14; custom HTTP clients use Reqwest 0.13.
- MSRV: Rust 1.88.
- Edition: Rust 2024.

## Related crates

- [detritus-client](https://crates.io/crates/detritus-client) - this client SDK.
- [detritus-protocol](https://crates.io/crates/detritus-protocol) - shared wire types and OTLP log facade.
- [detritus-server](https://crates.io/crates/detritus-server) - receiver binary and embeddable server.

## Documentation

- [API docs](https://docs.rs/detritus-client)
- [Workspace](https://github.com/caniko/rs-detritus)
- [Documentation book](https://detritus.tartanoglu.com/docs/)
- [Architecture](https://github.com/caniko/rs-detritus/blob/trunk/docs/src/concepts/architecture.md)
- [Operations](https://github.com/caniko/rs-detritus/blob/trunk/docs/src/deployment/operations.md)

## License

Licensed under the [Apache License, Version 2.0](https://github.com/caniko/rs-detritus/blob/trunk/LICENSE).
