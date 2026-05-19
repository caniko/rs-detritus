# Phase 02 — Consolidate workspace license to Apache-2.0

> **Recommended Codex model: GPT 5.5 low**
>
> The license change itself is mechanical (one workspace field,
> one LICENSE file at repo root, one flake.nix meta key, one
> CHANGELOG entry). The current `MIT OR Apache-2.0` declaration
> ships no LICENSE files — this phase replaces the dual
> declaration with `Apache-2.0` and ships the canonical license
> text. No dep-license complications: Apache-2.0 is the most
> compatible permissive license in the Rust ecosystem. Leaf
> execution, trivial complexity. `medium` would over-engineer a
> straightforward license consolidation.

## Working tree

`/data/nvme0/can/Projects/detritus`. Phase 01's RFC should be
committed first so the workspace decisions are traceable, but
the license consolidation itself doesn't depend on RFC content
(the license is pre-decided as `Apache-2.0`).

## Goal

The workspace's declared license is `Apache-2.0`. A `LICENSE`
file at the repo root contains the canonical Apache-2.0 text.
Any prior `LICENSE-MIT`, `LICENSE-APACHE`, or `LICENSE-*` files
(none exist today; verified pre-phase) are absent. The
`flake.nix` `meta.license` collapses from `[ mit asl20 ]` to
just `pkgs.lib.licenses.asl20`. A `CHANGELOG.md` entry records
the license consolidation. `cargo build --workspace` still
compiles cleanly.

## Why this matters now

User decision (this conversation, 2026-05-19): consolidate from
the dual `MIT OR Apache-2.0` declaration to **`Apache-2.0`
only**. Rationale: keeping the SDK permissive lets regicide
remain license-flexible (commercial Steam release on the
roadmap). Apache-2.0 is the most ecosystem-compatible
permissive choice (better patent-grant clause than MIT, widely
accepted by downstreams). The current state declares a dual
license without shipping the underlying text:

```toml
[workspace.package]
license = "MIT OR Apache-2.0"
```

…and `flake.nix` mirrors it:

```nix
meta = {
  …
  license = with pkgs.lib.licenses; [ mit asl20 ];
  …
};
```

This dual declaration is technically out of compliance — both
MIT and Apache-2.0 require their text to ship with the work,
and neither LICENSE file exists in the repo today. This phase
collapses to a single license AND ships the text, fixing both
issues at once.

### Cost of deferring

- Each subsequent phase (metadata, READMEs, CI, dry-run) bakes
  in the license string. Changing it mid-flight forces a sweep.
- A v0.1.0 release with the wrong license is published
  permanently — crates.io disallows re-releasing the same
  version, and yanking does not change the license metadata on
  the existing crate page.
- Contributors don't know which license to apply to new code
  during the gap.

## Out of scope

- Choosing a different license. The user has named
  `Apache-2.0`. This phase implements that decision; it does
  not relitigate.
- Keeping the dual `MIT OR Apache-2.0` declaration.
  Consolidating to a single license simplifies the metadata
  surface and avoids the dual-licensing dance for downstreams.
- Per-file SPDX headers. Avoid touching every `.rs` file with
  an SPDX comment — that's churn; the workspace-level
  `license = "Apache-2.0"` in Cargo.toml is the authoritative
  declaration.
