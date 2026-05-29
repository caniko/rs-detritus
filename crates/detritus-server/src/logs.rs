use std::{collections::HashMap, sync::Arc};

use chrono::{NaiveDate, Utc};
use detritus_protocol::{
    GRPC_VERSION_KEY, PROTOCOL_VERSION,
    otlp::{
        common::{AnyValue, KeyValue, any_value},
        logs::{
            ExportLogsServiceRequest, ExportLogsServiceResponse, LogRecord, LogsService,
            ResourceLogs,
        },
    },
};
use serde_json::{Value, json};
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use tonic::{Request, Response, Status};

use crate::{
    auth::token_from_extensions,
    metrics::Metrics,
    rate_limit::RateLimiter,
    schemas::{SchemaError, SchemaKind, SchemaRegistry},
    storage::{SourceKey, StorageError, StoragePaths},
};

const WRITER_CHANNEL_CAPACITY: usize = 10_000;

#[derive(Clone)]
pub(crate) struct LogsHandler {
    writers: LogWriterPool,
    rate_limiter: RateLimiter,
    metrics: Metrics,
    schema_registry: SchemaRegistry,
}

impl LogsHandler {
    pub(crate) fn new(
        writers: LogWriterPool,
        rate_limiter: RateLimiter,
        metrics: Metrics,
        schema_registry: SchemaRegistry,
    ) -> Self {
        Self {
            writers,
            rate_limiter,
            metrics,
            schema_registry,
        }
    }
}

#[tonic::async_trait]
impl LogsService for LogsHandler {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let started = std::time::Instant::now();
        let result = self.export_inner(request).await;
        let status = result
            .as_ref()
            .map_or_else(|error| grpc_status_label(error.code()), |_| "200");
        self.metrics
            .observe_request("logs", status, started.elapsed());
        result
    }
}

impl LogsHandler {
    async fn export_inner(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        validate_protocol_metadata(request.metadata())?;
        let token = token_from_extensions(request.extensions())?;
        let resource_logs_batch = request.into_inner().resource_logs;

        for resource_logs in &resource_logs_batch {
            let value = resource_logs_to_validation_value(resource_logs);
            match self
                .schema_registry
                .validate(&token.project, SchemaKind::LogAttributes, &value)
            {
                Ok(()) => {}
                Err(SchemaError::Validation { errors, .. }) => {
                    self.metrics.observe_validation_failure("logs");
                    let joined = errors
                        .iter()
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let message = format!("log attribute validation failed: {joined}");
                    let truncated = if message.len() > 1024 {
                        // Truncate at a UTF-8 char boundary at or below byte 1021.
                        // Validation messages embed attribute content, which can
                        // contain multi-byte characters; slicing mid-character
                        // would panic. The loop terminates because byte 0 is always
                        // a boundary, and `end <= 1021 < message.len()` here.
                        let mut end = 1021;
                        while !message.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}…", &message[..end])
                    } else {
                        message
                    };
                    return Err(Status::invalid_argument(truncated));
                }
                Err(other) => {
                    tracing::error!(error = %other, "schema registry internal error");
                    return Err(Status::internal("schema validation internal error"));
                }
            }
        }

        for resource_logs in resource_logs_batch {
            let source = source_from_resource_logs(&resource_logs)?;
            if !token.permits(&source) {
                return Err(Status::permission_denied(
                    "token is not permitted to write this source",
                ));
            }
            self.rate_limiter
                .check_logs(&token, &source)
                .await
                .map_err(|_| Status::resource_exhausted("log rate limit exceeded"))?;
            let sender = self.writers.sender_for(source.clone()).await?;
            for scope_logs in resource_logs.scope_logs {
                for record in scope_logs.log_records {
                    let line = log_record_json(&record);
                    sender
                        .try_send(line)
                        .map_err(|_| Status::resource_exhausted("log writer queue is full"))?;
                    let depth = sender.max_capacity() - sender.capacity();
                    self.metrics
                        .set_writer_queue_depth(&source.canonical(), depth as i64);
                }
            }
        }

        Ok(Response::new(ExportLogsServiceResponse {
            partial_success: None,
        }))
    }
}

