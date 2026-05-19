# Phase 05 — Examples, CHANGELOG, repository hygiene

> **Recommended Codex model: GPT 5.5 low**
>
> Mechanical work: write 1-2 runnable example programs per
> publishable crate, flesh out the CHANGELOG that Phase 02
> seeded, add a `.gitignore`/`.cargo/config.toml` tightening
> pass, ensure no stray binaries/log files leak into the
> packaged .crate. No design calls; the examples are
> small variations of the doctest patterns established in
> Phase 04. `low` matches the scope; `medium` is unnecessary
> once Phase 04 has set the example-code idioms.

## Working tree

`/data/nvme0/can/Projects/detritus`. Phase 04's READMEs and
rustdoc are committed first — the example programs mirror the
Quick Start code shown in the READMEs, and the CHANGELOG
entries reference rustdoc items that this phase doesn't move.

## Goal

Each PUBLISH crate has a `examples/` directory containing at
least one runnable example program that exercises the public
API as a downstream user would (no internal-only path imports,
no `pub(crate)` reach-through). `cargo run --example
<name> -p <crate>` succeeds for each. `CHANGELOG.md` has a
fleshed-out `## [Unreleased]` section recording every
user-visible change since the project's first commit
(license migration, metadata addition, public-surface trim,
README authoring). The repo's `.gitignore` excludes
common Rust + editor artifacts; `cargo package` for each
crate produces a clean file list (no editor swap files, no
test fixtures from `tests/`, no .DS_Store).

## Why this matters now

Examples and a CHANGELOG are two of the four "make-or-break"
signals downstream users use to evaluate a crate (the other
two are tests and rustdoc, addressed in Phase 04). A v0.1.0
without examples reads as "this crate's author hasn't tried
to actually use it" — even when the doctest in lib.rs proves
otherwise. A v0.1.0 without a CHANGELOG forces every user to
reconstruct the project's history from `git log` when they
upgrade.

Repository hygiene matters because `cargo publish` packages
*every* file under the crate directory that isn't in the
`exclude` list or `.gitignore`. A stray `*.swp` or
`fragpipe.log` from a developer's session ends up in the
.crate archive permanently.

### Cost of deferring

- v0.1.0 ships, downstream users open issues asking for
  "example code, please". Each issue is a documentation gap
  that should have been closed pre-publish.
- A polluted .crate archive (stray test fixtures, IDE
  metadata) inflates downloads and can leak local paths or
  internal hostnames.
- CHANGELOG-less v0.1.0 → "ah, this project doesn't
  maintain release notes" reputation that's hard to undo.

## Out of scope

- Designing new APIs the examples would exercise. Examples
  use the existing public surface.
