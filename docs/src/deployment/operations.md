# Operations Overview

This page documents the server-side operational contract for `detritusd`.

## Token Configuration

Start the server with:

```sh
detritusd --tokens-config /etc/detritus/tokens.toml
```

The token file uses Argon2 PHC hashes. The literal bearer token is distributed out-of-band and is not stored in the config.

Generate a new hash by supplying one line on stdin:

```sh
detritusd hash-token < /run/secrets/receiver-token
```

The command prints a randomly salted Argon2id PHC string and needs no server
configuration. It strips the line ending while preserving spaces in the token,
rejects empty input, and does not print the secret. Hashes created with older
Argon2 versions continue to authenticate.

```toml
[[token]]
id = "regicide-prod"
secret = "$argon2id$v=19$m=19456,t=2,p=1$..."
project = "regicide"
source_prefix = "regicide/"

[[token]]
id = "rs-modde-prod"
secret = "$argon2id$v=19$m=19456,t=2,p=1$..."
project = "rs-modde"
source_prefix = "rs-modde/"

[rate_limit]
logs_per_minute = 1000
logs_burst = 200
crashes_per_minute = 30
crashes_burst = 5
```

Every request except `/healthz` and `/metrics` requires `Authorization: Bearer <token>`.

## Request correlation and shutdown

Every HTTP and gRPC response includes `x-request-id`. The server preserves a
supplied ID or generates a UUID, and attaches it to the request's tracing span.
Authorization headers are marked sensitive before request tracing.

Ctrl-C and Unix SIGTERM both initiate graceful shutdown, stop the retention
worker, and drain log writers before the process exits. `detritusd --version`
prints the installed crate version.

## Request Limits

Default limits are per `(token, source-id)`:

```text
logs:    1000 batches/minute, burst 200
crashes:   30 dumps/minute, burst 5
```

The server returns gRPC `ResourceExhausted` for logs and HTTP `429` for crashes when a bucket is empty.

## Retention

Defaults:

```text
logs TTL:    14 days
crashes TTL: 90 days
janitor:      1 hour interval
```

Relevant CLI flags:

```sh
detritusd \
  --logs-ttl-days 14 \
  --crashes-ttl-days 90 \
  --janitor-interval-secs 3600
```

The janitor removes expired log files, expired crash source indexes, and then any content-addressed blobs no longer referenced by an index.
If a surviving crash index is corrupt, the janitor logs an error with its path
and still deletes expired logs and indexes. It skips blob deletion for that cycle
because the corrupt index may still reference live blobs. Blob cleanup resumes
after the index is repaired, removed, or expires.

## Purging a source

Set `can_purge = true` on a dedicated `[[token]]` entry to allow deletion. It defaults
to `false`. The token's `project` and `source_prefix` still restrict which sources it
can purge.

Block `/v1/admin/*` at public ingress (including Caddy) and make the purge
endpoint reachable only through a private operator path.

Send `POST /v1/admin/purge-source` with `Authorization: Bearer <token>`,
`Content-Type: application/json`, and a body such as:

```json
{"project":"regicide","install_id":"11111111-1111-1111-1111-111111111111"}
```

The server closes the source's log writer and checks crash references before
deleting anything. If a crash index needed for that check cannot be read or
parsed, the request returns `500` without deleting the source's files; retry
after repairing or removing the corrupt index. A logs-only purge does not check
other sources' indexes. Only dumps and attachments referenced by the target
source but by no surviving index are deleted. A successful response is `200 OK`
with `logs_deleted`, `indexes_deleted`, `blobs_deleted`, and `bytes_freed`. An
unknown or already deleted source returns zero for every count. The periodic
janitor can still clean up orphans left by an interrupted purge.

A missing or invalid bearer returns `401`. A token without purge capability
returns `403` before the body is parsed. For a purge-enabled token, an invalid
body or path component returns `400`, an out-of-scope source `403`, and a storage
failure `500`. Ingest rate limits do not apply. Purging does not block future
uploads for that source; stop them upstream if the source must stay deleted.

Each call emits a structured `purge_source` event at `detritus::audit` with the token ID
(not the bearer), project, source ID, deletion counts, and `ok` or `error`. Unavailable
identifiers are left empty. `/metrics` exposes `detritus_purge_total{project,result}`
using the authenticated token's project, or `unknown` for authentication failures.

## Health And Metrics

`/healthz` is unauthenticated and returns `204 No Content`.

`/metrics` is also unauthenticated and is intended for loopback scraping. It emits OpenMetrics-style text with counters and histograms for request status, ingest volume, dedup hits, writer queue depth, janitor activity, and validation failures.

## Ingress Topologies

For the self-hosted reverse-proxy path, see [Caddy Reverse Proxy](./caddy-reverse-proxy.md).

For Cloudflare's hosted edge, body-size limits, and TLS tradeoffs, see [Cloudflare Ingress](./cloudflare-ingress.md).
