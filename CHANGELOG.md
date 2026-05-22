# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- Per-crate `README.md` files for `detritus-protocol`, `detritus-client`,
  and `detritus-server`, including quick-start code, feature flags,
  compatibility notes, related crates, documentation links, and examples.
- Runnable example programs for every publishable crate:
  `crash_envelope`, `install_layer`, and `embed_server`.
- Full rustdoc coverage for the v0.1.0 public API surface.
- Workspace MSRV declaration with `rust-version = "1.85"` inherited by each
  publishable crate.
- Workspace lint policy enforcing `missing_docs`, `unsafe_code = forbid`,
  and strict clippy groups at warning level.
- crates.io publish metadata for every publishable crate, including
  descriptions, keywords, categories, homepage, repository, documentation, and
  per-crate README paths.
- Workspace README crate overview for the published package set.

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

[Unreleased]: https://codeberg.org/caniko/rs-detritus/compare/v0.1.0...HEAD
[0.1.0]: https://codeberg.org/caniko/rs-detritus/releases/tag/v0.1.0
