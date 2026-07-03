# Architecture

A 5-minute tour of how the codebase fits together.

## The pipeline

```
SQL string
  → rspark_sql::parser::parse_sql              sqlparser-rs AST
  → rspark_sql::planner::Planner.plan_sql       builds LogicalPlan
  → rspark_exec::LocalExecutor::execute         applies operators row-by-row
  → RecordBatch
```

The same executor runs in the CLI and inside the master's `/v1/sql`
handler. Cluster workers use the same executor via
`Worker::execute_task`. Cluster mode doesn't shuffle — workers re-run the
whole plan against the same data the master saw.

## Crate layout

```
crates/
├── rspark-core        — Value, Schema, Record, RecordBatch, Expr
├── rspark-storage     — DataSource trait + CSV / JSON (NDJSON) readers
├── rspark-sql         — sqlparser-rs AST → LogicalPlan builder
├── rspark-exec        — PhysicalOp algebra + LocalExecutor pipeline
├── rspark-cluster     — Master / Worker state machine + HTTP polling
├── rspark-api         — axum HTTP API (cluster control + SQL exec)
├── rspark-dashboard   — self-contained HTML/JS dashboard
├── rspark-cli         — clap subcommands
└── rspark-operator    — kube-rs controller for the SparkCluster CRD
```

The rspark crates own the data path (SQL → executor → record batch).
The k3d cluster holds the runtime; the rspark-operator owns the
lifecycle of the master/worker as a single SparkCluster CR. The rspark
dashboard is the SQL-facing UI; Headlamp (under `k8s/headlamp/`) is the
cluster-facing UI (CRD inspector, pod logs, logins).

### rspark-core

The vocabulary of the engine. `Value` is a tagged enum
(Null / Boolean / Int32 / Int64 / Float32 / Float64 / String); `Schema`
is a `Vec<Field>`; `RecordBatch` is `Schema + Vec<Record>`. `Expr` is the
AST used by both the planner and executor.

This crate has its own `regex` dep for `LIKE`.

### rspark-storage

`DataSource` trait (`infer_schema` + `scan`) plus CSV and JSON (NDJSON)
implementations. Each `DataSource::scan(path, Option<&Schema>)` returns a
`RecordBatch`. `OutputWriter` renders a batch as a table or writes CSV.

CSV source uses `zip(headers.iter(), effective_schema.fields())` — the
schema controls type coercion, but only as many fields are emitted as
the schema declares. Pass the right schema or you get truncated rows.

### rspark-sql

`planner::Planner::plan_sql(sql, catalog)` is the entry point. It
delegates to `parser::parse_sql` (sqlparser-rs) then walks the AST.

The planner builds a `LogicalPlan` tree of: `Scan | Project | Filter
| Aggregate | Sort | Limit | Join | Union | Distinct | Empty`. Each
variant carries its own `schema: Schema` so we never have to re-infer.

`SessionState` is the in-memory catalog: `RwLock<HashMap<name,
(path, source_format, schema)>>`. `try_show_create` + `render_create_table`
short-circuit `SHOW CREATE TABLE` so the planner doesn't have to know
about DDL.

The `Expr` AST has these variants (each `match` is exhaustive — adding
one forces compile errors everywhere):

- `Column(String)`
- `Literal(Literal)`
- `Binary { op, left, right }`
- `Not(Box<Expr>)`
- `IsNull / IsNotNull`
- `If { cond, then_expr, else_expr }`
- `Aggregate { func, arg, distinct }`
- `Aliased { expr, alias }`
- `Star`

### rspark-exec

The driver. `LocalExecutor::execute(plan)` calls
`materialize_input(plan)` (which recursively executes nested Joins/Unions),
then `apply_tree(plan, batch, op, ctx)` which is one big `match` on
each `LogicalPlan` variant that mutates the batch in place.

`aggregate_batch` is the most interesting operator: it groups records by
the group keys, accumulates per group, then produces one row per group
in BTreeMap order (deterministic output). `MIN`/`MAX` use the existing
`Value::try_cmp`.

