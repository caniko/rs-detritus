//! Per-tenant schema registry for crash and log payload validation.
//!
//! [`SchemaRegistry`] holds a compiled (or, in this phase, a no-op marker)
//! schema for every `(project, SchemaKind)` pair that was declared in the
//! tokens configuration file.  It is populated once at startup via
//! [`SchemaRegistry::load`] and then held inside [`crate::server::AppState`]
//! for the lifetime of the process.
//!
//! # Validation contract
//!
//! [`SchemaRegistry::validate`] returns `Ok(())` for any `(project, kind)`
//! pair that has no registered schema (accept-by-default — tenants without
//! a schema are not gated). For registered pairs it runs the compiled
//! `jsonschema` validator and returns [`SchemaError::Validation`] with all
//! collected errors on failure.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

pub(crate) use detritus_protocol::schema::SchemaError;
pub use detritus_protocol::schema::SchemaKind;
use jsonschema::Validator;
use tokio::fs;

/// One `[[schema]]` entry as parsed from `tokens.toml`.
///
/// `path` is always resolved relative to the tokens config file's parent
/// directory by the caller before being passed to [`SchemaRegistry::load`].
#[derive(Debug, Clone)]
pub struct ProjectSchemaEntry {
    /// Project identifier; must match an existing `[[token]].project` value.
    pub project: String,
    /// The payload kind this schema governs.
    pub kind: SchemaKind,
    /// Absolute (already-resolved) path to the JSON Schema document on disk.
    pub path: PathBuf,
}

/// A compiled JSON Schema validator behind an `Arc` so registry clones
/// stay cheap.
#[derive(Clone)]
struct CompiledSchema(Arc<Validator>);

impl std::fmt::Debug for CompiledSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledSchema").finish_non_exhaustive()
    }
}

/// Registry of per-tenant JSON Schema validators, keyed by `(project, kind)`.
///
/// Obtain an instance with [`SchemaRegistry::empty`] (for tests / configs
/// without schema entries) or [`SchemaRegistry::load`] (production path).
#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    schemas: HashMap<(String, SchemaKind), CompiledSchema>,
}

