//! Zstd compression helpers for crash dump and attachment bytes.
//!
//! The hash that identifies a blob in the server's content-addressed store
//! covers the **compressed** bytes, so dedup semantics are preserved: two
//! uploads of the same source data compressed with the same settings will
//! produce the same SHA-256.

/// Default zstd compression level used for dump and attachment bytes.
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 19;

/// Compresses `bytes` with zstd at the given level.
///
/// Returns the compressed bytes on success. The caller is responsible for
/// recording that the result is zstd-encoded (e.g. via a `Content-Encoding:
/// zstd` multipart header).
///
/// # Errors
///
/// Returns an [`std::io::Error`] if zstd encoding fails.
pub fn compress(bytes: &[u8], level: i32) -> std::io::Result<Vec<u8>> {
    zstd::encode_all(bytes, level)
}

/// Returns `true` when the attachment content-type should be compressed.
///
/// **Compressed types** (text-ish):
/// - `text/*`
/// - `application/json`
/// - `application/x-ndjson`
/// - `application/yaml`
/// - `application/xml`
///
/// **Skipped types** (already compressed or binary):
/// - `application/zstd`
/// - `application/gzip` / `application/x-gzip`
/// - `image/*`
/// - `video/*`
/// - `audio/*`
/// - `application/zip`
/// - `application/x-tar` (tarballs are uncompressed but often already small)
/// - `application/octet-stream`
pub fn should_compress_content_type(content_type: &str) -> bool {
    // Normalise: strip parameters (e.g. `text/plain; charset=utf-8`)
    let base = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();

    // Fast-path: already-compressed or raw binary types
    if matches!(
        base.as_str(),
        "application/zstd"
            | "application/gzip"
            | "application/x-gzip"
            | "application/zip"
            | "application/x-tar"
            | "application/octet-stream"
    ) {
        return false;
    }
    if base.starts_with("image/") || base.starts_with("video/") || base.starts_with("audio/") {
        return false;
    }

    // Compress text-ish types
    if base.starts_with("text/") {
        return true;
    }
    matches!(
        base.as_str(),
        "application/json"
            | "application/x-ndjson"
            | "application/yaml"
            | "application/x-yaml"
            | "application/xml"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressible_types() {
        assert!(should_compress_content_type("text/plain"));
        assert!(should_compress_content_type("text/plain; charset=utf-8"));
        assert!(should_compress_content_type("application/json"));
        assert!(should_compress_content_type("application/x-ndjson"));
        assert!(should_compress_content_type("application/yaml"));
        assert!(should_compress_content_type("application/xml"));
    }

    #[test]
    fn skip_compressed_types() {
        assert!(!should_compress_content_type("application/zstd"));
        assert!(!should_compress_content_type("application/gzip"));
        assert!(!should_compress_content_type("image/png"));
        assert!(!should_compress_content_type("video/mp4"));
        assert!(!should_compress_content_type("application/octet-stream"));
        assert!(!should_compress_content_type("application/zip"));
    }
}
