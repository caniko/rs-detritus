use std::{
    backtrace::Backtrace,
    ffi::OsStr,
    fs, io,
    panic::PanicHookInfo,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use detritus_protocol::{AttachmentManifest, BuildInfo, CrashKind, CrashMetadata, SourceId};
use flate2::{Compression, write::GzEncoder};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;
use uuid::Uuid;

#[cfg(all(feature = "minidump", not(target_os = "android")))]
static MINIDUMPER_HANDLES: std::sync::OnceLock<
    parking_lot::Mutex<Vec<minidumper_child::ClientHandle>>,
> = std::sync::OnceLock::new();

/// Configuration for the process-wide panic hook.
#[derive(Debug, Clone)]
pub struct PanicHookConfig {
    /// HTTP endpoint for crash upload. A base server URL is joined with `/v1/crashes`.
    pub endpoint: Url,
    /// Bearer token used by [`crate::ship_pending_crashes`].
    pub token: SecretString,
    /// Source identity stored in crash metadata.
    pub source: SourceId,
    /// Root directory containing `pending/` and `sent/` crash entries.
    pub spool_dir: PathBuf,
    /// Crash artifact strategy.
    pub kind: PanicKind,
    /// Build metadata stored in crash reports.
    pub build: BuildInfo,
    /// Extra JSON context stored in crash reports.
    pub context: serde_json::Value,
    /// Extra files copied into each crash entry as attachments.
    pub context_files: Vec<PathBuf>,
    /// Number of days to retain successfully sent entries locally.
    pub sent_retention_days: u64,
}

/// Crash artifact strategy used by [`install_panic_hook`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanicKind {
    /// Native minidump output where platform support is available.
    ///
    /// `minidumper-child` supports Linux, Windows, and macOS. Android runtime
    /// minidumps require Crashpad or Breakpad integration, so Android builds
    /// should use [`PanicKind::PanicTarball`].
    Minidump,
    /// Portable panic bundle containing `panic.txt`, `backtrace.txt`, and `env.json`.
    PanicTarball,
}

impl PanicKind {
    fn panic_crash_kind(self) -> CrashKind {
        match self {
            Self::Minidump => CrashKind::PanicTarball,
            Self::PanicTarball => CrashKind::PanicTarball,
        }
    }
}

/// Errors returned while installing or running the panic hook setup.
#[derive(Debug, thiserror::Error)]
pub enum PanicHookError {
    /// Hook spool directory could not be created.
    #[error("failed to prepare panic spool directory: {0}")]
    Io(#[from] io::Error),
    /// Native minidump handler could not be started.
    #[cfg(all(feature = "minidump", not(target_os = "android")))]
    #[error("failed to start minidump child process: {0}")]
    Minidump(#[from] minidumper_child::Error),
}

/// Installs a process-wide hook that synchronously spools panic artifacts.
///
/// The installed hook performs no network I/O. It writes a pending crash entry
/// and then chains to the hook that was installed previously.
pub fn install_panic_hook(config: PanicHookConfig) -> Result<(), PanicHookError> {
    fs::create_dir_all(config.spool_dir.join("pending"))?;
    fs::create_dir_all(config.spool_dir.join("sent"))?;
    install_minidumper_if_requested(&config)?;
    let previous = std::panic::take_hook();
    let previous = Arc::new(previous);
    std::panic::set_hook(Box::new(move |info| {
        if let Err(error) = write_pending_crash(&config, info) {
            eprintln!("[observability] failed to write panic artifact: {error}");
        }
        previous(info);
    }));
    Ok(())
}

#[cfg(all(feature = "minidump", not(target_os = "android")))]
fn install_minidumper_if_requested(config: &PanicHookConfig) -> Result<(), PanicHookError> {
    if config.kind != PanicKind::Minidump {
        return Ok(());
    }
    let config = config.clone();
    let handle = minidumper_child::MinidumperChild::new()
        .with_crashes_dir(config.spool_dir.join("minidumper"))
        .on_minidump(move |buffer, _path| {
            if let Err(error) = write_pending_minidump(&config, &buffer) {
                eprintln!("[observability] failed to write minidump artifact: {error}");
            }
        })
        .spawn()?;
    MINIDUMPER_HANDLES
        .get_or_init(|| parking_lot::Mutex::new(Vec::new()))
        .lock()
        .push(handle);
    Ok(())
}

#[cfg(any(not(feature = "minidump"), target_os = "android"))]
fn install_minidumper_if_requested(_config: &PanicHookConfig) -> Result<(), PanicHookError> {
    Ok(())
}

fn write_pending_crash(config: &PanicHookConfig, info: &PanicHookInfo<'_>) -> io::Result<()> {
    let id = Uuid::new_v4();
    let dir = config.spool_dir.join("pending").join(id.to_string());
    fs::create_dir_all(&dir)?;
    let panic_text = panic_text(info);
    let dump = match config.kind {
        PanicKind::PanicTarball => panic_tarball(&panic_text)?,
        PanicKind::Minidump => panic_tarball(&panic_text)?,
    };
    let mut metadata = CrashMetadata::new(
        config.source.clone(),
        Utc::now(),
        config.kind.panic_crash_kind(),
        config.build.clone(),
        config.context.clone(),
    );
    metadata.panic_text = Some(panic_text);
    metadata.attachments.push(AttachmentManifest {
        key: "sdk-config".to_owned(),
        filename: Some("sdk-config.json".to_owned()),
        content_type: "application/json".to_owned(),
        len: serde_json::to_vec(&StoredUploadConfig::from_config(config))
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0),
    });
    let context_attachments = copy_context_files(config, &dir)?;
    metadata.attachments.extend(context_attachments);

    fs::write(
        dir.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)
            .map_err(|error| io::Error::other(error.to_string()))?,
    )?;
    fs::write(dir.join("dump.bin"), dump)?;
    fs::write(
        dir.join("sdk-config.json"),
        serde_json::to_vec_pretty(&StoredUploadConfig::from_config(config))
            .map_err(|error| io::Error::other(error.to_string()))?,
    )?;
    Ok(())
}

