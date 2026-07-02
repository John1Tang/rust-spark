# Contributing to rspark

Thanks for your interest in making rspark better! This is a learning project,
so the bar for contributions is intentionally approachable — small fixes,
examples, and tests are all welcome.

## Ground rules

- **Be kind.** Disagreements happen; be respectful.
- **Stay scoped.** One focused change per pull request. If you want to refactor
  a crate and add a new SQL feature, do them in two PRs.
- **Match the existing style.** Read the file you're editing before you change
  it. The codebase favours small focused functions, immutable data, and
  early returns over nested conditionals.
- **No drive-by refactors.** If you spot something that bugs you on the way to
  your change, file an issue rather than folding it in.

## Development setup

Prerequisites:

- **Rust 1.75 or later** (edition 2021). `rustup default stable` is fine.
- A C toolchain for the `ring`-style dependencies pulled in by some crates
  (`apt install build-essential pkg-config libssl-dev` on Debian/Ubuntu,
  Xcode command line tools on macOS).

Clone and build:

```bash
git clone https://github.com/John1Tang/rust-spark
cd rust-spark
cargo build
cargo test --workspace
cargo run -p rspark-cli -- sql --input examples/data/employees.csv \
    "SELECT dept, AVG(salary) FROM employees GROUP BY dept"
```

The local CLI doesn't need any service running.

## Workflow

1. Fork the repo and create a topic branch:
   `git checkout -b fix/empty-table-message`
2. Make your change. Add a test that would have caught the bug (or that
   demonstrates the new behaviour). For the executor and SQL parser, an
   integration test in `crates/rspark-exec/tests/integration.rs` is the
   easiest place.
3. Run the test suite locally: `cargo test --workspace`.
4. Run `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings`
   if you have them.
5. Push your branch and open a pull request against `main`. In the description,
   link to the relevant issue, paste before/after output for behaviour changes,
   and note any backwards-compatibility concerns.

## Where things live

```
crates/rspark-core        Schema, Value, Record, RecordBatch, Expr
crates/rspark-storage     DataSource trait + CSV / JSON readers, OutputWriter
crates/rspark-sql         sqlparser-based parser → Expr → LogicalPlan builder
crates/rspark-exec        PhysicalOp algebra + LocalExecutor pipeline driver
crates/rspark-cluster     Master / Worker state machine + task scheduling
crates/rspark-api         axum HTTP API for cluster control
crates/rspark-dashboard   Self-contained HTML/JS dashboard
crates/rspark-cli         clap subcommands: master, worker, sql, submit, shell, dashboard
```

A `LogicalPlan` lowers to a `PhysicalOp` tree (`Scan / Project / Filter /
Aggregate / Sort / Limit / Join`) and applies row-by-row over `RecordBatch`es
in `rspark-exec::LocalExecutor::execute`. In cluster mode the master splits a
job into tasks, hands each task to a worker, and the worker reuses the same
executor pipeline locally.

## Coding style

- **Immutability** — return new values rather than mutating in place.
- **KISS / YAGNI** — don't add abstractions for hypothetical future needs.
- **Small files** — 200–400 lines is a good target; 800 is the hard ceiling.
- **One responsibility per function.** If a function has multiple `?` for
  different reasons, split it.
- **Match on enums exhaustively** — fall-throughs hide bugs when the enum grows.
- **Errors are values.** Use `Result<_, Error>` at the boundary, `?` to bubble.

## SQL surface

rspark supports a meaningful subset of Spark SQL — `SELECT`, `WHERE`,
`GROUP BY ... HAVING`, `ORDER BY`, `LIMIT`, `DISTINCT`, joins, `COALESCE`/`NVL`,
`LIKE`, `IN`, `BETWEEN`, `COUNT/SUM/AVG/MIN/MAX`, and `SHOW CREATE TABLE`.
Things it does **not** yet support: CTEs (`WITH`), window functions, subqueries
in `FROM`, lateral joins, DDL other than `SHOW CREATE TABLE`. If you add a new
SQL feature, please add at least one test that runs end-to-end through the
planner + executor.

## Reporting bugs

Open an issue at <https://github.com/John1Tang/rust-spark/issues>. Include:

- What you ran (the exact `cargo run` / `curl` / SQL).
- What you expected.
- What you got (full error message, stack trace, or screenshot).
- Your OS and Rust version (`rustc --version`).

## Security

If you find a security issue, please email the maintainers instead of opening
a public issue. We will coordinate a fix and a disclosure.
