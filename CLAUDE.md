# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A small, embeddable Rust re-implementation of Apache Spark's local + distributed SQL execution model. Status: learning project. Single-master cluster, no shuffles, no UDFs, no streaming. The local CLI is solid; the cluster mode is a thin shim over the same executor that the CLI uses. The dashboard's `/v1/sql` actually executes SQL in the master's process (in-memory state) — it does not dispatch to workers.

## Build, test, lint

```bash
# Build the whole workspace
cargo build --workspace

# Run all tests (unit + integration). Expect 36 passing.
cargo test --workspace

# Run a single test
cargo test -p rspark-exec test::aggregate_count_group_by
cargo test -p rspark-sql --lib planner::tests::plan_with_filter_and_groupby

# Run integration tests only
cargo test -p rspark-exec --test integration

# Lint (the CI gate is lib + bins only, not tests)
cargo fmt --all
cargo clippy --workspace --lib --bins -- -D warnings

# Build the release image (used by deploy.sh)
docker build -f docker/Dockerfile -t rspark:latest .
```

CI runs `cargo fmt --check`, `cargo clippy --workspace --lib --bins -- -D warnings`, `cargo build --workspace --all-targets`, and `cargo test --workspace --all-targets --no-fail-fast` on Linux + macOS for every PR. See `.github/workflows/ci.yml`.

## Deploy loop (the per-feature script)

`scripts/deploy.sh` is the loop you run after every change:

```bash
./scripts/deploy.sh             # build rspark:latest, k3d image import, kubectl rollout restart
./scripts/port-forward.sh       # dashboard at http://127.0.0.1:8088, API at :7077
./scripts/sql.sh "..."          # curl wrapper around /v1/sql
./scripts/cluster-up.sh         # idempotent: create the k3d cluster if missing
./scripts/seed-mock-data.sh     # idempotent: upload fixtures to MinIO, register batch tables in the catalog, run the clickstream pipeline so `click_events` is `kind: streaming_table`
```

`scripts/port-forward.sh` forwards `8088:8080` (not `8080:8080`) because Docker Desktop's IPv6 listener on `[::1]:8080` would otherwise intercept browser requests and return 404. The k3d cluster itself still publishes port 8080 on the dashboard Service; only the host-side port-forward has moved.

`deploy.sh` uses `kubectl rollout restart` (not `kubectl set image`) because the image tag stays at `:latest` and only the digest changes — `set image` is a no-op in that case. The master Deployment uses `maxSurge: 1, maxUnavailable: 0` for zero-downtime rolling updates. The same `rspark:latest` image serves both the master and the worker — the subcommand (`master` / `worker`) is decided at runtime via CLI args.

`imagePullPolicy: Never` in the k8s manifests is required because the image is loaded by the deploy script via `k3d image import` (not pulled from a registry). If you change the pull policy, the rolling update will hang waiting for an image that no registry has.

## Architecture at a glance

A SQL string flows through this pipeline:

```
String
  → rspark_sql::parser::parse_sql        (sqlparser-rs AST)
  → rspark_sql::planner::Planner.plan_sql
       builds LogicalPlan tree:
       Scan | Project | Filter | Aggregate | Sort | Limit | Join
       | Union | Distinct | Empty
  → rspark_exec::LocalExecutor.execute
       materialize_input (recursive for nested Joins/Unions)
       apply_tree (per-node match: Project → project_record, Filter → eval_predicate,
                   Aggregate → aggregate_batch, etc.)
  → RecordBatch
```

The same executor runs both in the CLI (`rspark-cli sql`) and inside the master's `/v1/sql` handler. Workers use it via `Worker::execute_task`. Cluster mode does not shuffle — workers run a single task that re-executes the whole plan against the same data; the master is the source of truth for job/stage/task state.

The dashboard HTML is built inline in `crates/rspark-dashboard/src/ui.rs` as a `const &str` (no build step, no asset pipeline). The JavaScript is hand-written and self-contained — no frameworks.

## Crate map

