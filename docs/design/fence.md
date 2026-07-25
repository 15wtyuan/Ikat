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

- **标签 + 不可变结构属性决定稳定对象类型**。`<input type="range">` 永远是 Slider，`<button>` 永远是 Button。
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

### 2.2 运行时标签（23 个）

下表是完整的运行时标签注册表。列含义：

- **SemanticKind**：打包期标注的稳定语义类型。`InputDispatch` 表示需根据 `type` 属性进一步分派。
- **Display**：不写 CSS `display` 时的默认显示值。**注意：LoomGUI 运行时不实现 CSS inline flow**——`Inline` 标签运行时被当作 block-level flex（撑满父宽、竖向堆叠），与浏览器的横向 inline 行为不同。为防止 AI 先验错误，`Button`/`Link`/`Label` 三类布局 box 必须**显式声明 `display`**（inline style 或 class 规则均可），否则打包报错（见阶段 6.5）。其余 inline 标签豁免：`span/strong/em`（文本行内，终态 TextRun）、`input/select/textarea/img/canvas/progress`（叶子控件/媒体）、`br/slot`。
- **Category**：HTML 分类简化为四值——Block（块级结构）、Phrasing（行内文本级）、Void（自闭合）、Transparent（透明，继承父级）。
- **ContentModel**：允许的子内容——None（无子内容）、Text（仅文本）、Phrasing（行内元素+文本）、Flow（任意）、Transparent（继承父级）、Only([...])（仅列出的子标签）。

| 标签 | SemanticKind | Display | Category | ContentModel | Void |
|---|---|---|---|---|---|
| `div` | Container | Block | Block | Flow | |
| `header` | Container | Block | Block | Flow | |
| `nav` | Container | Block | Block | Flow | |
| `p` | TextBlock | Block | Block | Phrasing | |
| `span` | TextElement | Inline | Phrasing | Phrasing | |
| `strong` | TextElement | Inline | Phrasing | Phrasing | |
| `em` | TextElement | Inline | Phrasing | Phrasing | |
| `br` | LineBreak | Inline | Void | None | ✓ |
| `label` | Label | Inline | Phrasing | Phrasing | |
| `button` | Button | Inline | Phrasing | Phrasing | |
| `a` | Link | Inline | Transparent | Transparent | |
| `img` | Image | Inline | Void | None | ✓ |
| `canvas` | Canvas | Inline | Phrasing | Flow | |
| `input` | InputDispatch | Inline | Void | None | ✓ |
| `textarea` | TextArea | Inline | Phrasing | Text | |
| `select` | Dropdown | Inline | Phrasing | Only([`option`]) | |
| `option` | OptionItem | Block | Block | Text | |
| `progress` | ProgressBar | Inline | Phrasing | Phrasing | |
| `ul` | ListView | Block | Block | Only([`li`, `template`]) | |
| `ol` | ListView | Block | Block | Only([`li`, `template`]) | |
| `li` | ListItem | Block | Block | Flow | |
| `template` | Template | None | Phrasing | Flow | |
| `slot` | Slot | Inline | Transparent | Transparent | |

### 2.3 自定义元素

标签名含 `-`（如 `<my-widget>`）识别为 CustomElement（`SemanticKind::CustomElement`）。围栏放行含 hyphen 的标签名通过 Fence Gate；注册验证（`customElements.define()` 注册表）defer 到 R3。

---

## 3. 稳定语义签名

对象类型由不可变签名决定，实例化后不能改变。

### 3.1 签名 = tag + 不可变结构属性

