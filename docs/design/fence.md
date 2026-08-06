# LoomGUI 围栏（Fence）权威清单

> **单一真相源**：`crates/fence/src/schema/` 下的 Rust const 注册表（machine-readable）。本文档为人类可读副本，以代码为准。
>
> **防漂移门**：`cargo test -p loomgui_fence`——改围栏后必跑。

---

## 1. 设计哲学

### 1.1 标准 HTML 语义 + AI 强先验

围栏是一个面向游戏 UI、能够完整兑现语义的标准 HTML/CSS 子集。所有围栏决策的第一判据：AI 读 HTML 能否正确预测渲染结果。

AI 对标准 HTML/CSS 有海量训练数据先验。因此围栏只用标准 HTML 元素和 CSS 属性，不自创框架标签（如 `<scroll-view>`）——已有的标准能力（如 `overflow`）不自定义标签重复发明。

### 1.2 标签决定类型，CSS 赋予行为

这是围栏的核心原则：

- **标签 + 不可变结构属性决定稳定对象类型**。`<button>` 永远是 Button，`<div role="slider">` 永远是 Slider。
- **CSS（class、伪类、computed style）永远不改变对象类型**。`display:flex` 选择内部布局 Strategy，`overflow:auto` 选择滚动 Strategy。策略切换不重建节点、不丢状态。
- 这条原则让围栏设计在许多设计模式（Strategy、State）上自然落地，后续工作变得顺畅。

### 1.3 失败策略：明确报错，不静默降级

围栏外输入在打包期明确报错。围栏外 CSS 不静默忽略——写什么得到什么，围栏外即失败。

流水线一次性收集所有 diagnostic 再输出（collect-all），不 fail-fast。这对 AI 辅助创作至关重要：一轮修完所有错误比多轮对话高效得多。

---

## 2. 围栏元素

### 2.1 文档壳标签（8 个，不进运行时树）

| 标签 | 用途 |
|---|---|
| `html` | 文档根，解析时消费 |
| `head` | 元数据容器 |
| `body` | 内容根，子元素提升为模板根 |
| `title` | 文档标题 |
| `meta` | 元数据 |
| `style` | 内联 CSS（打包期消费） |
| `link` | 外部 CSS 引用（`rel=stylesheet`） |
| `script` | 脚本（围栏外，打包期报错或跳过） |

这些标签在 `tree_builder` 阶段被消费，不产生运行时对象。

### 2.2 运行时标签（6 个）

下表是完整的运行时标签注册表。控件与列表没有专属标签——作者在 `<div>` 上写 WAI-ARIA `role` 表达（spec §2.2），如 `<div role="slider">`、`<div role="list">`。列含义：

- **SemanticKind**：打包期标注的稳定语义类型（base 标签按 tag、控件/列表按 `role`）。
- **Display**：不写 CSS `display` 时的默认显示值。**注意：LoomGUI 运行时不实现 CSS inline flow**——`Inline` 标签运行时被当作 block-level flex（撑满父宽、竖向堆叠），与浏览器的横向 inline 行为不同。为防止 AI 先验错误，inline 布局 box（button/img）**必须放进 flex 容器**，不能裸放在 block 容器里，否则打包报错（见阶段 6.5）。文本级 `span` 豁免（其行内混排要等文本模型，roadmap §4）。
- **Category**：HTML 分类简化为四值——Block（块级结构）、Phrasing（行内文本级）、Void（自闭合）、Transparent（透明，继承父级）。
- **ContentModel**：允许的子内容——None（无子内容）、Text（仅文本）、Phrasing（行内元素+文本）、Flow（任意）、Transparent（继承父级）、Only([...])（仅列出的子标签）。

| 标签 | SemanticKind | Display | Category | ContentModel | Void |
|---|---|---|---|---|---|
| `div` | Container | Block | Block | Flow | |
| `span` | TextElement | Inline | Phrasing | Phrasing | |
| `button` | Button | Inline | Phrasing | Phrasing | |
| `img` | Image | Inline | Void | None | ✓ |
| `template` | Template | None | Phrasing | Flow | |
| `slot` | Slot | Inline | Transparent | Transparent | |

### 2.3 控件与列表：role 驱动

控件与列表没有专属标签。作者在 `<div>`（或其他 base 标签）上写 WAI-ARIA `role` 表达，打包期 `resolve_semantic` 把 `role` 映射到对应 `SemanticKind`：

