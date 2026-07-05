# Showcase 浏览器预览 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 showcase HTML 双击在浏览器打开即还原 LoomGUI 渲染效果，用来对照"Unity 实际"和"AI 想要"的视觉偏差。

**Architecture:** 9 个 showcase 页 head 各加 3 行（`<base href="..">` + polyfill css + driver js），打包器只遍历 body（dom.rs:35）看不到 → pkg.bin 零变化。driver 是单文件经典脚本（ES module 在 file:// 被 CORS 拦），移植 LoomShowcaseDriver 的"够看视觉"子集：导航 + 动态页内容 + overlay。

**Tech Stack:** 原生 HTML/CSS/JS（无构建、无依赖）、Rust（仅 Task 1 加一个 characterization 测试）。

## Global Constraints

- **packager 只遍历 `<body>`**（`loomgui_core/src/parse/dom.rs:35-50`）——head 整个不碰。showcase 页 head 加 `<base>/<link>/<script>` 不进 pkg.bin。Task 1 的测试锁定此假设。
- **纯双击、零服务器**：driver 是经典 `<script src>`（非 `type="module"`，后者 file:// 被 CORS 拦）；overlay（mail/tips）用 driver 内嵌模板（`fetch()` 在 file:// 被拦）。
- **图片路径用 `<base href="..">` 修**：showcase HTML 在 `LoomUI/showcase/`，图片引用 `res/icons/...`（工作区根相对），`<base>` 锚到 `LoomUI/` 后浏览器能找到。
- **视觉导向，非行为镜像**：虚拟列表不做 slot 复用（全量塞 item）、tween 不移植 ease 数学（CSS transition）、不卡文本/动画像素。
- **围栏是 Chrome 原生子集**：polyfill 让 div=flex-column 对齐 LoomGUI 契约，其余靠浏览器原生。
- **commit 中文**（项目惯例）；每个 Task 末尾 commit。
- **路径基准**：`loomgui_unity/Assets/LoomUI/` 下有 `showcase/`（包源）、`res/icons/`（图）、新增 `preview/`（预览专属）。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `loomgui_core/src/parse/dom.rs` | 加 characterization 测试锁定 head-blind | Modify（mod tests 内加一个测试） |
| `loomgui_unity/Assets/LoomUI/preview/preview-base.css` | polyfill（div flex-column/border-box/body reset）+ 预览壳（居中/设备框） | Create |
| `loomgui_unity/Assets/LoomUI/preview/loom-preview.js` | driver：CONFIG(NAV/TEMPLATES) + boot(fit-scale/路由) + shared(导航/overlay/lamp) + 9 个 page handler | Create（Task 3 建骨架，Task 5-10 填 handler） |
| `loomgui_unity/Assets/LoomUI/preview/README.md` | 怎么开 + 保真度说明 | Create |
| 9 个 showcase HTML（`home.html` + 8 个 `page_*.html`） | head 加 3 行 | Modify（同一个 edit） |

---

### Task 1: 锁定 packager head-blind 假设（characterization 测试）

整份设计的基石：showcase 页 head 加 `<base>/<link>/<script>` 后 pkg.bin 不变。先写测试锁定 `parse_html` 忽略 head 的现行行为——若哪天 parse 改成遍历 head，测试会挡住。

**Files:**
- Modify: `loomgui_core/src/parse/dom.rs`（mod tests 内，最后一个 `#[test]` 后、`}` 闭合前加）

**Interfaces:**
- Produces: 无外部接口；仅锁定 `parse_html` 行为。

- [ ] **Step 1: 加 characterization 测试**

在 `loomgui_core/src/parse/dom.rs` 的 `mod tests` 内（`fence_tags_all_accepted` 测试之后、mod 闭合 `}` 之前）插入：

```rust
    #[test]
    fn ignores_head_when_parsing_body() {
        // 浏览器预览往 showcase 页 head 塞 <base>/<link>/<script>/<style>（preview-only）。
        // parse_html 只遍历 body（dom.rs:35 body_sel），head 整个不碰 → pkg.bin 不受影响。
        // 本测试锁定该假设：head 里的围栏外 tag（base/link/script）不报错，body 树正确解析。
        let html = r#"<!DOCTYPE html><html><head>
            <meta charset="utf-8">
            <base href="..">
            <link rel="stylesheet" href="preview/preview-base.css">
            <script src="preview/loom-preview.js"></script>
            <style>.x{color:red}</style>
            </head><body><div class="root">hi</div></body></html>"#;
        let tree = parse_html(html).unwrap();
        assert_eq!(tree.roots.len(), 1, "head 内容不应产生额外 root");
        let root = &tree.nodes[tree.roots[0].0];
        assert_eq!(root.tag, "div");
        assert_eq!(root.classes, vec!["root"]);
        assert_eq!(root.text.as_deref(), Some("hi"));
    }
```

- [ ] **Step 2: 跑测试确认通过（characterization——锁定现行行为）**

