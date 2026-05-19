# Phase 04 — Per-crate READMEs + rustdoc polish

> **Recommended Codex model: GPT 5.5 medium**
>
> Writing per-crate READMEs that render on crates.io and
> tightening the rustdoc on every `pub` item is genuine craft
> work — the wrong tone or omitted example renders the crate
> page useless even with correct metadata. Plus this phase
> demotes inadvertently-public items per the RFC's API audit,
> which is a code change with semver implications even at
> v0.x. Moderate complexity, writing-heavy, leaf execution.
> `low` would skimp on the README narrative; `high` is overkill
> once the RFC has named the public surface.

## Working tree

`/data/nvme0/can/Projects/detritus`. Phase 01's RFC names the
intended public surface per crate; Phase 03 has created
placeholder READMEs and declared `readme = "README.md"` in each
crate's Cargo.toml. This phase fills the content and tightens
the API.

## Goal

Each PUBLISH crate has a real README.md (≥ 60 lines,
explaining what the crate is, what it does, a quick-start
code block, feature flags, MSRV, links to the workspace and
other detritus crates) that renders cleanly on crates.io.
Every `pub` item in every PUBLISH crate has a rustdoc comment
documenting its purpose; every public module has a top-level
doc comment. Items the RFC tagged "Inadvertent" are demoted to
`pub(crate)` or made `#[doc(hidden)]` with a one-line comment
naming why. `cargo doc --workspace --no-deps` builds with zero
warnings (specifically: zero `missing_docs` warnings).
`cargo test --workspace --doc` passes (every code example
compiles).

## Why this matters now

A v0.1.0 published to crates.io is the public face of the
project. The current state of the artifacts at publish time
would be:

