import { chromium } from '/Users/john/.npm/_npx/a80a913f4f8f2557/node_modules/playwright/index.mjs';

const url = 'http://127.0.0.1:8088/';
const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ bypassCSP: true });
const page = await ctx.newPage();
const consoleMsgs = [];
page.on('console', m => consoleMsgs.push(`[${m.type()}] ${m.text()}`));
page.on('pageerror', e => consoleMsgs.push(`[pageerror] ${e.message}`));

const resp = await page.goto(url, { waitUntil: 'networkidle', timeout: 15000 });
console.log('HTTP', resp.status());

// Look at the samples row
const buttons = await page.locator('.samples button').allTextContents();
console.log('samples row buttons:', JSON.stringify(buttons));

// Specifically the two new ones
const new1 = await page.locator('button[data-sql*="click_events c LEFT JOIN users u"]').count();
const new2 = await page.locator('button[data-sql*="page_views"]').count();
console.log('new-button 1 (stream×batch join):', new1);
console.log('new-button 2 (page views / signup country):', new2);

// Screenshot the editor area for visual proof
await page.screenshot({ path: '/tmp/rspark-test/samples.png', clip: { x: 0, y: 200, width: 1200, height: 400 } });
console.log('screenshot: /tmp/rspark-test/samples.png');

// Click the first new button and verify the editor updates
if (new1 > 0) {
  await page.click('button[data-sql*="click_events c LEFT JOIN users u"]');
  const editorText = await page.locator('#sql').inputValue();
  console.log('after-click editor length:', editorText.length);
  console.log('editor starts with:', editorText.slice(0, 60).replace(/\n/g, '\\n'));
}

if (consoleMsgs.length) console.log('console:\n', consoleMsgs.join('\n'));
await browser.close();
