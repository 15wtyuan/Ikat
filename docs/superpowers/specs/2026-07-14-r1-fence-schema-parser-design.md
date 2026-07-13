# R1 围栏 Schema 与新解析器设计

> 日期：2026-07-14
>
> 状态：已完成逐节设计讨论，待书面 spec 审阅
>
> 范围：R1（围栏 schema + 新 HTML 解析器）
>
> 参考：`docs/superpowers/specs/2026-07-13-api-refactor-design.md`（API 重构总 spec）、`docs/design/main-design.md`、`docs/roadmap/roadmap.md` 2.2

## 1. 结论

R1 建立一份机器可读 schema 作为标签、属性、CSS 值和运行时类型映射的单一真相源，在其上重写 HTML 解析器（替换 scraper/html5ever），实现打包期围栏验证。旧 `FENCE_TAGS = [div,span,img,button]`、`display:block` RichText 暗号和 `apply_decl` 巨型 match 全部退役。

R1 只交付 schema + 解析器 + 验证器 + 契约测试。不实现类型化对象树（R2）、不实现 Package 格式升级（R3）、不实现控件行为（R5）。R1 的产物是一棵已验证、已标注 SemanticKind 的 IrNode 树——R2 消费它构建运行时对象。

## 2. 选型结论

### 2.1 Schema 表示：Rust const tables

Schema 用 Rust `const` 表达，编译期检查内部一致性。不引入外部数据文件（TOML/JSON），不引入代码生成步骤。

- 标签列表相对固定，值约束是 Rust 表达式而非纯数据，用 enum 表达最自然。
- 文档生成和 C# 绑定生成器已需读取 Rust（FFI 用 csbindgen），增加一个"导出 schema 为 JSON"的 xtask 命令是轻量的。
- 避免运行时解析 schema 文件、避免维护文件格式、避免增加解析依赖。

### 2.2 HTML 解析器：html5gum

调研了 Rust 生态所有主流 HTML 解析库，结论是 html5gum 最适合：

| 库 | 90 天下载 | 最后更新 | Span 支持 | 树构建 | 结论 |
|---|---|---|---|---|---|
| html5ever（当前） | 25M | 2026-03 | 差 | 内部，不可控 | 静默错误恢复与我们的原则冲突 |
| html5gum | 157K | 2026-06 | 优秀 | 我们实现 | WHATWG tokenizer + 自定义 emitter |
| tl | 1M | 2024-01 | 差 | 内部 | 停滞 2 年，为 scraper 设计 |
| html5tokenizer | 214 | 2023-09 | 有 | 仅 tokenizer | 停滞，采用率低 |
| lol_html | 1.2M | 2026-06 | 流式 | 流式 rewriter | 架构不匹配（流处理非树） |
| ludtwig-parser | 256 | 2026-04 | rowan | 手写 | HTML+Twig 利基；rowan 偏重 |

html5gum 是纯 WHATWG tokenizer（不是 tree builder）。它正确处理 HTML 词汇（实体解码、标签/属性词法、doctype），但把树构建留给我们。我们通过自定义 Emitter trait 实现构建自己的 IrNode 树。

- 内置 Span<usize>（byte offset start..end），所有 token 和属性携带源位置——诊断系统免费获得文件/行/列。
- emit_error 回调 + should_emit_errors 标志：html5gum 知道自己的解析错误，我们选择收集它们作为 Diagnostic 而非静默恢复。
- naive_next_state() 提供 content model 状态切换（script/textarea 等正确分词）。
- 我们不实现隐式闭合（标准 HTML 的 auto-close 规则），要求显式闭合标签。void 元素（img/input/br/meta/link）由 schema 声明，tree builder 查 schema 判断。

### 2.3 CSS 模式：属性定义 + 值解析器 + 简写展开（三正交维度）

调研了 RmlUi 和 Stylelint 的做法，两者指向同一模式：属性定义、值解析器、简写展开是三个正交维度，不混在一起。

RmlUi 的 PropertyDefinition 声明默认值、是否继承、是否强制布局重排、可挂载哪些解析器。解析器（PropertyParser）是独立可复用类型——keyword、length_percent、color、transform 等十几个。一个属性可挂载多个解析器，按序尝试。简写（ShorthandDefinition）单独建模，只声明"展开成哪些 longhand"。

