# 靶子冻结 Showcase 设计

> **状态**：Draft（待评审）
> **日期**：2026-07-14
> **流程**：superpowers:brainstorming（已走完 7 项决策，会话 `showcase-freeze-20260714181831`）→ spec
> **关联**：本文档冻结 R2–R7 运行时重写的渲染靶子；运行时 API 另起 spec，不在此处。

---

## 0. 定位与范围

**靶子（target）** = 一套覆盖围栏全部标签与 CSS 的 showcase 工作区：设计期 HTML/CSS 源 + 设计令牌 + 浏览器预览辅助代码。R2–R7 运行时重写全程以"渲染结果是否对齐靶子"为验收标准。靶子在重写期冻结，运行时只冲刺着它做，不再回头改靶子。

**HTML 定位：展示优先。** showcase 的 HTML 是设计期视觉/结构靶子，浏览器预览只求"看起来对"。运行时真实交互（数据驱动、事件、虚拟列表 slot 映射、TweenManager 动画、NativeHost 3D）由配套 **C# 运行时辅助代码** 兑现，不要求 HTML 预览做行为镜像。

**范围切分**（本轮决策）：

- 本 spec：showcase 靶子设计（页面结构、标签覆盖、CSS 覆盖、视觉令牌、预览基建、动效要求）。
- API spec：运行时类型化对象树 API 另起一份，不在本文档。

---

## 1. 围栏约束（硬约束）

**权威真相源**：`crates/fence/src/schema/`（`tag.rs` / `attr.rs` / `css.rs`）+ [docs/design/fence.md](/F:/WorkSpace/projects/LoomGUI/docs/design/fence.md)。`api-refactor-design.md §8` 的标签清单已过时，**不作数**。

showcase 只允许使用当前围栏的 **23 个运行时标签**：

```
div  header  nav  p  span  strong  em  br
label  button  a  img  canvas
input  textarea  select  option  progress
ul  ol  li  template  slot
```

外加：含 hyphen 的标签名 = CustomElement（如 `<item-card>`）；全局属性 `role` / `aria-*` / `data-*` / `--*` 全可用。

### 严禁清单与替代映射

下列标签**一律不用**（即便旧设计文档列过），改用围栏内等价物：

| 语义需求 | 禁用标签 | 围栏内替代 |
|---|---|---|
| 标题层级 | h1–h6 | `<p>` + `font-size` / `font-weight` |
| Tab 切换 | （无原生） | `<div role="tablist">` + `<button role="tab">` + `<div role="tabpanel">` |
| 弹窗/确认 | dialog | `<div role="dialog">` + `display:none/flex` 切换，或 custom-element |
| 折叠面板 | details / summary | `<button>` + `<div>` + `display` 切换 |
| 状态条 | meter | `<progress>`（HP/MP/经验/库存全用它） |
| 表单分组 | form / fieldset / legend | `<div>` + `<p>`（组标题） |
| 结构容器 | main / section / footer / article / aside | `<div>` / `<header>` / `<nav>` |
| 附注小字 | small | `<span>` + `font-size` |

> 这条原则比"标签好不好用"更重要：23 标签就是围栏给 AI 的全部词汇表。靶子的价值在于证明这套词汇 + CSS 足以拼出完整游戏 UI。若某处不够用，那是要讨论"围栏是否该扩"的设计决策，而不是默认假设标签会存在。

---

## 2. 视觉令牌 — Deep Ocean

深青底 + 线条款边框 + 暖琥珀强调。冷静科技感，冷色不偏纯蓝（青底调），琥珀破单色。落到 `design-systems/tokens.css`。

**色板**

| token | 值 | 用途 |
|---|---|---|
| `--bg` | `#0e1620` | 画布底色 |
| `--surface` | `#152433` | 卡片/面板底 |
| `--surface-2` | `#1a2f45` | 次级面板/嵌套 |
| `--border` | `#2a5a75` | 描边/分隔线 |
| `--accent` | `#5fb4d4` | 主强调（青） |
| `--accent-soft` | `#8ec5d8` | 强调亮调（文字/图标） |
| `--gold` | `#d4a44e` | 次强调（琥珀，CTA/数值） |
| `--fg` | `#e0e6ec` | 主文本 |
| `--muted` | `#9aa0b4` | 次级文本 |
| `--dim` | `#6c7080` | 弱化文本/占位 |
| `--success` | `#7db86a` | 成功/正向 |
| `--danger` | `#c2605a` | 危险/扣减 |

