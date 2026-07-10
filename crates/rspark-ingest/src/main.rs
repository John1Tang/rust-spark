//! Thin wrapper that lets the standalone `rspark-ingest` binary reuse
//! the same run loop as the `rspark ingest` CLI subcommand. The real
//! implementation lives in `lib.rs` so the CLI can embed it without
//! re-spawning a process.

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    rspark_ingest::run().await
}
