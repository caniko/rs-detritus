# Phase 03 — Fill in Cargo.toml metadata + version internal path deps

> **Recommended Codex model: GPT 5.5 medium**
>
> The work is mechanical once Phase 01's RFC has named the
> description / keywords / categories / homepage / docs URLs for
> each crate: paste the values into the per-crate Cargo.toml,
> add `version = "0.1.0"` alongside every internal path dep,
> declare workspace-level lints, set MSRV. The design decisions
> are pre-resolved in the RFC. One judgement call survives:
> whether to add `publish = false` to any crate the RFC tagged
> INTERNAL-ONLY (rather than just omitting metadata, which would
> let an accidental `cargo publish` succeed). `medium` matches
> the scope; `low` would skip the verification of the resulting
> `cargo package` output; `high` is overkill once Phase 01 has
> set the content.

## Working tree

`/data/nvme0/can/Projects/detritus`. Phase 01's RFC and Phase 02's
license migration both must be committed first — the metadata
phase consumes the RFC's content verbatim and the license string
from the post-Phase-02 `Cargo.toml`.

## Goal

Each crate in the workspace carries the full crates.io metadata
the RFC specified: `description`, `keywords`, `categories`,
`homepage`, `documentation`, `readme`. Internal path
dependencies between detritus crates use the dual
`path = "..."` + `version = "0.1.0"` form so `cargo publish` can
resolve them against crates.io. The workspace declares
`rust-version = "<MSRV>"`, the RFC's `[workspace.lints]` table
is in place, and any `INTERNAL-ONLY` crates carry
`publish = false`. `cargo package --list` for each PUBLISH crate
shows the expected file set; `cargo package --no-verify` produces
a `.crate` file without errors.

## Why this matters now

Today none of the three Cargo.toml files declares any of the
crates.io-rendered fields:

```
$ grep -nE "description|keywords|categories|homepage|documentation|readme" \
    Cargo.toml crates/*/Cargo.toml
# (zero matches)
```

Internal deps reference each other by path only:

```toml
detritus-protocol = { path = "../detritus-protocol", features = ["multipart"] }
```

A `cargo publish` invocation against any of these errors out
with:

```
error: all dependencies must have a version specified when publishing.
dependency `detritus-protocol` does not specify a version
```

Without `rust-version` set, the published crate has no MSRV
contract — early-adopter users on older stable will hit cryptic
build errors instead of a clear "MSRV is X.Y" message.

### Cost of deferring

- Phase 07's dry-run fails at the first crate, blocking the
  entire release.
- crates.io users browsing the package page see "no
  description" / "no keywords" / "Other" category — the crate
  looks unmaintained.
- Each subsequent semver bump (v0.2.0, v0.3.0) requires
  re-deciding metadata. Lock it in once.

## Out of scope

- Writing the actual README content for each crate's
  `readme = "README.md"`. That's Phase 04 (the file must
  exist for the `readme` key to make sense, but the
  *content* is Phase 04's work; this phase just declares the
  key and creates a placeholder file).
- Changing crate names. The RFC names the canonical names and
  any fallbacks; this phase uses the canonical name. Phase
  07's dry-run is where the fallback decision actually fires
  if a name is taken.
- Adding examples. Phase 05.
- Adding CI. Phase 06.
- Touching `flake.nix` further — the license already moved in
  Phase 02.
- Adjusting the public API surface. Phase 04 (with rustdoc
  and surface-trimming).

## Plan

1. **Read the RFC.** Open
   `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`.
   Print the per-crate metadata table at the top of this
   phase's working notes. Reject any "TBD" or "verify-before-
   publish" entry — those should have been resolved in Phase
   01.

2. **Update workspace-level Cargo.toml.** Edit
   [Cargo.toml](../../../../Cargo.toml):

   ```toml
   [workspace.package]
   edition = "2024"
   license = "Apache-2.0"
   repository = "https://codeberg.org/caniko/rs-detritus"
   homepage = "https://codeberg.org/caniko/rs-detritus"
   documentation = "https://docs.rs/detritus-client"  # or per-crate, see step 4
   rust-version = "<MSRV from RFC>"
   authors = ["Caniko <gpg@rotas.mozmail.com>"]  # or actual canonical author per RFC
   ```

   Then paste the RFC's `[workspace.lints.rust]` and
   `[workspace.lints.clippy]` tables verbatim into the
   `[workspace.lints]` block.

