# Phase 06 — Forgejo CI for lint, build, test, publish dry-run

> **Recommended Codex model: GPT 5.5 low**
>
> Generating a Forgejo Actions workflow YAML for a Rust
> workspace is mechanical work that follows an established
> template (the project has both a `forgejo-ci` and
> `codeberg-ci` skill that pre-defines the runner labels and
> attic-publish patterns). The decisions are: which jobs to
> run on every push vs only on tag, which toolchain matrix
> (MSRV + stable + nightly?), and whether to run a
> `cargo publish --dry-run` in CI. Once those are pinned, the
> work is filling a template. `low` matches the mechanical
> nature; `medium` would only be needed if the CI design were
> novel, which it isn't.

## Working tree

`/data/nvme0/can/Projects/detritus`. Independent of Phases
02, 03, 04, 05 in file scope (CI adds new files under
`.forgejo/workflows/`). In execution order, Phase 06 wants
Phases 02-05 mostly green so the CI is exercising a
publish-ready state, but CI authoring can proceed in parallel
with them.

## Goal

A `.forgejo/workflows/ci.yml` workflow file that runs on every
push to `trunk` and every pull request, exercising:
- `cargo fmt --check`
- `cargo clippy --workspace --all-features --all-targets -- -D warnings`
- `cargo check --workspace --all-features`
- `cargo test --workspace --all-features` (unit + integration + doctest)
- `cargo doc --workspace --no-deps --all-features` (zero warnings)
- `cargo publish --dry-run -p <each crate>` (in topological dep order)

A second workflow `.forgejo/workflows/release.yml` triggers on
git tags matching `v[0-9]*.[0-9]*.[0-9]*` and runs `cargo
publish` (real, not dry-run) for each PUBLISH crate against
`crates.io`, using a `CARGO_REGISTRY_TOKEN` secret. The
release workflow is the only place that uses the secret;
the CI workflow does not.

## Why this matters now

Currently there is no CI. Every regression is caught locally
on the author's machine or discovered post-publish by
downstream users. Without CI, the publish-readiness work in
the preceding phases is auditable manually but not enforceable
on contributions.

For crates.io publishing specifically, the release workflow is
the mechanism that turns a `git tag v0.1.0` into a live
crates.io publish without ad-hoc shell commands on the
author's laptop. Eliminates "I forgot to bump versions before
publishing" and "I published from a dirty checkout" classes
of mistakes.

### Cost of deferring

- Phase 07's dry-run is a one-shot verification, not a
  continuous gate. Without CI, regressions reappear between
  releases.
- Releasing v0.1.0 manually means the author runs
  `cargo publish` from a laptop with their personal cargo
  credentials — increases the chance of an unintended
  publish (wrong version, wrong crate, secret leaked into
  shell history).
- Contributors (if any) have no green-light signal for PRs
  unless they run the full suite locally.

## Out of scope

- Self-hosting infrastructure beyond what the project's
  forgejo-ci / codeberg-ci skills already document. Use the
  shared atlas runner with the `atlas` label per project
  convention if available; otherwise fall back to
  Codeberg's shared `codeberg-small` runner.
- Code coverage reports / coveralls / codecov wiring.
  Possible follow-up; out of scope here.
- Multi-OS CI (macOS, Windows). The detritus server is
  Linux-only by design (axum + standard tokio stack works
  everywhere, but the operator story is Linux); add Windows
  in a follow-up only if a user requests it.
- MSRV-matrix CI (running every toolchain from MSRV to
  stable). One stable toolchain + one MSRV check is enough
  for v0.x.
- Auto-tagging or bumping versions from a commit message.
  Versions are bumped manually per the release procedure
  Phase 07 documents.
- Signing the published crates. crates.io doesn't sign
  uploads; the chain of trust is the API token. Don't try
  to PGP-sign the `.crate` files.

## Plan

1. **Inspect the project's CI conventions.** The user
   maintains a `forgejo-ci` skill at
   `/home/can/.claude/skills/forgejo-ci/` and a
   `codeberg-ci` skill at
   `/home/can/.claude/skills/codeberg-ci/`. Read both:

   ```sh
   ls /home/can/.claude/skills/forgejo-ci /home/can/.claude/skills/codeberg-ci 2>/dev/null
   cat /home/can/.claude/skills/codeberg-ci/SKILL.md
   ```

   Codeberg hosts detritus (`codeberg.org/caniko/rs-detritus`),
   so use the `codeberg-ci` template as the base. The atlas
   self-hosted runner with label `atlas` is the preferred
   runner per the project's conventions.