**排版尺度**（继承旧 tokens，font-size 阶梯）：`12 / 14 / 16 / 18 / 22 / 28 / 36 / 48 px`。

**间距 / 圆角**：`--space-1..6 = 4/8/12/16/20/24 px`；`--radius-sm=4px`、`--radius-md=8px`、`--radius-lg=10px`。

> LoomGUI 不强制用 `var()`，可直接写 hex。tokens.css 主要给设计师/AI 一处统一调色。

---

## 3. 画布与分辨率

**1920×1080 横屏**（PC/主机游戏主流）。每页根容器 `.root { width:1920px; height:1080px }` 做设备框，浏览器预览居中 + letterbox 缩放（由 `loom-preview.js` 的 body 缩放模拟 engine letterbox）。

旧 showcase 是 1080×1920 竖屏；新靶子整体改横屏，旧布局需重新适配，不复用旧 `.root` 尺寸。

---

## 4. 预览架构 — 分离注入

靶子要在浏览器里直接预览（像旧 showcase 那样），但预览代码**不得污染围栏 HTML 源**，否则差分门比的是"预览补丁"而非围栏本身。

**方案：分离注入。** 每页 `<head>` 里引入预览资源，`body` 零预览代码。打包器只消费 `body`，`head` 不进 `pkg.bin`。

```html
<!DOCTYPE html>
<html lang="zh">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="preview/preview-base.css">
  <script src="preview/loom-preview.js"></script>
  <title>...</title>
  <style> /* 本页组件 CSS（围栏内） */ </style>
</head>
<body>
  <!-- 仅围栏 HTML，零预览代码。打包器从这里消费。 -->
  <div class="root"> ... </div>
</body>
</html>
```

### preview-base.css（UA 基线）

polyfill 对齐 LoomGUI 渲染约定，让 Chromium 预览尽量贴运行时：

- `div { display:flex; flex-direction:column }`（旧范式 div 永远 flex column 的 UA 基线；R2 后由围栏 DisplayDefault 决定，preview 仍以此 polyfill 贴近默认 flex 体感）。
- `* { box-sizing:border-box }`（taffy border-box）。
- `body { margin:0 }` + 居中 + `.root` 设备框 box-shadow。
- `@font-face` 加载预览字体（对齐默认字体）。

### loom-preview.js（导航 + 交互模拟）

经典脚本（非 ES module，避免 file:// 被 CORS 拦），职责：

- **导航**：home 的 nav 卡片 → 各子页；各页 `#back-home` → home。同目录跳转，规避 `<base>` 解析坑。
- **role=tablist 切换模拟**：`role=tab` 点击 → 切 `role=tabpanel` 显隐 + aria-selected。
- **弹窗模拟**：`role=dialog` 的按钮 → `display` 切换。
- **ListView 视觉填充**：浏览器无真实虚拟化，preview 把 `<template>` 克隆 N 份填进 `<ul>`，只验视觉（不等高补偿/slot 复用属运行时）。
- **NativeHost 占位**：`canvas#native-slot` 渲染占位文本（"3D 角色 slot · 运行时渲染"），无真实 3D。
- **body 缩放 letterbox**：按视口等比缩放 `.root`，模拟 engine letterbox。

### 可信度分层

| 层 | 可信（对齐运行时） | 近技（抓布局偏，不像素级） | 纯运行时（不在 HTML 镜像） |
|---|---|---|---|
| 内容 | flex 布局、px/% 尺寸、color/opacity/border/radius、bg-image/size、filter、transform、overflow:scroll、九宫 border-image-slice、列表骨架 | 文本换行/字距（Chrome 引擎 vs unicode-linebreak 偶偏）、tween 动画（CSS transition 近似）、drag/longpress/key 事件 | TweenManager 逐曲线 ease、虚拟列表 slot 复用、NativeHost 3D/粒子、事件系统、overlay 叠加时序 |

---

## 5. 目录结构