Run: `cargo test -p loomgui_core ignores_head_when_parsing_body`
Expected: PASS（一行 `test parse::dom::tests::ignores_head_when_parsing_body ... ok`）。

> 若 FAIL：说明 parse_html 实际会读 head——整份设计要重评（停，找用户）。预期会 PASS（dom.rs:35 只 `body_sel`）。

- [ ] **Step 3: Commit**

```bash
git add loomgui_core/src/parse/dom.rs
git commit -m "test(parse): 锁定 head-blind——浏览器预览 head 注入不影响 pkg.bin"
```

---

### Task 2: preview-base.css（polyfill + 预览壳）

**Files:**
- Create: `loomgui_unity/Assets/LoomUI/preview/preview-base.css`

**Interfaces:**
- Produces: 全局 CSS——`div` 强制 flex-column（LoomGUI 契约）、`*` border-box、body 居中深色底、`.root` 设备框阴影。

- [ ] **Step 1: 写 preview-base.css**

```css
/* LoomGUI 浏览器预览基础样式（preview-only——打包器只遍历 body，head 不进 pkg.bin）。
   polyfill 对齐 LoomGUI 契约（main-design.md §3.1）：
     - div 永远 flex column（Chromium 默认 div=block，会让 .grid/.header 的 flex-direction 失效）
     - taffy border-box（Chromium 默认 content-box）
     - body 无 8px 默认 margin
   预览壳：深色底 + 居中 + .root 设备框。fit-scale（镜像 engine letterbox）由 loom-preview.js 的 body.zoom 做。 */
div { display: flex; flex-direction: column; }
* { box-sizing: border-box; }
body {
  margin: 0;
  background: #0e1018;
  display: flex;
  justify-content: center;
  align-items: flex-start;
  min-height: 100vh;
  padding: 24px;
}
.root { box-shadow: 0 0 0 1px #2a2f45, 0 8px 40px rgba(0, 0, 0, .6); }
```

- [ ] **Step 2: 人工核对（CSS 无自动化测试）**

打开文件确认语法无误（无未闭合括号）。无需运行。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/preview-base.css
git commit -m "feat(preview): preview-base.css——polyfill 对齐 LoomGUI 布局 + 预览壳"
```

---

### Task 3: loom-preview.js 骨架 + README

建 driver 主结构：CONFIG（NAV 导航表 + TEMPLATES mail/tips 内嵌模板）、boot（fit-scale + 按文件名路由）、shared（`$`/bind/wireBackHome/wireNav/ensureOverlay/showTips/showMail/hideMail/pulseLamp）、pages 派发器（9 个 handler 先 stub 只调 wireBackHome）。后续 Task 5-10 填具体 handler。

**Files:**
- Create: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`
- Create: `loomgui_unity/Assets/LoomUI/preview/README.md`

**Interfaces:**
- Produces: 全局 IIFE。后续 task 通过替换 `pages` 里各 stub 填 handler。stub 形如 `page_list: function () { wireBackHome(); /*Task7*/ },`（每页 stub 注释唯一，便于 Edit 定位）。
- shared helper 签名（后续 task 直接用）：
  - `$(id)` → `Element|null`
  - `bind(id, type, fn)` / `bindClick(id, fn)`
  - `wireBackHome()` / `wireNav()`
  - `ensureOverlay(kind)` → Element（kind='tips'|'mail'）
  - `showTips()` / `showMail()` / `hideMail()`
  - `pulseLamp(name)`

- [ ] **Step 1: 写 loom-preview.js 骨架**

