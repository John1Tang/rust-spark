use crate::routes::{build_router, ApiState};
use rspark_cluster::master::Master;
use rspark_cluster::state::ClusterState;
use rspark_sql::SessionState;
use rspark_storage::SourceRegistry;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub async fn run_server(
    addr: SocketAddr,
    master: Arc<Master>,
    catalog: Arc<SessionState>,
) -> std::io::Result<()> {
    let registry = Arc::new(SourceRegistry::with_defaults());
    let api_state = ApiState::new(master, catalog, registry);
    let app = build_router(api_state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());
    tracing::info!(%addr, "rspark master API listening");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await
}

/// Convenience constructor used by CLI to wire a complete server.
pub fn build_state(master_id: impl Into<String>) -> (ClusterState, Arc<Master>, Arc<SessionState>) {
    let state = ClusterState::new(master_id);
    let master = Arc::new(Master::new(state.clone()));
    let catalog = Arc::new(SessionState::new());
    (state, master, catalog)
}
