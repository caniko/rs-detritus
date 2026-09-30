# detritus

<!-- simit:badges:start -->

[![CI](https://img.shields.io/badge/CI-managed+extra-2088ff)](.github/workflows/ci.yaml) [![Nix](https://img.shields.io/badge/Nix-drift-5277c3)](flake.nix) [![docs](https://img.shields.io/badge/docs-enabled-6f42c1)](docs) [![crates.io](https://img.shields.io/badge/crates.io-ready-f46623)](https://crates.io/crates/detritus-client)

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

- [Documentation book](https://detritus.tartanoglu.com/docs/)
- [Architecture](docs/src/concepts/architecture.md)
- [Operations](docs/src/deployment/operations.md)
- [Storage layout](docs/src/concepts/storage.md)

## CI

- [CI status](https://github.com/caniko/rs-detritus/actions/workflows/ci.yaml)
- On every branch push and PR, aggregate CI checks formatting, the Nix flake,
  all-feature and no-default-feature tests/Clippy, warning-free docs, Rust 1.88
  compatibility, dependency policy, and per-crate coverage. The dedicated
  Coverage workflow also checks isolated features and uploads HTML/JSON reports.
- Signed tags `X.Y.Z` trigger dependency-ordered publication to crates.io using
  the `CRATES_IO_API_TOKEN` GitHub Actions secret. Each archive is verified and
  dry-run checked before upload.
- Release procedure: [RELEASING.md](RELEASING.md)

## Testing and coverage

Run the complete suite, including doctests, with:

```sh
nix develop -c cargo test --workspace --all-features --locked -j 4
```

The dedicated [coverage workflow](.github/workflows/coverage.yaml) enforces at
least **90% line coverage independently for each crate**. Run the same gate locally:

```sh
nix develop -c bash scripts/coverage.sh
```

JSON reports are written to `target/coverage/<crate>.json`; browsable reports are
at `target/coverage/<crate>/html/index.html`. The gate measures handwritten
production code, including `detritusd`. Generated protobuf bindings, build
scripts, examples, and test code are excluded. Inline unit-test functions use
`#[cfg_attr(coverage_nightly, coverage(off))]`; new test modules belong in
`src/<module>/tests.rs` or `tests/` so they stay outside the coverage denominator.
The script starts with a clean coverage build to avoid stale instrumentation.

Tests cover offline replay and upload failures, panic artifacts, native Linux
minidumps in subprocesses, authentication against legacy Argon2 hashes, malformed
multipart requests, schema validation, concurrent deduplication, rate limits,
retention, and server startup/shutdown. Isolated feature checks run in CI:

```sh
nix develop -c cargo test -p detritus-client --no-default-features --locked -j 4
nix develop -c cargo test -p detritus-protocol --no-default-features --locked -j 4
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
