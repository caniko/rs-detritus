# RFC: Detritus crates.io publish readiness

Date: 2026-05-19
Status: ADOPTED - feeds Phases 02-07

This audit is based on committed `HEAD` (`b23ab20 Move operations docs into detritus`). Local WIP in the checkout was intentionally excluded.

External facts checked on 2026-05-19:

- crates.io package API returned "does not exist" for `detritus-protocol`, `detritus-client`, and `detritus-server`.
- crates.io category API was used for category slugs. The current taxonomy exposes `development-tools` and `web-programming`; it does not expose `development-tools::debugging` or `web-programming::http-server`.
- The Apache-2.0 SPDX identifier matches the nixpkgs `asl20` license attribute (shortName `Apache-2.0`).
- The Rust 1.85.0 release announcement states that Rust 2024 was stabilized in Rust 1.85.0.

## Workspace inventory

| Crate | Package name | Lib name | Version | Edition | Current license | Source modules / LOC | Tests | Bin | `missing_docs` | Public modules from `lib.rs` | Public re-exports from `lib.rs` | Direct deps | Path deps |
|---|---|---|---:|---|---|---:|---|---|---|---|---|---|---|
| protocol | `detritus-protocol` | `detritus_protocol` | 0.1.0 | 2024 | `MIT OR Apache-2.0` | 5 / 435 | `tests/roundtrip.rs` | no | no | `crash`, `multipart` behind feature, `otlp`, `source` | `AttachmentManifest`, `BuildInfo`, `CrashAttachment`, `CrashEnvelope`, `CrashKind`, `CrashMetadata`, `ProtocolError`, `SourceId` | 11 normal, 2 build, 1 dev | none |
| client | `detritus-client` | `detritus` | 0.1.0 | 2024 | `MIT OR Apache-2.0` | 5 / 992 | `tests/crash_smoke.rs`, `tests/layer_smoke.rs` | no | yes | none | `SourceId`, `Layer`, `LayerBuilder`, `LayerError`, `PanicHookConfig`, `PanicHookError`, `PanicKind`, `install_panic_hook`, `ShipError`, `ship_pending_crashes` | 20 normal/target, 3 dev | `detritus-protocol`, dev `detritus-server` |
| server | `detritus-server` | `detritus_server` | 0.1.0 | 2024 | `MIT OR Apache-2.0` | 10 / 1,875 | `tests/crash_smoke.rs`, `tests/janitor_smoke.rs`, `tests/logs_smoke.rs`, `tests/rate_limit_smoke.rs` | `detritusd` | no | `auth`, `crashes`, `janitor`, `logs`, `metrics`, `rate_limit`, `server`, `storage` | `ServerConfig`, `serve`, `serve_with_shutdown` | 24 normal, 3 dev | `detritus-protocol` |

Phase 02 consolidates the workspace license to `Apache-2.0` and ships the canonical text at `LICENSE`. Phase 03 adds publish metadata and `version = "0.1.0"` qualifiers to publish-bound path dependencies.

## Per-crate publish decisions

### `detritus-protocol`: PUBLISH-v0.1.0

Persona: Rust developers implementing Detritus-compatible clients, test fixtures, or ingestion services that need shared wire types. The crash schema and `SourceId` model are already the shared contract between client and server, and the OTLP facade is required by sibling crates. The API is stable enough for a v0.x release if Phase 04 hides generated internals behind curated modules and commits only the listed schema/configuration items. All resolved dep licenses are permissive — no publication blockers. (Apache-2.0 is itself permissive; consuming any combination of permissive or copyleft deps is unrestricted.)

### `detritus-client`: PUBLISH-v0.1.0

Persona: application developers who want `cargo add detritus-client` and then install a `tracing-subscriber` layer plus a panic/crash hook. The crate already documents a narrow public API in `src/lib.rs`, and most implementation modules are private. The current surface is acceptable for v0.1.0 once per-crate metadata, README, changelog, and dependency version qualifiers are added. All resolved dep licenses are permissive — no publication blockers. (Apache-2.0 is itself permissive; consuming any combination of permissive or copyleft deps is unrestricted.)

