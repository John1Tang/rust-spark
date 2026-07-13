/// Return the inline dashboard HTML page.
///
/// The page is intentionally self-contained: it ships its own CSS and JS
/// and only depends on the JSON API exposed by `rspark-api`. The
/// `data-cluster` attribute on `<body>` lets the JS detect which host to
/// call when the dashboard is served from a worker.
pub fn render_dashboard() -> String {
    DASHBOARD_HTML.to_string()
}

const DASHBOARD_HTML: &str = r##"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>rspark dashboard</title>
    <style>
      :root {
        color-scheme: dark;
        --bg: #0b0f17;
        --panel: #131a26;
        --border: #1f2a3c;
        --text: #e6edf3;
        --muted: #8b97aa;
        --accent: #7dd3fc;
        --success: #34d399;
        --warn: #fbbf24;
        --danger: #f87171;
        --code: #f0abfc;
      }
      * { box-sizing: border-box; }
      body {
        margin: 0;
        font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text",
          "Helvetica Neue", Arial, sans-serif;
        background: var(--bg);
        color: var(--text);
      }
      header {
        padding: 18px 28px;
        border-bottom: 1px solid var(--border);
        background: linear-gradient(180deg, rgba(125,211,252,0.05), transparent);
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: 12px;
      }
      header h1 {
        margin: 0;
        font-size: 22px;
        font-weight: 600;
        letter-spacing: -0.01em;
      }
      header .meta {
        margin-top: 4px;
        color: var(--muted);
        font-size: 13px;
      }
      nav.tabs {
        display: flex;
        gap: 4px;
        padding: 0 28px;
        background: var(--bg);
        border-bottom: 1px solid var(--border);
        position: sticky;
        top: 0;
        z-index: 5;
      }
      nav.tabs button {
        background: transparent;
        color: var(--muted);
        border: 0;
        border-bottom: 2px solid transparent;
        border-radius: 0;
        padding: 12px 18px;
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        letter-spacing: 0.02em;
        display: inline-flex;
        align-items: center;
        gap: 8px;
      }
      nav.tabs button:hover { color: var(--text); }
      nav.tabs button.active {
        color: var(--accent);
        border-bottom-color: var(--accent);
      }
      nav.tabs button .badge {
        background: rgba(125,211,252,0.12);
        color: var(--accent);
        border-radius: 999px;
        padding: 1px 8px;
        font-size: 11px;
        font-weight: 600;
      }
      .layout {
        display: grid;
        grid-template-columns: 240px 1fr;
        gap: 20px;
        padding: 24px 28px 48px;
      }
      .layout.no-sidebar { grid-template-columns: 1fr; }
      @media (max-width: 900px) {
        .layout, .layout.no-sidebar { grid-template-columns: 1fr; }
      }
      aside.sidebar {
        position: sticky;
        top: 72px;
        align-self: start;
        max-height: calc(100vh - 96px);
        overflow-y: auto;
      }
      main {
        display: grid;
        gap: 20px;
        min-width: 0;
      }
      .tab-panel { display: none; }
      .tab-panel.active { display: grid; gap: 20px; min-width: 0; }
      section {
        background: var(--panel);
        border: 1px solid var(--border);
        border-radius: 14px;
        padding: 18px 20px;
        min-width: 0;
      }
      section h2 {
        margin: 0 0 12px;
        font-size: 15px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--accent);
        display: flex;
        align-items: center;
        justify-content: space-between;
      }
      section h2 .count {
        font-size: 11px;
        color: var(--muted);
        font-weight: 400;
        text-transform: none;
        letter-spacing: 0;
      }
      .stat-grid {
        display: grid;
        grid-template-columns: repeat(4, 1fr);
        gap: 12px;
      }
      .stat-grid.cols-5 { grid-template-columns: repeat(5, 1fr); }
      .stat {
        background: rgba(255, 255, 255, 0.02);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 14px;
      }
      .stat .label {
        font-size: 11px;
        text-transform: uppercase;
        color: var(--muted);
        letter-spacing: 0.08em;
      }
      .stat .value {
        margin-top: 4px;
        font-size: 26px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
      }
      .stat .value.small { font-size: 18px; }
      table {
        width: 100%;
        border-collapse: collapse;
        font-size: 13px;
      }
      th, td {
        text-align: left;
        padding: 8px 10px;
        border-bottom: 1px solid var(--border);
      }
      th {
        color: var(--muted);
        font-weight: 500;
        text-transform: uppercase;
        font-size: 11px;
        letter-spacing: 0.08em;
      }
      td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
      .pill {
        display: inline-block;
        padding: 2px 8px;
        border-radius: 999px;
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.04em;
      }
      .pill.running { background: rgba(125,211,252,0.12); color: var(--accent); }
      .pill.success { background: rgba(52,211,153,0.12); color: var(--success); }
      .pill.failed, .pill.dead { background: rgba(248,113,113,0.12); color: var(--danger); }
      .pill.pending, .pill.alive { background: rgba(251,191,36,0.12); color: var(--warn); }
      .pill.assigned { background: rgba(167,139,250,0.12); color: #c4b5fd; }
      .empty {
        color: var(--muted);
        font-style: italic;
        font-size: 13px;
        padding: 8px 0;
      }
      pre {
        background: rgba(0, 0, 0, 0.35);
        border-radius: 8px;
        padding: 12px;
        font-size: 12px;
        line-height: 1.45;
        max-height: 220px;
        overflow: auto;
        white-space: pre-wrap;
        word-break: break-all;
        margin: 0;
      }
      .footer {
        padding: 16px 28px 32px;
        color: var(--muted);
        font-size: 12px;
        text-align: center;
      }
      .auto-refresh {
        display: inline-block;
        margin-left: 12px;
        font-size: 12px;
        color: var(--muted);
      }
      .editor {
        display: flex;
        flex-direction: column;
        gap: 10px;
      }
      .editor textarea {
        width: 100%;
        min-height: 140px;
        background: rgba(0, 0, 0, 0.35);
        color: var(--text);
        border: 1px solid var(--border);
        border-radius: 10px;
        padding: 12px 14px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        font-size: 13px;
        line-height: 1.5;
        resize: vertical;
        outline: none;
      }
      .editor textarea:focus { border-color: var(--accent); }
      .editor-row {
        display: flex;
        gap: 8px;
        align-items: center;
        flex-wrap: wrap;
      }
      button {
        background: var(--accent);
        color: #06212e;
        border: 0;
        border-radius: 8px;
        padding: 8px 14px;
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition: opacity 0.15s ease;
      }
      button:disabled { opacity: 0.55; cursor: progress; }
      button.secondary {
        background: transparent;
        color: var(--text);
        border: 1px solid var(--border);
      }
      .history {
        display: flex;
        gap: 6px;
        flex-wrap: wrap;
      }
      .history button {
        background: rgba(255, 255, 255, 0.04);
        color: var(--text);
        font-weight: 400;
        padding: 4px 10px;
        font-size: 12px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        max-width: 240px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .result-meta {
        display: flex;
        gap: 16px;
        color: var(--muted);
        font-size: 12px;
        margin-bottom: 10px;
        flex-wrap: wrap;
      }
      .result-meta strong { color: var(--text); }
      .result-error {
        background: rgba(248,113,113,0.08);
        border: 1px solid rgba(248,113,113,0.3);
        color: #fecaca;
        border-radius: 8px;
        padding: 10px 12px;
        font-size: 13px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        white-space: pre-wrap;
      }
      .result-table-wrap {
        max-height: 360px;
        overflow: auto;
        border: 1px solid var(--border);
        border-radius: 8px;
      }
      .result-table-wrap table { font-variant-numeric: tabular-nums; }
      .null-cell { color: var(--muted); font-style: italic; }
      .table-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
      .table-list li {
        background: rgba(255, 255, 255, 0.02);
        border: 1px solid var(--border);
        border-radius: 8px;
        padding: 10px 12px;
        font-size: 12px;
        cursor: pointer;
        transition: border-color 0.15s ease;
      }
      .table-list li:hover { border-color: var(--accent); }
      .table-list .name {
        color: var(--code);
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        font-weight: 600;
        margin-bottom: 2px;
      }
      .table-list .meta { color: var(--muted); font-size: 11px; }
      .table-list button.remove {
        margin-top: 6px;
        background: transparent;
        color: var(--muted);
        border: 1px solid var(--border);
        padding: 2px 8px;
        font-size: 10px;
      }
      .add-table { display: flex; flex-direction: column; gap: 6px; margin-top: 12px; }
      .add-table input {
        background: rgba(0, 0, 0, 0.35);
        color: var(--text);
        border: 1px solid var(--border);
        border-radius: 6px;
        padding: 6px 10px;
        font-size: 12px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        outline: none;
      }
      .add-table input:focus { border-color: var(--accent); }
      .add-table .row { display: flex; gap: 6px; }
      .add-table .row input { flex: 1; }
      .samples { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
      .samples button {
        background: rgba(240, 171, 252, 0.08);
        color: var(--code);
        font-weight: 400;
        padding: 4px 10px;
        font-size: 11px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        border: 1px solid rgba(240, 171, 252, 0.25);
      }
      .samples-prominent {
        gap: 10px;
        margin-top: 4px;
        padding: 12px;
        background: rgba(15, 23, 42, 0.4);
        border: 1px solid rgba(240, 171, 252, 0.18);
        border-radius: 8px;
      }
      .example-pill {
        background: rgba(240, 171, 252, 0.10) !important;
        color: var(--code) !important;
        font-weight: 500 !important;
        padding: 8px 14px !important;
        font-size: 13px !important;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        border: 1px solid rgba(240, 171, 252, 0.35) !important;
        border-radius: 999px !important;
        cursor: pointer;
        transition: background 120ms ease, transform 120ms ease, border-color 120ms ease;
      }
      .example-pill:hover {
        background: rgba(240, 171, 252, 0.22) !important;
        border-color: rgba(240, 171, 252, 0.7) !important;
        transform: translateY(-1px);
      }
      .example-pill:active { transform: translateY(0); }
      .example-pill.example-stream {
        background: rgba(96, 165, 250, 0.14) !important;
        border-color: rgba(96, 165, 250, 0.55) !important;
        color: #bfdbfe !important;
      }
      .example-pill.example-stream:hover {
        background: rgba(96, 165, 250, 0.28) !important;
        border-color: rgba(96, 165, 250, 0.9) !important;
      }
      .suggest {
        display: flex;
        gap: 6px;
        flex-wrap: wrap;
        max-height: 220px;
        overflow-y: auto;
      }
      .suggest .group { display: flex; gap: 4px; flex-wrap: wrap; align-items: center; width: 100%; }
      .suggest .group-label {
        color: var(--muted);
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        margin-right: 4px;
      }
      .suggest button {
        background: rgba(255, 255, 255, 0.04);
        color: var(--text);
        font-weight: 400;
        padding: 3px 8px;
        font-size: 11px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        border: 1px solid var(--border);
        cursor: pointer;
      }
      .suggest button.table { color: var(--code); border-color: rgba(240,171,252,0.3); }
      .suggest button.streaming { color: #7dd3fc; border-color: rgba(125,211,252,0.4); background: rgba(125,211,252,0.05); }
      .suggest button.view { color: #c4b5fd; border-color: rgba(196,181,253,0.4); background: rgba(196,181,253,0.05); }
      .suggest button.column { color: var(--accent); border-color: rgba(125,211,252,0.3); }
      .suggest button.function { color: var(--success); border-color: rgba(52,211,153,0.3); }
      .suggest button.keyword { color: var(--muted); }
      .suggest .group-label { color: var(--muted); font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; margin-right: 8px; padding: 2px 0; }
      .suggest .group { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 4px 0; border-bottom: 1px solid var(--border); }
      .suggest .group:last-child { border-bottom: none; }
      .suggest button .kind {
        color: var(--muted);
        font-size: 9px;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        padding: 1px 5px;
        border-radius: 999px;
        background: rgba(255,255,255,0.04);
        margin-left: 4px;
      }
      .suggest button.streaming .kind { color: #7dd3fc; background: rgba(125,211,252,0.12); }
      .suggest button.view .kind { color: #c4b5fd; background: rgba(196,181,253,0.12); }
      .exec-strip {
        display: grid;
        grid-template-columns: repeat(5, 1fr);
        gap: 12px;
      }
      .two-col {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 20px;
      }
      @media (max-width: 1100px) {
        .two-col { grid-template-columns: 1fr; }
      }
      .autocomplete-popup {
        position: absolute;
        z-index: 100;
        background: var(--panel);
        border: 2px solid var(--accent);
        border-radius: 8px;
        padding: 4px;
        min-width: 220px;
        max-height: 260px;
        overflow-y: auto;
        box-shadow: 0 8px 32px rgba(0,0,0,0.55);
        font-size: 12px;
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
      }
      .autocomplete-popup .item {
        padding: 6px 10px;
        border-radius: 4px;
        cursor: pointer;
        color: var(--text);
        display: flex;
        justify-content: space-between;
        gap: 8px;
        align-items: center;
      }
      .autocomplete-popup .item:hover, .autocomplete-popup .item.active {
        background: rgba(125, 211, 252, 0.18);
        outline: 1px solid var(--accent);
      }
      .autocomplete-popup .item .kind {
        color: var(--muted);
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        padding: 2px 6px;
        border-radius: 999px;
        background: rgba(255,255,255,0.04);
      }
      .autocomplete-popup .item[data-kind="table"] .kind { color: var(--code); }
      .autocomplete-popup .item[data-kind="batch"] .kind { color: var(--code); }
      .autocomplete-popup .item[data-kind="streaming_table"] .kind { color: #7dd3fc; background: rgba(125,211,252,0.12); }
      .autocomplete-popup .item[data-kind="materialized_view"] .kind { color: #c4b5fd; background: rgba(196,181,253,0.12); }
      .autocomplete-popup .item[data-kind="column"] .kind { color: var(--accent); }
      .autocomplete-popup .item[data-kind="function"] .kind { color: var(--success); }
      .autocomplete-popup .hint {
        padding: 6px 10px;
        color: var(--muted);
        font-size: 10px;
        border-top: 1px solid var(--border);
        margin-top: 4px;
      }
    </style>
  </head>
  <body data-cluster="">
    <header>
      <div>
        <h1>rspark dashboard</h1>
        <div class="meta">
          <span id="master-id">master: …</span>
          <span id="captured-at" class="auto-refresh">last refresh: never</span>
        </div>
      </div>
      <div class="meta">
        <span id="round-counter">runs: 0</span>
      </div>
    </header>
    <nav class="tabs" id="tabs">
      <button data-tab="sql-lab" class="active">SQL Lab</button>
      <button data-tab="cluster">Cluster <span class="badge" id="badge-running" style="display:none;">0</span></button>
      <button data-tab="pipelines">Pipelines <span class="badge" id="badge-pipelines" style="display:none;">0</span></button>
    </nav>
    <div class="layout" id="layout-sql-lab">
      <aside class="sidebar">
        <section>
          <h2>Tables <button class="secondary" id="refresh-tables" style="padding:2px 8px;font-size:10px;">refresh</button></h2>
          <ul class="table-list" id="tables"></ul>
          <div class="add-table">
            <div class="row">
              <input id="add-name" placeholder="name" />
              <input id="add-path" placeholder="/path/to/file.csv" />
            </div>
            <button id="add-table-btn">register table</button>
          </div>
        </section>
        <section>
          <h2>Suggestions</h2>
          <div class="suggest" id="suggestions"></div>
        </section>
      </aside>
      <main>
        <section>
          <h2>SQL editor</h2>
          <div class="editor">
            <div style="position:relative;">
              <span id="mirror" aria-hidden="true" style="position:absolute;visibility:hidden;white-space:pre;font-family:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,monospace;font-size:13px;line-height:1.5;padding:0 14px;top:0;left:0;"></span>
              <textarea id="sql" spellcheck="false" placeholder="SELECT * FROM employees LIMIT 10">SELECT * FROM employees LIMIT 10</textarea>
              <div id="autocomplete" class="autocomplete-popup" style="display:none;"></div>
            </div>
            <div class="editor-row">
              <button id="run-sql">Run (Ctrl+Enter)</button>
              <button class="secondary" id="format-sql">format</button>
              <button class="secondary" id="clear-sql">clear</button>
              <button class="secondary" id="open-collector" title="Open the click/collect demo page in a new tab. Clicks there are sent to rspark-ingest → Kafka → click_events, so a streaming-table join query will grow as you click.">open event collector ↗</button>
              <label class="secondary" id="live-refresh-wrap" title="Re-run the current query every 1.5 s. Useful with the event collector to watch a streaming-⨯-batch join grow in real time." style="display:inline-flex;align-items:center;gap:4px;padding:6px 10px;">
                <input type="checkbox" id="live-refresh" />
                live refresh
              </label>
              <span class="auto-refresh" id="run-status"></span>
              <span style="margin-left:auto;color:var(--muted);font-size:11px;">type to see suggestions · Tab/Enter accepts · Ctrl+Space forces</span>
            </div>
            <div class="history" id="history"></div>
          </div>
        </section>
        <section>
          <h2>Examples — click to load into the editor</h2>
          <div class="samples samples-prominent">
            <button class="example-pill" data-sql="SELECT * FROM employees LIMIT 5">employees*5</button>
            <button class="example-pill" data-sql="SELECT dept, AVG(salary) AS avg_sal, COUNT(*) AS n FROM employees GROUP BY dept ORDER BY avg_sal DESC">avg salary / dept</button>
            <button class="example-pill" data-sql="SELECT e.name, SUM(s.amount) AS total FROM employees e LEFT JOIN sales s ON e.id = s.id GROUP BY e.name ORDER BY total DESC">top earners+ sales</button>
            <button class="example-pill" data-sql="SELECT product, region, SUM(amount) AS total FROM sales GROUP BY product, region ORDER BY product, region">sales by product+region</button>
            <button class="example-pill" data-sql="SELECT event, COUNT(*) AS n FROM events GROUP BY event ORDER BY n DESC">events by type</button>
            <button class="example-pill" data-sql="SHOW CREATE TABLE employees">SHOW CREATE TABLE</button>
            <button class="example-pill example-stream" data-sql="SELECT c.ts, c.event_type, c.url, u.name AS user_name, u.email, u.country AS signup_country FROM click_events c LEFT JOIN users u ON c.user_id = u.user_id WHERE c.event_type = 'page_view' ORDER BY c.ts LIMIT 20">stream × batch join</button>
            <button class="example-pill example-stream" data-sql="SELECT u.country AS signup_country, COUNT(*) AS page_views FROM click_events c JOIN users u ON c.user_id = u.user_id WHERE c.event_type = 'page_view' GROUP BY u.country ORDER BY page_views DESC">page views / signup country</button>
          </div>
        </section>
        <section>
          <h2>Execution metrics</h2>
          <div class="exec-strip">
            <div class="stat"><div class="label">Total runs</div><div class="value" id="m-total">0</div></div>
            <div class="stat"><div class="label">Succeeded</div><div class="value" id="m-ok" style="color:var(--success);">0</div></div>
            <div class="stat"><div class="label">Failed</div><div class="value" id="m-fail" style="color:var(--danger);">0</div></div>
            <div class="stat"><div class="label">Avg duration</div><div class="value small" id="m-avg">—</div></div>
            <div class="stat"><div class="label">Last query</div><div class="value small" id="m-last">—</div></div>
          </div>
        </section>
        <section>
          <h2>Result</h2>
          <div id="result-meta" class="result-meta" style="display:none;"></div>
          <div id="result-error" style="display:none;"></div>
          <div id="result-empty" class="empty">no query has been run yet</div>
          <div id="result-table" class="result-table-wrap" style="display:none;"></div>
        </section>
      </main>
    </div>
    <div class="layout no-sidebar" id="layout-cluster" style="display:none;">
      <main>
        <section>
          <h2>Cluster summary</h2>
          <div class="stat-grid">
            <div class="stat"><div class="label">Workers</div><div class="value" id="stat-workers">0</div></div>
            <div class="stat"><div class="label">Alive</div><div class="value" id="stat-workers-alive" style="color:var(--success);">0</div></div>
            <div class="stat"><div class="label">Total cores</div><div class="value" id="stat-cores">0</div></div>
            <div class="stat"><div class="label">Total memory</div><div class="value small" id="stat-mem">0 MB</div></div>
          </div>
        </section>
        <section>
          <h2>Jobs <span class="count" id="jobs-count">0</span></h2>
          <div id="jobs-active"></div>
        </section>
        <section>
          <h2>Recent jobs</h2>
          <div id="jobs-recent"></div>
        </section>
        <section>
          <h2>SQL currently running</h2>
          <div id="running-sql"></div>
        </section>
        <section>
          <h2>Stages</h2>
          <div id="stages"></div>
        </section>
        <section>
          <h2>Workers <span class="count" id="workers-count">0</span></h2>
          <div id="workers"></div>
        </section>
        <section>
          <h2>Tasks <span class="count" id="tasks-count">0</span></h2>
          <div id="tasks"></div>
        </section>
      </main>
    </div>
    <div class="layout" id="layout-pipelines" style="display:none;grid-template-columns: 320px 1fr;">
      <aside class="sidebar">
        <section>
          <h2>Pipelines</h2>
          <ul class="table-list" id="pipelines-list"></ul>
          <div class="add-table">
            <h3 style="font-size:11px;color:var(--text-dim);text-transform:uppercase;letter-spacing:0.04em;margin:8px 0 4px;">Submit YAML</h3>
            <textarea id="pipeline-yaml" spellcheck="false" placeholder="pipeline: my_pipe&#10;flows:&#10;  - name: a&#10;    kind: materialized_view&#10;    source: { kind: sql }&#10;    query: 'SELECT 1'&#10;    destination: { kind: file, path: /tmp/a.csv }" style="min-height:140px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:11px;"></textarea>
            <button id="submit-pipeline-btn" style="margin-top:6px;">run pipeline</button>
            <div id="pipeline-status" style="font-size:11px;color:var(--text-dim);margin-top:6px;"></div>
          </div>
        </section>
      </aside>
      <main>
        <section>
          <h2>DAG <span id="dag-name" style="color:var(--text-dim);font-weight:400;"></span></h2>
          <svg id="dag-svg" width="100%" height="500" style="background:var(--bg);border:1px solid var(--border);border-radius:4px;"></svg>
        </section>
      </main>
    </div>
    <div class="footer">
      rspark — a small Spark-compatible engine in Rust.
    </div>
    <script>
      const STATUS_CLASSES = {
        Running: "running", Succeeded: "success", Failed: "failed",
        Cancelled: "failed", Pending: "pending", Assigned: "assigned",
        Alive: "alive", Dead: "dead",
      };
      const HISTORY_KEY = "rspark.sql.history.v1";
      const HISTORY_MAX = 8;
      const ACTIVE_TAB_KEY = "rspark.activeTab.v1";
      const EXEC_KEY = "rspark.execStats.v1";
      const MAX_SUGGESTIONS = 80;

      const ExecState = {
        total: 0,
        ok: 0,
        fail: 0,
        totalMs: 0,
        lastSql: "",
        lastDurationMs: 0,
        lastAt: null,
      };

      function loadExec() {
        try { return Object.assign(ExecState, JSON.parse(localStorage.getItem(EXEC_KEY) || "{}")); }
        catch { return ExecState; }
      }
      function saveExec() {
        localStorage.setItem(EXEC_KEY, JSON.stringify({
          total: ExecState.total, ok: ExecState.ok, fail: ExecState.fail,
          totalMs: ExecState.totalMs, lastSql: ExecState.lastSql,
          lastDurationMs: ExecState.lastDurationMs, lastAt: ExecState.lastAt,
        }));
      }
      function recordExec(ok, durationMs, sql) {
        ExecState.total += 1;
        if (ok) ExecState.ok += 1; else ExecState.fail += 1;
        ExecState.totalMs += durationMs;
        ExecState.lastSql = sql;
        ExecState.lastDurationMs = durationMs;
        ExecState.lastAt = new Date().toISOString();
        saveExec();
        renderExec();
      }

      function escapeHtml(text) {
        if (text === undefined || text === null) return "";
        return String(text)
          .replaceAll("&", "&amp;")
          .replaceAll("<", "&lt;")
          .replaceAll(">", "&gt;")
          .replaceAll('"', "&quot;");
      }
      function fmtDuration(start, end) {
        if (!start || !end) return "—";
        const ms = new Date(end) - new Date(start);
        if (ms < 1000) return ms + " ms";
        if (ms < 60000) return (ms / 1000).toFixed(2) + " s";
        return (ms / 60000).toFixed(2) + " min";
      }
      function fmtRelative(iso) {
        if (!iso) return "—";
        const ms = Date.now() - new Date(iso).getTime();
        if (ms < 1000) return "just now";
        if (ms < 60000) return Math.floor(ms / 1000) + "s ago";
        if (ms < 3600000) return Math.floor(ms / 60000) + "m ago";
        if (ms < 86400000) return Math.floor(ms / 3600000) + "h ago";
        return new Date(iso).toLocaleString();
      }
      function isNumeric(v) {
        return typeof v === "number" && Number.isFinite(v);
      }
      function fmtCell(v) {
        if (v === null || v === undefined) return '<span class="null-cell">null</span>';
        if (typeof v === "object") return escapeHtml(JSON.stringify(v));
        return escapeHtml(v);
      }

      // --- Tabs ---
      function setTab(name) {
        localStorage.setItem(ACTIVE_TAB_KEY, name);
        document.querySelectorAll("nav.tabs button").forEach(b => {
          b.classList.toggle("active", b.dataset.tab === name);
        });
        document.getElementById("layout-sql-lab").style.display = name === "sql-lab" ? "grid" : "none";
        document.getElementById("layout-cluster").style.display = name === "cluster" ? "grid" : "none";
        document.getElementById("layout-pipelines").style.display = name === "pipelines" ? "grid" : "none";
        if (name === "pipelines") refreshPipelines();
      }
      document.querySelectorAll("nav.tabs button").forEach(b => {
        b.addEventListener("click", () => setTab(b.dataset.tab));
      });
      setTab(localStorage.getItem(ACTIVE_TAB_KEY) || "sql-lab");

      // --- Exec metrics ---
      function renderExec() {
        document.getElementById("m-total").textContent = ExecState.total;
        document.getElementById("m-ok").textContent = ExecState.ok;
        document.getElementById("m-fail").textContent = ExecState.fail;
        const avg = ExecState.total === 0 ? "—"
          : (ExecState.totalMs / ExecState.total) < 1
            ? (ExecState.totalMs / ExecState.total).toFixed(2) + " ms"
            : Math.round(ExecState.totalMs / ExecState.total) + " ms";
        document.getElementById("m-avg").textContent = avg;
        document.getElementById("m-last").textContent = ExecState.lastSql
          ? `${ExecState.lastDurationMs} ms · ${fmtRelative(ExecState.lastAt)}`
          : "—";
      }
      loadExec();
      renderExec();

      // --- Cluster snapshot ---
      async function refresh() {
        try {
          const res = await fetch("/v1/cluster/snapshot");
          if (!res.ok) {
            document.getElementById("captured-at").textContent = "snapshot unavailable (HTTP " + res.status + ")";
            return;
          }
          const snap = await res.json();
          document.getElementById("master-id").textContent = "master: " + snap.master_id;
          document.getElementById("captured-at").textContent = "last refresh: " + new Date(snap.captured_at).toLocaleString();
          document.getElementById("stat-workers").textContent = snap.workers.length;
          document.getElementById("round-counter").textContent = "runs: " + snap.total_runs;

          const alive = snap.workers.filter(w => w.status === "Alive").length;
          const totalCores = snap.workers.reduce((a, w) => a + (w.cores || 0), 0);
          const totalMem = snap.workers.reduce((a, w) => a + (w.memory_mb || 0), 0);
          document.getElementById("stat-workers-alive").textContent = alive;
          document.getElementById("stat-cores").textContent = totalCores;
          document.getElementById("stat-mem").textContent = totalMem + " MB";

          const active = snap.jobs.filter(j => j.status === "Running");
          const recent = [...snap.jobs].sort((a, b) => new Date(b.submitted_at) - new Date(a.submitted_at)).slice(0, 8);
          document.getElementById("jobs-count").textContent = snap.jobs.length;
          document.getElementById("workers-count").textContent = snap.workers.length;
          document.getElementById("tasks-count").textContent = snap.tasks.length;
          const badge = document.getElementById("badge-running");
          if (active.length > 0) {
            badge.style.display = "inline-block";
            badge.textContent = active.length;
          } else {
            badge.style.display = "none";
          }
          renderJobs(document.getElementById("jobs-active"), active, "no active jobs");
          renderJobs(document.getElementById("jobs-recent"), recent, "no jobs submitted yet");

          const runningJobs = snap.jobs.filter(j => j.status === "Running");
          const sqlHtml = runningJobs.length === 0
            ? '<div class="empty">no queries are currently running</div>'
            : runningJobs.map(j => '<pre>' + escapeHtml(j.sql) + '</pre>').join("");
          document.getElementById("running-sql").innerHTML = sqlHtml;

          renderStages(document.getElementById("stages"), snap.stages);
          renderWorkers(document.getElementById("workers"), snap.workers);
          renderTasks(document.getElementById("tasks"), snap.tasks);
        } catch (err) {
          document.getElementById("captured-at").textContent = "error: " + err.message;
        }
      }
      function renderJobs(host, jobs, emptyMsg) {
        if (!jobs.length) { host.innerHTML = '<div class="empty">' + emptyMsg + '</div>'; return; }
        host.innerHTML = `
          <table>
            <thead>
              <tr>
                <th>Name</th><th>Status</th><th>Duration</th><th class="num">Rows</th><th>Submitted</th>
              </tr>
            </thead>
            <tbody>
              ${jobs.map(j => `
                <tr>
                  <td>${escapeHtml(j.name)}<br><span style="color:var(--muted);font-size:11px">${escapeHtml(j.id.slice(0,8))}</span></td>
                  <td><span class="pill ${STATUS_CLASSES[j.status] || ''}">${escapeHtml(j.status)}</span></td>
                  <td>${fmtDuration(j.started_at, j.completed_at)}</td>
                  <td class="num">${j.result_rows ?? "—"}</td>
                  <td>${new Date(j.submitted_at).toLocaleTimeString()}</td>
                </tr>`).join("")}
            </tbody>
          </table>`;
      }
      function renderStages(host, stages) {
        if (!stages.length) { host.innerHTML = '<div class="empty">no stages</div>'; return; }
        host.innerHTML = `
          <table>
            <thead><tr><th>Stage</th><th>Status</th><th class="num">Tasks</th><th>Progress</th></tr></thead>
            <tbody>
              ${stages.map(s => {
                const done = s.tasks.filter(t => t.status === "Succeeded" || (typeof t.status === "object" && t.status.Failed !== undefined)).length;
                const total = s.tasks.length;
                const pct = total === 0 ? 0 : Math.round(done / total * 100);
                return `
                <tr>
                  <td>${escapeHtml(s.label)}<br><span style="color:var(--muted);font-size:11px">${escapeHtml(s.id.slice(0,8))}</span></td>
                  <td><span class="pill ${STATUS_CLASSES[s.status] || (typeof s.status === 'object' && s.status.Failed !== undefined ? 'failed' : '')}">${escapeHtml(typeof s.status === 'string' ? s.status : 'Failed')}</span></td>
                  <td class="num">${total}</td>
                  <td>
                    <div style="background:rgba(255,255,255,0.06);border-radius:6px;height:6px;width:120px;">
                      <div style="background:var(--accent);height:100%;width:${pct}%;border-radius:6px;"></div>
                    </div>
                    <span style="color:var(--muted);font-size:11px;margin-left:6px">${done}/${total}</span>
                  </td>
                </tr>`;
              }).join("")}
            </tbody>
          </table>`;
      }
      function renderWorkers(host, workers) {
        if (!workers.length) { host.innerHTML = '<div class="empty">no workers registered</div>'; return; }
        host.innerHTML = `
          <table>
            <thead><tr><th>Worker</th><th>Status</th><th class="num">Cores</th><th>Mem</th><th class="num">Running</th><th>Last heartbeat</th></tr></thead>
            <tbody>
              ${workers.map(w => `
                <tr>
                  <td>${escapeHtml(w.address)}<br><span style="color:var(--muted);font-size:11px">${escapeHtml(w.id.slice(0,8))}</span></td>
                  <td><span class="pill ${STATUS_CLASSES[w.status] || ''}">${escapeHtml(w.status)}</span></td>
                  <td class="num">${w.cores}</td>
                  <td>${w.memory_mb} MB</td>
                  <td class="num">${w.running_tasks.length}</td>
                  <td>${fmtRelative(w.last_heartbeat)}</td>
                </tr>`).join("")}
            </tbody>
          </table>`;
      }
      function renderTasks(host, tasks) {
        if (!tasks.length) { host.innerHTML = '<div class="empty">no tasks yet</div>'; return; }
        const sorted = [...tasks].sort((a, b) => (b.started_at ? new Date(b.started_at) : 0) - (a.started_at ? new Date(a.started_at) : 0)).slice(0, 50);
        host.innerHTML = `
          <table>
            <thead><tr><th>Task</th><th>Status</th><th>Worker</th><th class="num">Rows</th><th>Duration</th></tr></thead>
            <tbody>
              ${sorted.map(t => `
                <tr>
                  <td>${escapeHtml(t.partition_label)}<br><span style="color:var(--muted);font-size:11px">${escapeHtml(t.id.slice(0,8))}</span></td>
                  <td><span class="pill ${typeof t.status === 'string' ? STATUS_CLASSES[t.status] || '' : 'failed'}">${typeof t.status === 'string' ? escapeHtml(t.status) : 'Failed'}</span></td>
                  <td>${escapeHtml(t.assigned_worker || '—')}</td>
                  <td class="num">${t.result_rows ?? '—'}</td>
                  <td>${fmtDuration(t.started_at, t.completed_at)}</td>
                </tr>`).join("")}
            </tbody>
          </table>`;
      }

      // --- Suggestions & autocomplete ---
      const SuggState = {
        tables: [],          // [{name, kind}] — kind is "batch" | "streaming_table" | "materialized_view"
        columns: [],
        functions: [],
        keywords: [],
      };

      async function refreshSuggestions() {
        try {
          const res = await fetch("/v1/catalog/suggestions");
          if (!res.ok) return;
          const data = await res.json();
          // Backwards-compat: the old shape was a plain string list.
          // New shape is `{name, kind}`. Normalise to the new shape so
          // the rest of the renderer can stay simple.
          SuggState.tables = (data.tables || []).map(t =>
            typeof t === "string" ? { name: t, kind: "batch" } : t
          );
          SuggState.columns = data.columns || [];
          SuggState.functions = data.functions || [];
          SuggState.keywords = data.keywords || [];
          renderSuggestions();
        } catch (err) {
          // ignore
        }
      }
      function tableKindLabel(kind) {
        if (kind === "streaming_table") return "stream";
        if (kind === "materialized_view") return "view";
        return "table";
      }
      function renderSuggestions() {
        const host = document.getElementById("suggestions");
        const batch = SuggState.tables.filter(t => t.kind === "batch");
        const streaming = SuggState.tables.filter(t => t.kind === "streaming_table");
        const views = SuggState.tables.filter(t => t.kind === "materialized_view");
        const groups = [
          { label: "streaming tables", items: streaming, cls: "table streaming", kind: "streaming_table" },
          { label: "tables", items: batch, cls: "table", kind: "batch" },
          { label: "materialized views", items: views, cls: "table view", kind: "materialized_view" },
          { label: "functions", items: SuggState.functions, cls: "function", kind: "function" },
          { label: "columns", items: SuggState.columns, cls: "column", kind: "column" },
          { label: "keywords", items: SuggState.keywords, cls: "keyword", kind: "keyword" },
        ];
        host.innerHTML = groups
          .filter(g => g.items.length > 0)
          .map(g => {
            const items = g.items.slice(0, 16).map(it => {
              const name = typeof it === "string" ? it : it.name;
              const sub = typeof it === "string" ? g.kind : (it.kind || g.kind);
              return `<button class="${g.cls}" data-insert="${escapeHtml(name)}" title="${escapeHtml(name)} (${sub})">${escapeHtml(name)}<span class="kind">${tableKindLabel(sub)}</span></button>`;
            }).join("");
            const more = g.items.length > 16 ? `<span style="color:var(--muted);font-size:10px;">+${g.items.length - 16}</span>` : "";
            return `<div class="group"><span class="group-label">${g.label}</span>${items}${more}</div>`;
          }).join("") || '<div class="empty" style="width:100%;">no suggestions yet</div>';
        host.querySelectorAll("button[data-insert]").forEach(b => {
          b.addEventListener("click", () => insertAtCursor(b.dataset.insert));
        });
      }

      function insertAtCursor(text) {
        const ta = document.getElementById("sql");
        const start = ta.selectionStart ?? ta.value.length;
        const end = ta.selectionEnd ?? ta.value.length;
        const before = ta.value.slice(0, start);
        const after = ta.value.slice(end);
        const needsSpace = before.length > 0 && !/[\s\(\,]$/.test(before);
        const insertion = (needsSpace ? " " : "") + text;
        ta.value = before + insertion + after;
        const cursor = start + insertion.length;
        ta.focus();
        ta.setSelectionRange(cursor, cursor);
      }

      // --- Autocomplete popup ---
      const AcState = { items: [], selected: 0, open: false, anchor: null, start: 0 };

      function currentToken(textarea) {
        const cursor = textarea.selectionStart;
        const before = textarea.value.slice(0, cursor);
        const match = /([A-Za-z_][A-Za-z0-9_]*)$/.exec(before);
        return { token: match ? match[1].toLowerCase() : "", start: match ? cursor - match[1].length : cursor, cursor };
      }
      function allCandidates() {
        return [
          ...SuggState.tables.map(t => ({ name: t.name, kind: t.kind })),
          ...SuggState.columns.map(n => ({ name: n, kind: "column" })),
          ...SuggState.functions.map(n => ({ name: n, kind: "function" })),
          ...SuggState.keywords.map(n => ({ name: n, kind: "keyword" })),
        ];
      }
      function priorityKind(kind) {
        // Streaming tables and materialized views float to the top when
        // the user is typing a table name — they're the "interesting"
        // ones (vs. static batch tables).
        if (kind === "streaming_table") return 0;
        if (kind === "materialized_view") return 1;
        if (kind === "batch") return 2;
        if (kind === "column") return 3;
        if (kind === "function") return 4;
        return 5;
      }
      function updateAutocomplete() {
        const ta = document.getElementById("sql");
        const popup = document.getElementById("autocomplete");
        const { token, start, cursor } = currentToken(ta);
        if (!token || token.length < 1) {
          popup.style.display = "none";
          AcState.open = false;
          return;
        }
        const candidates = allCandidates();
        const matches = candidates
          .filter(it => it.name.toLowerCase().startsWith(token))
          .sort((a, b) => {
            const aExact = a.name.toLowerCase() === token;
            const bExact = b.name.toLowerCase() === token;
            if (aExact !== bExact) return aExact ? -1 : 1;
            const ap = priorityKind(a.kind) - priorityKind(b.kind);
            if (ap !== 0) return ap;
            return a.name.localeCompare(b.name);
          })
          .slice(0, 10);
        if (matches.length === 0) {
          popup.innerHTML = `<div class="hint">no matches for "${escapeHtml(token)}"</div>`;
          popup.style.display = "block";
          positionPopup(ta, popup, cursor);
          AcState.open = true;
          AcState.items = [];
          AcState.start = start;
          return;
        }
        AcState.items = matches;
        AcState.selected = 0;
        AcState.open = true;
        AcState.start = start;
        popup.innerHTML = matches.map((m, i) => `
          <div class="item${i === 0 ? " active" : ""} kind-${escapeHtml(m.kind)}" data-idx="${i}" data-kind="${escapeHtml(m.kind)}">
            <span>${escapeHtml(m.name)}</span><span class="kind">${escapeHtml(tableKindLabel(m.kind))}</span>
          </div>
          <div class="hint">↑↓ navigate · Tab/Enter accept · Esc dismiss</div>
        `).join("");
        popup.querySelectorAll(".item").forEach(el => {
          el.addEventListener("mousedown", (ev) => {
            ev.preventDefault();
            const idx = Number(el.dataset.idx);
            acceptAutocomplete(idx);
          });
          el.addEventListener("mouseenter", () => {
            AcState.selected = Number(el.dataset.idx);
            popup.querySelectorAll(".item").forEach(e => e.classList.toggle("active", e === el));
          });
        });
        popup.style.display = "block";
        positionPopup(ta, popup, cursor);
      }
      function positionPopup(textarea, popup, cursor) {
        const rect = textarea.getBoundingClientRect();
        const style = window.getComputedStyle(textarea);
        const lineHeight = parseFloat(style.lineHeight) || 20;
        const paddingTop = parseFloat(style.paddingTop) || 12;
        const paddingLeft = parseFloat(style.paddingLeft) || 14;
        const before = textarea.value.slice(0, cursor);
        const lineOffset = (before.match(/\n/g) || []).length;
        const lastLine = before.split("\n").pop() || "";
        const charWidth = 8.4;
        // Mirror element handles the absolute positioning within the relative wrapper
        const mirror = document.getElementById("mirror");
        if (mirror) {
          const tokens = before.split("\n");
          const last = tokens[tokens.length - 1] || "";
          mirror.textContent = last;
          const mirrorRect = mirror.getBoundingClientRect();
          const wrapperRect = textarea.parentElement.getBoundingClientRect();
          const top = (mirrorRect.bottom - wrapperRect.top) + 2;
          const left = (mirrorRect.right - wrapperRect.left);
          popup.style.top = top + "px";
          popup.style.left = Math.min(left, wrapperRect.width - 240) + "px";
        } else {
          const top = paddingTop + (lineOffset + 1) * lineHeight;
          const left = paddingLeft + lastLine.length * charWidth;
          popup.style.top = top + "px";
          popup.style.left = Math.min(left, textarea.clientWidth - 200) + "px";
        }
      }
      function acceptAutocomplete(idx) {
        const ta = document.getElementById("sql");
        const item = AcState.items[idx];
        if (!item) return;
        const before = ta.value.slice(0, AcState.start);
        const cursor = ta.selectionStart;
        const after = ta.value.slice(cursor);
        const insertion = (item.kind === "function" ? item.name + "(" : item.name);
        ta.value = before + insertion + after;
        const caret = before.length + insertion.length;
        ta.focus();
        ta.setSelectionRange(caret, caret);
        document.getElementById("autocomplete").style.display = "none";
        AcState.open = false;
        updateAutocomplete();
      }
      function closeAutocomplete() {
        document.getElementById("autocomplete").style.display = "none";
        AcState.open = false;
      }

      // --- SQL editor & result ---
      function loadHistory() {
        try { return JSON.parse(localStorage.getItem(HISTORY_KEY) || "[]"); }
        catch { return []; }
      }
      function saveHistory(sql) {
        const h = loadHistory().filter(s => s !== sql);
        h.unshift(sql);
        localStorage.setItem(HISTORY_KEY, JSON.stringify(h.slice(0, HISTORY_MAX)));
        renderHistory();
      }
      function renderHistory() {
        const host = document.getElementById("history");
        const items = loadHistory();
        if (!items.length) { host.innerHTML = ""; return; }
        host.innerHTML = items
          .map((s, i) => `<button data-history="${i}" title="${escapeHtml(s)}">${escapeHtml(s)}</button>`)
          .join("");
        host.querySelectorAll("button[data-history]").forEach(b => {
          b.addEventListener("click", () => {
            document.getElementById("sql").value = loadHistory()[Number(b.dataset.history)];
          });
        });
      }

      async function runSql() {
        const textarea = document.getElementById("sql");
        const sql = textarea.value.trim();
        if (!sql) return;
        const btn = document.getElementById("run-sql");
        const status = document.getElementById("run-status");
        btn.disabled = true;
        status.textContent = "running…";
        const t0 = performance.now();
        try {
          const res = await fetch("/v1/sql", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ sql, job_name: "dashboard" }),
          });
          const payload = await res.json();
          const durationMs = payload.duration_ms ?? Math.round(performance.now() - t0);
          if (!res.ok) {
            renderError(payload.error || "request failed", payload.kind);
            recordExec(false, durationMs, sql);
          } else {
            renderResult(payload);
            saveHistory(sql);
            recordExec(true, durationMs, sql);
          }
          status.textContent = "";
        } catch (err) {
          renderError(err.message || String(err));
          recordExec(false, Math.round(performance.now() - t0), sql);
          status.textContent = "";
        } finally {
          btn.disabled = false;
        }
      }

      function renderResult(payload) {
        const meta = document.getElementById("result-meta");
        const err = document.getElementById("result-error");
        const empty = document.getElementById("result-empty");
        const tableHost = document.getElementById("result-table");
        err.style.display = "none";
        empty.style.display = "none";
        meta.style.display = "flex";
        meta.innerHTML = `
          <span><strong>${payload.row_count}</strong> row${payload.row_count === 1 ? "" : "s"}</span>
          <span><strong>${payload.duration_ms} ms</strong></span>
          <span>job: <code>${escapeHtml(payload.job.id.slice(0, 8))}</code> <span class="pill ${STATUS_CLASSES[payload.job.status] || ''}">${escapeHtml(payload.job.status)}</span></span>
          <span>columns: ${payload.columns.map(c => `<code>${escapeHtml(c.name)}</code>`).join(", ")}</span>
        `;
        if (payload.row_count === 0) {
          tableHost.style.display = "none";
          tableHost.innerHTML = "";
          empty.style.display = "block";
          empty.textContent = "query returned 0 rows";
          return;
        }
        const numericCols = new Set(
          payload.columns
            .map((c, i) => ({ name: c.name, idx: i, t: c.data_type }))
            .filter(c => /Int|Float|Double|Decimal/.test(c.t))
            .map(c => c.idx)
        );
        const preview = payload.rows.slice(0, 200);
        const colsHtml = payload.columns
          .map((c, i) => `<th${numericCols.has(i) ? ' class="num"' : ""}>${escapeHtml(c.name)} <span style="color:var(--muted);font-weight:400;">${escapeHtml(c.data_type)}</span></th>`)
          .join("");
        const rowsHtml = preview
          .map(row => `<tr>${row
            .map((v, i) => `<td${numericCols.has(i) ? ' class="num"' : ""}>${fmtCell(v)}</td>`)
            .join("")}</tr>`)
          .join("");
        tableHost.style.display = "block";
        tableHost.innerHTML = `
          <table>
            <thead><tr>${colsHtml}</tr></thead>
            <tbody>${rowsHtml}</tbody>
          </table>
        `;
      }

      function renderError(message, kind) {
        const meta = document.getElementById("result-meta");
        const err = document.getElementById("result-error");
        const empty = document.getElementById("result-empty");
        const tableHost = document.getElementById("result-table");
        meta.style.display = "none";
        tableHost.style.display = "none";
        tableHost.innerHTML = "";
        empty.style.display = "none";
        err.style.display = "block";
        err.className = "result-error";
        err.textContent = (kind ? `[${kind}] ` : "") + (message || "unknown error");
      }

      // --- Tables ---
      async function refreshTables() {
        try {
          const res = await fetch("/v1/catalog/tables");
          if (!res.ok) return;
          const tables = await res.json();
          renderTables(tables);
        } catch (err) {
          // ignore
        }
      }
      function renderTables(tables) {
        const host = document.getElementById("tables");
        if (!tables.length) { host.innerHTML = '<li class="empty" style="background:transparent;border:0;padding:4px 0;">no tables — register one below</li>'; return; }
        host.innerHTML = tables.map(t => `
          <li data-name="${escapeHtml(t.name)}" title="click to insert '${escapeHtml(t.name)}' into the editor">
            <div class="name">${escapeHtml(t.name)}</div>
            <div class="meta">${escapeHtml(t.source)} · ${escapeHtml(t.path)}</div>
            <div class="meta">${t.columns.length} column${t.columns.length === 1 ? "" : "s"}: ${t.columns.map(c => escapeHtml(c.name)).join(", ")}</div>
            <button class="remove" data-name="${escapeHtml(t.name)}">remove</button>
          </li>`).join("");
        host.querySelectorAll("li[data-name]").forEach(li => {
          li.addEventListener("click", (ev) => {
            if (ev.target.closest("button.remove")) return;
            insertAtCursor(li.dataset.name);
          });
        });
        host.querySelectorAll("button.remove").forEach(b => {
          b.addEventListener("click", async (ev) => {
            ev.stopPropagation();
            const name = b.dataset.name;
            await fetch(`/v1/catalog/tables/${encodeURIComponent(name)}`, { method: "DELETE" });
            refreshTables();
            refreshSuggestions();
          });
        });
      }
      async function addTable() {
        const name = document.getElementById("add-name").value.trim();
        const path = document.getElementById("add-path").value.trim();
        if (!name || !path) return;
        const res = await fetch("/v1/catalog/tables", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name, path }),
        });
        if (res.ok) {
          document.getElementById("add-name").value = "";
          document.getElementById("add-path").value = "";
          refreshTables();
          refreshSuggestions();
        } else {
          const err = await res.json().catch(() => ({}));
          alert("register failed: " + (err.error || res.status));
        }
      }

      function formatSql(sql) {
        const keywords = ["SELECT","FROM","WHERE","GROUP BY","ORDER BY","HAVING","LIMIT","LEFT JOIN","RIGHT JOIN","INNER JOIN","JOIN","ON","AND","OR","BY","ASC","DESC"];
        let out = sql.replace(/\s+/g, " ").trim();
        for (const kw of keywords) {
          const re = new RegExp("\\b" + kw.replace(" ", "\\s+") + "\\b", "gi");
          out = out.replace(re, kw);
        }
        // Uppercase keywords
        for (const kw of keywords) {
          const re = new RegExp("(^|\\s)(" + kw + ")(\\s|$|,)", "g");
          out = out.replace(re, (m, p1, p2, p3) => p1 + p2.toUpperCase() + p3);
        }
        out = out
          .replace(/\s*,\s*/g, ",\n  ")
          .replace(/\b(SELECT|FROM|WHERE|GROUP BY|ORDER BY|HAVING|LIMIT|LEFT JOIN|RIGHT JOIN|INNER JOIN|JOIN|ON)\b/g, "\n$1")
          .replace(/^\n+/, "")
          .replace(/^/, "  ");
        return out.replace(/\n/g, "\n  ");
      }

      document.getElementById("run-sql").addEventListener("click", runSql);
      document.getElementById("clear-sql").addEventListener("click", () => {
        document.getElementById("sql").value = "";
        document.getElementById("sql").focus();
      });
      document.getElementById("format-sql").addEventListener("click", () => {
        const ta = document.getElementById("sql");
        ta.value = formatSql(ta.value);
      });
      document.getElementById("open-collector").addEventListener("click", () => {
        // The demo page reads window.RSPARK_INGEST_URL but defaults to
        // 127.0.0.1:8081 — that's where scripts/port-forward.sh forwards
        // rspark-ingest. The page also lives at the dashboard's own
        // origin, so it can fetch relative URLs without CORS issues.
        window.open("/examples/e2e/demo_page.html", "_blank", "noopener");
      });

      // Live refresh: re-run the current query every 1.5 s while the
      // checkbox is on. Pairs with the event collector to make a
      // streaming-⨯-batch join visibly grow as new events arrive.
      let liveTimer = null;
      const liveBox = document.getElementById("live-refresh");
      const liveStatus = document.getElementById("run-status");
      function stopLive() {
        if (liveTimer !== null) {
          clearInterval(liveTimer);
          liveTimer = null;
        }
        liveBox.checked = false;
        liveStatus.textContent = "";
      }
      liveBox.addEventListener("change", () => {
        if (liveBox.checked) {
          runSql();
          liveTimer = setInterval(runSql, 1500);
          liveStatus.textContent = "live · every 1.5 s";
        } else {
          stopLive();
        }
      });
      // Stop live refresh when the user manually edits the SQL — they'd
      // expect their changes to land before the next auto-run.
      document.getElementById("sql").addEventListener("input", () => {
        if (liveTimer !== null) {
          stopLive();
        }
      });
      document.getElementById("add-table-btn").addEventListener("click", addTable);
      document.getElementById("refresh-tables").addEventListener("click", () => {
        refreshTables();
        refreshSuggestions();
      });
      document.getElementById("sql").addEventListener("keydown", (ev) => {
        if ((ev.metaKey || ev.ctrlKey) && ev.key === "Enter") {
          ev.preventDefault();
          runSql();
        } else if (ev.key === "Escape" && AcState.open) {
          closeAutocomplete();
          ev.preventDefault();
        } else if ((ev.key === "ArrowDown" || ev.key === "ArrowUp") && AcState.open) {
          ev.preventDefault();
          const delta = ev.key === "ArrowDown" ? 1 : -1;
          AcState.selected = (AcState.selected + delta + AcState.items.length) % AcState.items.length;
          document.querySelectorAll("#autocomplete .item").forEach((el, i) => {
            el.classList.toggle("active", i === AcState.selected);
          });
          const sel = document.querySelector("#autocomplete .item.active");
          if (sel) sel.scrollIntoView({ block: "nearest" });
        } else if (ev.key === "Tab" && AcState.open) {
          ev.preventDefault();
          acceptAutocomplete(AcState.selected);
        } else if (ev.key === "Enter" && AcState.open && AcState.items.length > 0) {
          ev.preventDefault();
          acceptAutocomplete(AcState.selected);
        } else if ((ev.ctrlKey || ev.metaKey) && ev.key === " ") {
          ev.preventDefault();
          updateAutocomplete();
        }
      });
      document.getElementById("sql").addEventListener("input", updateAutocomplete);
      document.getElementById("sql").addEventListener("keyup", (ev) => {
        if (ev.key !== "Shift" && ev.key !== "Control" && ev.key !== "Alt" && ev.key !== "Meta") {
          updateAutocomplete();
        }
      });
      document.getElementById("sql").addEventListener("click", updateAutocomplete);
      document.getElementById("sql").addEventListener("focus", updateAutocomplete);
      document.getElementById("sql").addEventListener("blur", () => {
        setTimeout(closeAutocomplete, 120);
      });
      document.querySelectorAll(".samples button").forEach(b => {
        b.addEventListener("click", () => {
          document.getElementById("sql").value = b.dataset.sql;
          runSql();
        });
      });

      renderHistory();
      refreshTables();
      refreshSuggestions();
      refresh();
      setInterval(refresh, 1500);
      setInterval(refreshTables, 5000);
      setInterval(refreshSuggestions, 5000);
      setInterval(refreshPipelines, 5000);

      // --- Pipelines tab ---
      // Hand-rolled layered DAG renderer. Given the layer structure
      // returned by `GET /v1/pipelines/:name/dag`, layout each layer
      // vertically and draw boxes connected by arrows. Barycenter
      // ordering reduces edge crossings without needing dagre/d3.
      const PipelinesState = { selected: null };

      async function refreshPipelines() {
        const ul = document.getElementById("pipelines-list");
        const badge = document.getElementById("badge-pipelines");
        let items = [];
        try {
          const r = await fetch("/v1/pipelines");
          if (!r.ok) return;
          items = await r.json();
        } catch (e) { return; }
        const count = items.length;
        badge.textContent = count;
        badge.style.display = count > 0 ? "inline-block" : "none";
        if (count === 0) {
          ul.innerHTML = '<li style="color:var(--text-dim);padding:8px;font-size:11px;">no pipelines yet — submit one →</li>';
          document.getElementById("dag-svg").innerHTML = "";
          document.getElementById("dag-name").textContent = "";
          return;
        }
        ul.innerHTML = items.map(p => {
          const sel = PipelinesState.selected === p.name ? ' style="background:var(--bg-elev);"' : '';
          return `<li${sel} class="clickable" data-pipe="${escapeHtml(p.name)}">${escapeHtml(p.name)} <span style="color:var(--text-dim);font-size:10px;">${p.flows.length}f</span></li>`;
        }).join("");
        ul.querySelectorAll("li.clickable").forEach(li => {
          li.addEventListener("click", () => {
            PipelinesState.selected = li.dataset.pipe;
            renderDag(PipelinesState.selected);
            refreshPipelines();
          });
        });
        if (!PipelinesState.selected && items.length > 0) {
          PipelinesState.selected = items[0].name;
          renderDag(PipelinesState.selected);
          ul.querySelectorAll("li.clickable").forEach(li => {
            if (li.dataset.pipe === PipelinesState.selected) li.style.background = "var(--bg-elev)";
          });
        }
      }

      async function renderDag(name) {
        const svg = document.getElementById("dag-svg");
        const title = document.getElementById("dag-name");
        title.textContent = name;
        let dag;
        try {
          const r = await fetch("/v1/pipelines/" + encodeURIComponent(name) + "/dag");
          if (!r.ok) {
            svg.innerHTML = `<text x="20" y="30" fill="var(--danger)">dag fetch failed (${r.status})</text>`;
            return;
          }
          dag = await r.json();
        } catch (e) {
          svg.innerHTML = `<text x="20" y="30" fill="var(--danger)">network error</text>`;
          return;
        }
        const kinds = {};
        (dag.flows || []).forEach(f => { kinds[f.name] = f.kind; });
        const layers = dag.layers || [];
        if (layers.length === 0) {
          svg.innerHTML = '<text x="20" y="30" fill="var(--text-dim)">empty pipeline</text>';
          return;
        }
        // Barycenter ordering: for each node, compute the average
        // index of its predecessors in the previous layer. Iterating
        // top-down keeps edges short.
        const positions = {};
        const ySpacing = 100;
        const xSpacing = 180;
        const layerY = (i) => 60 + i * ySpacing;
        for (let i = 0; i < layers.length; i++) {
          if (i === 0) {
            layers[i].forEach((n, j) => { positions[n] = { x: 40 + j * xSpacing, y: layerY(i) }; });
            continue;
          }
          const prevLayer = layers[i - 1];
          const prevX = new Map(prevLayer.map((n, j) => [n, j]));
          const sorted = layers[i].slice().sort((a, b) => {
            const pa = (dag.flows || []).find(f => f.name === a)?.depends_on || [];
            const pb = (dag.flows || []).find(f => f.name === b)?.depends_on || [];
            const avgA = pa.length ? pa.reduce((s, p) => s + (prevX.get(p) ?? 0), 0) / pa.length : 0;
            const avgB = pb.length ? pb.reduce((s, p) => s + (prevX.get(p) ?? 0), 0) / pb.length : 0;
            return avgA - avgB;
          });
          sorted.forEach((n, j) => { positions[n] = { x: 40 + j * xSpacing, y: layerY(i) }; });
        }
        // Build SVG
        const w = Math.max(640, 40 + Math.max(...layers.map(l => l.length)) * xSpacing + 40);
        const h = 60 + layers.length * ySpacing + 40;
        svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
        svg.innerHTML = "";
        // Edges first
        (dag.flows || []).forEach(f => {
          (f.depends_on || []).forEach(dep => {
            const a = positions[dep];
            const b = positions[f.name];
            if (!a || !b) return;
            const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
            const mx = (a.x + b.x) / 2;
            path.setAttribute("d", `M ${a.x + 70} ${a.y + 35} C ${mx} ${a.y + 35}, ${mx} ${b.y + 35}, ${b.x + 70} ${b.y}`);
            path.setAttribute("stroke", "var(--accent)");
            path.setAttribute("stroke-width", "1.5");
            path.setAttribute("fill", "none");
            path.setAttribute("marker-end", "url(#arrowhead)");
            svg.appendChild(path);
          });
        });
        // Arrowhead marker
        const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
        defs.innerHTML = `<marker id="arrowhead" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="var(--accent)"/></marker>`;
        svg.appendChild(defs);
        // Nodes
        (dag.flows || []).forEach(f => {
          const pos = positions[f.name];
          if (!pos) return;
          const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
          const stripeColor = f.kind === "streaming_table" ? "var(--accent)" : "var(--success)";
          g.innerHTML = `
            <rect x="${pos.x}" y="${pos.y}" width="140" height="70" rx="4" fill="var(--bg-elev)" stroke="var(--border)" stroke-width="1"/>
            <rect x="${pos.x}" y="${pos.y}" width="6" height="70" fill="${stripeColor}"/>
            <text x="${pos.x + 14}" y="${pos.y + 24}" fill="var(--text)" font-size="13" font-family="ui-monospace,SFMono-Regular,Menlo,monospace">${escapeHtml(f.name)}</text>
            <text x="${pos.x + 14}" y="${pos.y + 44}" fill="var(--text-dim)" font-size="10">${escapeHtml(f.kind)}</text>
            <text x="${pos.x + 14}" y="${pos.y + 60}" fill="var(--text-dim)" font-size="9">deps: ${escapeHtml((f.depends_on || []).join(", ") || "—")}</text>
          `;
          svg.appendChild(g);
        });
      }

      document.getElementById("submit-pipeline-btn").addEventListener("click", async () => {
        const ta = document.getElementById("pipeline-yaml");
        const status = document.getElementById("pipeline-status");
        const yaml = ta.value.trim();
        if (!yaml) { status.textContent = "paste a spec first"; return; }
        status.textContent = "running…";
        try {
          const r = await fetch("/v1/pipelines", {
            method: "POST",
            headers: { "content-type": "text/plain" },
            body: yaml,
          });
          if (!r.ok) {
            const err = await r.text();
            status.textContent = "failed: " + err.slice(0, 200);
            return;
          }
          const body = await r.json();
          const flowStats = (body.report && body.report.flows) || [];
          const total = flowStats.reduce((s, f) => s + (f.row_count || 0), 0);
          status.textContent = `ran ${flowStats.length} flow(s), ${total} row(s) written`;
          refreshPipelines();
        } catch (e) {
          status.textContent = "network error: " + e;
        }
      });
    </script>
  </body>
</html>
"##;
