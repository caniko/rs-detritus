use std::{
    fmt,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use detritus_protocol::{
    GRPC_VERSION_KEY, PROTOCOL_VERSION, SourceId,
    otlp::{
        common::{AnyValue, InstrumentationScope, KeyValue, any_value},
        logs::{
            ExportLogsServiceRequest, LogRecord, LogsServiceClient, ResourceLogs, ScopeLogs,
            SeverityNumber,
        },
        resource::Resource,
    },
};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{mpsc, oneshot};
use tonic::metadata::MetadataValue;
use tracing::{Event, Subscriber};
use tracing_core::{Level, field};
use tracing_subscriber::{Layer as SubscriberLayer, layer::Context};
use url::Url;

use crate::spool;

const DEFAULT_BATCH_SIZE: usize = 256;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_CHANNEL_CAPACITY: usize = 4096;

/// A tracing subscriber layer that exports events to the observability server.
#[derive(Debug, Clone)]
pub struct Layer {
    sender: mpsc::Sender<WorkerMessage>,
}

impl Layer {
    /// Starts a builder for an observability tracing layer.
    #[must_use]
    pub fn builder() -> LayerBuilder {
        LayerBuilder::default()
    }

    /// Requests a best-effort flush of queued records.
    ///
    /// # Errors
    ///
    /// Returns [`LayerError::WorkerStopped`] if the background exporter has
    /// stopped, or [`LayerError::Flush`] if the flush could not complete (for
    /// example it timed out or the export failed and could not be spooled).
    pub async fn flush(&self) -> Result<(), LayerError> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(WorkerMessage::Flush(sender))
            .await
            .map_err(|_| LayerError::WorkerStopped)?;
        receiver.await.map_err(|_| LayerError::WorkerStopped)?
    }
}

/// Builder for [`Layer`].
#[derive(Debug, Clone)]
pub struct LayerBuilder {
    endpoint: Option<Url>,
    token: Option<SecretString>,
    source: Option<SourceId>,
    batch_size: usize,
    flush_interval: Duration,
    flush_timeout: Duration,
    queue_dir: Option<PathBuf>,
    sample_rate: f64,
}

impl Default for LayerBuilder {
    fn default() -> Self {
        Self {
            endpoint: None,
            token: None,
            source: None,
            batch_size: DEFAULT_BATCH_SIZE,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            flush_timeout: DEFAULT_FLUSH_TIMEOUT,
            queue_dir: None,
            sample_rate: 1.0,
        }
    }
}

impl LayerBuilder {
    /// Sets the OTLP/gRPC endpoint, for example `http://127.0.0.1:4317`.
    #[must_use]
    pub fn endpoint(mut self, endpoint: Url) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    /// Sets the bearer token sent to the observability server.
    #[must_use]
    pub fn token(mut self, token: SecretString) -> Self {
        self.token = Some(token);
        self
    }

    /// Sets the source identity attached to every exported batch.
    #[must_use]
    pub fn source(mut self, source: SourceId) -> Self {
        self.source = Some(source);
        self
    }

    /// Sets the maximum records per export request.
    #[must_use]
    pub fn batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    /// Sets the periodic flush interval.
    #[must_use]
    pub fn flush_interval(mut self, flush_interval: Duration) -> Self {
        self.flush_interval = flush_interval;
        self
    }

    /// Sets the maximum time a requested flush waits for network export.
    #[must_use]
    pub fn flush_timeout(mut self, flush_timeout: Duration) -> Self {
        self.flush_timeout = flush_timeout;
        self
    }

    /// Sets the directory used for offline protobuf log batches.
    #[must_use]
    pub fn queue_dir(mut self, queue_dir: PathBuf) -> Self {
        self.queue_dir = Some(queue_dir);
        self
    }

    /// Sets a deterministic sample rate in the inclusive range `0.0..=1.0`.
    #[must_use]
    pub fn sample_rate(mut self, sample_rate: f64) -> Self {
        self.sample_rate = sample_rate.clamp(0.0, 1.0);
        self
    }

