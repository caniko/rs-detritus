# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-30

### Added

- Opt-in gzip/zstd OTLP message compression, pooled gRPC connections, HTTPS
  trust roots, and custom CA/mutual-TLS log transport configuration.
- Reusable `CrashShipper` with caller-configured HTTP clients and bounded default
  connect/read/request timeouts. Convenience shippers reuse a pool per scan.
- Shared offline JSON Schema reference resources, optional format validation,
  and masked diagnostics with instance and schema locations.
- HTTP gzip/zstd request and response negotiation, request IDs in responses and
  tracing spans, and sensitive authorization metadata.
- `detritusd hash-token` for randomly salted Argon2id hashes from stdin, a version
  flag, and graceful SIGTERM shutdown on Unix.
- Curated OTLP exports for resource entity references and partial-success responses.
- Independent 90% production-line coverage gates and downloadable HTML/JSON
  reports for all three crates, plus isolated feature checks.
- Regression tests for offline replay, timeout persistence, native Linux crash
  capture, malformed uploads, legacy authentication, concurrent deduplication,
  rate-limit isolation, schema errors, and graceful CLI/writer shutdown.
- Stored-config crash shipping APIs for flushing spooled crashes with endpoints
  and sent-entry retention recovered per entry while keeping the bearer token
  caller-supplied and off disk.

### Changed

- Upgrade Cargo dependencies to current releases, including Tonic/Prost,
  Argon2, jsonschema, Reqwest, TOML, SHA-2, tower-http, and zstd, while retaining
  the Rust 1.88 MSRV and existing wire/spool formats.
- Refresh Nix inputs and the Harbor pin; include matching LLVM coverage tools in
  the nightly development shell. Derive the MSRV hook from Cargo metadata.
- Nix outputs target x86_64 Linux, aarch64 Linux, and aarch64 macOS; the upgraded
  Nixpkgs 26.11 input has retired x86_64 macOS support.
- Move the Plinth flake input to GitHub and refresh locked dependencies.
- GitHub Actions uses Nix-based aggregate workspace CI and a signed-tag,
  dependency-ordered publisher. Coverage is enforced before publication;
  workflow concurrency no longer cancels sibling checks or partial publishes.
- Nix flake inputs `rust-overlay`, `treefmt-nix`, and `git-hooks` now follow
  the workspace `nixpkgs` input for consistency.

### Fixed

- Native crash reporter subprocesses retain application arguments so they can
  reconstruct command-line configuration.
- The HTTP body limit applies to decompressed requests, and the first periodic
  log export waits the configured batching interval.
- Retention preserves attachment blobs referenced by live crash indexes.
- Exporter shutdown flushes queued records and terminates when the last layer
  is dropped; all export attempts honor the configured timeout and persist
  timed-out batches for replay.

### Removed

- Removed `id-arena`, `leb128fmt`, `unicode-xid`, `wasm-encoder`,
  `wasm-metadata`, `wasmparser`,
  `wasip3`, `wit-bindgen 0.46.0`, `wit-bindgen-core`, `wit-bindgen-rust`,
  `wit-bindgen-rust-macro`, `wit-component`, `wit-parser` transient
  dependency trees.

### Migration

- Use `0.2` for all Detritus crate dependencies. Public OTLP types now use
  Tonic/Prost 0.14; consumers naming those types must upgrade their matching
  dependencies. `ShipError::Http` wraps Reqwest 0.13 errors.
- The protocol remains `PROTOCOL_VERSION == 1`, and existing wire, crash spool,
  and stored-data formats remain supported. Legacy Argon2 0.5 password hashes
  continue to authenticate.
- Nix users on x86_64 macOS need a supported platform or an older Nixpkgs input.
  Rust consumers retain the Rust 1.88 minimum supported version.

## [0.1.0] - 2026-05-22

### Added

- Per-crate `README.md` files for `detritus-protocol`, `detritus-client`,
  and `detritus-server`, including quick-start code, feature flags,
  compatibility notes, related crates, documentation links, and examples.
- Runnable example programs for every publishable crate:
  `crash_envelope`, `install_layer`, and `embed_server`.
- Full rustdoc coverage for the v0.1.0 public API surface.
- Workspace MSRV declaration with `rust-version = "1.88"` inherited by each
  publishable crate.
- Workspace lint policy enforcing `missing_docs`, `unsafe_code = forbid`,
  and strict clippy groups at warning level.
- crates.io publish metadata for every publishable crate, including
  descriptions, keywords, categories, homepage, repository, documentation, and
  per-crate README paths.
- Workspace README crate overview for the published package set.
- Simit-managed formatting flake check, with repository files normalized to the
  generated formatter before publication.

### Changed

- License consolidated from the dual `MIT OR Apache-2.0` declaration to
  `Apache-2.0` only. The canonical Apache-2.0 text is now shipped at `LICENSE`
  and in each published crate package root (previously absent; the dual
  declaration had no underlying text files). Single-author project across
  three commits; consolidating the offered license options is uncontroversial.
- Internal path dependencies between Detritus crates now carry both `path = "..."`
  and `version = "0.1.0"` so `cargo publish` resolves them against crates.io.
- `detritus-protocol` now exposes generated protobuf bindings through the
  curated `otlp::{common, resource, logs}` facade instead of committing the
  generated package tree as the intended public surface.
- `detritus-server` now exposes only embedding configuration, authentication
  configuration, rate-limit/retention configuration, and serving entry points
  from the crate root. Handler, metric, storage, middleware, and writer modules
  are internal implementation details.

### Removed

- Per-file `#![warn(missing_docs)]` configuration in favor of the workspace lint
  policy.

[Unreleased]: https://github.com/caniko/rs-detritus/compare/0.2.0...HEAD
[0.2.0]: https://github.com/caniko/rs-detritus/compare/0.1.0...0.2.0
[0.1.0]: https://github.com/caniko/rs-detritus/releases/tag/0.1.0
