# Storage Layout

`detritusd --data-dir <path>` stores all ingest artifacts under that root:

```text
<data-dir>/
  logs/<project>/<source-id>/YYYY-MM-DD.ndjson
  crashes/by-hash/<2-hex-prefix>/<sha256>.bin
  crashes/by-source/<project>/<source-id>/<timestamp>-<sha256>.json
  tmp/
```

`<source-id>` is the `SourceId.install_id` UUID. Project, platform, version, and install ID are still preserved inside crash metadata and the required OTLP resource attributes.

## Logs

Logs are append-only NDJSON.

Each accepted OTLP `LogRecord` produces one JSON line in the current UTC day file for its source.

## Crash Blobs

Crash dumps and attachments are content-addressed by SHA-256.

The server writes uploaded bytes into `<data-dir>/tmp/`, fsyncs the temporary file, links it into `crashes/by-hash/<2-hex-prefix>/<sha256>.bin`, and then removes the temporary file.

If the hash already exists, the upload is treated as a deduplicated blob and the temporary file is removed.

## Crash Indexes

Each accepted crash upload also writes a per-source JSON index under:

```text
crashes/by-source/<project>/<source-id>/
```

The index contains:

- the submitted `CrashMetadata`
- the dump blob pointer
- attachment blob pointers
- `content_encoding` for any part uploaded with an encoding such as `zstd`
