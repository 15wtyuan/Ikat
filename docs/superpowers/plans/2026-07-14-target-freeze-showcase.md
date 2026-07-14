# 靶子冻结 Showcase 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 repo 顶层新建 `showcase/` 工作区，产出覆盖围栏全部 23 标签 + CSS 全白名单的 8 页 showcase（HTML/CSS 设计期靶子）+ 浏览器预览辅助代码，冻结为 R2–R7 运行时重写的渲染验收靶子。

**Architecture:** 分离注入预览——`<head>` 引 preview-base.css + loom-preview.js，body 零预览代码；打包器只消费 body。每页是独立 HTML 文件，共享 Deep Ocean 设计令牌（design-systems/tokens.css）和 UA 基线（preview/preview-base.css）。验证回路 = 围栏打包门（`loom-pkg build showcase/` 零 diagnostic）+ 浏览器可视 + 覆盖矩阵脚本。

**Tech Stack:** 围栏内 HTML 子集（23 标签）、围栏内 CSS 子集、经典 JS（非 ES module，避 file:// CORS）、LoomGUI 打包器（loomgui_pkg）。

**权威约束（违反 = 隐 bug）：**
- 只用 [docs/design/fence.md](/F:/WorkSpace/projects/LoomGUI/docs/design/fence.md) 的 23 运行时标签 + custom-element（含 hyphen 标签名）。
- 严禁 h1–h6 / meter / dialog / details / summary / form / fieldset / legend / main / section / footer / article / aside / small。
- 标题用 `<p>` + font-size/font-weight；Tab 用 `role=tablist`/`role=tab`/`role=tabpanel`；弹窗用 `div role=dialog` + display 切换；状态条用 `<progress>`；分组用 `div`+`p`。
- preview CSS/JS 只进 `<head>`，body 零预览代码。

**围栏权威真值（每个标签 + input type 的语义签名，实现期对账用）：**

| 签名 | SemanticKind |
|---|---|
| div / header / nav | Container |
| p | TextBlock |
| span / strong / em | TextElement |
| br | LineBreak |
| label | Label |
| button | Button |
| a | Link |
| img | Image |
| canvas | Canvas |
| input[text/password/search] | TextField |
| input[number] | NumberField |
| input[range] | Slider |
| input[checkbox] | Toggle |
| input[radio] | RadioButton |
| textarea | TextArea |
| select | Dropdown |
| option | OptionItem |
| progress | ProgressBar |
| ul / ol | ListView |
| li | ListItem |
| template | Template |
| slot | Slot |
| tag-name（含 hyphen） | CustomElement |

**围栏 CSS 白名单分组（lab.html 必须逐组 specimen，对账用）：** 尺寸（width/height/min/max/aspect-ratio）、布局（display/flex-*/gap/justify/align/order）、定位（position/top/right/bottom/left）、盒模型（padding/margin）、边框（border-color/radius/border-image-slice）、背景（background-color/image/size/clip）、视觉（opacity/box-shadow/pointer-events/transform/filter）、文本（color/font-*/text-align/line-height/letter-spacing/white-space/text-shadow/-webkit-text-stroke/font-effect/transition）、溢出（overflow-x/y）。

---

## File Structure

每个文件的职责（创建/修改）：

- **Create** `showcase/loom.workspace.json` — 工作区清单（打包器 init 生成，不手写）。
- **Create** `showcase/design-systems/tokens.css` — Deep Ocean 设计令牌（全部重写，不搬旧版）。
- **Create** `showcase/res/fonts/*` — 从旧 `showcase_project/res/fonts/` 搬迁（迁出后旧目录删）。
- **Create** `showcase/res/icons/*` — 旧图标搬 + 新增游戏物品图标（Task 14）。
- **Create** `showcase/showcase/home.html` — 导航枢纽。
- **Create** `showcase/showcase/settings.html` — 控件全覆盖（7 input 变体 + select + role dispatch）。
- **Create** `showcase/showcase/inventory.html` — 背包 ListView + 物品网格 + 详情。
- **Create** `showcase/showcase/mail.html` — 邮件 ListView + 富文本。
- **Create** `showcase/showcase/shop.html` — 商店 progress + 弹窗 + 数量输入。
- **Create** `showcase/showcase/character.html` — canvas 3D slot + 属性条 + 技能 ol。
- **Create** `showcase/showcase/form.html` — 角色创建表单（div+p 分组）。
- **Create** `showcase/showcase/lab.html` — CSS 全属性标本馆。
- **Create** `showcase/showcase/components/nav-bar.html` — 共享顶栏（custom-element）。
- **Create** `showcase/showcase/components/stat-bar.html` — stat + progress 组件（custom-element）。
- **Create** `showcase/showcase/components/item-card.html` — 物品卡（custom-element + slot）。
- **Create** `showcase/showcase/preview/preview-base.css` — UA 基线 polyfill。
- **Create** `showcase/showcase/preview/loom-preview.js` — 导航 + 交互模拟 + letterbox 缩放。
- **Create** `showcase/showcase/preview/README.md` — 预览可信度说明。
- **Create** `showcase/scripts/coverage-check.py` — 覆盖矩阵扫描脚本（标签 + CSS 分组）。
- **Delete** `showcase_project/` — 全部内容迁出后整体删除（Task 15）。

---

## Verification Commands（每页通用）

围栏打包门（每页改完必跑，零 diagnostic 才算过）：

```bash
cargo run -p loomgui_pkg -- build showcase
```

浏览器可视：双击 `showcase/showcase/home.html`，逐页点 nav 卡片验证渲染。

覆盖矩阵（全部页完成后跑）：

```bash
python showcase/scripts/coverage-check.py
```

围栏契约门（不常改，但改 schema 后必跑）：

```bash
cargo test -p loomgui_fence
```

---

## Task 1: 工作区脚手架 + res 搬迁 + tokens.css

**Files:**
- Create: `showcase/loom.workspace.json`
- Create: `showcase/design-systems/tokens.css`
- Move: `showcase_project/res/fonts/*` → `showcase/res/fonts/`
- Move: `showcase_project/res/icons/*` → `showcase/res/icons/`

- [ ] **Step 1: 创建工作区目录骨架**

```bash
mkdir showcase
mkdir showcase/showcase
mkdir showcase/showcase/components
mkdir showcase/showcase/preview
mkdir showcase/design-systems
mkdir showcase/res
```

- [ ] **Step 2: 写 loom.workspace.json（打包器 init 风格手写，避免 init 交互）**

Create `showcase/loom.workspace.json`:

```json
{
  "version": 1,
  "output_dir": "../../unity/showcase-unity/Assets/Bundles",
  "packages": [
    {
      "name": "showcase",
      "dirs": ["showcase"],
      "html": []
    }
  ],
  "atlases": [
    {
      "name": "icons",
      "standalone": false,
      "dirs": ["res/icons"],
      "max_size": 1024,
      "padding": 4
    }
  ],
  "fonts": [
    { "family": "LXGWWenKai", "file": "res/fonts/LXGWWenKai.ttf", "default": true, "fallback": false },
    { "family": "wqy-microhei", "file": "res/fonts/wqy-microhei.ttc", "default": false, "fallback": true },
    { "family": "PressStart2P", "file": "res/fonts/PressStart2P.ttf", "default": false, "fallback": false },
    { "family": "DejaVuSans", "file": "res/fonts/DejaVuSans.ttf", "default": false, "fallback": false },
    { "family": "JetBrainsMono", "file": "res/fonts/JetBrainsMono.ttf", "default": false, "fallback": false }
  ]
}
```

- [ ] **Step 3: 搬迁字体**

```bash
mkdir showcase/res/fonts
cp showcase_project/res/fonts/*.ttf showcase/res/fonts/
cp showcase_project/res/fonts/*.ttc showcase/res/fonts/
```

- [ ] **Step 4: 搬迁图标**

```bash
mkdir showcase/res/icons
cp showcase_project/res/icons/*.png showcase/res/icons/
```

- [ ] **Step 5: 写 Deep Ocean tokens.css（全部重写）**

Create `showcase/design-systems/tokens.css`:

```css
/* Deep Ocean 设计令牌。围栏不强制 var()，设计师/AI 也可直接写 hex。
   这里统一调色用。 */
:root {
  /* 色板 */
  --bg: #0e1620;
  --surface: #152433;
  --surface-2: #1a2f45;
  --border: #2a5a75;
  --accent: #5fb4d4;
  --accent-soft: #8ec5d8;
  --gold: #d4a44e;
  --fg: #e0e6ec;
  --muted: #9aa0b4;
  --dim: #6c7080;
  --success: #7db86a;
  --danger: #c2605a;

  /* 字号阶梯 */
  --text-xs: 12px;
  --text-sm: 14px;
  --text-base: 16px;
  --text-lg: 18px;
  --text-xl: 22px;
  --text-2xl: 28px;
  --text-3xl: 36px;
  --text-4xl: 48px;

  /* 间距 */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;

  /* 圆角 */
  --radius-sm: 4px;
  --radius-md: 8px;
  --radius-lg: 10px;
}
```

- [ ] **Step 6: 验证打包器能识别空工作区（应零 diagnostic，即便无 HTML）**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 打包成功，无 diagnostic 报错（packages.dirs 为空 HTML 时打包器应正常产出空 pkg 或最小 pkg）。

- [ ] **Step 7: Commit**

```bash
git add showcase/loom.workspace.json showcase/design-systems/tokens.css showcase/res
git commit -m "feat(showcase): scaffold workspace, migrate res, add Deep Ocean tokens"
```

> 旧 `showcase_project/` 此时暂不删，Task 15 全部搬完后删。

---

## Task 2: 预览基建（preview-base.css + loom-preview.js + README）

**Files:**
- Create: `showcase/showcase/preview/preview-base.css`
- Create: `showcase/showcase/preview/loom-preview.js`
- Create: `showcase/showcase/preview/README.md`

- [ ] **Step 1: 写 preview-base.css（UA 基线 polyfill）**