```js
// LoomGUI showcase 浏览器预览 driver（preview-only——打包器只遍历 body，head 不进 pkg.bin）。
// 移植 LoomShowcaseDriver 的"够看视觉"子集：导航 + 动态页内容 + overlay。
// 非忠实行为镜像——虚拟列表不做 slot 复用、tween 不移植 ease 数学（视觉验证导向）。
// 经典脚本（非 ES module）——后者在 file:// 被 CORS 拦。
(function () {
  'use strict';

  // === CONFIG ===
  // home nav 按钮 id → 目标页文件名（抄 LoomShowcaseDriver.SubscribeHome）。
  var NAV = {
    'nav-controls': 'page_controls',
    'nav-text': 'page_text',
    'nav-image': 'page_image',
    'nav-scroll': 'page_scroll',
    'nav-tween': 'page_tween',
    'nav-interact': 'page_interact',
    'nav-dyntree': 'page_dyntree',
    'nav-list': 'page_list'
  };

  // overlay 组件模板：<style> + markup 一起（宿主页没这些类的 CSS）。
  // mail 的 .root 改名 .loom-mail-root 避撞宿主页 .root(1080×1920)。
  // 改了 mail.html/tips_toast.html 须同步这里。
  var TEMPLATES = {
    mail:
      '<style>' +
      '.loom-mail-root{width:600px;height:800px;background-color:#1a1d2e;flex-direction:column;padding:16px;gap:12px}' +
      '.loom-mail-root .header{flex-direction:row;justify-content:space-between;align-items:center;padding-bottom:8px;border-width:0 0 1px 0;border-color:#3a3f55}' +
      '.loom-mail-root .title{color:#e6e6e0;font-size:28px;font-weight:700}' +
      '.loom-mail-root .count{color:#5fb2c4;font-size:16px}' +
      '.loom-mail-root .list{flex-direction:column;gap:8px;flex:1}' +
      '.loom-mail-root .mail-item{flex-direction:row;gap:12px;padding:12px;background-color:#2a2f45;border-radius:8px;align-items:center}' +
      '.loom-mail-root .mail-icon{width:40px;height:40px;border-radius:20px;color:#ffffff;font-size:18px;font-weight:700;justify-content:center;align-items:center}' +
      '.loom-mail-root .mail-body{flex-direction:column;gap:4px;flex:1}' +
      '.loom-mail-root .mail-from{color:#e6e6e0;font-size:16px;font-weight:600}' +
      '.loom-mail-root .mail-sub{color:#8a8fa3;font-size:14px}' +
      '.loom-mail-root .footer{flex-direction:row;justify-content:center;padding-top:8px}' +
      '.loom-mail-root .btn{background-color:#5fb2c4;color:#ffffff;font-size:18px;font-weight:600;padding:10px 32px;border-radius:6px}' +
      '</style>' +
      '<div class="loom-mail-root">' +
        '<div class="header"><span class="title">邮件</span><span class="count">3 封未读</span></div>' +
        '<div class="list">' +
          '<div class="mail-item"><div class="mail-icon" style="background-color:#5fb2c4">系</div><div class="mail-body"><span class="mail-from">系统奖励</span><span class="mail-sub">每日登录奖励已发放</span></div></div>' +
          '<div class="mail-item"><div class="mail-icon" style="background-color:#c2605a">战</div><div class="mail-body"><span class="mail-from">竞技场</span><span class="mail-sub">赛季结束，你的排名 127</span></div></div>' +
          '<div class="mail-item"><div class="mail-icon" style="background-color:#6fa66c">友</div><div class="mail-body"><span class="mail-from">好友小明</span><span class="mail-sub">送了你 100 金币</span></div></div>' +
        '</div>' +
        '<div class="footer"><button class="btn">一键领取</button></div>' +
      '</div>',
    tips:
      '<style>' +
      '.loom-toast{background-color:#252839;border:1px solid #5fb2c4;border-radius:8px;padding:20px 32px;gap:10px;align-items:center;box-shadow:0 4px 12px #0008}' +
      '.loom-toast .toast-icon{font-size:28px;color:#5fb2c4}' +
      '.loom-toast .toast-text{color:#e0e0e0;font-size:20px}' +
      '.loom-toast .toast-sub{color:#9aa0b4;font-size:14px}' +
      '</style>' +
      '<div class="loom-toast">' +
        '<div class="toast-icon">✦</div>' +
        '<div style="flex-direction:column;gap:4px">' +
          '<div class="toast-text">操作成功</div>' +
          '<div class="toast-sub">tips_layer 叠加演示（定时摘除）</div>' +
        '</div>' +
      '</div>'
  };

  // === shared helpers ===
  function $(id) { return document.getElementById(id); }
  function bind(id, type, fn) { var el = $(id); if (el) el.addEventListener(type, fn); }
  function bindClick(id, fn) { bind(id, 'click', fn); }

  // 导航：所有页 #back-home → home；home 的 #nav-* → 各页。
  // <base href=".."> 锚到 LoomUI/，故 location.href 用 'showcase/xxx.html'。
  function wireBackHome() {
    bindClick('back-home', function () { location.href = 'showcase/home.html'; });
  }
  function wireNav() {
    Object.keys(NAV).forEach(function (navId) {
      bindClick(navId, function () { location.href = 'showcase/' + NAV[navId] + '.html'; });
    });
  }

  // overlay 容器（position:fixed 盖页面）。tips 底部居中、pointer-events:none；
  // mail 居中 + 半透明遮罩。
  function ensureOverlay(kind) {
    var id = 'loom-overlay-' + kind;
    var existing = document.getElementById(id);
    if (existing) return existing;
    var ov = document.createElement('div');
    ov.id = id;
    if (kind === 'tips') {
      ov.style.cssText = 'position:fixed;inset:0;flex-direction:column;align-items:center;justify-content:flex-end;padding:40px;pointer-events:none;z-index:50';
    } else {
      ov.style.cssText = 'position:fixed;inset:0;flex-direction:column;align-items:center;justify-content:center;background:rgba(0,0,0,.5);z-index:60';
    }
    document.body.appendChild(ov);
    return ov;
  }
  var tipsTimer = null;
  function showTips() {
    var ov = ensureOverlay('tips');
    ov.innerHTML = TEMPLATES.tips;
    if (tipsTimer) clearTimeout(tipsTimer);
    tipsTimer = setTimeout(function () { ov.innerHTML = ''; tipsTimer = null; }, 2000);
  }
  function showMail() { ensureOverlay('mail').innerHTML = TEMPLATES.mail; }
  function hideMail() { var ov = $('loom-overlay-mail'); if (ov) ov.innerHTML = ''; }

  // 灯阵脉冲：lamp-{name} 容器 opacity 1→0.3→1（CSS transition，近似 C# LightLamp）。
  function pulseLamp(name) {
    var c = $('lamp-' + name);
    if (!c) return;
    c.style.transition = 'opacity .2s';
    c.style.opacity = '0.3';
    setTimeout(function () { c.style.opacity = '1'; }, 200);
  }

  // fit-scale：镜像 engine letterbox（sf=min(vw/1080, vh/1920)）。
  // Chrome/Edge 支持 body.zoom；旧 Firefox 无效（降级不缩放，用户 Ctrl+-）。
  function applyFitScale() {
    var sf = Math.min(window.innerWidth / 1080, window.innerHeight / 1920);
    if (sf > 0 && sf < 1) document.body.style.zoom = sf;
  }

  // === page handlers（Task 5-10 填充，这里 stub 只调 wireBackHome） ===
  var pages = {
    home:          function () { wireBackHome(); /*Task5*/ },
    page_controls: function () { wireBackHome(); /*Task6*/ },
    page_text:     function () { wireBackHome(); },
    page_image:    function () { wireBackHome(); },
    page_scroll:   function () { wireBackHome(); },
    page_tween:    function () { wireBackHome(); /*Task9*/ },
    page_interact: function () { wireBackHome(); /*Task10*/ },
    page_dyntree:  function () { wireBackHome(); /*Task8*/ },
    page_list:     function () { wireBackHome(); /*Task7*/ }
  };

  // === boot ===
  function boot() {
    applyFitScale();
    window.addEventListener('resize', applyFitScale);
    var file = (location.pathname.split('/').pop() || 'home.html').replace('.html', '');
    if (pages[file]) pages[file]();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
```