3. **Bump internal path deps.** For each crate-to-crate dep
   inside the workspace, add `version = "0.1.0"` alongside the
   existing `path = "..."`. The dual form lets cargo resolve
   from path during local build and from crates.io after
   publish. Specifically:

   In `crates/detritus-client/Cargo.toml`:

   ```diff
   - detritus-protocol = { path = "../detritus-protocol", features = ["multipart"] }
   + detritus-protocol = { path = "../detritus-protocol", version = "0.1.0", features = ["multipart"] }
   ```

   In `crates/detritus-server/Cargo.toml`:

   ```diff
   - detritus-protocol = { path = "../detritus-protocol", features = ["multipart"] }
   + detritus-protocol = { path = "../detritus-protocol", version = "0.1.0", features = ["multipart"] }
   ```

   In `crates/detritus-client/Cargo.toml` `[dev-dependencies]`:

   ```diff
   - detritus-server = { path = "../detritus-server" }
   + detritus-server = { path = "../detritus-server", version = "0.1.0" }
   ```

   (Dev-deps don't strictly need a version for `cargo publish`
   to succeed — cargo drops dev-deps from the published
   `.crate` file — but the consistency is worth it for the
   commit's diff legibility.)

4. **Fill per-crate metadata.** For each PUBLISH crate, add
   the fields the RFC named. Example for
   `crates/detritus-protocol/Cargo.toml`:

   ```toml
   [package]
   name = "detritus-protocol"
   version = "0.1.0"
   edition.workspace = true
   license.workspace = true
   repository.workspace = true
   homepage.workspace = true
   rust-version.workspace = true
   authors.workspace = true

   description = "Wire protocol types for the Detritus observability ingestion SDK (OTLP logs, crash envelopes)."
   keywords = ["observability", "otlp", "telemetry", "crash-reporting", "protocol"]
   categories = ["network-programming", "encoding"]
   readme = "README.md"
   documentation = "https://docs.rs/detritus-protocol"
   ```

   Note `documentation` is per-crate (not workspace-inherited)
   because the docs.rs URL is name-specific. Repeat the
   pattern for `detritus-client` and `detritus-server` using
   the RFC's values.

   For any INTERNAL-ONLY crate (per the RFC), add
   `publish = false` to `[package]` and skip the metadata
   fields (no readme, no description). `cargo publish`
   refuses to upload such crates.

5. **Verify workspace inheritance works.** Run:

   ```sh
   cargo metadata --format-version 1 \
     | jq -r '.packages[] | select(.name | startswith("detritus")) | "\(.name): \(.license) — \(.description // "MISSING")"'
   ```

   Each detritus crate prints: name, `Apache-2.0`, the
   description from step 4. Any `MISSING` is a step-4 oversight.

6. **Create placeholder per-crate READMEs.** For each PUBLISH
   crate, create `crates/<name>/README.md` with a single line:

   ```md
   # <crate-name>

   See [the workspace README](../../README.md) until this
   crate's README lands in the next release.
   ```

   Phase 04 will fill in the real content. The placeholder
   exists so `readme = "README.md"` in `[package]` resolves
   *and* the dry-run in Phase 07 has a non-empty file to
   package.

7. **Verify each crate packages cleanly.**

   ```sh
   for c in detritus-protocol detritus-client detritus-server; do
     echo "=== $c ==="
     cd /data/nvme0/can/Projects/detritus
     cargo package -p $c --no-verify --allow-dirty --list 2>&1 | head -20
   done
   ```

   Each must list a sane file set (no `target/`, no `.git/`,
   no editor backups, no working-dir artifacts). If a stray
   file leaks in, address via `package.exclude` in the
   crate's Cargo.toml — but try `package.include` first
   (whitelist is safer).

8. **Run `cargo check --workspace` and `cargo clippy --workspace`.**

   ```sh
   cargo check --workspace --all-features 2>&1 | tail -5
   cargo clippy --workspace --all-features -- -D warnings 2>&1 | tail -10
   ```

   The new `[workspace.lints]` table will surface lints that
   were silent before. **Do not fix lint hits in this phase**
   — note them for Phase 04 (which is the documentation pass,
   and natural place to also address newly-surfaced lint
   warnings). If a NEW lint actually fails the build, decide:
   downgrade to `warn` for now (Phase 04 fixes), or fix in
   this phase if it's a one-line change.

9. **Commit.** Single commit. Subject:

   ```
   workspace: fill crates.io metadata + version internal path deps
   ```

   Body: list every metadata field added per crate, the MSRV
   value, the dep-versioning diff, and any lint warnings that
   surfaced. Reference the Phase 01 RFC.

## Acceptance criteria

- [ ] `cargo metadata --format-version 1 | jq -r '.packages[] | select(.name | startswith("detritus")) | .description' | grep -c "null\|\"\""` returns 0 — every detritus crate has a description.
- [ ] Each PUBLISH crate's `Cargo.toml` declares all of: `description`, `keywords`, `categories`, `homepage` (via workspace), `documentation`, `readme`, `license` (via workspace), `repository` (via workspace), `rust-version` (via workspace).
- [ ] Every keyword in every crate matches the regex `^[a-z][a-z0-9-]{0,19}$` (lowercase, ≤ 20 chars, alphanumeric+dash, starts with letter).
- [ ] Every category in every crate appears in the official crates.io taxonomy (verify against the RFC's pre-cleared list).
- [ ] `grep -nE 'path = "\.\./' crates/*/Cargo.toml` shows every match also has `version = "0.1.0"` on the same line.
- [ ] `Cargo.toml`'s `[workspace.package]` includes `rust-version = "<MSRV>"`.
- [ ] `[workspace.lints]` table is populated with the RFC's exact lint set.
- [ ] Each PUBLISH crate has `crates/<name>/README.md` as a placeholder file (≥ 1 line, non-empty).
- [ ] `cargo package -p <crate> --no-verify --allow-dirty --list` succeeds for every PUBLISH crate; output does NOT include `target/`, `.git/`, swap/backup files, or unrelated working-dir artifacts.
- [ ] `cargo check --workspace --all-features` succeeds.
- [ ] `cargo clippy --workspace --all-features` produces no NEW `-D warnings` failure (warnings are acceptable; errors are not). Existing clippy warnings recorded in the commit body for Phase 04 cleanup.
- [ ] `cargo build --workspace` succeeds.
- [ ] Any INTERNAL-ONLY crate per the RFC carries `publish = false`.

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- Edit: `Cargo.toml` — workspace.package fields + workspace.lints tables + (optionally) rust-version.workspace = true wiring.
- Edit: `crates/detritus-protocol/Cargo.toml` — metadata block.
- Edit: `crates/detritus-client/Cargo.toml` — metadata block + path-dep versioning.
- Edit: `crates/detritus-server/Cargo.toml` — metadata block + path-dep versioning.
- New: `crates/detritus-protocol/README.md` (placeholder).
- New: `crates/detritus-client/README.md` (placeholder).
- New: `crates/detritus-server/README.md` (placeholder).
- Edit: `Cargo.lock` (auto-regenerated).

## Pitfalls

- **Workspace inheritance bug for `authors`.** The
  `authors.workspace = true` field requires
  `authors = [...]` in `[workspace.package]`. If the workspace
  block doesn't declare it, cargo errors with "key `authors`
  must be in the workspace's manifest". Same for any
  `<field>.workspace = true` reference — declare at workspace
  scope first.

- **`documentation` is per-crate, not workspace.** docs.rs
  hosts at `https://docs.rs/<crate-name>` per crate — a
  workspace-level docs URL would point at one crate and
  mislead users browsing the others. Each crate's
  Cargo.toml owns its `documentation = ".../docs/<name>"`.

- **`readme = "../../README.md"` doesn't work for crates.io.**
  Cargo packages the file referenced by `readme` and uploads
  it as the crate page. The path is resolved relative to the
  crate's Cargo.toml; relative paths escaping the crate
  directory don't make it into the `.crate` archive. Each
  crate must own its own `README.md` (placeholder in this
  phase; real content in Phase 04).

- **Keyword regex enforcement.** crates.io rejects on
  publish with errors like "invalid keyword". Test each
  keyword against the regex named in acceptance criteria
  *before* dry-running. A keyword with an underscore (e.g.
  `crash_reporting`) gets rejected — use a dash
  (`crash-reporting`).

- **MSRV declared too high.** Setting
  `rust-version = "1.85"` is the obvious value for edition
  2024 + resolver 3. But if any *dependency* requires a
  newer compiler, our MSRV must accommodate. Verify with:

  ```sh
  cargo +1.85.0 check --workspace 2>&1 | grep -A2 "requires rustc"
  ```

  If the check fails, raise the MSRV to the actually-needed
  minimum, and update the RFC accordingly.

- **`cargo publish` strips path-only deps from the .crate
  archive but rejects upload if they lack a `version =`
  entry.** This is the specific failure the dual path+version
  form fixes. Don't try to be clever by omitting the path
  entirely — local development still benefits from path-first
  resolution.

- **`[workspace.lints]` inheritance not automatic.** Each
  crate must opt in with `[lints] workspace = true`. The
  RFC names this; the per-crate Cargo.toml block must
  include it. Currently every detritus crate already has
  `[lints] workspace = true` (verified pre-phase) — but if
  a crate gets created later, it'll need the same.

- **`publish = false` semantics.** Setting it on the
  workspace-level Cargo.toml does NOT propagate to member
  crates (cargo doesn't support that inheritance). It must
  appear in each member's `[package]` block for INTERNAL-ONLY
  crates.

## Reference

- Phase 01 RFC: `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md` (named section "crates.io metadata" provides the values).
- Phase 02 license migration: `docs/planning/detritus-crates-io-finalization/02-license-migration-to-eupl.md` (must be committed first).
- Cargo `publish` docs: <https://doc.rust-lang.org/cargo/reference/publishing.html>.
- Cargo `[workspace.package]` docs: <https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table>.
- Cargo `[workspace.lints]` docs: <https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table>.
- Sequencing: depends on Phases 01 + 02. Blocks Phase 07 (dry-run consumes the metadata). Independent of Phases 04, 05, 06 in principle; in practice Phase 04 wants the metadata stable so the rustdoc links resolve, and Phase 06 wants the MSRV stable so the CI matrix names the right toolchain.
