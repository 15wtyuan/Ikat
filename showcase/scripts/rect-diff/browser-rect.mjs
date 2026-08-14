// Browser rect exporter: measure every body descendant's getBoundingClientRect
// in a real Chromium via Playwright, dump JSON for later diff against LoomGUI core.
//
// Usage: node browser-rect.mjs <showcase-html-abs-path> <out.json>
// Loads HTML via file://, injects A1 reset (strip UA defaults LoomGUI lacks),
// waits briefly for reflow, then captures per-element rects.

import { chromium } from 'playwright';
import { readFileSync, readdirSync, writeFileSync } from 'fs';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';


const [, , htmlPath, outPath] = process.argv;
if (!htmlPath || !outPath) {
  console.error('usage: node browser-rect.mjs <showcase-html-abs-path> <out.json>');
  process.exit(1);
}

const reset = readFileSync(new URL('./reset.css', import.meta.url), 'utf8');

// Workspace fonts (JetBrainsMono / PressStart2P / DejaVuSans) load via the
// preview-base.css @font-face rules — see that file. Core measures text with
// the same real files, so both sides must resolve the same families.

const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  // loom-preview.js stays ENABLED: it is the browser-side simulator of core
  // runtime behavior (textbox placeholder line via contenteditable+::before,
  // progressbar fill width, slider thumb positioning). Killing it wholesale
  // would strip those alignments and fabricate diffs (an empty textbox drops
  // from 39px to 20px in the browser while core still renders the placeholder
  // line). The ONE piece we must undo is fillListViews — it clones template
  // items into role=list[data-fill] containers, but core's dump_page has no
  // C# driver to set ItemCount, so the cloned items have no core counterpart.
  // Driver-driven list virtualization is verified on the Unity machine
  // (roadmap task 4); the core-dump path here only checks static layout.
  // Component registry (packer components/ dir, same auto-scan rule): inject as
  // window.__LOOM_COMPONENTS__ so loom-preview.js expandComponents() can mirror the
  // pack-time Custom Element expansion (host + slot projection). Manual file://
  // double-click preview has no injection source and leaves components unexpanded
  // (accepted degradation — this Playwright path is the alignment gate).
  const components = {};
  const compDir = join(dirname(htmlPath), 'components');
  try {
    for (const f of readdirSync(compDir)) {
      if (f.endsWith('.html')) components[f.replace(/\.html$/, '')] = readFileSync(join(compDir, f), 'utf8');
    }
  } catch { /* no components dir = no components */ }
  await page.addInitScript((regs) => { window.__LOOM_COMPONENTS__ = regs; }, components);

  await page.goto(pathToFileURL(htmlPath).href, { waitUntil: 'networkidle' });
  await page.addStyleTag({ content: reset });
  await page.waitForTimeout(100); // let reset reflow settle

  const rects = await page.evaluate(() => {
    document.body.style.zoom = '';
    // Undo fillListViews: remove cloned (non-template) children of data-fill
    // lists so the browser shows the same empty list core laid out.
    document.querySelectorAll('[role="list"][data-fill]').forEach((list) => {
      Array.from(list.children).forEach((ch) => {
        if (ch.tagName !== 'TEMPLATE') ch.remove();
      });
    });
    const els = document.querySelectorAll('body *');
    return Array.from(els).map((el, i) => {
      const r = el.getBoundingClientRect();
      return {
        domIndex: i,
        tag: semanticTag(el),
        id: el.id || null,
        classes: Array.from(el.classList),
        x: r.x,
        y: r.y,
        w: r.width,
        h: r.height,
      };
    });

    // Core dump reports each node's NodeKind via `kind_to_html_tag`, which maps
    // role-driven controls to their semantic element (role=listitem -> li,
    // progressbar -> progress, spinbutton -> input, ...). The literal DOM tag
    // for those is a plain div, so without this normalization every role-driven
    // node lands in a different tag+classes bucket and pairs with nothing.
    // Table mirrors core's NodeKind mapping (crates/core/src/dump.rs).
    function semanticTag(el) {
      const role = el.getAttribute('role');
      if (role) {
        if (role === 'textbox' && el.getAttribute('aria-multiline') === 'true') return 'textarea';
        const roleTags = {
          listitem: 'li',
          list: 'ul',
          progressbar: 'progress',
          spinbutton: 'input',
          slider: 'input',
          switch: 'input',
          radio: 'input',
          textbox: 'input',
          combobox: 'select',
          option: 'option',
          tab: 'button',
        };
        if (roleTags[role]) return roleTags[role];
      }
      const t = el.tagName.toLowerCase();
      // Hyphenated custom elements: core's dump emits the custom_tag literal
      // (pkg v35), so the browser side pairs on the literal tagName too.
      return t;
    }
  });

  writeFileSync(outPath, JSON.stringify(rects, null, 2));
  console.log(`wrote ${rects.length} elements -> ${outPath}`);
} finally {
  await browser.close();
}
