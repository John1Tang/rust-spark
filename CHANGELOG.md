# Changelog

All notable changes to rspark are documented in this file. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project does not yet follow Semantic Versioning.

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