- crates.io page renders the placeholder README from Phase 03
  ("See the workspace README until this crate's README
  lands"). That's a publish-blocker for adoption.
- Only `detritus-client` has `#![warn(missing_docs)]`; the
  other two crates have undocumented public items. docs.rs
  builds the documentation but produces a sparse,
  uninformative page.
- The RFC named items like `detritus-protocol::otlp` and
  `detritus-protocol::multipart` as "Inadvertent `pub`" —
  every downstream user that imports them locks us into
  supporting them. Demoting before v0.1.0 is free; after, it
  requires a semver bump.

### Cost of deferring

- v0.1.0 ships with sparse docs and "Inadvertent" public
  items → those items become accidentally load-bearing for
  early users → demoting them in v0.2.0 forces breaking
  changes.
- A README-less crates.io page has a published correlation
  with low adoption (anecdotal; visible across crates.io's
  ecosystem).
- doctests are a real coverage source — without working
  examples in the rustdoc, regressions slip in unnoticed.

## Out of scope

- Adding new public APIs. This phase trims, documents, and
  examples the existing surface — does not add.
- Cross-crate refactoring. If a doc requires a type to move
  between crates, defer to a follow-up.
- Adding examples files under `examples/`. That's Phase 05.
- Adding CI rustdoc checks. Phase 06.
- Expanding the workspace README beyond a `## License`
  section (already in place from Phase 02) and a `##
  Documentation` link list. Larger expansion is its own
  follow-up.

## Plan

1. **Read the RFC's public-API surface tables.** For each
   PUBLISH crate, note the three buckets:
   - **Stable** — keep `pub`, document fully.
   - **Re-exports** — keep `pub`, document at the re-export
     site (single `///` line is fine if the target item is
     fully documented).
   - **Inadvertent** — demote to `pub(crate)` OR mark
     `#[doc(hidden)]` if removing `pub` would break an
     existing internal call site that isn't easy to
     untangle. Each `#[doc(hidden)]` decision is recorded in
     the commit body.

2. **Demote inadvertent items.** For each Inadvertent item:

   - Open the file. Find the `pub fn`/`pub struct`/`pub
     mod` line.
   - Default action: change `pub` → `pub(crate)`.
   - If the build then fails because the item is used
     across crates in the workspace via the public path,
     consider whether it should actually be Stable (revise
     the RFC if so, with a one-line CHANGELOG note) or
     whether the cross-crate import should rewrite to a
     `pub(crate)` path via a sibling module. Prefer
     restructuring over re-promoting.
   - For each item demoted, record in a TODO list: the
     file, the item name, the new visibility.

3. **Lift `#![warn(missing_docs)]` to workspace level.** If
   the RFC's lint table includes `missing_docs = "warn"`
   under `[workspace.lints.rust]`, every member crate
   inherits it via `[lints] workspace = true`. Remove the
   per-file `#![warn(missing_docs)]` in
   `crates/detritus-client/src/lib.rs:31` (now redundant)
   and ensure the workspace lint is set.

4. **Document every public item in `detritus-protocol`.**
   File-by-file walkthrough:

   - `src/lib.rs` — root doc comment should explain the
     crate's role (wire types), the relationship to
     `detritus-client` and `detritus-server`, the OTLP
     compatibility scope (logs only at v0.1.0), and the
     `PROTOCOL_VERSION` constant. Include a minimal code
     example showing how to construct a `CrashEnvelope` or
     read `PROTOCOL_VERSION`.
   - `src/crash.rs` — every `pub` struct/enum/fn gets a
     `///` comment. `CrashEnvelope`, `CrashKind`,
     `CrashMetadata`, `AttachmentManifest`, `BuildInfo`,
     `CrashAttachment`, `ProtocolError`.
   - `src/source.rs` — `SourceId` fields are public; each
     field gets a `///` comment.
   - `src/otlp.rs` — if the RFC kept this `pub mod`, add a
     module-level doc comment explaining "this re-exports
     auto-generated OTLP proto bindings; use them only if
     you're constructing OTLP messages directly".
   - `src/multipart.rs` — similar; the multipart helpers
     are advanced surface.

5. **Document every public item in `detritus-client`.**

   - `src/lib.rs` — already has a good example. Verify it
     still compiles after step 2's demotion sweep:

     ```sh
     cargo test --doc -p detritus-client
     ```

   - `src/layer.rs` — every `pub` item gets `///`. `Layer`,
     `LayerBuilder`, all builder methods.
   - `src/panic_hook.rs` — `install_panic_hook`,
     `PanicHookConfig`, `PanicKind`. Each method's `Errors`
     section names the conditions where it returns an
     error.
   - `src/shipper.rs`, `src/spool.rs` — likely fully
     `pub(crate)` after step 2's demotion. If any item
     stayed `pub`, document it.

6. **Document every public item in `detritus-server`.**

   - `src/lib.rs` — root doc comment should explain that
     this is the receiver-side crate; `cargo install
     detritus-server` ships the `detritusd` binary; the
     library surface (`ServerConfig`, `serve`,
     `serve_with_shutdown`) is for embedding the server in
     another process (testing, custom orchestration).
   - `src/server.rs` — `ServerConfig`, `serve`,
     `serve_with_shutdown` get full doc comments with
     `# Errors` and `# Examples` sections.
   - Other modules (`auth`, `crashes`, `janitor`, `logs`,
     `metrics`, `rate_limit`, `storage`) — per the RFC,
     decide which stay `pub mod` (advanced surface for
     embedders) and which demote. The bias is "demote, only
     `pub mod` what an embedder genuinely needs". Likely
     only `server` stays.

7. **Write each crate's README.md.** Replace the
   placeholder created in Phase 03 with real content.
   Template (adjust per crate):

   ```md
   # <crate-name>

   [![Crates.io](https://img.shields.io/crates/v/<crate-name>.svg)](https://crates.io/crates/<crate-name>)
   [![Documentation](https://docs.rs/<crate-name>/badge.svg)](https://docs.rs/<crate-name>)
   [![License](https://img.shields.io/crates/l/<crate-name>.svg)](LICENSE)

   <one-paragraph description — same as Cargo.toml's `description` but expanded>

   ## Quick start

   <runnable code block — the doctest, ideally>

   ## Feature flags

   <table of `[features]` with their effects>

   ## Compatibility

   - Detritus v0.1.0 protocol
   - MSRV: <MSRV>
   - Edition 2024

   ## Related crates

   - [detritus-client](https://crates.io/crates/detritus-client) — client SDK
   - [detritus-protocol](https://crates.io/crates/detritus-protocol) — wire types
   - [detritus-server](https://crates.io/crates/detritus-server) — receiver

   ## Documentation

   - [API docs (docs.rs)](https://docs.rs/<crate-name>)
   - [Architecture](https://codeberg.org/caniko/rs-detritus/src/branch/trunk/docs/architecture.md)

   ## License

   Licensed under the [Apache License, Version 2.0](LICENSE).
   ```

   Per-crate adaptations:
   - `detritus-protocol`: protocol-version contract,
     OTLP-logs scope, what's in scope for v0.x semver,
     no Quick Start example (it's a types crate — show
     `PROTOCOL_VERSION` and a roundtrip).
   - `detritus-client`: tracing-subscriber `Layer` setup,
     panic-hook install, offline-spool semantics. Quick
     Start is the Layer-builder example already in lib.rs.
   - `detritus-server`: how to embed via `serve_with_shutdown`,
     how to run `detritusd` from the command line, link to
     `docs/operations.md` for deployment.

8. **Run rustdoc.**

   ```sh
   cargo doc --workspace --no-deps --all-features 2>&1 | tee /tmp/rustdoc.log
   grep -E "warning|error" /tmp/rustdoc.log | grep -v "^Documenting" | head
   ```

   Zero warnings. Specifically, zero `missing_docs`
   warnings, zero broken intra-doc links, zero broken
   external links (rustdoc has `--cfg=docsrs` machinery; we
   don't need that for v0.1.0).

9. **Run doctests.**

   ```sh
   cargo test --workspace --doc 2>&1 | tail -10
   ```

   All pass. Doctests in code examples in `///` and `//!`
   must compile and run.

10. **Run clippy with the workspace lints.**

    ```sh
    cargo clippy --workspace --all-features --all-targets -- -D warnings 2>&1 | tail
    ```

    Phase 03 surfaced lints but didn't enforce them. This
    phase resolves them — either by fixing the code or by
    downgrading the lint to `allow` with a justification
    comment in `[workspace.lints]`. Default: fix.

11. **Update workspace README.md.** Add a "Crates" section
    listing the three published crates with a 1-line
    description each, between the existing intro and the
    `## Documentation` block (the latter was likely
    untouched since the initial 14-line README).

12. **Commit.** Single commit. Subject:

    ```
    docs: per-crate READMEs, rustdoc coverage, trim public surface
    ```

    Body: list every demoted item with old/new visibility,
    the `cargo doc` warning delta, the doctest count, the
    clippy delta, and the per-crate README line counts.

## Acceptance criteria

- [ ] `cargo doc --workspace --no-deps --all-features 2>&1 | grep -cE "warning|error" | grep -v "^Documenting"` returns 0.
- [ ] Every PUBLISH crate's `README.md` is ≥ 60 lines (real content, not placeholder).
- [ ] Every PUBLISH crate's `README.md` includes: title, badges (crates.io, docs.rs, license), 1-paragraph description, Quick start (with at least one code block or "see the documentation" link), Feature flags section, Compatibility section with MSRV, Related crates section, License section.
- [ ] `cargo test --workspace --doc` exits 0 (every doctest compiles and passes).
- [ ] `cargo clippy --workspace --all-features --all-targets -- -D warnings` exits 0.
- [ ] `grep -rE "^\s*pub " crates/*/src/*.rs | grep -v "^[^:]*: //"` shows no item the RFC tagged "Inadvertent" — every Inadvertent item is now `pub(crate)`, removed, or `#[doc(hidden)]`.
- [ ] `cargo build --workspace` succeeds.
- [ ] No file at `crates/*/src/lib.rs` retains a per-file `#![warn(missing_docs)]` line (the workspace lint covers it).
- [ ] Workspace `README.md` has a `## Crates` section listing the published crates with descriptions.
- [ ] Commit body lists every demoted item with its old/new visibility, and the rustdoc-warning + doctest counts before/after.

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- Edit: `crates/detritus-protocol/README.md` (placeholder → real content).
- Edit: `crates/detritus-client/README.md` (placeholder → real content).
- Edit: `crates/detritus-server/README.md` (placeholder → real content).
- Edit: `README.md` (add `## Crates` section).
- Edit: `crates/detritus-protocol/src/lib.rs`, `src/crash.rs`, `src/source.rs`, `src/otlp.rs`, `src/multipart.rs` (rustdoc).
- Edit: `crates/detritus-client/src/lib.rs`, `src/layer.rs`, `src/panic_hook.rs`, `src/shipper.rs`, `src/spool.rs` (rustdoc + visibility tightening; drop redundant `#![warn(missing_docs)]`).
- Edit: `crates/detritus-server/src/lib.rs`, `src/server.rs`, possibly others (rustdoc + visibility tightening).
- Possibly Edit: `crates/*/Cargo.toml` if a visibility demotion exposes a missing feature gate.

## Pitfalls

- **Demotion cascades.** A `pub fn` demoted to
  `pub(crate)` may be called from an `examples/` file (Phase
  05 adds these but Phase 03's READMEs might paste code
  that uses the public path). Run `cargo check` between
  every batch of demotions to catch breakage early.

- **Doctest network access.** Doctests with
  `Url::parse("http://example.com")` are fine — they parse
  the URL but don't make a request. A doctest that
  attempts an actual HTTP call (or spawns a real server)
  fails on CI. Mark such tests with `no_run` or `ignore`,
  but prefer rewriting them to be self-contained.

- **`#[doc(hidden)]` as a get-out-of-jail card.** Using it
  liberally pushes the v0.1.0 public surface into "hidden
  but technically still `pub`" territory. A determined user
  can still depend on the hidden item, and breaking it is
  still technically a breaking change. Use sparingly; prefer
  `pub(crate)`.

- **rustdoc intra-doc links failing on docs.rs.** Link
  syntax `[Layer]` works in rustdoc if `Layer` is in scope.
  Cross-crate links like `[detritus_protocol::CrashEnvelope]`
  require the target crate to be in `[dependencies]` (or
  `[dev-dependencies]` with `#[cfg(doc)]` indirection).
  When in doubt, use HTML links to docs.rs:
  `[`CrashEnvelope`](https://docs.rs/detritus-protocol/latest/detritus_protocol/struct.CrashEnvelope.html)`.

- **README crates.io vs docs.rs vs codeberg rendering.**
  crates.io renders the README as GitHub-Flavored Markdown
  (relative image/link paths resolve against the .crate
  archive — i.e. the crate dir). docs.rs renders it
  similarly. Codeberg renders against the repo root. Use
  absolute URLs for any link that needs to work in all
  three contexts; reserve relative paths for assets that
  ship inside the .crate.

- **The license badge.** Avoid Shields.io URLs that fetch
  the license from crates.io (they 404 until first publish).
  Use a static badge:
  `https://img.shields.io/badge/license-Apache--2.0-blue.svg`
  during the pre-publish window.

- **Lifting `#![warn(missing_docs)]` to workspace before
  every crate is documented.** Phase 03 left this off; Phase
  04 turns it on AFTER documenting every item. If you flip
  the workspace lint mid-pass, the build floods with
  hundreds of warnings until you finish, which obscures
  legitimate issues. Document first, then lift, then verify
  zero warnings.

## Reference

- Phase 01 RFC public-API tables: `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`.
- Phase 03 placeholder READMEs and metadata: `docs/planning/detritus-crates-io-finalization/03-cargo-metadata-and-dep-versioning.md`.
- Rustdoc book: <https://doc.rust-lang.org/rustdoc/>.
- API guidelines (item documentation): <https://rust-lang.github.io/api-guidelines/documentation.html>.
- crates.io README rendering: relative paths resolve against the .crate archive root.
- Sequencing: depends on Phases 01 + 03 (RFC names items + metadata fields exist). Independent of Phase 02 by file scope. Independent of Phases 05, 06. Blocks Phase 07 (dry-run package contents include the README files).
