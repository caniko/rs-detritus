# Schema Validation

Detritus supports optional per-project JSON Schema validation for crash metadata and OTLP log attributes.

## Schema Kinds

The implemented schema kinds are:

- `crash_metadata`
- `log_attributes`

Each schema registration is keyed by `(project, kind)`.

## Token File Extension

Schema registrations live in the same TOML file as bearer tokens:

```toml
[[schema]]
project = "acme"
kind = "crash_metadata"
path = "schemas/crash.schema.json"

[[schema]]
project = "acme"
kind = "log_attributes"
path = "schemas/log.schema.json"
```

Schema paths are resolved relative to the token file's parent directory, not relative to the current working directory.

## Validation Behavior

At startup, the server loads and compiles every declared schema into a process-wide registry.

Request handling then applies the registry like this:

- crash uploads validate the `metadata` JSON against the project's `crash_metadata` schema
- OTLP log exports validate each `ResourceLogs` payload against the project's `log_attributes` schema

If no schema is registered for a given `(project, kind)` pair, the request is accepted.

## Failure Modes

Validation failures are endpoint-specific:

- crash validation failures are rejected on the HTTP path
- log validation failures are rejected on the OTLP/gRPC path

The server also increments validation-failure metrics so operators can distinguish malformed client payloads from transport or auth failures.

## Deployment Note

The NixOS module exposes `schemaDir`. When set, it is mounted read-only into the service and is intended to hold the JSON Schema files referenced by `tokensConfig`.
