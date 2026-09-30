use super::*;
use crate::auth::{TestToken, TokenStore};
use detritus_protocol::otlp::{
    common::{ArrayValue, KeyValueList},
    logs::ScopeLogs,
    resource::Resource,
};

fn attr(key: &str, value: any_value::Value) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue { value: Some(value) }),
    }
}

fn resource_logs() -> ResourceLogs {
    ResourceLogs {
        resource: Some(Resource {
            attributes: [
                ("source.project", "tests"),
                ("source.platform", "linux"),
                ("source.version", "1"),
                ("source.install_id", "install"),
            ]
            .into_iter()
            .map(|(k, v)| attr(k, any_value::Value::StringValue(v.into())))
            .collect(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn otlp_values_preserve_json_types_nested_arrays_and_hex_bytes() {
    let values = [
        (
            any_value::Value::StringValue("hello".into()),
            json!("hello"),
        ),
        (any_value::Value::BoolValue(true), json!(true)),
        (any_value::Value::IntValue(-7), json!(-7)),
        (any_value::Value::DoubleValue(1.5), json!(1.5)),
        (any_value::Value::BytesValue(vec![0, 255]), json!("00ff")),
        (
            any_value::Value::ArrayValue(ArrayValue {
                values: vec![
                    AnyValue {
                        value: Some(any_value::Value::BoolValue(false)),
                    },
                    AnyValue::default(),
                ],
            }),
            json!([false, null]),
        ),
        (
            any_value::Value::KvlistValue(KeyValueList {
                values: vec![
                    attr("n", any_value::Value::IntValue(42)),
                    KeyValue {
                        key: "missing".into(),
                        value: None,
                    },
                ],
            }),
            json!({"n": 42}),
        ),
    ];
    for (value, expected) in values {
        assert_eq!(any_value_json(&AnyValue { value: Some(value) }), expected);
    }
    assert_eq!(any_value_json(&AnyValue::default()), Value::Null);
    let record = LogRecord {
        trace_id: vec![0, 255],
        span_id: vec![1],
        ..Default::default()
    };
    let json = log_record_json(&record);
    assert_eq!(json["trace_id"], "00ff");
    assert_eq!(json["span_id"], "01");
    assert!(json["body"].is_null());
    let resource = ResourceLogs {
        scope_logs: vec![ScopeLogs::default()],
        ..Default::default()
    };
    assert_eq!(
        resource_logs_to_validation_value(&resource),
        json!({"resource": {}, "scopes": [{"name": "", "attributes": {}}]})
    );
}

#[test]
fn source_identity_requires_string_attributes_and_safe_path_components() {
    assert_eq!(
        source_from_resource_logs(&ResourceLogs::default())
            .unwrap_err()
            .code(),
        tonic::Code::InvalidArgument
    );
    for index in 0..4 {
        let mut resource = resource_logs();
        resource.resource.as_mut().unwrap().attributes[index].value = Some(AnyValue {
            value: Some(any_value::Value::IntValue(42)),
        });
        assert_eq!(
            source_from_resource_logs(&resource).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }
    let mut resource = resource_logs();
    resource.resource.as_mut().unwrap().attributes[3] = attr(
        "source.install_id",
        any_value::Value::StringValue("../escape".into()),
    );
    assert_eq!(
        source_from_resource_logs(&resource).unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
}

#[test]
fn invalid_protocol_metadata_and_grpc_statuses_have_stable_error_codes() {
    let mut metadata = tonic::metadata::MetadataMap::new();
    assert_eq!(
        validate_protocol_metadata(&metadata).unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    for value in ["abc", "4294967296", "999"] {
        metadata.insert(GRPC_VERSION_KEY, value.parse().unwrap());
        assert_eq!(
            validate_protocol_metadata(&metadata).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
    }
    for (code, expected) in [
        (tonic::Code::Ok, "200"),
        (tonic::Code::Unauthenticated, "401"),
        (tonic::Code::PermissionDenied, "403"),
        (tonic::Code::ResourceExhausted, "429"),
        (tonic::Code::InvalidArgument, "400"),
        (tonic::Code::FailedPrecondition, "412"),
        (tonic::Code::Internal, "500"),
    ] {
        assert_eq!(grpc_status_label(code), expected);
    }
}

#[tokio::test]
async fn writer_shutdown_drains_all_records_and_reuses_source_channel() {
    let dir = tempfile::tempdir().unwrap();
    let storage = StoragePaths::new(dir.path().to_owned());
    let pool = LogWriterPool::new(storage.clone());
    let source = SourceKey::new("tests".into(), "install".into()).unwrap();
    let sender = pool.sender_for(source.clone()).await.unwrap();
    assert!(sender.same_channel(&pool.sender_for(source.clone()).await.unwrap()));
    for n in 0..50 {
        sender.send(json!({"sequence": n})).await.unwrap();
    }
    drop(sender);
    pool.shutdown().await;
    pool.shutdown().await;
    let text = tokio::fs::read_to_string(storage.log_file(&source, Utc::now().date_naive()))
        .await
        .unwrap();
    let lines = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 50);
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(line["sequence"], index);
    }
    flush_file(None).await.unwrap();
}

#[tokio::test]
async fn utf8_schema_errors_are_bounded_without_panicking_or_writing_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema.json");
    // Payload values are masked now. A long Unicode *schema property path*
    // still exercises UTF-8-safe truncation without relying on leaked values.
    let long_key = "☃".repeat(2000);
    tokio::fs::write(
        &path,
        serde_json::to_vec(&json!({"properties": {"resource": {"properties": {
            "source.version": {"enum": ["allowed"]},
            (long_key.clone()): {"enum": ["allowed"]}
        }}}}))
        .unwrap(),
    )
    .await
    .unwrap();
    let registry = SchemaRegistry::load(&[crate::schemas::ProjectSchemaEntry {
        project: "tests".into(),
        kind: SchemaKind::LogAttributes,
        path,
    }])
    .await
    .unwrap();
    let store = TokenStore::for_tests(vec![TestToken {
        id: "test".into(),
        secret_hash: include_str!("../../tests/fixtures/legacy-token.phc")
            .trim()
            .into(),
        project: "tests".into(),
        source_prefix: "tests/".into(),
    }]);
    let token = store.authenticate("secret-token").unwrap();
    let storage = StoragePaths::new(dir.path().join("data"));
    let pool = LogWriterPool::new(storage.clone());
    let handler = LogsHandler::new(
        pool.clone(),
        RateLimiter::new(crate::RateLimitConfig::default()),
        Metrics::new().unwrap(),
        registry,
    );
    let mut resource = resource_logs();
    resource.resource.as_mut().unwrap().attributes[2] = attr(
        "source.version",
        any_value::Value::StringValue("☃".repeat(2000)),
    );
    resource.resource.as_mut().unwrap().attributes.push(attr(
        &long_key,
        any_value::Value::StringValue("private-diagnostic-value".into()),
    ));
    let mut request = Request::new(ExportLogsServiceRequest {
        resource_logs: vec![resource],
    });
    request.metadata_mut().insert(
        GRPC_VERSION_KEY,
        PROTOCOL_VERSION.to_string().parse().unwrap(),
    );
    request.extensions_mut().insert(token);
    let error = handler.export(request).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    assert!(error.message().len() <= 1024);
    assert!(error.message().ends_with('…'));
    assert!(!error.message().contains("private-diagnostic-value"));
    assert!(
        !storage
            .log_file(
                &SourceKey::new("tests".into(), "install".into()).unwrap(),
                Utc::now().date_naive()
            )
            .exists()
    );
    pool.shutdown().await;
}