- `rspark-core` — `Value`, `Schema`, `Field`, `DataType`, `Record`, `RecordBatch`, `Expr`. `Expr` is the AST used by both the planner and the executor. Has its own `regex` dep for `LIKE`.
- `rspark-storage` — `DataSource` trait + `CsvSource` / `JsonSource` (NDJSON) implementations + `OutputWriter` (table renderer + CSV writer). The dashboard pulls autocomplete suggestions from the catalog, not from here.
- `rspark-sql` — parser, logical planner, `SessionState` (in-memory catalog = `HashMap<String, (path, source, schema)>`), and `try_show_create` / `render_create_table` for the `SHOW CREATE TABLE` shortcut.
- `rspark-exec` — `LocalExecutor` (the pipeline driver) and the physical operator functions (`project_record`, `aggregate_batch`, `join_batches`, …). `Executor::execute` returns a `RecordBatch`. The integration tests in `crates/rspark-exec/tests/integration.rs` exercise the planner→executor round-trip.
- `rspark-cluster` — `Master` (state machine: `submit_job` → plan → `pop_pending_task` → worker polls) and `Worker` (HTTP loop: register, poll for tasks, execute, report). State is in `ClusterState` behind `parking_lot::RwLock` and is per-master-pod (not shared between pods — that's why the master Deployment is `replicas: 1`).
- `rspark-api` — `axum` router. The interesting handler is `POST /v1/sql` in `routes.rs`: it intercepts `SHOW CREATE TABLE` first, then plans + executes the query, updates the in-memory job state, and returns `{columns, rows, row_count, duration_ms, job}`. Catalog endpoints: `GET/POST/DELETE /v1/catalog/tables`, `GET /v1/catalog/suggestions`.
- `rspark-dashboard` — self-contained HTML/JS dashboard. Single source file: `src/ui.rs` (the `DASHBOARD_HTML` constant). Two tabs: `SQL Lab` (editor + result + metrics) and `Cluster` (workers / jobs / stages / tasks). SQL Lab has inline autocomplete (`updateAutocomplete` + `currentToken` + `positionPopup`) and an **Examples** section between the editor and the metrics strip — pill buttons that load preset queries into the editor (the streaming-table pills use the `.example-stream` class to render in blue).
- `rspark-cli` — clap subcommands. `master` and `worker` are the two halves of the cluster; `sql` and `shell` are the local execution paths; `submit` posts a job to the master; `dashboard` runs a dashboard-only server that fetches state from a remote master URL.

## Conventions and gotchas

- **Match on enums exhaustively.** The `Expr` and `LogicalPlan` enums are the type system's way of forcing you to handle every SQL feature. When you add a variant, every `match` over that type becomes a compile error pointing at the new arm.
- **Aliased aggregates.** `SELECT AVG(salary) AS avg_sal FROM …` parses to `Aliased { expr: Aggregate, alias }`. `aggregate_batch` and `project_record` both have a `match inner.as_ref() { Aggregate { .. } => … }` arm to unwrap the alias before doing the work. If you add a new expression wrapper (e.g. a `CASE`), add the unwrap there too.
- **`HAVING` references projected aliases, not raw aggregates.** The planner rewrites `HAVING AVG(salary) > 80000` to reference the projected column `avg_sal` via `rewrite_having` in `planner.rs`. The Aggregate's output schema is built from `aggregate_exprs` (which is the projected slice), so its column names use the alias when present.
- **`SELECT *` and `Star` expansion.** `build_project_schema` in `rspark-sql/src/plan.rs` expands `Expr::Star` into the input plan's output schema fields. The executor's `project_record` has a special case for `[Star]` (just pass through the records). Adding `Star` to anything other than a top-level projection requires re-thinking.
- **In-memory cluster state is per-pod.** Two master pods each have their own `ClusterState`; the `rspark-master` Service round-robins between them, so workers and `/v1/cluster/snapshot` would see inconsistent state. The Deployment is `replicas: 1` for that reason — comment in `k8s/11-master-deployment.yaml` explains.
- **Worker registration is once-only.** `run_worker` in `crates/rspark-cli/src/commands.rs` does one HTTP `POST /v1/workers` at startup, then enters a poll loop. If the master rolls, the worker's registration dies and it doesn't re-register. A `rollout restart` of the workers after a master rollout is the workaround. (Filed in the codebase as a known limitation.)
- **Dashboard autocomplete popup.** A live `positionPopup()` walks back from the cursor to find the current token, then renders matches as a `<div class="autocomplete-popup">` absolutely positioned under the textarea. The `mirror` `<span>` is a hidden element used to compute caret pixel position from line metrics. The popup must be `display: block` (not just have content) — there was a bug where HTML was built correctly but the popup stayed hidden.
- **SQL Lab "no matches" hint** uses the same popup with a different inner body. The `min length 1` filter is intentional — single-character matches would always be huge for the keyword list.
- **Examples section ordering.** The Examples pill row lives between the SQL editor and the Execution metrics strip in `DASHBOARD_HTML`. If you move it, the visual "click to load, then Ctrl+Enter to run" rhythm breaks (the pills feel like they belong to the editor). The two streaming-table pills (`stream × batch join`, `page views / signup country`) only work after `./scripts/seed-mock-data.sh` has registered `click_events` as `kind: streaming_table` — without it, the catalog reports `NotFound("table 'click_events' not found")`.
- **Catalog `kind` matters for re-registration.** `POST /v1/catalog/tables` now takes an optional `kind` field (`batch` | `streaming_table` | `materialized_view`). The seed script re-uses this to re-point `click_events` back at the raw NDJSON (the pipeline output is pipe-delimited; the CsvSource uses comma) **without** demoting it to batch. Forgetting `kind` on the re-registration silently drops the streaming-table badge from autocomplete.

## Adding a SQL feature — the file tour

A new SQL feature (new function, new operator, new clause) typically touches:

1. `crates/rspark-sql/src/expr_builder.rs` — translate the `sqlparser::ast` node into an `Expr` variant (or extend an existing one).
2. `crates/rspark-core/src/expr.rs` — add the `Expr` variant (or a new field), implement `eval`, `display_name`, `collect_columns`, `contains_aggregate`. `eval` is where type-mismatch errors live.
3. `crates/rspark-sql/src/plan.rs` — if it adds a new `LogicalPlan` variant, add an arm to `lower_plan`; if it changes the schema inference, update `build_*_schema` and `infer_data_type`.
4. `crates/rspark-exec/src/operators.rs` — for a new operator (e.g. window function), add the physical function and a new `LogicalPlan` arm in `apply_tree` in `executor.rs`.
5. `crates/rspark-exec/tests/integration.rs` — end-to-end test that runs the plan through the executor. This is the regression net.
6. `crates/rspark-sql/src/show_create.rs` — if it changes the schema, `render_create_table` may need to know the new type's SQL spelling.
7. `crates/rspark-api/src/routes.rs` — the `SQL_KEYWORDS` / `SQL_FUNCTIONS` constants in `routes.rs` should be updated so the dashboard's autocomplete picks it up.

For DDL other than `SHOW CREATE TABLE`, intercept in `execute_sql` (see the `try_show_create` pattern) — `Master::submit_job` runs the planner, so a statement that the planner doesn't understand will 500.

## Tests

- 36 tests total: 3 `rspark-core`, 7 `rspark-sql`, 6 `rspark-exec` (unit), 2 `rspark-cluster`, 1 `rspark-api`, 1 `rspark-dashboard`, 6 `rspark-exec` (integration in `tests/integration.rs`).
- The integration tests in `crates/rspark-exec/tests/integration.rs` are the best place to add tests for new SQL features — they exercise the full planner→executor round-trip with real data files from `examples/data/`.
- For pure planner tests, look at `planner::tests::*` in `rspark-sql/src/planner.rs`.

## Things that will trip you up

- **The CI uses `--cap-lints warn` in spirit** — the actual command is `cargo clippy --workspace --lib --bins -- -D warnings`. Warnings in `tests/` and `examples/` don't fail the build. Don't add `-D warnings` to the test build.
- **Edition 2024 deps need rustc ≥ 1.86.** The `docker/Dockerfile` uses `rust:1.86-slim`. Bump in lockstep if Rust deps force a newer MSRV.
- **Cargo.lock is committed.** Single binary, no surprises from dep updates.
- **`--record` is deprecated** in `kubectl set image`. The deploy script uses `kubectl rollout restart` instead, which doesn't need it.
- **The K3s cluster expects k3d, not raw k3s.** The deploy script uses `k3d image import` and `imagePullPolicy: Never`. On a real k3s cluster, set `imagePullPolicy: Always` and push to a registry.
- **`ROUND` is not implemented** — only the four aggregates in the SQL functions list. If you add one, the autocomplete list in `routes.rs` needs the new name too.
- **Apache-2.0 license is enforced.** New files need to be compatible. The `NOTICE` file lists third-party deps; if you add a new one with a non-standard license, update it.

## Mock data

`examples/data/` has `employees.csv` (20 rows, 6 cols), `sales.csv` (20 rows, 5 cols, with shared IDs to make JOIN meaningful), `users.csv` (200 rows), `orders.csv` (400 rows), and `clickstream.jsonl` (NDJSON, 1500 events; 23 anonymous `user_id`s exercise the LEFT JOIN null branch). `examples/demo.sh` is a 12-query tour of the SQL surface. The Dockerfile `COPY`s the examples into `/app/examples` so the master has them baked in. The same data is also in `k8s/01-configmap.yaml` as a backup if you ever want to mount a ConfigMap instead. After a rolling restart wipes the in-memory catalog, re-run `./scripts/seed-mock-data.sh` to re-register the batch tables in the catalog and re-flip `click_events` to `kind: streaming_table`.
