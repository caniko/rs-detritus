# Release procedure

This document describes how to cut a new release of the detritus workspace.
The workspace publishes three crates to crates.io in topological dependency
order. Versions are synchronized across all three during v0.x.

## Prerequisites

- You have a crates.io account with `publish` ownership on all three crates.
  To verify: `cargo owner --list detritus-protocol` (etc.).
- `CRATES_IO_API_TOKEN` is configured as a Codeberg repo secret (one-time
  setup; see `.forgejo/workflows/release.yml`).
- You're on a clean checkout of `trunk` with CI green on the latest commit.

## Procedure

1. **Bump versions.** Edit each `crates/*/Cargo.toml`'s `[package].version`.
   Bump every PUBLISH crate to the new version simultaneously:

   ```
   detritus-protocol/Cargo.toml  → version = "X.Y.Z"
   detritus-client/Cargo.toml    → version = "X.Y.Z"
   detritus-server/Cargo.toml    → version = "X.Y.Z"
   ```

2. **Bump internal dep versions.** In each crate that depends on a sibling,
   update the `version = "X.Y.Z"` qualifier on the path dep:

   ```sh
   grep -nE 'detritus-(protocol|client|server) = \{.*version' crates/*/Cargo.toml
   ```

   Each line shows the version. Bump together with step 1.

3. **Move CHANGELOG entries.** In `CHANGELOG.md`, rename the
   `## [Unreleased]` heading to `## [X.Y.Z] — YYYY-MM-DD`, then add a fresh
   empty `## [Unreleased]` section above it.

4. **Commit the bump.**

   ```sh
   git add Cargo.toml crates/*/Cargo.toml CHANGELOG.md
   git commit -m "release: vX.Y.Z"
   ```

5. **Verify CI is green on this commit.** Push and wait for the `ci` workflow:

   ```sh
   git push origin trunk
   # Watch CI at codeberg.org/caniko/rs-detritus/actions
   ```

6. **Tag and push.**

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

   The `release.yml` workflow triggers automatically. It runs `cargo publish`
   for each crate in topological order (protocol → server → client) using the
   `CRATES_IO_API_TOKEN` secret.

7. **Verify the publish.** After ~5 minutes:

   ```sh
   for c in detritus-protocol detritus-server detritus-client; do
     curl -sf "https://crates.io/api/v1/crates/${c}" \
       | jq -r '.crate.max_version'
   done
   ```

   Each prints `X.Y.Z`. docs.rs builds run automatically and complete within
   ~30 minutes (verify at `https://docs.rs/<crate>/X.Y.Z`).

## If something goes wrong

- **Wrong version published.** `cargo yank --version X.Y.Z -p <crate>`. Yanked
  versions remain downloadable but won't satisfy new resolves. Then publish a
  corrected `X.Y.Z+1`.
- **Wrong crate name.** Cannot rename a crate after publishing. Yank, publish
  under the new name, update consumers.
- **Lost the token.** Revoke at crates.io/settings, generate a new one, update
  the Codeberg secret.

## First release (v0.1.0) special steps

1. **Verify each crate name is free** before tagging:

   ```sh
   for c in detritus-protocol detritus-server detritus-client; do
     status=$(curl -sf -o /dev/null -w "%{http_code}" \
       "https://crates.io/api/v1/crates/${c}" -A "detritus-publish-check/1.0")
     echo "$c → HTTP $status"   # 404 = free, 200 = taken
   done
   ```

   As of 2026-05-19, all three names returned HTTP 404 (free).

2. **Add co-owners** after first publish:

   ```sh
   for c in detritus-protocol detritus-server detritus-client; do
     cargo owner --add <username> "$c"
   done
   ```

3. **Verify docs.rs** built the doc set (the first build may take longer due
   to no cache).
