# detritus Architecture

This document freezes the Phase 01 architecture for detritus. Later phases may fill in schemas, server handlers, client SDKs, retention policies, and deployment packaging, but they should not change these v1 decisions without an explicit architecture revision.

## Naming

The project name is `detritus`.

Crate names follow the pattern `detritus-protocol`, `detritus-server`, and `detritus-client`. The server binary is `detritusd`. The client public API is exposed as `detritus::Layer` and `detritus::install_panic_hook`.

## Logs

Logs use OTLP/gRPC, and the server implements `opentelemetry.proto.collector.logs.v1.LogsService.Export`. This is the industry-standard log ingestion protocol for OpenTelemetry clients, supports batching and gzip on the wire, and leaves an escape hatch to forward into systems such as Tempo, Loki, or SigNoz later without re-instrumenting clients.

## Crash Dumps

Crash dumps are uploaded with an HTTP/2 multipart POST to `/v1/crashes`. The upload has two required parts: `metadata`, a JSON document containing `build_id`, `version`, `source`, `timestamp`, `panic_text`, and an attachments manifest; and `dump`, a binary artifact containing either a minidump for sources such as regicide or a tarball of `panic_text`, backtrace, and environment data for non-minidump sources such as rs-modde. This mirrors Sentry, Crashpad, and Watson-style crash collection while keeping the protocol language-neutral and independent of regicide's private rkyv format.

## One Process, One Port

detritus runs as one process on one port. The server uses axum and tonic through `tonic::routes()` integration so gRPC and HTTP requests are served from the same `axum::Router`. Keeping both ingestion paths in one listener makes local deployment and future NixOS service wiring simple.

## Persistence

Persistence is filesystem-only in v1. Logs are appended as NDJSON files per `(source, day)`, and crash artifacts are stored in a content-addressed SHA-256 blob store with a per-source index file. This avoids introducing a database before the service has real operational pressure, which preserves the lightweight requirement and still leaves room for a future object store or indexer behind a compatibility layer.

## Auth

Auth uses bearer tokens in the `Authorization` header. Tokens are scoped to `(project, source-id-prefix)` and stored in a TOML config file on the server side, with no SSO and no OAuth. This is enough to separate projects and constrain uploads from individual client identities while keeping installation and recovery simple for solo-operated services.

## Compression

Detritus uses a two-layer compression model for crash uploads.

**Wire-level gzip (Layer 1)** — gzip is mandatory on both endpoints and is
transparent to application logic.  Logs benefit from OTLP/gRPC's built-in gzip
support.  Crash multipart uploads are sent over HTTP/2 with the
`tower-http` gzip layer active on the server.

**Payload-level zstd (Layer 2)** — the client SDK compresses the `dump`
multipart part and optionally text-ish attachment parts with zstd before
computing the SHA-256 and uploading.  The hash therefore covers the
**compressed** bytes, preserving content-addressed dedup semantics: two uploads
of the same source dump will compress to identical bytes (given the same
compression level) and land on the same hash.  The server stores the bytes
exactly as received — it never decompresses.

Each compressed part signals its encoding via a per-part
`Content-Encoding: zstd` header, which `multer` exposes through
`Field::headers()`.  The server records this value in the on-disk index as
`BlobPointer.content_encoding` / `AttachmentPointer.content_encoding`
(serialised as `"content_encoding": "zstd"` in the index JSON; absent when the
part was uploaded without encoding).

The client applies zstd level 19 by default.  Text-ish content types
(`text/*`, `application/json`, `application/x-ndjson`, `application/yaml`,
`application/xml`) are compressed; already-compressed types (`application/zstd`,
`application/gzip`, `image/*`, `video/*`, `audio/*`, `application/octet-stream`,
`application/zip`) are passed through unmodified.  The metadata JSON part is
never compressed (bounded at 64 KB; the overhead is not worth per-part encoding
negotiation).  The OTLP/gRPC logs path is unaffected — transport gzip already
handles it.

## Multi-Tenancy

Multi-tenancy uses `project` as the top-level namespace and `source-id` as the client identity inside a project, for example `regicide/desktop-linux/v0.4.1/<install-id>`. This keeps authorization, storage layout, and later retention controls aligned around the same two identifiers instead of inventing separate tenant, app, and device models.
