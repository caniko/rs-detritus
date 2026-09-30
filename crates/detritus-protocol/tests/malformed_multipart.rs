//! Protocol failures and fixed protobuf wire-format compatibility.
#![cfg(feature = "multipart")]

use detritus_protocol::{
    BuildInfo, CrashEnvelope, CrashKind, CrashMetadata, ProtocolError, SourceId,
};

fn metadata() -> CrashMetadata {
    CrashMetadata::new(
        SourceId {
            project: "tests".into(),
            platform: "linux".into(),
            version: "1".into(),
            install_id: uuid::Uuid::nil(),
        },
        chrono::Utc::now(),
        CrashKind::Minidump,
        BuildInfo {
            git_sha: "abc".into(),
            profile: "test".into(),
            target_triple: "test".into(),
        },
        serde_json::json!({}),
    )
}

fn body(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (name, value) in parts {
        bytes.extend_from_slice(
            format!("--test\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        bytes.extend_from_slice(value);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"--test--\r\n");
    bytes
}

#[tokio::test]
async fn malformed_envelopes_report_missing_parts_json_and_version_errors() {
    let metadata = serde_json::to_vec(&metadata()).unwrap();
    for (parts, missing) in [
        (vec![], "metadata"),
        (vec![("metadata", metadata.as_slice())], "dump"),
    ] {
        assert!(
            matches!(CrashEnvelope::read_from_with_boundary(&mut body(&parts).as_slice(), "test").await, Err(ProtocolError::MissingPart(name)) if name == missing)
        );
    }
    assert!(matches!(
        CrashEnvelope::read_from_with_boundary(
            &mut body(&[("metadata", b"not-json")]).as_slice(),
            "test"
        )
        .await,
        Err(ProtocolError::Json(_))
    ));
    assert!(matches!(
        CrashEnvelope::read_from_with_boundary(
            &mut body(&[("unknown", b"data")]).as_slice(),
            "test"
        )
        .await,
        Err(ProtocolError::InvalidMultipart(_))
    ));
    let mut outdated = self::metadata();
    outdated.schema_version = 999;
    let bytes = serde_json::to_vec(&outdated).unwrap();
    assert!(matches!(
        CrashEnvelope::read_from_with_boundary(
            &mut body(&[("metadata", &bytes), ("dump", b"dump")]).as_slice(),
            "test"
        )
        .await,
        Err(ProtocolError::InvalidMultipart(_))
    ));
    assert!(matches!(
        CrashEnvelope::read_from_with_boundary(&mut b"invalid".as_slice(), "test").await,
        Err(ProtocolError::Multipart(_))
    ));
}

#[tokio::test]
async fn absent_part_names_are_rejected_and_write_errors_propagate() {
    let bytes = b"--test\r\nContent-Disposition: form-data\r\n\r\ndata\r\n--test--\r\n";
    assert!(matches!(
        CrashEnvelope::read_from_with_boundary(&mut bytes.as_slice(), "test").await,
        Err(ProtocolError::InvalidPartName)
    ));
    let envelope = CrashEnvelope {
        metadata: metadata(),
        dump: vec![1, 2],
        attachments: vec![],
    };
    let (mut writer, reader) = tokio::io::duplex(1);
    drop(reader);
    assert!(matches!(
        envelope.write_to(&mut writer).await,
        Err(ProtocolError::Io(_))
    ));
}
