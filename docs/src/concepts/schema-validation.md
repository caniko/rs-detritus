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

## Shared references and format policy

Use local resource documents to share definitions across tenant schemas:

```toml
[schema_validation]
validate_formats = true
ignore_unknown_formats = false

[[schema_validation.resources]]
uri = "urn:detritus:common"
path = "schemas/common.json"
```

A registered schema can reference a definition with
`{"$ref": "urn:detritus:common#/$defs/build"}`. Resource paths are resolved
relative to the token file, just like tenant schema paths. All resources are
prepared at startup; no network or implicit filesystem retrieval is enabled.
Resource URIs must be unique.

If `validate_formats` is omitted, format validation follows the schema draft's
default. Setting it to `true` enforces formats such as `email`, `uuid`, and
`date-time`, including Draft 2020-12 schemas where formats are normally
annotations. `ignore_unknown_formats = false` makes unknown format names a
startup error when format validation is enabled. Both options preserve the
library defaults when omitted.

Embedded callers can use `SchemaRegistry::load_with_options`, `SchemaOptions`,
and `SchemaResourceEntry` for the same behavior. The implementation uses
[jsonschema's prepared reference registry](https://docs.rs/jsonschema/0.58.3/jsonschema/struct.Registry.html)
and [validation options](https://docs.rs/jsonschema/0.58.3/jsonschema/struct.ValidationOptions.html).

## Failure Modes

Validation failures are endpoint-specific:

- crash validation failures are rejected on the HTTP path
- log validation failures are rejected on the OTLP/gRPC path

The server also increments validation-failure metrics so operators can distinguish malformed client payloads from transport or auth failures.

Diagnostics include the instance JSON Pointer and the schema keyword path.
The validator's `masked` error display hides rejected payload values, allowing
clients to locate a validation failure without echoing their data in the error.

## Deployment Note

The NixOS module exposes `schemaDir`. When set, it is mounted read-only into the service and is intended to hold the JSON Schema files referenced by `tokensConfig`.
