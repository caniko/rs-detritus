# Architecture

This document captures the implemented v1 architecture decisions.

## Naming

The project name is `detritus`.

Workspace crates are:

- `detritus-protocol`
- `detritus-client`
- `detritus-server`

The server binary is `detritusd`. The client-facing Rust API is centered on `detritus::Layer` and `detritus::install_panic_hook`.

## Logs

Logs use OTLP/gRPC. The server implements `opentelemetry.proto.collector.logs.v1.LogsService.Export`.

This keeps the log path aligned with standard OpenTelemetry clients while avoiding a custom log wire protocol.

## Crash Uploads

Crash dumps use an HTTP/2 multipart `POST` to `/v1/crashes`.

Two multipart parts are required:

- `metadata`, a JSON `CrashMetadata` document
- `dump`, the binary crash artifact

Optional attachments use multipart names of the form `attach:<key>`.

## One Process, One Port

Detritus runs as one process on one port. The receiver uses `axum` and `tonic::routes()` so gRPC and HTTP requests share one `axum::Router`.

That single listener is the deployment contract used by the local quick start, the NixOS module, and reverse-proxy configurations.

## Persistence

Persistence is filesystem-only in v1.

- Logs are appended as NDJSON files per `(project, source-id, day)`.
- Crash dumps and attachments are stored in a content-addressed SHA-256 blob store.
- Per-source crash indexes are stored as JSON alongside the source namespace.

The server does not require a database.

## Auth

Auth uses bearer tokens in the `Authorization` header.

Each token is scoped to:

- one `project`
- one canonical `source_prefix`

Server-side token files store Argon2 PHC hashes instead of raw bearer tokens.

## Compression

Crash uploads use a two-layer compression model.

Wire-level gzip is handled by the HTTP/gRPC stack. Payload-level zstd is handled by `detritus-client` before upload. The server stores the uploaded bytes exactly as received and records per-part `content_encoding` in the on-disk crash index when a part was encoded.

See [Compression Model](./compression.md) for the concrete rules.

## Multi-Tenancy

The top-level namespace is `project`. The client identity inside a project is `install_id`.

That shape is reused across:

- token authorization
- storage layout
- schema validation
- multi-instance deployment planning
