# CSS 语义对齐设计（围栏一致性引导 + 缺失属性 + bug 清零）

## 背景

定位背包 showcase「topbar 只显示一条分割线」时，发现根因是 `pkg.bin` stale（GUI exe 过期）。
顺藤摸瓜暴露了一个系统性问题：**LoomGUI 围栏的 CSS 语义和标准 CSS 规范不对齐**，
导致「同一份 HTML，浏览器预览 ≠ 游戏运行时渲染」。`border-style` 缺失只是最先暴露的一个。

审计（见本文末附录）发现 4 个高严重度 + 7 个中严重度问题，根因归结为三类：
1. **默认值门控缺失**：CSS 靠默认值控制「画不画」（border-style:none 不渲染），
   LoomGUI 只看「值是否存在」(Option::Some)，不查语义门控。
2. **shorthand 元数据脱节**：`CSS_SHORTHANDS.expands_to` 是死元数据（从未被消费），
   shorthand 能否工作全靠 `apply_decl` 有没有 match 分支——`flex:1`、`background:red`
   注册了却坏掉。
3. **schema default 死元数据**：`CssPropSpec.default` 字段从没被 `ResolvedStyle::default()` 消费，
   两层各自硬编码，可能不一致。

本设计一次性解决这三类，并建立一个**通用机制**避免「发现一条补一条」。

## 核心设计原则

围栏对 CSS 属性的处理遵循清晰的**三分法**（这是本次确立的核心模型）：

| 分类 | 含义 | 遇到时的行为 | 例子 |
|------|------|-------------|------|
| **支持** | 在 `CSS_PROPS` 注册 + `apply_decl` 有实现 | 正常解析 | border-width、flex-direction |
| **围栏外（不支持）** | LoomGUI 不实现该属性 | **error**（FenceUnknownCssProp），message 内给替代方案引导 | box-sizing、visibility、z-index、cursor |
| **围栏内但用法致不一致** | 属性合法，但漏写/默认值冲突导致预览≠运行时 | **warning**，强调预览/运行时不一致 + 引导补全 | border-width+color 无 style、background-image 无 size |

**关键边界**（与 box-sizing 决策一致）：
- 围栏外属性 → **error**，不是 warning。符合 AGENTS.md「围栏外输入打包期报错，不静默降级」。
  AI 看到 error 知道该删（box-sizing:border-box 在 LoomGUI 是多余声明，删了行为不变——
  LoomGUI 本就固定 border-box）。
- 围栏内属性的不一致用法 → **warning**，引导补全到规范写法。
  不阻断打包，但明确告知「预览会与实际渲染不一致」。

**box-sizing 明确排除**：游戏 UI 场景 box-sizing content-box 无真实需求（CSS 默认 content-box
反直觉，Web 开发几乎都 reset 成 border-box；LoomGUI 固定 border-box 正是大家想要的）。
排到围栏外 → error。本轮不实现，不碰 layout 层（taffy）。

## 机制一：围栏一致性诊断

### 1.1 Warning 类（围栏内属性，漏写/默认值冲突）

新建一个打包期诊断 pass（`crates/fence/src/consistency_check.rs`），扫描每个元素的
resolved style，检测「合法但会导致预览≠运行时」的组合，发 warning。

**W1 — border 声明不完整**

检测：元素声明了 `border-width`（任一边 > 0）且**没有** `border-style`。

（注：不检测「仅有 border-color 无 width」——CSS 下无 width 本就不渲染，非不一致问题。
触发点是 width 存在会尝试画 border，此时缺 style 才导致预览≠运行时。）

warning 文本（强调预览/运行时不一致）：
```
border-width/border-color declared without border-style — CSS default
border-style:none renders NO border in the preview, but the intent (width+color
declared) suggests a visible border was wanted. Add `border-style:solid`
(or use the `border` shorthand: `border:2px solid <color>`) so preview and
runtime both render the border consistently.
```

**W2 — background-size 默认值冲突**

检测：元素声明了 `background-image`（非 none）但**没有** `background-size`。

warning 文本：
```
background-image declared without background-size — CSS default is `auto`
(image at natural size), but LoomGUI default is `stretch` (fill the box).
Preview ≠ runtime. Add an explicit `background-size` (e.g. cover/contain/stretch)
so both render identically.
```

### 1.2 Error 类（围栏外属性 / 不支持的值）

这部分**零新机制**——围栏外属性现在就已经报 `FenceUnknownCssProp` error。
本轮只做两件事：

**E1 — 优化 error message，内嵌替代方案引导**

现有 error message 只说「property X is not in the fence」。改为附带 LoomGUI 行为说明 +
替代方案。涉及围栏外属性（优先处理审计报告里的高频项）：

- `box-sizing` → 「LoomGUI uses border-box model exclusively (width includes padding+border).
  This declaration has no effect — remove it.」
- `visibility` → 「LoomGUI has no visibility:hidden. To hide: `display:none` (no layout space)
  or `opacity:0` (keeps space).」
