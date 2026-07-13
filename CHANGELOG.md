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

### Changed
- Dashboard port-forward moved from `:8080` to `:8088` to avoid
  collision with Docker Desktop's IPv6 listener on the same port.
  `scripts/port-forward.sh` now forwards `8088:8080` and the comment
  documents the conflict.
- `crates/rspark-sql` now exports a `TableKind` enum
  (`Batch | StreamingTable | MaterializedView`) so API and runner code
  can refer to the kind by name rather than by string parsing.

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
