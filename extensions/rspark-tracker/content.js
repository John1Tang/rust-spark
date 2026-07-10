// rspark-tracker content script.
//
// Runs at document_start on every page (matches <all_urls>). Listens
// for page events — view, scroll, click — and forwards them to the
// background service worker, which batches and POSTs them to the
// ingest backend.
//
// We deliberately avoid pulling in any third-party tracker. The
// schema is small and stable:
//
//   {
//     "type": "page_view" | "page_scroll" | "page_click",
//     "ts_ms": 1736451234567,
//     "url": "https://example.com/foo",
//     "title": "...",
//     "viewport_w": 1440,
//     "viewport_h": 900,
//     // type-specific fields below
//     "x": 230, "y": 540, "tag": "A", "id": "...", "class": "...",
//     "depth_pct": 67,
//   }

(() => {
  // Throttle scroll events: at most one per ~150ms while the user is
  // actively scrolling. Clicks are sent immediately.
  let lastScrollSent = 0;
  const SCROLL_THROTTLE_MS = 150;

  function send(payload) {
    try {
      chrome.runtime.sendMessage({ kind: "rspark_event", payload });
    } catch (_) {
      // Background worker may have just shut down (extension reloaded).
      // Events are best-effort; dropping one is fine.
    }
  }

  function pageView() {
    send({
      type: "page_view",
      ts_ms: Date.now(),
      url: location.href,
      title: document.title || "",
      viewport_w: window.innerWidth,
      viewport_h: window.innerHeight,
      referrer: document.referrer || "",
    });
  }

  function pageScroll() {
    const now = Date.now();
    if (now - lastScrollSent < SCROLL_THROTTLE_MS) return;
    lastScrollSent = now;
    const doc = document.documentElement;
    const scrollTop = window.scrollY || doc.scrollTop || 0;
    const scrollHeight = Math.max(
      doc.scrollHeight,
      document.body ? document.body.scrollHeight : 0
    );
    const depthPct = scrollHeight > window.innerHeight
      ? Math.min(100, Math.round((scrollTop / (scrollHeight - window.innerHeight)) * 100))
      : 100;
    send({
      type: "page_scroll",
      ts_ms: now,
      url: location.href,
      depth_pct: depthPct,
      scroll_top: scrollTop,
      scroll_height: scrollHeight,
      viewport_h: window.innerHeight,
    });
  }

  function pageClick(ev) {
    const t = ev.target;
    if (!t) return;
    send({
      type: "page_click",
      ts_ms: Date.now(),
      url: location.href,
      x: ev.clientX,
      y: ev.clientY,
      tag: t.tagName || "",
      id: t.id || "",
      class: (typeof t.className === "string" ? t.className : "") || "",
      text: (t.innerText || "").slice(0, 80),
      href: t.closest && t.closest("a") ? t.closest("a").href || "" : "",
    });
  }

  // page_view on load
  pageView();

  // scroll
  window.addEventListener("scroll", pageScroll, { passive: true });

  // click — capture phase so we still see clicks stopped by other handlers
  document.addEventListener("click", pageClick, true);
})();