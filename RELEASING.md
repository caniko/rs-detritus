# Release procedure

The workspace publishes `detritus-protocol`, `detritus-client`, and
`detritus-server` to crates.io. Versions are synchronized across all three
during v0.x. GitHub Actions owns publication through
[Publish Workspace](.github/workflows/publish-workspace.yaml).

## Prerequisites

- The crates.io account behind `CRATES_IO_API_TOKEN` owns all three crates.
  Check ownership with `cargo owner --list <crate>`.
- `CRATES_IO_API_TOKEN` is configured in the GitHub repository's Actions
  secrets. Supply the existing credential through stdin or the secret manager;
  never commit it or put it in a command-line argument.
- The signing key matches `keys/maintainers.gpg`. Run
  `simit release trust check` before creating a tag.
- Use Simit 0.19.0 or newer with the versioned-dev-dependency ordering fix for
  the aggregate CI and coordinated publisher configured in `simit.toml`.
  `simit release plan --workspace` must order protocol → server → client because
  the client's packaged dev-dependency requires the new server version.
  Check generated workflows with
  `simit init ci --platform github --check --diff`.
- Start from a clean `trunk` checkout. CI and Coverage must pass on the exact
  release commit before the tag is pushed.

## Prepare and validate

1. Bump all three `crates/*/Cargo.toml` package versions and every versioned
   sibling path dependency, including dev-dependencies. Refresh `Cargo.lock`.
   Nix package and documentation versions are derived from the server manifest.
2. Promote the changelog's unreleased entries into a dated release section and
   add a fresh `Unreleased` section. Include migration notes for breaking public
   dependency or API changes.
3. Regenerate the workflows after changing versions or CI policy:

   ```sh
   simit init ci --platform github
   simit init ci --platform github --check --diff
   simit release trust check
   ```

4. Run the release gates in the pinned Nix environment:

   ```sh
   nix develop -c treefmt --ci
   nix flake check --no-update-lock-file --keep-going --print-build-logs
   nix develop -c cargo clippy --workspace --all-targets --all-features --locked -- --deny warnings
   nix develop -c cargo test --workspace --all-features --locked -j 4
   nix develop .#docs -c cargo doc --workspace --no-deps --all-features --locked
   nix develop .#msrv -c cargo check --workspace --all-targets --all-features --locked
   nix develop -c cargo audit
   nix develop -c cargo deny check
   nix develop -c bash scripts/coverage.sh
   ```

   The coverage gate requires at least 90% handwritten production-line coverage
   independently for each crate. Also run the isolated no-default-feature tests
   documented in [README.md](README.md).

5. Inspect each `cargo package -p <crate> --list` result for the README, license,
   source, examples, tests, and protocol build inputs. Use the pinned Cargo's
   workspace packaging to verify unpublished sibling archives through its
   temporary registry:

   ```sh
   nix develop -c cargo package --workspace --all-features --allow-dirty --locked
   nix develop -c cargo publish --workspace --all-features --dry-run --allow-dirty --locked
   ```

   Individual registry dry-runs for a dependent require its new sibling
   versions to be available. The publisher repeats those checks immediately
   before uploading each crate in dependency order.

6. Commit and push the preparation to `trunk`, then wait for GitHub CI and
   Coverage to pass on that commit:

   ```sh
   git push origin trunk
   gh run list --repo caniko/rs-detritus
   gh run watch <run-id> --repo caniko/rs-detritus --exit-status
   ```

## Tag and publish

Use a signed, annotated **bare semver** tag. The workflow accepts `0.2.0`,
not `v0.2.0`.

```sh
git tag -s X.Y.Z -m "Release X.Y.Z" -m "Validated workspace tests, coverage, MSRV, docs, dependency policy, and package contents."
git verify-tag X.Y.Z
git push origin X.Y.Z
```

The coordinated publisher validates the tag against the pinned maintainer key
and checks lockstep Cargo versions. Required gates run before uploads, each
crate is packaged and dry-run checked, and dependent jobs wait for their
prerequisites to appear on crates.io. Publishing runs are serialized and are
never cancelled halfway through an upload.

Watch [GitHub Actions](https://github.com/caniko/rs-detritus/actions) until every
publish job succeeds. Verify each exact version on crates.io and then docs.rs:

```sh
for crate in detritus-protocol detritus-server detritus-client; do
  curl -fsS "https://crates.io/api/v1/crates/$crate/X.Y.Z" \
    | jq -r '.version | [.crate, .num, .yanked] | @tsv'
done
```

Documentation builds are asynchronous. Check
`https://docs.rs/<crate>/X.Y.Z` and its build status before reporting the
documentation release as complete.

## Recovery

- Fix a workflow or transient failure, then rerun the failed jobs or dispatch
  `publish-workspace.yaml` at the same signed tag. Already-published versions
  are accepted only when their archive checksums match the intended release.
- Never move an existing published tag. If shipped code is wrong, prepare a new
  version; yank a broken version only after considering consumers.
- If the token is revoked or expired, rotate the GitHub Actions secret from its
  authoritative credential source and rerun the failed publication.
