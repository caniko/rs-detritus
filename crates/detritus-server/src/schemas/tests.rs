use super::*;

#[tokio::test]
async fn schema_loading_reports_file_json_and_compilation_errors_with_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema.json");
    let entry = ProjectSchemaEntry {
        project: "tests".into(),
        kind: SchemaKind::CrashMetadata,
        path: path.clone(),
    };
    assert!(
        matches!(SchemaRegistry::load(std::slice::from_ref(&entry)).await, Err(SchemaError::Io { path: failed, .. }) if failed == path)
    );
    for text in ["{not-json", "{\"type\":42}"] {
        fs::write(&path, text).await.unwrap();
        assert!(
            matches!(SchemaRegistry::load(std::slice::from_ref(&entry)).await, Err(SchemaError::Parse { path: failed, .. }) if failed == path)
        );
    }
    fs::write(&path, "{\"type\":\"object\"}").await.unwrap();
    let registry = SchemaRegistry::load(&[entry]).await.unwrap();
    assert!(format!("{registry:?}").contains("CompiledSchema"));
    let cloned = registry.clone();
    assert!(
        registry
            .validate("tests", SchemaKind::CrashMetadata, &serde_json::json!({}))
            .is_ok()
    );
    assert!(
        cloned
            .validate("tests", SchemaKind::CrashMetadata, &serde_json::json!({}))
            .is_ok()
    );
    assert!(matches!(
        cloned.validate("tests", SchemaKind::CrashMetadata, &serde_json::json!([])),
        Err(SchemaError::Validation { .. })
    ));
    assert!(
        cloned
            .validate("other", SchemaKind::CrashMetadata, &serde_json::json!([]))
            .is_ok()
    );
}