Create `showcase/showcase/preview/preview-base.css`:

```css
/* LoomGUI 浏览器预览基础样式（preview-only——打包器只消费 body，head 不进 pkg.bin）。
   polyfill 对齐 LoomGUI 渲染约定：
     - div 默认 flex column（围栏 DisplayDefault；贴近默认 flex 体感）
     - border-box（taffy）
     - body 无默认 margin
   预览壳：深色底 + 居中 + .root 设备框。letterbox 由 loom-preview.js 的 body 缩放做。 */
@font-face {
  font-family: 'LXGW WenKai';
  src: url('../../res/fonts/LXGWWenKai.ttf') format('truetype');
  font-display: block;
}
div { display: flex; flex-direction: column; }
* { box-sizing: border-box; }
body {
  margin: 0;
  font-family: 'LXGW WenKai', sans-serif;
  background: #05080d;
  display: flex;
  justify-content: center;
  align-items: flex-start;
  min-height: 100vh;
  padding: 24px;
}
.root { box-shadow: 0 0 0 1px #2a2f45, 0 8px 40px rgba(0,0,0,.6); }
button { background: none; border: none; color: inherit; font: inherit; padding: 0; cursor: pointer; }
a { color: inherit; text-decoration: none; }
```

- [ ] **Step 2: 写 loom-preview.js（导航 + 交互模拟 + letterbox）**

Create `showcase/showcase/preview/loom-preview.js`:

```js
// LoomGUI showcase 浏览器预览 driver（preview-only——打包器只消费 body）。
// 职责：导航 + role=tablist 切换 + 弹窗 display 切换 + ListView 视觉填充 +
//       NativeHost 占位 + body letterbox 缩放。
// 经典脚本（非 ES module）——避免 file:// 被 CORS 拦。
(function () {
  'use strict';

  var NAV = {
    'nav-settings': 'settings',
    'nav-inventory': 'inventory',
    'nav-mail': 'mail',
    'nav-shop': 'shop',
    'nav-character': 'character',
    'nav-form': 'form',
    'nav-lab': 'lab'
  };

  function $(id) { return document.getElementById(id); }
  function bind(id, type, fn) { var el = $(id); if (el) el.addEventListener(type, fn); }

  function goPage(name) {
    var dir = location.href.substring(0, location.href.lastIndexOf('/') + 1);
    location.href = dir + name + '.html';
  }

  // 导航：home nav-* → 各页；各页 #back-home → home
  function wireNav() {
    Object.keys(NAV).forEach(function (id) {
      bind(id, 'click', function () { goPage(NAV[id]); });
    });
    bind('back-home', 'click', function () { goPage('home'); });
  }

  // role=tablist 切换：role=tab 点击 → 切 role=tabpanel 显隐 + aria-selected
  function wireTabs() {
    var tabs = document.querySelectorAll('[role="tab"]');
    tabs.forEach(function (tab) {
      tab.addEventListener('click', function () {
        var list = tab.closest('[role="tablist"]');
        if (!list) return;
        list.querySelectorAll('[role="tab"]').forEach(function (t) {
          t.setAttribute('aria-selected', 'false');
        });
        tab.setAttribute('aria-selected', 'true');
        var target = tab.getAttribute('aria-controls');
        document.querySelectorAll('[role="tabpanel"]').forEach(function (p) {
          p.style.display = (p.id === target) ? '' : 'none';
        });
      });
    });
  }

  // 弹窗模拟：[data-open-dialog] → 显示 #dialog；[data-close-dialog] → 隐藏
  function wireDialogs() {
    document.querySelectorAll('[data-open-dialog]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var id = btn.getAttribute('data-open-dialog');
        var dlg = $(id);
        if (dlg) dlg.style.display = '';
      });
    });
    document.querySelectorAll('[data-close-dialog]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var dlg = btn.closest('[role="dialog"]');
        if (dlg) dlg.style.display = 'none';
      });
    });
  }

  // ListView 视觉填充：浏览器无真实虚拟化，把 <template> 克隆 N 份填进 <ul>。
+  // 只验视觉（不等高补偿/slot 复用属运行时）。
  function fillListViews() {
    document.querySelectorAll('ul[data-fill], ol[data-fill]').forEach(function (ul) {
      var tpl = ul.querySelector('template');
      if (!tpl) return;
      var count = parseInt(ul.getAttribute('data-fill'), 10) || 8;
      for (var i = 1; i < count; i++) {
        ul.appendChild(tpl.content.cloneNode(true));
      }
    });
  }

  // NativeHost 占位：canvas#native-slot 渲染占位文本（运行时渲染 3D + 特效）。
  function fillNativeHost() {
    var cv = document.getElementById('native-slot');
    if (!cv || cv.tagName.toLowerCase() !== 'canvas') return;
    var ctx = cv.getContext && cv.getContext('2d');
    if (!ctx) return;
    ctx.fillStyle = '#0e1620';
    ctx.fillRect(0, 0, cv.width, cv.height);
    ctx.fillStyle = '#5fb4d4';
    ctx.font = '20px "LXGW WenKai", sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('NativeHost 3D 角色 slot', cv.width / 2, cv.height / 2 - 10);
    ctx.fillStyle = '#9aa0b4';
    ctx.font = '14px "LXGW WenKai", sans-serif';
    ctx.fillText('运行时渲染角色模型 + 粒子特效', cv.width / 2, cv.height / 2 + 18);
  }

  // body letterbox 缩放：按视口等比缩放 .root，模拟 engine letterbox。
  function fitScale() {
    var root = document.querySelector('.root');
    if (!root) return;
    var rw = 1920, rh = 1080;
    var sw = (window.innerWidth - 48) / rw;
    var sh = (window.innerHeight - 48) / rh;
    var s = Math.min(sw, sh, 1);
    document.body.style.zoom = s;
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
  function init() {
    wireNav();
    wireTabs();
    wireDialogs();
    fillListViews();
    fillNativeHost();
    fitScale();
    window.addEventListener('resize', fitScale);
  }
})();
```

- [ ] **Step 3: 写 README.md（可信度说明）**

Create `showcase/showcase/preview/README.md`:

````markdown
# Showcase 浏览器预览

双击 `../home.html` 即可在浏览器预览 showcase（视觉对照用，非行为镜像）。

## 可信（对齐运行时）
flex 布局/方向/gap/justify/align；px/% 尺寸；color/opacity/border/radius；
background-image/size；filter；transform；overflow:scroll；九宫 border-image-slice；列表骨架。

## 近技（抓布局偏，不像素级）
- 文本换行/字距：Chrome 引擎 vs unicode-linebreak，换行点偶偏。
- tween 动画：CSS transition 近似，不逐曲线对 ease。
- drag/longpress/key 事件：浏览器事件近技。

## 纯运行时（不在 HTML 镜像）
TweenManager 逐曲线 ease、虚拟列表 slot 复用/不等高补偿、NativeHost 3D/粒子、事件系统、overlay 叠加时序。

