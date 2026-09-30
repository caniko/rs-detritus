# Compression Model

Detritus uses two distinct compression layers for crash ingest.

## Layer 1: Transport Compression

Transport compression is handled by the HTTP and gRPC stack.

- OTLP/gRPC accepts uncompressed, gzip, and zstd messages. The client sends
  uncompressed requests by default; select an algorithm with
  `LayerBuilder::compression`. Response compression is negotiated with Tonic.
- Crash uploads accept whole-request gzip and zstd compression, independently of
  multipart part encodings. HTTP responses negotiate gzip or zstd using
  `Accept-Encoding`.
- The 150 MiB HTTP request-body limit is applied after decompression.

For example, add `.compression(detritus::CompressionEncoding::Zstd)` to a log
layer builder when using an upgraded receiver. A failed compressed export is
spooled as the original protobuf request, so replay remains independent of the
transport encoding selected by the next process.

The log exporter reuses a Tonic channel across requests and applies the flush
timeout to both connection establishment and each RPC, including the gRPC
deadline sent to the receiver. HTTPS uses system trust roots; a
`detritus::ClientTlsConfig` supplied to `LayerBuilder::tls_config` supports private
CAs, domain overrides, and client identities for mutual TLS. TLS termination
remains at the reverse proxy in the standard receiver deployment.

These integrations use the documented
[Tonic compression API](https://docs.rs/tonic/0.14.6/tonic/codec/enum.CompressionEncoding.html),
[Tonic TLS configuration](https://docs.rs/tonic/0.14.6/tonic/transport/struct.ClientTlsConfig.html),
and [tower-http decompression](https://docs.rs/tower-http/0.7.1/tower_http/decompression/index.html).

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
