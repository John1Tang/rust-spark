# rspark

A small, embeddable Rust re-implementation of Apache Spark's local + distributed SQL execution model.

> **Status:** learning project. Single-master cluster, no shuffles, no UDFs, no streaming. The local CLI is solid; the cluster mode is a thin shim over the same executor.

![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)
![Tests](https://img.shields.io/badge/tests-36_passing-brightgreen.svg)

## Highlights

- **Local mode** — run SQL against CSV/JSON files from the command line.
- **Cluster mode** — a master + worker pool that distributes task execution over HTTP.
- **Spark SQL surface** — `SELECT`, `WHERE`, `GROUP BY ... HAVING`, `ORDER BY`, `LIMIT`, `DISTINCT`, `JOIN`, `COUNT/SUM/AVG/MIN/MAX`, `LIKE`, `IN`, `BETWEEN`, aliases, nested expressions, `COALESCE` / `NVL`, `SHOW CREATE TABLE`.
- **Live dashboard** — a two-tab (SQL Lab / Cluster) UI with a SQL editor, live autocomplete (tables, columns, functions, keywords), result preview, and a real-time view of jobs, stages, tasks, and workers.
- **Container-ready** — Docker Compose and Kubernetes manifests in `docker/` and `k8s/`.

## Quick start (local)

```bash
# Run SQL over a CSV
cargo run -p rspark-cli -- sql --input examples/data/employees.csv \
    "SELECT dept, AVG(salary) AS avg_sal, COUNT(*) AS n FROM employees GROUP BY dept"

# Start an interactive REPL
cargo run -p rspark-cli -- shell --input examples/data/employees.csv

# Read SQL from a file and write to a CSV
cargo run -p rspark-cli -- sql \
    --input examples/data/employees.csv \
    --file query.sql \
    --output out.csv
```

## Dashboard (interactive SQL)

The dashboard serves a SQL editor backed by the master's catalog. It actually
executes the query (server-side, in-process) and renders the result table
inline, with timing, column types, and a 200-row preview.

```bash
# Pre-load the bundled mock data, then open http://127.0.0.1:8080
cargo run -p rspark-cli -- master --examples
```

The sidebar lets you register more tables (CSV/JSON paths) on the fly, the
SQL editor supports `Ctrl+Enter` to run, and the result panel surfaces
errors with their `Error` kind. A local-history list (8 most recent
queries) and one-click sample queries are included.

You can also bring your own tables at startup:

```bash
cargo run -p rspark-cli -- master \
    --load users=examples/data/employees.csv \
    --load orders=examples/data/sales.csv
```

## Cluster mode

In one terminal, start the master (API + dashboard):

```bash
cargo run -p rspark-cli -- master --api-addr 127.0.0.1:7077 --dashboard-addr 127.0.0.1:8080
```

Open `http://127.0.0.1:8080` for the live dashboard.

In one or more other terminals, start workers:

```bash
cargo run -p rspark-cli -- worker --master http://127.0.0.1:7077 --bind 127.0.0.1:9091 --cores 2 --memory-mb 1024
```

Submit a job to the cluster:

```bash
cargo run -p rspark-cli -- submit --master http://127.0.0.1:7077 --file query.sql --name my-job --parallelism 2
```

The `submit` command reads SQL from `--file` and the workers pull task definitions from the master.

## SQL examples

```sql
-- Aggregate + alias + filter + sort + limit
SELECT dept, AVG(salary) AS avg_sal
FROM employees
WHERE salary > 50000
GROUP BY dept
HAVING AVG(salary) > 60000
ORDER BY avg_sal DESC
LIMIT 5;

-- Distinct
SELECT DISTINCT dept FROM employees;

-- Join
SELECT e.name, s.product, s.amount
FROM employees e JOIN sales s ON e.id = s.id;

-- Pattern matching
SELECT name FROM employees WHERE name LIKE 'A%';
```

## Docker Compose

```bash
cd docker
docker compose up --build
```

This brings up:
- `master` — API on `localhost:7077`, dashboard on `http://localhost:8080`
- `worker-1`, `worker-2` — registered against the master

To submit a query once the cluster is up:

```bash
cargo run -p rspark-cli -- submit --master http://localhost:7077 \
    --file query.sql --name demo
```

## Kubernetes

```bash
# Build and load the image into your cluster
docker build -f docker/Dockerfile -t rspark:latest .
kind load docker-image rspark:latest   # or your registry of choice

kubectl apply -f k8s/
kubectl -n rspark get pods
```

Port-forward the dashboard:

```bash
kubectl -n rspark port-forward svc/rspark-master 8080:8080
```

## Architecture

```
crates/
├── rspark-core        Schema, Value, Record, RecordBatch, Expr
├── rspark-storage     DataSource trait + CSV / JSON readers, OutputWriter
├── rspark-sql         sqlparser-based parser → Expr → LogicalPlan builder
├── rspark-exec        PhysicalOp algebra + LocalExecutor pipeline driver
├── rspark-cluster     Master / Worker state machine + task scheduling
├── rspark-api         axum HTTP API for cluster control
├── rspark-dashboard   Self-contained HTML/JS dashboard
└── rspark-cli         clap subcommands: master, worker, sql, submit, shell, dashboard
```

A `LogicalPlan` is lowered to a `PhysicalOp` tree (`Scan / Project / Filter / Aggregate / Sort / Limit / Join`) and applied row-by-row over `RecordBatch`es in `rspark-exec::LocalExecutor::execute`. In cluster mode the master splits a job into tasks, hands each task to a worker, and the worker reuses the same executor pipeline locally.

## Testing

```bash
cargo test --workspace
```

21 unit tests + 6 integration tests covering scan, projection, filter, sort, limit, aggregate, join, and the planner→executor round-trip.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Issues and PRs welcome at
<https://github.com/John1Tang/rust-spark>.

## Security

See [SECURITY.md](SECURITY.md) for the disclosure policy.

## Code of conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).