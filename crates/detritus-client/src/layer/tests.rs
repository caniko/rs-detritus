use super::*;
use tempfile::TempDir;
use tracing_subscriber::prelude::*;

fn source() -> SourceId {
    SourceId {
        project: "tests".into(),
        platform: "linux".into(),
        version: "1".into(),
        install_id: uuid::Uuid::nil(),
    }
}

fn worker(dir: &std::path::Path) -> (Layer, Worker) {
    let (sender, receiver) = mpsc::channel(32);
    (
        Layer { sender },
        Worker {
            endpoint: "http://127.0.0.1:1".parse().unwrap(),
            token: SecretString::from("test-token"),
            source: source(),
            batch_size: 10,
            flush_interval: Duration::from_secs(60),
            flush_timeout: Duration::from_millis(50),
            queue_dir: dir.to_owned(),
            sample_rate: 1.0,
            receiver,
        },
    )
}

#[tokio::test]
async fn dropping_last_layer_flushes_and_stops_worker() {
    let dir = TempDir::new().unwrap();
    let (layer, worker) = worker(dir.path());
    let handle = tokio::spawn(worker.run());
    layer
        .sender
        .send(WorkerMessage::Record(LogRecord {
            time_unix_nano: 42,
            ..Default::default()
        }))
        .await
        .unwrap();
    drop(layer);
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("worker must terminate")
        .unwrap();
    let paths = spool::pending_log_batches(dir.path()).unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(
        spool::read_log_batch(&paths[0]).unwrap().resource_logs[0].scope_logs[0].log_records[0]
            .time_unix_nano,
        42
    );
}

#[tokio::test]
async fn flush_timeout_preserves_records_for_replay() {
    let dir = TempDir::new().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (layer, mut worker) = worker(dir.path());
    worker.endpoint = format!("http://{}", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let handle = tokio::spawn(worker.run());
    // Consume the initial empty interval tick before enqueueing the batch.
    layer.flush().await.unwrap();
    layer
        .sender
        .send(WorkerMessage::Record(LogRecord {
            time_unix_nano: 77,
            ..Default::default()
        }))
        .await
        .unwrap();
    assert!(matches!(layer.flush().await, Err(LayerError::Flush(_))));
    let paths = spool::pending_log_batches(dir.path()).unwrap();
    assert_eq!(
        paths.len(),
        1,
        "a timed-out network request must not discard the batch"
    );
    assert_eq!(
        spool::read_log_batch(&paths[0]).unwrap().resource_logs[0].scope_logs[0].log_records[0]
            .time_unix_nano,
        77
    );
    drop(layer);
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn builder_requires_each_configuration_field() {
    assert!(matches!(
        Layer::builder().build(),
        Err(LayerError::MissingEndpoint)
    ));
    let builder = Layer::builder().endpoint("http://localhost:4317".parse().unwrap());
    assert!(matches!(
        builder.clone().build(),
        Err(LayerError::MissingToken)
    ));
    let builder = builder.token(SecretString::from("token"));
    assert!(matches!(
        builder.clone().build(),
        Err(LayerError::MissingSource)
    ));
    let builder = builder.source(source());
    assert!(matches!(builder.build(), Err(LayerError::MissingQueueDir)));
}

#[tokio::test]
async fn stopped_worker_is_reported_to_flush_caller() {
    let dir = TempDir::new().unwrap();
    let (layer, worker) = worker(dir.path());
    drop(worker);
    assert!(matches!(
        layer.flush().await,
        Err(LayerError::WorkerStopped)
    ));
    let (layer, mut worker) = self::worker(dir.path());
    let handle = tokio::spawn(async move {
        let _ = worker.receiver.recv().await;
    });
    assert!(matches!(
        layer.flush().await,
        Err(LayerError::WorkerStopped)
    ));
    handle.await.unwrap();
}

#[test]
fn sampling_is_deterministic_at_bucket_boundaries() {
    let dir = TempDir::new().unwrap();
    let (_, mut worker) = worker(dir.path());
    for (rate, time, expected) in [
        (0.0, 0, false),
        (1.0, 9999, true),
        (0.5, 4999, true),
        (0.5, 5000, false),
        (0.5, 10000, true),
    ] {
        worker.sample_rate = rate;
        assert_eq!(
            worker.should_sample(&LogRecord {
                time_unix_nano: time,
                ..Default::default()
            }),
            expected
        );
    }
    assert_eq!(
        Layer::builder()
            .batch_size(0)
            .sample_rate(-1.0)
            .sample_rate(2.0)
            .batch_size,
        1
    );
    assert_eq!(Layer::builder().sample_rate(2.0).sample_rate, 1.0);
    assert_eq!(unix_nanos(UNIX_EPOCH - Duration::from_secs(1)), 0);
}

#[tokio::test]
async fn events_preserve_typed_fields_messages_and_all_severities() {
    let dir = TempDir::new().unwrap();
    let (layer, mut worker) = worker(dir.path());
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(message = "literal", signed = -7_i64, unsigned = 42_u64, ready = true, value = ?vec![1, 2]);
        tracing::warn!("formatted {}", 42);
        tracing::info!(answer = 42);
        tracing::debug!(message = "debug");
        tracing::trace!(message = "trace");
    });
    for (index, severity) in [
        SeverityNumber::Error,
        SeverityNumber::Warn,
        SeverityNumber::Info,
        SeverityNumber::Debug,
        SeverityNumber::Trace,
    ]
    .into_iter()
    .enumerate()
    {
        let WorkerMessage::Record(record) = worker.receiver.recv().await.unwrap() else {
            panic!("record")
        };
        assert_eq!(record.severity_number, severity as i32);
        assert_eq!(record.observed_time_unix_nano, record.time_unix_nano);
        assert!(record.time_unix_nano > 0);
        let body = record.body.unwrap().value.unwrap();
        if index == 0 {
            assert_eq!(body, any_value::Value::StringValue("literal".into()));
            for (key, value) in [
                ("signed", "-7"),
                ("unsigned", "42"),
                ("ready", "true"),
                ("value", "[1, 2]"),
            ] {
                assert!(record.attributes.contains(&string_attr(key, value)));
            }
        } else if index == 1 {
            assert_eq!(body, any_value::Value::StringValue("formatted 42".into()));
        } else if index == 2 {
            assert_eq!(body, any_value::Value::StringValue(record.event_name));
        }
    }
}

