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

Compression is gzip on both endpoints and is mandatory on the wire. Logs already benefit from OTLP/gRPC gzip support, and crash uploads often include repetitive text attachments or structured metadata, so requiring gzip gives predictable bandwidth behavior without adding negotiation complexity in v1.

## Multi-Tenancy

Multi-tenancy uses `project` as the top-level namespace and `source-id` as the client identity inside a project, for example `regicide/desktop-linux/v0.4.1/<install-id>`. This keeps authorization, storage layout, and later retention controls aligned around the same two identifiers instead of inventing separate tenant, app, and device models.