我们当前的 apply_decl 600 行 match 把三件事混在一起（属性查找 + 值解析 + 字段写入）。R1 将其拆开。

## 3. Schema 结构

Schema 由四个注册表组成，全部是 Rust const 或 static。

### 3.1 标签注册表

```rust
/// 标签规格：标准 HTML 元素 -> 语义类型 + 显示默认 + 类别 + 内容模型
pub struct TagSpec {
    pub name: &'static str,
    pub semantic: SemanticKind,
    pub display: DisplayDefault,
    pub category: Category,
    pub content: ContentModel,
    pub void: bool,
    pub structural_attrs: &'static [AttrSpec],
    pub content_attrs: &'static [&'static str],
}
```

### 3.2 元素类别与内容模型（二维）

一个元素有两个独立的维度：

Category（参与类别）——这个元素能出现在什么上下文里：

```rust
pub enum Category {
    Void,         // img, input, br, meta, link
    Phrasing,     // span, strong, em, a, label, img, input, button...
    Block,        // div, header, nav, p, ul, ol, dialog...
    Transparent,  // a, slot——继承父元素类别
}
```

ContentModel（内容模型）——这个元素能装什么子节点：

```rust
pub enum ContentModel {
    None,        // 无子节点（void 元素）
    Text,        // 纯文本
    Phrasing,    // inline 内容：文本 + phrasing 元素
    Flow,        // 任意内容：phrasing + block + 文本
    Transparent, // 继承父元素内容模型
    Only(&'static [&'static str]),  // 只允许指定标签
}
```

验证算法：对每个子节点，查它的 Category，再查父节点的 ContentModel，判断是否允许：

```
父 ContentModel = Flow        -> 接受 Phrasing, Block, Void, Transparent（全部）
父 ContentModel = Phrasing    -> 接受 Phrasing, Void, Transparent；拒绝 Block
父 ContentModel = Text        -> 只接受文本节点，拒绝所有元素
父 ContentModel = None        -> 拒绝一切（连文本都拒绝）
父 ContentModel = Only(tags)  -> 只接受指定标签的元素
父 ContentModel = Transparent -> 向上找最近非 Transparent 祖先，用它的 ContentModel
```

### 3.3 围栏标签完整清单（24 个）

经过游戏 UI 上下文筛选，从标准 HTML ~45 个标签减到 30 个（含 7 个文档壳 + 23 个运行时标签）。砍掉的标签解决的问题在游戏 UI 里要么不存在（文档大纲、文章语义），要么已有更好手段覆盖（dialog->多窗口、details->button+class 切换、form->即时生效无 submit）。

| 类别 | 标签 |
|---|---|
| 文档壳 | html head body title meta style link |
| 结构容器 | div header nav |
| 文本 | span p strong em br |
| 关联文本 | label |
| 交互 | button a |
| 图像与绘 | img canvas |
| 输入 | input textarea select option |
| 状态反馈 | progress |
| 列表 | ul ol li template |
| 内容投影 | slot |

完整标签规格表（Category + ContentModel 映射）：

| 标签 | Category | ContentModel | SemanticKind | 说明 |
|---|---|---|---|---|
| html head | — | — | — | 文档壳，不进运行时树 |
| body | — | Flow | — | UI 内容根 |
| title | — | Text | — | 文档元数据 |
| meta link | Void | None | — | 文档元数据 |
| style | — | Text | — | 原始 CSS 源码 |
| div | Block | Flow | Container | 通用容器 |
| header nav | Block | Flow | Container | 语义结构容器 |
| p | Block | Phrasing | TextBlock | 段落 |
| span | Phrasing | Phrasing | TextElement | inline 文本容器 |
| strong em | Phrasing | Phrasing | TextElement | inline 强调 |
| br | Void | None | LineBreak | 换行 |
| label | Phrasing | Phrasing | Label | 表单关联 |
| button | Phrasing | Phrasing | Button | 按钮 |
| a | Transparent | Transparent | Link | 内联链接 |
| img | Void | None | Image | 图片 |
| canvas | Phrasing | Flow | Canvas | 绘图画布 |
| input | Void | None | InputDispatch | type 属性分发语义 |
| textarea | Phrasing | Text | TextArea | 多行输入 |
| select | Phrasing | Only(["option"]) | Dropdown | 下拉选择 |
| option | Block | Text | OptionItem | 选项条目 |
| progress | Phrasing | Phrasing | ProgressBar | 状态反馈 |
| ul ol | Block | Only(["li","template"]) | ListView | 列表 |
| li | Block | Flow | ListItem | 列表项 |
| template | Phrasing | Flow | Template | 惰性模板 |
| slot | Transparent | Transparent | Slot | 内容投影 |

