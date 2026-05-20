# Quick Start

This quick start brings up a local receiver and shows the minimum configuration the server expects.

## 1. Prepare a token file

`detritusd` requires a TOML file containing Argon2 PHC hashes, not raw bearer tokens. The file shape is:

```toml
[[token]]
id = "local-dev"
secret = "$argon2id$v=19$m=19456,t=2,p=1$..."
project = "detritus"
source_prefix = "detritus/"

[rate_limit]
logs_per_minute = 1000
logs_burst = 200
crashes_per_minute = 30
crashes_burst = 5
```

Every ingest request except `/healthz` and `/metrics` must present the matching bearer token with `Authorization: Bearer <token>`.

## 2. Start the receiver

From the repository root:

```sh
cargo run -p detritus-server -- \
  --bind 127.0.0.1:4317 \
  --data-dir ./detritus-data \
  --tokens-config ./tokens.toml
```

The default listener is `127.0.0.1:4317`. That single listener serves both OTLP/gRPC logs and multipart crash uploads.

## 3. Verify the local listener

```sh
curl -i http://127.0.0.1:4317/healthz
```

The handler returns `204 No Content` on success.

## 4. Exercise crash ingest

```sh
curl -X POST \
  -H "Authorization: Bearer <token>" \
  http://127.0.0.1:4317/v1/crashes \
  -F 'metadata={"schema_version":1,"source":{"project":"detritus","platform":"linux","version":"smoke","install_id":"00000000-0000-4000-8000-000000000001"},"timestamp":"2026-05-19T00:00:00Z","kind":"PanicTarball","build":{"git_sha":"smoke","profile":"release","target_triple":"x86_64-unknown-linux-gnu"},"panic_text":"smoke","context":{},"attachments":[]}' \
  -F dump=@/tmp/fake.bin
```

On success the server responds with `201 Created` and a JSON body containing the crash blob hash and whether the upload deduplicated against an existing blob.

## 5. Point a Rust application at the receiver

The client SDK installs as a tracing layer plus an optional panic hook. See the crate examples for runnable code:

- `cargo run --example install_layer -p detritus-client`
- `cargo run --example crash_envelope -p detritus-protocol`
- `cargo run --example embed_server -p detritus-server`
