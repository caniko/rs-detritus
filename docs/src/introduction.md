# Introduction

Detritus is a lightweight crash and log receiver for small Rust projects.

Version `0.1.0` has two ingest paths:

- OTLP/gRPC logs through `opentelemetry.proto.collector.logs.v1.LogsService.Export`
- HTTP/2 multipart crash uploads at `/v1/crashes`

The workspace is split into three crates:

- `detritus-protocol` provides the shared wire types, crash schema, and curated OTLP facade.
- `detritus-client` provides the tracing layer, panic hook, and offline crash shipper for Rust applications.
- `detritus-server` provides the `detritusd` receiver binary and an embeddable server API.

The documentation in this book focuses on the stable operational and architectural contract that is already implemented in the repository:

- the one-process, one-port receiver model
- filesystem-backed persistence
- per-project bearer-token auth
- payload-level schema validation
- zstd-compressed crash uploads with byte-exact server-side storage
- single-instance and multi-instance NixOS deployment

API reference for the published crates lives on docs.rs:

- <https://docs.rs/detritus-protocol>
- <https://docs.rs/detritus-client>
- <https://docs.rs/detritus-server>