fn grpc_status_label(code: tonic::Code) -> &'static str {
    match code {
        tonic::Code::Ok => "200",
        tonic::Code::Unauthenticated => "401",
        tonic::Code::PermissionDenied => "403",
        tonic::Code::ResourceExhausted => "429",
        tonic::Code::InvalidArgument => "400",
        tonic::Code::FailedPrecondition => "412",
        _ => "500",
    }
}

#[allow(clippy::result_large_err)]
fn validate_protocol_metadata(metadata: &tonic::metadata::MetadataMap) -> Result<(), Status> {
    let version = metadata
        .get(GRPC_VERSION_KEY)
        .ok_or_else(|| Status::failed_precondition("missing x-protocol-version metadata"))?
        .to_str()
        .map_err(|_| Status::failed_precondition("x-protocol-version metadata is not UTF-8"))?;
    let parsed = version
        .parse::<u32>()
        .map_err(|_| Status::failed_precondition("x-protocol-version metadata is not a u32"))?;
    if parsed == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(Status::failed_precondition(format!(
            "protocol version {parsed} does not match {PROTOCOL_VERSION}"
        )))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LogWriterPool {
    inner: Arc<Mutex<HashMap<SourceKey, WriterHandle>>>,
    storage: StoragePaths,
}

impl LogWriterPool {
    pub(crate) fn new(storage: StoragePaths) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            storage,
        }
    }

    async fn sender_for(&self, source: SourceKey) -> Result<mpsc::Sender<Value>, Status> {
        let mut writers = self.inner.lock().await;
        if let Some(handle) = writers.get(&source) {
            return Ok(handle.sender.clone());
        }

        let (sender, receiver) = mpsc::channel(WRITER_CHANNEL_CAPACITY);
        let join = tokio::spawn(writer_loop(self.storage.clone(), source.clone(), receiver));
        writers.insert(
            source,
            WriterHandle {
                sender: sender.clone(),
                join,
            },
        );
        Ok(sender)
    }

    pub(crate) async fn shutdown(&self) {
        let handles = {
            let mut writers = self.inner.lock().await;
            writers
                .drain()
                .map(|(_, handle)| handle.join)
                .collect::<Vec<_>>()
        };

        for handle in handles {
            if let Err(error) = handle.await {
                tracing::error!(%error, "log writer task panicked during shutdown");
            }
        }
    }
}

#[derive(Debug)]
struct WriterHandle {
    sender: mpsc::Sender<Value>,
    join: JoinHandle<()>,
}

async fn writer_loop(
    storage: StoragePaths,
    source: SourceKey,
    mut receiver: mpsc::Receiver<Value>,
) {
    let mut current_date = None;
    let mut file = None;

    while let Some(line) = receiver.recv().await {
        let today = Utc::now().date_naive();
        if current_date != Some(today) {
            if let Err(error) = flush_file(file.take()).await {
                tracing::error!(%error, ?source, "failed to flush log file before rotation");
            }
            match open_log_file(&storage, &source, today).await {
                Ok(opened) => {
                    file = Some(opened);
                    current_date = Some(today);
                }
                Err(error) => {
                    tracing::error!(%error, ?source, "failed to open log file");
                    continue;
                }
            }
        }

        if let Some(opened) = file.as_mut() {
            match serde_json::to_vec(&line) {
                Ok(mut encoded) => {
                    encoded.push(b'\n');
                    if let Err(error) = opened.write_all(&encoded).await {
                        tracing::error!(%error, ?source, "failed to append log record");
                    }
                }
                Err(error) => tracing::error!(%error, ?source, "failed to encode log record"),
            }
        }
    }

    if let Err(error) = flush_file(file).await {
        tracing::error!(%error, ?source, "failed to flush log file during shutdown");
    }
}

async fn open_log_file(
    storage: &StoragePaths,
    source: &SourceKey,
    date: NaiveDate,
) -> Result<File, StorageError> {
    let path = storage.log_file(source, date);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?)
}

