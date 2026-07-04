# Showcase 浏览器预览 · 设计

> 日期：2026-07-04
> 目的：让 showcase 的 HTML 能直接双击在浏览器打开，视觉上还原 LoomGUI 的渲染结果，用来对照"Unity 实际渲染"和"AI 想要的效果"是否一致。

## 1. 目标与非目标

### 目标
- **双击即用**：任意 showcase 页（`home.html` / `page_*.html`）双击在浏览器打开，布局/颜色/尺寸/图片与 LoomGUI 渲染一致。零服务器、零构建。
- **可浏览**：home 的 nav 卡片能点进各页，各页 `← 返回` 能回 home。
- **动态页有内容看**：虚拟列表、动态树、tween、灯阵这些靠 C# driver 运行时建子节点的页，在浏览器里也能看到内容、能交互。
- **核心价值**：视觉验证——快速发现"Unity 渲染 ≠ AI 想要的样子"的偏差。

### 非目标（明确不做）
- **不**忠实移植 C# driver 的行为逻辑（虚拟列表的 slot 复用/reuse_key、tween 的 10 条 ease 曲线、drag/longpress 的精确触发条件）——只做"够看视觉"的简化版。
- **不**做通用 LoomGUI-in-browser 运行时（不移植 taffy/scene/render）。
- **不**支持 NativeHost（外部 Cube GO，浏览器无法复刻，显示占位文本）。
- **不**追求像素级文本/动画保真（preview-trust.md 已记录的不可信项仍不可信）。

## 2. 关键事实（设计成立的前提）

- **打包器只遍历 `<body>`**（`loomgui_core/src/parse/dom.rs:35-50`，`body_sel` + `body.children()`），head 整个不碰。→ showcase 页 head 里加 `<base>`/`<link>`/`<script>` 都不进 pkg.bin，包源语义零变化。**实现时先改一页实测重打 pkg.bin 验证此假设**（见 §6）。
- **图片在工作区根的 `res/icons/`**（`LoomUI/res/icons/*.png`），但 showcase HTML 在 `LoomUI/showcase/` 子目录，且 HTML 里写的是工作区根相对路径 `res/icons/...`。→ 浏览器从 `showcase/page.html` 解析 `res/icons/` 会找 `showcase/res/` → 404。需修。
- **ES module 在 file:// 被 CORS 拦**，经典 `<script src>` 不拦。→ driver 必须是单文件经典脚本（非 `type="module"`）。
- **`fetch()`/XHR 在 file:// 被拦**，`<iframe>` 导航和 `<script src>` 不拦。→ 组件 overlay（mail/tips）不能 fetch，改用内嵌模板。

## 3. 架构

### 3.1 文件布局

```
loomgui_unity/Assets/LoomUI/
├── showcase/                       # 包源（packager 吃这个）
│   ├── home.html                   # head 加 3 行（见 §3.2）
│   ├── page_*.html (8 个)          # 同上
│   ├── mail.html / tips_toast.html # 组件，不加 head（driver 用内嵌模板，不 fetch）
│   └── list_item.html
├── res/icons/*.png                 # 图片原位
└── preview/                        # ← 新增，浏览器预览专属
    ├── loom-preview.js             # driver（单文件经典脚本，~200 行）
    ├── preview-base.css            # polyfill + 预览壳（居中/设备框）
    └── README.md                   # 怎么开（双击 home.html 即可）
```

### 3.2 每个 showcase 页 head 加 3 行

紧跟 `<meta charset>` 之后插入（`<base>` 必须在所有带相对 URL 的元素之前）：

```html
<base href="..">                                          <!-- 锚到 LoomUI/，修图片路径 -->
<link rel="stylesheet" href="preview/preview-base.css">   <!-- polyfill + 预览壳 -->
<script src="preview/loom-preview.js"></script>           <!-- driver -->
```

需加的页：`home.html` + 8 个 `page_*.html` = **9 个**。`mail.html`/`tips_toast.html`/`list_item.html` 是组件（被 driver 用内嵌模板呈现），不加。

### 3.3 三个关键机制

