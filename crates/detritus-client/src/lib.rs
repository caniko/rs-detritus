//! Client SDK for Detritus ingestion.
//!
//! Public API is limited to [`Layer`], [`LayerBuilder`], [`install_panic_hook`],
//! [`ship_pending_crashes`], [`ship_pending_crashes_with_config`], [`ShipConfig`],
//! [`PanicHookConfig`], [`PanicKind`], and [`SourceId`].
//! Everything else is an implementation detail and may change between releases.
//!
//! ```no_run
//! use std::{path::PathBuf, time::Duration};
//! use detritus::{Layer, SourceId};
//! use secrecy::SecretString;
//! use url::Url;
//! use uuid::Uuid;
//!
//! let source = SourceId {
//!     project: "detritus".to_owned(),
//!     platform: "linux".to_owned(),
//!     version: "0.1.0".to_owned(),
//!     install_id: Uuid::nil(),
//! };
//! let layer = Layer::builder()
//!     .endpoint(Url::parse("http://127.0.0.1:4317").unwrap())
//!     .token(SecretString::from("secret-token"))
//!     .source(source)
//!     .queue_dir(PathBuf::from("observability-spool/logs"))
//!     .flush_interval(Duration::from_secs(5))
//!     .build()
//!     .unwrap();
//! # drop(layer);
//! ```

pub mod compression;
mod layer;
mod panic_hook;
mod shipper;
mod spool;

/// Build metadata type re-exported for panic-hook configuration.
pub use detritus_protocol::{BuildInfo, SourceId};
/// Tracing subscriber layer API.
pub use layer::{Layer, LayerBuilder, LayerError};
/// Panic-hook crash capture API.
pub use panic_hook::{PanicHookConfig, PanicHookError, PanicKind, install_panic_hook};
/// Offline crash spool shipping API.
pub use shipper::{ShipConfig, ShipError, ship_pending_crashes, ship_pending_crashes_with_config};
