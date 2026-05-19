//! Feature-gated multipart helpers for crash envelopes.

use bytes::Bytes;
use futures_util::stream;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    PROTOCOL_VERSION,
    crash::{CrashAttachment, CrashEnvelope, CrashMetadata, ProtocolError},
};

/// Default multipart boundary used by tests and simple SDK callers.
pub const DEFAULT_BOUNDARY: &str = "detritus-boundary-v1";
const CRLF: &str = "\r\n";

impl CrashEnvelope {
    /// Writes the envelope as an RFC 7578 multipart body with [`DEFAULT_BOUNDARY`].
    pub async fn write_to<W>(&self, writer: &mut W) -> Result<(), ProtocolError>
    where
        W: AsyncWrite + Unpin,
    {
        self.write_to_with_boundary(writer, DEFAULT_BOUNDARY).await
    }

    /// Writes the envelope as an RFC 7578 multipart body.
    pub async fn write_to_with_boundary<W>(
        &self,
        writer: &mut W,
        boundary: &str,
    ) -> Result<(), ProtocolError>
    where
        W: AsyncWrite + Unpin,
    {
        let metadata = serde_json::to_vec(&self.metadata)?;
        write_part(writer, boundary, "metadata", "application/json", &metadata).await?;
        write_part(
            writer,
            boundary,
            "dump",
            "application/octet-stream",
            &self.dump,
        )
        .await?;
        for attachment in &self.attachments {
            let name = format!("attach:{}", attachment.key);
            write_part(
                writer,
                boundary,
                &name,
                &attachment.content_type,
                &attachment.bytes,
            )
            .await?;
        }
        writer
            .write_all(format!("--{boundary}--{CRLF}").as_bytes())
            .await?;
        writer.flush().await?;
        Ok(())
    }

    /// Parses an envelope from a body using [`DEFAULT_BOUNDARY`].
    pub async fn read_from<R>(reader: &mut R) -> Result<Self, ProtocolError>
    where
        R: AsyncRead + Unpin,
    {
        Self::read_from_with_boundary(reader, DEFAULT_BOUNDARY).await
    }

    /// Parses an envelope from a body with the supplied multipart boundary.
    pub async fn read_from_with_boundary<R>(
        reader: &mut R,
        boundary: &str,
    ) -> Result<Self, ProtocolError>
    where
        R: AsyncRead + Unpin,
    {
        let mut body = Vec::new();
        reader.read_to_end(&mut body).await?;
        let body = Bytes::from(body);
        let stream = stream::once(async move { Ok::<Bytes, std::io::Error>(body) });
        let mut multipart = multer::Multipart::new(stream, boundary);
        let mut metadata = None;
        let mut dump = None;
        let mut attachments = Vec::new();

        while let Some(field) = multipart.next_field().await? {
            let name = field
                .name()
                .ok_or(ProtocolError::InvalidPartName)?
                .to_owned();
            let content_type = field.content_type().map_or_else(
                || "application/octet-stream".to_owned(),
                ToString::to_string,
            );
            let bytes = field.bytes().await?.to_vec();
            match name.as_str() {
                "metadata" => metadata = Some(serde_json::from_slice::<CrashMetadata>(&bytes)?),
                "dump" => dump = Some(bytes),
                name if name.starts_with("attach:") => {
                    let key = name
                        .strip_prefix("attach:")
                        .ok_or_else(|| ProtocolError::InvalidMultipart(name.to_owned()))?
                        .to_owned();
                    attachments.push(CrashAttachment {
                        key,
                        content_type,
                        bytes,
                    });
                }
                _ => return Err(ProtocolError::InvalidMultipart(name)),
            }
        }

        let metadata = metadata.ok_or(ProtocolError::MissingPart("metadata"))?;
        if metadata.schema_version != PROTOCOL_VERSION {
            return Err(ProtocolError::InvalidMultipart(format!(
                "schema version {} does not match protocol version {}",
                metadata.schema_version, PROTOCOL_VERSION
            )));
        }
        let dump = dump.ok_or(ProtocolError::MissingPart("dump"))?;
        Ok(Self {
            metadata,
            dump,
            attachments,
        })
    }
}

async fn write_part<W>(
    writer: &mut W,
    boundary: &str,
    name: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(format!("--{boundary}{CRLF}").as_bytes())
        .await?;
    writer
        .write_all(
            format!(
                "Content-Disposition: form-data; name=\"{name}\"{CRLF}Content-Type: {content_type}{CRLF}{CRLF}"
            )
            .as_bytes(),
        )
        .await?;
    writer.write_all(bytes).await?;
    writer.write_all(CRLF.as_bytes()).await?;
    Ok(())
}