**① `<base href="..">` 一次解决所有相对路径**
HTML 在 `showcase/`，`<base href="..">` 把基准锚到 `LoomUI/`：
- 静态 `res/icons/home.png`（含 `<style>` 里的 `url(res/...)`）→ `LoomUI/res/icons/` ✓
- driver 动态建的 `<img src="res/icons/skin.png">` → 同样 ✓（统一用 `res/` 形式）
- `preview/loom-preview.js` 自身、`preview/preview-base.css` → `LoomUI/preview/` ✓
- 页导航 `location.href='showcase/page_list.html'` → `LoomUI/showcase/...` ✓

**② polyfill 对齐 LoomGUI 布局语义**（`preview-base.css`）
```css
div { display: flex; flex-direction: column; }   /* LoomGUI 契约：div 永远 flex column */
* { box-sizing: border-box; }                     /* taffy border-box */
body { margin: 0; background: #0e1018;
       display: flex; justify-content: center; align-items: flex-start;
       min-height: 100vh; padding: 24px; }
.root { box-shadow: 0 0 0 1px #2a2f45, 0 8px 40px rgba(0,0,0,.6); }   /* 设备框 */
```
页自带类选择器（如 `.grid{flex-direction:row}`，specificity 0,1,0）盖过 polyfill 的 `div{}`（0,0,1），与 taffy 行为一致。

**③ fit-scale 镜像 engine letterbox**
driver boot 时 `document.body.style.zoom = min(vw/1080, vh/1920)`（onload + onresize），对应 engine 的 `sf = min(area.w/dw, area.h/dh)`。Chrome/Edge 支持 `zoom`；旧 Firefox 无效（降级为不缩放，用户 Ctrl+-，可接受）。

## 4. driver.js 结构

单文件经典脚本，IIFE：

```
CONFIG
├ NAV: { 'nav-controls':'page_controls', 'nav-text':'page_text', ... }   // 8 项，抄 SubscribeHome
└ TEMPLATES: { mail: `<style>...</style><div class="root">...`, tips: `<style>...</style><div class="toast">...` }
   // 从 mail.html/tips_toast.html 拷 **`<style>` + body markup 一起**——overlay 注入到别的页时
   // 宿主页没有这些类（.root/.mail-item/.toast）的 CSS，模板须自带样式才不裸奔。
   // **注意改名**：mail.html 的 wrapper 也叫 `.root`，会和宿主页 `.root`(1080×1920) 撞
   // （注入的 `<style>` 全局生效）。拷贝时把 mail 的 `.root` → `.mail-root`（CSS + div class 同改）。

boot()         → injectFitScale() + 按文件名路由到 page handler
shared
├ wireBackHome()   #back-home → location.href='showcase/home.html'
├ wireNav()        home 的 #nav-* → 各页
├ ensureOverlay(name, containerCss)   按需建 overlay 容器（见下），返回元素
├ showTips()       ensureOverlay('tips') → 注入 TEMPLATES.tips → 2s 移除
├ showMail()/hideMail()   ensureOverlay('mail') → 注入 TEMPLATES.mail / 移除
└ pulseLamp(name)  lamp-{name} 容器 opacity 脉冲（CSS transition）
pages
├ home()        wireNav + nav-tips-demo → showTips
├ controls()    back + #model-slot 注入占位 "[NativeHost 外部 GO·预览不支持]"
├ text/image/scroll()   仅 back
├ tween()       back + 各 t-* 按钮 → CSS transition 动画（toggle class 到末态）
├ interact()    back + click/hover/drag/longpress/key/route → pulseLamp
├ dyntree()     back + dyn-add/add20/del/clear/style + dyn-load-mail/showcase
└ list()        back + 往两个 scroll 容器塞全量 item
```

## 5. 各页行为（简化版，够看视觉即可）

