# Releasing

This page consolidates the stable release and publishing procedure for the workspace.

## Published Crates

The workspace publishes three crates:

- `detritus-protocol`
- `detritus-server`
- `detritus-client`

All three stay version-synchronized during `0.x`.

## Release Order

The release workflow publishes in topological dependency order:

1. `detritus-protocol`
2. `detritus-server`
3. `detritus-client`

After publishing `detritus-protocol` and `detritus-server`, the release workflow waits for the new version to become visible on the crates.io sparse index before continuing.

## CI Contract

On every push to `trunk` and every pull request, CI runs:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-features --all-targets -- -D warnings`
- `cargo check --workspace --all-features`
- `cargo test --workspace --all-features`
- `cargo doc --workspace --no-deps --all-features`
- an MSRV check on Rust `1.88`
- `cargo audit --deny warnings`
- `cargo deny --all-features check`

CI dry-runs only `detritus-protocol`:

```sh
cargo publish --dry-run -p detritus-protocol --allow-dirty
```

The dependent crates cannot be dry-run published on arbitrary push and PR commits because their versioned sibling dependencies are resolved against the live crates.io index.

## docs.rs Contract

Each published crate carries:

```toml
[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
```

That keeps docs.rs builds aligned with the feature-complete public API.

## Manual Release Procedure

1. Bump every published crate version together.
2. Bump the `version = "X.Y.Z"` qualifiers on internal path dependencies.
3. Move the `CHANGELOG.md` unreleased entries under a dated release heading.
4. Commit the release bump.
5. Push the commit to `trunk` and wait for CI to go green.
6. Create and push the `vX.Y.Z` tag.
7. Wait for the release workflow to publish the crates in order.
8. Verify the published versions on crates.io and then verify the docs.rs builds.

## First Release Checklist

Before the first publish of a crate name:

- verify the crate name is free on crates.io
- publish with the tag-driven workflow
- add any intended co-owners after the first successful publish

## Supply-Chain Checks

The repository keeps `deny.toml` at the root and enforces both `cargo-audit` and `cargo-deny` in CI. Treat advisory or licensing failures as release blockers.