- `z-index` → 「LoomGUI renders in DOM order, z-index has no effect. Reorder DOM siblings
  or use `position:absolute` to control stacking.」
- `cursor` / `outline` / `user-select` 等 → 通用 message「not supported by fence, remove.」

实现：`find_css_prop` / FenceUnknownCssProp 处增加一个「已知围栏外属性的引导文案表」
（或扩展现有 schema 加 `unsupported_hint` 字段）。

**E2 — flex-wrap: wrap-reverse 从 schema 删值**

现状：`flex-wrap` schema 允许 `["nowrap","wrap","wrap-reverse"]`，但 `apply_decl` 把
`wrap-reverse` 静默降级成 `NoWrap`（schema 接受却不真支持——违反围栏严格性）。

修复：从 `flex-wrap` 允许值删掉 `wrap-reverse`。之后写 `flex-wrap:wrap-reverse` →
`FenceBadCssValue` error，message 引导用 `wrap`。

## 机制二：缺失属性实现 + shorthand 展开

### 2.1 border-style 属性 + 运行时门控

**fence 层**：
- `CSS_PROPS` 新增 `border-style`（Keyword `["none","solid","dashed","dotted","double"]`，default `"none"`，inherited=false）。
- `parse_border_value`（mapping.rs:510）当前只返 `(width, color)`，**扩展返 `(width, style, color)`**
  ——`border` shorthand（:635）与单边 longhand（:540）都走它，改一处全链生效。

**core 层**：
- `ResolvedStyle` 新增 `border_style: BorderStyle`（enum：None/Solid/Dashed/Dotted/Double，default=None）。
  字段序列化进 pkg.bin（version 不必 bump——ResolvedStyle 走 bincode，新字段 Option/enum
  默认值向后兼容，但需确认 read 兼容性；若 bincode strict 则 bump pkg version 22→23）。
- `apply_decl` 新增 `"border-style"` match 分支 → 写 `border_style` 字段。
- `border` shorthand 展开时同时写 `border_style`。

**render 层（门控）**：
- `render/mod.rs:425` 画 border 的条件，从
  `if let Some(border_col) = n.style.border_color { ... widths > 0 ... }`
  改为**额外检查** `n.style.border_style != BorderStyle::None`。
- 即：只有 `border_style != None && border_color.is_some() && width > 0` 才画 border。
- 对齐 CSS 规范：border-style 默认 none → 不画。

**与 W1 闭环**：W1 warning 引导作者补 `border-style:solid`，补了之后门控放行、border 渲染、
预览=运行时。没补则 warning + 运行时不画（对齐 CSS 规范 + 对齐预览）。

### 2.2 flex shorthand 展开

现状：`CSS_SHORTHANDS` 注册了 `flex`（expands_to=[flex-grow,flex-shrink,flex-basis]），
但 `apply_decl` 无 `"flex"` 分支 → `flex:1` 报 `FenceBadCssValue` 假阳性。

修复：`apply_decl` 新增 `"flex"` 分支，按 CSS 规范展开：
- `flex: <grow>` → grow=given, shrink=1, basis=0%
- `flex: <grow> <shrink>` → grow, shrink given, basis=0%
- `flex: <grow> <shrink> <basis>` → 全设
- `flex: none` → grow=0, shrink=0, basis=auto
- `flex: initial` → grow=0, shrink=1, basis=auto（即各 longhand 的 initial）
- 单值若是合法 length（如 `flex: 100px`）→ grow=1, shrink=1, basis=100px

注：`expands_to` 死元数据可顺手清理（或保留作文档，加注释说明 apply_decl 直接处理）。

### 2.3 background shorthand 展开纯色/图片

现状：`apply_decl` 的 `"background"` 分支只识别 `linear-gradient(...)`。
`background: red` / `background: url(x)` 报 `FenceBadCssValue`。

修复：扩展 `"background"` 分支：
- 若值是合法颜色 → 写 `background_color`（等价 `background-color`）
- 若值是 `url(...)` → 写 `background_image`
- 若值是 `linear-gradient(...)` → 维持现状（2 色渐变）
- 其它 → false（FenceBadCssValue）

注：不实现完整 CSS `background` 多层语法（游戏 UI 用不到），单层即可。

## 机制三：防漂移门 + bug 清零

### 3.1 default 一致性测试锁

现状：`CssPropSpec.default`（schema 表）与 `ResolvedStyle::default()`（core）各自硬编码，
无同步保证，可能漂移（如 border-color schema 写 `transparent`，resolved.rs 可能是 None）。

修复：新增测试 `schema_default_matches_resolved_default`（fence crate 测试，防漂移门风格）：
- 遍历 `CSS_PROPS`，对每个有对应 ResolvedStyle 字段的属性，断言 schema default 字符串解析后
  == ResolvedStyle::default() 该字段的值。
- 不一致 → 测试失败，指向具体属性。
- 这是**单一真相源的弱保证**：不强行让一层派生自另一层（避免运行时解析字符串的丑陋），
  而是用测试锁两层数据必须一致。