- [ ] **Step 2: 写 README.md**

```md
# Showcase 浏览器预览

双击 `../showcase/home.html` 即可在浏览器预览 showcase（视觉对照用，非行为镜像）。

## 可信（≈ Unity 渲染）
flex 布局/方向/gap/justify/align、px/% 尺寸、color/opacity/border/radius、
background-image/size、filter、transform、overflow:scroll、九宫 border-image-slice、列表几何/滚动。

## 近似（抓布局偏差，不卡像素）
- tween 动画：CSS transition 近似，不逐曲线对齐 ease。
- 文本换行/字距像素：Chrome 文本引擎 vs LoomGUI(unicode-linebreak)，换行点会偏。
- drag/longpress/key 触发条件：浏览器事件近似。
- NativeHost（外部 Cube GO）：`#model-slot` 显示占位文本，无法复刻。
- overlay mail/tips：用 loom-preview.js 内嵌模板。

## 维护
- 改了 showcase HTML：刷新浏览器即可。
- 改了 `../showcase/mail.html` 或 `tips_toast.html`：须同步 `loom-preview.js` 里的 `TEMPLATES`。
```

- [ ] **Step 3: 人工核对**

打开 `loom-preview.js` 确认括号/IIFE 闭合、无语法错（浏览器加载时 F12 Console 应无报错——但此刻还没页引用它，Task 4 后才能跑）。

- [ ] **Step 4: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js loomgui_unity/Assets/LoomUI/preview/README.md
git commit -m "feat(preview): loom-preview.js 骨架——CONFIG/boot/shared + 9 页 stub + README"
```

---

### Task 4: 9 个 showcase 页 head 加 3 行

让每个页加载 polyfill + driver。同一个 edit 应用到 9 个文件。

**Files:**
- Modify（每个都一样）：
  - `loomgui_unity/Assets/LoomUI/showcase/home.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_controls.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_text.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_image.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_scroll.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_tween.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_interact.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_dyntree.html`
  - `loomgui_unity/Assets/LoomUI/showcase/page_list.html`

**Interfaces:** 无。

- [ ] **Step 1: 对每个文件，把 `<meta charset="utf-8">` 替换为 4 行**

对上面 9 个文件，每个用 Edit 工具：

- `old_string`: `<meta charset="utf-8">`
- `new_string`:
```
<meta charset="utf-8">
<base href="..">
<link rel="stylesheet" href="preview/preview-base.css">
<script src="preview/loom-preview.js"></script>
```

（`<base>` 必须在所有带相对 URL 的元素之前，故紧跟 meta。`<meta charset="utf-8">` 每文件唯一，Edit 安全。）

- [ ] **Step 2: 浏览器验证 home（骨架 + 导航已可用）**

双击打开 `loomgui_unity/Assets/LoomUI/showcase/home.html`（用 Chrome/Edge）。
Expected:
- 页面居中、深色底、`.root` 1080×1920 有设备框阴影、flex 布局正确（nav 卡片 2×4 横排换行——`.grid` flex-direction:row + flex-wrap:wrap 生效）。
- F12 Console 无报错。
- 点 `nav-controls` 卡片 → 跳到 page_controls.html（该页也布局正确、有 `← 返回`）。
- 点 `← 返回` → 回 home。

