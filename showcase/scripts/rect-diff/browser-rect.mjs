// Browser rect exporter: measure every body descendant's getBoundingClientRect
// in a real Chromium via Playwright, dump JSON for later diff against Ikat core.
//
// Usage: node browser-rect.mjs <showcase-html-abs-path> <out.json>
//
// 页面经 `ikat preview` server 加载（单一注入事实源：人类预览和本工具看到的是
// 同一个「页面 + 预览模拟脚本」栈——main.js/pages/*.js 的注入由 server 做，本
// 脚本不再手工拼装）。脚本内起临时 server（--port 0 OS 挑端口），finally 杀掉。
// 仍由本脚本注入/撤销的只有测量面：A1 reset（剥 UA 默认）+ zoom 清零 + data-fill
// 克隆撤销（core 静态 dump 无 C# 驱动，克隆条目无对拍对象）。

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const [, , htmlPath, outPath] = process.argv;
if (!htmlPath || !outPath) {
  console.error('usage: node browser-rect.mjs <showcase-html-abs-path> <out.json>');
  process.exit(1);
}

const htmlAbs = resolve(htmlPath);

// 工作区根 = 自页面路径向上找 ikat.workspace.json。
function findWorkspaceRoot(dir) {
  for (let d = dir; ; d = dirname(d)) {
    try {
      readFileSync(join(d, 'ikat.workspace.json'));
      return d;
    } catch { /* not here */ }
    const parent = dirname(d);
    if (parent === d) throw new Error(`no ikat.workspace.json above ${dir}`);
  }
}
const wsRoot = findWorkspaceRoot(dirname(htmlAbs));
const relFromWs = htmlAbs.slice(wsRoot.length + 1).replace(/\\/g, '/');

// ikat.exe：环境变量优先，其次仓库构建产物（rect-diff 在仓库 showcase/scripts/ 下）。
function findIkat() {
  if (process.env.IKAT_EXE) return process.env.IKAT_EXE;
  const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
  for (const c of [join(repoRoot, 'target/release/ikat.exe'), join(repoRoot, 'target/release/ikat')]) {
    try {
      readFileSync(c);
      return c;
    } catch { /* keep looking */ }
  }
  throw new Error(`ikat binary not found (build it or set IKAT_EXE): ${join(repoRoot, 'target/release/ikat.exe')}`);
}

async function startPreview() {
  const ikat = findIkat();
  const child = spawn(ikat, ['preview', wsRoot, '--port', '0', '--idle-timeout', '600'], {
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  const port = await new Promise((res, rej) => {
    let buf = '';
    const timer = setTimeout(() => rej(new Error('ikat preview did not report within 20s')), 20000);
    child.stdout.on('data', (d) => {
      buf += d;
      const line = buf.split('\n').find((l) => l.trim().startsWith('{'));
      if (line) {
        clearTimeout(timer);
        try {
          res(JSON.parse(line).port);
        } catch (e) {
          rej(new Error(`ikat preview stdout not JSON: ${line}`));
        }
      }
    });
    child.on('exit', (code) => rej(new Error(`ikat preview exited early (code ${code})`)));
  });
  return { child, port };
}

const reset = readFileSync(new URL('./reset.css', import.meta.url), 'utf8');
// Workspace fonts (JetBrainsMono / PressStart2P / DejaVuSans) load via the
// preview-base.css @font-face rules — see that file. Core measures text with
// the same real files, so both sides must resolve the same families.

const preview = await startPreview();
const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  // 预览模拟脚本（main.js/pages/*.js）由 server 注入并保持启用：它是 core 运行时
  // 行为的浏览器侧模拟（textbox 占位行、progressbar fill 宽、slider thumb 定位）。
  // 整体禁掉会制造假 diff（空 textbox 浏览器侧 39px、core 侧仍渲染占位行 20px）。
  // 唯一要撤销的是 data-fill 演示克隆（见下）。
  await page.goto(`http://127.0.0.1:${preview.port}/ws/${encodeURI(relFromWs)}`, { waitUntil: 'networkidle' });
  await page.addStyleTag({ content: reset });
  await page.waitForTimeout(100); // let reset reflow settle

  // Core dump reports each node's NodeKind via `kind_to_html_tag`, which maps
  // role-driven controls to their semantic element (role=listitem -> li,
  // progressbar -> progress, spinbutton -> input, ...). The literal DOM tag
  // for those is a plain div, so without this normalization every role-driven
  // node lands in a different tag+classes bucket and pairs with nothing.
  // role→tag 表来自 Rust 单源导出（semantic-tags.json，ikat_pkg 测试钉新鲜度，
  // 真相源链 = fence ROLE_TO_SEMANTIC → bridge semantic_to_kind → core
  // kind_to_html_tag）。textbox 的 aria-multiline→textarea 分流留在浏览器侧
  // （只有这里能看到该属性），经 evaluate 参数传进去。
  const roleTags = JSON.parse(
    readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'semantic-tags.json'), 'utf8'),
  ).role;

  const rects = await page.evaluate((roleTags) => {
    document.body.style.zoom = '';
    // Undo demo fill: remove cloned (non-template) children of data-fill
    // lists so the browser shows the same empty list core laid out.
    document.querySelectorAll('[role="list"][data-fill]').forEach((list) => {
      Array.from(list.children).forEach((ch) => {
        if (ch.tagName !== 'TEMPLATE') ch.remove();
      });
    });
    function semanticTag(el) {
      const role = el.getAttribute('role');
      if (role) {
        if (role === 'textbox' && el.getAttribute('aria-multiline') === 'true') return 'textarea';
        if (roleTags[role]) return roleTags[role];
      }
      // Hyphenated custom elements: core's dump emits the custom_tag literal
      // (pkg v35), so the browser side pairs on the literal tagName too.
      return el.tagName.toLowerCase();
    }
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
  }, roleTags);

  writeFileSync(outPath, JSON.stringify(rects, null, 2));
  console.log(`wrote ${rects.length} elements -> ${outPath}`);
} finally {
  await browser.close();
  preview.child.kill();
}