### 3.4 结构验证规则（ContentModel 之外）

以下约束不走 ContentModel，是独立的结构验证规则：

- select 内只能有 option（ContentModel = Only(["option"]) 覆盖）。
- ul/ol 内只能有 li 或 template（ContentModel 覆盖）。
- label for="id" 的 for 属性指向的 ID 在当前组件作用域必须存在（Stage 5 验证）。
- aria-controls/aria-labelledby 指向的 ID 在当前组件作用域必须存在，且目标元素 role 与关系匹配（Stage 5 验证）。
- template 在 ListView 内的根必须是 li（Stage 5 验证）。
- Custom Element 名称必须包含 -（Stage 5 验证）。

### 3.5 SemanticKind 枚举

```rust
pub enum SemanticKind {
    Container,
    TextBlock,        // p
    TextElement,      // span, strong, em
    LineBreak,        // br
    Label,            // label
    Button,           // button
    Link,             // a
    Image,            // img
    Canvas,           // canvas
    InputDispatch,    // input（type 属性决定子类型）
    TextField,        // input[type=text/password/search]
    NumberField,      // input[type=number]
    Slider,           // input[type=range]
    Toggle,           // input[type=checkbox]
    RadioButton,      // input[type=radio]
    TextArea,         // textarea
    Dropdown,         // select
    OptionItem,       // option
    ProgressBar,      // progress
    ListView,         // ul, ol
    ListItem,         // li
    Template,         // template
    Slot,             // slot
    CustomElement,    // 用户自定义 Web Component（含 -）
}
```

InputDispatch 不是最终语义类型，是 input 标签在 type 属性解析前的占位。resolve_semantic(tag, structural_attrs) 函数完成最终分发：

```
resolve_semantic("input", {type: "range"})    -> Slider
resolve_semantic("input", {type: "checkbox"}) -> Toggle
resolve_semantic("input", {type: "radio"})    -> RadioButton
resolve_semantic("input", {type: "number"})   -> NumberField
resolve_semantic("input", {type: "text"})     -> TextField
resolve_semantic("input", {type: "password"}) -> TextField
resolve_semantic("input", {type: "search"})   -> TextField
resolve_semantic("input", {})                 -> TextField（type 默认 text）
resolve_semantic("input", {type: "bogus"})    -> Diagnostic error
```

### 3.6 DisplayDefault

取代旧系统 ResolvedStyle::default() 里硬编码的 flex-direction: Column。

```rust
pub enum DisplayDefault {
    Block,    // div, header, nav, p, ul, ol, li, template, progress
    Inline,   // span, strong, em, br, label, button, a, img, input, canvas, textarea, select, option
    None,     // template（不进实时树，但 template 本身是 Phrasing）
}
```

CSS display 声明可覆盖 DisplayDefault，但只改变布局策略，不改变 SemanticKind：

- display:block -> Block 布局策略
- display:flex -> Flex 布局策略（默认 flex-direction:row，标准 CSS）
- display:none -> 不渲染
- display:grid -> 围栏外，打包期报错（不降级为 Flex）
- display:inline -> inline 布局策略

纵向堆叠需要显式声明 display:flex; flex-direction:column。旧"div 永远 flex column"铁律彻底废除。

## 4. 属性注册表

属性分三类。

### 4.1 结构属性（structural）

影响对象的稳定类型或建立不可变结构关系。打包期必须验证，不能通过运行时 API 动态改变。

