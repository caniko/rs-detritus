//! Install a Detritus tracing layer and emit a couple of log events.
//!
//! Run with:
//!
//!     cargo run --example install_layer -p detritus-client

use std::{path::PathBuf, time::Duration};

use detritus::{Layer, SourceId};
use secrecy::SecretString;
use tracing_subscriber::{Registry, layer::SubscriberExt};
use url::Url;
use uuid::Uuid;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = SourceId {
        project: "detritus-example".to_owned(),
        platform: "linux".to_owned(),
        version: "0.1.0".to_owned(),
        install_id: Uuid::nil(),
    };

    let queue_dir = std::env::temp_dir().join("detritus-example-spool");
    let layer = Layer::builder()
        .endpoint(Url::parse("http://127.0.0.1:4317")?)
        .token(SecretString::from("dev-token"))
        .source(source)
        .queue_dir(PathBuf::from(&queue_dir))
        .flush_interval(Duration::from_secs(60))
        .flush_timeout(Duration::from_millis(50))
        .build()?;

    let subscriber = Registry::default().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("hello from detritus example");
        tracing::warn!(target = "example", "something to spool");
    });

    println!("queued example events under {}", queue_dir.display());
    Ok(())
}
