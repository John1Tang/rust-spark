pub mod controller;
pub mod crd;

use kube::Client;
use tracing_subscriber::{prelude::*, EnvFilter};

pub use crd::{Phase, SparkCluster, SparkClusterSpec, SparkClusterStatus};

/// Build a Kubernetes client from the cluster's in-cluster service
/// account token (the standard `KUBERNETES_SERVICE_HOST` / `KUBERNETES_PORT`
/// env vars) or, when running locally, the developer's `~/.kube/config`.
pub async fn client() -> anyhow::Result<Client> {
    let opts = kube::Config::infer().await?;
    Ok(Client::try_from(opts)?)
}

/// Install a tracing subscriber. Idempotent so tests can call it freely.
pub fn install_tracing() {
    let _ = tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,rspark_operator=debug")),
        )
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .try_init();
}
