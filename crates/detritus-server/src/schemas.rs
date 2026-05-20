//! Per-tenant schema registry for crash and log payload validation.
//!
//! [`SchemaRegistry`] holds a compiled (or, in this phase, a no-op marker)
//! schema for every `(project, SchemaKind)` pair that was declared in the
//! tokens configuration file.  It is populated once at startup via
//! [`SchemaRegistry::load`] and then held inside [`crate::server::AppState`]
//! for the lifetime of the process.
//!
//! # No-op contract (Phase 01)
//!
//! [`SchemaRegistry::validate`] currently returns `Ok(())` for every
//! *registered* `(project, kind)` pair.  For unregistered pairs it returns
//! [`SchemaError::UnknownSchema`] so that Phase 02 inherits the correct
//! call-site error-handling shape without any further refactoring.
//!
//! > **NOTE:** Do not change the no-op return to real validation without an
//! > architecture revision.  Phase 02 is where the JSON Schema compiler is
//! > introduced; see `docs/planning/multi-tenant-validation-and-compression/`.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use detritus_protocol::schema::{SchemaError, SchemaKind};
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

/// Opaque placeholder for a compiled schema.
///
/// In Phase 01 this is an empty sentinel behind an `Arc` so that the
/// `HashMap` entries are cheap to clone and Phase 02 can swap the inner
/// type without touching call-sites.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase 02 will read the inner value when running real validation.
struct CompiledSchema(Arc<()>);

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

    /// Loads schema files from disk and builds a registry.
    ///
    /// Each file is read and parsed as JSON to catch obvious on-disk
    /// corruption at startup.  The JSON Schema compiler is *not* invoked in
    /// this phase; that step is deferred to Phase 02.
    ///
    /// `entries` must already have their `path` fields resolved to absolute
    /// paths (i.e. relative to the tokens config's parent directory, not to
    /// the current working directory).
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::Io`] if a file cannot be read, or
    /// [`SchemaError::Parse`] if the file content is not valid JSON.
    pub async fn load(entries: &[ProjectSchemaEntry]) -> Result<Self, SchemaError> {
        let mut schemas = HashMap::with_capacity(entries.len());
        for entry in entries {
            let raw = fs::read_to_string(&entry.path)
                .await
                .map_err(|source| SchemaError::Io {
                    path: entry.path.clone(),
                    source,
                })?;
            // Parse to confirm valid JSON; compilation happens in Phase 02.
            let _: serde_json::Value =
                serde_json::from_str(&raw).map_err(|source| SchemaError::Parse {
                    path: entry.path.clone(),
                    source,
                })?;
            schemas.insert(
                (entry.project.clone(), entry.kind),
                CompiledSchema(Arc::new(())),
            );
        }
        Ok(Self { schemas })
    }

    /// Validates `payload` against the schema registered for `(project, kind)`.
    ///
    /// # No-op guarantee (Phase 01)
    ///
    /// For every *registered* `(project, kind)` pair this method currently
    /// returns `Ok(())` unconditionally, preserving byte-identical server
    /// behaviour relative to the pre-registry baseline.
    ///
    /// For *unregistered* pairs it returns [`SchemaError::UnknownSchema`] so
    /// that Phase 02 inherits the correct error-handling shape at call-sites.
    ///
    /// > **NOTE:** Phase 02 wires the real validator in here.  Do not
    /// > silently change this to real validation without an architecture
    /// > revision and a corresponding update to this doc-comment.
    pub fn validate(
        &self,
        project: &str,
        kind: SchemaKind,
        _payload: &serde_json::Value,
    ) -> Result<(), SchemaError> {
        if self.schemas.contains_key(&(project.to_owned(), kind)) {
            // NOTE: Phase 02 wires the real validator in here.
            Ok(())
        } else {
            Err(SchemaError::UnknownSchema {
                project: project.to_owned(),
                kind,
            })
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
    use crate::schemas::SchemaError;

    // ------------------------------------------------------------------
    // empty_registry_validates_anything
    // ------------------------------------------------------------------

    /// An empty registry has no registered pairs, so every validate call
    /// returns `UnknownSchema`.  But the caller (Phase 02 handlers) will only
    /// call validate when a schema *is* registered; the empty registry is the
    /// "no schemas configured" fast path that must not block any project.
    ///
    /// What we actually test here is the no-op contract: calling validate on
    /// an empty registry for any project/kind returns `UnknownSchema` (not a
    /// panic or unexpected error), which is the correct sentinel for
    /// "no schema configured — Phase 02 handlers skip validation".
    #[test]
    fn empty_registry_validates_anything() {
        let registry = SchemaRegistry::empty();
        let payload = json!({"key": "value"});
        // An empty registry has no registered entries, so validate returns
        // UnknownSchema for any project/kind — this is the no-op path that
        // Phase 02 handlers check before calling validate.
        let result = registry.validate("acme", SchemaKind::CrashMetadata, &payload);
        assert!(
            matches!(result, Err(SchemaError::UnknownSchema { .. })),
            "empty registry should return UnknownSchema, got: {result:?}",
        );
        // Confirm that an *empty* registry's `schemas` map is empty
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
    // unknown_project_validation_returns_unknown_schema
    // ------------------------------------------------------------------

    /// Calling validate for a project that was never registered must return
    /// `SchemaError::UnknownSchema`, even though validate is currently a
    /// no-op for *registered* tenants.  This ensures Phase 02 inherits the
    /// correct error-handling shape.
    #[tokio::test]
    async fn unknown_project_validation_returns_unknown_schema() {
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
        let err = registry
            .validate("nope", SchemaKind::CrashMetadata, &payload)
            .expect_err("unknown project must return an error");
        assert!(
            matches!(
                err,
                SchemaError::UnknownSchema {
                    ref project,
                    kind: SchemaKind::CrashMetadata,
                } if project == "nope"
            ),
            "unexpected error variant: {err:?}",
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
