//! Source identity for observability payloads.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies the game build and installation that produced a payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceId {
    /// Project or product identifier.
    pub project: String,
    /// Runtime platform, such as `linux`, `windows`, or `android`.
    pub platform: String,
    /// User-facing application version.
    pub version: String,
    /// Stable installation identifier.
    pub install_id: Uuid,
}

impl SourceId {
    /// Returns `<project>/<platform>/<version>/<install_id>`.
    ///
    /// This canonical form is used in storage paths and rate-limit keys.
    #[must_use]
    pub fn canonical(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.project, self.platform, self.version, self.install_id
        )
    }
}
