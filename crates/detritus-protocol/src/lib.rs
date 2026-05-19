//! Wire protocol types for Detritus ingestion.
//!
//! This crate owns the telemetry and crash-report schemas shared by the data
//! server and future client SDKs.

pub mod crash;
#[cfg(feature = "multipart")]
pub mod multipart;
pub mod otlp;
pub mod source;

pub use crash::{
    AttachmentManifest, BuildInfo, CrashAttachment, CrashEnvelope, CrashKind, CrashMetadata,
    ProtocolError,
};
pub use source::SourceId;

/// Current observability ingestion schema version.
pub const PROTOCOL_VERSION: u32 = 1;
/// gRPC metadata key carrying [`PROTOCOL_VERSION`].
pub const GRPC_VERSION_KEY: &str = "x-protocol-version";
/// HTTP header carrying [`PROTOCOL_VERSION`].
pub const HTTP_VERSION_HEADER: &str = "X-Protocol-Version";