2. **Author `.forgejo/workflows/ci.yml`.** Structure:

   ```yaml
   name: ci

   on:
     push:
       branches: [trunk]
     pull_request:
       branches: [trunk]

   env:
     CARGO_TERM_COLOR: always
     RUSTFLAGS: "-D warnings"

   jobs:
     fmt:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         - run: rustup component add rustfmt
         - run: cargo fmt --all --check

     clippy:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         - run: rustup component add clippy
         - run: cargo clippy --workspace --all-features --all-targets -- -D warnings

     check:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         - run: cargo check --workspace --all-features

     test:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         - run: cargo test --workspace --all-features

     doc:
       runs-on: atlas
       env:
         RUSTDOCFLAGS: "-D warnings"
       steps:
         - uses: actions/checkout@v4
         - run: cargo doc --workspace --no-deps --all-features

     msrv:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         - run: rustup toolchain install <MSRV from RFC> --profile minimal
         - run: cargo +<MSRV> check --workspace --all-features

     publish-dry-run:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         # Order matters: protocol → client → server
         - run: cargo publish --dry-run -p detritus-protocol --allow-dirty
         - run: cargo publish --dry-run -p detritus-client --allow-dirty
         - run: cargo publish --dry-run -p detritus-server --allow-dirty
   ```

   Adapt the runner label and toolchain installation step
   per the codeberg-ci skill's actual template. The `atlas`
   label is the project's self-hosted-runner convention;
   if unavailable in the detritus repo context, fall back
   to `ubuntu-latest` or the Codeberg shared runner.

3. **Author `.forgejo/workflows/release.yml`.** Structure:

   ```yaml
   name: release

   on:
     push:
       tags:
         - 'v[0-9]+.[0-9]+.[0-9]+'

   jobs:
     publish:
       runs-on: atlas
       steps:
         - uses: actions/checkout@v4
         - run: cargo login "${{ secrets.CARGO_REGISTRY_TOKEN }}"
         - run: cargo publish -p detritus-protocol
         - run: cargo publish -p detritus-client
         - run: cargo publish -p detritus-server
       env:
         CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
   ```

   The `--dry-run` flag is intentionally absent here — this
   is the real publish. The CI workflow's `publish-dry-run`
   job runs on every push and catches regressions before
   tag-time.

   Document in the file's header comment that
   `CARGO_REGISTRY_TOKEN` must be configured as a Codeberg
   secret on the repo settings page before the workflow can
   succeed.

4. **Verify the workflow files lint.** Forgejo Actions uses
   the GitHub Actions schema. A local validator:

   ```sh
   nix run nixpkgs#actionlint -- .forgejo/workflows/ci.yml
   nix run nixpkgs#actionlint -- .forgejo/workflows/release.yml
   ```

   Both pass (or produce only warnings about
   self-hosted-runner labels, which is expected).

