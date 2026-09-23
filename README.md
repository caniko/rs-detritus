# detritus

<!-- simit:badges:start -->

![CI](https://img.shields.io/badge/CI-managed-2088ff) [![Nix](https://img.shields.io/badge/Nix-managed-5277c3)](flake.nix) [![docs](https://img.shields.io/badge/docs-enabled-6f42c1)](docs) [![crates.io](https://img.shields.io/badge/crates.io-ready-f46623)](https://crates.io/crates/detritus-client)

<!-- simit:badges:end -->

detritus is a lightweight crash and log receiver for small Rust projects.

The v1 receiver exposes two ingestion endpoints:

- `/v1/logs` via OTLP/gRPC by implementing `opentelemetry.proto.collector.logs.v1.LogsService.Export`.
- `/v1/crashes` via HTTP/2 multipart upload with JSON metadata and a binary crash artifact.

## Crates

- [`detritus-protocol`](crates/detritus-protocol) - shared wire types, crash schema, and curated OTLP log bindings.
- [`detritus-client`](crates/detritus-client) - tracing layer, panic hook, and offline crash shipper for Rust applications.
- [`detritus-server`](crates/detritus-server) - `detritusd` receiver binary and embeddable server entry points.

## Documentation

- [Documentation book](https://caniko.codeberg.page/rs-detritus/)
- [Architecture](docs/src/concepts/architecture.md)
- [Operations](docs/src/deployment/operations.md)
- [Storage layout](docs/src/concepts/storage.md)

## CI

- [CI status](https://codeberg.org/caniko/rs-detritus/actions/workflows/ci.yml)
- On every push to `trunk` and every PR, CI runs fmt, clippy, check, test, doc, an MSRV check, `cargo-audit`, `cargo-deny`, and a `cargo publish --dry-run` for `detritus-protocol`.
- On every tag `vX.Y.Z`, the release workflow runs the real `cargo publish` to crates.io using the `CRATES_IO_API_TOKEN` repo secret.
- Release procedure: [RELEASING.md](RELEASING.md)

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
