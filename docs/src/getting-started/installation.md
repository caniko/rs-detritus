# Installation

Detritus is a Rust workspace with a Nix flake. You can build it either with Cargo directly or through the flake outputs.

## Prerequisites

- Rust `1.85` or newer for direct Cargo builds
- `protoc` available on `PATH` for direct Cargo builds of the protocol crate
- Nix with flakes enabled if you want the reproducible flake environment

## Build With Nix

From the repository root:

```sh
nix develop
nix build .#detritus
nix build .#docs
```

`nix develop` gives you the workspace toolchain plus `mdbook`.

## Build With Cargo

From the repository root:

```sh
cargo build --workspace --all-features
cargo test --workspace --all-features
```

To run the receiver binary from source:

```sh
cargo run -p detritus-server -- --help
```

## Crate Installation

Operators can install the published receiver binary with Cargo:

```sh
cargo install detritus-server
```

Library users depend on the published crates in the usual Cargo way:

```toml
[dependencies]
detritus-client = "0.1.0"
detritus-protocol = "0.1.0"
```
