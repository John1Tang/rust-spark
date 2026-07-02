use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use rspark_api::routes::ApiState;
use rspark_api::routes::build_router;
use rspark_cluster::master::Master;
use rspark_sql::SessionState;
use rspark_storage::SourceRegistry;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub async fn run_dashboard(
    addr: SocketAddr,
    master: Arc<Master>,
    catalog: Arc<SessionState>,
) -> std::io::Result<()> {
    let registry = Arc::new(SourceRegistry::with_defaults());
    let api_state = ApiState::new(master, catalog, registry);
    let api_router = build_router(api_state);
    let app = Router::new()
        .merge(api_router)
        .fallback(dashboard_fallback)
        .layer(CorsLayer::permissive());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "rspark dashboard listening on http://{addr}");
    axum::serve(listener, app).await
}

async fn dashboard_fallback() -> Response {
    let html = crate::render_dashboard();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use crate::render_dashboard;

    #[test]
    fn dashboard_renders_html() {
        let html = render_dashboard();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("rspark dashboard"));
        assert!(html.contains("/v1/cluster/snapshot"));
    }
}