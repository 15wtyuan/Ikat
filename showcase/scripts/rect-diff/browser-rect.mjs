// Browser rect exporter: measure every body descendant's getBoundingClientRect
// in a real Chromium via Playwright, dump JSON for later diff against LoomGUI core.
//
// Usage: node browser-rect.mjs <showcase-html-abs-path> <out.json>
// Loads HTML via file://, injects A1 reset (strip UA defaults LoomGUI lacks),
// waits briefly for reflow, then captures per-element rects.

import { chromium } from 'playwright';
import { readFileSync, writeFileSync } from 'fs';
import { pathToFileURL } from 'node:url';

const [, , htmlPath, outPath] = process.argv;
if (!htmlPath || !outPath) {
  console.error('usage: node browser-rect.mjs <showcase-html-abs-path> <out.json>');
  process.exit(1);
}

const reset = readFileSync(new URL('./reset.css', import.meta.url), 'utf8');

const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.goto(pathToFileURL(htmlPath).href, { waitUntil: 'networkidle' });
  // Inject reset AFTER load so it overrides UA defaults before measurement;
  // showcase <style> rules still win where they specify values.
  await page.addStyleTag({ content: reset });
  // loom-preview.js fitScale() sets body.style.zoom to letterbox the 1920x1080
  // .root inside the preview window — a preview-only transform with no core
  // counterpart that uniformly shrinks every rect ~4.5% and cascades into
  // hundreds of false width/position diffs. Clear it so the measurement
  // reflects the true 1:1 layout core produces.
  await page.evaluate(() => { document.body.style.zoom = ''; });
  await page.waitForTimeout(100); // let reset + zoom-clear reflow settle

  const rects = await page.evaluate(() => {
    const els = document.querySelectorAll('body *');
    return Array.from(els).map((el, i) => {
      const r = el.getBoundingClientRect();
      return {
        domIndex: i,
        tag: el.tagName.toLowerCase(),
        id: el.id || null,
        classes: Array.from(el.classList),
        x: r.x,
        y: r.y,
        w: r.width,
        h: r.height,
      };
    });
  });

  writeFileSync(outPath, JSON.stringify(rects, null, 2));
  console.log(`wrote ${rects.length} elements -> ${outPath}`);
} finally {
  await browser.close();
}