    /// Builds the layer and spawns its background exporter on the current Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns [`LayerError::MissingEndpoint`], [`LayerError::MissingToken`],
    /// [`LayerError::MissingSource`], or [`LayerError::MissingQueueDir`] if the
    /// corresponding builder field was not set.
    pub fn build(self) -> Result<Layer, LayerError> {
        let endpoint = self.endpoint.ok_or(LayerError::MissingEndpoint)?;
        let token = self.token.ok_or(LayerError::MissingToken)?;
        let source = self.source.ok_or(LayerError::MissingSource)?;
        let queue_dir = self.queue_dir.ok_or(LayerError::MissingQueueDir)?;
        let (sender, receiver) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        let worker = Worker {
            endpoint,
            token,
            source,
            batch_size: self.batch_size,
            flush_interval: self.flush_interval,
            flush_timeout: self.flush_timeout,
            queue_dir,
            sample_rate: self.sample_rate,
            receiver,
        };
        // The exporter is a process-lifetime background task: it is intentionally
        // detached. The handle is deliberately dropped rather than joined — the
        // worker only returns once every `Layer` clone is dropped, so awaiting it
        // here would hang, and `let _ =` on the `JoinHandle` future would trip
        // `clippy::let_underscore_future`. Graceful shutdown is coordinated over
        // the channel via `Layer::flush`.
        tokio::spawn(worker.run());
        Ok(Layer { sender })
    }
}

/// Errors returned while building or flushing a [`Layer`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LayerError {
    /// Builder is missing an endpoint.
    #[error("observability layer endpoint is required")]
    MissingEndpoint,
    /// Builder is missing a bearer token.
    #[error("observability layer token is required")]
    MissingToken,
    /// Builder is missing a source identity.
    #[error("observability layer source is required")]
    MissingSource,
    /// Builder is missing a queue directory.
    #[error("observability layer queue_dir is required")]
    MissingQueueDir,
    /// Background worker has stopped.
    #[error("observability layer worker stopped")]
    WorkerStopped,
    /// Export failed and the batch could not be spooled.
    #[error("observability layer flush failed: {0}")]
    Flush(String),
}

impl<S> SubscriberLayer<S> for Layer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let record = event_to_log_record(event);
        let _ = self.sender.try_send(WorkerMessage::Record(record));
    }
}

#[derive(Debug)]
enum WorkerMessage {
    Record(LogRecord),
    Flush(oneshot::Sender<Result<(), LayerError>>),
}

struct Worker {
    endpoint: Url,
    token: SecretString,
    source: SourceId,
    batch_size: usize,
    flush_interval: Duration,
    flush_timeout: Duration,
    queue_dir: PathBuf,
    sample_rate: f64,
    receiver: mpsc::Receiver<WorkerMessage>,
}

impl Worker {
    async fn run(mut self) {
        let mut batch = Vec::with_capacity(self.batch_size);
        let mut interval = tokio::time::interval(self.flush_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        self.drain_spooled().await;

        loop {
            tokio::select! {
                Some(message) = self.receiver.recv() => {
                    match message {
                        WorkerMessage::Record(record) => {
                            if self.should_sample(&record) {
                                batch.push(record);
                            }
                            if batch.len() >= self.batch_size {
                                self.export_or_spool(std::mem::take(&mut batch)).await;
                            }
                        }
                        WorkerMessage::Flush(reply) => {
                            let records = std::mem::take(&mut batch);
                            let result = tokio::time::timeout(
                                self.flush_timeout,
                                self.export_records(records),
                            )
                            .await
                            .unwrap_or_else(|_| Err(LayerError::Flush("flush timed out".to_owned())));
                            let _ = reply.send(result);
                        }
                    }
                }
                _ = interval.tick() => {
                    self.export_or_spool(std::mem::take(&mut batch)).await;
                }
                else => {
                    self.export_or_spool(batch).await;
                    break;
                }
            }
        }
    }

    fn should_sample(&self, record: &LogRecord) -> bool {
        if self.sample_rate >= 1.0 {
            true
        } else if self.sample_rate <= 0.0 {
            false
        } else {
            let bucket = record.time_unix_nano % 10_000;
            (bucket as f64 / 10_000.0) < self.sample_rate
        }
    }

    async fn drain_spooled(&self) {
        let Ok(_lock) = spool::SpoolLock::acquire(&self.queue_dir) else {
            return;
        };
        let Ok(paths) = spool::pending_log_batches(&self.queue_dir) else {
            return;
        };
        for path in paths {
            let Ok(request) = spool::read_log_batch(&path) else {
                continue;
            };
            if export_request(&self.endpoint, &self.token, request)
                .await
                .is_ok()
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    async fn export_or_spool(&self, records: Vec<LogRecord>) {
        if let Err(error) = self.export_records(records).await {
            // stderr, not tracing: this runs inside the exporter worker, and the
            // detritus Layer may be installed in the subscriber — reporting export
            // failures via tracing would feed them straight back into this worker.
            eprintln!("[observability] failed to export logs: {error}");
        }
    }

    async fn export_records(&self, records: Vec<LogRecord>) -> Result<(), LayerError> {
        if records.is_empty() {
            return Ok(());
        }
        let request = export_request_for(&self.source, records);
        match export_request(&self.endpoint, &self.token, request.clone()).await {
            Ok(()) => Ok(()),
            Err(error) => {
                spool::write_log_batch(&self.queue_dir, &request)
                    .map_err(|io| LayerError::Flush(io.to_string()))?;
                Err(LayerError::Flush(error.to_string()))
            }
        }
    }
}

async fn export_request(
    endpoint: &Url,
    token: &SecretString,
    request: ExportLogsServiceRequest,
) -> Result<(), tonic::Status> {
    let mut client = LogsServiceClient::connect(endpoint.to_string())
        .await
        .map_err(|error| tonic::Status::unavailable(error.to_string()))?;
    let mut request = tonic::Request::new(request);
    request.metadata_mut().insert(
        GRPC_VERSION_KEY,
        MetadataValue::try_from(PROTOCOL_VERSION.to_string())
            .map_err(|error| tonic::Status::internal(error.to_string()))?,
    );
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token.expose_secret()))
            .map_err(|error| tonic::Status::internal(error.to_string()))?,
    );
    client.export(request).await?;
    Ok(())
}