> text/image/scroll 三页此时仅 `← 返回` 可用（stub），但布局应正确（polyfill 生效）。list/dyntree/tween/interact 内容在 Task 5-10 填。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/showcase/home.html loomgui_unity/Assets/LoomUI/showcase/page_*.html
git commit -m "feat(showcase): 9 个页 head 注入预览三件套——<base>+polyfill+driver"
```

---

### Task 5: home handler（nav-tips-demo → showTips）

home 已在骨架里调 `wireBackHome` + `wireNav`（Task 3 的 stub 含 `/*Task5*/`）。本 task 加 `nav-tips-demo` → showTips。

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`

**Interfaces:**
- Consumes: `wireNav()`、`bindClick(id,fn)`、`showTips()`（骨架已定义）。

- [ ] **Step 1: 替换 home stub**

Edit `loom-preview.js`：
- `old_string`: `    home:          function () { wireBackHome(); /*Task5*/ },`
- `new_string`:
```js
    home:          function () {
      wireBackHome();
      wireNav();
      bindClick('nav-tips-demo', showTips);
    },
```

- [ ] **Step 2: 浏览器验证**

双击 `home.html`。
Expected: 点底部"弹出 tips 演示"卡片 → 屏幕底部出现 toast（✦ 操作成功 / tips_layer 叠加演示）→ 2 秒后自动消失。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git commit -m "feat(preview): home handler——nav-tips-demo 弹 toast"
```

---

### Task 6: controls handler（model-slot 占位）

`#model-slot` 是 NativeHost 槽位（Unity 挂外部 Cube GO），浏览器无法复刻，注入占位文本。

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`

**Interfaces:**
- Consumes: `wireBackHome()`、`$`。

- [ ] **Step 1: 替换 controls stub**

Edit `loom-preview.js`：
- `old_string`: `    page_controls: function () { wireBackHome(); /*Task6*/ },`
- `new_string`:
```js
    page_controls: function () {
      wireBackHome();
      var slot = $('model-slot');
      if (slot) {
        slot.style.justifyContent = 'center';
        slot.style.alignItems = 'center';
        slot.innerHTML = '<div style="color:#9aa0b4;font-size:12px;text-align:center;padding:8px">[NativeHost<br>外部 GO<br>预览不支持]</div>';
      }
    },
```

- [ ] **Step 2: 浏览器验证**

双击 `page_controls.html`（或从 home 进"控件"）。
Expected: `1.6 NativeHost` 卡片的 120×120 虚线框内居中显示 `[NativeHost 外部 GO 预览不支持]`。其余卡片（色块/img/span/button/flex 练兵场）布局正确。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git commit -m "feat(preview): controls handler——model-slot NativeHost 占位"
```

---

### Task 7: list handler（全量塞 item + 原生滚动）

不做虚拟列表逻辑——直接往两个 scroll 容器塞全量 item（equal 1000 + variable 200），普通 flex 流 + 浏览器原生 `overflow-y:scroll`。item 视觉对齐 C# `VirtualListDriver.CreateItem`。

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`

**Interfaces:**
- Consumes: `wireBackHome()`、`$`。

- [ ] **Step 1: 替换 list stub**

Edit `loom-preview.js`：
- `old_string`: `    page_list:     function () { wireBackHome(); /*Task7*/ },`
- `new_string`:
```js
    page_list:     function () {
      wireBackHome();
      // item 视觉对齐 C# VirtualListDriver.CreateItem（灰底 + icon + 标题）。
      // icon 用 res/icons/skin.png（<base href=".."> 解析到 LoomUI/res/）。
      function renderItem(height, title) {
        var row = document.createElement('div');
        row.style.cssText = 'width:100%;height:' + height + 'px;flex-direction:row;align-items:center;gap:12px;padding:0 16px;background-color:#252839';
        var icon = document.createElement('img');
        icon.src = 'res/icons/skin.png';
        icon.style.cssText = 'width:48px;height:48px';
        row.appendChild(icon);
        var t = document.createElement('span');
        t.style.cssText = 'color:#e0e0e0;font-size:20px';
        t.textContent = title;
        row.appendChild(t);
        return row;
      }
      // equal：1000 个等高 80px。
      var eq = $('list-equal');
      if (eq) {
        var f1 = document.createDocumentFragment();
        for (var i = 0; i < 1000; i++) f1.appendChild(renderItem(80, 'Item ' + i));
        eq.appendChild(f1);
      }
      // variable：200 个 sin 高（60~140px，抄 C# sizes[i]=100+40*sin(i*0.3)）。
      var vr = $('list-variable');
      if (vr) {
        var f2 = document.createDocumentFragment();
        for (var j = 0; j < 200; j++) {
          var h = 100 + 40 * Math.sin(j * 0.3);
          f2.appendChild(renderItem(h, 'Item ' + j + '  (' + Math.round(h) + 'px)'));
        }
        vr.appendChild(f2);
      }
    },