- Renaming the project, changing the copyright holder line, or
  contacting prior contributors for relicensing consent. The
  repo has only three commits and one author (verified
  pre-phase via `git log --pretty=format:'%an' | sort -u`), so
  unilateral consolidation is uncontroversial. (Moving from
  `MIT OR Apache-2.0` to `Apache-2.0` only is a narrowing of
  options the project offered to consumers; legally it's still
  the same author's choice.)

## Plan

1. **Verify the contributor list.** From the detritus repo:

   ```sh
   git log --all --pretty=format:'%an <%ae>' | sort -u
   ```

   Confirm there's exactly one author (the project owner). If
   any other name appears, stop and surface — relicensing
   requires per-contributor consent, and this phase will not
   handle that workflow.

2. **Locate any existing LICENSE files.** From the repo root:

   ```sh
   ls -la LICENSE* COPYING* NOTICE* 2>/dev/null
   ```

   Inventory the current state. If `LICENSE-MIT` or
   `LICENSE-APACHE` files exist, they'll be deleted in step 5.
   If a generic `LICENSE` file exists, it'll be overwritten.

3. **Sanity-check dep licenses.** Apache-2.0 is permissive,
   so consuming any combination of permissive/copyleft deps
   is fine — the published `.crate` archives contain only
   our own source. Still, surface any dep with an
   `UNKNOWN`/proprietary/exotic license so it can be
   addressed before publish:

   ```sh
   cargo metadata --format-version 1 \
     | jq -r '.packages[] | "\(.name)=\(.license // "UNKNOWN")"' \
     | sort -u | tee /tmp/dep-licenses.tsv
   grep -E "=UNKNOWN|proprietary" /tmp/dep-licenses.tsv
   ```

   Empty output is expected and required. If any
   non-standard license appears, surface it in the commit
   body (informational only — does not block this phase).

4. **NOTICE file decision.** Apache-2.0 doesn't require a
   NOTICE file unless the project itself imports
   Apache-2.0-licensed third-party NOTICE clauses (most
   library deps don't carry such clauses; verify in step 3).
   Default: skip NOTICE creation. Re-evaluate if step 3
   surfaces a dep with a NOTICE that must be reproduced.

5. **Write the LICENSE file.** Source: the official
   Apache-2.0 text from <https://www.apache.org/licenses/LICENSE-2.0.txt>
   or the SPDX-published version at
   <https://spdx.org/licenses/Apache-2.0.html>. Save as
   `/data/nvme0/can/Projects/detritus/LICENSE`. The file
   must be the **unmodified** plain-text English version.
   Do not paraphrase, abridge, or annotate.

   Verify the file's first non-empty line reads:

   ```
                                    Apache License
                              Version 2.0, January 2004
   ```

   The file is ~11 KB. The Apache-2.0 text does NOT itself
   contain an embedded SPDX identifier line — Apache's
   canonical text predates SPDX. The SPDX declaration lives
   in `Cargo.toml` via `license = "Apache-2.0"`.

6. **Remove obsolete license files (if any).** Verified
   pre-phase: no `LICENSE-MIT`, `LICENSE-APACHE`, or
   `COPYING-*` files exist in the repo today (the prior dual
   declaration shipped no text). If step 2 surfaced any,
   `git rm` them:

   ```sh
   git rm LICENSE-MIT 2>/dev/null || true
   git rm LICENSE-APACHE 2>/dev/null || true
   ```

7. **Update `Cargo.toml`.** Edit
   [Cargo.toml](../../../../Cargo.toml) under
   `[workspace.package]`:

   ```diff
   - license = "MIT OR Apache-2.0"
   + license = "Apache-2.0"
   ```

   No other workspace fields change in this phase.

8. **Update `flake.nix`.** Edit the `meta` block:

   ```diff
   - license = with pkgs.lib.licenses; [ mit asl20 ];
   + license = pkgs.lib.licenses.asl20;
   ```

   (`asl20` is the canonical nixpkgs attribute for
   Apache-2.0 — short for "Apache Software License 2.0".
   Verify with
   `nix eval --raw 'github:NixOS/nixpkgs/nixos-unstable#lib.licenses.asl20.shortName'`
   if uncertain; expected output: `"Apache-2.0"`.)

9. **Update root `README.md`.** Append a `## License` section
   if not already present:

   ```md
   ## License

   Licensed under the [Apache License, Version 2.0](LICENSE).
   ```

   Do not refactor the rest of the README in this phase
   (that's Phase 04).

10. **Create or update `CHANGELOG.md`.** If no CHANGELOG exists
    yet (current state — none does), create one with the
    standard "Keep a Changelog" header:

    ```md
    # Changelog

    All notable changes to this project will be documented in
    this file.

    The format is based on
    [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
    and this project adheres to
    [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

    ## [Unreleased]

    ### Changed

    - License consolidated from the dual
      `MIT OR Apache-2.0` declaration to `Apache-2.0` only.
      The canonical Apache-2.0 text is now shipped at
      `LICENSE` (previously absent — the dual declaration
      had no underlying text files). Single-author project
      across three commits; consolidating the offered
      license options is uncontroversial.
    ```

    Phase 05 expands the CHANGELOG with additional entries; do
    not pre-populate them here.

11. **Verify the workspace still builds.**

    ```sh
    cargo check --workspace --all-targets 2>&1 | tail -5
    cargo build --workspace 2>&1 | tail -5
    ```

    Both exit 0. The license change is metadata-only — no
    source needs to recompile, but a stale lockfile or
    accidental edit elsewhere could break the build. Catch it
    here, not in Phase 07.

12. **Commit.** Single commit. Subject:

    ```
    license: consolidate workspace to Apache-2.0
    ```

    Body: note the prior dual-declaration without LICENSE
    files (out-of-compliance state), the consolidation to a
    single permissive license, the canonical Apache-2.0 text
    now shipping at `LICENSE`, the single-author history
    that permits consolidation, and a summary of the
    dep-license audit (e.g. "47 direct deps surveyed, no
    UNKNOWN/proprietary licenses").

## Acceptance criteria

- [ ] `Cargo.toml`'s `[workspace.package]` block has `license = "Apache-2.0"` and only that license (no dual-license string).
- [ ] `LICENSE` file at repo root exists, has > 9000 bytes (sanity check — Apache-2.0 full text is ~11 KB), and its first non-empty line block contains `Apache License` and `Version 2.0, January 2004`.
- [ ] `flake.nix`'s `meta.license` is `pkgs.lib.licenses.asl20` (single license, not a list).
- [ ] `nix eval --raw '.#packages.x86_64-linux.default.meta.license.shortName'` returns `"Apache-2.0"`.
- [ ] `README.md` contains a `## License` section pointing at the `LICENSE` file with the Apache-2.0 name.
- [ ] `CHANGELOG.md` exists with a Keep-a-Changelog header and an Unreleased / Changed entry naming the license consolidation.
- [ ] `cargo metadata --format-version 1 | jq -r '.packages[] | .license' | sort -u` produces no `UNKNOWN` or proprietary licenses among the resolved deps. (Any non-permissive copyleft licenses surfaced here are fine — Apache-2.0 can consume them; just document the list in the commit body.)
- [ ] `cargo build --workspace` succeeds.
- [ ] No `LICENSE-MIT`, `LICENSE-APACHE`, `COPYING-*`, `NOTICE-*` files remain in the repo (verified via `ls -la`).
- [ ] Commit body documents the dep-license audit summary.

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- New: `LICENSE` (Apache-2.0 canonical text).
- New: `CHANGELOG.md`.
- Edit: `Cargo.toml` (workspace.package license).
- Edit: `flake.nix` (meta.license).
- Edit: `README.md` (append `## License` section).
- (Conditional) Delete: any pre-existing `LICENSE-*` / `COPYING-*` files.

## Pitfalls

- **Paraphrasing the Apache-2.0 text.** The license text is
  reproduced verbatim by convention. Don't reformat the
  pre-formatted heading block (the centered "Apache License /
  Version 2.0, January 2004" lines depend on whitespace). If
  the agent re-wraps long paragraphs, that's acceptable but
  not necessary; ship the upstream text byte-for-byte if
  practical.

- **`asl20` attribute name in nixpkgs.** The canonical name
  for Apache-2.0 in `lib/licenses.nix` is `asl20` ("Apache
  Software License 2.0"). There's also `apache2` as an alias
  in some older nixpkgs — verify which one your nixpkgs pin
  exposes. The shortName resolves to `"Apache-2.0"` and the
  spdxId to `"Apache-2.0"` regardless of which attribute name
  you use.

- **Existing dual `MIT OR Apache-2.0` declaration without
  text files.** That's technically out-of-compliance with
  both licenses (which require their text to ship with the
  work). This consolidation fixes that as a side-effect.
  Don't pre-add the missing MIT LICENSE file before
  consolidating — that creates pointless churn for a license
  you're about to drop.

- **Downstream regicide consuming detritus.** Regicide's
  workspace
  (`/data/nvme0/can/Projects/solo/game-dev/regicide`) pulls
  detritus via path or git URL. Apache-2.0 is fully
  permissive — regicide can be licensed under anything
  (including future commercial). No regicide changes
  required by this phase.

- **`git log` showing the WIP modified files as having a
  different author.** The `git status` shows uncommitted
  changes, but `git log` only counts committed work. The
  step 1 check is authoritative.

- **Forgetting CHANGELOG semantics.** "Unreleased" sections
  accumulate until a release tag bumps them into a
  versioned section. Don't mark this entry as `## [0.2.0]
  — 2026-05-19` — there's no release yet. Phase 07's
  RELEASING.md procedure will move the entry when v0.1.0
  cuts.

- **NOTICE-clause obligations from deps.** Apache-2.0 §4(d)
  requires that any NOTICE files supplied by Apache-2.0
  dependencies be reproduced in derivative works. For a
  Rust SDK publishing only its own source via
  `cargo publish`, this typically does NOT apply (the
  .crate archive does not bundle dep source). But if your
  release artifacts ever include vendored dep source
  (e.g. a Nix-built binary that statically links Apache-2.0
  crates), you'll need to reproduce their NOTICE files at
  that distribution boundary. Out of scope for this phase;
  flag in Phase 07's RELEASING.md if applicable.

## Reference

- Phase 01 RFC (committed first): `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`.
- Apache-2.0 text (canonical): <https://www.apache.org/licenses/LICENSE-2.0.txt>.
- SPDX Apache-2.0 page: <https://spdx.org/licenses/Apache-2.0.html>.
- Nixpkgs licenses table: `<nixpkgs>/lib/licenses.nix` — search for `asl20`.
- Keep a Changelog: <https://keepachangelog.com/en/1.1.0/>.
- Apache-2.0 explanation aimed at Rust projects: <https://choosealicense.com/licenses/apache-2.0/>.
- Sequencing: depends on Phase 01 (RFC). Blocks Phase 03 (metadata depends on the license being final). Independent of Phases 04, 05, 06 by file scope but should land before them so the license string they reference is stable.
