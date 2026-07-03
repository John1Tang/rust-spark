# SQL surface

rspark speaks a meaningful subset of Spark SQL. This page is the
canonical list of what's supported, what's not, and where to look when
you want to add something.

## What works

### Queries

- `SELECT [DISTINCT] <expr_list> FROM <table> [WHERE ...] [GROUP BY ...]
  [HAVING ...] [ORDER BY ...] [LIMIT n]`
- `*` and `table.*` (table-qualified wildcards work; database-qualified
  `db.table.*` is parsed but the catalog is single-namespace).
- Column aliases: `SELECT col AS alias`.
- Table aliases: `FROM employees e`.
- Sub-expressions: `WHERE salary > 80000 AND dept = 'Engineering'`.
- Multiple `JOIN`s in the `FROM` clause (cross-join is used by default
  when no `ON` is given; an `ON` predicate is required for an
  inner join).

### Expressions

- Literals: integers, floats, strings, `TRUE`/`FALSE`, `NULL`.
- Operators: `=`, `<>`, `!=`, `<`, `<=`, `>`, `>=`,
  `AND`, `OR`, `NOT`, `IN`, `BETWEEN`, `LIKE`, `IS NULL`,
  `IS NOT NULL`, `+`, `-`, `*`, `/`, `%`.
- Functions: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `COALESCE`, `NVL`,
  `ABS`, `UPPER`, `UCASE`, `LOWER`, `LCASE`, `LENGTH`, `CHAR_LENGTH`,
  `CHARACTER_LENGTH`.
- `COALESCE(a, b)` returns `a` if non-null else `b` — equivalent to a
  `CASE WHEN a IS NOT NULL THEN a ELSE b END` shortcut.
- `IF` / `CASE` expressions are parsed by `sqlparser` and lowered into
  the internal `Expr::If { cond, then_expr, else_expr }` variant.

### Aggregates

- `COUNT(*)`, `COUNT(col)`, `COUNT(DISTINCT col)` (parsing only; the
  executor currently treats all counts identically).
- `SUM`, `AVG`, `MIN`, `MAX` over numeric or comparable columns.
- `GROUP BY` accepts both expressions and `GROUP BY ALL`.
- `HAVING` filters on aggregate results. The planner rewrites
  `HAVING AVG(salary) > 80000` to reference the projected column so the
  executor can look it up after aggregation.
- `GROUP BY` may not be exhaustive — non-grouped columns in `SELECT`
  are not validated against the SQL standard.

### Joins

- Cross joins, inner joins (with `ON` predicate).
- The `FROM t1, t2` comma-join form is parsed as a cross join; the
  operator emits an `INNER JOIN ... ON (matching column names)`.

### DDL

- `SHOW CREATE TABLE <name>` — renders a `CREATE TABLE` block with
  the registered schema, source format, and on-disk location. The
  executor short-circuits this in `rspark_sql::show_create::try_show_create`
  before the planner sees it.

### Source formats

- CSV (with type inference from a sample of the file).
- JSON / NDJSON (one JSON object per line).
- The `format` is auto-detected from the file extension unless
  overridden via `SourceRegistry::register(name, source)`.

## What's not yet supported

- **`WITH` (CTEs)** — rejected by the planner with "CTE (WITH) not yet
  supported".
- **Window functions** (`OVER`, `PARTITION BY`).
- **Subqueries in `FROM`** — `SELECT * FROM (SELECT ...)` is rejected.
- **Lateral joins** (`LEFT JOIN LATERAL`).
- **DDL other than `SHOW CREATE TABLE`** — `CREATE TABLE`, `DROP TABLE`,
  `ALTER TABLE` all go through the planner and error.
- **Recursive CTEs** (the parser accepts them; the planner doesn't).
- **Type coercion / `CAST`** — the parser accepts `CAST(x AS INT)` but
  the executor treats the cast as a no-op.
- **`UNION` / `INTERSECT` / `EXCEPT`** — `UNION` is parsed and the
  LogicalPlan has a `Union` variant, but `INTERSECT`/`EXCEPT` are not.

## SQL surface in tests

The integration tests in `crates/rspark-exec/tests/integration.rs`
exercise the surface end-to-end against `examples/data/employees.csv`
and `examples/data/sales.csv`. If you add a feature, add a test there
first.

The `examples/demo.sh` shell script runs 12 queries against the
bundled data and is a quick smoke test for the dashboard surface.

## Adding a new SQL feature

A new SQL feature (function, operator, clause) typically touches:

1. **`crates/rspark-sql/src/expr_builder.rs`** — translate the
   `sqlparser::ast` node into an `Expr` variant (or extend an existing
   one).
2. **`crates/rspark-core/src/expr.rs`** — implement `eval`,
   `display_name`, `collect_columns`, `contains_aggregate`. `eval` is
   where type-mismatch errors live.
3. **`crates/rspark-sql/src/plan.rs`** — if it adds a new
   `LogicalPlan` variant, add an arm to `lower_plan`; if it changes
   schema inference, update `build_*_schema` and `infer_data_type`.
4. **`crates/rspark-exec/src/operators.rs`** — for a new operator
   (e.g. window function), add the physical function and a new
   `LogicalPlan` arm in `apply_tree` in `executor.rs`.
5. **`crates/rspark-exec/tests/integration.rs`** — end-to-end test
   that runs the plan through the executor. This is the regression
   net.
6. **`crates/rspark-sql/src/show_create.rs`** — if the schema or
   syntax spelling changes.
7. **`crates/rspark-api/src/routes.rs`** — the `SQL_KEYWORDS` /
   `SQL_FUNCTIONS` constants in `routes.rs` should be updated so the
   dashboard's autocomplete picks it up.

For DDL other than `SHOW CREATE TABLE`, intercept in
`rspark_api::routes::execute_sql` (see the `try_show_create` pattern) —
`Master::submit_job` runs the planner, so a statement that the planner
doesn't understand will 500.

## Why certain things are missing

Most of the gaps above are conscious decisions in a learning project.
Window functions, recursive CTEs, and subqueries are big enough
features that they'd each warrant a few days of design work — the
planner data structures (`LogicalPlan`) and the executor pipeline
(`apply_tree` in `executor.rs`) are the places to extend. The roadmap
has no specific order; pick whichever you find most interesting.

## Quirks worth knowing

- **MIN/MAX over different types**: ordered by `Value::try_cmp`,
  which panics if the types aren't comparable. Don't `MIN` between a
  String and an Int.
- **`COUNT(*)` returns Int64, `AVG` returns Float64, `SUM` returns
  Float64** even on integer inputs. This matches Spark's behaviour.
- **NULL handling**: `NULL = NULL` is `NULL` (not true). `NULL <> NULL`
  is also `NULL`. Use `IS NULL` / `IS NOT NULL` to compare to NULL.
- **String quoting**: both single and double quotes work in the
  parser; the executor treats both as String.