| role | SemanticKind | 必需子结构（打包期校验，阶段 6.8） |
|---|---|---|
| `combobox` | Dropdown | `role=listbox` 子（内含 `role=option`） |
| `listbox` | Container | ≥1 `role=option` 子 |
| `option` | OptionItem | — |
| `slider` | Slider | `data-slot=thumb` 子 |
| `spinbutton` | NumberField | — |
| `switch` | Toggle | — |
| `radio` | RadioButton | — |
| `progressbar` | ProgressBar | `data-slot=fill` 子 |
| `textbox` | TextField（默认）/ TextArea（`aria-multiline=true`） | — |
| `list` | ListView | `role=listitem` 子（或 `template > role=listitem` 蓝图） |
| `listitem` | ListItem | — |
| `tablist` | TabList | `role=tab` 子（panel 靠 `aria-controls` 关联，非 role） |
| `tab` | Tab | — |
| `tabpanel` | Container | — |

控件初始值放 ARIA（`aria-valuenow`/`aria-checked`/...）或 `data-*`（`data-step`/`data-name`）属性里——围栏禁止 `<div>` 上出现 plain 控件属性。

### 2.4 自定义元素

标签名含 `-`（如 `<my-widget>`）识别为 CustomElement（`SemanticKind::CustomElement`）。围栏放行含 hyphen 的标签名通过 Fence Gate；注册验证（`customElements.define()` 注册表）defer 到 R3。

---

## 3. 稳定语义签名

对象类型由不可变签名决定，实例化后不能改变。

### 3.1 签名 = tag + role

Base 标签按 tag 映射；控件/列表按 `role` 映射（`role` 优先于 tag）。

| 签名 | SemanticKind |
|---|---|
| `div` | Container |
| `span` | TextElement |
| `button` | Button |
| `img` | Image |
| `div role=slider` | Slider |
| `div role=spinbutton` | NumberField |
| `div role=switch` | Toggle |
| `div role=radio` | RadioButton |
| `div role=textbox` | TextField |
| `div role=textbox aria-multiline=true` | TextArea |
| `div role=combobox` | Dropdown |
| `div role=option` | OptionItem |
| `div role=progressbar` | ProgressBar |
| `div role=list` | ListView |
| `div role=listitem` | ListItem |
| `div role=tablist` | TabList |
| `div role=tab`（或 `button role=tab`） | Tab |
| `div role=tabpanel` | Container（panel 靠 `aria-controls` 关联，非 role 分派） |
| `template` | Template |
| `slot` | Slot |
| `tag-name`（含 hyphen） | CustomElement |

`resolve_semantic(tag, role, aria_multiline)`：`role` 优先，未识别的 role 回退到 tag 映射。CSS（class/伪类/computed style）永远不改变 SemanticKind。

### 3.2 CSS 不改变类型

`display:block/flex/none` 选择布局 Strategy，`overflow:auto/scroll` 选择滚动 Strategy——都是行为切换，不重建节点、不丢状态、不改 SemanticKind。

---

## 4. 属性围栏

### 4.1 全局属性（所有元素接受）

| 属性 | 用途 |
|---|---|
| `id` | 组件作用域内唯一标识（打包期校验唯一性） |
| `class` | CSS 类选择器目标 |
| `style` | 行内 CSS（Fence Gate 校验属性名 + 关键字值） |
| `slot` | 投影到父组件的具名 slot |
| `hidden` | 隐藏元素 |
| `tabindex` | 焦点顺序 |
| `role` | WAI-ARIA 角色（白名单，影响复合控件语义） |
| `aria-*` | WAI-ARIA 状态/属性（打包期校验 IdRef 关系） |
| `data-*` | 自定义数据属性（透传，不做结构验证） |
| `--*` | CSS 自定义属性（透传） |

### 4.2 结构属性（影响类型/核心行为，Fence Gate 校验值域）

当前运行时标签无专属结构属性（控件语义由全局 `role` 驱动，见 §2.3）。

### 4.3 内容属性（初始值透传，Fence Gate 校验属性名）

| 元素 | 内容属性 |
|---|---|
| `img` | `src`, `alt`, `width`, `height` |
| `button` | `disabled` |
| `slot` | `name` |

