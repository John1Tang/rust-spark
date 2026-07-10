const DEFAULT = "http://127.0.0.1:8081/v1/events";

const urlInput = document.getElementById("url");
const saveBtn = document.getElementById("save");
const flushBtn = document.getElementById("flush");
const status = document.getElementById("status");

chrome.storage.session.get(["ingestUrl"], (got) => {
  urlInput.value = (got && got.ingestUrl) || DEFAULT;
});

saveBtn.addEventListener("click", () => {
  chrome.storage.session.set({ ingestUrl: urlInput.value.trim() }, () => {
    status.textContent = "saved";
    setTimeout(() => { status.textContent = ""; }, 1200);
  });
});

flushBtn.addEventListener("click", () => {
  chrome.runtime.sendMessage({ kind: "rspark_flush" });
  status.textContent = "flushed";
});