### `detritus-server`: PUBLISH-v0.1.0

Persona: operators and developers who want `cargo install detritus-server` to run `detritusd`, plus advanced users embedding the server in tests or local tooling. Publishing the binary crate is useful distribution, but its library surface is currently too wide because all implementation modules are `pub mod`. Phase 04 must narrow it to configuration and serving entry points before publish. All resolved dep licenses are permissive — no publication blockers. (Apache-2.0 is itself permissive; consuming any combination of permissive or copyleft deps is unrestricted.)

## Crate names + fallbacks

| Crate | Claimed crates.io name | 2026-05-19 availability result | Fallback order | Phase 07 action |
|---|---|---|---|---|
| protocol | `detritus-protocol` | crates.io API: crate does not exist | `detritus-wire`, `detritus-types` | Reserve/publish `detritus-protocol`; use fallback only if publish races or policy rejection occurs. |
| client | `detritus-client` | crates.io API: crate does not exist | `detritus-sdk`, `detritus-tracing` | Reserve/publish `detritus-client`; keep lib name `detritus` unless cargo packaging rejects it. |
| server | `detritus-server` | crates.io API: crate does not exist | `detritusd`, `detritus-receiver` | Reserve/publish `detritus-server`; keep binary name `detritusd`. |

## Public-API surface per crate

### `detritus-protocol`

Stable surface:

- Root constants: `PROTOCOL_VERSION`, `GRPC_VERSION_KEY`, `HTTP_VERSION_HEADER`.
- Crash schema: `AttachmentManifest`, `BuildInfo`, `CrashKind`, `CrashMetadata`, `CrashMetadata::new`, `CrashEnvelope`, `CrashAttachment`, `ProtocolError`.
- Source identity: `SourceId`, `SourceId::canonical`.
- Curated OTLP facade required by sibling crates: `otlp::common::{AnyValue, OtlpAnyValue, ArrayValue, InstrumentationScope, KeyValue, KeyValueList, any_value}`, `otlp::resource::Resource`, `otlp::logs::{ExportLogsServiceRequest, ExportLogsServiceResponse, LogsServiceClient, LogsService, LogsServiceServer, LogRecord, LogsData, ResourceLogs, ScopeLogs, SeverityNumber}`.
- Feature-gated multipart helper surface: `multipart::DEFAULT_BOUNDARY`, `CrashEnvelope::write_to`, `CrashEnvelope::write_to_with_boundary`, `CrashEnvelope::read_from`, `CrashEnvelope::read_from_with_boundary`.

Re-exports:

- Keep root re-exports for `crash::{AttachmentManifest, BuildInfo, CrashAttachment, CrashEnvelope, CrashKind, CrashMetadata, ProtocolError}` and `source::SourceId`.
- Do not root-re-export OTLP generated/prost types; keep them under the explicit `otlp::*` facade so generated names remain visually advanced.

Inadvertent `pub`:

- `otlp::generated` and all nested generated package modules are implementation detail. Phase 04 should make them private or doc-hidden and preserve only the curated `otlp::common`, `otlp::resource`, and `otlp::logs` facade.
- The modules `crash` and `source` may remain public for discoverability, but root re-exports are the preferred stable path.
- The module `multipart` may remain public only behind the `multipart` feature; its helper methods are advanced but intentionally published because `detritus-client` uses the boundary and envelope helpers.

### `detritus-client`

Stable surface:

- Layer API: `Layer`, `Layer::builder`, `Layer::flush`, `LayerBuilder`, `LayerBuilder::{endpoint, token, source, batch_size, flush_interval, flush_timeout, queue_dir, sample_rate, build}`, `LayerError`.
- Panic/crash API: `PanicHookConfig` and its fields, `PanicKind`, `PanicHookError`, `install_panic_hook`.
- Offline shipping API: `ShipError`, `ship_pending_crashes`.
- Shared identity: re-exported `SourceId`.

Re-exports:

