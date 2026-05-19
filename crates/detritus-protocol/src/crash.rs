//! JSON crash-report schema and multipart envelope description.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{PROTOCOL_VERSION, source::SourceId};

/// Describes a file attached to a crash report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentManifest {
    /// Multipart attachment key, paired with a part named `attach:<key>`.
    pub key: String,
    /// Original file name when one is available.
    pub filename: Option<String>,
    /// MIME content type for the attachment bytes.
    pub content_type: String,
    /// Attachment size in bytes.
    pub len: u64,
}

/// Build metadata captured with a crash report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Git revision used to build the binary.
    pub git_sha: String,
    /// Cargo profile, such as `dev` or `release`.
    pub profile: String,
    /// Rust target triple.
    pub target_triple: String,
}

/// Crash artifact family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrashKind {
    /// Native minidump output.
    Minidump,
    /// Plain panic tarball, including rs-modde-style panic bundles.
    PanicTarball,
    /// Rust compiler internal compiler error artifact.
    RustcIce,
}

/// JSON metadata part for the crash-dump endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrashMetadata {
    /// Schema version. New values must match [`PROTOCOL_VERSION`].
    pub schema_version: u32,
    /// Project, platform, app version, and installation identity.
    pub source: SourceId,
    /// UTC timestamp for when the crash was captured.
    pub timestamp: DateTime<Utc>,
    /// Crash artifact family.
    pub kind: CrashKind,
    /// Build metadata for the crashing binary.
    pub build: BuildInfo,
    /// Panic text, when the crash originated from Rust panic handling.
    pub panic_text: Option<String>,
    /// Free-form context, such as tick, RNG seed, or recent events.
    pub context: serde_json::Value,
    /// Files expected as `attach:<key>` multipart parts.
    pub attachments: Vec<AttachmentManifest>,
}

impl CrashMetadata {
    /// Creates metadata with the current protocol schema version.
    #[must_use]
    pub fn new(
        source: SourceId,
        timestamp: DateTime<Utc>,
        kind: CrashKind,
        build: BuildInfo,
        context: serde_json::Value,
    ) -> Self {
        Self {
            schema_version: PROTOCOL_VERSION,
            source,
            timestamp,
            kind,
            build,
            panic_text: None,
            context,
            attachments: Vec::new(),
        }
    }
}

/// In-memory description of the crash multipart layout.
///
/// Wire parts:
/// - `metadata`: `Content-Type: application/json`, payload is [`CrashMetadata`].
/// - `dump`: `Content-Type: application/octet-stream`, payload is the crash dump.
/// - `attach:<key>`: optional files declared in [`CrashMetadata::attachments`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrashEnvelope {
    /// JSON metadata part.
    pub metadata: CrashMetadata,
    /// Raw dump bytes.
    pub dump: Vec<u8>,
    /// Additional attachment payloads.
    pub attachments: Vec<CrashAttachment>,
}

/// In-memory multipart attachment payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashAttachment {
    /// Attachment key matching a manifest entry.
    pub key: String,
    /// MIME content type.
    pub content_type: String,
    /// Raw attachment bytes.
    pub bytes: Vec<u8>,
}

/// Protocol serialization and parsing errors.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// JSON metadata failed to encode or decode.
    #[error("crash metadata JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Multipart body failed to parse.
    #[cfg(feature = "multipart")]
    #[error("multipart parse error: {0}")]
    Multipart(#[from] multer::Error),
    /// Async I/O failed.
    #[cfg(feature = "multipart")]
    #[error("multipart I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Required multipart part is absent.
    #[cfg(feature = "multipart")]
    #[error("missing multipart part `{0}`")]
    MissingPart(&'static str),
    /// Multipart part name was invalid UTF-8 or absent.
    #[cfg(feature = "multipart")]
    #[error("invalid multipart part name")]
    InvalidPartName,
    /// Multipart part had unexpected bytes.
    #[cfg(feature = "multipart")]
    #[error("invalid multipart payload: {0}")]
    InvalidMultipart(String),
}
