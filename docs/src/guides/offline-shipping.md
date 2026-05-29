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