```rust
pub struct AttrSpec {
    pub name: &'static str,
    pub values: AttrValueDomain,
    pub required: bool,
}

pub enum AttrValueDomain {
    Enum(&'static [&'static str]),  // 固定枚举值
    IdRef,                          // ID 引用——打包期验证目标 ID 存在
    FreeText,                       // 自由文本
    Number,                         // 数值
}
```

结构属性按标签注册（见 3.1 TagSpec.structural_attrs）。示例：

input：
- type：Enum(["range","checkbox","radio","text","password","number","search"])，required=false（默认 text）

label：
- for：IdRef，required=false

a：
- href：FreeText，required=false（opaque 字符串，业务侧自己解释）

role（全局结构属性）：
- 值域为 WAI-ARIA 标准角色白名单（tablist、tab、tabpanel、tree、treeitem、dialog、region 等）
- 值域外报错
- role 可出现在任何元素上

aria-controls/aria-labelledby/aria-label（全局）：
- aria-controls/aria-labelledby 是 IdRef，Stage 5 验证目标 ID 存在且 role 匹配

### 4.2 内容属性（content）

提供初始值，不决定类型。R1 只验证属性名在白名单里（围栏外属性名报错），值透传到运行时。

input content_attrs：value、min、max、step、placeholder、readonly、disabled、checked、name、pattern、maxlength

progress content_attrs：value、max

img content_attrs：src、alt、width、height

### 4.3 全局属性（global）

所有元素都能用，不按标签注册：

| 属性 | 处理方式 |
|---|---|
| id | 标识符，Stage 5 验证 ID 唯一性 |
| class | 样式类，自由文本，CSS 解析 |
| style | 内联 CSS，CSS 值解析器验证 |
| slot | Web Components 内容投影 |
| hidden | 布尔属性 |
| tabindex | 焦点顺序 |
| role | ARIA 角色（结构属性，见 4.1） |
| aria-* | ARIA 属性集 |
| data-* | 透传，不验证 |
| --* | CSS 自定义属性，透传 |

## 5. CSS Schema

### 5.1 属性注册表

```rust
pub struct CssPropSpec {
    pub name: &'static str,        // "background-size"
    pub default: &'static str,     // "stretch"
    pub inherited: bool,            // color/font-* 继承
    pub parser: CssValueParser,     // 用哪个解析器验值
}
```

### 5.2 值解析器枚举

每个变体对应一个独立的解析函数，返回 Result<ParsedValue, Diagnostic>。这些函数复用现有 mapping.rs 里的自由函数（parse_four、parse_color、parse_overflow 等），提升为 schema 的一等公民。

```rust
pub enum CssValueParser {
    Keyword(&'static [&'static str]),  // ["cover", "contain", "100%"]
    Length,                              // px
    LengthPercent,                       // px | %
    LengthPercentAuto,                   // px | % | auto
    Color,                               // #rrggbb | #rgb
    Number,                              // f32
    Integer,                             // i32
    FourSidedPx,                         // 1-4 值 px 展开
    FourSidedMargin,                     // 1-4 值 px/%/auto
    BorderRadius,                        // 1-4 值 px/% + / 垂直值
    Transform,                           // translate/rotate/scale 函数链
    Overflow,                            // visible/hidden/scroll/auto
    Filter,                              // color filter 函数链
    BoxShadow,                           // ox oy [blur] [spread] color
    TextShadow,                          // ox oy [blur] color（逗号分隔多段）
    Transition,                          // prop duration ease delay
    Gradient2,                           // linear-gradient 2色4方向
    TextEffect,                          // font-effect: glow/blur
    TextStroke,                          // -webkit-text-stroke: w color
    BackgroundClipText,                  // background-clip: text
    Url,                                 // url("path")
    Raw,                                 // 原样存储（font-family）
}
```

### 5.3 简写注册表

```rust
pub struct ShorthandSpec {
    pub name: &'static str,
    pub expands_to: &'static [&'static str],
    pub kind: ShorthandKind,
}

pub enum ShorthandKind {
    Box,           // 1-4 值展开四向（margin、padding、border-width）
    Replicate,     // 双轴同值复制（overflow -> overflow-x, overflow-y）
    FallThrough,   // 依次尝试展开（border-top -> border-top-width + border-top-color）
    BorderShorthand, // border 简写特殊处理（width + color）
    BackgroundShorthand, // background 简写特殊处理（gradient or color）
}
```

