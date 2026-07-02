use clap::Parser;

mod cli;
mod commands;
mod repl;

use cli::Cli;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rspark=debug")),
        )
        .compact()
        .init();
    let cli = Cli::parse();
    if let Err(err) = commands::dispatch(cli).await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
