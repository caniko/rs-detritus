use super::*;

#[tokio::test]
async fn shared_resources_formats_and_masked_locations_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let common = dir.path().join("common.json");
    fs::write(
        &common,
        serde_json::json!({
            "$id": "urn:detritus:common",
            "$defs": {"email": {"type": "string", "format": "email"}}
        })
        .to_string(),
    )
    .await
    .unwrap();
    let path = dir.path().join("root.json");
    fs::write(
        &path,
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"contact": {"$ref": "urn:detritus:common#/$defs/email"}}
        })
        .to_string(),
    )
    .await
    .unwrap();
    let entries = [ProjectSchemaEntry {
        project: "tests".into(),
        kind: SchemaKind::CrashMetadata,
        path,
    }];
    let mut options = SchemaOptions {
        resources: vec![SchemaResourceEntry {
            uri: "urn:detritus:common".into(),
            path: common,
        }],
        ..Default::default()
    };
    let payload = serde_json::json!({"contact": "private-invalid-address"});
    let registry = SchemaRegistry::load_with_options(&entries, &options)
        .await
        .unwrap();
    assert!(
        registry
            .validate("tests", SchemaKind::CrashMetadata, &payload)
            .is_ok()
    );
    options.validate_formats = Some(true);
    let registry = SchemaRegistry::load_with_options(&entries, &options)
        .await
        .unwrap();
    assert!(
        registry
            .validate(
                "tests",
                SchemaKind::CrashMetadata,
                &serde_json::json!({"contact": "user@example.org"})
            )
            .is_ok()
    );
    let SchemaError::Validation { errors, .. } = registry
        .validate("tests", SchemaKind::CrashMetadata, &payload)
        .unwrap_err()
    else {
        panic!("validation error")
    };
    assert!(errors[0].contains("/contact"));
    assert!(errors[0].contains("schema"));
    assert!(!errors[0].contains("private-invalid-address"));
}

#[tokio::test]
async fn shared_resource_configuration_resolves_paths_and_reports_bad_resources() {
    let dir = tempfile::tempdir().unwrap();
    let resource = dir.path().join("common.json");
    let root = dir.path().join("root.json");
    fs::write(&resource, r#"{"type":"string","format":"email"}"#)
        .await
        .unwrap();
    fs::write(&root, r#"{"$ref":"urn:common"}"#).await.unwrap();
    let config = format!(
        r#"
[[token]]
id = "test"
secret = "{}"
project = "tests"
source_prefix = "tests/"
[[schema]]
project = "tests"
kind = "crash_metadata"
path = "root.json"
[schema_validation]
validate_formats = true
ignore_unknown_formats = false
[[schema_validation.resources]]
uri = "urn:common"
path = "common.json"
"#,
        include_str!("../../tests/fixtures/legacy-token.phc").trim()
    );
    let config_path = dir.path().join("tokens.toml");
    fs::write(&config_path, config).await.unwrap();
    let loaded = crate::auth::load_security_config(&config_path)
        .await
        .unwrap();
    assert!(
        loaded
            .schema_registry
            .validate(
                "tests",
                SchemaKind::CrashMetadata,
                &serde_json::json!("invalid")
            )
            .is_err()
    );
    let mut options = SchemaOptions {
        resources: vec![SchemaResourceEntry {
            uri: "urn:common".into(),
            path: resource.clone(),
        }],
        ..Default::default()
    };
    options.resources.push(options.resources[0].clone());
    assert!(matches!(
        SchemaRegistry::load_with_options(&[], &options).await,
        Err(SchemaError::Parse { .. })
    ));
    options.resources.pop();
    fs::write(&resource, "not-json").await.unwrap();
    assert!(matches!(
        SchemaRegistry::load_with_options(&[], &options).await,
        Err(SchemaError::Parse { .. })
    ));
    fs::remove_file(&resource).await.unwrap();
    assert!(matches!(
        SchemaRegistry::load_with_options(&[], &options).await,
        Err(SchemaError::Io { .. })
    ));
}

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