```

- [ ] **Step 2: 浏览器验证**

双击 `page_list.html`（或从 home 进"虚拟列表"）。
Expected:
- 左列表（等高）：每行灰底 + 图标 + "Item N"，行高一致，能上下滚动，滚动顺畅。
- 右列表（不等高）：行高 60~140px 变化，标题含 "(Hpx)"，能滚动。
- F12 Console 无报错。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git commit -m "feat(preview): list handler——全量塞 item + 原生滚动（不做虚拟化）"
```

---

### Task 8: dyntree handler（建/删 panel + mail overlay）

动态建/删 panel（对齐 C# `CreateDynPanel`），dyn-load-mail/showcase 用内嵌 mail 模板做 overlay。

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`

**Interfaces:**
- Consumes: `wireBackHome()`、`bindClick`、`$`、`showMail()`、`hideMail()`（骨架已定义）。
- Produces: 闭包内 `dynPanels`/`dynSeq`/`dynStyleToggled` 状态（handler 内部，无外部消费者）。

- [ ] **Step 1: 替换 dyntree stub**

Edit `loom-preview.js`：
- `old_string`: `    page_dyntree:  function () { wireBackHome(); /*Task8*/ },`
- `new_string`:
```js
    page_dyntree:  function () {
      wireBackHome();
      var dynPanels = [];
      var dynSeq = 0;
      var dynStyleToggled = false;
      var anchor = $('dyn-anchor');
      // panel 视觉对齐 C# CreateDynPanel（深色卡 + span 标题 + img icon）。
      function createPanel() {
        dynSeq++;
        var panel = document.createElement('div');
        panel.style.cssText = 'width:120px;height:90px;background:#2a2f45;border-radius:8px;flex-direction:column;gap:4px;padding:6px';
        var title = document.createElement('span');
        title.style.cssText = 'font-size:14px;color:#e6e6e0';
        title.textContent = 'item-' + dynSeq;
        panel.appendChild(title);
        var icon = document.createElement('img');
        icon.src = 'res/icons/skin.png';
        icon.style.cssText = 'width:40px;height:40px';
        panel.appendChild(icon);
        return panel;
      }
      bindClick('dyn-add', function () { if (anchor) { var p = createPanel(); anchor.appendChild(p); dynPanels.push(p); } });
      bindClick('dyn-add20', function () {
        if (!anchor) return;
        var frag = document.createDocumentFragment();
        for (var i = 0; i < 20; i++) { var p = createPanel(); frag.appendChild(p); dynPanels.push(p); }
        anchor.appendChild(frag);
      });
      bindClick('dyn-del', function () {
        var last = dynPanels.pop();
        if (last && last.parentNode) last.parentNode.removeChild(last);
      });
      bindClick('dyn-clear', function () {
        dynPanels.forEach(function (p) { if (p.parentNode) p.parentNode.removeChild(p); });
        dynPanels = [];
      });
      bindClick('dyn-style', function () {
        if (!dynPanels.length) return;
        var last = dynPanels[dynPanels.length - 1];
        dynStyleToggled = !dynStyleToggled;
        last.style.cssText = dynStyleToggled
          ? 'background:#c2605a;width:160px;height:70px;border-radius:16px;flex-direction:column;gap:4px;padding:6px'
          : 'width:120px;height:90px;background:#2a2f45;border-radius:8px;flex-direction:column;gap:4px;padding:6px';
      });
      bindClick('dyn-load-mail', function () {
        showMail();
        var s = $('dyn-load-status'); if (s) s.textContent = '当前：mail';
      });
      bindClick('dyn-load-showcase', function () {
        hideMail();
        var s = $('dyn-load-status'); if (s) s.textContent = '当前：showcase';
      });
    },
```

- [ ] **Step 2: 浏览器验证**

双击 `page_dyntree.html`（或从 home 进"动态树"）。
Expected:
- 点"建 1 个 panel" → anchor 区出现一个深色小卡（item-1 标题 + 图标）。
- 点"建 20 个" → 一批 panel 出现。
- 点"删除最后" → 末个 panel 消失。
- 点"清空" → 全没。
- 点"toggle 末个样式" → 末个 panel 变红/变大/圆角变化。
- 点"instantiate 邮件界面" → 半透明遮罩 + 居中显示邮件列表（600×800，3 封邮件 + 一键领取按钮）；状态文本变"当前：mail"。
- 点"切回 showcase" → 邮件消失；状态变"当前：showcase"。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git commit -m "feat(preview): dyntree handler——建删 panel + mail overlay（内嵌模板）"
```

---

### Task 9: tween handler（CSS transition 动画）

