# HTTP API

The master exposes a single JSON HTTP API on port 7077 (by default).
This page is the canonical reference for endpoints, request shapes, and
response codes.

## Conventions

- **Base URL**: `http://<master-host>:7077`. From inside the cluster:
  `http://rspark-master.<namespace>.svc.cluster.local:7077`.
- **Content type**: `application/json` for request and response bodies.
- **Errors**: `500 Internal Server Error` with body
  `{"error": "...", "kind": "VariantName"}`.
- **CORS**: the dashboard server adds a permissive `CorsLayer`. Direct
  API calls from browsers work; cookies and credentials are not used.

## Health & cluster

### `GET /health`

```json
{"now": "2026-07-03T05:11:21Z", "status": "ok"}
```

Returns `200`. The dashboard polls this on load.

### `GET /v1/cluster/snapshot`

Returns the entire cluster state — workers, jobs, stages, tasks,
metrics. Used by the dashboard.

```json
{
  "master_id": "master-demo-master-xxx",
  "captured_at": "2026-07-03T05:11:00Z",
  "workers": [
    {"id": "…", "address": "0.0.0.0:9090", "last_heartbeat": "…", "cores": 2, "memory_mb": 1024, "status": "Alive", "running_tasks": []}
  ],
  "jobs": [
    {"id": "…", "name": "dashboard", "sql": "SELECT …", "status": "Succeeded", "submitted_at": "…", "started_at": "…", "completed_at": "…", "stages": ["…"], "result_rows": 42, "error": null, "parallelism": 1}
  ],
  "stages": [...],
  "tasks": [...],
  "pending_queue": [],
  "running_round": 0,
  "total_completed_rounds": 0,
  "total_runs": 0
}
```

`running_round` and `total_completed_rounds` are rolling counters;
`total_runs` is monotonic.

## SQL

### `POST /v1/sql`

Execute a SQL statement against the master's catalog. Always runs
in-process on the master (cluster mode is a thin shim — workers are
only consulted for plans the master decided to shard).

**Request**:
```json
{"sql": "SELECT …", "job_name": "optional", "parallelism": 1}
```

**Response**:
```json
{
  "job": { /* full job object */ },
  "columns": [{"name": "dept", "data_type": "Int64"}, ...],
  "rows": [["Engineering", 92000.0, 9], ...],
  "row_count": 3,
  "duration_ms": 1
}
```

Errors:
- `500 NotFound` if a referenced table isn't in the catalog.
- `500 InvalidState` if the SQL fails to parse or plan.
- `500 Execution` if the executor fails (e.g. type mismatch).
- The body's `kind` field is the `Error` variant name.

For `SHOW CREATE TABLE foo`, the response has a single row in column
`create_statement` (String).

### `POST /v1/catalog/tables`

Register a table from a file path. The schema is inferred from the
file.

**Request**:
```json
{
  "name": "users",
  "path": "/data/users.csv",
  "source": "csv",
  "kind": "batch"
}
```

Fields:

- `name` (required) — catalog name. Re-registering with the same name
  overwrites the entry.
- `path` (required) — file path readable by the master. Can be a local
  path (e.g. `/app/examples/data/users.csv`) or an `s3://…` URI if the
  S3 source is registered.
- `source` (optional) — `csv` or `json`. Defaults from the file
  extension.
- `kind` (optional) — `batch` (default), `streaming_table`, or
  `materialized_view`. The seed script uses this to re-promote
  `click_events` to `streaming_table` after a rolling restart wipes
  the in-memory catalog.

Returns `201 Created` on success.

### `GET /v1/catalog/tables`

List all registered tables.

```json
[
  {"name": "employees", "path": "/data/employees.csv", "source": "csv", "columns": [{"name": "id", "data_type": "Int64"}, ...]},
  ...
]
```

### `DELETE /v1/catalog/tables/:name`

Unregister a table. Returns `204 No Content`.

### `GET /v1/catalog/suggestions`

Used by the dashboard's autocomplete. Returns:

```json
{
  "tables": ["employees", "sales", ...],
  "columns": ["id", "name", "dept", ...],
  "functions": ["COUNT", "SUM", "AVG", "MIN", "MAX", ...],
  "keywords": ["SELECT", "FROM", "WHERE", ...]
}
```

`columns` is the union of all columns across all tables — the
autocomplete shows them even if the user hasn't typed a table prefix.

## Cluster control

These endpoints exist but are mostly used by workers internally. The
dashboard doesn't call them; the cluster snapshot is sufficient.

### `POST /v1/workers`

Workers register here at startup. The body is a `WorkerInfo`:
```json
{"address": "0.0.0.0:9090", "cores": 2, "memory_mb": 1024}
```
Returns `201` with the assigned worker id.

### `GET /v1/workers/:id/task`

Workers poll this. Returns `200` with a task or `204 No Content` if no
task is available.

### `POST /v1/tasks/:id/complete`

Body `{"rows": 42}`. Marks the task succeeded.

### `POST /v1/tasks/:id/fail`

Body `{"error": "..."}`. Marks the task failed.

### `POST /v1/workers/:id/heartbeat`

Workers call this every few seconds to keep their status `Alive`.

### `GET /v1/jobs` / `GET /v1/jobs/:id` / `POST /v1/jobs`

List / fetch / submit a job (returns `JobRequest`).

## Pipelines

Pipelines are declarative DAGs of flows. They're submitted as YAML in
the request body and run against the master's catalog. A pipeline
**lives forever in the master's in-memory registry** until the master
pods roll.

### `POST /v1/pipelines`

Run a pipeline. Body is the raw YAML spec. Returns `200 OK` for
one-shot (`refresh: full` / `incremental`) flows with the full
report, or `202 Accepted` for `refresh: live` flows (which tail
forever) with a `status_url` to poll.

```bash
curl -sS -X POST -H 'Content-Type: application/yaml' \
    --data-binary @examples/pipelines/clickstream_live.yaml \
    http://127.0.0.1:7077/v1/pipelines
```

### `GET /v1/pipelines`

List registered pipelines by name. Returns an array of `{name, flows}`.

### `GET /v1/pipelines/:name/dag`

Returns the layered DAG (topo-sort) for the named pipeline, suitable
for the dashboard's visual rendering.

### `GET /v1/pipelines/:name/status`

Returns the live-run status (only meaningful for pipelines submitted
with `refresh: live`):

```json
{
  "pipeline": "clickstream_live",
  "started_at": "2026-07-13T14:33:02Z",
  "last_batch_rows": 12,
  "total_batches": 47,
  "total_rows": 1823,
  "last_batch_at": "2026-07-13T14:35:11Z",
  "last_error": null
}
```

### `GET /v1/pipelines/live`

Lists all currently-running live pipelines (same shape as
`/:name/status`).

### `Refresh::Live { poll_ms }`

Live flows are long-running tails: they poll the source every
`poll_ms` ms and append each non-empty batch to the destination
file. The runner bypasses the SQL planner entirely (the polled
batch IS the output) so the `query` field is ignored for live
flows. See `examples/pipelines/clickstream_live.yaml`.

## Example: a cURL session

```bash
BASE=http://127.0.0.1:7077

# Health
curl $BASE/health

# Register a table
curl -X POST -H "content-type: application/json" \
    -d '{"name":"sales","path":"examples/data/sales.csv"}' \
    $BASE/v1/catalog/tables

# Run a query
curl -X POST -H "content-type: application/json" \
    -d '{"sql":"SELECT product, SUM(amount) FROM sales GROUP BY product"}' \
    $BASE/v1/sql | jq

# Snapshot
curl $BASE/v1/cluster/snapshot | jq '.workers | length'
```