async fn flush_file(file: Option<File>) -> std::io::Result<()> {
    if let Some(mut opened) = file {
        opened.flush().await?;
        opened.sync_all().await?;
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
/// Builds the validation document for one [`ResourceLogs`] entry.
///
/// Shape: `{"resource": <resource attrs object>, "scopes": [{"name": "...",
/// "attributes": <scope attrs object>}, ...]}`. The `project` used for the
/// schema lookup comes from `token.project`, never from resource attributes —
/// auth scoping is the source of authority.
fn resource_logs_to_validation_value(resource_logs: &ResourceLogs) -> Value {
    let resource_attrs = resource_logs.resource.as_ref().map_or_else(
        || Value::Object(serde_json::Map::new()),
        |r| attributes_json(&r.attributes),
    );

    let scopes: Vec<Value> = resource_logs
        .scope_logs
        .iter()
        .map(|sl| {
            let name = sl
                .scope
                .as_ref()
                .map(|s| s.name.as_str())
                .unwrap_or("")
                .to_owned();
            let scope_attrs = sl.scope.as_ref().map_or_else(
                || Value::Object(serde_json::Map::new()),
                |s| attributes_json(&s.attributes),
            );
            json!({ "name": name, "attributes": scope_attrs })
        })
        .collect();

    json!({ "resource": resource_attrs, "scopes": scopes })
}

fn source_from_resource_logs(resource_logs: &ResourceLogs) -> Result<SourceKey, Status> {
    let attributes = resource_logs
        .resource
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("resource_logs.resource is required"))?
        .attributes
        .as_slice();
    let project = required_attr(attributes, "source.project")?;
    let _platform = required_attr(attributes, "source.platform")?;
    let _version = required_attr(attributes, "source.version")?;
    let install_id = required_attr(attributes, "source.install_id")?;
    SourceKey::new(project, install_id)
        .map_err(|error| Status::invalid_argument(format!("invalid source identity: {error}")))
}

#[allow(clippy::result_large_err)]
fn required_attr(attributes: &[KeyValue], key: &'static str) -> Result<String, Status> {
    attributes
        .iter()
        .find(|attr| attr.key == key)
        .and_then(|attr| attr.value.as_ref())
        .and_then(any_value_string)
        .ok_or_else(|| Status::invalid_argument(format!("missing resource attribute `{key}`")))
}

fn any_value_string(value: &AnyValue) -> Option<String> {
    match value.value.as_ref()? {
        any_value::Value::StringValue(value) => Some(value.clone()),
        _ => None,
    }
}

fn any_value_json(value: &AnyValue) -> Value {
    match value.value.as_ref() {
        Some(any_value::Value::StringValue(value)) => json!(value),
        Some(any_value::Value::BoolValue(value)) => json!(value),
        Some(any_value::Value::IntValue(value)) => json!(value),
        Some(any_value::Value::DoubleValue(value)) => json!(value),
        Some(any_value::Value::ArrayValue(value)) => {
            Value::Array(value.values.iter().map(any_value_json).collect())
        }
        Some(any_value::Value::KvlistValue(value)) => Value::Object(
            value
                .values
                .iter()
                .filter_map(|entry| {
                    entry
                        .value
                        .as_ref()
                        .map(|value| (entry.key.clone(), any_value_json(value)))
                })
                .collect(),
        ),
        Some(any_value::Value::BytesValue(value)) => json!(hex::encode(value)),
        None => Value::Null,
    }
}

fn attributes_json(attributes: &[KeyValue]) -> Value {
    Value::Object(
        attributes
            .iter()
            .filter_map(|entry| {
                entry
                    .value
                    .as_ref()
                    .map(|value| (entry.key.clone(), any_value_json(value)))
            })
            .collect(),
    )
}

fn log_record_json(record: &LogRecord) -> Value {
    json!({
        "time_unix_nano": record.time_unix_nano,
        "observed_time_unix_nano": record.observed_time_unix_nano,
        "severity_number": record.severity_number,
        "severity_text": record.severity_text,
        "body": record.body.as_ref().map(any_value_json),
        "attributes": attributes_json(&record.attributes),
        "dropped_attributes_count": record.dropped_attributes_count,
        "flags": record.flags,
        "trace_id": hex::encode(&record.trace_id),
        "span_id": hex::encode(&record.span_id),
        "event_name": record.event_name,
    })
}
