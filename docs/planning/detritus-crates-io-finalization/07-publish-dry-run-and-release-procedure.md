# Phase 07 — `cargo publish --dry-run` verification + RELEASING.md

> **Recommended Codex model: GPT 5.5 medium**
>
> The dry-run itself is mechanical (`cargo publish --dry-run`
> in topological order). But this phase is the final
> verification gate — surfacing any gap from Phases 01-06
> that the agent's earlier checks missed. The agent must
> read the `.crate` archive contents, validate the published
> README renders correctly (best-effort given offline
> session), reserve names on crates.io (the verify-before-
> publish step from Phase 01), and write a RELEASING.md
> procedure document that the project owner runs at tag
> time. Moderate complexity, leaf execution.
> `low` would skip the post-dry-run inspection rigor;
> `high` is overkill for a verification phase with
> well-defined commands.

## Working tree

`/data/nvme0/can/Projects/detritus`. Phases 02-06 all
committed and green. This phase verifies the entire chain
and produces the release runbook.

## Goal

`cargo publish --dry-run` succeeds for `detritus-protocol`,
`detritus-client`, and `detritus-server` in topological dep
order on a clean checkout, with no warnings about missing
metadata, missing README, leaked files, or unspecified
license. The packaged `.crate` archive for each crate, when
extracted, contains exactly the files Phase 05's package-list
acceptance prescribed. A `RELEASING.md` at repo root
documents the per-release procedure (version bump locations,
CHANGELOG move, tag creation, post-publish verification) so
the project owner can cut v0.1.0 (and subsequent versions)
without consulting external docs. Crate names are confirmed
available on crates.io (if network is available; otherwise
the RELEASING.md flags the name-reservation step as the
operator's first action).

## Why this matters now

This is the integration test for the entire publish-readiness
push. If any of Phases 01-06 left a gap, the dry-run surfaces
it here — but only here. Without this phase, the next time
anyone runs `cargo publish` they hit the issues in production
(against the real crates.io, which doesn't have a "dry-run
mode" once you're past the prepare step).

A RELEASING.md is the second deliverable: even with green CI
and a working `release.yml` workflow, the human operator
still needs to know what to do (tag from where, CHANGELOG
shape, post-publish steps like `cargo yank` if needed).

### Cost of deferring

- The first real publish goes through ad-hoc, with the
  operator discovering procedure questions in real-time.
- Mistakes during the v0.1.0 publish are permanent —
  crates.io disallows re-using a version number, even after
  yanking.
- Subsequent releases lack a checklist; semver bumps,
  CHANGELOG moves, and version-string consistency degrade
  over time.

## Out of scope

- The actual `cargo publish` to crates.io (not dry-run).
  That's the operator's job at tag time, executed via
  Phase 06's release workflow.
- Setting up the crates.io API token. The operator does this
  once via the crates.io account settings page;
  RELEASING.md references the procedure without
  performing it.
- Reserving crate names by actually creating placeholder
  uploads. If a name is taken, fall back per Phase 01's RFC
  alternatives, NOT by squatting.
- Refactoring the workspace to fix issues the dry-run
  surfaces. If the dry-run fails, the failure must trace
  to one of Phases 02-06's acceptance criteria — reopen
  that phase, not this one.
- Adding more examples / tests / docs.
- Bumping the version above 0.1.0.

## Plan

1. **Verify all prior phases are merged.** Each phase's
   work may have landed under its own commit OR bundled
   into an adjacent commit. Verify by artifact state, not
   by commit subject:

   ```sh
   git log --oneline -10
   git status --short
   ```

   Then check the artifacts each phase was supposed to
   produce:

   - **Phase 01 (RFC):**
     `test -f docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`
   - **Phase 02 (license):**
     `test -f LICENSE && grep -q "Apache License" LICENSE && \
      grep -q 'license = "Apache-2.0"' Cargo.toml && \
      test -f CHANGELOG.md`
   - **Phase 03 (metadata + dep versioning):** every
     PUBLISH crate's `Cargo.toml` has `description`,
     `keywords`, `categories`, `readme`, `documentation`;
     workspace `Cargo.toml` has `rust-version` and a
     populated `[workspace.lints]` table; every internal
     path dep carries `version = "0.1.0"`:

     ```sh
     for c in detritus-protocol detritus-client detritus-server; do
       grep -qE "^description" crates/$c/Cargo.toml || echo "FAIL: $c missing description"
       grep -qE "^keywords" crates/$c/Cargo.toml || echo "FAIL: $c missing keywords"
       grep -qE "^categories" crates/$c/Cargo.toml || echo "FAIL: $c missing categories"
     done
     grep -qE "^rust-version" Cargo.toml || echo "FAIL: rust-version not set"
     grep -qE "^\[workspace\.lints\." Cargo.toml || echo "FAIL: workspace.lints not set"
     grep -nE 'path = "\.\./' crates/*/Cargo.toml | grep -v 'version = "0\.1\.0"' \
       && echo "FAIL: path dep without version qualifier"
     ```

   - **Phase 04 (READMEs + rustdoc):** every PUBLISH crate
     has `README.md` ≥ 60 lines;
     `cargo doc --workspace --no-deps --all-features` exits
     0; `cargo test --workspace --doc` passes.

   - **Phase 05 (examples + CHANGELOG):** every PUBLISH
     crate has `examples/<at-least-one>.rs`;
     `cargo build --workspace --examples` exits 0;
     `CHANGELOG.md` has an `## [Unreleased]` section
     populated.

   - **Phase 06 (CI):**
     `test -f .forgejo/workflows/ci.yml && test -f .forgejo/workflows/release.yml`.

   If any artifact check fails, stop and reopen the prior
   phase. The commit history may not look canonical (work
   bundled into adjacent commits is fine for bisect; the
   artifact contract is the source of truth).

2. **Run a clean-checkout dry-run.** Use a fresh worktree
   so any uncommitted state in the active checkout doesn't
   pollute the test:

   ```sh
   wt=$(mktemp -d)
   git -C /data/nvme0/can/Projects/detritus worktree add "$wt" HEAD
   cd "$wt"
   for c in detritus-protocol detritus-client detritus-server; do
     echo "=== $c ==="
     cargo publish --dry-run -p "$c" 2>&1 | tee /tmp/${c}-dryrun.log
   done
   ```

   Each must exit 0. The `--allow-dirty` flag is NOT used
   here — the clean checkout should have no dirty files.
   If cargo reports "package would include files not in
   git", Phase 05's hygiene work missed a file.

3. **Inspect each `.crate` archive.** `cargo publish
   --dry-run` produces a `.crate` file under
   `target/package/`. For each:

   ```sh
   for c in detritus-protocol detritus-client detritus-server; do
     echo "=== $c ==="
     tar -tzf target/package/${c}-0.1.0.crate | head -50
     echo "..."
     ls -lh target/package/${c}-0.1.0.crate
   done
   ```

   Verify:
   - Contains `Cargo.toml`, `Cargo.lock`, `LICENSE`,
     `README.md`, `src/`, `examples/`, `tests/`.
   - Does NOT contain `target/`, `.git/`, IDE files,
     `*.swp`, large binary fixtures.
   - Size is reasonable (< 500 KB per crate is typical
     for a v0.1.0 SDK without binary assets).

4. **Validate the README renders correctly.** Cargo's
   dry-run packages the README but doesn't render it.
   Approximate by running a Markdown linter:

   ```sh
   nix run nixpkgs#markdownlint-cli -- crates/*/README.md
   ```

   Address any errors (broken links, bad heading structure,
   missing code-block language tags). Warnings (line
   length) can be acknowledged in the commit body but don't
   block.

5. **Reserve crate names on crates.io (if network
   available).** If the agent can reach crates.io:

   ```sh
   for c in detritus-protocol detritus-client detritus-server; do
     status=$(curl -sf -o /dev/null -w "%{http_code}" \
       "https://crates.io/api/v1/crates/${c}")
     echo "$c → HTTP $status"
   done
   ```

   - `200` → name is taken; reopen Phase 01's RFC and pick
     the documented fallback name. Update Cargo.toml's
     `name` field and re-run the dry-run from step 2.
   - `404` → name is free; proceed.

   If the agent has no network access in this session, the
   RELEASING.md documents this check as the operator's
   first action.

6. **Author `RELEASING.md`.** File at repo root. Structure:

   ```md
   # Release procedure

   This document describes how to cut a new release of the
   detritus workspace. The workspace publishes three crates
   to crates.io in topological dependency order. Versions
   are synchronized across all three during v0.x.

   ## Prerequisites

   - You have a crates.io account with `publish` ownership
     on all three crates. To verify: `cargo owner --list
     detritus-protocol` (etc.).
   - `CARGO_REGISTRY_TOKEN` is configured as a Codeberg
     repo secret (one-time setup; see Phase 06).
   - You're on a clean checkout of `trunk` with CI green
     on the latest commit.

   ## Procedure

   1. **Bump versions.** Edit
      `Cargo.toml`'s `[workspace.package]` `version` (if
      using workspace-level version inheritance) OR each
      `crates/*/Cargo.toml`'s `[package].version`. Bump
      every PUBLISH crate to the new version
      simultaneously.

   2. **Bump internal dep versions.** In each crate that
      depends on a sibling, update the `version = "X.Y.Z"`
      qualifier on the path dep:

      ```sh
      grep -nE 'detritus-(protocol|client|server) = \{.*version' crates/*/Cargo.toml
      ```

      Each line shows the version. Bump together.

   3. **Move CHANGELOG entries.** In `CHANGELOG.md`, rename
      the `## [Unreleased]` heading to
      `## [X.Y.Z] — YYYY-MM-DD`, then add a fresh empty
      `## [Unreleased]` section above it.

   4. **Commit the bump.**

      ```sh
      git add Cargo.toml crates/*/Cargo.toml CHANGELOG.md
      git commit -m "release: vX.Y.Z"
      ```

   5. **Verify CI is green on this commit.** Push and wait
      for the `ci` workflow to complete:

      ```sh
      git push origin trunk
      # Watch CI at codeberg.org/caniko/rs-detritus/actions
      ```

   6. **Tag and push.**

      ```sh
      git tag vX.Y.Z
      git push origin vX.Y.Z
      ```

      The `release.yml` workflow triggers automatically.
      It runs `cargo publish` for each crate in order
      (protocol → client → server) using the
      `CARGO_REGISTRY_TOKEN` secret.

   7. **Verify the publish.** After ~5 minutes:

      ```sh
      for c in detritus-protocol detritus-client detritus-server; do
        curl -sf "https://crates.io/api/v1/crates/${c}" \
          | jq -r '.crate.max_version'
      done
      ```

      Each prints `X.Y.Z`. docs.rs builds run automatically
      and complete within ~30 minutes (verify at
      `https://docs.rs/<crate>/X.Y.Z`).

   ## If something goes wrong

   - **Wrong version published.** `cargo yank --version
     X.Y.Z -p <crate>`. Yanked versions remain
     downloadable but won't satisfy new resolves. Then
     publish a corrected `X.Y.Z+1`.
   - **Wrong crate name.** Cannot rename a crate after
     publishing. Yank, publish under a new name, update
     consumers.
   - **Lost the token.** Revoke at crates.io/settings,
     generate a new one, update the Codeberg secret.

   ## First release (v0.1.0) special steps

   The very first publish has additional considerations:
   1. Verify each crate name is free (Phase 07 step 5).
   2. Run `cargo owner --add <username> <crate>` for each
      after first publish to allow co-owners.
   3. Verify docs.rs builds the doc set (the first build
      may take longer due to no cache).
   ```

7. **Wire `RELEASING.md` from the workspace README.** Add a
   one-line link in the workspace `README.md` under the CI
   section:

   ```md
   - Release procedure: [RELEASING.md](RELEASING.md)
   ```

8. **Commit.** Single commit. Subject:

   ```
   docs: RELEASING.md + verify cargo publish dry-run
   ```

   Body: list the dry-run results per crate (exit code,
   .crate size, file count), the name-availability check
   results (or note "operator must verify pre-tag"), and
   the markdownlint result.

## Acceptance criteria

- [ ] `cargo publish --dry-run -p detritus-protocol` exits 0 on a clean checkout (no `--allow-dirty`).
- [ ] `cargo publish --dry-run -p detritus-client` exits 0 on a clean checkout.
- [ ] `cargo publish --dry-run -p detritus-server` exits 0 on a clean checkout.
- [ ] Each `.crate` archive contains: `Cargo.toml`, `Cargo.lock`, `LICENSE`, `README.md`, `src/`, `examples/`, `tests/`. None contain `target/`, `.git/`, editor backups, or files exceeding 1 MB in `src/`.
- [ ] `markdownlint-cli` exits with no errors against each crate's `README.md` and against the new `RELEASING.md`.
- [ ] `RELEASING.md` exists at repo root and documents: prerequisites, the 7-step release procedure, troubleshooting (yank, wrong-crate-name, lost-token), and first-release special steps.
- [ ] Workspace `README.md` references `RELEASING.md` from a one-line link.
- [ ] If network was available during this phase: each PUBLISH crate name was verified free on crates.io OR the workspace's `Cargo.toml` was updated to the RFC's fallback name (with the change committed and the RFC's status updated to reflect the chosen name).
- [ ] If network was NOT available: the RELEASING.md "first release" section explicitly names the name-availability check as the operator's first action.
- [ ] Commit body lists per-crate `.crate` archive size in bytes and file count, plus the dry-run exit code per crate.

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- New: `RELEASING.md`.
- Edit: `README.md` — add link to `RELEASING.md`.
- (Conditional) Edit: `crates/<name>/Cargo.toml` if a name needed swapping to a fallback per step 5.
- (Conditional) Edit: `Cargo.toml` if the workspace-level `description.workspace = true` or other inheritance is updated due to name changes.

## Pitfalls

- **`cargo publish --dry-run` requiring network.** The dry
  run downloads the registry index to validate
  `version = "..."` qualifiers on deps. If the local
  environment has no network, the command fails with a
  cryptic error. Acceptance test: if cargo says
  "index not found" or "could not contact registry", the
  agent has no network — fall back to running just
  `cargo package --no-verify -p <crate>` which doesn't
  contact the registry. Note this limitation in the
  commit body and have the operator re-run the dry-run
  on a connected machine before tagging.

- **Name collision discovery on the day of release.**
  Phase 01's RFC pre-clears fallback names, but the
  fallback hasn't been tested by Phase 07's dry-run. If a
  name collision forces a switch to a fallback, repeat
  steps 2-4 from this phase with the new name to verify
  nothing depends on the old name internally (a stray
  `use detritus_client::...` line in an example would
  break).

- **`Cargo.lock` exclusion from library crates.**
  Convention: library crates omit `Cargo.lock` from the
  published archive (cargo handles this automatically —
  it includes Cargo.lock only for crates with a `[[bin]]`
  target). `detritus-server` ships its lockfile;
  `detritus-protocol` and `detritus-client` don't. Don't
  fight this; it's the correct behavior. Just verify in
  step 3's archive inspection that the right behavior is
  happening.

- **Empty `[features]` sections triggering a warning.**
  Phase 03 may have created a `[features]` table with
  only `default = [...]`. That's fine. But an empty
  `[features]` block produces a clippy warning on some
  versions; just remove the empty block if it exists.

- **Publishing in the wrong order.** crates.io requires
  the dep tree to be published bottom-up. If
  `detritus-client` is published before
  `detritus-protocol`, the dep can't resolve. The order
  is hardcoded in both this phase's step 2 and the
  RELEASING.md procedure; don't reorder.

- **Yanking is not deletion.** A common mistake is
  yanking a published crate version and assuming the
  source is gone from crates.io. The .crate file remains
  downloadable indefinitely. The lesson: be very sure
  before publishing — yank limits new resolves but
  doesn't undo distribution.

- **`cargo owner` not configured pre-publish.** Only the
  publishing user owns the crate by default. To allow a
  co-owner (project organization, other maintainer),
  run `cargo owner --add <username> <crate>` AFTER the
  first publish. RELEASING.md documents this; don't skip
  it.

## Reference

- Phase 01-06 all merged.
- crates.io API for name check: <https://crates.io/api/v1/crates/{name}>.
- `cargo publish` reference: <https://doc.rust-lang.org/cargo/commands/cargo-publish.html>.
- `cargo yank` reference: <https://doc.rust-lang.org/cargo/commands/cargo-yank.html>.
- `cargo owner` reference: <https://doc.rust-lang.org/cargo/commands/cargo-owner.html>.
- Sequencing: depends on all of Phases 01-06 being merged. Last phase of the plan. After this commit lands, the project owner can tag v0.1.0 and trigger the real publish via Phase 06's release.yml workflow.
