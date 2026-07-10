use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rspark",
    version,
    about = "A small Spark-compatible engine in Rust"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the master node (HTTP API + dashboard).
    Master {
        #[arg(long, default_value = "0.0.0.0:7077")]
        api_addr: String,
        #[arg(long, default_value = "0.0.0.0:8080")]
        dashboard_addr: String,
        #[arg(long, default_value_t = default_master_id())]
        master_id: String,
        /// Pre-load tables from CSV/JSON files at startup. Format: name=path
        /// (one per --load flag, e.g. --load employees=examples/data/employees.csv).
        #[arg(long = "load", value_name = "NAME=PATH")]
        load: Vec<String>,
        /// Pre-load the bundled example datasets (employees, sales, events).
        #[arg(long)]
        examples: bool,
    },
    /// Start a worker node that connects to the master.
    Worker {
        #[arg(long, default_value = "http://127.0.0.1:7077")]
        master: String,
        #[arg(long, default_value = "0.0.0.0:0")]
        bind: String,
        #[arg(long, default_value_t = num_cpus())]
        cores: usize,
        #[arg(long, default_value_t = 1024)]
        memory_mb: usize,
    },
    /// Execute a SQL statement locally and print the result.
    Sql {
        #[arg(long)]
        file: Option<String>,
        /// Inline SQL to run when --file is not given.
        sql: Option<String>,
        #[arg(long, default_value = "csv")]
        input_format: String,
        #[arg(long)]
        input: Vec<String>,
        #[arg(long)]
        catalog: Option<String>,
        #[arg(long)]
        output: Option<String>,
    },
    /// Submit a SQL file to the cluster.
    Submit {
        #[arg(long)]
        master: Option<String>,
        #[arg(long)]
        file: String,
        #[arg(long, default_value = "batch-job")]
        name: String,
        #[arg(long, default_value_t = 1)]
        parallelism: usize,
    },
    /// Start an interactive REPL.
    Shell {
        #[arg(long, default_value = "csv")]
        input_format: String,
        #[arg(long)]
        input: Vec<String>,
    },
    /// Run the dashboard server alone.
    Dashboard {
        #[arg(long, default_value = "0.0.0.0:8080")]
        addr: String,
        #[arg(long, default_value = "http://127.0.0.1:7077")]
        master: Option<String>,
    },
    /// Run the page-event ingest backend (receives events from the
    /// rspark-tracker extension and produces them to Kafka).
    Ingest {
        #[arg(long, default_value = "0.0.0.0:8081")]
        addr: String,
        #[arg(long, default_value = "kafka.rspark.svc.cluster.local:9092")]
        brokers: String,
        #[arg(long, default_value = "rspark.page_events")]
        topic: String,
    },
}

fn default_master_id() -> String {
    format!(
        "master-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0")
    )
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
