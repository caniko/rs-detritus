#![cfg_attr(docsrs, feature(doc_cfg))]
//! Wire protocol types for Detritus telemetry and crash ingestion.
//!
//! `detritus-protocol` owns the schema shared by `detritus-client` and
//! `detritus-server`: source identity, crash envelopes, and the curated OTLP
//! log facade used by the client and receiver. Version `0.1.0` supports OTLP
//! logs only; traces and metrics are intentionally outside the published
//! compatibility surface.
//!
//! [`PROTOCOL_VERSION`] is carried in HTTP and gRPC metadata so clients and
//! servers can reject payloads encoded for an incompatible schema.
//!
//! ```
//! use detritus_protocol::PROTOCOL_VERSION;
//!
//! assert_eq!(PROTOCOL_VERSION, 1);
//! ```
//!
//! # Features
//!
//! - **`multipart`** *(enabled by default)* — RFC 7578 multipart encoding and
//!   parsing of crash envelopes ([`multipart`]). Building with
//!   `default-features = false` drops it along with the `bytes`,
//!   `futures-util`, `multer`, and `tokio` dependencies, leaving the schema
//!   types for a logs-only consumer.

/// Crash-report schema types and protocol errors.
pub mod crash;
#[cfg(feature = "multipart")]
#[cfg_attr(docsrs, doc(cfg(feature = "multipart")))]
/// Multipart encoding and parsing helpers for crash envelopes.
pub mod multipart;
/// Curated OpenTelemetry Protocol log-service bindings.
pub mod otlp;
/// Per-tenant JSON Schema descriptors for payload validation.
pub mod schema;
/// Source identity attached to Detritus payloads.
pub mod source;

/// Crash-report schema types re-exported for the common import path.
pub use crash::{
    AttachmentManifest, BuildInfo, CrashAttachment, CrashEnvelope, CrashKind, CrashMetadata,
    ProtocolError,
};
/// Source identity re-exported for the common import path.
pub use source::SourceId;

/// Current observability ingestion schema version.
pub const PROTOCOL_VERSION: u32 = 1;
/// gRPC metadata key carrying [`PROTOCOL_VERSION`].
pub const GRPC_VERSION_KEY: &str = "x-protocol-version";
/// HTTP header carrying [`PROTOCOL_VERSION`].
pub const HTTP_VERSION_HEADER: &str = "X-Protocol-Version";
