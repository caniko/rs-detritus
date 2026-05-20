use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use detritus_protocol::{
    CrashAttachment, CrashEnvelope, CrashMetadata,
    multipart::{EnvelopeEncodings, PartEncoding},
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    compression::{DEFAULT_COMPRESSION_LEVEL, compress, should_compress_content_type},
    install_default_crypto_provider,
    panic_hook::StoredUploadConfig,
    spool::SpoolLock,
};

/// Configuration knobs for crash-dump upload compression.
///
/// Created with [`ShipConfig::default`] and customised with the builder
/// methods.  The defaults compress dumps at zstd level 19 and also
/// compress text-ish attachments.
#[derive(Debug, Clone)]
pub struct ShipConfig {
    /// Zstd level used for the `dump` part. Valid range is `1..=22`.
    /// Defaults to [`DEFAULT_COMPRESSION_LEVEL`] (19).
    pub dump_compression_level: i32,
    /// When `true`, text-ish attachments are compressed with zstd before
    /// upload. Defaults to `true`.
    pub attachment_compression: bool,
}

impl Default for ShipConfig {
    fn default() -> Self {
        Self {
            dump_compression_level: DEFAULT_COMPRESSION_LEVEL,
            attachment_compression: true,
        }
    }
}

impl ShipConfig {
    /// Sets the zstd compression level for the `dump` part.
    #[must_use]
    pub fn with_dump_compression_level(mut self, level: i32) -> Self {
        self.dump_compression_level = level;
        self
    }

    /// Enables or disables zstd compression for text-ish attachment parts.
    #[must_use]
    pub fn with_attachment_compression(mut self, enabled: bool) -> Self {
        self.attachment_compression = enabled;
        self
    }
}