| 页 | driver 行为 |
|---|---|
| **home** | `#nav-*` → `location.href` 跳页；`nav-tips-demo` → showTips |
| **controls** | back；`btn-demo-disabled` HTML 已带 `.disabled` 不绑事件；`#model-slot` 显示占位文本 |
| **text / image / scroll** | 仅 back。image 的图靠 `<base>` 加载；scroll 用浏览器原生 `overflow:scroll` |
| **tween** | `kill-target` 进页即 CSS animation 持续旋转；各按钮（tween-play/ease-play/delay-play/complete-play/kill/clear）toggle class 触发 CSS `transition` 到末态。**不移植 10 条 ease 数学**，用 CSS 默认 timing function，看到"动了、方向对"即可 |
| **interact** | click/dblclick/hover/drag/longpress/key/route 各绑简单事件 → `pulseLamp`。**不抠触发条件微差**（如长按精确秒数） |
| **dyntree** | dyn-add 建 panel（div+span+img `res/icons/skin.png`）进 `#dyn-anchor`；add20/del/clear/style；dyn-load-mail→showMail / dyn-load-showcase→hideMail（clone 内嵌模板）；status 文本更新 |
| **list** | **不做虚拟列表逻辑**。equal：往 `#list-equal` 塞 1000 个 item（row=灰底 `#252839` + 48×48 icon + "Item N"，height 80px）；variable：往 `#list-variable` 塞 200 个 item，height 用 `100+40*sin(i*0.3)`，标题 "Item N (Hpx)"。容器 `overflow-y:scroll` 普通流，浏览器原生滚动 |

**list item 视觉**（对齐 C# `CreateItem` 样式，让浏览器看的和 Unity 一致）：
```
<div style="width:100%;height:{H}px;flex-direction:row;align-items:center;
            gap:12px;padding:0 16px;background-color:#252839">
  <img src="res/icons/skin.png" style="width:48px;height:48px">
  <span style="color:#e0e0e0;font-size:20px">Item {N}</span>
</div>
```

**overlay 容器样式**（driver `ensureOverlay` 建的宿主 div，position:fixed 盖在页面上）：
- `tips` 容器：`position:fixed;inset:0;flex-direction:column;align-items:center;justify-content:flex-end;padding:40px;pointer-events:none;z-index:50`（对齐 C# tips_layer）。toast 进它底部居中。
- `mail` 容器：`position:fixed;inset:0;display:flex;align-items:center;justify-content:center;background:rgba(0,0,0,.5);z-index:60`（半透明遮罩 + 居中）。mail.root（600×800）进它中央。

## 6. 保真度边界

**对齐得好的（浏览器 ≈ LoomGUI）**——靠 polyfill + 围栏本来就是 Chrome 原生：flex 布局/方向/gap/justify/align、px/% 尺寸、color/opacity/border/radius、background-image/size、filter、transform、overflow:scroll、九宫 border-image-slice、列表几何/滚动。这是"AI 想要的样子"的主体，浏览器可信。

**有差距的（预览用来抓布局偏差，不卡像素）**：
- **tween 动画**：CSS transition 近似，不逐曲线对齐 ease（看得到动，节奏大致对）。
- **文本换行/字距像素**：Chrome 文本引擎 vs LoomGUI(unicode-linebreak)，换行点/宽度偏（preview-trust 已知）。CJK 缺字等细节以 Unity 为准。
- **drag/longpress/key 触发条件**：浏览器事件近似 LoomGUI 语义，微差。
- **NativeHost**：外部 Cube GO 无法复刻，占位文本。
- **overlay mail/tips**：用 driver.js 内嵌模板（从 mail.html/tips_toast.html 拷的 markup）。**若改这俩组件源文件，须手动同步 driver.js 里的 TEMPLATES**（代码注释提醒）。

## 7. 实现验证（关键假设先测）

1. **packager head 盲区**：改 `home.html` head 加 3 行 → 跑打包器（`loomgui_pkg.exe` 或 `cargo run -p loomgui_pkg`）→ 确认 pkg.bin 正常产出 + 无报错。理论依据 dom.rs:35，实测兜底。
2. **`<base href="..">` 路径**：双击 `home.html` → 确认 nav 跳页 OK + page_image 的 `res/icons/` 图加载 OK + driver.js 加载 OK。
3. **list 滚动**：双击 `page_list.html` → 两个列表有内容、能滚、视觉对。

## 8. 实施步骤（给 writing-plans 的输入）

1. 建 `LoomUI/preview/preview-base.css`（polyfill + 壳）。
2. 建 `LoomUI/preview/loom-preview.js`（CONFIG + boot + shared + 9 个 page handler + 内嵌 mail/tips 模板）。
3. 给 9 个 showcase 页 head 各加 3 行（`<base>`+`<link>`+`<script>`）。
4. 建 `LoomUI/preview/README.md`（一句"双击 showcase/home.html"）。
5. 验证 §7 三项。
6. 跑打包器确认 pkg.bin 不受影响。
