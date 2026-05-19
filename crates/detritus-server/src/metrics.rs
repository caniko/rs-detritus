use std::{sync::Arc, time::Duration};

use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts,
    Registry, TextEncoder,
};

#[derive(Clone)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

struct MetricsInner {
    registry: Registry,
    requests_total: IntCounterVec,
    request_duration_seconds: HistogramVec,
    bytes_ingested_total: IntCounterVec,
    dedup_hits_total: IntCounter,
    writer_queue_depth: IntGaugeVec,
    janitor_cycles_total: IntCounter,
    janitor_blobs_freed_total: IntCounter,
    janitor_bytes_freed_total: IntCounter,
    janitor_cycle_duration_seconds: HistogramVec,
    janitor_last_logs_deleted: IntGauge,
    janitor_last_indexes_deleted: IntGauge,
    janitor_last_blobs_deleted: IntGauge,
}

impl Metrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        let requests_total = IntCounterVec::new(
            Opts::new(
                "detritus_requests_total",
                "Total requests by endpoint and status.",
            ),
            &["endpoint", "status"],
        )?;
        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "detritus_request_duration_seconds",
                "Request duration by endpoint.",
            ),
            &["endpoint"],
        )?;
        let bytes_ingested_total = IntCounterVec::new(
            Opts::new(
                "detritus_bytes_ingested_total",
                "Bytes ingested by endpoint.",
            ),
            &["endpoint"],
        )?;
        let dedup_hits_total = IntCounter::new(
            "detritus_dedup_hits_total",
            "Crash blob deduplication hits.",
        )?;
        let writer_queue_depth = IntGaugeVec::new(
            Opts::new(
                "detritus_writer_queue_depth",
                "Current queued log records by source.",
            ),
            &["source_id"],
        )?;
        let janitor_cycles_total = IntCounter::new(
            "detritus_janitor_cycles_total",
            "Retention janitor cycles completed.",
        )?;
        let janitor_blobs_freed_total = IntCounter::new(
            "detritus_janitor_blobs_freed_total",
            "Crash blobs removed by the janitor.",
        )?;
        let janitor_bytes_freed_total = IntCounter::new(
            "detritus_janitor_bytes_freed_total",
            "Bytes removed by the janitor.",
        )?;
        let janitor_cycle_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "detritus_janitor_cycle_duration_seconds",
                "Retention janitor cycle duration.",
            ),
            &["result"],
        )?;
        let janitor_last_logs_deleted = IntGauge::new(
            "detritus_janitor_last_logs_deleted",
            "NDJSON files removed in the last janitor cycle.",
        )?;
        let janitor_last_indexes_deleted = IntGauge::new(
            "detritus_janitor_last_indexes_deleted",
            "Crash index files removed in the last janitor cycle.",
        )?;
        let janitor_last_blobs_deleted = IntGauge::new(
            "detritus_janitor_last_blobs_deleted",
            "Crash blobs removed in the last janitor cycle.",
        )?;

        registry.register(Box::new(requests_total.clone()))?;
        registry.register(Box::new(request_duration_seconds.clone()))?;
        registry.register(Box::new(bytes_ingested_total.clone()))?;
        registry.register(Box::new(dedup_hits_total.clone()))?;
        registry.register(Box::new(writer_queue_depth.clone()))?;
        registry.register(Box::new(janitor_cycles_total.clone()))?;
        registry.register(Box::new(janitor_blobs_freed_total.clone()))?;
        registry.register(Box::new(janitor_bytes_freed_total.clone()))?;
        registry.register(Box::new(janitor_cycle_duration_seconds.clone()))?;
        registry.register(Box::new(janitor_last_logs_deleted.clone()))?;
        registry.register(Box::new(janitor_last_indexes_deleted.clone()))?;
        registry.register(Box::new(janitor_last_blobs_deleted.clone()))?;

        writer_queue_depth
            .with_label_values(&["none"])
            .set(0);

        Ok(Self {
            inner: Arc::new(MetricsInner {
                registry,
                requests_total,
                request_duration_seconds,
                bytes_ingested_total,
                dedup_hits_total,
                writer_queue_depth,
                janitor_cycles_total,
                janitor_blobs_freed_total,
                janitor_bytes_freed_total,
                janitor_cycle_duration_seconds,
                janitor_last_logs_deleted,
                janitor_last_indexes_deleted,
                janitor_last_blobs_deleted,
            }),
        })
    }

    pub fn observe_request(&self, endpoint: &str, status: &str, duration: Duration) {
        self.inner
            .requests_total
            .with_label_values(&[endpoint, status])
            .inc();
        self.inner
            .request_duration_seconds
            .with_label_values(&[endpoint])
            .observe(duration.as_secs_f64());
    }

    pub fn observe_bytes(&self, endpoint: &str, bytes: u64) {
        self.inner
            .bytes_ingested_total
            .with_label_values(&[endpoint])
            .inc_by(bytes);
    }

    pub fn observe_dedup_hit(&self) {
        self.inner.dedup_hits_total.inc();
    }

    pub fn set_writer_queue_depth(&self, source_id: &str, depth: i64) {
        self.inner
            .writer_queue_depth
            .with_label_values(&[source_id])
            .set(depth);
    }

    pub fn observe_janitor(&self, result: &str, duration: Duration, stats: &JanitorMetricStats) {
        self.inner.janitor_cycles_total.inc();
        self.inner
            .janitor_blobs_freed_total
            .inc_by(stats.blobs_deleted);
        self.inner
            .janitor_bytes_freed_total
            .inc_by(stats.bytes_freed);
        self.inner
            .janitor_cycle_duration_seconds
            .with_label_values(&[result])
            .observe(duration.as_secs_f64());
        self.inner
            .janitor_last_logs_deleted
            .set(stats.logs_deleted as i64);
        self.inner
            .janitor_last_indexes_deleted
            .set(stats.indexes_deleted as i64);
        self.inner
            .janitor_last_blobs_deleted
            .set(stats.blobs_deleted as i64);
    }

    pub fn render(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let families = self.inner.registry.gather();
        let mut bytes = Vec::new();
        encoder.encode(&families, &mut bytes)?;
        let mut body = String::from_utf8(bytes).unwrap_or_default();
        body.push_str("# EOF\n");
        Ok(body)
    }
}

#[derive(Debug, Default)]
pub struct JanitorMetricStats {
    pub logs_deleted: u64,
    pub indexes_deleted: u64,
    pub blobs_deleted: u64,
    pub bytes_freed: u64,
}
