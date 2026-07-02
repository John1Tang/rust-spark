//! HTTP API for the rspark master and worker nodes.
//!
//! Routes are intentionally REST-flavored so workers can poll over plain HTTP.

pub mod routes;
pub mod server;

pub use routes::build_router;
pub use server::run_server;