5. **Verify the CI jobs would pass locally.** Run each job's
   commands sequentially in the working tree:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-features --all-targets -- -D warnings
   cargo check --workspace --all-features
   cargo test --workspace --all-features
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
   cargo publish --dry-run -p detritus-protocol --allow-dirty
   cargo publish --dry-run -p detritus-client --allow-dirty
   cargo publish --dry-run -p detritus-server --allow-dirty
   ```

   Each must exit 0. If any fails, surface the issue —
   probably caused by an incomplete Phase 04 (clippy not
   clean) or Phase 03 (metadata still missing on a crate).

6. **Document the CI surface in the workspace README.** Add
   a one-paragraph "CI status" section to the workspace
   `README.md`:

   ```md
   ## CI

   - [CI status](https://codeberg.org/caniko/rs-detritus/actions/workflows/ci.yml)
   - On every push to `trunk` and every PR, CI runs:
     fmt, clippy, check, test, doc, MSRV check, and
     `cargo publish --dry-run` for each crate.
   - On every tag `vX.Y.Z`, the release workflow runs the
     real `cargo publish` to crates.io (token-gated).
   ```

7. **Commit.** Single commit. Subject:

   ```
   ci: add Forgejo Actions workflows for CI and tag-driven release
   ```

   Body: list the jobs (fmt/clippy/check/test/doc/msrv/dry-run),
   the runner label chosen, the release-workflow's
   tag-trigger pattern, and the operator one-time setup
   needed (configure `CARGO_REGISTRY_TOKEN` secret on the
   Codeberg repo settings).

## Acceptance criteria

- [ ] `.forgejo/workflows/ci.yml` exists with jobs: `fmt`, `clippy`, `check`, `test`, `doc`, `msrv`, `publish-dry-run`.
- [ ] `.forgejo/workflows/release.yml` exists, triggered on `v[0-9]+.[0-9]+.[0-9]+` tags, and runs `cargo publish` for each PUBLISH crate in dep-topological order.
- [ ] The release workflow references `secrets.CARGO_REGISTRY_TOKEN` and does NOT reference any other secret.
- [ ] `nix run nixpkgs#actionlint -- .forgejo/workflows/ci.yml` produces no errors.
- [ ] Every command in the CI workflow's jobs exits 0 when run locally on the current `trunk` state.
- [ ] The MSRV job uses the exact toolchain version named in `[workspace.package].rust-version`.
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` exits 0 (the doc job's strict-warnings mode is satisfiable).
- [ ] `cargo publish --dry-run -p <each crate> --allow-dirty` exits 0 for each PUBLISH crate.
- [ ] The workspace `README.md` references the CI workflow with a one-paragraph "CI" section.
- [ ] Commit body documents the one-time operator setup (configuring `CARGO_REGISTRY_TOKEN` on the Codeberg repo).

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- New: `.forgejo/workflows/ci.yml`.
- New: `.forgejo/workflows/release.yml`.
- Edit: `README.md` — add the `## CI` section.

## Pitfalls

- **`atlas` self-hosted runner availability in the detritus
  repo's Codeberg organization.** The atlas runner is
  registered for the project's primary organization; if
  detritus lives under a different org (`caniko/`), the
  runner may not be registered there. Verify; fall back to
  `runs-on: codeberg-small` (Codeberg's shared runner) or
  install the atlas runner under the new org if needed.

- **Rust toolchain installation in CI.** If the runner has
  no pre-installed Rust, every job must install it. Use
  `rustup-action` (Codeberg's mirror or
  `actions-rs/toolchain@v1`). The codeberg-ci skill has
  the canonical pattern; copy it.

- **Self-hosted runner secret exposure.** A bad CI job
  could `printenv | grep CARGO` and leak the token via
  logs. Confine the secret to the release workflow only;
  the CI workflow must not reference it. The release
  workflow should also redact the token from logs by using
  `cargo login "${SECRET}"` and not `echo "${SECRET}"`.

- **`cargo publish --dry-run` requiring network.** The dry
  run downloads the registry index. If the runner lacks
  network access to crates.io, the job fails. The atlas
  runner has network access; verify before relying on it.

- **Topological order in the release workflow.** crates.io
  imposes a "newly-published crate is queryable after a
  small delay" race. If the release workflow publishes
  `detritus-protocol` and immediately tries to publish
  `detritus-client` (which depends on it), the second
  publish may fail with "could not find detritus-protocol
  on crates.io". Add a `sleep 60` between publishes, or
  use `cargo publish --no-verify` (skip the pre-publish
  build check, which is what triggers the index query).

- **Tag-driven release publishing the wrong crate.** If a
  user accidentally tags `v0.1.0` on the wrong branch, the
  release workflow publishes from that branch. Document
  the release procedure clearly (Phase 07's RELEASING.md):
  always tag from `trunk`, always after the CI on `trunk`
  is green.

- **The `release.yml` workflow not gated on CI passing.**
  Forgejo Actions doesn't have a built-in `needs:` across
  workflows. The release workflow doesn't depend on the
  ci workflow's success on the same SHA. Mitigate by
  having the release workflow re-run a `test` job before
  the publish step — slower but safer.

## Reference

- forgejo-ci skill: `/home/can/.claude/skills/forgejo-ci/SKILL.md` (project conventions).
- codeberg-ci skill: `/home/can/.claude/skills/codeberg-ci/SKILL.md`.
- Forgejo Actions docs: <https://forgejo.org/docs/v1.21/user/actions/>.
- crates.io publishing reference: <https://doc.rust-lang.org/cargo/reference/publishing.html#cargo-publish>.
- actionlint: <https://github.com/rhysd/actionlint>.
- Sequencing: independent of Phases 02-05 by file scope. Should land alongside or after Phase 05 so the green CI signal reflects the final pre-publish state. Blocks Phase 07 (the dry-run is a CI job; Phase 07 verifies it ran green).