控件初始值不走内容属性——role 驱动控件把初始值放 ARIA（`aria-valuenow`/`aria-checked`/...）或 `data-*`（`data-step`/`data-name`）里（见 §2.3）。

---

## 5. CSS 围栏

CSS 在围栏中以三个正交维度建模：

### 5.1 属性白名单（CssPropSpec）

每个 CSS 属性注册为一个 `CssPropSpec`，包含属性名、默认值、是否继承、值解析器类型。围栏只认注册表中的属性名，未注册属性名打包期报错。

当前注册的 CSS 属性按功能分组：

**尺寸**

`width`, `height`, `min-width`, `min-height`, `max-width`, `max-height` — 值域 Length / Percent / Auto。

**布局**

- `display`（`block` / `flex` / `none` / `inline`，**不含 `grid`**）
- `flex-direction`（默认 `row`——标准 CSS 默认），`flex-wrap`, `flex-grow`, `flex-shrink`, `flex-basis`, `gap`, `row-gap`, `column-gap`
- `justify-content`, `align-items`, `align-content`, `align-self`
- `order`, `aspect-ratio`

**定位**

- `position`（`absolute` / `relative`）
- `top`, `right`, `bottom`, `left`

**盒模型**

`padding-top/right/bottom/left`, `margin-top/right/bottom/left`

**边框**

`border-color`, `border-style`（`none` / `solid` / `dashed` / `dotted` / `double`，默认 `none`）, `border-radius`, `border-image-slice`

**背景**

`background-color`, `background-image`, `background-size`（`cover` / `contain` / `100%` / `stretch`）, `background-clip`, `-webkit-background-clip`

**视觉**

`opacity`, `box-shadow`, `pointer-events`, `transform`, `filter`

**文本**

`color`（继承）, `font-size`（继承）, `font-family`（继承）, `font-weight`（继承）, `text-align`（继承）, `line-height`（继承）, `letter-spacing`（继承）, `white-space`（继承）, `text-shadow`（继承）, `-webkit-text-stroke`（继承）, `font-effect`（继承，LoomGUI 私有扩展）, `transition`

**动画**

`animation`——`<name> <duration> [easing] [iteration-count|infinite] [fill-mode] [direction] [play-state] [delay]` 简写。对齐 public-api.md「动画定义全在 CSS」终态契约：fence 校验拼写错误并解析存值（逗号多声明 → 多个 `AnimationSpec` bake 进 `base_style.animation`，委托 core `parse_animation` 共用同一解析器防 spec §8.2/§8.3 语义漂移）。当前仅简写存在，标准 CSS 的 8 个长划子属性（`animation-name`/`animation-duration` 等）未加入；runtime 驱动（@keyframes 表查询 + KeyframePlayer 时间轴）**M2 已交付**（见 main-design §13）。`transition`——`<prop?> <dur> <ease?> <delay?>` 简写，逗号多 spec 解析存值（bake 进 `base_style.transition`，core transition 引擎消费）；ease 关键字按 spec §8.3 对齐（`ease`→CubicOut 等）。

`@keyframes <name> { <stop> { decls } ... }` at-rule——`<style>` 内定义命名关键帧。stop 选择器子集：`from` / `to` / `<N>%`（0..=100 整数）；逗号多 stop（`0%,100%{...}`）按 CSS 语义展开为多条 stop（共享同声明块）。其他 at-rule（`@media` / `@font-face` 等）不在围栏子集，整块丢弃 + 诊断。

`/* @loom-hook <name> */` 注释锦点——写在 keyframes 的 stop 声明块内或块间（如 `from{...}/* @loom-hook start */ to{...}` 或 `from{/* @loom-hook start */ ...} to{...}`），挂在该 stop 上。合法锦点注释保留为内部 marker 供 stop 解析，普通注释照常移除；纯文本 `@loom-hook`（非注释上下文）不识别。运行时 player 跨越该 stop 百分比时 emit `AnimationHookEvent`，C# 经 `Animation.OnHook(name)` 或 `On<AnimationHookEvent>` 路由（见 public-api §9.3）。