不移植 10 条 ease 数学——注入一段 `<style>` 定义各 target 的 `.play` 末态 + `kill-target` 持续旋转，按钮 toggle class 触发 transition。ease 用 3 条近似 cubic-bezier。

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`

**Interfaces:**
- Consumes: `wireBackHome()`、`bindClick`、`$`、`pulseLamp`（骨架已定义）。

- [ ] **Step 1: 替换 tween stub**

Edit `loom-preview.js`：
- `old_string`: `    page_tween:    function () { wireBackHome(); /*Task9*/ },`
- `new_string`:
```js
    page_tween:    function () {
      wireBackHome();
      // 注入 tween 动画样式：.play = 末态（CSS transition 驱动，不移植 ease 数学）。
      var st = document.createElement('style');
      st.textContent =
        '#t-opacity,#t-translate,#t-scale,#t-rotate,#t-bgcolor,#t-textcolor,' +
        '#ease-0,#ease-1,#ease-2,#d-0,#d-1,#d-2{transition:all .8s ease}' +
        '#t-opacity.play{opacity:0}' +
        '#t-translate.play{transform:translateX(40px)}' +
        '#t-scale.play{transform:scale(1.4)}' +
        '#t-rotate.play{transform:rotate(360deg)}' +
        '#t-bgcolor.play{background-color:#6fa66c}' +
        '#t-textcolor.play{color:#c2605a}' +
        '#ease-0.play,#ease-1.play,#ease-2.play{transform:translateX(200px)}' +
        '#d-0.play,#d-1.play,#d-2.play{opacity:1}' +
        '#kill-target{animation:loom-spin 4s linear infinite}' +
        '@keyframes loom-spin{to{transform:rotate(360deg)}}' +
        '#kill-target.paused{animation-play-state:paused}' +
        '#kill-target.cleared{animation:none;transform:none}';
      document.head.appendChild(st);
      // d-0/1/2 初始隐（delay 错峰从隐到显）。
      ['d-0', 'd-1', 'd-2'].forEach(function (id) { var el = $(id); if (el) el.style.opacity = '0'; });
      function tg(id) { var el = $(id); if (el) el.classList.toggle('play'); }
      // 6 prop 同放（末态来自上面 .play 规则）。
      bindClick('tween-play', function () {
        ['t-opacity', 't-translate', 't-scale', 't-rotate', 't-bgcolor', 't-textcolor'].forEach(tg);
      });
      // 3 条近似 ease（QuadIn / CubicOut / BackInOut）。
      bindClick('ease-play', function () {
        var eases = ['cubic-bezier(.55,.085,.68,.53)', 'cubic-bezier(.215,.61,.355,1)', 'cubic-bezier(.68,-.55,.265,1.55)'];
        ['ease-0', 'ease-1', 'ease-2'].forEach(function (id, i) {
          var el = $(id);
          if (el) { el.style.transition = 'transform 1s ' + eases[i]; el.classList.toggle('play'); }
        });
      });
      // delay 错峰：递增 transition-delay。
      bindClick('delay-play', function () {
        ['d-0', 'd-1', 'd-2'].forEach(function (id, i) {
          var el = $(id);
          if (el) { el.style.transitionDelay = (i * 0.2) + 's'; el.classList.toggle('play'); }
        });
      });
      // complete：t-opacity 动画结束后亮灯。
      bindClick('complete-play', function () {
        var el = $('t-opacity');
        if (!el) return;
        el.classList.toggle('play');
        el.addEventListener('transitionend', function done() {
          el.removeEventListener('transitionend', done);
          pulseLamp('complete');
        });
      });
      // kill 冻结当前角（pause）；clear 清动画回 CSS 初始。
      bindClick('kill-btn', function () {
        var el = $('kill-target');
        if (el) { el.classList.add('paused'); el.classList.remove('cleared'); }
      });
      bindClick('clear-btn', function () {
        var el = $('kill-target');
        if (el) { el.classList.remove('paused'); el.classList.add('cleared'); }
      });
    },
```

- [ ] **Step 2: 浏览器验证**

双击 `page_tween.html`（或从 home 进"动效"）。
Expected:
- `kill-target` 进页即持续旋转。
- 点"播放全部" → 6 个 demo 块动画到末态（opacity/translate/scale/rotate/bg-color/text-color 各自变化）；再点 → 回初始。
- 点 7.2 "播放" → 3 条 ease-row 块以不同节奏横移 200px。
- 点 7.3 "播放（错峰）" → 3 块依次淡入。
- 点 7.4 "播放（结束后亮灯）" → t-opacity 动画结束后 lamp-complete 闪一下。
- 点"kill（停末值）" → kill-target 停在当前角度；点"clear（回 CSS）" → 重置（不再旋转，回正）。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git commit -m "feat(preview): tween handler——CSS transition 动画（ease 近似）"
```

---

### Task 10: interact handler（灯阵事件）