/// Errors returned while shipping pending crash reports.
#[derive(Debug, thiserror::Error)]
pub enum ShipError {
    /// Filesystem operation failed.
    #[error("crash spool I/O error: {0}")]
    Io(#[from] io::Error),
    /// JSON metadata failed to parse or encode.
    #[error("crash spool JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// HTTP client failed.
    #[error("crash upload HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    /// Server rejected the upload.
    #[error("crash upload failed with status {0}")]
    Status(reqwest::StatusCode),
    /// Multipart encoding failed.
    #[error("crash multipart encoding failed: {0}")]
    Protocol(#[from] detritus_protocol::ProtocolError),
}

/// Ships pending crash artifacts and moves successful entries to `sent/`.
///
/// A filesystem lock on `spool_dir/.lock` prevents two processes from scanning
/// the same pending directory concurrently. One spool directory per process is
/// still the recommended default.
///
/// Dump bytes and text-ish attachments are compressed with zstd
/// (level [`DEFAULT_COMPRESSION_LEVEL`]) **before** the SHA-256 is computed,
/// so content-addressed dedup is based on the compressed bytes.
pub async fn ship_pending_crashes(
    spool_dir: impl AsRef<Path>,
    endpoint: Url,
    token: SecretString,
) -> Result<usize, ShipError> {
    ship_pending_crashes_with_config(spool_dir, endpoint, token, ShipConfig::default()).await
}

/// Like [`ship_pending_crashes`] but with explicit compression settings.
pub async fn ship_pending_crashes_with_config(
    spool_dir: impl AsRef<Path>,
    endpoint: Url,
    token: SecretString,
    config: ShipConfig,
) -> Result<usize, ShipError> {
    let spool_dir = spool_dir.as_ref();
    let _lock = SpoolLock::acquire(spool_dir)?;
    let pending = spool_dir.join("pending");
    let sent = spool_dir.join("sent");
    fs::create_dir_all(&pending)?;
    fs::create_dir_all(&sent)?;
    cleanup_sent(&sent, 90)?;

    let mut shipped = 0;
    for entry in pending_entries(&pending)? {
        let envelope = read_envelope(&entry)?;
        post_envelope(&endpoint, &token, &envelope, &config).await?;
        let destination = sent.join(
            entry
                .file_name()
                .ok_or_else(|| io::Error::other("pending entry has no file name"))?,
        );
        if destination.exists() {
            fs::remove_dir_all(&destination)?;
        }
        fs::rename(&entry, destination)?;
        shipped += 1;
    }
    Ok(shipped)
}

fn pending_entries(pending: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = match fs::read_dir(pending) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    entries.sort();
    Ok(entries)
}

fn read_envelope(entry: &Path) -> Result<CrashEnvelope, ShipError> {
    let metadata: CrashMetadata = serde_json::from_slice(&fs::read(entry.join("metadata.json"))?)?;
    let dump = fs::read(entry.join("dump.bin"))?;
    let attachments = metadata
        .attachments
        .iter()
        .filter_map(|manifest| {
            let filename = manifest.filename.as_ref()?;
            let path = entry.join(filename);
            Some((manifest, path))
        })
        .map(|(manifest, path)| {
            Ok(CrashAttachment {
                key: manifest.key.clone(),
                content_type: manifest.content_type.clone(),
                bytes: fs::read(path)?,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(CrashEnvelope {
        metadata,
        dump,
        attachments,
    })
}

/// Applies zstd compression to the envelope's dump (and optionally its
/// attachments), then serialises to multipart and POSTs it.
///
/// Compressed bytes replace the originals in-place inside a locally-owned
/// clone so the original `CrashEnvelope` is not mutated.
async fn post_envelope(
    endpoint: &Url,
    token: &SecretString,
    envelope: &CrashEnvelope,
    config: &ShipConfig,
) -> Result<(), ShipError> {
    install_default_crypto_provider();

    // Build a locally-owned envelope with compressed bytes plus the matching
    // EnvelopeEncodings that records which parts are zstd-encoded.
    let mut compressed_envelope = envelope.clone();
    let mut encodings = EnvelopeEncodings {
        dump: PartEncoding::default(),
        attachments: Vec::with_capacity(envelope.attachments.len()),
    };

    // Compress the dump unconditionally.
    let compressed_dump = compress(&envelope.dump, config.dump_compression_level)?;
    compressed_envelope.dump = compressed_dump;
    encodings.dump = PartEncoding {
        content_encoding: Some("zstd".to_owned()),
    };

    // Optionally compress text-ish attachments.
    for attachment in &envelope.attachments {
        let should =
            config.attachment_compression && should_compress_content_type(&attachment.content_type);
        if should {
            let compressed = compress(&attachment.bytes, config.dump_compression_level)?;
            // Find the matching attachment in our local clone and replace bytes.
            if let Some(local) = compressed_envelope
                .attachments
                .iter_mut()
                .find(|a| a.key == attachment.key)
            {
                local.bytes = compressed;
            }
            encodings.attachments.push(PartEncoding {
                content_encoding: Some("zstd".to_owned()),
            });
        } else {
            encodings.attachments.push(PartEncoding::default());
        }
    }

    let mut body = Vec::new();
    compressed_envelope
        .write_to_with_boundary_and_encodings(
            &mut body,
            detritus_protocol::multipart::DEFAULT_BOUNDARY,
            &encodings,
        )
        .await?;

    let url = crash_url(endpoint);
    let response = reqwest::Client::builder()
        .build()?
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {}", token.expose_secret()))
        .header(
            CONTENT_TYPE,
            format!(
                "multipart/form-data; boundary={}",
                detritus_protocol::multipart::DEFAULT_BOUNDARY
            ),
        )
        .body(body)
        .send()
        .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(ShipError::Status(response.status()))
    }
}

fn crash_url(endpoint: &Url) -> Url {
    if endpoint.path().ends_with("/v1/crashes") {
        endpoint.clone()
    } else {
        endpoint
            .join("/v1/crashes")
            .unwrap_or_else(|_| endpoint.clone())
    }
}

fn cleanup_sent(sent: &Path, retention_days: u64) -> io::Result<()> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(retention_days * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in pending_entries(sent)? {
        let modified = entry.metadata()?.modified().unwrap_or(SystemTime::now());
        if modified < cutoff {
            fs::remove_dir_all(entry)?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn read_stored_upload_config(entry: &Path) -> Result<Option<StoredUploadConfig>, ShipError> {
    let path = entry.join("sdk-config.json");
    if path.exists() {
        Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
    } else {
        Ok(None)
    }
}
