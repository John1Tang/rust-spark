# Changelog

All notable changes to rspark are documented in this file. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project does not yet follow Semantic Versioning.

## [In progress]

### Added
- **Streaming-⨯-batch demo path** — a curated `Examples` section on the
  dashboard's SQL Lab with two streaming-table queries (one left join,
  one aggregate-after-join) that you can click to load into the editor.
  Styled with a dedicated `.example-pill` class; streaming pills use
  `.example-stream` (blue) to distinguish them from batch examples.
- **Expanded mock data** in `examples/data/`:
  `users.csv` 200 rows, `orders.csv` 400 rows, `clickstream.jsonl`
  1500 events across 200 users (plus 23 anonymous events that exercise
  the LEFT JOIN's null branch).
- **Idempotent seed script** `scripts/seed-mock-data.sh` — uploads the
  fixtures to MinIO, copies them into the master pod (so the pipeline's
  local `kind: json` source can read the new size), registers the
  batch tables in the in-memory catalog, runs the clickstream
  pipeline to flip `click_events` to `kind: streaming_table`, and
  re-points the catalog at the raw NDJSON (the pipeline output is
  pipe-delimited; the CsvSource uses comma and would otherwise yield
  a 1-column schema mismatch).
- **Kind-aware catalog registration** — `POST /v1/catalog/tables` now
  accepts an optional `kind` field (`batch` / `streaming_table` /
  `materialized_view`) so external callers can register streaming
  tables without going through a pipeline run.
- **Live event collector + SQL Lab live refresh** — end-to-end demo of
  streaming-table joins that visibly grow as you click. Two pieces:
  - **Open event collector ↗ button** in SQL Lab's editor row opens
    `examples/e2e/demo_page.html` (embedded into the dashboard binary
    via `include_str!`; served at `/examples/e2e/demo_page.html`) in a
    new tab. Clicks there POST to the in-cluster `rspark-ingest`,
    which produces to Kafka topic `rspark.page_events`.
  - **Live refresh checkbox** in SQL Lab re-runs the current query
    every 1.5 s, so a streaming-⨯-batch join query visibly grows as
    new events arrive in the demo page.
  - **`Refresh::Live { poll_ms }`** added to the pipeline spec. Live
    flows tail the source forever and append each polled batch to the
    destination NDJSON file; the runner bypasses the planner (the
    polled batch IS the output) so circular `FROM click_events` is
    avoided. `POST /v1/pipelines` returns `202 Accepted` for live
    flows; status is available at `GET /v1/pipelines/:name/status` and
    `GET /v1/pipelines/live`.
  - **`scripts/seed-mock-data.sh`** starts `examples/pipelines/
    clickstream_live.yaml` (Kafka topic → `click_events` NDJSON tail)
    on every run and re-points the catalog at the live destination.
  - **`scripts/port-forward.sh`** now also forwards `:8081` for the
    ingest service so the demo page (running in the host browser) can
    reach it.
- **Demo page redesigned as a real e-commerce shop** (`examples/e2e/
  demo_page.html`). The page used to be a placeholder button pair
  with hardcoded "Item 1…Item 20"; it's now a 16-card product grid
  (desks, monitors, keyboards, webcams, … — the same slugs that
  appear in `examples/data/clickstream.jsonl`) with search,
  category filter pills, sticky cart counter, and a **live
  activity ticker** that fetches `/v1/sql` every 5 s. The ticker
  shows real `click_events` rows on first paint so the demo
  doesn't feel empty before you click anything.
- **Demo page emits normalized events** (`event_type` + `ts` in ms,
  matching the `clickstream.jsonl` schema). Previously the page
  emitted `type` + `ts_ms`, which landed in the live NDJSON with a
  divergent schema and made the JSON source's CSV reader fail with
  "found record with 6 fields, but the previous record has 5
  fields" once any pre-existing rows were mixed in. The emitter now
  normalizes legacy field names before buffering.
- **Live NDJSON backfilled** with ~300 historical events on first
  seed so streaming-⨯-batch joins return rows immediately. Without
  the backfill the `click_events` streaming table starts empty and
  a `SELECT … FROM click_events` returns 0 rows until you click the
  demo page enough times.
- **`LogicalPlan::collect_table_refs()`** — a non-invasive primitive
  on `crates/rspark-sql/src/plan.rs::LogicalPlan` that returns the
  catalog tables referenced by any `Scan` in the plan tree, in
  first-occurrence order with duplicates removed. Six unit tests
  cover Scan / Project / Filter / Join / Union / Empty / dedup cases.
  Future work: `/v1/sql` will call this on ad-hoc SQL to decide
  whether to auto-spawn a live pipeline.

### Changed
- Dashboard port-forward moved from `:8080` to `:8088` to avoid
  collision with Docker Desktop's IPv6 listener on the same port.
  `scripts/port-forward.sh` now forwards `8088:8080` and the comment
  documents the conflict.
- `crates/rspark-sql` now exports a `TableKind` enum
  (`Batch | StreamingTable | MaterializedView`) so API and runner code
  can refer to the kind by name rather than by string parsing.

### Known limitations
- **`ORDER BY col` where `col` is not in the SELECT list fails**
  with `schema error: column '<col>' not found in schema`. The Sort
  node references the column from its child's schema, but the
  executor only sees the post-Project schema. Workaround: include
  the ORDER BY column in the SELECT list (or use a column-alias).
  Tracked as a follow-on fix; out of scope for the streaming-table
  demo work.
- **`substr()` function not implemented**. Use `LIKE '/products/%'`
  for prefix matching, or aggregate on the full column.

## [Unreleased]

### Added
- Initial public release of the workspace.
- `rspark-core` — typed `Value` / `Schema` / `Record` / `RecordBatch` primitives
  with `serde` support.
- `rspark-sql` — `sqlparser-rs` based parser, logical planner with
  `LogicalPlan` (Scan / Project / Filter / Aggregate / Sort / Limit / Join /
  Union / Distinct) and a Spark SQL surface (`SELECT`, `WHERE`, `GROUP BY` /
  `HAVING`, `ORDER BY`, `LIMIT`, `DISTINCT`, joins, `COALESCE` / `NVL`,
  `LIKE`, `IN`, `BETWEEN`, aggregate functions, `SHOW CREATE TABLE`).
- `rspark-exec` — physical algebra and `LocalExecutor` pipeline.
- `rspark-storage` — `DataSource` trait + CSV and JSON (NDJSON) readers.
- `rspark-cluster` — Master / Worker state machine, task scheduling, HTTP
  polling, heartbeat.
- `rspark-api` — `axum` HTTP API for cluster control and SQL execution.
- `rspark-dashboard` — self-contained HTML / JS dashboard with SQL Lab and
  Cluster tabs, live autocomplete, and a result panel.
- `rspark-cli` — `master`, `worker`, `sql`, `submit`, `shell`, `dashboard`
  subcommands.
- Docker Compose and Kubernetes manifests under `docker/` and `k8s/`.
- Bundled mock data in `examples/data/` and a 12-query demo script
  (`examples/demo.sh`).
- 36 tests across the workspace.

### Notes
- This is the first open-source release; expect rough edges.
- The cluster path is single-master and does not yet implement task result
  shuffling — workers execute their assigned task locally with the same
  executor pipeline the CLI uses.