```
showcase/                       工作区根（repo 顶层，新建；旧 showcase_project/ 搬迁后删除）
  loom.workspace.json          ← 打包器 init 生成，不手写
  showcase/
    home.html                  导航枢纽
    settings.html              控件全覆盖
    inventory.html             背包 · ListView 主战场
    mail.html                  邮件 · ListView + 富文本
    shop.html                  商店 · progress + 弹窗
    character.html             角色 · canvas 3D + 特效
    form.html                  角色创建 · 表单
    lab.html                   CSS 全属性标本馆
    components/
      nav-bar.html             共享顶栏（custom-element 或 include）
      stat-bar.html            stat + progress 组件
      item-card.html           物品卡（custom-element + slot）
    preview/
      preview-base.css         UA 基线
      loom-preview.js          导航 + 交互模拟
      README.md                预览可信度说明
  design-systems/              ← 全部重写（不搬旧版令牌）
    tokens.css                 Deep Ocean 设计令牌
  res/
    fonts/                     从旧 showcase_project 搬（搬迁后旧目录删）
    icons/                     游戏图标（搬 + 扩充，见 §10）
```

---

## 6. 页面设计

每页：定位 / 主标签 / 区块 / 动效。所有页都加动效（hover / 进入 / 状态切换 / 循环），验证动效通道——见 §7。

### 6.1 home.html — 导航枢纽

- **定位**：游戏主菜单，跳所有子页。
- **主标签**：`header` `nav` `div` `p` `span` `strong` `a` `img` `button`。
- **区块**：
  - Hero：`img`（logo）+ `p`（大标题，靠 font-size/font-weight）+ `span`（副标题/版本号）。
  - 主操作：`button` 组（开始游戏 / 继续 / 退出）。
  - `nav` 导航卡片网格：7 张 `a`（href 到 settings/inventory/mail/shop/character/form/lab），每张含 `img` 图标 + `span` 标签 + `span` 描述。
  - 底部信息条：`div` + `span`（替代 footer）。
- **动效**：卡片 hover 缩放高亮、入场序列淡入。

### 6.2 settings.html — 控件全覆盖

- **定位**：吃掉全部 input 类型 + select + role dispatch。
- **主标签**：`div[role=tablist]` `button[role=tab]` `div[role=tabpanel]` `label` `input`（全 6 型）`select` `option` `p` `span`。
- **区块**（左 tablist + 右 panel）：
  - 音效：`input[type=range]`（音量）+ `input[type=number]`（数值）。
  - 画面：`input[type=range]`（亮度）+ `select`/`option`（分辨率）+ `input[type=checkbox]`（全屏/垂直同步）。
  - 操作：`input[type=radio]`（按键方案）+ `input[type=text]`（自定义按键）+ `label[for]`。
  - 账号：`input[type=text]` + `input[type=password]` + `input[type=checkbox]`（记住）。
  - 搜索框可用 `input[type=search]`（围栏内，补足第 7 个 text 变体）。
- **动效**：tab 切换淡入、滑块/开关态过渡。

### 6.3 inventory.html — 背包 · ListView 主战场

- **定位**：虚拟列表视觉骨架 + 物品网格 + 详情。
- **主标签**：`ul` `template` `li` `img` `span` `progress` `button` `strong` `em` `p` `div`。
- **区块**：
  - 左：`ul` + `<template>`（根 `<li>`）物品格列表；每格 `img`（图标）+ `span`（数量徽章）+ `progress`（耐久）。
  - 右：详情面板——`img` 大图 + `p` 名称 + `p` 描述 + `strong`/`em` 属性富文本 + `button`（使用/丢弃/分解）。
  - 网格容器用 `div` + `flex-wrap`。
- **动效**：格子 hover 高亮、选中描边脉冲、详情面板切换滑入。

### 6.4 mail.html — 邮件 · ListView + 富文本

- **定位**：ListView 第二实例 + 富文本全家族（R7 文本目标）。
- **主标签**：`ul` `template` `li` `p` `strong` `em` `br` `a` `span` `button`。
- **区块**：
  - 左：`ul` + `<template>` 邮件列表；未读用 `span` + 强调色。
  - 右：邮件正文 `p` 段落 + `strong`/`em` 嵌套 + `br` 硬换行 + `a`（领奖链接）跨行 + 行内物品 `span`。
- **动效**：列表项滑入、未读标记呼吸闪烁、正文切换淡入。
- **搬迁**：富文本内容直接搬旧 [page_text.html](/F:/WorkSpace/projects/LoomGUI/showcase_project/showcase/page_text.html) B1–B9（多色聊天卡 / 粗斜体嵌套 / 装饰线 / 行内图 / 超链接跨行）。关键变化：旧版靠 `display:block` div 暗号触发富文本，新版用标准 `<p>` + inline 子树，语义天然正确。

### 6.5 shop.html — 商店 · progress + 弹窗