`project_record` does per-row evaluation; it has special-case handling
for `Expr::Aliased { expr: Aggregate, alias }` so aliased aggregates
look up the precomputed value by display name (e.g. `avg_sal`) in the
input batch rather than trying to re-evaluate the aggregate.

### rspark-cluster

The state machine. `Master::submit_job` calls `Planner::plan_sql`,
splits the plan into partitions (currently one partition per worker),
creates a `Stage` with `Task`s, and stores everything in `ClusterState`.

`Worker::execute_task` runs the same executor as the CLI. Workers
register once at startup and then poll `/v1/workers/{id}/task` forever.
They do **not** re-register after a master rolling restart — known
limitation, see `docs/operator.md`.

`ClusterState` is per-master-pod in memory (parking_lot RwLock). This
is why `k8s/11-master-deployment.yaml` has `replicas: 1` and why
`MasterSpec.replicas` defaults to 1 in the CRD.

### rspark-api

The axum router. The interesting endpoint is `POST /v1/sql`:

1. Try `try_show_create(sql)` — if it's `SHOW CREATE TABLE foo`, render
   the DDL directly and return it as a one-row result.
2. Submit a `JobRequest` to the master (records in cluster state, increments
   round counter).
3. Plan the SQL via `Planner`.
4. Execute via `LocalExecutor` against an `ExecutionContext` that has the
   master's `SourceRegistry`.
5. Build the JSON response: `{columns, rows, row_count, duration_ms, job}`.

Catalog endpoints: `GET/POST/DELETE /v1/catalog/tables`,
`GET /v1/catalog/suggestions`.

### rspark-dashboard

`crates/rspark-dashboard/src/ui.rs` is the entire UI — a `const &str` of
HTML + JS + CSS, served by `axum`'s fallback handler when no API route
matches. No bundler, no framework.

The JS has three notable pieces:
- `runSql()` posts to `/v1/sql` and renders the result.
- `updateAutocomplete()` shows a popup with table/column/function/keyword
  matches under the caret as you type.
- `refresh()` polls `/v1/cluster/snapshot` every 1.5s.

### rspark-cli

The clap subcommands:
- `master` — runs the API + dashboard concurrently.
- `worker` — runs the HTTP poll loop.
- `sql` — runs a single SQL statement and prints the result.
- `submit` — POSTs a job to the master.
- `shell` — interactive REPL.
- `dashboard` — runs a dashboard-only server that proxies state from a
  remote master URL.

### rspark-operator

`SparkCluster` is a namespaced CRD (`spark.rspark.io/v1alpha1`). The
controller is a single binary using kube-rs. It owns (with
`ownerReferences`) the ServiceAccount, master Service + ConfigMap +
Deployment, worker Deployment, and PodDisruptionBudgets.

The reconciler is narrow and idempotent: every child object is
server-side-patched via `Patch::Apply`, and 404 falls through to create.

## Cross-cutting concerns

### Logging

`tracing` everywhere. The CLI sets `RUST_LOG` from `RSPARK_LOG`. The
operator uses `tracing-subscriber` with `EnvFilter::try_from_default_env`.

### Errors

`rspark_core::error::Error` is the canonical enum:
- `Sql(String)`
- `Storage(String)`
- `Type(String)`
- `Schema(String)`
- `Network(String)`
- `Cluster(String)`
- `NotFound(String)`
- `InvalidState(String)`

It implements `std::error::Error` (via `thiserror`). The API serializes
errors as `{error: String, kind: String}` so the dashboard can show
both the message and the variant name.

### Testing

- Unit tests live next to the code (`#[cfg(test)] mod tests`).
- Integration tests for the executor live in
  `crates/rspark-exec/tests/integration.rs`. They plan + execute real
  SQL against the bundled example data.
- The operator's CRD is tested in `crates/rspark-operator/tests/crd.rs`.

### Build / release

The workspace uses resolver = "2" and edition = "2021". `Cargo.lock` is
committed. The release profile uses `lto = "thin"` to keep binaries
small.