- Keep `pub use detritus_protocol::SourceId`.
- Keep `pub use layer::{Layer, LayerBuilder, LayerError}`.
- Keep `pub use panic_hook::{PanicHookConfig, PanicHookError, PanicKind, install_panic_hook}`.
- Keep `pub use shipper::{ShipError, ship_pending_crashes}`.

Inadvertent `pub`:

- None from module visibility: `layer`, `panic_hook`, `shipper`, and `spool` are private modules.
- `StoredUploadConfig`, spool helpers, and `SpoolLock` are already `pub(crate)` and remain internal.
- Phase 04 should review whether `LayerError`, `PanicHookError`, and `ShipError` expose only durable variants before publish; after v0.1.0, removing or renaming error variants is breaking for the stable surface.

### `detritus-server`

Stable surface:

- Serving API: `ServerConfig`, `serve`, `serve_with_shutdown`.
- Configuration types needed to construct `ServerConfig`: `TokenStore`, `SecurityConfig`, `load_security_config`, `AuthConfigError`, `RateLimitConfig`, `RetentionConfig`.
- Test/embedded configuration escape hatch: `TestToken`, `TokenStore::for_tests`. This is stable during v0.x because integration tests and local harnesses need a programmatic token store.

Re-exports:

- Keep `pub use server::{ServerConfig, serve, serve_with_shutdown}`.
- Phase 04 should add root re-exports for the stable configuration types above so downstream users do not import from implementation modules.

Inadvertent `pub`:

- Demote these modules to `pub(crate)` or private in Phase 04 after adding root re-exports: `auth`, `crashes`, `janitor`, `logs`, `metrics`, `rate_limit`, `server`, `storage`.
- Demote implementation-only items: `TokenContext`, `AuthState`, `auth_middleware`, `token_from_extensions`, `endpoint_label`, `CrashResponse`, `crashes_handler`, `CrashError`, `JanitorStats`, `spawn_janitor`, `run_janitor_cycle`, `JanitorError`, `LogsHandler`, `LogWriterPool`, `Metrics`, `JanitorMetricStats`, `RateLimiter`, `RateLimitError`, `AppState`, `SourceKey`, `StoragePaths`, `StorageError`, `validate_component`.
- If an internal integration test needs those items after demotion, move the test into crate-internal modules or add narrow test-only helpers instead of preserving public API by accident.

## crates.io metadata (categories, keywords, descriptions)

| Crate | Description | Keywords | Categories | Homepage | Documentation | Readme |
|---|---|---|---|---|---|---|
| `detritus-protocol` | Wire protocol types for Detritus telemetry and crash ingestion | `observability`, `otlp`, `telemetry`, `crash-reporting`, `protocol` | `network-programming`, `encoding` | `https://codeberg.org/caniko/rs-detritus` | `https://docs.rs/detritus-protocol` | `crates/detritus-protocol/README.md` |
| `detritus-client` | Client SDK for Detritus telemetry and crash reporting | `observability`, `tracing-subscriber`, `crash-reporting`, `panic-hook`, `telemetry` | `development-tools`, `network-programming` | `https://codeberg.org/caniko/rs-detritus` | `https://docs.rs/detritus-client` | `crates/detritus-client/README.md` |
| `detritus-server` | Detritus telemetry and crash ingestion server | `observability`, `otlp`, `crash-reporting`, `server`, `telemetry` | `command-line-utilities`, `web-programming`, `network-programming` | `https://codeberg.org/caniko/rs-detritus` | `https://docs.rs/detritus-server` | `crates/detritus-server/README.md` |

All descriptions are under 80 characters. All keywords are lowercase, use only `[a-z0-9-]`, and are at most 20 characters.

## MSRV + edition decisions

Set workspace `rust-version = "1.85"`. The workspace already uses `edition = "2024"` and `resolver = "3"`; Rust 1.85.0 is the first stable release that stabilized Rust 2024. A current-toolchain validation build passed with `nix develop -c cargo build --workspace` using rustc 1.94.1, but this checkout does not have `rustup`, so `rustup run 1.85.0 cargo build --workspace` could not be executed in this phase.

