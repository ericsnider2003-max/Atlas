const fs = require('fs');
const assert = require('assert');
const path = require('path');
// Optional rendered regression: use installed Playwright/browser tooling.
// No dependencies are downloaded, files selected, or uploads submitted.
const { chromium } = require(process.env.ATLAS_PLAYWRIGHT_MODULE || 'playwright');
const project = path.resolve(__dirname, '..');
(async () => {
  const source = fs.readFileSync(project + '/src/hub/pages.rs', 'utf8');
  const body = source.split('pub(super) fn files_in_and_out() -> String {')[1].split('".to_string()')[0].trim() + '"';
  const html = JSON.parse(body.replace(/\\\r?\n\s*/g, ''));
  const css = fs.readFileSync(project + '/assets/hub/hub.css', 'utf8');
  const browser = await chromium.launch({ channel: process.env.ATLAS_BROWSER_CHANNEL || 'msedge', headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent('<html lang=en><style>' + css + '</style><main>' + html + '</main></html>');
    for (const theme of ['paper', 'dark', 'access']) {
      await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
      await page.locator('body').click({ position: { x: 1, y: 1 } });
      const reached = new Set();
      for (let i = 0; i < 12; i++) {
        await page.keyboard.press('Tab');
        const focused = await page.evaluate(() => {
          const node = document.activeElement;
          if (node.tagName !== 'INPUT' || node.type !== 'file') return null;
          const label = node.closest('label');
          if (!label || parseFloat(getComputedStyle(label).outlineWidth) < 3) throw new Error('upload focus is not visible');
          return node.id || label.dataset.prefix;
        });
        if (focused) reached.add(focused);
      }
      console.log(theme + ' keyboard reached upload controls:', [...reached]);
      assert.equal(reached.size, 5, 'all five upload controls must be keyboard reachable');
    }
    await page.emulateMedia({ forcedColors: 'active' });
    await page.locator('#bring-in-file').focus();
    const chooser = page.waitForEvent('filechooser', { timeout: 2000 });
    await page.keyboard.press('Enter');
    await chooser;
    const outlined = await page.locator('#bring-in-file').evaluate(node => {
      const label = node.closest('label');
      return label && parseFloat(getComputedStyle(label).outlineWidth) >= 3;
    });
    assert(outlined, 'the focused upload control must visibly outline its label');
    console.log('Native keyboard file chooser and visible label focus passed. No file selected or uploaded.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
