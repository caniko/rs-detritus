# Detritus Server Operations

## Token Configuration

Start the server with:

```sh
detritusd --tokens-config /etc/detritus/tokens.toml
```

The token file uses Argon2 PHC hashes. The literal bearer token is distributed to clients out-of-band and is not stored in the config.

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

Every request except `/healthz` and `/metrics` requires `Authorization: Bearer <token>`. A token may write only sources whose canonical source key starts with `source_prefix`; this server uses `<project>/<install_id>` for that key.

To rotate a token:

1. Add a new `[[token]]` entry with a new `id` and Argon2 hash.
2. Reload or restart the server.
3. Update clients to use the new literal token.
4. Remove the old token entry after clients have moved.

On canix-managed NixOS hosts, edit the agenix source secret and redeploy:

```sh
canix agenix edit <host>-detritus-tokens
canix deploy <host>
```

The NixOS module expects the decrypted token file to be mode `0400` and owned by
`detritus:detritus`. The systemd unit refuses to start if the file is more
widely readable.

## NixOS Deployment Contract

The flake exports:

```text
packages.<system>.detritus
nixosModules.default
nixosConfigurations.detritus-test-vm
```

A host imports `inputs.detritus.nixosModules.default` and enables:

```nix
services.detritus = {
  enable = true;
  bind = "127.0.0.1:4317";
  tokensConfig = config.age.secrets.detritus-tokens.path;
  logsTtlDays = 14;
  crashesTtlDays = 90;
};
```

The public URL is provided by the host reverse proxy. Record the live value in
the canix host module when the host is chosen; the intended production shape is
`https://detritus.<domain>/`.

The service stores persistent data in `/var/lib/detritus` by default and runs as
`detritus:detritus`. The module guards `dataDir` with `knownDataDirs` because
changing it after deployment does not migrate existing blobs; it only points new
writes at another tree.

Expected layout:

```text
/var/lib/detritus/
  logs/<project>/<source-id>/YYYY-MM-DD.ndjson
  crashes/by-hash/<2-hex-prefix>/<sha256>.bin
  crashes/by-source/<project>/<source-id>/<timestamp>-<sha256>.json
  tmp/
```

Loopback health check after deployment:

```sh
curl http://127.0.0.1:4317/healthz
```

Public crash-ingest smoke:

```sh
curl -X POST \
  -H "Authorization: Bearer <test-token>" \
  https://detritus.<domain>/v1/crashes \
  -F 'metadata={"schema_version":1,"source":{"project":"regicide","platform":"linux","version":"smoke","install_id":"00000000-0000-4000-8000-000000000001"},"timestamp":"2026-05-19T00:00:00Z","kind":"PanicTarball","build":{"git_sha":"smoke","profile":"release","target_triple":"x86_64-unknown-linux-gnu"},"panic_text":"deployment smoke","context":{},"attachments":[]}' \
  -F dump=@/tmp/fake.bin
```

## Rate Limits

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

Flags:

```sh
--logs-ttl-days 14
--crashes-ttl-days 90
--janitor-interval-secs 3600
```

The janitor removes expired `logs/**/*.ndjson`, removes expired crash source indexes, then deletes content-addressed blobs that no remaining index references. Blob candidates are snapshotted before index scanning so a concurrent new upload is not deleted during the same cycle.

## Disk Footprint Example

For regicide at 100 concurrent peers, assuming each peer emits about 5 KiB/s of compressed structured logs:

```text
100 peers * 5 KiB/s * 86,400 s/day * 14 days = 604,800,000 KiB
604,800,000 KiB / 1,048,576 = ~577 GiB
```

Crash storage depends on dump size and deduplication. At 100 MiB maximum per dump, 100 unique dumps/day retained for 90 days is a worst-case `100 MiB * 100 * 90 = ~879 GiB`; identical dumps are stored once and referenced by multiple source index files.

## Metrics

`/metrics` is unauthenticated and intended for loopback scraping. It returns OpenMetrics-style text with:

```text
detritus_requests_total{endpoint,status}
detritus_request_duration_seconds{endpoint}
detritus_bytes_ingested_total{endpoint}
detritus_dedup_hits_total
detritus_writer_queue_depth{source_id}
detritus_janitor_cycles_total
detritus_janitor_blobs_freed_total
detritus_janitor_bytes_freed_total
detritus_janitor_cycle_duration_seconds
```

## Ingress topologies

See [Cloudflare proxy ingress evaluation](operations/cloudflare-ingress.md) for a detailed analysis of Cloudflare free-tier proxy, direct ingress, Tailscale/WireGuard, and Cloudflare Tunnel as alternatives. The evaluation helps you choose which topology fits your security and scale requirements.