| 签名 | SemanticKind |
|---|---|
| `div` / `header` / `nav` | Container |
| `p` | TextBlock |
| `span` / `strong` / `em` | TextElement |
| `br` | LineBreak |
| `label` | Label |
| `button` | Button |
| `a` | Link |
| `img` | Image |
| `canvas` | Canvas |
| `input[type=text]`（默认） | TextField |
| `input[type=password]` | PasswordField |
| `input[type=search]` | SearchField |
| `input[type=number]` | NumberField |
| `input[type=range]` | Slider |
| `input[type=checkbox]` | Toggle |
| `input[type=radio]` | RadioButton |
| `textarea` | TextArea |
| `select` | Dropdown |
| `option` | OptionItem |
| `progress` | ProgressBar |
| `ul` / `ol` | ListView |
| `li` | ListItem |
| `template` | Template |
| `slot` | Slot |
| `tag-name`（含 hyphen） | CustomElement |

`type`（input）是结构属性，在 Fence Gate 阶段校验取值，在 Annotate 阶段决定最终类型。

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

| 元素 | 属性 | 值域 | 必填 |
|---|---|---|---|
| `input` | `type` | `range` / `checkbox` / `radio` / `text` / `password` / `number` / `search` | 否（默认 `text`） |
| `label` | `for` | IdRef（指向同作用域内控件 ID） | 否 |
| `a` | `href` | FreeText（链接目标） | 否 |

### 4.3 内容属性（初始值透传，Fence Gate 校验属性名）

| 元素 | 内容属性 |
|---|---|
| `img` | `src`, `alt`, `width`, `height` |
| `canvas` | `width`, `height` |
| `input` | `value`, `min`, `max`, `step`, `placeholder`, `readonly`, `disabled`, `checked`, `name`, `pattern`, `maxlength` |
| `textarea` | `placeholder`, `readonly`, `disabled`, `name`, `rows`, `cols`, `maxlength` |
| `select` | `name`, `disabled` |
| `option` | `value`, `selected`, `disabled` |
| `progress` | `value`, `max` |
| `button` | `disabled` |
| `slot` | `name` |

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

`border-color`, `border-radius`, `border-image-slice`

**背景**

`background-color`, `background-image`, `background-size`（`cover` / `contain` / `100%` / `stretch`）, `background-clip`, `-webkit-background-clip`

**视觉**

`opacity`, `box-shadow`, `pointer-events`, `transform`, `filter`

**文本**

`color`（继承）, `font-size`（继承）, `font-family`（继承）, `font-weight`（继承）, `text-align`（继承）, `line-height`（继承）, `letter-spacing`（继承）, `white-space`（继承）, `text-shadow`（继承）, `-webkit-text-stroke`（继承）, `font-effect`（继承，LoomGUI 私有扩展）, `transition`

**动画**

`animation`——`<name> <duration> [easing] [iteration-count|infinite] [fill-mode] [direction] [play-state] [delay]` 简写。对齐 public-api.md「动画定义全在 CSS」终态契约：fence 接受语法 + 校验拼写错误。当前仅简写存在，标准 CSS 的 8 个长划子属性（`animation-name`/`animation-duration` 等）未加入；runtime 驱动（@keyframes 表查询 + tween 发射）在 §4 视觉束（v1.10）实现。`transition` 属性已注册但解析器为空壳（`CssValueParser::Transition` 未实现校验逻辑），接受任意值不报错但不生效。

`@keyframes <name> { <stop> { decls } ... }` at-rule——`<style>` 内定义命名关键帧。stop 选择器子集：`from` / `to` / `<N>%`（0..=100 整数）；逗号多 stop（`0%,100%{...}`）按 CSS 语义展开为多条 stop（共享同声明块）。其他 at-rule（`@media` / `@font-face` 等）不在围栏子集，整块丢弃 + 诊断。

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
| `Transition` | `property duration easing` |
| `Animation` | `<name> <duration> [easing/count/fill/direction/play-state/delay]` 简写——结构校验，runtime 驱动 §4 视觉束实现 |
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
- **Deferred 验证**（后续阶段新增）：ARIA 关系（`aria-controls` / `aria-labelledby` 的 IdRef 目标存在）、template 根（`<ul>`/`<ol>` 内 `<template>` 根必须是 `<li>`）、`label[for]` 目标存在。