Semver commitment for the v0.x trajectory:

- The stable-surface bullets in this RFC are the only intentional public API for v0.1.0.
- During `0.1.z`, patch releases may add docs, fix bugs, improve behavior, and add non-breaking trait impls or optional APIs, but must not remove, rename, or change signatures, fields, enum variants, error semantics, feature defaults, binary name, endpoint paths, wire schema fields, or storage paths listed as stable.
- Any breaking change to the stable surface bumps all published crates to the next synchronized minor version, starting at `0.2.0`.
- Inadvertent `pub` items listed above are not semver commitments because Phase 04 demotes them before v0.1.0 publish.
- `1.0.0` ships after at least one non-breaking v0.x cycle has supported both `regicide` and one non-regicide downstream, the crash/log wire schemas have been exercised in production, and the server retention/auth model has no known pending breaking redesign.

## Workspace lints policy

Phase 03 should lift this table verbatim into the workspace root:

```toml
[workspace.lints.rust]
missing_docs = "warn"
unsafe_code = "forbid"
rust_2018_idioms = "warn"
nonstandard_style = "warn"
future_incompatible = "warn"
unreachable_pub = "warn"
unused_crate_dependencies = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
cargo = { level = "warn", priority = -1 }
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
multiple_crate_versions = "allow"
must_use_candidate = "allow"
similar_names = "allow"
too_many_lines = "allow"
```

Policy: publish-bound crates warn on missing docs everywhere, forbid unsafe code unless a future RFC grants a crate-specific exception, and keep Clippy strict enough to catch publish hygiene issues without forcing noisy naming churn. Any new allow entry needs a short rationale in the commit that adds it.

Feature-flag philosophy:

- Features are additive. No feature may make another feature's public API disappear.
- Default features should match the production-ready experience: `detritus-protocol` keeps `multipart` by default, and `detritus-client` keeps `minidump` by default on supported targets.
- Optional platform integrations must use target-specific dependencies where possible.
- Disabling default features is allowed to remove optional transport/crash-capture helpers, but not shared schema types or core builder configuration.
- Feature changes that remove a public item, change default behavior, or alter dependency MSRV are breaking during v0.x.

## Apache-2.0 dependency-license sanity check

Compatibility rule used here: Apache-2.0 covers Detritus' own source after Phase 02. Apache-2.0 is permissive, so any combination of permissive or copyleft direct dependencies is compatible to consume — the published `.crate` archives contain only Detritus's own source. The table below is recorded for visibility (and to flag any `UNKNOWN`/proprietary entries that would need investigation before publish). No direct dependency below is exotic or unknown.