### 5.4 验证策略

两层验证：

1. 属性名白名单：CSS 属性名不在 CSS_PROPS 注册表和 CSS_SHORTHANDS 注册表里 -> 围栏外，打包期报 Diagnostic。取代旧 _ => false 静默忽略。
2. 值验证：属性名在白名单里，但值不在解析器接受的集合内 -> 打包期报 Diagnostic。取代旧值不合法时返回 false 但不报错的静默行为。

不做元素级 CSS 属性约束（即不限制某 CSS 属性只能用在某类元素上）。原因：标准 CSS 本身不按元素限制属性，围栏是标准 CSS 子集，与标准一致更利于 AI 预测。元素级约束收益有限，复杂度高。

例外：display 和 overflow 的值影响布局策略选择（Block/Flex/Scroll）。这是 R2 的 CSS Behavior Strategy 消费的对象，R1 只确保值被正确解析和验证。

## 6. 解析中间表示（IrNode）

### 6.1 IR 结构

取代旧 ElementTree { ElementData }。文本不再是一个 Option 字段，而是一等子节点。

```rust
pub struct IrNode {
    pub kind: IrNodeKind,
    pub span: Span,              // byte offset start..end in source
    pub parent: Option<IrNodeId>,
    pub children: Vec<IrNodeId>,
}

pub enum IrNodeKind {
    Element(IrElement),
    Text(String),                // entity-decoded by html5gum
    Comment(String),             // preserved for diagnostics, dropped from runtime tree
    Doctype { force_quirks: bool },
}

pub struct IrElement {
    pub tag: String,             // lowercased, validated against schema
    pub attributes: Vec<IrAttribute>,  // ordered, not HashMap
    pub semantic: Option<SemanticKind>, // filled by Stage 6
}

pub struct IrAttribute {
    pub name: String,
    pub value: String,
    pub span: Span,
}
```

与旧 ElementData 的关键区别：

- 文本是 IrNodeKind::Text 类型的子节点，不是父元素的 Option 字段。pHello strong world /strong ! /p 解析为 p 的三个子节点：Text("Hello ")、Element(strong)、Text("!")。行内混排错误和 display:block RichText 暗号不再需要。
- 属性是有序 Vec<IrAttribute>，不是 HashMap。保留属性顺序（ARIA 关系、调试），不丢重复。
- 每个节点和属性携带 Span（源文件 byte offset）。诊断系统将 offset 转 file/line/column。
- raw_rich 和 rich_runs 字段删除。富文本是普通 HTML 子树。

### 6.2 IR 示例

p id="description" 内含 strong/img/a 的富文本段落，在 IR 树中展开为 p 的 6 个子节点（3 段 Text + strong + img + a），strong 和 a 各含 1 个 Text 子节点。文本与 inline 元素交错排列，每段文本因处于不同语义上下文而独立成节点。纯文本不拆。

### 6.3 文档壳处理

html/head/body/title/meta/style/link 允许作者写正常 HTML 文档壳，但只有 body 内的 UI 内容进入组件语义树。head 里的 style 和 link[rel=stylesheet] 被 CSS 解析器抽取为样式源。title/meta 是文档元数据，不进运行时。

## 7. 诊断系统

### 7.1 结构化诊断

取代旧 Err(String) 返回。

```rust
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub location: SourceLocation,
    pub notes: Vec<DiagnosticNote>,
}

pub struct SourceLocation {
    pub file: String,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub source_text: String,    // 该行源码原文，便于展示
}

pub struct DiagnosticNote {
    pub kind: NoteKind,
    pub text: String,
    pub location: Option<SourceLocation>,
}

pub enum Severity { Error, Warning }
pub enum NoteKind { Help, Note, Related }
```

### 7.2 收集策略

解析器和验证器收集所有诊断，不遇到第一个错误就停。用户一次打包看到所有问题，避免"修一个再发现下一个"的多轮循环。这对 AI 驱动的工作流尤其关键——AI 可以一轮内修完所有错误。

html5gum tokenizer 报的 tokenization error（无效实体、标签格式错误）也收集为 Diagnostic，不中断解析。

### 7.3 诊断码（DiagnosticCode）