- **定位**：progress 多用 + 弹窗（div role=dialog 替代 dialog）+ 数量输入。
- **主标签**：`div` `img` `p` `span` `progress` `button` `a` `input[type=number]`。
- **区块**：
  - 商品卡片网格：`img` + `p`（名称/价格）+ `progress`（限时折扣倒计时 / 库存余量）+ `button`（购买）+ `a`（详情）。
  - 购买数量：`input[type=number]`（min/max/step）。
  - 确认弹窗：`div role=dialog` + `display` 切换（preview JS 模拟），含 `button`（确认/取消）+ `progress`（余额条）。
- **动效**：卡片 hover 抬升、弹窗缩放进入、倒计时条流动。

### 6.6 character.html — 角色 · 3D + 特效混合

- **定位**：验证 NativeHost 3D 渲染 + 粒子特效与 UI 混合渲染能力（旧 showcase page_nativehost 的延续）。
- **主标签**：`canvas` `progress` `ol` `li` `img` `p` `strong` `em` `span` `div` `button`。
- **区块**：
  - `canvas#native-slot`：NativeHost 3D 角色 slot。运行时渲染角色模型 + 粒子特效，UI 层叠加属性面板，验混合渲染。preview 渲染占位文本。
  - 属性条：`progress`（HP / MP / EXP）—— stat-bar 组件。
  - 技能列表：`ol` + `li`（有序，技能序号），每项 `img` 图标 + `p` 名称 + `span` 等级。
  - 装备槽：`img` 网格。
  - 角色信息：`p` / `strong` / `em`。
- **动效**：属性条充能动画、技能图标 hover 高亮、装备槽描边。

### 6.7 form.html — 角色创建 · 表单

- **定位**：表单控件编排（用 div + p 分组，替代 fieldset/legend）。
- **主标签**：`label` `input`（text/radio/range）`select` `option` `textarea` `button` `div` `p` `span`。
- **区块**：
  - 基本信息：`label` + `input[type=text]`（角色名）。
  - 职业/出身：`label` + `select`/`option`。
  - 性别/阵营：`label` + `input[type=radio]`。
  - 初始属性：`label` + `input[type=range]`（力量/敏捷/智力分配）。
  - 背景故事：`label` + `textarea`（rows/cols/maxlength）。
  - 分组：`div` + `p`（组标题，替代 legend）。
  - 操作：`button`（创建 / 重置）。
- **动效**：输入聚焦边框高亮、提交按钮态切换、属性滑块联动数值。

### 6.8 lab.html — CSS 全属性标本馆

- **定位**：吃掉所有"场景里自然用不到"的围栏 CSS 与剩余标签。全覆盖兜底。
- **主标签**：`div` `p` `span` `strong` `em` `br` `slot`（+ custom-element `<specimen>` / `<my-card>`）。
- **分区**（每区一个 specimen 卡片，标题用 `<p>` + 大字号）：
  1. **flex 全参数**：direction（row/column）/ wrap / justify-content × align-items 矩阵 / gap / grow-shrink / order。小枚举全列值，大枚举取代表值 + 矩阵。
  2. **尺寸单位**：width/height/min-max 的 px / % / auto / aspect-ratio。
  3. **盒模型**：padding / margin 四边 + 简写。
  4. **边框**：border-color / border-radius（1–4 值） / border-image-slice 九宫格。
  5. **背景**：background-color / background-image / background-size（cover / contain / 100% / stretch） / background-clip:text 渐变字。
  6. **文本排版**：font-size / weight / family / line-height / letter-spacing / white-space / text-align（搬旧 page_text A1–A6：CJK 断行 / ASCII 按词 / 中英混排 / nowrap / 行高字距）。
  7. **文本特效**：text-shadow / -webkit-text-stroke / font-effect:glow/blur / 渐变字 clip:text（搬旧 page_text C1–C5）。
  8. **视觉变换**：transform（translate/rotate/scale）/ opacity / filter 全滤镜（grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia）/ box-shadow。
  9. **溢出**：overflow-x/y（scroll/auto/hidden）+ 嵌套滚动。
  - **custom-element + slot**：`<my-card>` 带 `<slot name="...">` 投影演示。
  - **ol 有序列表**：结构标签 `header`/`nav` 在各页已覆盖，这里补 ol 编号 specimen。
- **深度策略**：混合深度——小枚举（display: block/flex/none/inline）全列值；大枚举（justify-content 6 值 × align-items 5 值）取代表值 + 交叉矩阵，不做笛卡尔全展开。