**:nth-child(An+B|odd|even|N) 选择器**——参数化伪类，`<style>` 规则选择器接受。括号内 An+B 语法（`2n+1`/`2n`/`odd`/`even`/`<N>`）解析为 `NthChildExpr{a,b}`，命中条件 = 子序号 i 满足 `i = a*k + b`（1-based）。常配合 `animation-delay` 实现错峰入场（同一规则按子序号算 delay，如 `.nav-card:nth-child(N){animation-delay:...}`）。语法越界（无括号/缺 `)`/坏参数）→ 选择器不匹配；组合子 `>` `+` `~` 仍越界（注意 `+`/`-` 在 `:nth-child(...)` 括号内是 An+B 合法语法，不判为组合子）。

> **⚠️ 虚拟化列表禁止 `:nth-child`**：虚拟化 `<ul>`（`role=list`）的 parked slot 留挂 ul 子树（`display:none`），按 CSS 仍计入 child count。`:nth-child` 的序数包含 parked slot → item 序号不可控。用 item-index / `data-*` 属性 + 属性选择器替代（如 `[data-index="0"]`）。详见 pool-slot-lifecycle design §2.9、§5.4。

**溢出**

`overflow-x`, `overflow-y`

### 5.2 值校验（CssValueParser）

每个属性的值通过对应的解析器校验。主要类型：

| 解析器 | 校验方式 |
|---|---|
| `Keyword([...])` | 必须是列出的关键字之一 |
| `Length` | 长度值（px） |
| `LengthPercent` | 长度或百分比 |
| `LengthPercentAuto` | 长度、百分比或 auto |
| `Color` | 颜色值（围栏仅校验属性名，值格式交 core `parse_color`：`#rgb`/`#rrggbb`/`#rrggbbaa` hex、`rgb()`/`rgba()` 函数式） |
| `Number` | 数字 |
| `Integer` | 整数 |
| `Overflow` | `visible` / `hidden` / `scroll` / `auto` |
| `Transform` | translate / rotate / scale |
| `Filter` | grayscale / brightness / contrast / saturate / hue-rotate / invert / sepia |
| `BoxShadow` | `ox oy [blur] [spread] color` |
| `TextShadow` | `ox oy [blur] color` |
| `Transition` | `property duration easing delay` 简写→`TransitionSpec`（逗号多 spec；ease 按 spec §8.3） |
| `Animation` | `<name> <duration> [easing/count/fill/direction/play-state/delay]` 简写→`AnimationSpec`（逗号多声明；ease 按 spec §8.3） |
| `Gradient2` | `linear-gradient(to dir, hex, hex)` |
| `TextEffect` | `glow(w color)` / `blur(w)` |
| `TextStroke` | `width color` |
| `BackgroundClipText` | `text` 触发渐变字形 |
| `Url` | `url("path")` |
| `BorderRadius` | 1-4 值 px/% + `/` 垂直值 |
| `FourSidedPx` | 1-4 值 px（九宫格等） |
| `FourSidedMargin` | 1-4 值 px/%/auto |
| `Raw` | 原样存储，不校验 |

关键字值校验在 `css_resolve` 阶段进行。非关键字值由 `apply_decl` 的值解析逻辑处理，解析失败也产生 `FenceBadCssValue` diagnostic。

### 5.3 简写展开（ShorthandSpec）

| 简写 | 展开为 | 展开方式 |
|---|---|---|
| `padding` | padding-top/right/bottom/left | Box（四边） |
| `margin` | margin-top/right/bottom/left | Box（四边） |
| `overflow` | overflow-x, overflow-y | Replicate（双轴同设） |
| `border` | border-color | BorderShorthand |
| `border-width` | — | Box |
| `border-top/right/bottom/left` | — | FallThrough（单边） |
| `background` | — | BackgroundShorthand |
| `flex` | flex-grow, flex-shrink, flex-basis | FallThrough |

---

## 6. 六阶段流水线（+ `<style>` 解析）

围栏验证是一条六阶段流水线，输入 HTML 字符串，输出 `ParsedTemplate`（IrTree + ResolvedStyle 数组 + Diagnostic 数组 + 引用精灵列表 + `dynamic_rules` 规则表 + `keyframes` 关键帧表）。

### 阶段 1+2：Tokenize + Tree Build

- html5gum 0.8 WHATWG tokenizer 词法分析。
- 构建中间表示 `IrTree`（元素节点 IrElement / 文本节点 Text / 注释 / Doctype）。
- 每个 IrNode 携带字节偏移 Span，用于后续 diagnostic 定位。
- 文档壳标签在此阶段被消费。