impl SchemaRegistry {
    /// Returns an empty registry that accepts any payload for any project.
    ///
    /// Used by tests and by tokens configs that omit `[[schema]]` tables.
    pub fn empty() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Loads schema files from disk and compiles them with `jsonschema`.
    ///
    /// `entries` must already have their `path` fields resolved to absolute
    /// paths (i.e. relative to the tokens config's parent directory, not to
    /// the current working directory).
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::Io`] if a file cannot be read, or
    /// [`SchemaError::Parse`] if the file content is not valid JSON or fails
    /// to compile as a JSON Schema.
    pub async fn load(entries: &[ProjectSchemaEntry]) -> Result<Self, SchemaError> {
        let mut schemas = HashMap::with_capacity(entries.len());
        for entry in entries {
            let raw = fs::read_to_string(&entry.path)
                .await
                .map_err(|source| SchemaError::Io {
                    path: entry.path.clone(),
                    source,
                })?;
            let value: serde_json::Value =
                serde_json::from_str(&raw).map_err(|source| SchemaError::Parse {
                    path: entry.path.clone(),
                    source,
                })?;
            let validator = Validator::new(&value).map_err(|err| SchemaError::Parse {
                path: entry.path.clone(),
                source: serde::de::Error::custom(err.to_string()),
            })?;
            schemas.insert(
                (entry.project.clone(), entry.kind),
                CompiledSchema(Arc::new(validator)),
            );
        }
        Ok(Self { schemas })
    }

    /// Validates `payload` against the schema registered for `(project, kind)`.
    ///
    /// Returns `Ok(())` when no schema is registered for the pair
    /// (accept-by-default — tenants without a schema are not gated). Returns
    /// [`SchemaError::Validation`] when a registered schema rejects the
    /// payload, with all collected errors.
    pub fn validate(
        &self,
        project: &str,
        kind: SchemaKind,
        payload: &serde_json::Value,
    ) -> Result<(), SchemaError> {
        let Some(compiled) = self.schemas.get(&(project.to_owned(), kind)) else {
            return Ok(());
        };
        let errors: Vec<String> = compiled
            .0
            .iter_errors(payload)
            .map(|e| e.to_string())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SchemaError::Validation { kind, errors })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use detritus_protocol::schema::SchemaKind;
    use serde_json::json;
    use tempfile::TempDir;

    use super::{ProjectSchemaEntry, SchemaRegistry};

    // ------------------------------------------------------------------
    // empty_registry_validates_anything
    // ------------------------------------------------------------------

    /// An empty registry accepts any payload for any project/kind. This is
    /// the accept-by-default contract: tenants without a registered schema
    /// are not gated.
    #[test]
    fn empty_registry_validates_anything() {
        let registry = SchemaRegistry::empty();
        let payload = json!({"key": "value"});
        assert!(
            registry
                .validate("acme", SchemaKind::CrashMetadata, &payload)
                .is_ok(),
            "empty registry should accept any payload",
        );
        assert!(registry.schemas.is_empty());
    }

    // ------------------------------------------------------------------
    // load_two_schemas_resolves_relative_paths
    // ------------------------------------------------------------------

    /// The loader reads files from disk and registers them under their keys.
    /// This test also exercises the path-resolution logic: we pass absolute
    /// paths (as `load_security_config` does after joining against the tokens
    /// config parent), confirming the loader does not re-join against CWD.
    #[tokio::test]
    async fn load_two_schemas_resolves_relative_paths() {
        // Locate the fixture directory next to this file's crate root.
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schemas");
        let crash_path = fixtures.join("crash.schema.json");
        let log_path = fixtures.join("log.schema.json");

        let entries = vec![
            ProjectSchemaEntry {
                project: "acme".to_owned(),
                kind: SchemaKind::CrashMetadata,
                path: crash_path,
            },
            ProjectSchemaEntry {
                project: "acme".to_owned(),
                kind: SchemaKind::LogAttributes,
                path: log_path,
            },
        ];

        let registry = SchemaRegistry::load(&entries)
            .await
            .expect("load should succeed");

        assert_eq!(registry.schemas.len(), 2, "both schemas should be loaded");
        // Validate returns Ok for registered entries (no-op contract).
        let payload = json!({});
        assert!(
            registry
                .validate("acme", SchemaKind::CrashMetadata, &payload)
                .is_ok(),
            "registered schema should validate (no-op Ok)",
        );
        assert!(
            registry
                .validate("acme", SchemaKind::LogAttributes, &payload)
                .is_ok(),
            "registered schema should validate (no-op Ok)",
        );
    }

    // ------------------------------------------------------------------
    // unknown_project_accepts_by_default
    // ------------------------------------------------------------------

    /// Calling validate for a project that was never registered returns
    /// `Ok(())` — accept-by-default. Only projects with a registered schema
    /// are subject to validation.
    #[tokio::test]
    async fn unknown_project_accepts_by_default() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schemas");
        let entries = vec![ProjectSchemaEntry {
            project: "known-project".to_owned(),
            kind: SchemaKind::CrashMetadata,
            path: fixtures.join("crash.schema.json"),
        }];
        let registry = SchemaRegistry::load(&entries)
            .await
            .expect("load should succeed");

        let payload = json!({"x": 1});
        assert!(
            registry
                .validate("nope", SchemaKind::CrashMetadata, &payload)
                .is_ok(),
            "unknown project should be accepted by default",
        );
    }

    // ------------------------------------------------------------------
    // tokens_config_without_schemas_loads_empty_registry
    // ------------------------------------------------------------------

    /// Tokens config files that have no `[[schema]]` table must still parse
    /// successfully and produce an empty registry.  This is the backward-
    /// compat guarantee that prevents existing deployments from breaking.
    #[tokio::test]
    async fn tokens_config_without_schemas_loads_empty_registry() {
        use std::io::Write as _;
        // Write a minimal tokens.toml with no [[schema]] table.
        let dir = TempDir::new().expect("tempdir");
        let tokens_path = dir.path().join("tokens.toml");
        {
            let mut f = std::fs::File::create(&tokens_path).expect("create tokens.toml");
            writeln!(
                f,
                r#"
[[token]]
id = "t1"
secret = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$bm90YXJlYWxoYXNoYnV0cGFzc2VzZm9ybWF0Y2hlY2s"
project = "proj"
source_prefix = "src/"
"#
            )
            .expect("write");
        }
        let config = crate::auth::load_security_config(&tokens_path)
            .await
            .expect("load_security_config should succeed");
        assert!(
            config.schema_registry.schemas.is_empty(),
            "no [[schema]] entries → empty registry",
        );
    }

    // ------------------------------------------------------------------
    // tokens_config_schema_project_mismatch_errors
    // ------------------------------------------------------------------

    /// A `[[schema]]` entry whose `project` does not match any `[[token]]`
    /// must cause `load_security_config` to fail with
    /// `AuthConfigError::SchemaProjectMismatch`.
    #[tokio::test]
    async fn tokens_config_schema_project_mismatch_errors() {
        use std::io::Write as _;
        let dir = TempDir::new().expect("tempdir");
        let schema_path = dir.path().join("crash.schema.json");
        std::fs::write(&schema_path, r#"{"type":"object"}"#).expect("write schema");

        let tokens_path = dir.path().join("tokens.toml");
        {
            let mut f = std::fs::File::create(&tokens_path).expect("create tokens.toml");
            writeln!(
                f,
                r#"
[[token]]
id = "t1"
secret = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$bm90YXJlYWxoYXNoYnV0cGFzc2VzZm9ybWF0Y2hlY2s"
project = "real-project"
source_prefix = "src/"

[[schema]]
project = "ghost-project"
kind = "crash_metadata"
path = "crash.schema.json"
"#
            )
            .expect("write");
        }
        let err = crate::auth::load_security_config(&tokens_path)
            .await
            .expect_err("mismatched project should fail");
        assert!(
            matches!(
                err,
                crate::auth::AuthConfigError::SchemaProjectMismatch { ref project, .. }
                if project == "ghost-project"
            ),
            "unexpected error: {err:?}",
        );
    }
}