---

## 7. 动效层（双层）

动效用**双层**建模：预览层验视觉，运行时层验精确。

### HTML 预览层（CSS 近似）

- 用 CSS `transition` / `@keyframes` 模拟，验证"过渡方向/节奏对不对"，不求逐曲线 ease 精确。
- 每页至少一种动效，覆盖四类：hover 反馈、入场/出场、状态切换（tab/弹窗/选中）、循环（脉冲/呼吸/流光）。

### 运行时层（C# 辅助代码，另立）

- TweenManager 单一时钟，逐曲线 ease、delay、kill。真实动效由运行时兑现，不在 HTML 镜像。
- C# 运行时辅助代码是配套交付物（见 §11），描述每页"运行时应有什么动效"，但实现不在本 showcase 范围。

### 各页动效清单（速查）

| 页 | hover | 入场 | 状态切换 | 循环 |
|---|---|---|---|---|
| home | 卡片缩放 | 序列淡入 | 主按钮态 | — |
| settings | tab 高亮 | panel 淡入 | tab/开关 | — |
| inventory | 格子高亮 | 列表滑入 | 选中脉冲 | 描边脉冲 |
| mail | 列表项 | 项滑入 | 正文切换 | 未读呼吸 |
| shop | 卡片抬升 | — | 弹窗缩放 | 倒计时流光 |
| character | 技能/装备高亮 | — | — | 属性条充能 |
| form | 聚焦边框 | — | 提交态 | — |
| lab | specimen hover | — | — | 渐变字流光 |

---

## 8. 旧 showcase 搬迁映射

读完全部旧页后的结论（旧页在 [showcase_project/showcase/](/F:/WorkSpace/projects/LoomGUI/showcase_project/showcase/)）：

| 旧页 | 处置 | 去向 |
|---|---|---|
| page_text A1–A6 基础排版 | 搬 | lab 分区6（文本排版） |
| page_text B1–B9 富文本 | 搬 | mail 正文 + character |
| page_text C1–C5 文本特效 | 搬 | lab 分区7（文本特效） |
| page_controls flex/视觉 | 散入 | lab flex/视觉变换区 |
| page_image bg-image/九宫/filter | 散入 | lab 背景/边框/视觉变换区 |
| page_scroll overflow 标本 | 散入 | lab 溢出区 |
| page_list 虚拟列表骨架 | 场景化 | inventory + mail ListView |
| page_controller tab/弹窗/嵌套 | 范式转换 | tablist→settings、dialog→shop、嵌套作用域→各组件（HTML 不复用，交互模式映射） |
| page_dyntree | 丢弃 | 全局 NodeId + CreatePanel 旧范式 |
| page_nativehost | 场景化 | character canvas（3D+特效混合） |
| page_tween | 范式转换 | 各页动效（CSS 近似 + 运行时 TweenManager） |
| page_interact | 丢弃 | 运行时事件，非设计期靶子 |
| tips_toast | 丢弃 | 运行时 overlay 叠加 |

> 文本页内容复用价值最高，直接搬；控制器页整页不复用 HTML，但其交互模式全映射到新标准。
> 搬迁方向：旧 `showcase_project/` → 新 `showcase/`；内容 + res 字体图标迁出后，旧 `showcase_project/` 整体删除。`.claude/`、`CLAUDE.md`、`loom.workspace.json` 不搬——后者由打包器 init 生成。

---

## 9. 覆盖矩阵

### 9.1 标签覆盖（23 + custom-element）

| 标签 | 覆盖页 |
|---|---|
| div | 全部 |
| header | home + 各页顶栏 |
| nav | home |
| p | 全部 |
| span | 全部 |
| strong | mail / character / inventory |
| em | mail / character |
| br | mail / lab |
| label | settings / form |
| button | home / settings / inventory / shop / form |
| a | home（nav 卡片）/ mail（链接） |
| img | home / inventory / shop / character |
| canvas | character |
| input[text] | settings / form |
| input[password] | settings |
| input[search] | settings |
| input[number] | shop / settings |
| input[range] | settings / form |
| input[checkbox] | settings |
| input[radio] | settings / form |
| textarea | form |
| select | settings / form |
| option | settings / form |
| progress | inventory / shop / character |
| ul | inventory / mail |
| ol | character / lab |
| li | inventory / mail / character |
| template | inventory / mail |
| slot | lab（custom-element 投影） |
| custom-element | lab / components（item-card 等） |