### 3.2 假阳性诊断修复（align-content / row-gap / column-gap）

现状：`CSS_PROPS` 注册了 `align-content`、`row-gap`、`column-gap`，schema 校验通过，
但 `apply_decl` 无对应分支 → 返回 false → `FenceBadCssValue` 假阳性（围栏说支持却不支持）。

修复（实现，而非删注册——因为 `gap` shorthand 已支持，补 longhand 成本低且语义完整）：
- `apply_decl` 新增 `"align-content"` 分支 → 写 `taffy_style.align_content`（复用现有 `parse_justify` 函数，
  返回 `taffy::AlignContent`）。已确认 taffy Style 有 `align_content: Option<AlignContent>` 字段。
- `apply_decl` 新增 `"row-gap"` / `"column-gap"` 分支 → 写 `taffy_style.gap.row` / `.column`
  （复用 `gap` shorthand 已有的解析路径）。
- `ResolvedStyle` 无需新增字段（走 taffy_style）。

### 3.3 border-color 默认值对齐

现状：schema `border-color` default = `"transparent"`，但 CSS 规范 initial = `currentColor`
（边框色默认 = 元素 color 值）。

决策：**保持 transparent，不改成 currentColor**。理由：
- 游戏 UI 场景 currentColor 几乎无需求（边框色几乎都显式声明）。
- 改成 currentColor 需在 resolve 时注入「取本节点 color」逻辑，增加复杂度。
- W1 检测覆盖了「border-width 有、border-color 无」的情况，会引导补 border-color。
- schema default `transparent` 与 resolved.rs 保持一致即可（机制 3.1 锁住）。

## 实现顺序（建议）

1. **机制二 2.1 border-style 门控**（解当前 showcase bug，建立门控先例）
2. **机制一 1.1 W1/W2 warning**（border 完整性 + background-size，配合 2.1 闭环）
3. **机制一 1.2 error 类**（message 优化 + wrap-reverse 删值，零风险）
4. **机制二 2.2/2.3 shorthand**（flex + background 展开）
5. **机制三**（一致性测试锁 + 假阳性修复）
6. **showcase HTML 扫查**：给所有真正想要边框的地方补 `border-style:solid`（W1 warning 会标出位置）。
7. **重打 pkg.bin + 重编 GUI exe**（坑 158：fence 改动后必须重编 GUI exe）。

每步 TDD：先写失败测试锁住正确行为（含围栏门 `cargo test -p loomgui_fence`）。

## 测试策略

- **fence crate 围栏门**：每个新 warning/error 用 `cargo test -p loomgui_fence` 锁住
  （诊断的 code + severity + message 关键词）。
- **core TDD**：border-style 门控、flex/background shorthand 展开各一个单测（先红后绿）。
- **机制 3.1 一致性锁**：跑一次确认现状（可能先红，暴露已有的 default 漂移），再修齐。
- **showcase 回归**：改完后重打 pkg，Unity PlayMode 验证 topbar/选中框/描边等 border 视觉
  与浏览器预览一致。

## 不做（明确排除）

- **box-sizing 支持**：排围栏外（error）。游戏 UI 无 content-box 需求，不碰 layout 层。
- **position static 默认值**：taffy 无 Static 概念，改默认值风险高、收益低（游戏 UI 多显式声明
  定位容器），留 roadmap。
- **完整 CSS `background` 多层语法 / `font` shorthand**：游戏 UI 用不到。
- **outline / text-decoration / text-overflow / object-fit 实现**：明确不支持，围栏 error 引导。
- **virtual list / 视口剔除**：与本设计无关，留复合束 ListView。

## 附录：审计报告摘要（2026-07-26）

围栏 CSS 语义对齐审计发现的问题及本设计的处理：

| 问题 | 严重度 | 处理 |
|------|--------|------|
| border-style 缺失（门控） | 高 | 机制二 2.1 实现 + 机制一 W1 warning |
| flex shorthand 损坏 | 高 | 机制二 2.2 展开 |
| background shorthand 不支持纯色 | 高 | 机制二 2.3 展开 |
| box-sizing 缺失 | 高 | 排围栏外（error），不实现 |
| position 默认 relative 非 static | 高 | 不做（留 roadmap，风险高收益低） |
| border-color 默认非 currentColor | 中 | 保持 transparent + 机制三一致性锁 + W1 引导 |
| background-size 默认 Stretch 非 auto | 中 | 机制一 W2 warning |
| visibility 缺失 | 中 | 围栏外 error（message 引导） |
| z-index 缺失 | 中 | 围栏外 error（message 引导） |
| align-content/row-gap/column-gap 假阳性 | 中 | 机制三 3.2 实现 |
| flex-wrap:wrap-reverse 静默降级 | 中 | 机制一 E2 删 schema 值（error） |
| cursor/outline/text-decoration 等缺失 | 低 | 围栏外 error（message 引导） |
