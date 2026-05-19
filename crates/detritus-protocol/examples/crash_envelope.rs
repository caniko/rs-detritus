//! Construct a crash envelope and serialize it as a Detritus multipart upload.
//!
//! Run with:
//!
//!     cargo run --example crash_envelope -p detritus-protocol

use chrono::Utc;
use detritus_protocol::{
    AttachmentManifest, BuildInfo, CrashAttachment, CrashEnvelope, CrashKind, CrashMetadata,
    SourceId,
};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = SourceId {
        project: "detritus-example".to_owned(),
        platform: "linux".to_owned(),
        version: "0.1.0".to_owned(),
        install_id: Uuid::nil(),
    };

    let mut metadata = CrashMetadata::new(
        source,
        Utc::now(),
        CrashKind::PanicTarball,
        BuildInfo {
            git_sha: "unknown".to_owned(),
            profile: "release".to_owned(),
            target_triple: "x86_64-unknown-linux-gnu".to_owned(),
        },
        json!({
            "tick": 42,
            "screen": "main-menu"
        }),
    );
    metadata.panic_text = Some("example panic payload".to_owned());
    metadata.attachments.push(AttachmentManifest {
        key: "breadcrumbs".to_owned(),
        filename: Some("breadcrumbs.json".to_owned()),
        content_type: "application/json".to_owned(),
        len: br#"{"recent_events":["launch","panic"]}"#.len() as u64,
    });

    let envelope = CrashEnvelope {
        metadata,
        dump: b"synthetic panic tarball bytes".to_vec(),
        attachments: vec![CrashAttachment {
            key: "breadcrumbs".to_owned(),
            content_type: "application/json".to_owned(),
            bytes: br#"{"recent_events":["launch","panic"]}"#.to_vec(),
        }],
    };

    let mut body = Vec::new();
    envelope.write_to(&mut body).await?;
    body.flush().await?;

    println!("serialized crash envelope: {} bytes", body.len());
    println!("source: {}", envelope.metadata.source.canonical());
    Ok(())
}
