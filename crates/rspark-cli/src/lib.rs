//! Command-line entry point for rspark.
//!
//! Mirrors the most-used Spark CLIs:
//! * `rspark master`  — start the master + dashboard
//! * `rspark worker`  — start a worker node
//! * `rspark sql`     — single-shot SQL execution (local or via cluster)
//! * `rspark submit`  — submit a SQL file as a batch job
//! * `rspark shell`   — REPL for interactive queries
//! * `rspark dashboard` — run the dashboard alone

pub mod cli;
pub mod commands;
pub mod repl;

pub use cli::Cli;