### 阶段 3：Fence Gate（逐元素校验）

对每个元素检查：

- **标签名**：不在注册表中、不是壳标签、不含 `-` → `FenceUnknownTag`
- **属性名**：全局属性放行；结构属性按值域校验（`FenceBadAttrValue`）；内容属性按白名单校验；未识别 → `FenceUnknownAttr`
- **行内 CSS 属性名**：不在 `CSS_PROPS` 且不在 `CSS_SHORTHANDS` → `FenceUnknownCssProp`

不做跨元素检查（那是阶段 5 的职责）。

### 阶段 4：CSS Resolve（行内样式解析）

对每个元素的 `style` 属性：

- 逐条声明解析 `prop: value`。
- 属性名校验（同阶段 3 但在 resolve 上下文）。
- 关键字值域校验（`Keyword([...])` 类型属性）。
- 值解析校验（`apply_decl` 返回 false 时报 `FenceBadCssValue`）。
- 应用 DisplayDefault（来自 schema）：Block → `DisplayMode::Block`，Inline → `DisplayMode::Flex` + `flex-direction:row`，None → `DisplayMode::None`。
- **`flex-direction` 默认 `row`**（标准 CSS 默认值）。如果未显式设置且 display 为 flex，强制覆盖为 Row。

产出 `Vec<ResolvedStyle>`（每节点一个，按 node-index 对齐）。

### 阶段 4.5：`<style>` 块解析

- 解析 `<style>` 标签的文本内容为 `DynamicRule` 选择器规则表（class/tag/id/后代空格/属性选择器/伪类 + specificity）。
- 解析 `@keyframes` at-rule 为 `KeyframesRule` 表（`from`/`to`/`N%` stop 选择器）。
- 产出存入 `ParsedTemplate.dynamic_rules` + `ParsedTemplate.keyframes`。
- 其他 at-rule（`@media` 等）丢弃 + 诊断。

### 阶段 5：Structural（跨元素结构校验）

- **Content Model**：子节点的 Category 必须被父节点的 ContentModel 允许。如 `<div>` 中放任何 Flow 内容合法，但 `<span>` 中放 `<div>`（Block inside Phrasing）报错。
- **文本内容**：ContentModel 为 `None` 或 `Only([...])` 的元素不接受文本子节点（空白文本节点跳过）。
- **ID 唯一性**：同一模板作用域内重复 `id` → `DuplicateId`。
- **Deferred 验证**（后续阶段新增）：ARIA 关系（`aria-controls` / `aria-labelledby` 的 IdRef 目标存在）。
  （`label[for]` 与 `ul/ol` 内 `template` 根校验随 `label`/`ul`/`ol`/`li` 标签下线而移除——列表结构契约改由阶段 6.8 按 `role` 校验。）

### 阶段 6：Annotate（语义类型填充）

对每个元素调用 `resolve_semantic(tag, role, aria_multiline)`，填充 `IrElement.semantic`。`role` 优先于 tag；这是确定性的：同样的 tag + role 永远产生同样的 SemanticKind。

### 阶段 6.5：inline 元素布局上下文检查

**根因**：taffy 0.12 不支持 CSS inline flow（inline 元素自动横排换行）。LoomGUI 只在一种上下文里让 inline 元素和浏览器一致：**flex 容器内**——inline 元素是 flex item，按 flex 规则排（两边行为相同）。

在 **block 容器**里（裸 `<div>` 等），LoomGUI 把 inline 标签当 block-level（撑满 + 竖排），和浏览器的 inline 行为（收缩 + 横排）必然不一致。放任这种写法会让 AI 按浏览器先验预期横排、运行时却竖排 → 渲染不可预测 → 返工。

**规则**：inline 布局 box（button/img）若**直接在 block 容器里**（parent 是 block、元素自己未显式 `display:block`）→ `FenceInlineElementInBlockContext` error，打包失败。错误信息教学两种改法（二选一）：
1. 父容器加 `display:flex`（多元素横排加 `flex-wrap:wrap`）。
2. 元素显式 `display:block`（有意当块级撑满）。

**豁免**：
- `span`（TextElement）/`slot`：文本级或结构占位，其行内混排要等文本模型（roadmap §4），不是 flex 能修的。
- 元素显式 `display:block`：作者有意当块级（撑满），浏览器也撑满，两边一致。
- parent 是 flex（inline style / tag 默认 / `<style>` class 规则声明 `display:flex`）。

