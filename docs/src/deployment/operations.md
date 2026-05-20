# Operations Overview

This page documents the server-side operational contract for `detritusd`.

## Token Configuration

Start the server with:

```sh
detritusd --tokens-config /etc/detritus/tokens.toml
```

The token file uses Argon2 PHC hashes. The literal bearer token is distributed out-of-band and is not stored in the config.

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

## Health And Metrics

`/healthz` is unauthenticated and returns `204 No Content`.

`/metrics` is also unauthenticated and is intended for loopback scraping. It emits OpenMetrics-style text with counters and histograms for request status, ingest volume, dedup hits, writer queue depth, janitor activity, and validation failures.

## Ingress Topologies

For the self-hosted reverse-proxy path, see [Caddy Reverse Proxy](./caddy-reverse-proxy.md).

For Cloudflare's hosted edge, body-size limits, and TLS tradeoffs, see [Cloudflare Ingress](./cloudflare-ingress.md).