#[tokio::test]
async fn batch_size_and_periodic_flush_spool_offline_records() {
    let dir = TempDir::new().unwrap();
    let layer = Layer::builder()
        .endpoint("http://127.0.0.1:1".parse().unwrap())
        .token(SecretString::from("token"))
        .source(source())
        .queue_dir(dir.path().to_owned())
        .batch_size(2)
        .flush_interval(Duration::from_millis(20))
        .flush_timeout(Duration::from_secs(1))
        .sample_rate(1.0)
        .build()
        .unwrap();
    for _ in 0..3 {
        layer
            .sender
            .send(WorkerMessage::Record(LogRecord::default()))
            .await
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if spool::pending_log_batches(dir.path())
                .unwrap()
                .iter()
                .map(|path| {
                    spool::read_log_batch(path).unwrap().resource_logs[0].scope_logs[0]
                        .log_records
                        .len()
                })
                .sum::<usize>()
                == 3
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    layer.flush().await.unwrap();
}

#[tokio::test]
async fn corrupt_batches_survive_replay_and_unwritable_spool_is_reported() {
    let dir = TempDir::new().unwrap();
    let corrupt = dir.path().join("corrupt.protobuf");
    std::fs::write(&corrupt, [0xff]).unwrap();
    let (_, worker) = worker(dir.path());
    worker.drain_spooled().await;
    assert!(corrupt.exists());
    let file = dir.path().join("file");
    std::fs::write(&file, b"not a directory").unwrap();
    let (_, worker) = self::worker(&file);
    worker.drain_spooled().await;
    assert!(matches!(
        worker.export_records(vec![LogRecord::default()]).await,
        Err(LayerError::Flush(_))
    ));
}
