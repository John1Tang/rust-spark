// rspark-tracker background service worker.
//
// Receives events from content scripts, batches them, and POSTs to
// the rspark ingest backend (default http://127.0.0.1:8081/v1/events).
// The backend produces to Kafka.
//
// MV3 service workers can be killed at any time, so state lives in
// chrome.storage.session. We flush every 2s and on `onSuspend`.

const DEFAULT_INGEST = "http://127.0.0.1:8081/v1/events";
const FLUSH_MS = 2000;
const MAX_BATCH = 100;

async function getIngestUrl() {
  return new Promise((resolve) => {
    chrome.storage.session.get(["ingestUrl"], (got) => {
      resolve(got && got.ingestUrl ? got.ingestUrl : DEFAULT_INGEST);
    });
  });
}

async function flush() {
  const url = await getIngestUrl();
  const got = await chrome.storage.session.get(["buffer"]);
  const buffer = (got && got.buffer) || [];
  if (buffer.length === 0) return;
  await chrome.storage.session.set({ buffer: [] });
  try {
    await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ events: buffer }),
    });
  } catch (e) {
    // Backend down — drop the batch. A real impl would retry, but for
    // a learning-project tracker the page events are best-effort.
    console.warn("[rspark-tracker] ingest failed:", e && e.message);
  }
}

let flushTimer = null;
function scheduleFlush() {
  if (flushTimer) return;
  flushTimer = setTimeout(() => {
    flushTimer = null;
    flush();
  }, FLUSH_MS);
}

chrome.runtime.onMessage.addListener((msg, _sender, _sendResponse) => {
  if (!msg || msg.kind !== "rspark_event") return;
  chrome.storage.session.get(["buffer"], async (got) => {
    const buffer = (got && got.buffer) || [];
    buffer.push(msg.payload);
    // Cap the buffer so a runaway tab doesn't fill memory.
    if (buffer.length > MAX_BATCH) buffer.splice(0, buffer.length - MAX_BATCH);
    await chrome.storage.session.set({ buffer });
    if (buffer.length >= MAX_BATCH) flush();
    else scheduleFlush();
  });
});

chrome.runtime.onSuspend.addListener(() => {
  flush();
});

// On install / startup, set sane defaults.
chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.session.set({ buffer: [] });
});