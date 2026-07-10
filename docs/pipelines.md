# Declarative Pipelines

`rspark-pipelines` runs a DAG of flows once, in topological layers, and writes each flow's output to a declared destination. The model mirrors Spark Declarative Pipelines (Lakeflow SDP / Delta Live Tables): declare the pipeline as YAML, the runner handles ordering, and each flow is either a `streaming_table` (incremental-style) or a `materialized_view` (full refresh).

## Spec format

```yaml
pipeline: my_pipeline
flows:
  - name: raw_lines
    kind: streaming_table
    source: { kind: json, path: /path/to/events.json }
    query: "SELECT * FROM raw_lines"
    refresh: full           # full | incremental  (default: full)
    destination:
      kind: file
      path: /tmp/rspark/raw_lines.csv

  - name: counts
    kind: materialized_view
    depends_on: [raw_lines]
    source: { kind: sql }
    query: "SELECT 'placeholder' AS k, 0 AS v"
    destination:
      kind: s3
      key: pipelines/counts.csv
      bucket: rspark-data   # optional — overrides AWS_S3_BUCKET
```

Fields:

- `pipeline` — name of the pipeline. Used as the registry key.
- `flows[]` — list of flows. Each flow runs in a topological layer.
  - `name` — flow name. Unique within the pipeline.
  - `kind` — `streaming_table` (incremental micro-batch source) or
    `materialized_view` (full refresh over its declared source).
  - `depends_on` — list of flow names that must finish first.
  - `source` — where rows come from (see below).
  - `query` — SQL the runner plans + executes against the master catalog.
  - `refresh` — `full` or `incremental`. The runner treats both as full
    refresh today; `incremental` is reserved for a future micro-batch
    mode.
  - `destination` — where the output goes (see below).

### Sources

| `kind` | Fields | Notes |
|--------|--------|-------|
| `tail_dir` | `tail_dir` | NDJSON tailing. Reads one whole file per poll, dedupes by name. |
| `csv` | `path` | One-shot CSV read. |
| `json` | `path` | One-shot NDJSON read. |
| `s3` | `key`, `bucket?` | Reserved — falls back to a clearer error today. |
| `kafka` | `topic`, `brokers`, `group_id` | Stub — returns empty batch. |
| `sql` | (none) | NullSource. Use when the flow's SQL drives everything. |

### Destinations

| `kind` | Fields | Notes |
|--------|--------|-------|
| `file` | `path` | Writes CSV-ish text to disk. |
| `s3` | `key`, `bucket?` | Writes to MinIO / S3. Bucket defaults to `AWS_S3_BUCKET`. |

## Running a pipeline

### CLI

```bash
cargo run -p rspark-pipelines --bin rspark-pipeline -- \
    examples/pipelines/wordcount.yaml
```

Outputs a JSON report like:

```json
{
  "pipeline": "wordcount_demo",
  "started_at": "2026-07-10T12:34:56Z",
  "duration_ms": 87,
  "flows": [
    {"flow": "raw_lines",   "row_count": 15, "destination": "file:///tmp/.../raw_lines.csv", ...},
    {"flow": "word_counts", "row_count":  0, "destination": "file:///tmp/.../word_counts.csv", ...},
    {"flow": "top_words",   "row_count":  0, "destination": "file:///tmp/.../top_words.csv", ...}
  ],
  "errors": []
}
```

### HTTP API

POST the YAML to a running master:

```bash
curl -sS --data-binary @examples/pipelines/wordcount.yaml \
     http://127.0.0.1:8080/v1/pipelines
```

This both stores the pipeline (visible in `GET /v1/pipelines`) and runs it once. The response is the same JSON shape as the CLI.

Endpoints:

- `POST /v1/pipelines` — body is YAML, returns the run report.
- `GET /v1/pipelines` — list `{name, flows}` for every pipeline seen
  since the master started. (Pipelines are not persisted across master
  restarts — a Pipeline CRD in the operator will handle that.)
- `GET /v1/pipelines/:name/dag` — layers + edges for the DAG viz.
  Returns `404` if the pipeline is unknown.

## Topology

The runner builds a `petgraph::DiGraph` with one node per flow and edges from each `depends_on` parent. It then groups nodes into layers via a longest-path layering — flows in the same layer can run in parallel, and the runner walks layers in order.

```text
[raw_lines] -> [word_counts] -> [top_words]
```

The DAG returned by `GET /v1/pipelines/:name/dag` is:

```json
{
  "name": "wordcount_demo",
  "layers": [["raw_lines"], ["word_counts"], ["top_words"]],
  "flows": [
    {"name": "raw_lines",   "kind": "streaming_table",    "depends_on": []},
    {"name": "word_counts", "kind": "materialized_view", "depends_on": ["raw_lines"]},
    {"name": "top_words",   "kind": "materialized_view", "depends_on": ["word_counts"]}
  ]
}
```

## Errors and recovery

Per-flow errors are captured in `PipelineRunReport.errors` and do not abort the run — a downstream flow that depends on a failed upstream will still attempt to run (and likely fail itself). A real SDP implementation would short-circuit on the first error in a layer; today the runner surfaces the error and continues.

Cycles in `depends_on` are detected up-front and rejected with `SpecError::Cycle`. Unknown `depends_on` names are rejected with `SpecError::UnknownFlow`.

## Tests

The crate ships with 9 unit tests covering:

- DAG layer ordering (topological with parallel layers).
- Cycle detection on the `depends_on` graph.
- `FileTailSource` (NDJSON tail) and `NullSource`.
- File destination rendering.
- Spec parsing (round-trip + default `refresh`).
- End-to-end runner: register an `employees` CSV in the catalog, run
  one flow, assert the row count.

```bash
cargo test -p rspark-pipelines
```