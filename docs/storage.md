# Detritus Storage Layout

`detritusd --data-dir <path>` stores all ingest artifacts under that root:

```text
<data-dir>/
  logs/<project>/<source-id>/YYYY-MM-DD.ndjson
  crashes/by-hash/<2-hex-prefix>/<sha256>.bin
  crashes/by-source/<project>/<source-id>/<timestamp>-<sha256>.json
  tmp/
```

`<source-id>` is the `SourceId.install_id` UUID. `project`, platform, version, and install ID are still preserved in each crash index's embedded metadata and in the required OTLP resource attributes.

Logs are append-only NDJSON. Each accepted OTLP `LogRecord` produces one JSON line in the current UTC day file for its source.

Crash dumps and attachments are content-addressed by SHA-256. The server writes upload bytes into `<data-dir>/tmp/`, fsyncs the temp file, links it into `crashes/by-hash/<2-hex-prefix>/<sha256>.bin`, then removes the temp file. If the hash already exists, the temp file is removed and the upload is treated as a deduplicated blob.

Each crash upload also writes a per-source JSON index entry under `crashes/by-source/<project>/<source-id>/`. The index contains the submitted metadata, the dump hash pointer, and any attachment hash pointers.