```rust
pub enum DiagnosticCode {
    FenceUnknownTag,       // video 不在围栏内
    FenceUnknownAttr,      // 围栏外属性
    FenceUnknownCssProp,   // 围栏外 CSS 属性
    FenceBadCssValue,      // CSS 值不合法
    FenceBadAttrValue,     // 属性值不合法
    DuplicateId,           // ID 重复
    UnclosedTag,           // 标签未显式闭合
    InvalidContentModel,   // 子节点不满足内容模型
    InvalidIdRef,          // for/aria-controls 引用的 ID 不存在
    InvalidTemplateRoot,   // ListView 内 template 根不是 li
    UnregisteredCustomElement, // 未注册的 Custom Element
    InvalidAriaRelation,   // aria 关系不匹配
}
```

### 7.4 诊断示例

```
views/home.html:7:14  E_FENCE_TAG (error)
围栏外元素 video，不在支持的标准 HTML 子集内
  help: 如需播放动画，使用 canvas 或 img 序列帧
```

```
views/inventory.html:14:5  E_DUP_ID (error)
ID "title" 在当前模板作用域内重复定义
  note: "title" 已在 views/inventory.html:8:9 定义
```

### 7.5 Offset -> Line/Column 转换

在 parse 前对源文件做一次扫描，记录每行起始的 byte offset（行偏移表）。O(n) 一次，之后所有 diagnostic 的行号转换是 O(log rows) 二分查找。工具放在 IR 模块，打包器和验证器共用。

## 8. 解析管线（六阶段）

每个阶段是独立纯函数，输入上一阶段产物，输出下一阶段输入。每步可独立测试。

```
Stage 1: Tokenize    html 字符串 -> Token 流（html5gum）
Stage 2: Tree Build   Token 流 -> IrNode 树（我们的 tree builder）
Stage 3: Fence Gate   IrNode 树 + 标签/属性 schema -> 单元素级验证
Stage 4: CSS Resolve  IrNode 树 + CSS 源 -> computed ResolvedStyle per node
Stage 5: Structural   IrNode 树 -> ID唯一性/ARIA/template/content-model 验证
Stage 6: Annotate     IrNode 树 + schema -> SemanticKind 标注
```

### 8.1 各阶段说明

Stage 1 — Tokenize：html5gum 将 HTML 字符串分词为 Token 流（StartTag、EndTag、String、Comment、Doctype、Error）。每个 Token 携带 Span。tokenizer 自身报的 Error 收集为 Diagnostic。

Stage 2 — Tree Build：我们的 tree builder 消费 Token 流，构建 IrNode 树。不实现隐式闭合——遇到未闭合标签报 Diagnostic。void 元素（schema 声明）不需要闭合标签。文档壳（html/head/body/title/meta/style/link）解析进树，但标记为"不进运行时"，后续阶段区分处理。

Stage 3 — Fence Gate：对每个元素节点，查标签注册表验证标签名合法性、查属性注册表验证属性名和值域合法性、查 CSS 注册表验证 CSS 属性名合法性。围栏外的标签/属性/CSS 属性 -> Diagnostic。此阶段只需当前节点和 schema，不需看其他节点。

Stage 4 — CSS Resolve：解析 CSS 源（style 和 link 抽取）、cascade（按 specificity 合并规则）、resolve（inline style 叠加 cascade 结果）。架构与现有 resolve_styles 一致，只改变 CSS 属性的验证方式（从 apply_decl match arm 改为查 CSS_PROPS 注册表 + 值解析器）。cascade 逻辑本身不变。

Stage 5 — Structural：验证跨元素的关系——ID 唯一性、ARIA 引用（aria-controls/aria-labelledby/for 指向的 ID 是否存在且 role 匹配）、template 根验证（ListView 内 template 根必须是 li）、content model 验证（子节点 Category 是否满足父节点 ContentModel）。此阶段只需 IrNode 树本身，不需 CSS 解析结果。

Stage 4 和 Stage 5 可并行执行——它们依赖不同，互不阻塞。