fn export_request_for(source: &SourceId, records: Vec<LogRecord>) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![
                    string_attr("source.project", &source.project),
                    string_attr("source.platform", &source.platform),
                    string_attr("source.version", &source.version),
                    string_attr("source.install_id", &source.install_id.to_string()),
                ],
                dropped_attributes_count: 0,
                entity_refs: Vec::new(),
            }),
            scope_logs: vec![ScopeLogs {
                scope: Some(InstrumentationScope {
                    name: "detritus-client".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                    attributes: Vec::new(),
                    dropped_attributes_count: 0,
                }),
                log_records: records,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

fn event_to_log_record(event: &Event<'_>) -> LogRecord {
    let now = unix_nanos(SystemTime::now());
    let metadata = event.metadata();
    let mut visitor = EventVisitor::default();
    event.record(&mut visitor);
    let severity_text = metadata.level().to_string();
    let EventVisitor { message, fields } = visitor;
    let body = message
        .or_else(|| {
            fields
                .iter()
                .find(|(key, _)| key == "message")
                .map(|(_, value)| value.clone())
        })
        .unwrap_or_else(|| metadata.name().to_owned());
    // Move the visited (key, value) strings straight into attributes rather than
    // re-allocating both through string_attr's to_owned, and pre-size for the two
    // trailing target/name attributes appended below.
    let mut attributes = Vec::with_capacity(fields.len() + 2);
    attributes.extend(
        fields
            .into_iter()
            .map(|(key, value)| owned_string_attr(key, value)),
    );
    attributes.push(string_attr("target", metadata.target()));
    attributes.push(string_attr("name", metadata.name()));

    LogRecord {
        time_unix_nano: now,
        observed_time_unix_nano: now,
        severity_number: severity_number(metadata.level()) as i32,
        severity_text,
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(body)),
        }),
        attributes,
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: Vec::new(),
        span_id: Vec::new(),
        event_name: metadata.name().to_owned(),
    }
}

fn unix_nanos(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

fn severity_number(level: &Level) -> SeverityNumber {
    match *level {
        Level::ERROR => SeverityNumber::Error,
        Level::WARN => SeverityNumber::Warn,
        Level::INFO => SeverityNumber::Info,
        Level::DEBUG => SeverityNumber::Debug,
        Level::TRACE => SeverityNumber::Trace,
    }
}

fn string_attr(key: &str, value: &str) -> KeyValue {
    owned_string_attr(key.to_owned(), value.to_owned())
}

fn owned_string_attr(key: String, value: String) -> KeyValue {
    KeyValue {
        key,
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value)),
        }),
    }
}

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl field::Visit for EventVisitor {
    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value.clone());
        }
        self.fields.push((field.name().to_owned(), value));
    }

    fn record_str(&mut self, field: &field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        }
        self.fields
            .push((field.name().to_owned(), value.to_owned()));
    }

    fn record_i64(&mut self, field: &field::Field, value: i64) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }

    fn record_u64(&mut self, field: &field::Field, value: u64) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }

    fn record_bool(&mut self, field: &field::Field, value: bool) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }
}
