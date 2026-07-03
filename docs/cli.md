# CLI

The CLI is one binary (`rspark-cli`) with subcommands. Run
`rspark-cli --help` for the canonical list.

## Subcommands

### `sql` — local one-shot SQL

```bash
rspark-cli sql \
    --input examples/data/employees.csv \
    --input examples/data/sales.csv \
    "SELECT dept, AVG(salary) FROM employees GROUP BY dept"
```

Files become tables named after the file's stem (`employees.csv` →
`employees`). You can pass `--input-format json` to override the default
CSV.

The query runs in-process, the result is printed as a table. Useful
flags:
- `--file query.sql` — read SQL from a file instead of an argument.
- `--output out.csv` — write the result as CSV instead of printing.
- `--catalog path/to/catalog.json` — load extra tables from a JSON
  catalog file (an array of `{name, path, source}` entries).

### `shell` — interactive REPL

```bash
rspark-cli shell --input examples/data/employees.csv
```

Prompts with `rspark>`. Statements end with `;`. Use `:tables` to list
the catalog, `:quit` to exit.

### `master` — start the cluster control plane

```bash
rspark-cli master \
    --api-addr 0.0.0.0:7077 \
    --dashboard-addr 0.0.0.0:8080 \
    --master-id master-1 \
    --examples \
    --load users=examples/data/employees.csv
```

| Flag                  | Meaning                                                                |
| --------------------- | ---------------------------------------------------------------------- |
| `--api-addr`          | Bind address for the cluster API (HTTP)                                |
| `--dashboard-addr`    | Bind address for the dashboard (HTTP)                                 |
| `--master-id`         | Stable identifier for this master pod (so workers can disambiguate)   |
| `--examples`           | Preload bundled example data into the catalog                          |
| `--load name=path`    | Register an extra table at startup (repeatable)                        |
| `--master-id` env var | Sets `RSPARK_MASTER_ID`                                                |

The master logs `rspark master ready: api=…, dashboard=…` on startup.

### `worker` — connect to a master

```bash
rspark-cli worker \
    --master http://master.example.com:7077 \
    --bind 0.0.0.0:9091 \
    --cores 4 \
    --memory-mb 4096
```

The worker registers itself with the master, then polls forever for
task assignments. It does not re-register after a master rolling
restart — see `docs/operator.md` for the trade-off.

### `submit` — push a job to a cluster

```bash
rspark-cli submit \
    --master http://127.0.0.1:7077 \
    --file query.sql \
    --name my-job \
    --parallelism 2
```

Reads SQL from `--file`, POSTs it to the master's `/v1/jobs` endpoint,
prints the assigned job id + status.

### `dashboard` — dashboard-only server

```bash
rspark-cli dashboard \
    --addr 127.0.0.1:8080 \
    --master http://master.example.com:7077
```

Runs the dashboard pointing at a remote master URL. Useful when the
master is on a different machine than the developer.

## Environment variables

| Variable          | Effect                                                            |
| ----------------- | ----------------------------------------------------------------- |
| `RSPARK_LOG`      | `tracing-subscriber` filter (e.g. `info,rspark=debug`)           |
| `RUST_LOG`        | Same as `RSPARK_LOG` (operator binary uses `RUST_LOG`)            |

## Exit codes

- `0` — success.
- `1` — `rspark_core::error::Error` of any kind.
- `2` — clap parse error (unknown subcommand, missing required flag).

## Examples

```bash
# Read a query file, print the result, save to CSV.
rspark-cli sql \
    --input examples/data/employees.csv \
    --file query.sql \
    --output out.csv

# Run a query against JSON.
rspark-cli sql \
    --input-format json \
    --input examples/data/events.json \
    "SELECT event, COUNT(*) FROM events GROUP BY event"

# Submit to a remote cluster.
rspark-cli submit \
    --master http://rspark-master.prod.svc.cluster.local:7077 \
    --file report.sql \
    --name nightly-report
```