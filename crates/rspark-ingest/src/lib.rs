//! rspark-ingest library — exposes `run()` so the CLI can launch the
//! ingest service from a `rspark ingest` subcommand without going
//! through a separate binary.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

#[derive(Clone)]
struct AppState {
    producer: Arc<FutureProducer>,
    topic: String,
}

#[derive(Debug, Deserialize)]
struct IngestBody {
    events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct IngestResponse {
    received: usize,
    produced: usize,
    failed: usize,
}

/// Run the ingest server. Reads `RSPORT_INGEST`, `KAFKA_BROKERS`, and
/// `KAFKA_TOPIC` from the environment. Returns when the listener
/// stops (e.g. Ctrl-C).
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let brokers = std::env::var("KAFKA_BROKERS")
        .unwrap_or_else(|_| "kafka.rspark.svc.cluster.local:9092".into());
    let topic = std::env::var("KAFKA_TOPIC").unwrap_or_else(|_| "rspark.page_events".into());
    let bind = std::env::var("RSPORT_INGEST").unwrap_or_else(|_| "0.0.0.0:8081".into());

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("message.timeout.ms", "5000")
        .set("compression.type", "lz4")
        .set("acks", "1")
        .create()?;
    let state = AppState {
        producer: Arc::new(producer),
        topic: topic.clone(),
    };
    info!(%brokers, topic = %state.topic, %bind, "rspark-ingest starting");

    let app = Router::new()
        .route("/v1/events", post(ingest))
        .route("/healthz", get(healthz))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

async fn ingest(
    State(state): State<AppState>,
    Json(body): Json<IngestBody>,
) -> axum::response::Response {
    let n = body.events.len();
    let mut produced = 0usize;
    let mut failed = 0usize;
    for ev in body.events {
        let bytes = match serde_json::to_vec(&ev) {
            Ok(b) => b,
            Err(e) => {
                warn!("encode failed: {e}");
                failed += 1;
                continue;
            }
        };
        let rec: FutureRecord<'_, str, [u8]> = FutureRecord::to(&state.topic).payload(&bytes);
        match state
            .producer
            .send(rec, std::time::Duration::from_secs(5))
            .await
        {
            Ok(_) => produced += 1,
            Err((e, _msg)) => {
                warn!("produce failed: {e}");
                failed += 1;
            }
        }
    }
    info!(received = n, produced, failed, "batch");
    if failed > 0 && produced == 0 {
        error!("all events in batch failed");
        return (
            StatusCode::BAD_GATEWAY,
            Json(IngestResponse {
                received: n,
                produced,
                failed,
            }),
        )
            .into_response();
    }
    (
        StatusCode::ACCEPTED,
        Json(IngestResponse {
            received: n,
            produced,
            failed,
        }),
    )
        .into_response()
}