### 9.2 CSS 覆盖（围栏 §5.1 全白名单）

| CSS 分组 | 覆盖位置 |
|---|---|
| 尺寸 width/height/min/max | lab 分区2 + 各页布局 |
| display/flex 全参数 | lab 分区1 + 全部 flex 布局 |
| position absolute/relative + top/right/bottom/left | lab（绝对定位 specimen）+ 各页浮层 |
| padding/margin | lab 分区3 + 全部 |
| border-color/radius/border-image-slice | lab 分区4 |
| background-color/image/size/clip | lab 分区5 + 各页底图 |
| opacity/box-shadow/pointer-events/transform/filter | lab 分区8 |
| color/font-*/text-align/line-height/letter-spacing/white-space/text-shadow/-webkit-text-stroke/font-effect | lab 分区6/7 + 全部文本 |
| transition | 各页动效 |
| overflow-x/y | lab 分区9 + mail/inventory 滚动 |

> 验收：覆盖矩阵全绿 = 每个标签至少一页用到、每个 CSS 分组在 lab 有 specimen。围栏门 `cargo test -p loomgui_fence` 保持绿。

---

## 10. 资源

> design-systems 全部重写（Deep Ocean 新令牌，不搬旧版）；res 的字体/图标从旧 `showcase_project/` 迁出后旧目录删除。

### 图标

- **搬**旧 [showcase_project/res/icons/](/F:/WorkSpace/projects/LoomGUI/showcase_project/res/icons/)（box/cpu/eye/hand/home/image/layout-grid/list/palette/skin/type/zap/arrow-right 等）；迁出后旧目录删除。
- **扩充**：背包/商店/角色页复用同一套物品图标；并**新增更多游戏物品图标**（武器/防具/药水/材料/货币），用来压测打包器图集（atlas）自绘 + atlas.json 尺寸注入。目标：图标数量足够触发多页 atlas 打包，而不只是几张零散 PNG。

### 字体

搬旧 `showcase_project/res/fonts/`（LXGWWenKai 中文字体对齐默认字体；DejaVuSans / JetBrainsMono / PressStart2P 做标本馆字体族 specimen）；迁出后旧目录删除。

---

## 11. 交付物与验收

**交付物**

1. `showcase/` 工作区：8 页 HTML + 组件 + design-systems（全部重写）+ res（图标扩充）；`loom.workspace.json` 由打包器 init 生成。
2. `preview/` 预览辅助代码：preview-base.css + loom-preview.js + README.md。
3. **C# 运行时辅助代码**（另立，不在本 showcase 实现）：描述每页运行时应有什么交互/数据/动效，供 R2–R7 运行时冲刺时对照。

**验收**

- 围栏门通过：`cargo test -p loomgui_fence` 绿；`loom-pkg build showcase/` 零 diagnostic 打包成功。
- 浏览器可视：双击 home.html 能逐页浏览，视觉对齐 Deep Ocean 设计稿。
- 覆盖矩阵全绿：§9 标签 + CSS 覆盖无空白。
- HTML 零围栏外标签：脚本扫描 body，确认无 §1 禁用清单标签。

---

## 12. 不在范围

- 运行时类型化对象树 API（另起 spec）。
- C# 运行时辅助代码的实现（仅在此 spec 描述，实现属 R2–R7 工作）。
- 真实 3D/粒子渲染（canvas 仅占位，运行时兑现）。
- 虚拟列表 slot 复用 / 不等高补偿（运行时）。
- TweenManager 逐曲线动画（运行时，preview 仅 CSS 近似）。
- 围栏本身是否需要扩标签（若靶子证明 23 标签不够，单开设计决策，不在本 spec）。

---

## 附：决策记录（brainstorm 锁定）

1. 形式：6 场景页 + 1 标本馆（混合式）。
2. 视觉调性：Deep Ocean（深青底 + 琥珀点缀）。
3. 分辨率：1920×1080 横屏。
4. 预览：分离注入（head 引入，body 零侵入）。
5. 标本馆深度：混合（小枚举全值，大枚举代表值 + 矩阵）。
6. 动效：双层（CSS 预览近似 + 运行时 TweenManager）。
7. 围栏约束：严格只用当前 23 标签，禁用清单见 §1。
8. HTML 定位：展示优先，运行时行为由 C# 辅助代码兑现。