各交互元素绑浏览器事件 → `pulseLamp`。disabled 不绑（HTML 已带 `.disabled` 类）。

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/preview/loom-preview.js`

**Interfaces:**
- Consumes: `wireBackHome()`、`bind`、`$`、`pulseLamp`（骨架已定义）。

- [ ] **Step 1: 替换 interact stub**

Edit `loom-preview.js`：
- `old_string`: `    page_interact: function () { wireBackHome(); /*Task10*/ },`
- `new_string`:
```js
    page_interact: function () {
      wireBackHome();
      // click / dblclick
      bind('hit-click', 'click', function () { pulseLamp('click'); });
      bind('hit-click', 'dblclick', function () { pulseLamp('click'); });
      // hover（RollOver/Out）
      bind('hit-hover', 'mouseenter', function () { pulseLamp('hover'); });
      bind('hit-hover', 'mouseleave', function () { pulseLamp('hover'); });
      // drag（HTML5 dragstart/drag）
      bind('hit-drag', 'dragstart', function () { pulseLamp('drag'); });
      bind('hit-drag', 'drag', function () { pulseLamp('drag'); });
      // longpress（mousedown 起 1.5s timer）
      var lpTimer = null;
      var lp = $('hit-longpress');
      if (lp) {
        lp.addEventListener('mousedown', function () {
          lpTimer = setTimeout(function () { pulseLamp('longpress'); }, 1500);
        });
        lp.addEventListener('mouseup', function () { if (lpTimer) clearTimeout(lpTimer); });
        lp.addEventListener('mouseleave', function () { if (lpTimer) clearTimeout(lpTimer); });
      }
      // key（聚焦后按键）
      bind('hit-key', 'keydown', function () { pulseLamp('key'); });
      // 路由：inner stopPropagation 止冒泡。
      bind('route-outer', 'click', function () { pulseLamp('route'); });
      bind('route-pe', 'click', function () { pulseLamp('route'); });
      bind('route-inner', 'click', function (e) { e.stopPropagation(); pulseLamp('route'); });
      // hit-disabled 不绑（HTML 已带 .disabled，视觉灰 + 不响应）
    },
```

- [ ] **Step 2: 浏览器验证**

双击 `page_interact.html`（或从 home 进"交互"）。
Expected:
- 点 4.1 "点我" → lamp-click 闪；双击 → 多闪。
- 悬停 4.2 "悬停我" → 进/离都闪 lamp-hover。
- 拖动 4.4 "拖我" → lamp-drag 闪。
- 长按 4.5 "长按 1.5s" 1.5 秒 → lamp-longpress 闪。
- 点 4.6 聚焦后按键 → lamp-key 闪。
- 点 4.7 内层（route-inner）→ lamp-route 闪，外层不闪（stopPropagation）；点 route-pe → 闪。
- 4.3 禁用块点击无反应。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/preview/loom-preview.js
git commit -m "feat(preview): interact handler——灯阵事件绑定"
```

---

### Task 11: 最终验证（全页走查 + packager 不变确认）

全 9 页浏览器走查 + 确认 head 注入不影响打包。

**Files:** 无（仅验证）。

- [ ] **Step 1: 浏览器全页走查**

双击 `home.html`，逐个 nav 卡片进入，每页确认：布局对（flex 正确、颜色/尺寸对）、`← 返回` 可用、Console 无报错。重点：
- home：8 卡片横排换行 + tips 演示。
- controls：色块/img/span/button 三态/flex 练兵场 + model-slot 占位。
- text：文本样式。
- image：border/opacity/transform/调色板/bg-image/border-radius/filter/九宫（图都加载，靠 `<base>`）。
- scroll：overflow 滚动。
- tween：6 prop 动画 + ease/delay/complete/kill。
- interact：灯阵全亮。
- dyntree：建删 panel + mail overlay。
- list：双列表全量 item + 滚动。

- [ ] **Step 2: 确认 packager 仍正常（head 注入不影响 pkg.bin）**

跑围栏契约门 + 包测试：
```bash
cargo test -p loomgui_core fence_contract
cargo test -p loomgui_core ignores_head_when_parsing_body
cargo test -p loomgui_pkg
```
Expected: 全 PASS。

再按 `.claude/skills/loomgui-editor/config.json` 里的 pkg 命令重打 showcase（读 config 拿 exe_path + 参数），确认：
- 退出码 0（无围栏违规报错）。
- `showcase.pkg.bin` 正常产出（对比大小合理——head 注入 3 行不应显著改变 pkg.bin，因为 head 不进包）。

> 若 pkg 报围栏违规：说明 head 内容被 parse 了——回头查 Task 1 测试为何没挡住（或 parse 行为变了）。

- [ ] **Step 3: 若一切正常，无需额外 commit（前面 10 个 task 已分步 commit）**

若 Step 2 发现需要微调（如某页 console 报错修了），修完 commit：
```bash
git add -A
git commit -m "fix(preview): 最终走查微调"
```

---

## Self-Review 记录

- **Spec 覆盖**：spec §3 架构（Task 2-4）、§4 driver 结构（Task 3 骨架）、§5 各页行为（Task 5-10 一页一 task）、§6 保真度（README + 各 task 验证说明）、§7 验证（Task 1 + Task 11）全覆盖。
- **占位符扫描**：无 TBD/TODO；每步都有完整代码或确切命令。
- **类型/命名一致**：`$`/`bind`/`bindClick`/`wireBackHome`/`wireNav`/`ensureOverlay`/`showTips`/`showMail`/`hideMail`/`pulseLamp` 在 Task 3 定义，Task 5-10 按相同签名使用，一致。
