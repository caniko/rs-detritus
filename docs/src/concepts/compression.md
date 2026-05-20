# Compression Model

Detritus uses two distinct compression layers for crash ingest.

## Layer 1: Transport Compression

Transport compression is handled by the HTTP and gRPC stack.

- OTLP/gRPC log export uses transport gzip.
- Crash uploads flow through the server's request decompression and response compression layers.

This layer is transparent to application code.

## Layer 2: Payload Compression

`detritus-client` compresses crash payload bytes before upload.

- Dump bytes are compressed with zstd by default.
- The default zstd level is `19`.
- Text-like attachments are also compressed by default.
- The metadata JSON part is never compressed.

The client sets `Content-Encoding: zstd` on the individual multipart parts that were compressed.

## Attachment Heuristic

The client compresses these content types:

- `text/*`
- `application/json`
- `application/x-ndjson`
- `application/yaml`
- `application/x-yaml`
- `application/xml`

The client leaves these content types uncompressed:

- `application/zstd`
- `application/gzip`
- `application/x-gzip`
- `application/zip`
- `application/x-tar`
- `application/octet-stream`
- `image/*`
- `video/*`
- `audio/*`

## Hashing And Deduplication

The crash blob hash covers the bytes sent over the wire, which means the SHA-256 is computed over the compressed bytes.

That preserves dedup semantics: the same source dump compressed the same way lands on the same content-addressed blob.

## Server Contract

The server does not decompress crash blobs during ingest.

It stores the uploaded bytes exactly as received and records part-level `content_encoding` in the crash index so later tooling can decide whether and how to decode the blob.
