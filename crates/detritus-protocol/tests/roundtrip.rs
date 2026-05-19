use detritus_protocol::{
    AttachmentManifest, BuildInfo, CrashAttachment, CrashEnvelope, CrashKind, CrashMetadata,
    PROTOCOL_VERSION, SourceId,
    otlp::{
        common::{AnyValue, KeyValue, any_value},
        logs::LogRecord,
    },
};
use chrono::Utc;
use prost::Message;
use serde_json::json;
use uuid::Uuid;

#[test]
fn roundtrip_log_record() {
    let record = LogRecord {
        time_unix_nano: 42,
        observed_time_unix_nano: 43,
        severity_number: 9,
        severity_text: "INFO".to_owned(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue("hello".to_owned())),
        }),
        attributes: vec![KeyValue {
            key: "message".to_owned(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue("one log".to_owned())),
            }),
        }],
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: vec![1; 16],
        span_id: vec![2; 8],
        event_name: String::new(),
    };

    let mut encoded = Vec::new();
    record.encode(&mut encoded).expect("record encodes");
    let decoded = LogRecord::decode(encoded.as_slice()).expect("record decodes");
    assert_eq!(record, decoded);
}

#[test]
fn roundtrip_crash_metadata_json() {
    for kind in [
        CrashKind::Minidump,
        CrashKind::PanicTarball,
        CrashKind::RustcIce,
    ] {
        let metadata = crash_metadata(kind);
        let encoded = serde_json::to_vec(&metadata).expect("metadata encodes");
        let decoded: CrashMetadata = serde_json::from_slice(&encoded).expect("metadata decodes");
        assert_eq!(metadata, decoded);
        assert_eq!(
            "detritus/linux/0.1.0/11111111-1111-1111-1111-111111111111",
            decoded.source.canonical()
        );
    }
}

#[tokio::test]
async fn roundtrip_multipart() {
    let mut metadata = crash_metadata(CrashKind::Minidump);
    metadata.attachments = vec![
        AttachmentManifest {
            key: "events".to_owned(),
            filename: Some("recent_events.json".to_owned()),
            content_type: "application/json".to_owned(),
            len: 11,
        },
        AttachmentManifest {
            key: "state".to_owned(),
            filename: Some("state_snapshot.txt".to_owned()),
            content_type: "text/plain".to_owned(),
            len: 5,
        },
    ];
    let envelope = CrashEnvelope {
        metadata,
        dump: b"minidump-bytes".to_vec(),
        attachments: vec![
            CrashAttachment {
                key: "events".to_owned(),
                content_type: "application/json".to_owned(),
                bytes: b"{\"turn\": 1}".to_vec(),
            },
            CrashAttachment {
                key: "state".to_owned(),
                content_type: "text/plain".to_owned(),
                bytes: b"board".to_vec(),
            },
        ],
    };

    let mut encoded = Vec::new();
    envelope
        .write_to(&mut encoded)
        .await
        .expect("multipart writes");
    let mut cursor = std::io::Cursor::new(encoded);
    let decoded = CrashEnvelope::read_from(&mut cursor)
        .await
        .expect("multipart reads");

    assert_eq!(envelope.metadata, decoded.metadata);
    assert_eq!(envelope.dump, decoded.dump);
    assert_eq!(envelope.attachments, decoded.attachments);
}

fn crash_metadata(kind: CrashKind) -> CrashMetadata {
    let source = SourceId {
        project: "detritus".to_owned(),
        platform: "linux".to_owned(),
        version: "0.1.0".to_owned(),
        install_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("valid uuid"),
    };
    CrashMetadata {
        schema_version: PROTOCOL_VERSION,
        source,
        timestamp: Utc::now(),
        kind,
        build: BuildInfo {
            git_sha: "abcdef0".to_owned(),
            profile: "release".to_owned(),
            target_triple: "x86_64-unknown-linux-gnu".to_owned(),
        },
        panic_text: Some("panic text".to_owned()),
        context: json!({ "tick": 10, "rng_seed": 123 }),
        attachments: Vec::new(),
    }
}
