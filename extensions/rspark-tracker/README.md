# rspark-tracker (Chrome extension)

A minimal Manifest V3 extension that captures three kinds of page events and forwards them to the rspark ingest backend. The backend then produces to Kafka (see `docs/kafka.md`), and the pipeline runner consumes from Kafka into a streaming table (see `docs/pipelines.md`).

## Captured events

| `type`        | When it fires                                       | Notable fields                                   |
|---------------|-----------------------------------------------------|--------------------------------------------------|
| `page_view`   | On load (after `document_start`)                    | `url`, `title`, `viewport_w`, `viewport_h`, `referrer` |
| `page_scroll` | On window scroll (throttled to ~150ms)              | `depth_pct`, `scroll_top`, `scroll_height`, `viewport_h` |
| `page_click`  | On any document click (capture phase)               | `x`, `y`, `tag`, `id`, `class`, `text`, `href`   |

## Install (unpacked)

1. Open `chrome://extensions` (or `brave://extensions`, `edge://extensions`).
2. Toggle **Developer mode** on (top-right).
3. Click **Load unpacked**.
4. Pick `extensions/rspark-tracker/`.
5. The extension icon appears in the toolbar.

To change the ingest URL, click the icon and edit the field — it's stored in `chrome.storage.session`, which means it persists across sessions but is local to the browser profile.

## Ingest endpoint

Default: `http://127.0.0.1:8081/v1/events`. The backend (a tiny Rust service in `crates/rspark-ingest/`) accepts `{ events: [...] }` and produces each event to Kafka topic `rspark.page_events`.

If you run the backend in the cluster instead of locally, change the URL to `http://127.0.0.1:8081` after a `kubectl port-forward svc/rspark-ingest 8081:8081`.

## Why a popup?

The popup only exists so the user can change the ingest URL and flush the buffer. There's no separate background page; everything goes through the service worker.

## Schema

Each event is a flat JSON object:

```json
{
  "type": "page_view",
  "ts_ms": 1736451234567,
  "url": "https://example.com/foo",
  "title": "Example",
  "viewport_w": 1440,
  "viewport_h": 900,
  "referrer": "https://google.com"
}
```

The Kafka key is `null` (round-robin partition) and the value is the event JSON encoded as UTF-8.

## Files

- `manifest.json` — MV3 manifest.
- `content.js` — content script (event listeners).
- `background.js` — service worker (batches + flushes to backend).
- `popup.html` / `popup.js` — settings UI.
- `icon*.png` — extension icons (placeholders — generate real ones if you ship it).