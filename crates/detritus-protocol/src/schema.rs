//! Per-tenant JSON Schema descriptors for crash and log payload validation.
//!
//! This module defines the public types consumed by `detritus-server` when
//! loading and eventually applying per-project validation rules.  The actual
//! JSON Schema compilation and validation lives in the server; the protocol
//! crate only owns the wire-level taxonomy (`SchemaKind`) and the on-disk
//! descriptor (`SchemaSpec`).
//!
//! # Error taxonomy
//!
//! [`SchemaError`](crate::schema::SchemaError) is the unified error type for
//! all schema operations: file I/O, JSON parsing, validation failures, and
//! look-ups for projects that were never registered.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The two payload shapes that may carry a per-tenant JSON Schema.
///
/// The `snake_case` wire names (`"crash_metadata"`, `"log_attributes"`) are
/// used both in tokens TOML configuration and in any future API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaKind {
    /// Validates the `metadata` field of a [`crate::CrashMetadata`] payload.
    CrashMetadata,
    /// Validates the resource / scope attributes of an OTLP log record.
    LogAttributes,
}

/// One schema-file declaration as it appears in `tokens.toml`.
///
/// `path` is always resolved relative to the tokens config file's parent
/// directory, never relative to the current working directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSpec {
    /// The payload kind this schema governs.
    pub kind: SchemaKind,
    /// Path to the JSON Schema document on disk.
    pub path: PathBuf,
}

/// Errors produced during schema loading or validation.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// A schema file could not be read from disk.
    #[error("schema I/O error reading `{path}`: {source}")]
    Io {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A schema file was read but is not valid JSON.
    #[error("schema parse error in `{path}`: {source}")]
    Parse {
        /// Path of the offending file.
        path: PathBuf,
        /// JSON parse error.
        #[source]
        source: serde_json::Error,
    },

    /// A payload failed validation against the registered schema.
    ///
    /// This variant is produced by `validate`; it is never produced by `load`.
    #[error("validation failed for {kind:?}: {errors:?}")]
    Validation {
        /// The kind of schema that rejected the payload.
        kind: SchemaKind,
        /// Human-readable description of each validation failure.
        errors: Vec<String>,
    },

    /// A handler requested validation for a `(project, kind)` pair that was
    /// never registered in the [`crate::schema`] registry.
    #[error("no schema registered for project `{project}` / kind `{kind:?}`")]
    UnknownSchema {
        /// Project identifier that was not found.
        project: String,
        /// The schema kind that was requested.
        kind: SchemaKind,
    },
}