- Building an end-to-end demo (e.g. "fully working
  observability stack with grafana"). One self-contained
  example per crate.
- Refactoring `docs/architecture.md`, `docs/operations.md`,
  `docs/storage.md` — those are operator docs; this phase
  is about cargo-level artifacts.
- Adding CHANGELOG entries for *un-committed* WIP. Only
  shipped changes earn a line.
- CI. Phase 06.

## Plan

1. **Inventory existing examples and CHANGELOG.**

   ```sh
   ls crates/*/examples 2>/dev/null
   wc -l CHANGELOG.md 2>/dev/null
   ```

   Phase 02 seeded a CHANGELOG with the license-migration
   entry. No examples exist yet. The phase starts from this
   baseline.

2. **Write `detritus-protocol` example.** File:
   `crates/detritus-protocol/examples/crash_envelope.rs`.

   Pattern: construct a `CrashEnvelope` with a synthetic
   crash, serialize it (the protocol's serialization
   method), print the result. Exercises every Stable item
   per the RFC.

   ```rust
   //! Construct a CrashEnvelope and serialize it as the
   //! detritus wire format would send to the receiver.
   //!
   //! Run with:
   //!     cargo run --example crash_envelope -p detritus-protocol

   use detritus_protocol::{
       CrashEnvelope, CrashKind, CrashMetadata, SourceId, PROTOCOL_VERSION,
   };

   fn main() {
       let source = SourceId {
           project: "example".to_owned(),
           platform: "linux".to_owned(),
           version: "0.1.0".to_owned(),
           install_id: uuid::Uuid::nil(),
       };
       let envelope = CrashEnvelope {
           protocol_version: PROTOCOL_VERSION,
           source,
           metadata: CrashMetadata {
               kind: CrashKind::Panic,
               // ... fill out per the actual struct
           },
           // ...
       };
       println!("envelope: {envelope:#?}");
   }
   ```

   Adjust to the actual struct shapes after re-reading
   `crates/detritus-protocol/src/crash.rs` (post-Phase-04
   visibility).

3. **Write `detritus-client` example.** File:
   `crates/detritus-client/examples/install_layer.rs`.

   Pattern: build a `Layer`, register it with
   `tracing-subscriber`, emit a few events, exit. Uses
   `secrecy`, `url`, `uuid` as the lib.rs doctest does.

   ```rust
   //! Install a detritus tracing layer and emit a couple of
   //! log events.
   //!
   //! This example does not require a running detritus server
   //! — the layer queues events to the offline spool. To
   //! actually ship them, run `detritus::ship_pending_crashes`
   //! against a configured receiver.
   //!
   //! Run with:
   //!     cargo run --example install_layer -p detritus-client

   use std::{path::PathBuf, time::Duration};
   use detritus::{Layer, SourceId};
   use secrecy::SecretString;
   use tracing_subscriber::prelude::*;

   fn main() {
       let source = SourceId {
           project: "detritus-example".to_owned(),
           platform: "linux".to_owned(),
           version: "0.1.0".to_owned(),
           install_id: uuid::Uuid::nil(),
       };
       let layer = Layer::builder()
           .endpoint(url::Url::parse("http://127.0.0.1:4317").unwrap())
           .token(SecretString::from("dev-token"))
           .source(source)
           .queue_dir(PathBuf::from("/tmp/detritus-example-spool"))
           .flush_interval(Duration::from_secs(5))
           .build()
           .unwrap();

       tracing_subscriber::registry().with(layer).init();

       tracing::info!("hello from detritus example");
       tracing::warn!(target = "example", "something to spool");
   }
   ```

   Verify it compiles:

   ```sh
   cargo build --example install_layer -p detritus-client
   ```

4. **Write `detritus-server` example.** File:
   `crates/detritus-server/examples/embed_server.rs`.

   Pattern: embed the server in a tokio task, accept one
   request, exit. Demonstrates the `serve_with_shutdown`
   API.

   ```rust
   //! Embed a detritus receiver in another process and
   //! shut it down after 1 second.
   //!
   //! Run with:
   //!     cargo run --example embed_server -p detritus-server

   use std::time::Duration;
   use detritus_server::{ServerConfig, serve_with_shutdown};
   use tokio::sync::oneshot;

   #[tokio::main(flavor = "current_thread")]
   async fn main() {
       let config = ServerConfig {
           // ... fields per actual struct
       };
       let (tx, rx) = oneshot::channel();
       tokio::spawn(async move {
           tokio::time::sleep(Duration::from_secs(1)).await;
           let _ = tx.send(());
       });
       serve_with_shutdown(config, async move { let _ = rx.await; })
           .await
           .unwrap();
   }
   ```

   Verify it compiles. (Don't run it as part of acceptance
   — port binding is environment-dependent. Compile-time
   verification is enough.)

5. **Document examples in each crate's README.** Add an
   `## Examples` section to each README:

   ```md
   ## Examples

   - [`crash_envelope`](examples/crash_envelope.rs) — construct and serialize a CrashEnvelope.

   Run with:

       cargo run --example crash_envelope -p detritus-protocol
   ```

6. **Flesh out the CHANGELOG.** Expand Phase 02's seed
   under `## [Unreleased]`:

   ```md
   ## [Unreleased]

   ### Added

   - Per-crate `README.md`, examples, and full rustdoc
     coverage on the public API surface.
   - Workspace MSRV declaration (`rust-version = "<MSRV>"`).
   - `[workspace.lints]` policy enforcing `missing_docs`,
     `unsafe_code = forbid`, plus clippy `all` /
     `pedantic` / `nursery` at `warn`.
   - crates.io publish metadata (description, keywords,
     categories, homepage, documentation) for each
     publishable crate.

   ### Changed

   - License consolidated from the dual
     `MIT OR Apache-2.0` declaration to `Apache-2.0` only,
     and the canonical license text is now shipped at
     `LICENSE` (previously absent). Single-author history;
     consolidating the offered options is uncontroversial.
   - Trimmed public API surface in `detritus-protocol`
     and `detritus-server` per the publish-readiness RFC:
     <list demoted items>.
   - Internal path dependencies between detritus crates
     now carry both `path = "..."` and
     `version = "0.1.0"` so `cargo publish` resolves
     them against crates.io.

   ### Removed

   - Per-file `#![warn(missing_docs)]` in
     `detritus-client/src/lib.rs` (replaced by the
     workspace lint).
   ```

   Phase 07 will move this block to `## [0.1.0] —
   <YYYY-MM-DD>` when the release tag is cut.

7. **`.gitignore` audit.** Read the current
   `.gitignore`; ensure it excludes:
   - `target/` (top-level)
   - `**/target/` (any nested workspace)
   - `Cargo.lock` — **keep tracked** for the binary crate
     (`detritus-server`); the workspace already commits it,
     so no `.gitignore` change needed.
   - `*.swp`, `*.swo`, `.vscode/`, `.idea/`, `.DS_Store` —
     editor cruft.
   - `*.log` — but verify no test fixtures rely on a
     committed `.log` file.
   - `result`, `result-*` — Nix build symlinks.

   Add missing entries; do not remove any unless they're
   demonstrably wrong.

8. **`cargo package` cleanliness sweep.** For each PUBLISH
   crate:

   ```sh
   cargo package -p <crate> --no-verify --allow-dirty --list 2>&1 \
     | tee /tmp/<crate>-pkg-list.txt
   ```

   Inspect each list for:
   - Stray fixtures from `tests/` — ok if they're in
     `tests/` and the crate is published with tests
     (cargo includes `tests/`).
   - Files outside the crate directory — should NOT
     appear; if they do, the `[package]` block has a
     `path = "../foo"` leak.
   - Editor backup files (`*.bak`, `*.swp`) — fix the
     `.gitignore` or add to `package.exclude`.
   - Anything > 1 MB inside `src/` — investigate; may
     indicate accidentally-committed binary fixtures.

   If a file leaks that's not appropriate, edit the crate's
   `Cargo.toml` to add:

   ```toml
   [package]
   ...
   exclude = ["*.swp", "tests/fixtures/big-binary.bin"]
   ```

   Prefer `package.include` (whitelist) over `exclude`
   (blacklist) if the file set is small and stable.

9. **Verify the examples build.**

   ```sh
   cargo build --workspace --examples 2>&1 | tail -10
   ```

   Exit 0. Each `examples/*.rs` compiles.

10. **Run the examples that don't require external
    services.**

    ```sh
    cargo run --example crash_envelope -p detritus-protocol 2>&1 | head
    cargo run --example install_layer -p detritus-client 2>&1 | head
    ```

    Each prints expected output and exits 0. The
    `embed_server` example is compile-only per step 4.

11. **Commit.** Single commit. Subject:

    ```
    docs: per-crate examples + CHANGELOG hygiene
    ```

    Body: list each example file added, the CHANGELOG
    additions, any `.gitignore` deltas, and the
    `cargo package --list` cleanliness verdict per crate.

## Acceptance criteria

- [ ] `crates/detritus-protocol/examples/<at-least-one>.rs` exists.
- [ ] `crates/detritus-client/examples/<at-least-one>.rs` exists.
- [ ] `crates/detritus-server/examples/<at-least-one>.rs` exists.
- [ ] `cargo build --workspace --examples` succeeds.
- [ ] At least one example per crate (the non-server-binding ones) runs to completion under `cargo run --example`.
- [ ] `CHANGELOG.md` has an `## [Unreleased]` section with `### Added`, `### Changed`, and (optionally) `### Removed` subsections that name concrete user-visible changes from Phases 02–04.
- [ ] `cargo package -p <crate> --no-verify --allow-dirty --list` for each PUBLISH crate produces a file list that contains: `Cargo.toml`, `Cargo.lock` (workspace), `README.md`, `src/`, `examples/`, `tests/`, `LICENSE`. It MUST NOT contain editor backups, `target/`, `*.log`, `.DS_Store`, or files outside the crate directory.
- [ ] `.gitignore` includes entries for editor backups (`*.swp`, `.vscode/`, `.idea/`, `.DS_Store`), Nix result symlinks (`result`, `result-*`), and `target/` (top-level + nested).
- [ ] Each crate's `README.md` includes an `## Examples` section linking to the example file(s).
- [ ] `cargo doc --workspace --no-deps --examples 2>&1 | grep -cE "warning|error" | grep -v "^Documenting"` returns 0 (examples don't break rustdoc).
- [ ] Commit body lists each example file with its purpose and the `cargo package --list` cleanliness verdict.

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- New: `crates/detritus-protocol/examples/<one or more>.rs`.
- New: `crates/detritus-client/examples/<one or more>.rs`.
- New: `crates/detritus-server/examples/<one or more>.rs`.
- Edit: `CHANGELOG.md` — expand the Unreleased section.
- Edit: `.gitignore` — append any missing entries.
- Edit: `crates/<name>/README.md` (each) — add `## Examples` section.
- (Conditional) Edit: `crates/<name>/Cargo.toml` if `package.exclude` or `package.include` needs to gate stray files.

## Pitfalls

- **Examples that depend on a running server.** The
  `install_layer` example creates a spool dir and shouldn't
  attempt to contact the endpoint. Verify the Layer is
  configured for offline-queue mode (or that
  `flush_interval` is long enough that nothing tries to
  send during the example's lifetime). If a network call
  attempts in the example, swap for a no-op endpoint or
  use a feature flag to bypass shipping.

- **Editor-cruft .gitignore. Be specific.** A blanket
  `*.log` in `.gitignore` would silently exclude any
  legitimate `tests/data/*.log` test fixture. If the
  workspace uses log fixtures for tests, scope the
  exclusion (`/logs/`, `*.log.tmp`).

- **`package.include` accidentally excluding the README.**
  When using `include = [...]` (whitelist), the list must
  explicitly include `README.md`, `LICENSE`,
  `CHANGELOG.md`, and the conventional source layout. A
  missing entry → `cargo publish` ships a crate with no
  README.

- **`Cargo.lock` for library crates.** Conventionally,
  library crates don't commit `Cargo.lock` (the workspace
  resolves it on the consumer side). For workspaces with a
  binary (like `detritusd`), commit the lockfile to pin
  the binary's tested deps. The detritus workspace
  already commits `Cargo.lock` — don't change that.

- **The `embed_server` example needing a config.** The
  `ServerConfig` struct may have required fields without
  defaults (storage paths, listen addr, token-hash). The
  example needs realistic values that don't accidentally
  start binding to a privileged port. Use `127.0.0.1:0`
  (kernel-assigned port), in-memory storage if available,
  or a `tempfile::TempDir`.

- **Doctests vs examples drift.** Examples often duplicate
  the doctest code in lib.rs. Keep the example as the
  authoritative version; the doctest may add `# fn main()
  { ... }` boilerplate the example doesn't need.

## Reference

- Phase 04 READMEs: `docs/planning/detritus-crates-io-finalization/04-per-crate-readmes-and-rustdoc.md`.
- Phase 02 CHANGELOG seed: `docs/planning/detritus-crates-io-finalization/02-license-consolidation-to-apache.md` (step 10).
- Cargo `[package]` `include` / `exclude`: <https://doc.rust-lang.org/cargo/reference/manifest.html#the-exclude-and-include-fields>.
- Cargo `examples` reference: <https://doc.rust-lang.org/cargo/reference/cargo-targets.html#examples>.
- Keep a Changelog: <https://keepachangelog.com/en/1.1.0/>.
- Sequencing: depends on Phase 04 (READMEs link to examples; rustdoc coverage assumes the public surface is documented). Independent of Phases 02, 03, 06 by file scope. Blocks Phase 07 (dry-run includes examples in the .crate archive).
