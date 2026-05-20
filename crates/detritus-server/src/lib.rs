//! Receiver-side Detritus server library.
//!
//! The `detritus-server` package ships the `detritusd` binary for normal
//! operation. This library surface is for embedding the receiver in another
//! process, usually tests, local tooling, or custom orchestration.
//!
//! ```
//! use detritus_server::{RateLimitConfig, RetentionConfig, SchemaRegistry, ServerConfig};
//! ```

mod auth;
mod crashes;
mod janitor;
mod logs;
mod metrics;
mod rate_limit;
mod schemas;
mod server;
mod storage;

/// Authentication and token configuration for embedded servers.
pub use auth::{AuthConfigError, SecurityConfig, TestToken, TokenStore, load_security_config};
/// Retention configuration used by the background janitor.
pub use janitor::RetentionConfig;
/// Rate-limit configuration for logs and crash uploads.
pub use rate_limit::RateLimitConfig;
/// Per-tenant JSON Schema registry (no-op validator in Phase 01).
pub use schemas::SchemaRegistry;
/// Server configuration and serving entry points.
pub use server::{ServerConfig, serve, serve_with_shutdown};

#[doc(hidden)]
/// Hidden test hook for exercising retention without exposing janitor internals.
pub use janitor::{JanitorStats, run_janitor_cycle};
#[doc(hidden)]
/// Hidden test hook for storage-layout smoke tests.
pub use storage::StoragePaths;