### 阶段 6：Annotate（语义类型填充）

对每个元素调用 `resolve_semantic(tag, input_type)`，填充 `IrElement.semantic`。这是确定性的：同样的 tag + input[type] 永远产生同样的 SemanticKind。

### 阶段 6.5：inline 元素 display 声明检查

**根因**：taffy 0.12 不支持 CSS inline flow（inline 元素自动横排换行）。LoomGUI 把 inline 标签在布局流里当 block-level（撑满、竖排）——与 AI 的浏览器先验（inline 横排）冲突。放任裸 inline 元素会让 AI 按浏览器先验预期横排、运行时却竖排 → 渲染不可预测 → 返工。

**规则**：`Button`/`Link`/`Label` 三类 inline 标签（布局 box）必须**显式声明 `display`**，来源不限：
- inline `style="display:..."`，或
- 匹配的 `<style>` class 规则含 `display` 声明。

两者任一即可（声明了就说明作者有意确定布局策略）。都没声明 → `FenceInlineElementMissingDisplay` error，打包失败。

**豁免名单**（display 对它们无意义或另有终态处理）：
- `span/strong/em`（TextElement）：终态是 `<p>` 文本 block 内的 TextRun（main-design §10），用 display 约束它们是错的。
- `input/select/textarea/img/canvas/progress`（控件/叶子媒体）：自绘叶子，display 对布局流无意义。
- `br/slot`：无 box 概念。
- **文本上下文豁免**：祖先链含 `<p>`（TextBlock）的 inline 元素（如 `<p>...<a>点此</a>...</p>`）是文本行内混排的一员（终态走 LinkRun/TextRun），display 不适用。

**class 匹配简化**：仅判单 compound 选择器（`.tab`、`button.tab`、`.btn.primary`）是否命中元素的 class 列表；多 compound 选择器（后代/子代，如 `.parent button`）声明 display 时保守放行（不报 error，避免假阳性）。

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
| `InvalidIdRef` | `label[for]` 指向的 ID 不存在 |
| `InvalidTemplateRoot` | ListView 内 template 根不是 `<li>` |
| `UnregisteredCustomElement` | 自定义元素未注册（defer 到 R3） |
| `InvalidAriaRelation` | `aria-controls` / `aria-labelledby` 目标不存在 |
| `TokenizerError` | html5gum tokenizer 遇到无法恢复的词法错误 |
| `FenceInlineElementMissingDisplay` | inline 布局 box（Button/Link/Label）未显式声明 `display`（taffy 无 inline flow，裸的不可预测） |

---

## 8. 防漂移机制

### 8.1 单一真相源

围栏的所有规则以 `crates/fence/src/schema/` 下的 Rust const 表为唯一真相源：

- `tag.rs` → `TAGS`（23 运行时标签注册表）+ `SHELL_TAGS`
- `attr.rs` → 全局属性、结构属性、内容属性定义
- `css.rs` → `CSS_PROPS`（属性白名单）+ `CSS_SHORTHANDS`（简写展开）

解析器、打包器、文档、测试不得各维护一份白名单。

### 8.2 防漂移门

```bash
cargo test -p loomgui_fence                                # 全部围栏测试
cargo test -p loomgui_fence --test schema_contract         # schema 注册表契约
cargo test -p loomgui_fence --test pipeline_integration    # 端到端流水线
```

改围栏后必跑。测试 fail = 围栏契约被破坏。

### 8.3 消费者

本文档（`fence.md`）的消费者有三处，都引用本文为源：

| 消费者 | 位置 |
|---|---|
| 设计契约 | `docs/design/main-design.md` §3 |
| 设计师工作区 AI 规则 | 打包器模板 `workspace-CLAUDE.md` + `skill/SKILL.md` |
| 重构路线 | `docs/roadmap/roadmap.md` |

**同步规则**：改 schema 代码 → 检查三处消费者是否需同步。