| Crate | Dependency | Kind / target | SPDX license from cargo metadata | Publish verdict |
|---|---|---|---|---|
| `detritus-protocol` | `bytes` | optional normal | `MIT` | compatible |
| `detritus-protocol` | `chrono` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-protocol` | `futures-util` | optional normal | `MIT OR Apache-2.0` | compatible |
| `detritus-protocol` | `multer` | optional normal | `MIT` | compatible |
| `detritus-protocol` | `prost` | normal | `Apache-2.0` | compatible |
| `detritus-protocol` | `serde` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-protocol` | `serde_json` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-protocol` | `thiserror` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-protocol` | `tokio` | optional normal, dev | `MIT` | compatible |
| `detritus-protocol` | `tonic` | normal | `MIT` | compatible |
| `detritus-protocol` | `uuid` | normal | `Apache-2.0 OR MIT` | compatible |
| `detritus-protocol` | `protoc-bin-vendored` | build | `MIT` | compatible |
| `detritus-protocol` | `tonic-build` | build | `MIT` | compatible |
| `detritus-client` | `detritus-protocol` | path normal | workspace license, Phase 02 `Apache-2.0` | compatible; add `version = "0.1.0"` before publish |
| `detritus-client` | `chrono` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `flate2` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `fs2` | normal | `MIT/Apache-2.0` | compatible |
| `detritus-client` | `parking_lot` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `prost` | normal | `Apache-2.0` | compatible |
| `detritus-client` | `reqwest` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `secrecy` | normal | `Apache-2.0 OR MIT` | compatible |
| `detritus-client` | `serde` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `serde_json` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `tar` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `thiserror` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `tokio` | normal | `MIT` | compatible |
| `detritus-client` | `tonic` | normal | `MIT` | compatible |
| `detritus-client` | `tracing` | normal | `MIT` | compatible |
| `detritus-client` | `tracing-core` | normal | `MIT` | compatible |
| `detritus-client` | `tracing-subscriber` | normal | `MIT` | compatible |
| `detritus-client` | `url` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `uuid` | normal | `Apache-2.0 OR MIT` | compatible |
| `detritus-client` | `minidumper-child` | optional target `cfg(not(target_os = "android"))` | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `argon2` | dev | `MIT OR Apache-2.0` | compatible |
| `detritus-client` | `detritus-server` | path dev | workspace license, Phase 02 `Apache-2.0` | compatible; dev-only, no publish qualifier required |
| `detritus-client` | `tempfile` | dev | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `detritus-protocol` | path normal | workspace license, Phase 02 `Apache-2.0` | compatible; add `version = "0.1.0"` before publish |
| `detritus-server` | `argon2` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `axum` | normal | `MIT` | compatible |
| `detritus-server` | `bytes` | normal | `MIT` | compatible |
| `detritus-server` | `chrono` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `clap` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `futures-util` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `hex` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `http-body-util` | normal | `MIT` | compatible |
| `detritus-server` | `multer` | normal | `MIT` | compatible |
| `detritus-server` | `prometheus` | normal | `Apache-2.0` | compatible |
| `detritus-server` | `serde` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `serde_json` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `sha2` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `subtle` | normal | `BSD-3-Clause` | compatible |
| `detritus-server` | `thiserror` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `tokio` | normal | `MIT` | compatible |
| `detritus-server` | `tonic` | normal | `MIT` | compatible |
| `detritus-server` | `tower` | normal | `MIT` | compatible |
| `detritus-server` | `tower-http` | normal | `MIT` | compatible |
| `detritus-server` | `tracing` | normal | `MIT` | compatible |
| `detritus-server` | `tracing-subscriber` | normal | `MIT` | compatible |
| `detritus-server` | `toml` | normal | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `uuid` | normal | `Apache-2.0 OR MIT` | compatible |
| `detritus-server` | `filetime` | dev | `MIT/Apache-2.0` | compatible |
| `detritus-server` | `reqwest` | dev | `MIT OR Apache-2.0` | compatible |
| `detritus-server` | `tempfile` | dev | `MIT OR Apache-2.0` | compatible |

## `detritus-server` distribution decision

Publish `detritus-server` as a binary crate at v0.1.0. The server is useful as an operator-facing install target (`cargo install detritus-server`), and keeping it publishable forces the runtime dependency graph, license metadata, docs, and configuration story to stay honest. The library surface should still be minimal: stable configuration plus `serve` entry points, not every handler, storage helper, middleware, or metric type.

## CI policy + release cadence

CI runs on Codeberg/Forgejo using the project convention for hosted or self-hosted runners. The minimum required gate is `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features`, `cargo test --workspace --all-features`, `cargo doc --workspace --all-features --no-deps`, and `cargo publish --dry-run` for each publish-bound crate in dependency order. Once Phase 02 lands, CI must also check that package metadata uses `Apache-2.0`.

Releases are manual and tag-driven. A maintainer creates a changelog entry, tags `vX.Y.Z`, runs dry-runs locally or in CI, publishes in dependency order (`detritus-protocol`, `detritus-client`, `detritus-server`), and verifies docs.rs builds.

Versions are synchronized across all three crates during v0.x. This costs a few harmless version bumps but avoids explaining protocol/client/server compatibility matrices while the public surface is still settling. Revisit independent versioning only after `1.0.0`.

## Open questions

None.