## 维护
- 改了 showcase HTML：刷新浏览器即可。
- 导航表（NAV）在 loom-preview.js 顶部，新增页要加映射。
````

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/preview
git commit -m "feat(showcase): add preview infrastructure (base css, preview js, readme)"
```

---

## Task 3: home.html — 导航枢纽

**Files:**
- Create: `showcase/showcase/home.html`

- [ ] **Step 1: 写 home.html（hero + 7 nav 卡片 + 底部信息条）**

Create `showcase/showcase/home.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 首页</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; padding:48px; gap:32px; }
  .hero { flex-direction:row; align-items:center; gap:24px; }
  .hero-logo { width:96px; height:96px; }
  .hero-title { color:#e0e6ec; font-size:48px; font-weight:700; }
  .hero-sub { color:#9aa0b4; font-size:16px; margin-top:6px; }
  .hero-version { color:#d4a44e; font-size:14px; font-weight:700; }
  .actions { flex-direction:row; gap:16px; }
  .btn-primary { background-color:#d4a44e; color:#0e1620; font-size:22px; font-weight:700; padding:14px 40px; border-radius:8px; }
  .btn-ghost { background-color:#152433; color:#8ec5d8; font-size:18px; padding:14px 32px; border-radius:8px; }
  .btn-primary:hover { filter:brightness(1.12); }
  .btn-ghost:hover { background-color:#1a2f45; }
  .nav-grid { flex-direction:row; flex-wrap:wrap; gap:24px; }
  .nav-card { width:420px; background-color:#152433; border-radius:10px; padding:24px; gap:12px; flex-direction:row; align-items:center; transition:transform .18s, background-color .18s; }
  .nav-card:hover { transform:translateY(-4px); background-color:#1a2f45; }
  .nav-card-icon { width:56px; height:56px; }
  .nav-card-body { gap:4px; flex:1; }
  .nav-card-label { color:#e0e6ec; font-size:22px; font-weight:700; }
  .nav-card-desc { color:#9aa0b4; font-size:14px; }
  .nav-card-arrow { color:#5fb4d4; font-size:24px; }
  .footbar { flex-direction:row; justify-content:space-between; align-items:center; }
  .foot-text { color:#6c7080; font-size:13px; }
  .foot-strong { color:#8ec5d8; font-weight:700; }
  @keyframes fadeIn { from { opacity:0; transform:translateY(12px); } to { opacity:1; transform:translateY(0); } }
  .nav-card { animation:fadeIn .4s both; }
  .nav-card:nth-child(1){animation-delay:.05s}.nav-card:nth-child(2){animation-delay:.1s}
  .nav-card:nth-child(3){animation-delay:.15s}.nav-card:nth-child(4){animation-delay:.2s}
  .nav-card:nth-child(5){animation-delay:.25s}.nav-card:nth-child(6){animation-delay:.3s}
  .nav-card:nth-child(7){animation-delay:.35s}
  </style>
</head>
<body>
  <div class="root">
    <div class="hero">
      <img class="hero-logo" src="../res/icons/zap.png" alt="logo">
      <div style="gap:6px">
        <p class="hero-title">LoomGUI 靶子冻结 Showcase</p>
        <p class="hero-sub">围栏全标签 + CSS 全白名单验收靶子 · <span class="hero-version">R2–R7 运行时重写冲刺目标</span></p>
      </div>
    </div>
    <div class="actions">
      <button class="btn-primary">开始游戏</button>
      <button class="btn-ghost">继续</button>
      <button class="btn-ghost">退出</button>
    </div>
    <nav class="nav-grid">
      <a class="nav-card" id="nav-settings" href="settings.html">
        <img class="nav-card-icon" src="../res/icons/cpu.png" alt="settings">
        <div class="nav-card-body"><span class="nav-card-label">设置</span><span class="nav-card-desc">input 全 7 型 · select · role dispatch</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
      <a class="nav-card" id="nav-inventory" href="inventory.html">
        <img class="nav-card-icon" src="../res/icons/box.png" alt="inventory">
        <div class="nav-card-body"><span class="nav-card-label">背包</span><span class="nav-card-desc">ListView 虚拟化主战场 · 物品网格</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
      <a class="nav-card" id="nav-mail" href="mail.html">
        <img class="nav-card-icon" src="../res/icons/list.png" alt="mail">
        <div class="nav-card-body"><span class="nav-card-label">邮件</span><span class="nav-card-desc">ListView + 富文本全家族</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
      <a class="nav-card" id="nav-shop" href="shop.html">
        <img class="nav-card-icon" src="../res/icons/image.png" alt="shop">
        <div class="nav-card-body"><span class="nav-card-label">商店</span><span class="nav-card-desc">progress + 弹窗 + 数量输入</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
      <a class="nav-card" id="nav-character" href="character.html">
        <img class="nav-card-icon" src="../res/icons/skin.png" alt="character">
        <div class="nav-card-body"><span class="nav-card-label">角色</span><span class="nav-card-desc">canvas 3D + 特效混合渲染</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
      <a class="nav-card" id="nav-form" href="form.html">
        <img class="nav-card-icon" src="../res/icons/type.png" alt="form">
        <div class="nav-card-body"><span class="nav-card-label">角色创建</span><span class="nav-card-desc">表单控件编排 · div+p 分组</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
      <a class="nav-card" id="nav-lab" href="lab.html">
        <img class="nav-card-icon" src="../res/icons/palette.png" alt="lab">
        <div class="nav-card-body"><span class="nav-card-label">标本馆</span><span class="nav-card-desc">CSS 全属性 specimen</span></div>
        <span class="nav-card-arrow">→</span>
      </a>
    </nav>
    <div class="footbar">
      <span class="foot-text">靶子冻结期：围栏标签 23 · CSS 白名单全覆盖</span>
      <span class="foot-text"><strong class="foot-strong">HTML 展示优先</strong> · 运行时行为由 C# 辅助代码兑现</span>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic（home.html 进 pkg 成功）。

- [ ] **Step 3: 浏览器可视**

双击 `showcase/showcase/home.html`，确认 hero、3 个操作按钮、7 张 nav 卡片、底部信息条渲染正常；hover 卡片有抬升 + 序列入场动画。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/home.html
git commit -m "feat(showcase): add home navigation hub page"
```

---

## Task 4: settings.html — 控件全覆盖

**Files:**
- Create: `showcase/showcase/settings.html`

- [ ] **Step 1: 写 settings.html（左 tablist + 右 panel，7 input 变体 + select）**

Create `showcase/showcase/settings.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 设置</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; flex-direction:row; }
  .sidebar { width:260px; background-color:#152433; padding:32px 20px; gap:6px; border-width:0 1px 0 0; border-color:#2a5a75; }
  .side-back { color:#8ec5d8; font-size:14px; margin-bottom:20px; }
  .side-title { color:#e0e6ec; font-size:22px; font-weight:700; margin-bottom:20px; }
  .tab { text-align:left; color:#9aa0b4; font-size:16px; padding:12px 16px; border-radius:8px; }
  .tab:hover { background-color:#1a2f45; }
  .tab[aria-selected="true"] { background-color:#1a2f45; color:#5fb4d4; font-weight:700; }
  .main { flex:1; padding:48px; gap:32px; overflow-y:auto; }
  .page-title { color:#e0e6ec; font-size:36px; font-weight:700; }
  .page-desc { color:#9aa0b4; font-size:15px; }
  .field { flex-direction:row; align-items:center; justify-content:space-between; gap:24px; padding:14px 0; border-width:0 0 1px 0; border-color:#1a2f45; }
  .field-label { color:#e0e6ec; font-size:16px; }
  .field-hint { color:#6c7080; font-size:13px; margin-top:3px; }
  .field-control { flex-direction:row; align-items:center; gap:12px; }
  .val { color:#d4a44e; font-size:14px; min-width:48px; text-align:right; }
  input[type="range"] { width:280px; accent-color:#5fb4d4; }
  input[type="number"], input[type="text"], input[type="password"], input[type="search"] { background-color:#152433; border-width:0 0 0 2px; border-color:#2a5a75; color:#e0e6ec; font-size:15px; padding:8px 12px; border-radius:4px; width:240px; }
  select { background-color:#152433; border-width:0 0 0 2px; border-color:#2a5a75; color:#e0e6ec; font-size:15px; padding:8px 12px; border-radius:4px; }
  .checkbox-row { flex-direction:row; align-items:center; gap:10px; }
  .radio-group { flex-direction:row; gap:20px; }
  .radio-opt { flex-direction:row; align-items:center; gap:6px; color:#9aa0b4; font-size:15px; }
  input[type="checkbox"], input[type="radio"] { accent-color:#5fb4d4; }
  .panel { transition:opacity .2s; }
  </style>
</head>
<body>
  <div class="root">
    <div class="sidebar">
      <button class="side-back" id="back-home">← 返回首页</button>
      <p class="side-title">设置</p>
      <div role="tablist">
        <button class="tab" role="tab" id="tab-audio" aria-controls="panel-audio" aria-selected="true">音效</button>
        <button class="tab" role="tab" id="tab-graphics" aria-controls="panel-graphics" aria-selected="false">画面</button>
        <button class="tab" role="tab" id="tab-controls" aria-controls="panel-controls" aria-selected="false">操作</button>
        <button class="tab" role="tab" id="tab-account" aria-controls="panel-account" aria-selected="false">账号</button>
        <button class="tab" role="tab" id="tab-search" aria-controls="panel-search" aria-selected="false">搜索</button>
      </div>
    </div>
    <div class="main">
      <div role="tabpanel" id="panel-audio" class="panel">
        <p class="page-title">音效</p>
        <p class="page-desc">input[type=range] · input[type=number]</p>
        <div class="field">
          <div><span class="field-label">主音量</span><p class="field-hint">range 0–100</p></div>
          <div class="field-control"><input type="range" id="vol-master" min="0" max="100" value="80"><span class="val">80</span></div>
        </div>
        <div class="field">
          <div><span class="field-label">音效音量</span><p class="field-hint">range 0–100</p></div>
          <div class="field-control"><input type="range" id="vol-sfx" min="0" max="100" value="65"><span class="val">65</span></div>
        </div>
        <div class="field">
          <div><span class="field-label">最多同时发声数</span><p class="field-hint">input[type=number] min/max/step</p></div>
          <div class="field-control"><input type="number" id="snd-voices" min="1" max="64" step="1" value="32"><span class="val"> voices</span></div>
        </div>
      </div>
      <div role="tabpanel" id="panel-graphics" class="panel" style="display:none">
        <p class="page-title">画面</p>
        <p class="page-desc">range · select/option · checkbox</p>
        <div class="field">
          <div><span class="field-label">亮度</span><p class="field-hint">range 0–100</p></div>
          <div class="field-control"><input type="range" id="gfx-bright" min="0" max="100" value="50"><span class="val">50</span></div>
        </div>
        <div class="field">
          <div><span class="field-label">分辨率</span><p class="field-hint">select + option</p></div>
          <div class="field-control">
            <select id="gfx-res">
              <option value="1080">1920×1080</option>
              <option value="1440">2560×1440</option>
              <option value="4k">3840×2160</option>
            </select>
          </div>
        </div>
        <div class="field">
          <div><span class="field-label">全屏模式</span></div>
          <div class="checkbox-row"><input type="checkbox" id="gfx-fullscreen" checked><label for="gfx-fullscreen">全屏</label></div>
        </div>
        <div class="field">
          <div><span class="field-label">垂直同步</span></div>
          <div class="checkbox-row"><input type="checkbox" id="gfx-vsync"><label for="gfx-vsync">启用</label></div>
        </div>
      </div>
      <div role="tabpanel" id="panel-controls" class="panel" style="display:none">
        <p class="page-title">操作</p>
        <p class="page-desc">radio · text · label[for]</p>
        <div class="field">
          <div><span class="field-label">按键方案</span></div>
          <div class="radio-group">
            <div class="radio-opt"><input type="radio" id="key-default" name="keyscheme" checked><label for="key-default">默认</label></div>
            <div class="radio-opt"><input type="radio" id="key-alt" name="keyscheme"><label for="key-alt">备用</label></div>
            <div class="radio-opt"><input type="radio" id="key-lefty" name="keyscheme"><label for="key-lefty">左撇子</label></div>
          </div>
        </div>
        <div class="field">
          <div><span class="field-label">自定义按键</span><p class="field-hint">input[type=text]</p></div>
          <div class="field-control"><input type="text" id="key-custom" placeholder="按键序列" value="Space"></div>
        </div>
      </div>
      <div role="tabpanel" id="panel-account" class="panel" style="display:none">
        <p class="page-title">账号</p>
        <p class="page-desc">text · password · checkbox</p>
        <div class="field">
          <div><span class="field-label">账号名</span></div>
          <div class="field-control"><input type="text" id="acc-name" placeholder="账号" value="player_01"></div>
        </div>
        <div class="field">
          <div><span class="field-label">密码</span></div>
          <div class="field-control"><input type="password" id="acc-pwd" placeholder="密码" value="secret"></div>
        </div>
        <div class="field">
          <div><span class="field-label">记住账号</span></div>
          <div class="checkbox-row"><input type="checkbox" id="acc-remember" checked><label for="acc-remember">记住</label></div>
        </div>
      </div>
      <div role="tabpanel" id="panel-search" class="panel" style="display:none">
        <p class="page-title">搜索</p>
        <p class="page-desc">input[type=search]（围栏内 text 第 7 变体）</p>
        <div class="field">
          <div><span class="field-label">搜索设置项</span></div>
          <div class="field-control"><input type="search" id="set-search" placeholder="输入关键词"></div>
        </div>
      </div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。注意验证：label[for] 目标存在、select 只含 option、role=tab/tabpanel 通过。

- [ ] **Step 3: 浏览器可视**

双击 home.html → 点"设置"卡片 → 确认左 tablist 切换右 panel；逐 panel 验证 range/number/select/checkbox/radio/text/password/search 全部渲染。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/settings.html
git commit -m "feat(showcase): add settings page with all input types + role dispatch"
```

---

## Task 5: components（nav-bar / stat-bar / item-card）

**Files:**
- Create: `showcase/showcase/components/nav-bar.html`
- Create: `showcase/showcase/components/stat-bar.html`
- Create: `showcase/showcase/components/item-card.html`

- [ ] **Step 1: 写 nav-bar.html（custom-element 风格共享顶栏片段）**

Create `showcase/showcase/components/nav-bar.html`:

```html
<!-- 共享顶栏片段（设计期参考；运行时由 C# 封装为组件）。
     各子页在 .root 顶部插入这段结构。 -->
<header class="topbar" style="flex-direction:row;align-items:center;justify-content:space-between;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
  <div style="flex-direction:row;align-items:center;gap:12px">
    <button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button>
    <span style="color:#e0e6ec;font-size:20px;font-weight:700">LoomGUI Showcase</span>
  </div>
  <nav style="flex-direction:row;gap:18px">
    <a href="settings.html" style="color:#9aa0b4;font-size:14px">设置</a>
    <a href="inventory.html" style="color:#9aa0b4;font-size:14px">背包</a>
    <a href="mail.html" style="color:#9aa0b4;font-size:14px">邮件</a>
    <a href="shop.html" style="color:#9aa0b4;font-size:14px">商店</a>
    <a href="character.html" style="color:#9aa0b4;font-size:14px">角色</a>
    <a href="form.html" style="color:#9aa0b4;font-size:14px">创建</a>
    <a href="lab.html" style="color:#9aa0b4;font-size:14px">标本馆</a>
  </nav>
</header>
```

- [ ] **Step 2: 写 stat-bar.html（stat + progress 组件，custom-element 风格）**

Create `showcase/showcase/components/stat-bar.html`:

```html
<!-- stat-bar 组件片段：标签 + progress。character/inventory 复用。
     用法：把 .stat-bar 块复制到页面，改 label/value/max/color。 -->
<div class="stat-bar" style="flex-direction:row;align-items:center;gap:12px">
  <span style="color:#9aa0b4;font-size:14px;min-width:48px">HP</span>
  <progress value="780" max="1000" style="flex:1;height:14px;accent-color:#c2605a"></progress>
  <span style="color:#e0e6ec;font-size:14px;min-width:72px;text-align:right">780 / 1000</span>
</div>
```

- [ ] **Step 3: 写 item-card.html（custom-element + slot 投影示例）**

Create `showcase/showcase/components/item-card.html`:

```html
<!-- item-card 自定义元素 + slot 投影示例。
     含 hyphen 标签名 = CustomElement；<slot> 接投影内容。
     lab.html 会演示真正投影；这里给出组件骨架。 -->
<item-card style="flex-direction:column;gap:8px;width:160px;background-color:#152433;border-radius:10px;padding:16px">
  <img src="../res/icons/box.png" alt="item" style="width:64px;height:64px;align-self:center">
  <slot name="item-name"><span style="color:#e0e6ec;font-size:16px;font-weight:700;align-self:center">物品名</span></slot>
  <slot name="item-meta"><span style="color:#9aa0b4;font-size:13px;align-self:center">描述</span></slot>
</item-card>
```

- [ ] **Step 4: 验证围栏打包门（components 进 pkg）**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。含 hyphen 标签名 `<item-card>` 通过 CustomElement 放行；`<slot>` 通过。

- [ ] **Step 5: Commit**

```bash
git add showcase/showcase/components
git commit -m "feat(showcase): add shared components (nav-bar, stat-bar, item-card)"
```

---

## Task 6: inventory.html — 背包 ListView

**Files:**
- Create: `showcase/showcase/inventory.html`

- [ ] **Step 1: 写 inventory.html（左 ListView + 右详情，ul/template/li/img/progress）**

Create `showcase/showcase/inventory.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 背包</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; }
  .body { flex:1; flex-direction:row; gap:24px; padding:24px; overflow:hidden; }
  .col-list { width:680px; flex-direction:column; gap:12px; overflow-y:auto; }
  .col-title { color:#e0e6ec; font-size:22px; font-weight:700; margin-bottom:4px; }
  .grid { flex-direction:row; flex-wrap:wrap; gap:12px; }
  .item { width:120px; height:120px; background-color:#152433; border-radius:8px; padding:10px; gap:6px; align-items:center; position:relative; transition:transform .15s, background-color .15s; }
  .item:hover { transform:scale(1.05); background-color:#1a2f45; }
  .item.selected { border-width:0 0 0 3px; border-color:#d4a44e; }
  .item-icon { width:56px; height:56px; }
  .item-badge { position:absolute; top:6px; right:6px; background-color:#d4a44e; color:#0e1620; font-size:12px; font-weight:700; padding:1px 6px; border-radius:8px; }
  .item-dur { width:80px; }
  .col-detail { flex:1; background-color:#152433; border-radius:12px; padding:32px; gap:16px; overflow-y:auto; }
  .detail-icon { width:160px; height:160px; align-self:center; }
  .detail-name { color:#e0e6ec; font-size:30px; font-weight:700; align-self:center; }
  .detail-rarity { color:#d4a44e; font-size:15px; align-self:center; }
  .detail-desc { color:#9aa0b4; font-size:15px; line-height:1.7; }
  .detail-attrs { flex-direction:row; gap:24px; justify-content:center; }
  .attr { flex-direction:column; align-items:center; gap:4px; }
  .attr-val { color:#8ec5d8; font-size:22px; font-weight:700; }
  .attr-label { color:#6c7080; font-size:13px; }
  .detail-actions { flex-direction:row; gap:12px; justify-content:center; margin-top:8px; }
  .btn { padding:10px 24px; border-radius:8px; font-size:15px; font-weight:700; }
  .btn-use { background-color:#5fb4d4; color:#0e1620; }
  .btn-drop { background-color:#152433; color:#c2605a; }
  </style>
</head>
<body>
  <div class="root">
    <header class="topbar" style="flex-direction:row;align-items:center;justify-content:space-between;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
      <div style="flex-direction:row;align-items:center;gap:12px"><button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button><span style="color:#e0e6ec;font-size:20px;font-weight:700">背包</span></div>
    </header>
    <div class="body">
      <div class="col-list">
        <p class="col-title">物品 · ListView</p>
        <ul class="grid" data-fill="10">
          <template>
            <li class="item">
              <span class="item-badge">x9</span>
              <img class="item-icon" src="../res/icons/box.png" alt="item">
              <progress class="item-dur" value="70" max="100"></progress>
            </li>
          </template>
        </ul>
      </div>
      <div class="col-detail">
        <img class="detail-icon" src="../res/icons/box.png" alt="detail">
        <span class="detail-name">精铁宝箱</span>
        <span class="detail-rarity">史诗 · 可堆叠</span>
        <p class="detail-desc">从远古遗迹中发掘的精铁宝箱，开启后有概率获得稀有装备与材料。耐久度归零后碎裂。</p>
        <div class="detail-attrs">
          <div class="attr"><span class="attr-val">+128</span><span class="attr-label">攻击</span></div>
          <div class="attr"><span class="attr-val">+64</span><span class="attr-label">防御</span></div>
          <div class="attr"><span class="attr-val">+12%</span><span class="attr-label">暴击</span></div>
        </div>
        <p class="detail-desc"><strong>套装效果：</strong><em>2 件</em>提升 15% 移速；<em>4 件</em>触发反击。</p>
        <div class="detail-actions">
          <button class="btn btn-use">使用</button>
          <button class="btn btn-drop">丢弃</button>
        </div>
      </div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。ul > template（根 li）通过 ListView 契约；li 内 img/progress/span 通过。

- [ ] **Step 3: 浏览器可视**

双击 home → 背包 → 确认左侧物品格（preview 克隆 10 份）、右侧详情面板、hover 缩放、详情属性富文本（strong/em）。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/inventory.html
git commit -m "feat(showcase): add inventory page with ListView + detail panel"
```

---

## Task 7: mail.html — 邮件 ListView + 富文本

**Files:**
- Create: `showcase/showcase/mail.html`

- [ ] **Step 1: 写 mail.html（左邮件列表 + 右富文本正文，搬旧 page_text B1–B9 富文本）**

Create `showcase/showcase/mail.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 邮件</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; }
  .body { flex:1; flex-direction:row; gap:24px; padding:24px; overflow:hidden; }
  .col-list { width:560px; gap:12px; overflow-y:auto; }
  .col-title { color:#e0e6ec; font-size:22px; font-weight:700; margin-bottom:4px; }
  .mail-item { flex-direction:row; gap:14px; align-items:center; padding:16px; background-color:#152433; border-radius:10px; transition:background-color .15s; }
  .mail-item:hover { background-color:#1a2f45; }
  .mail-dot { width:10px; height:10px; border-radius:5px; background-color:#d4a44e; }
  .mail-dot.read { background-color:#3a5060; }
  .mail-body { flex:1; gap:4px; }
  .mail-from { color:#e0e6ec; font-size:16px; font-weight:700; }
  .mail-sub { color:#9aa0b4; font-size:14px; }
  .col-read { flex:1; background-color:#152433; border-radius:12px; padding:40px; gap:18px; overflow-y:auto; }
  .read-title { color:#e0e6ec; font-size:28px; font-weight:700; }
  .read-meta { color:#6c7080; font-size:14px; }
  .read-body { color:#dfe4ec; font-size:16px; line-height:1.9; gap:14px; }
  .loot { color:#d4a44e; font-weight:700; }
  .sys { color:#5fb4d4; font-weight:700; }
  .enemy { color:#c2605a; font-weight:700; }
  @keyframes breathe { 0%,100%{opacity:1} 50%{opacity:.4} }
  .mail-dot { animation:breathe 1.6s infinite; }
  </style>
</head>
<body>
  <div class="root">
    <header class="topbar" style="flex-direction:row;align-items:center;gap:12px;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
      <button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button><span style="color:#e0e6ec;font-size:20px;font-weight:700">邮件</span>
    </header>
    <div class="body">
      <div class="col-list">
        <p class="col-title">收件箱 · ListView</p>
        <ul data-fill="6" style="gap:12px">
          <template>
            <li class="mail-item">
              <div class="mail-dot"></div>
              <div class="mail-body"><span class="mail-from">系统奖励</span><span class="mail-sub">每日登录奖励已发放</span></div>
            </li>
          </template>
        </ul>
      </div>
      <div class="col-read">
        <p class="read-title">赛季结算奖励</p>
        <p class="read-meta">来自：竞技场 · 2 小时前</p>
        <div class="read-body">
          <p>勇士你好，<br>本赛季已结束，你的最终排名为 <span class="loot">127</span>。</p>
          <p>奖励清单：<span class="loot">金币 ×5000</span>、<span class="loot">传说宝箱 ×1</span>、<span class="sys">称号「不屈」</span>。</p>
          <p><strong>注意：</strong>奖励将于 <em>7 天内</em>发放，请及时领取。<a href="#" style="color:#5fb4d4;text-decoration:underline">点此领取全部奖励</a></p>
          <p>击败 <span class="enemy">暗影领主</span> 可额外获得赛季专属外观。详细规则见 <a href="#" style="color:#5fb4d4;text-decoration:underline">赛季手册</a>。</p>
        </div>
      </div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。p > strong/em/br/a/span 全部 Phrasing-in-Phrasing 通过。

- [ ] **Step 3: 浏览器可视**

双击 home → 邮件 → 确认左侧邮件列表（preview 克隆 6 份 + 未读点呼吸动画）、右侧富文本（粗体/斜体/硬换行/超链接/多色 span）。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/mail.html
git commit -m "feat(showcase): add mail page with ListView + rich text"
```

---

## Task 8: shop.html — 商店 progress + 弹窗

**Files:**
- Create: `showcase/showcase/shop.html`

- [ ] **Step 1: 写 shop.html（商品网格 + progress 倒计时/库存 + 购买弹窗 div role=dialog）**

Create `showcase/showcase/shop.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 商店</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; }
  .body { flex:1; padding:32px; gap:24px; overflow-y:auto; }
  .page-title { color:#e0e6ec; font-size:28px; font-weight:700; }
  .grid { flex-direction:row; flex-wrap:wrap; gap:24px; }
  .product { width:360px; background-color:#152433; border-radius:12px; padding:20px; gap:14px; transition:transform .15s; }
  .product:hover { transform:translateY(-6px); }
  .product-img { width:100%; height:160px; background-color:#1a2f45; border-radius:8px; align-self:center; }
  .product-name { color:#e0e6ec; font-size:18px; font-weight:700; }
  .product-price { color:#d4a44e; font-size:20px; font-weight:700; }
  .product-meta { flex-direction:row; justify-content:space-between; align-items:center; }
  .stock-row { flex-direction:row; align-items:center; gap:8px; }
  .stock-label { color:#9aa0b4; font-size:13px; min-width:56px; }
  .btn-buy { background-color:#d4a44e; color:#0e1620; font-size:15px; font-weight:700; padding:10px 20px; border-radius:8px; align-self:flex-start; }
  .btn-buy:hover { filter:brightness(1.12); }
  .link-detail { color:#5fb4d4; font-size:14px; }
  /* 弹窗（运行时由 display 切换；preview JS 模拟） */
  .dialog-overlay { position:absolute; top:0; left:0; width:1920px; height:1080px; background-color:#000a; justify-content:center; align-items:center; }
  .dialog { width:480px; background-color:#152433; border-radius:14px; padding:32px; gap:20px; box-shadow:0 12px 48px #000c; }
  .dialog-title { color:#e0e6ec; font-size:22px; font-weight:700; }
  .dialog-row { flex-direction:row; align-items:center; justify-content:space-between; }
  .dialog-label { color:#9aa0b4; font-size:15px; }
  .qty { background-color:#1a2f45; border-width:0 0 0 2px; border-color:#2a5a75; color:#e0e6ec; font-size:18px; padding:8px 12px; border-radius:6px; width:90px; text-align:center; }
  .balance-row { flex-direction:row; align-items:center; gap:10px; }
  .dialog-actions { flex-direction:row; gap:12px; justify-content:flex-end; }
  .btn-confirm { background-color:#5fb4d4; color:#0e1620; font-size:15px; font-weight:700; padding:10px 24px; border-radius:8px; }
  .btn-cancel { background-color:#1a2f45; color:#9aa0b4; font-size:15px; padding:10px 24px; border-radius:8px; }
  </style>
</head>
<body>
  <div class="root">
    <header class="topbar" style="flex-direction:row;align-items:center;gap:12px;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
      <button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button><span style="color:#e0e6ec;font-size:20px;font-weight:700">商店</span>
    </header>
    <div class="body">
      <p class="page-title">限时折扣</p>
      <div class="grid">
        <div class="product">
          <img class="product-img" src="../res/icons/image.png" alt="product">
          <span class="product-name">新手礼包</span>
          <span class="product-price">¥ 6</span>
          <div class="product-meta">
            <div class="stock-row"><span class="stock-label">倒计时</span><progress value="35" max="100" style="flex:1;accent-color:#d4a44e"></progress></div>
            <a class="link-detail" href="#">详情</a>
          </div>
          <button class="btn-buy" data-open-dialog="buy-dialog">购买</button>
        </div>
        <div class="product">
          <img class="product-img" src="../res/icons/skin.png" alt="product">
          <span class="product-name">勇者外观包</span>
          <span class="product-price">¥ 30</span>
          <div class="product-meta">
            <div class="stock-row"><span class="stock-label">库存</span><progress value="72" max="100" style="flex:1;accent-color:#5fb4d4"></progress></div>
            <a class="link-detail" href="#">详情</a>
          </div>
          <button class="btn-buy" data-open-dialog="buy-dialog">购买</button>
        </div>
        <div class="product">
          <img class="product-img" src="../res/icons/box.png" alt="product">
          <span class="product-name">传说宝箱</span>
          <span class="product-price">¥ 98</span>
          <div class="product-meta">
            <div class="stock-row"><span class="stock-label">倒计时</span><progress value="8" max="100" style="flex:1;accent-color:#c2605a"></progress></div>
            <a class="link-detail" href="#">详情</a>
          </div>
          <button class="btn-buy" data-open-dialog="buy-dialog">购买</button>
        </div>
      </div>
    </div>
    <div class="dialog-overlay" role="dialog" id="buy-dialog" style="display:none">
      <div class="dialog">
        <span class="dialog-title">确认购买</span>
        <div class="dialog-row">
          <span class="dialog-label">购买数量</span>
          <input type="number" class="qty" min="1" max="99" step="1" value="1">
        </div>
        <div class="dialog-row balance-row">
          <span class="dialog-label">余额</span>
          <progress value="4200" max="5000" style="width:200px;accent-color:#d4a44e"></progress>
        </div>
        <div class="dialog-actions">
          <button class="btn-cancel" data-close-dialog>取消</button>
          <button class="btn-confirm" data-close-dialog>确认</button>
        </div>
      </div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。div role=dialog 通过；input[type=number] min/max/step 通过；data-* 透传通过。

- [ ] **Step 3: 浏览器可视**

双击 home → 商店 → 确认 3 个商品卡（hover 抬升）、倒计时/库存 progress 条、点"购买"弹窗缩放进入、数量输入 + 余额 progress、取消/确认关闭弹窗。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/shop.html
git commit -m "feat(showcase): add shop page with progress bars + purchase dialog"
```

---

## Task 9: character.html — canvas 3D + 特效混合

**Files:**
- Create: `showcase/showcase/character.html`

- [ ] **Step 1: 写 character.html（canvas#native-slot + HP/MP/EXP progress + ol 技能列表 + 装备槽）**

Create `showcase/showcase/character.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 角色</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; }
  .body { flex:1; flex-direction:row; gap:24px; padding:24px; overflow:hidden; }
  .col-stage { width:720px; gap:16px; }
  .native-slot { width:720px; height:680px; background-color:#0a121b; border-radius:12px; border-width:2px; border-color:#2a5a75; }
  .stage-info { flex-direction:row; justify-content:space-between; }
  .stage-name { color:#e0e6ec; font-size:28px; font-weight:700; }
  .stage-lv { color:#d4a44e; font-size:18px; font-weight:700; }
  .stat-bars { gap:10px; padding:0 4px; }
  .stat-row { flex-direction:row; align-items:center; gap:12px; }
  .stat-label { color:#9aa0b4; font-size:14px; min-width:52px; }
  .stat-val { color:#e0e6ec; font-size:14px; min-width:90px; text-align:right; }
  .col-side { flex:1; gap:20px; overflow-y:auto; }
  .panel { background-color:#152433; border-radius:12px; padding:24px; gap:14px; }
  .panel-title { color:#8ec5d8; font-size:16px; font-weight:700; }
  .equip-grid { flex-direction:row; flex-wrap:wrap; gap:10px; }
  .equip { width:72px; height:72px; background-color:#1a2f45; border-radius:8px; justify-content:center; align-items:center; transition:filter .15s; }
  .equip:hover { filter:brightness(1.3); }
  .equip img { width:44px; height:44px; }
  .skill { flex-direction:row; align-items:center; gap:14px; padding:10px 0; border-width:0 0 1px 0; border-color:#1a2f45; transition:background-color .15s; }
  .skill:hover { background-color:#1a2f45; }
  .skill-no { color:#6c7080; font-size:14px; min-width:24px; }
  .skill-icon { width:40px; height:40px; }
  .skill-name { color:#e0e6ec; font-size:15px; flex:1; }
  .skill-lv { color:#d4a44e; font-size:14px; font-weight:700; }
  @keyframes charge { from{filter:brightness(.7)} to{filter:brightness(1)} }
  progress { animation:charge 2s infinite alternate; }
  </style>
</head>
<body>
  <div class="root">
    <header class="topbar" style="flex-direction:row;align-items:center;gap:12px;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
      <button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button><span style="color:#e0e6ec;font-size:20px;font-weight:700">角色</span>
    </header>
    <div class="body">
      <div class="col-stage">
        <canvas class="native-slot" id="native-slot" width="720" height="680"></canvas>
        <div class="stage-info">
          <span class="stage-name">暗影游侠 · <em>影刃</em></span>
          <span class="stage-lv">Lv.58</span>
        </div>
        <div class="stat-bars">
          <div class="stat-row"><span class="stat-label">HP</span><progress value="780" max="1000" style="flex:1;height:16px;accent-color:#c2605a"></progress><span class="stat-val">780/1000</span></div>
          <div class="stat-row"><span class="stat-label">MP</span><progress value="340" max="600" style="flex:1;height:16px;accent-color:#5fb4d4"></progress><span class="stat-val">340/600</span></div>
          <div class="stat-row"><span class="stat-label">EXP</span><progress value="62" max="100" style="flex:1;height:16px;accent-color:#d4a44e"></progress><span class="stat-val">62%</span></div>
        </div>
      </div>
      <div class="col-side">
        <div class="panel">
          <span class="panel-title">装备槽</span>
          <div class="equip-grid">
            <div class="equip"><img src="../res/icons/skin.png" alt="helm"></div>
            <div class="equip"><img src="../res/icons/box.png" alt="armor"></div>
            <div class="equip"><img src="../res/icons/cpu.png" alt="weapon"></div>
            <div class="equip"><img src="../res/icons/zap.png" alt="ring"></div>
            <div class="equip"><img src="../res/icons/hand.png" alt="glove"></div>
            <div class="equip"><img src="../res/icons/eye.png" alt="boots"></div>
          </div>
        </div>
        <div class="panel">
          <span class="panel-title">技能（ol 有序列表）</span>
          <ol style="gap:4px">
            <li class="skill"><span class="skill-no">1</span><img class="skill-icon" src="../res/icons/zap.png" alt="skill"><span class="skill-name">影袭</span><span class="skill-lv">Lv.9</span></li>
            <li class="skill"><span class="skill-no">2</span><img class="skill-icon" src="../res/icons/rotate-cw.png" alt="skill"><span class="skill-name">旋风斩</span><span class="skill-lv">Lv.7</span></li>
            <li class="skill"><span class="skill-no">3</span><img class="skill-icon" src="../res/icons/eye.png" alt="skill"><span class="skill-name">鹰眼</span><span class="skill-lv">Lv.5</span></li>
            <li class="skill"><span class="skill-no">4</span><img class="skill-icon" src="../res/icons/zap.png" alt="skill"><span class="skill-name">瞬步</span><span class="skill-lv">Lv.8</span></li>
          </ol>
        </div>
      </div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。canvas + ol/li + progress + img 全通过。

- [ ] **Step 3: 浏览器可视**

双击 home → 角色 → 确认 canvas 占位文字（preview 渲染"NativeHost 3D 角色 slot"）、HP/MP/EXP 充能动画、装备槽 hover 高亮、ol 技能序号。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/character.html
git commit -m "feat(showcase): add character page with canvas 3D slot + stat bars + skill list"
```

---

## Task 10: form.html — 角色创建表单

**Files:**
- Create: `showcase/showcase/form.html`

- [ ] **Step 1: 写 form.html（label/input 全型 + select + textarea，div+p 分组）**

Create `showcase/showcase/form.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 角色创建</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; }
  .body { flex:1; padding:40px 64px; gap:28px; overflow-y:auto; }
  .page-title { color:#e0e6ec; font-size:32px; font-weight:700; }
  .page-desc { color:#9aa0b4; font-size:15px; }
  .group { background-color:#152433; border-radius:12px; padding:28px; gap:18px; }
  .group-title { color:#8ec5d8; font-size:18px; font-weight:700; }
  .field { flex-direction:column; gap:8px; }
  .field-label { color:#9aa0b4; font-size:14px; }
  input[type="text"], select, textarea { background-color:#1a2f45; border-width:0 0 0 2px; border-color:#2a5a75; color:#e0e6ec; font-size:15px; padding:10px 14px; border-radius:6px; transition:border-color .15s; }
  input[type="text"]:focus, select:focus, textarea:focus { border-color:#5fb4d4; }
  textarea { width:100%; height:100px; resize:none; font-family:inherit; }
  .radio-group { flex-direction:row; gap:24px; }
  .radio-opt { flex-direction:row; align-items:center; gap:8px; color:#9aa0b4; font-size:15px; }
  input[type="radio"], input[type="checkbox"] { accent-color:#5fb4d4; }
  .slider-field { flex-direction:row; align-items:center; gap:16px; }
  .slider-val { color:#d4a44e; font-size:16px; font-weight:700; min-width:40px; }
  input[type="range"] { flex:1; accent-color:#5fb4d4; }
  .actions { flex-direction:row; gap:16px; justify-content:flex-end; }
  .btn { padding:12px 32px; border-radius:8px; font-size:16px; font-weight:700; transition:filter .15s; }
  .btn-create { background-color:#d4a44e; color:#0e1620; }
  .btn-reset { background-color:#1a2f45; color:#9aa0b4; }
  .btn:hover { filter:brightness(1.12); }
  </style>
</head>
<body>
  <div class="root">
    <header class="topbar" style="flex-direction:row;align-items:center;gap:12px;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
      <button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button><span style="color:#e0e6ec;font-size:20px;font-weight:700">角色创建</span>
    </header>
    <div class="body">
      <p class="page-title">创建新角色</p>
      <p class="page-desc">表单控件编排 · 用 div + p 分组（替代 fieldset/legend）</p>
      <div class="group">
        <p class="group-title">基本信息</p>
        <div class="field"><label class="field-label" for="char-name">角色名</label><input type="text" id="char-name" placeholder="输入角色名" maxlength="12"></div>
      </div>
      <div class="group">
        <p class="group-title">职业与出身</p>
        <div class="field"><label class="field-label" for="char-class">职业</label>
          <select id="char-class">
            <option value="ranger">游侠</option>
            <option value="mage">法师</option>
            <option value="warrior">战士</option>
            <option value="assassin">刺客</option>
          </select>
        </div>
      </div>
      <div class="group">
        <p class="group-title">性别与阵营</p>
        <div class="radio-group">
          <div class="radio-opt"><input type="radio" id="g-m" name="gender" checked><label for="g-m">男</label></div>
          <div class="radio-opt"><input type="radio" id="g-f" name="gender"><label for="g-f">女</label></div>
          <div class="radio-opt"><input type="radio" id="a-light" name="faction" checked><label for="a-light">光明阵营</label></div>
          <div class="radio-opt"><input type="radio" id="a-dark" name="faction"><label for="a-dark">暗影阵营</label></div>
        </div>
      </div>
      <div class="group">
        <p class="group-title">初始属性分配</p>
        <div class="field"><label class="field-label" for="attr-str">力量</label>
          <div class="slider-field"><input type="range" id="attr-str" min="0" max="20" value="10"><span class="slider-val">10</span></div>
        </div>
        <div class="field"><label class="field-label" for="attr-agi">敏捷</label>
          <div class="slider-field"><input type="range" id="attr-agi" min="0" max="20" value="12"><span class="slider-val">12</span></div>
        </div>
        <div class="field"><label class="field-label" for="attr-int">智力</label>
          <div class="slider-field"><input type="range" id="attr-int" min="0" max="20" value="8"><span class="slider-val">8</span></div>
        </div>
      </div>
      <div class="group">
        <p class="group-title">背景故事</p>
        <div class="field"><label class="field-label" for="char-bio">简介（textarea）</label><textarea id="char-bio" rows="4" cols="60" maxlength="200" placeholder="一段角色背景故事..."></textarea></div>
      </div>
      <div class="actions">
        <button class="btn btn-reset">重置</button>
        <button class="btn btn-create">创建角色</button>
      </div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。label[for] 全部指向存在 ID；select 只含 option；textarea Text 内容模型通过。

- [ ] **Step 3: 浏览器可视**

双击 home → 角色创建 → 确认 5 个分组（div+p 标题）、text/select/radio/range/textarea 全渲染、聚焦边框高亮、滑块联动数值。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/form.html
git commit -m "feat(showcase): add form page with grouped form controls"
```

---

## Task 11: lab.html — CSS 全属性标本馆

**Files:**
- Create: `showcase/showcase/lab.html`

- [ ] **Step 1: 写 lab.html（9 分区 specimen + custom-element + slot）**

这是最大的页。每分区一个 specimen 卡片，标题用 `<p>` + 大字号。分区：flex 全参数 / 尺寸单位 / 盒模型 / 边框 / 背景 / 文本排版 / 文本特效 / 视觉变换 / 溢出 + custom-element slot 投影。

Create `showcase/showcase/lab.html`:

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>LoomGUI 靶子冻结 Showcase · 标本馆</title>
  <style>
  .root { width:1920px; height:1080px; background-color:#0e1620; }
  .body { flex:1; padding:40px 64px; gap:32px; overflow-y:auto; }
  .page-title { color:#e0e6ec; font-size:32px; font-weight:700; }
  .page-desc { color:#9aa0b4; font-size:15px; }
  .section { background-color:#152433; border-radius:12px; padding:28px; gap:16px; }
  .section-title { color:#8ec5d8; font-size:20px; font-weight:700; }
  .specimen { background-color:#0e1620; border-radius:8px; padding:16px; gap:12px; }
  /* flex specimen */
  .fx-demo { flex-direction:row; gap:8px; background-color:#1a2f45; padding:10px; border-radius:6px; }
  .fx-item { background-color:#2a5a75; color:#e0e6ec; padding:10px 16px; border-radius:4px; font-size:13px; }
  /* 盒模型 specimen */
  .box-pad { background-color:#2a5a75; padding:20px; }
  .box-pad-inner { background-color:#d4a44e; padding:12px; color:#0e1620; font-size:13px; }
  .box-margin { background-color:#1a2f45; padding:4px; }
  .box-margin-inner { background-color:#5fb4d4; margin:16px; color:#0e1620; padding:8px; font-size:13px; }
  /* 边框 specimen */
  .brd { width:80px; height:80px; background-color:#1a2f45; }
  .brd-1 { border-radius:8px; border-color:#5fb4d4; }
  .brd-2 { border-radius:16px 4px; border-color:#d4a44e; }
  .brd-3 { border-radius:50%; border-color:#c2605a; }
  /* 背景 specimen */
  .bg-cover { width:120px; height:80px; background-image:url("../res/icons/palette.png"); background-size:cover; border-radius:6px; }
  .bg-contain { width:120px; height:80px; background-image:url("../res/icons/palette.png"); background-size:contain; background-color:#1a2f45; border-radius:6px; }
  .bg-grad { width:120px; height:80px; background-image:linear-gradient(to right,#5fb4d4,#d4a44e); border-radius:6px; }
  /* 文本 specimen */
  .txt p { color:#dfe4ec; }
  .txt-serif { font-family:"DejaVuSans",serif; font-size:18px; }
  .txt-mono { font-family:"JetBrainsMono",monospace; font-size:16px; }
  .txt-pixel { font-family:"PressStart2P",monospace; font-size:12px; }
  /* 文本特效 specimen */
  .fx-glow { color:#e0e6ec; font-size:28px; font-effect:glow(4 #5fb4d4); }
  .fx-stroke { color:#152433; font-size:28px; -webkit-text-stroke:1px #d4a44e; }
  .fx-shadow { color:#e0e6ec; font-size:24px; text-shadow:2px 2px 4px #000000; }
  .fx-grad { font-size:28px; font-weight:700; background-image:linear-gradient(to right,#5fb4d4,#d4a44e); -webkit-background-clip:text; background-clip:text; color:#00000000; }
  @keyframes shimmer { to { background-position:200% center; } }
  .fx-grad { background-size:200% auto; animation:shimmer 3s linear infinite; }
  /* 视觉变换 specimen */
  .vf { width:60px; height:60px; background-color:#5fb4d4; border-radius:6px; }
  .vf-rot { transform:rotate(15deg); }
  .vf-scale { transform:scale(1.2); }
  .vf-trans { transform:translate(10px,10px); }
  .vf-gray { filter:grayscale(1); background-color:#d4a44e; }
  .vf-bright { filter:brightness(1.4); background-color:#d4a44e; }
  .vf-hue { filter:hue-rotate(120deg); background-color:#c2605a; }
  /* 溢出 specimen */
  .ov-box { width:280px; height:80px; background-color:#1a2f45; border-radius:6px; padding:8px; }
  .ov-scroll { overflow-y:scroll; }
  .ov-auto { overflow-y:auto; }
  .ov-hidden { overflow:hidden; }
  </style>
</head>
<body>
  <div class="root">
    <header class="topbar" style="flex-direction:row;align-items:center;gap:12px;padding:20px 32px;background-color:#152433;border-width:0 0 1px 0;border-color:#2a5a75">
      <button id="back-home" style="color:#8ec5d8;font-size:15px">← 首页</button><span style="color:#e0e6ec;font-size:20px;font-weight:700">CSS 标本馆</span>
    </header>
    <div class="body">
      <p class="page-title">CSS 全属性标本馆</p>
      <p class="page-desc">围栏 §5.1 全白名单逐组 specimen · 小枚举全值，大枚举代表值 + 矩阵</p>

      <div class="section">
        <p class="section-title">1 · flex 全参数</p>
        <div class="specimen" style="flex-direction:row;gap:16px;flex-wrap:wrap">
          <div style="gap:6px"><span style="color:#9aa0b4;font-size:12px">row + gap</span><div class="fx-demo"><span class="fx-item">A</span><span class="fx-item">B</span><span class="fx-item">C</span></div></div>
          <div style="gap:6px"><span style="color:#9aa0b4;font-size:12px">justify flex-end</span><div class="fx-demo" style="justify-content:flex-end"><span class="fx-item">A</span><span class="fx-item">B</span></div></div>
          <div style="gap:6px"><span style="color:#9aa0b4;font-size:12px">justify space-between</span><div class="fx-demo" style="justify-content:space-between"><span class="fx-item">A</span><span class="fx-item">B</span><span class="fx-item">C</span></div></div>
          <div style="gap:6px"><span style="color:#9aa0b4;font-size:12px">align center</span><div class="fx-demo" style="align-items:center"><span class="fx-item" style="height:40px">A</span><span class="fx-item">B</span></div></div>
          <div style="gap:6px"><span style="color:#9aa0b4;font-size:12px">flex-grow</span><div class="fx-demo"><span class="fx-item" style="flex-grow:1">grow</span><span class="fx-item">B</span></div></div>
        </div>
      </div>

      <div class="section">
        <p class="section-title">2 · 尺寸单位</p>
        <div class="specimen" style="flex-direction:row;gap:16px">
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">width:200px</span><div style="width:200px;height:50px;background-color:#2a5a75;border-radius:4px"></div></div>
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">width:50%</span><div style="width:50%;height:50px;background-color:#2a5a75;border-radius:4px"></div></div>
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">aspect-ratio 16/9</span><div style="width:160px;aspect-ratio:16/9;background-color:#d4a44e;border-radius:4px"></div></div>
        </div>
      </div>

      <div class="section">
        <p class="section-title">3 · 盒模型（padding / margin）</p>
        <div class="specimen" style="flex-direction:row;gap:24px">
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">padding 20px</span><div class="box-pad"><div class="box-pad-inner">content</div></div></div>
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">margin 16px</span><div class="box-margin"><div class="box-margin-inner">content</div></div></div>
        </div>
      </div>

      <div class="section">
        <p class="section-title">4 · 边框（color / radius / border-image-slice）</p>
        <div class="specimen" style="flex-direction:row;gap:16px;align-items:center">
          <div class="brd brd-1"></div>
          <div class="brd brd-2"></div>
          <div class="brd brd-3"></div>
          <span style="color:#9aa0b4;font-size:12px">border-image-slice 九宫格见运行时资源</span>
        </div>
      </div>

      <div class="section">
        <p class="section-title">5 · 背景（color / image / size / clip）</p>
        <div class="specimen" style="flex-direction:row;gap:16px;align-items:center">
          <div class="bg-cover"></div>
          <div class="bg-contain"></div>
          <div class="bg-grad"></div>
          <span class="fx-grad">渐变字</span>
        </div>
      </div>

      <div class="section">
        <p class="section-title">6 · 文本排版（font / line-height / letter-spacing / white-space）</p>
        <div class="specimen txt" style="gap:10px">
          <p class="txt-serif">Serif 字体 · 中英文混排排版 The quick brown fox.</p>
          <p class="txt-mono">Monospace 等宽 · code style</p>
          <p class="txt-pixel">PIXEL 像素字体</p>
          <p style="color:#dfe4ec;font-size:15px;letter-spacing:2px">字间距 2px · letter-spacing</p>
          <p style="color:#dfe4ec;font-size:15px;white-space:nowrap;overflow:hidden">不换行长文本 white-space:nowrap · 截断演示截断演示截断演示截断演示</p>
        </div>
      </div>

      <div class="section">
        <p class="section-title">7 · 文本特效（text-shadow / stroke / font-effect / 渐变字）</p>
        <div class="specimen" style="flex-direction:row;gap:24px;flex-wrap:wrap;align-items:center">
          <span class="fx-glow">发光</span>
          <span class="fx-stroke">描边</span>
          <span class="fx-shadow">投影</span>
          <span class="fx-grad">流光渐变</span>
        </div>
      </div>

      <div class="section">
        <p class="section-title">8 · 视觉变换（transform / opacity / filter / box-shadow）</p>
        <div class="specimen" style="flex-direction:row;gap:20px;align-items:center;flex-wrap:wrap">
          <div class="vf vf-rot"></div>
          <div class="vf vf-scale"></div>
          <div class="vf vf-trans"></div>
          <div class="vf vf-gray"></div>
          <div class="vf vf-bright"></div>
          <div class="vf vf-hue"></div>
          <div class="vf" style="opacity:0.4"></div>
          <div class="vf" style="box-shadow:0 6px 16px #000a"></div>
        </div>
      </div>

      <div class="section">
        <p class="section-title">9 · 溢出（overflow-x/y scroll/auto/hidden）</p>
        <div class="specimen" style="flex-direction:row;gap:16px">
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">scroll</span><div class="ov-box ov-scroll"><p style="color:#9aa0b4;font-size:12px">行1<br>行2<br>行3<br>行4<br>行5<br>行6<br>行7<br>行8</p></div></div>
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">auto</span><div class="ov-box ov-auto"><p style="color:#9aa0b4;font-size:12px">行1<br>行2<br>行3<br>行4<br>行5<br>行6<br>行7<br>行8</p></div></div>
          <div style="gap:4px"><span style="color:#9aa0b4;font-size:12px">hidden</span><div class="ov-box ov-hidden"><p style="color:#9aa0b4;font-size:12px">行1<br>行2<br>行3<br>行4<br>行5<br>行6<br>行7<br>行8</p></div></div>
        </div>
      </div>

      <div class="section">
        <p class="section-title">10 · custom-element + slot 投影</p>
        <div class="specimen" style="flex-direction:row;gap:16px">
          <item-card style="flex-direction:column;gap:8px;width:160px;background-color:#1a2f45;border-radius:10px;padding:16px">
            <img src="../res/icons/box.png" alt="item" style="width:64px;height:64px;align-self:center">
            <slot name="ic-name"><span style="color:#e0e6ec;font-size:16px;font-weight:700;align-self:center">默认名</span></slot>
            <slot name="ic-desc"><span style="color:#9aa0b4;font-size:13px;align-self:center">默认描述</span></slot>
          </item-card>
          <span style="color:#6c7080;font-size:13px;align-self:center">含 hyphen 标签名 = CustomElement；&lt;slot&gt; 接投影（运行时由组件系统填充）</span>
        </div>
      </div>

    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 验证围栏打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic。custom-element（item-card）+ slot 通过；所有 CSS 属性在白名单内。若某 CSS 值报 `FenceBadCssValue`，对照 [fence.md §5.2](/F:/WorkSpace/projects/LoomGUI/docs/design/fence.md) 值校验器修正。

- [ ] **Step 3: 浏览器可视**

双击 home → 标本馆 → 逐分区确认：flex 矩阵、尺寸单位、盒模型 padding/margin、边框 radius、背景 cover/contain/渐变、文本排版字体族、文本特效（发光/描边/投影/流光）、视觉变换（旋转/缩放/滤镜）、溢出 scroll/auto/hidden、custom-element + slot。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/lab.html
git commit -m "feat(showcase): add CSS specimen lab page covering full fence whitelist"
```

---

## Task 12: 覆盖矩阵扫描脚本

**Files:**
- Create: `showcase/scripts/coverage-check.py`

- [ ] **Step 1: 写覆盖矩阵扫描脚本**

Create `showcase/scripts/coverage-check.py`:

```python
#!/usr/bin/env python3
"""Showcase 围栏覆盖矩阵扫描。
扫描 showcase/showcase/*.html 的 <body>，校验：
  1. 23 标签 + custom-element 全部至少出现一次
  2. 无围栏外标签（h1-h6/meter/dialog/details/summary/form/fieldset/legend/main/section/footer/article/aside/small）
  3. input 7 型全部覆盖
退出码 0 = 全绿；非 0 = 有缺口。"""
import re, sys, glob, os

SHOWCASE_DIR = os.path.join(os.path.dirname(__file__), "..", "showcase")
RUNTIME_TAGS = ["div","header","nav","p","span","strong","em","br","label","button",
                "a","img","canvas","input","textarea","select","option","progress",
                "ul","ol","li","template","slot"]
INPUT_TYPES = ["text","password","search","number","range","checkbox","radio"]
FORBIDDEN = ["h1","h2","h3","h4","h5","h6","meter","dialog","details","summary",
             "form","fieldset","legend","main","section","footer","article","aside","small"]
CSS_GROUPS = {
  "尺寸": ["width","height","min-width","min-height","max-width","max-height","aspect-ratio"],
  "布局": ["display","flex-direction","flex-wrap","flex-grow","justify-content","align-items","gap","order"],
  "定位": ["position","top","right","bottom","left"],
  "盒模型": ["padding","margin"],
  "边框": ["border-color","border-radius","border-image-slice"],
  "背景": ["background-color","background-image","background-size","background-clip"],
  "视觉": ["opacity","box-shadow","pointer-events","transform","filter"],
  "文本": ["color","font-size","font-family","font-weight","text-align","line-height","letter-spacing","white-space","text-shadow","-webkit-text-stroke","font-effect"],
  "溢出": ["overflow-x","overflow-y"],
}

def body_html():
    all_html = ""
    for f in glob.glob(os.path.join(SHOWCASE_DIR, "*.html")):
        t = open(f, encoding="utf-8").read()
        m = re.search(r"<body[^>]*>(.*)</body>", t, re.S | re.I)
        all_html += m.group(1) if m else t
    return all_html

def main():
    body = body_html()
    tag_re = re.compile(r"<(\w[\w-]*)")
    found_tags = set(tag_re.findall(body))
    found_lower = {t.lower() for t in found_tags}

    # custom-element = any tag with hyphen
    custom = {t for t in found_lower if "-" in t}

    errors = []

    # 1. runtime tags coverage
    for tag in RUNTIME_TAGS:
        if tag not in found_lower:
            errors.append(f"MISSING tag: <{tag}>")

    # 2. custom-element presence
    if not custom:
        errors.append("MISSING custom-element (no hyphenated tag found)")

    # 3. forbidden tags
    for tag in FORBIDDEN:
        if tag in found_lower:
            errors.append(f"FORBIDDEN tag found: <{tag}>")

    # 4. input types
    for it in INPUT_TYPES:
        if f'type="{it}"' not in body and f"type='{it}'" not in body:
            errors.append(f"MISSING input type: {it}")

    # 5. CSS groups (scan style attrs + <style> blocks across pages)
    full = ""
    for f in glob.glob(os.path.join(SHOWCASE_DIR, "*.html")):
        full += open(f, encoding="utf-8").read()
    for group, props in CSS_GROUPS.items():
        hit = any(re.search(rf"\b{re.escape(p)}\b", full) for p in props)
        if not hit:
            errors.append(f"MISSING CSS group: {group} ({', '.join(props)})")

    if errors:
        print("COVERAGE GAPS:")
        for e in errors:
            print("  -", e)
        sys.exit(1)
    print("COVERAGE OK: all 23 tags + custom-element + 7 input types + 9 CSS groups covered, no forbidden tags.")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 运行覆盖扫描**

Run: `python showcase/scripts/coverage-check.py`
Expected: `COVERAGE OK` 退出码 0。若有 MISSING，回到对应 Task 补内容。

- [ ] **Step 3: Commit**

```bash
git add showcase/scripts/coverage-check.py
git commit -m "feat(showcase): add coverage matrix scanner"
```

---

## Task 13: 图标扩充（压测图集）

**Files:**
- Create: `showcase/res/icons/*`（新增游戏物品图标）

- [ ] **Step 1: 生成/获取新增游戏物品图标**

目标：图标数量足够触发打包器多页 atlas 打包，而不只是几张零散 PNG。用 imagegen 技能生成或获取以下物品图标（64×64 PNG，透明背景）：
- 武器类：sword.png、bow.png、staff.png、dagger.png
- 防具类：helmet.png、armor.png、shield.png、boots.png
- 消耗品：potion-hp.png、potion-mp.png、scroll.png、food.png
- 材料类：ore.png、herb.png、gem.png、crystal.png
- 货币类：coin.png、gem-currency.png、token.png

放进 `showcase/res/icons/`。

- [ ] **Step 2: 在 inventory/character/shop 页引用新图标**

把 inventory 的物品格、character 的装备槽/技能图标、shop 的商品图换成新图标，确保 atlas 真正打包这些资源。

- [ ] **Step 3: 验证打包器图集自绘**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 产出 `atlas/*.png` + `atlas/*.atlas.json`，图标尺寸由 atlas.json 注入。检查 output_dir 下 atlas 产物包含新增图标。

- [ ] **Step 4: 重新跑覆盖扫描 + 浏览器可视**

Run: `python showcase/scripts/coverage-check.py`
Expected: COVERAGE OK。
浏览器确认新图标渲染正确。

- [ ] **Step 5: Commit**

```bash
git add showcase/res/icons
git commit -m "feat(showcase): expand item icons for atlas stress test"
```

---

## Task 14: 删除旧 showcase_project

**Files:**
- Delete: `showcase_project/`（整体）

- [ ] **Step 1: 确认旧目录内容已全部迁出**

确认 `showcase/res/fonts/` 和 `showcase/res/icons/` 已包含旧目录的全部字体和图标；design-systems 已重写（不依赖旧 tokens）。

- [ ] **Step 2: 删除旧目录**

```bash
git rm -r showcase_project
```

- [ ] **Step 3: 验证新 showcase 仍完整可用**

Run: `cargo run -p loomgui_pkg -- build showcase`
Run: `python showcase/scripts/coverage-check.py`
Expected: 打包零 diagnostic + COVERAGE OK。

- [ ] **Step 4: Commit**

```bash
git commit -m "chore: remove legacy showcase_project after migration"
```

---

## Task 15: 全量验收

- [ ] **Step 1: 围栏契约门**

Run: `cargo test -p loomgui_fence`
Expected: 全绿。

- [ ] **Step 2: 打包门**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: 零 diagnostic，产出 pkg.bin + atlas 产物。

- [ ] **Step 3: 覆盖矩阵**

Run: `python showcase/scripts/coverage-check.py`
Expected: COVERAGE OK。

- [ ] **Step 4: 浏览器逐页验收**

双击 `showcase/showcase/home.html`，逐页点击 nav 卡片，对照 spec §6 验收每页：
- home：hero + 3 按钮 + 7 卡片 + 序列入场动画
- settings：左 tablist 切右 panel，7 input 变体全渲染
- inventory：ListView 10 格 + 详情面板 + strong/em 富文本
- mail：邮件列表 + 未读呼吸 + 富文本（strong/em/br/a 多色 span）
- shop：3 商品卡 + progress 倒计时/库存 + 购买弹窗
- character：canvas 占位 + HP/MP/EXP 充能 + ol 技能 + 装备槽
- form：5 分组表单 + text/select/radio/range/textarea
- lab：9 分区 specimen + custom-element slot

- [ ] **Step 5: 最终 Commit**

```bash
git add -A
git commit -m "feat(showcase): freeze target showcase for R2-R7 runtime rewrite"
```

> 靶子冻结完成。后续 R2–R7 运行时重写以"渲染结果是否对齐此靶子"为验收标准，不再回头改靶子。