#[cfg(all(feature = "minidump", not(target_os = "android")))]
fn write_pending_minidump(config: &PanicHookConfig, dump: &[u8]) -> io::Result<()> {
    let id = Uuid::new_v4();
    let dir = config.spool_dir.join("pending").join(id.to_string());
    fs::create_dir_all(&dir)?;
    let metadata = CrashMetadata::new(
        config.source.clone(),
        Utc::now(),
        CrashKind::Minidump,
        config.build.clone(),
        config.context.clone(),
    );
    let mut metadata = metadata;
    let context_attachments = copy_context_files(config, &dir)?;
    metadata.attachments.extend(context_attachments);
    fs::write(
        dir.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)
            .map_err(|error| io::Error::other(error.to_string()))?,
    )?;
    fs::write(dir.join("dump.bin"), dump)?;
    fs::write(
        dir.join("sdk-config.json"),
        serde_json::to_vec_pretty(&StoredUploadConfig::from_config(config))
            .map_err(|error| io::Error::other(error.to_string()))?,
    )?;
    Ok(())
}

fn copy_context_files(config: &PanicHookConfig, dir: &Path) -> io::Result<Vec<AttachmentManifest>> {
    config
        .context_files
        .iter()
        .enumerate()
        .map(|(index, path)| copy_context_file(index, path, dir))
        .collect()
}

fn copy_context_file(index: usize, path: &Path, dir: &Path) -> io::Result<AttachmentManifest> {
    let filename = path
        .file_name()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("context-{index}.json"));
    let bytes = fs::read(path)?;
    fs::write(dir.join(&filename), &bytes)?;
    Ok(AttachmentManifest {
        key: format!("context-{index}"),
        filename: Some(filename),
        content_type: "application/json".to_owned(),
        len: bytes.len() as u64,
    })
}

fn panic_text(info: &PanicHookInfo<'_>) -> String {
    let payload = if let Some(value) = info.payload().downcast_ref::<&str>() {
        (*value).to_owned()
    } else if let Some(value) = info.payload().downcast_ref::<String>() {
        value.clone()
    } else {
        "<non-string panic payload>".to_owned()
    };
    match info.location() {
        Some(location) => format!(
            "{payload}\n\nat {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        ),
        None => payload,
    }
}

fn panic_tarball(panic_text: &str) -> io::Result<Vec<u8>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    append_bytes(&mut builder, "panic.txt", panic_text.as_bytes())?;
    let backtrace = format!("{:?}", Backtrace::force_capture());
    append_bytes(&mut builder, "backtrace.txt", backtrace.as_bytes())?;
    let env = json!({
        "args": std::env::args().collect::<Vec<_>>(),
        "current_dir": std::env::current_dir().ok(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    });
    let env =
        serde_json::to_vec_pretty(&env).map_err(|error| io::Error::other(error.to_string()))?;
    append_bytes(&mut builder, "env.json", &env)?;
    let encoder = builder.into_inner()?;
    encoder.finish()
}

fn append_bytes<W: io::Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(path)?;
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredUploadConfig {
    pub(crate) endpoint: String,
    pub(crate) sent_retention_days: u64,
}

impl StoredUploadConfig {
    fn from_config(config: &PanicHookConfig) -> Self {
        Self {
            endpoint: config.endpoint.to_string(),
            sent_retention_days: config.sent_retention_days,
        }
    }
}