Stage 6 — Annotate：对每个元素节点，用 resolve_semantic(tag, structural_attrs) 计算 SemanticKind 并写入 IrElement.semantic。这是 R1 产物的最终形态——一棵已验证、已标注 SemanticKind 的 IrNode 树，附带 per-node 的 computed ResolvedStyle。R2 直接消费。

### 8.2 R1 产物

```rust
pub struct ParsedTemplate {
    pub tree: IrNode,                    // 已验证、已标注 SemanticKind
    pub styles: Vec<ResolvedStyle>,      // per-node computed style（与 IrNode 同序）
    pub diagnostics: Vec<Diagnostic>,    // 所有诊断（空 = 成功）
    pub referenced_sprites: Vec<String>, // img src / background-image url 解析后的 sprite_key
}
```

## 9. 现有资产复用

以下资产的算法实现保留复用，只重写接口形状：

- CSS 值解析函数：parse_four、parse_color、parse_lp、parse_dimension、parse_transform、parse_overflow、parse_url、parse_filter、parse_border_value 等。这些自由函数保留，从 apply_decl 内部提取为 CssValueParser 的解析后端。
- CSS cascade：specificity 计算、继承、伪类匹配。架构不变。
- ResolvedStyle：字段不变（taffy_style + 视觉字段），但 DisplayMode 的默认值从 Flex 改为 schema 驱动的 DisplayDefault。
- img src 路径解析：resolve_img_src（workspace-root-relative sprite_key）不变。
- 行偏移表工具：新增，O(n) 扫描 + O(log) 二分。

以下旧代码退役：

- parse/dom.rs：parse_html、build_element、ElementTree、ElementData、FENCE_TAGS、is_inline_display_block、raw_rich/rich_runs 字段。
- style/mapping.rs：apply_decl 巨型 match 语句。值解析自由函数保留。
- style/resolved.rs：DisplayMode::default() = Flex 和 flex_direction: Column 硬编码。
- tests/fence_contract.rs：旧围栏契约测试。R1 用新 schema 驱动的契约测试替代。

## 10. 测试策略

### 10.1 契约测试分层

遵循 API 重构 spec 18 的分层：

1. Schema 契约：每个围栏内标签有正例（合法 HTML 片段 -> 解析成功），每类围栏外输入有反例（非法标签/属性/CSS -> Diagnostic 报错）。
2. Content model 契约：每对 Category x ContentModel 组合有正例和反例。
3. CSS 属性契约：每个 CssValueParser 变体有正例（合法值）和反例（非法值）。
4. 诊断契约：Diagnostic 携带正确的 file/line/column/code/message。

### 10.2 测试文件

- crates/core/tests/r1_schema_contract.rs：标签/CSS/属性 schema 契约。
- crates/core/tests/r1_content_model.rs：内容模型验证。
- crates/core/tests/r1_diagnostics.rs：诊断结构化输出。
- 单元测试内联在各模块。

## 11. R1 不做的事

- 不实现类型化运行时对象树（R2）。
- 不升级 Package 格式（R3）。
- 不实现控件行为（R5）。
- 不让旧 API 保持工作（开发期间无人使用）。
- 不实现 display:grid 布局（围栏外，报错）。
- 不做元素级 CSS 属性约束（标准 CSS 不按元素限制属性）。
- 不实现隐式闭合（标准 HTML 的 auto-close 规则）。

## 12. 已锁定决策

1. Schema 用 Rust const tables，不用外部数据文件。
2. HTML 解析器用 html5gum（WHATWG tokenizer），自己实现 tree builder。
3. 不实现隐式闭合，要求显式闭合标签。
4. CSS 模式三正交维度：属性定义 + 值解析器 + 简写展开。
5. 诊断收集所有错误，一次性报告，不遇到第一个就停。
6. 文本是一等子节点，不是父元素 Option 字段。
7. 围栏 30 个标签（7 文档壳 + 23 运行时），砍掉文档大纲/表单/弹窗/折叠等游戏 UI 不需要的语义。
8. 属性三层：结构属性（验证值域）+ 内容属性（透传）+ 全局属性。
9. div 默认 display:block，display:flex 默认 flex-direction:row。
10. display:grid 围栏外报错，不降级 Flex。
11. 不做元素级 CSS 属性约束。
12. Stage 4（CSS Resolve）和 Stage 5（Structural）可并行执行。
