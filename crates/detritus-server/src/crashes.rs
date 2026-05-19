use std::{io, path::PathBuf};

use axum::{
    Extension, Json,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use detritus_protocol::{CrashMetadata, PROTOCOL_VERSION};
use futures_util::TryStreamExt;
use multer::Field;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt};
use uuid::Uuid;

use crate::{
    auth::TokenContext,
    server::AppState,
    storage::{SourceKey, StorageError, StoragePaths},
};

const METADATA_MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize)]
pub struct CrashResponse {
    pub id: String,
    pub dedup: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CrashIndex {
    metadata: CrashMetadata,
    dump: BlobPointer,
    attachments: Vec<AttachmentPointer>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BlobPointer {
    sha256: String,
    len: u64,
    path: String,
    dedup: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AttachmentPointer {
    key: String,
    content_type: String,
    sha256: String,
    len: u64,
    path: String,
    dedup: bool,
}

pub async fn crashes_handler(
    State(state): State<AppState>,
    Extension(token): Extension<TokenContext>,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<CrashResponse>), CrashError> {
    let started = std::time::Instant::now();
    let result = crashes_inner(&state, &token, headers, body).await;
    let status = result
        .as_ref()
        .map_or_else(|error| error.status_code(), |(status, _)| *status);
    state
        .metrics
        .observe_request("crashes", status.as_str(), started.elapsed());
    result
}

async fn crashes_inner(
    state: &AppState,
    token: &TokenContext,
    headers: HeaderMap,
    body: Body,
) -> Result<(StatusCode, Json<CrashResponse>), CrashError> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .ok_or_else(|| CrashError::BadRequest("missing content-type header".to_owned()))?
        .to_str()
        .map_err(|_| CrashError::BadRequest("content-type header is not UTF-8".to_owned()))?;
    let boundary = multer::parse_boundary(content_type)
        .map_err(|_| CrashError::BadRequest("missing multipart boundary".to_owned()))?;
    let stream = body.into_data_stream().map_err(io::Error::other);
    let mut multipart = multer::Multipart::new(stream, boundary);

    let metadata_field = multipart
        .next_field()
        .await?
        .ok_or(CrashError::MissingPart("metadata"))?;
    let metadata_name = metadata_field.name().map(str::to_owned);
    if metadata_name.as_deref() != Some("metadata") {
        return Err(CrashError::BadRequest(
            "first multipart part must be `metadata`".to_owned(),
        ));
    }
    let metadata = read_metadata(metadata_field).await?;
    if metadata.schema_version != PROTOCOL_VERSION {
        return Err(CrashError::ProtocolVersion {
            actual: metadata.schema_version,
            expected: PROTOCOL_VERSION,
        });
    }
    let source = SourceKey::new(
        metadata.source.project.clone(),
        metadata.source.install_id.to_string(),
    )?;
    if !token.permits(&source) {
        return Err(CrashError::PermissionDenied);
    }
    state
        .rate_limiter
        .check_crashes(token, &source)
        .await
        .map_err(|_| CrashError::RateLimited)?;

    let mut dump = None;
    let mut attachments = Vec::new();

    while let Some(field) = multipart.next_field().await? {
        let name = field
            .name()
            .ok_or_else(|| CrashError::BadRequest("multipart part is missing a name".to_owned()))?
            .to_owned();
        match name.as_str() {
            "metadata" => {
                return Err(CrashError::BadRequest(
                    "metadata part must appear exactly once".to_owned(),
                ));
            }
            "dump" => {
                if dump.is_some() {
                    return Err(CrashError::BadRequest(
                        "dump part must appear exactly once".to_owned(),
                    ));
                }
                dump = Some(write_part_to_blob(&state.storage, field, state.max_dump_bytes).await?);
            }
            name if name.starts_with("attach:") => {
                let key = name
                    .strip_prefix("attach:")
                    .ok_or_else(|| CrashError::BadRequest("invalid attachment name".to_owned()))?
                    .to_owned();
                let content_type = field.content_type().map_or_else(
                    || "application/octet-stream".to_owned(),
                    ToString::to_string,
                );
                let pointer =
                    write_part_to_blob(&state.storage, field, state.max_dump_bytes).await?;
                attachments.push(AttachmentPointer {
                    key,
                    content_type,
                    sha256: pointer.sha256,
                    len: pointer.len,
                    path: pointer.path,
                    dedup: pointer.dedup,
                });
            }
            _ => return Err(CrashError::BadRequest(format!("unexpected part `{name}`"))),
        }
    }

    let dump = dump.ok_or(CrashError::MissingPart("dump"))?;
    state.metrics.observe_bytes("crashes", dump.len);
    if dump.dedup {
        state.metrics.observe_dedup_hit();
    }
    write_index(&state.storage, &source, metadata, &dump, attachments).await?;

    Ok((
        StatusCode::CREATED,
        Json(CrashResponse {
            id: dump.sha256,
            dedup: dump.dedup,
        }),
    ))
}

async fn read_metadata(mut field: Field<'_>) -> Result<CrashMetadata, CrashError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await? {
        let next_len = bytes.len() as u64 + chunk.len() as u64;
        if next_len > METADATA_MAX_BYTES {
            return Err(CrashError::PayloadTooLarge {
                max: METADATA_MAX_BYTES,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

async fn write_part_to_blob(
    storage: &StoragePaths,
    mut field: Field<'_>,
    max_bytes: u64,
) -> Result<BlobPointer, CrashError> {
    fs::create_dir_all(storage.tmp_dir()).await?;
    let temp_path = storage.tmp_dir().join(format!("{}.tmp", Uuid::new_v4()));
    let mut file = fs::File::create(&temp_path).await?;
    let mut hasher = Sha256::new();
    let mut len = 0_u64;

    while let Some(chunk) = field.chunk().await? {
        len += chunk.len() as u64;
        if len > max_bytes {
            let _ = fs::remove_file(&temp_path).await;
            return Err(CrashError::PayloadTooLarge { max: max_bytes });
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }

    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    let sha256 = hex::encode(hasher.finalize());
    let (path, dedup) = finalize_blob(storage, temp_path, &sha256).await?;
    Ok(BlobPointer {
        sha256,
        len,
        path,
        dedup,
    })
}

async fn finalize_blob(
    storage: &StoragePaths,
    temp_path: PathBuf,
    sha256: &str,
) -> Result<(String, bool), CrashError> {
    let dest = storage.blob_path(sha256);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await?;
    }

    let dedup = if fs::try_exists(&dest).await? {
        fs::remove_file(&temp_path).await?;
        true
    } else {
        match fs::hard_link(&temp_path, &dest).await {
            Ok(()) => {
                fs::remove_file(&temp_path).await?;
                false
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                fs::remove_file(&temp_path).await?;
                true
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                return Err(error.into());
            }
        }
    };

    let relative = storage
        .relative_to_data_dir(&dest)
        .to_string_lossy()
        .into_owned();
    Ok((relative, dedup))
}

async fn write_index(
    storage: &StoragePaths,
    source: &SourceKey,
    metadata: CrashMetadata,
    dump: &BlobPointer,
    attachments: Vec<AttachmentPointer>,
) -> Result<(), CrashError> {
    let timestamp = metadata.timestamp.format("%Y%m%dT%H%M%S%.fZ").to_string();
    let path = storage.index_path(source, &timestamp, &dump.sha256);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let index = CrashIndex {
        metadata,
        dump: BlobPointer {
            sha256: dump.sha256.clone(),
            len: dump.len,
            path: dump.path.clone(),
            dedup: dump.dedup,
        },
        attachments,
    };
    let encoded = serde_json::to_vec_pretty(&index)?;
    fs::write(path, encoded).await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum CrashError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("missing multipart part `{0}`")]
    MissingPart(&'static str),
    #[error("protocol version {actual} does not match {expected}")]
    ProtocolVersion { actual: u32, expected: u32 },
    #[error("token is not permitted to write this source")]
    PermissionDenied,
    #[error("crash rate limit exceeded")]
    RateLimited,
    #[error("payload exceeds maximum size of {max} bytes")]
    PayloadTooLarge { max: u64 },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Multipart(#[from] multer::Error),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl IntoResponse for CrashError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": self.to_string(),
            }
        }));
        (status, body).into_response()
    }
}

impl CrashError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) | Self::MissingPart(_) | Self::Json(_) | Self::Multipart(_) => {
                StatusCode::BAD_REQUEST
            }
            Self::ProtocolVersion { .. } => StatusCode::PRECONDITION_FAILED,
            Self::PermissionDenied => StatusCode::FORBIDDEN,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::PayloadTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Storage(StorageError::InvalidComponent { .. }) => StatusCode::BAD_REQUEST,
            Self::Storage(StorageError::Io(_)) | Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
