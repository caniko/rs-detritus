# Detritus vision

Detritus is a lightweight crash and log receiver for small Rust projects. It
ships three crates: `detritus-protocol` (wire types, crash schema, curated OTLP
log bindings), `detritus-client` (tracing layer, panic hook, offline crash
shipper) and `detritus-server` (`detritusd`, plus embeddable server entry
points). It is for projects that want crash reports and logs collected somewhere
they control, without standing up a full observability platform.

## Problem

Good crash and log collection normally means a vendor SDK or a full
OTLP/collector stack. A small project needs neither. Detritus accepts the two
things that matter in a form that already fits the ecosystem — OTLP/gRPC for
logs, HTTP/2 multipart for crash artifacts — and keeps the client side small
enough to embed in a game or a CLI.

## Principles

- **Standard wire formats over custom ones.** Logs use OTLP/gRPC by implementing
  `LogsService.Export`, so existing OpenTelemetry clients work. Only crashes get
  a bespoke endpoint (`POST /v1/crashes` with `metadata`, `dump` and optional
  `attach:<key>` parts), because crash artifacts have no standard carrier.
- **One process, one port.** The server is a single `axum` router with
  `tonic::routes()` mounted on it, so gRPC and HTTP share one listener. That is
  the deployment contract used by the quick start, the NixOS module and
  reverse-proxy configs.
- **Offline first.** The client spools crashes and log batches and retries them,
  with bounded attempts and a real exporter shutdown, so a network outage does
  not lose the report that explains it.
- **Secrets stay off disk and out of argv.** Bearer tokens are caller-supplied;
  `detritusd hash-token` reads from stdin and produces salted Argon2id hashes.
- **Limits are explicit.** Uploads, log batches and replays are bounded, and
  malformed or oversized uploads are rejected with tests for each case.
- **Coverage and policy are gates, not goals.** Each crate must independently
  hold at least 90% line coverage, and audit, deny, MSRV, warning-free docs and
  isolated-feature checks run before release.
- **Unsafe is forbidden.** The workspace sets `unsafe_code = "forbid"` and warns
  on `missing_docs`.

## Scope

In scope: the log and crash ingestion endpoints; request IDs, tracing spans and
curated OTLP resource entities; gzip/zstd negotiation; pooled gRPC connections
with custom CA and mutual-TLS trust roots; the tracing layer, panic hook,
`CrashShipper` and stored-config shipping APIs; native Linux crash capture;
retention and deduplication; the `detritusd` binary; a NixOS module; and the
mdBook covering architecture, storage, deployment and operations.

## Non-goals

- Not a metrics or traces backend. Logs and crashes only.
- Not a general-purpose tracing layer for large organisations; it is sized for
  small Rust projects that run their own receiver.

## Direction

Version 0.2.0 (2026-09-30) landed compression, pooled connections, richer TLS
configuration, reusable crash shipping, shared offline JSON Schema references,
masked diagnostics, graceful shutdown, and 90% per-crate coverage gates with
published reports. The unreleased section only adds zstd-compressed log export
examples to the 0.2 quick starts. Recent maintenance kept Rust 1.88 and existing
wire/spool formats stable while upgrading Tonic/Prost, Argon2, jsonschema and
related dependencies, and moved publication to a signed-tag, dependency-ordered
GitHub Actions publisher (`protocol → server → client`).

## Place in the fleet

Canix runs Detritus on `thething`: the host imports its NixOS module, binds
`detritusd` to loopback on the OTLP port, and generates the bearer-token TOML
from an agenix secret with Argon2id hashes. Chessbender consumes it for
diagnostics, and it is also a Simit updater target on atlas.