**parent display 判定**：stage 4 css_resolve 只烘 inline style + tag 默认 display；`<style>` class 规则的 display 在 dynamic_rules（运行时 rematch）。检查合并两个来源判定 parent 是 block 还是 flex：class 匹配用单 compound 选择器（`.tab`/`button.tab`/`.btn.primary`）；多 compound（后代/子代）声明 flex 时保守放行（避免假阳性）。

### 阶段 6.7：控件 CSS 命中校验

**根因**：LoomGUI 控件（role 驱动：`progressbar`/`slider`/`switch`/`radio`/`textbox`/`spinbutton`/`combobox`）**不带 UA 默认样式**——core 刻意保持纯净，不开「框架自带样式源」先例。写了控件却没匹配的 CSS 规则 = 运行时渲染空白。浏览器会套自己的 UA 样式表，预览看着正常，打包进 LoomGUI 却空——作者无法从预览察觉。本检查在打包期拦下，明确告诉作者差异。

**规则**：受校验控件（`role` 在控件 role 白名单）若**无任何 `<style>` 规则的选择器命中它本身** → `FenceControlWithoutCss` error，打包失败。tag / class / id / 后代 / 属性选择器落地在该节点都算命中；伪类（`:hover` 等）不门控（带状态规则同样表明作者在样式控件）。**只有完全无命中才报错**。

**选择器匹配**：复用 stage 4.5 解析出的 `dynamic_rules`，按 tag/class/id/attr 字面对照 IrElement 判定（fence-local，不依赖运行时 Node）。后代选择器沿祖先链逐层尝试（fence 子集只有后代组合空格，拒 `>` `+` `~`）。

**教学文案**：指出控件无内置默认样式，再按 role 给出修复指引（`data-slot` 子节点型：progressbar/slider 引导为控件本身 + `data-slot=fill`/`thumb` 子配 CSS；switch/radio 引导 `[aria-checked]` 属性选择器；combobox 引导控件本身 + `role=listbox`/`role=option` 子；textbox/spinbutton 引导 background/border + caret-color）。

### 阶段 6.8：控件结构契约校验（必需子角色）

**根因**：role 化重构后（§2.2）控件结构由**作者写**，不再由框架运行时注入。作者可能漏写必需子角色（`<div role="slider">` 缺 `data-slot=thumb`、`<div role="combobox">` 缺 `role=listbox`）。把这种保证留到运行时 = 拿确定性换自由度，违背「围栏外输入打包期报错」的项目原则。本 pass 在 Annotate 之后严格拦截。

**规则**：带必需子角色的控件（见 §2.3 表）若**直接子节点**中缺对应 role / data-slot → `FenceMissingControlChild` error，打包失败。契约：

- `combobox` → 直接子含 `role=listbox`（listbox 再要求含 `role=option`，递归校验）
- `listbox` → 直接子含 ≥1 个 `role=option`
- `slider` → 直接子含 `data-slot=thumb`
- `progressbar` → 直接子含 `data-slot=fill`
- `list` → 直接子含 `role=listitem`
- `tablist` → 直接子含 `role=tab`（panel 靠 `aria-controls` 跨树关联，不在此校验）

`textbox`/`spinbutton`/`switch`/`radio`/`option`/`listitem`/`tab`/`tabpanel` 无必需子角色（不校验）。

**直接子字面**：校验只看**直接子节点**，与 §2.2 结构字面对齐——把必需子角色嵌进 wrapper div（如 `slider > div.wrap > data-slot=thumb`）不算满足契约，仍报 error。唯一例外是 **`list` 的 template 蓝图模式**：数据驱动 ListView 把 item 蓝图写在 `<template>` 子节点里（运行时克隆产 slot），`role=list > template > role=listitem` 视同满足 list→listitem 契约（template 的首个元素子节点被当成 listitem 检查）。

**教学文案**：诊断 message 按 role 给出作者应写的完整结构（如 combobox 引导 `role=listbox` 子 + `role=option` 孙；slider 引导 `data-slot=thumb` 子），取代旧 `.loom-*` 「照着填」的提示载体。

### 流水线特性

