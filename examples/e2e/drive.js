#!/usr/bin/env node
// Drive the demo page with Playwright to generate a realistic stream of
// page_view / page_scroll / page_click events. The page itself emits
// events via fetch to the rspark-ingest backend, which produces them
// to Kafka. The pipeline runner consumes them into a streaming table.
//
// Usage:
//   npm i -D playwright
//   npx playwright install chromium
//   RSPARK_DEMO_URL=http://127.0.0.1:8080/examples/e2e/demo_page.html \
//     node examples/e2e/drive.js
//
// Or, for the file:// case (no server):
//   RSPARK_DEMO_URL=file:///Users/john/projects/rust-spark/examples/e2e/demo_page.html \
//     node examples/e2e/drive.js
//
// Default URL assumes the page is served at port 8080 alongside the
// master; for local dev without a server, set RSPARK_DEMO_URL to the
// file:// path.

const { chromium } = require("playwright");

const URL = process.env.RSPARK_DEMO_URL ||
  "http://127.0.0.1:8080/examples/e2e/demo_page.html";
const DURATION_MS = parseInt(process.env.RSPARK_DEMO_DURATION_MS || "30000", 10);
const VIEWPORTS = [
  { width: 1440, height: 900 },
  { width: 1024, height: 768 },
  { width: 768, height: 1024 },
];

async function main() {
  const browser = await chromium.launch();
  let totalClicks = 0;
  let totalScrolls = 0;

  for (const vp of VIEWPORTS) {
    const ctx = await browser.newContext({ viewport: vp });
    const page = await ctx.newPage();
    page.on("console", (m) => {
      if (m.type() === "error") console.error("page console:", m.text());
    });
    await page.goto(URL, { waitUntil: "domcontentloaded" });
    console.log(`▸ viewing ${URL} at ${vp.width}x${vp.height}`);
    const t0 = Date.now();

    // Click loop
    let clicks = 0;
    page.locator("#cta-buy, #cta-info").first().click().catch(() => {});

    // Scroll + click loop for the configured duration.
    while (Date.now() - t0 < DURATION_MS) {
      // Click a random link/button if any.
      const buttons = await page.locator("button, .card").all();
      if (buttons.length > 0 && Math.random() < 0.5) {
        const which = buttons[Math.floor(Math.random() * buttons.length)];
        await which.click({ timeout: 500 }).catch(() => {});
        clicks++;
      }
      // Scroll a bit.
      await page.mouse.wheel(0, Math.random() < 0.3 ? -300 : 200);
      totalScrolls++;
      await page.waitForTimeout(150 + Math.random() * 200);
    }
    totalClicks += clicks;
    console.log(`  ${clicks} click(s), ${totalScrolls} scroll(s) on this viewport`);
    await ctx.close();
  }

  await browser.close();
  console.log(`✓ done. ${totalClicks} click(s), ${totalScrolls} scroll(s) total across viewports.`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});