# Offline Shipping

Detritus panic hooks write crash reports to a local spool before any network work
happens. A later process can flush those pending reports without reconstructing
the original SDK configuration by using the stored-config shipper.

Use this path for next-launch cleanup, recovery tools, or a small helper process
that only knows the spool directory and has access to a bearer token:

```rust,no_run
use detritus::ship_pending_crashes_using_stored_config;
use secrecy::SecretString;

# async fn run() -> Result<(), detritus::ShipError> {
let shipped = ship_pending_crashes_using_stored_config(
    "/var/lib/my-app/detritus",
    SecretString::from("receiver-token"),
)
.await?;
# let _ = shipped;
# Ok(())
# }
```

Each pending entry carries an `sdk-config.json` written by the panic hook. The
stored-config shipper reads that file for the upload endpoint and sent-entry
retention, so entries in the same spool can be posted to different receivers.
The bearer token is still supplied by the caller and is never stored in the
spool. Keeping the token outside the crash directory lets applications persist
recoverable routing information without leaving long-lived credentials on disk.

Entries missing `sdk-config.json`, or entries whose stored endpoint is not a
valid URL, fail with `ShipError::MissingStoredConfig`. That is intentional:
mixed spools produced by older clients should be handled explicitly rather than
silently skipped.

## Reusable clients and transport policy

For repeated scans, keep a `CrashShipper` to reuse HTTP connections:

```rust,no_run
use detritus::{CrashShipper, ShipConfig};
use secrecy::SecretString;

# async fn run() -> Result<(), detritus::ShipError> {
let shipper = CrashShipper::new(SecretString::from("receiver-token"))?
    .with_config(ShipConfig::default().with_dump_compression_level(3));
let shipped = shipper.ship_using_stored_config("/var/lib/my-app/detritus").await?;
# let _ = shipped;
# Ok(())
# }
```

The default connect timeout is 10 seconds, the read timeout is 30 seconds, and
the total request timeout is 60 seconds. Failed or timed-out uploads remain in
`pending/`. Default HTTP-level retries are disabled: the spool controls replay,
and a single upload attempt should not create multiple crash indexes.

For a private CA, mutual TLS, a proxy, or different timeouts, configure a
[Reqwest client](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html)
and pass it to `CrashShipper::with_client(client, token)`. Its retry and timeout
settings replace the defaults. Call `detritus::install_default_crypto_provider`
before constructing a custom HTTPS client.

The convenience shipping functions use the same default transport and share
one connection pool throughout each scan. Compression and the on-disk spool
format are identical for both APIs.
