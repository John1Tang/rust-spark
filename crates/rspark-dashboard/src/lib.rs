//! Web dashboard for rspark: serves the cluster snapshot over HTTP and renders
//! a single-page UI with live-updating tables for jobs, stages, tasks,
//! workers, and the SQL currently running.

pub mod server;
pub mod ui;

pub use server::run_dashboard;
pub use ui::render_dashboard;