- **Collect-all**：所有阶段的 diagnostic 汇总到一个 `Vec<Diagnostic>` 输出，不 fail-fast。
- **Diagnostic 结构**：每条包含 severity（Error/Warning）、code（DiagnosticCode）、message、SourceLocation（文件名/行/列/源文本）、notes（Help/Note/Related）。
- **LineMap**：预计算的行偏移表，O(log n) 偏移到行列转换。

---

## 7. DiagnosticCode 完整清单

| Code | 含义 |
|---|---|
| `FenceUnknownTag` | 标签不在围栏注册表中 |
| `FenceUnknownAttr` | 属性不在元素的白名单中 |
| `FenceUnknownCssProp` | CSS 属性名不在围栏中 |
| `FenceBadCssValue` | CSS 值解析失败或不在允许的关键字域内 |
| `FenceBadAttrValue` | 结构属性值不在允许的枚举域内 |
| `DuplicateId` | 同一模板作用域内 ID 重复 |
| `UnclosedTag` | 标签未闭合 |
| `InvalidContentModel` | 子元素不满足父元素的 ContentModel |
| `UnregisteredCustomElement` | 自定义元素未注册（defer 到 R3） |
| `InvalidAriaRelation` | `aria-controls` / `aria-labelledby` 目标不存在 |
| `TokenizerError` | html5gum tokenizer 遇到无法恢复的词法错误 |
| `FenceInlineElementInBlockContext` | inline 布局 box（button/img）裸放在 block 容器里（非 flex）；LoomGUI 无 flex 之外的 inline flow，撑满竖排会和浏览器不一致 |
| `FenceBorderWithoutStyle` | **warning**：`border-width` 已声明但 `border-style` 缺省（CSS initial=none，浏览器不画边框，LoomGUI 会画）；预览 ≠ 运行时 |
| `FenceBgImageWithoutSize` | **warning**：`background-image` 已声明但 `background-size` 缺省（CSS 默认 auto=原始尺寸，LoomGUI 默认 stretch=拉伸填满）；预览 ≠ 运行时 |
| `FenceControlWithoutCss` | role 驱动控件（`progressbar`/`slider`/`switch`/`radio`/`textbox`/`spinbutton`/`combobox`）无任何 `<style>` 规则命中。控件不带 UA 默认样式，无 CSS = 运行时空白；须为控件及其 `data-slot` 子节点提供 CSS（详见阶段 6.7） |
| `FenceMissingControlChild` | role 驱动控件缺必需子角色/slot（`combobox` 缺 `role=listbox`、`listbox` 缺 `role=option`、`slider` 缺 `data-slot=thumb`、`progressbar` 缺 `data-slot=fill`、`list` 缺 `role=listitem`、`tablist` 缺 `role=tab`）。控件结构由作者写，漏写 = 运行时半残控件；详见阶段 6.8 |

---

## 8. 防漂移机制

### 8.1 单一真相源

围栏的所有规则以 `crates/fence/src/schema/` 下的 Rust const 表为唯一真相源：

- `tag.rs` → `TAGS`（6 运行时标签注册表）+ `SHELL_TAGS`（8 壳标签）
- `attr.rs` → 全局属性、结构属性、内容属性定义
- `css.rs` → `CSS_PROPS`（属性白名单）+ `CSS_SHORTHANDS`（简写展开）

解析器、打包器、文档、测试不得各维护一份白名单。

### 8.2 防漂移门

```bash
cargo test -p loomgui_fence                                # 全部围栏测试
cargo test -p loomgui_fence --test schema_contract         # schema 注册表契约
cargo test -p loomgui_fence --test doc_schema_sync         # 文档↔schema 交叉校验（防描述层漂移）
cargo test -p loomgui_fence --test pipeline_integration    # 端到端流水线
```

改围栏后必跑。测试 fail = 围栏契约被破坏。`doc_schema_sync` 从本文档主表解析标签清单与 schema 注册表比对，防止「代码改了文档没跟上」的描述层漂移。

### 8.3 消费者

本文档（`fence.md`）的消费者有三处，都引用本文为源：

| 消费者 | 位置 |
|---|---|
| 设计契约 | `docs/design/main-design.md` §3 |
| 设计师工作区 AI 规则 | 打包器模板 `workspace-CLAUDE.md` + `skill/SKILL.md` |
| 重构路线 | `docs/roadmap/roadmap.md` |

**同步规则**：改 schema 代码 → 检查三处消费者是否需同步。
