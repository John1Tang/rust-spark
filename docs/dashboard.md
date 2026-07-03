# Dashboard

The dashboard is a single self-contained HTML page served from the
master pod on port 8080. It is the primary interactive surface — open
`http://localhost:8080` after running `master` and you have a live SQL
Lab with autocomplete and a real-time cluster view.

## Where the code lives

Everything is in `crates/rspark-dashboard/src/ui.rs`:

- The HTML, CSS, and JS are a single `const DASHBOARD_HTML: &str = r##"…"##;`
- `render_dashboard()` returns it as a `String`.
- `crates/rspark-dashboard/src/server.rs` mounts it as the
  `axum::Router::fallback` so any path that isn't an API route
  serves the page.

There is no build step, no asset pipeline, and no framework. Open the
HTML in your editor and edit it directly.

## Tabs

### SQL Lab

- **Editor** (large textarea). Press **Ctrl+Enter** to run. Press
  **Tab** or **Enter** while the autocomplete popup is open to accept
  the highlighted suggestion.
- **Suggestions panel** (left sidebar, bottom). Clicking a token
  inserts it at the cursor in the editor.
- **Tables panel** (left sidebar, top). Clicking a table inserts its
  name; "remove" unregisters it from the catalog via `DELETE
  /v1/catalog/tables/:name`.
- **Sample queries**. One click runs the query.
- **History**. Last 8 queries, persisted in `localStorage`.
- **Execution metrics** strip — total runs, succeeded, failed, average
  duration, last query. Persisted in `localStorage` so reloads don't
  reset them.
- **Result panel**. Columns with their types, rows right-aligned for
  numbers, errors with the `Error` variant name in the prefix.

### Cluster

- **Cluster summary** — workers, alive count, total cores, total
  memory.
- **Jobs** — active (Running) and recent (last 8 by submitted_at).
- **SQL currently running** — verbatim SQL of any in-flight job.
- **Stages**, **Workers**, **Tasks** — full state from the cluster
  snapshot.

The "Cluster" tab gets a small badge with the running job count while
it's not the active tab.

## Autocomplete

When you type a token (1+ letters) in the editor, a popup appears under
the caret listing:

- **Tables** — exact prefix match, sorted alphabetically. Highest priority.
- **Columns** — same, but with `column` kind tag.
- **Functions** — `COUNT`, `SUM`, etc.
- **Keywords** — `SELECT`, `FROM`, …

Use ↑/↓ to navigate, Tab/Enter to accept, Esc to dismiss. Ctrl+Space
forces the popup open.

The mirror `<span id="mirror">` next to the textarea is invisible but
the JS copies your last line into it to compute the caret pixel
position. This is what makes the popup line up correctly regardless of
indentation or character widths.

## Local state

Three things are persisted in `localStorage`:

- `rspark.sql.history.v1` — the last 8 SQL queries.
- `rspark.execStats.v1` — total runs / succeeded / failed / duration
  counters.
- `rspark.activeTab.v1` — which tab is selected.

Clear them via DevTools → Application → Local Storage → `http://localhost:8080`.

## API integration

The dashboard calls these endpoints:

| Action                       | Endpoint                                    |
| ---------------------------- | ------------------------------------------- |
| Initial load                 | `GET /health`                               |
| Tab badge refresh            | `GET /v1/cluster/snapshot` (every 1.5s)     |
| Run query                    | `POST /v1/sql`                              |
| Register table               | `POST /v1/catalog/tables`                   |
| Remove table                 | `DELETE /v1/catalog/tables/:name`           |
| Autocomplete suggestions     | `GET /v1/catalog/suggestions`               |

If any endpoint returns 500, the error message + variant name is shown
inline.

## Customising

Edit `DASHBOARD_HTML` in `crates/rspark-dashboard/src/ui.rs`. The dashboard
test (`crates/dashboard/src/server.rs`) only checks the HTML starts
with `<!doctype html>` and contains `/v1/cluster/snapshot`, so most
visual changes won't break tests — but changes that remove those
markers will. Add new endpoints and the dashboard will start using
them.

## Known quirks

- **CORS**: the dashboard is served by the same axum router as the
  API. `CorsLayer::permissive()` is set on the dashboard server, so
  cross-origin fetches from the dashboard itself work fine.
- **localStorage size**: the 8-entry history + counters are well under
  any browser limit.
- **IME composition**: the autocomplete uses `input` + `keyup` events.
  Composition events (`compositionend`) may not trigger `input` until
  the user commits, which can feel laggy in some IMEs. We don't
  currently handle `compositionstart`/`compositionend` separately.