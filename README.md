# detritus

detritus is a lightweight crash and log receiver for small Rust projects.

The v1 receiver exposes two ingestion endpoints:

- `/v1/logs` via OTLP/gRPC by implementing `opentelemetry.proto.collector.logs.v1.LogsService.Export`.
- `/v1/crashes` via HTTP/2 multipart upload with JSON metadata and a binary crash artifact.

Documentation:

- [Architecture](docs/architecture.md)
- [Operations](docs/operations.md)
- [Storage layout](docs/storage.md)

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
