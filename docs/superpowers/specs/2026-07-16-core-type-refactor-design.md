# Spec-2：core 类型化重构（档位2）

> **路线位置**：roadmap §2 ①（core 类型化重构）。Spec-1（阶段 S spike）已完成，
> 三假设证成、4 验收断言绿。本 spec = spike 产物的机械化升级：在旧 enum 上验证过的
> cascade 机制不变，只重做节点表示层。
>
> **范围**：NodeKind enum 扩容 + Node struct 拆分 + 数据进 side table +
> TemplateNode 跨包传输叶子数据 + set-ness 持久化 + pkg 格式升 v17。
>
> **不做**：打包编排（② Spec-3）、cascade 全量产品化（③ Spec-3）、后端对象层（④ Spec-4）、
> 控件状态表实现（控件束阶段）。摸黑骨架链（div + 文字 + 图 + flex + cascade）的
> 端到端验证在 Spec-3 ② 之后（① 阶段 HTML 还进不来）。

---

## 1. NodeKind enum 扩容

### 1.1 设计决策（已锁定）

- **全变体一次到位**：承载全部围栏语义（用户决策 A）。
- **纯标签变体（unit variant）**：enum 变体不承载数据，所有运行时可变数据进 side table（详见 §3）。
- **类型化用户表面留给 C# 投影层**：Rust 侧只需 enum + match，不上 trait object（roadmap §3.1 档位2）。

### 1.2 SemanticKind -> NodeKind 映射

fence 的 SemanticKind 24 种，减去 InputDispatch（annotate 阶段中间态，不进 IrTree）和
Template（打包期消费，不进实时 Scene 树），= 22 个 NodeKind 变体：

| SemanticKind | NodeKind | HTML 来源 | 公共类型 | 有 children |
|---|---|---|---|---|
| Container | Container | div/header/nav/main/section/article/aside/footer | Container | Yes |
| TextBlock | TextBlock | p/h1-h6 | TextBlock: Container | Yes |
| TextElement | TextElement | span/strong/em | TextElement: Container | Yes |
| LineBreak | LineBreak | br | —（内部叶子） | No |
| Label | Label | label | Label: Container | Yes |
| Button | Button | button | Button: Container | Yes |
| Link | Link | a | Link: Container | Yes |
| Image | Image | img | Image: Node | No |
| Canvas | Canvas | canvas | Canvas: Container | Yes |
| TextField | TextField | input[text/password/search] | TextField: Node | No |
| NumberField | NumberField | input[number] | NumberField: Node | No |
| Slider | Slider | input[range] | Slider: Node | No |
| Toggle | Toggle | input[checkbox] | Toggle: Node | No |
| RadioButton | RadioButton | input[radio] | RadioButton: Node | No |
| TextArea | TextArea | textarea | TextArea: Node | No |
| Dropdown | Dropdown | select | Dropdown: Node | No |
| OptionItem | OptionItem | option | —（Dropdown 内部子项） | No |
| ProgressBar | ProgressBar | progress | ProgressBar: Node | No |
| ListView | ListView | ul/ol | ListView: Container | Yes |
| ListItem | ListItem | li | ListItem: Container | Yes |
| Slot | Slot | slot | —（Custom Element 投影锚点） | Yes |
| CustomElement | CustomElement | my-widget（含 `-`） | Container | Yes |
| — | **TextNode** | DOM text node（IrNodeKind::Text） | TextNode: Node | No |

> **新增 TextNode**：不对应任何 SemanticKind（SemanticKind 只标 HTML 元素），对应 fence
> IrTree 的 `IrNodeKind::Text(String)`（裸文本节点）。它是旧 `Text { content }` 变体的
> 纯标签化替代——content 进 side table，变体变 unit。

### 1.3 映射规则

- `input[type]` 的 5 种 dispatch（TextField/NumberField/Slider/Toggle/RadioButton）是
  围栏 `resolve_semantic` 在 annotate 阶段就定好的，IrTree 里已经是具体 SemanticKind。
- `header/nav/main/section/article/aside/footer` 在 `resolve_semantic` 里全归 Container
  SemanticKind -> 全映射 Container NodeKind（它们是结构性容器，行为无差异）。
- `RichText` 变体退役（v1.7 富文本暗号，roadmap 退役清单）。功能由 TextBlock/TextElement +
  内部 inline formatting 替代（复合束文本模型，非 Spec-2 scope）。

### 1.4 旧变体退役

| 旧 NodeKind | 去向 |
|---|---|
| Container | 保留（unit variant） |
| Text { content } | -> TextNode（unit，content 进 §3.2 text_contents） |
| RichText { runs } | 删除（退役，复合束替代） |
| Image { src } | -> Image（unit，src 进 §3.2 image_srcs） |
| Button | 保留（unit variant） |

### 1.5 新 NodeKind 定义

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum NodeKind {
    // 结构容器
    #[default]
    Container,
    // 文本语义
    TextNode,      // DOM text node（裸文本叶子）
    TextBlock,     // p/h1-h6
    TextElement,   // span/strong/em
    LineBreak,     // br
    Label,         // label
    // 操作
    Button,
    Link,          // a
    // 叶子
    Image,
    // 控件叶子（私有内部结构，状态表在控件束实现）
    TextField,
    NumberField,
    Slider,
    Toggle,
    RadioButton,
    TextArea,
    Dropdown,
    OptionItem,
    ProgressBar,
    // 复合容器
    ListView,
    ListItem,
    // 组件系统
    Slot,
    CustomElement,
    // 引擎渲染挂载
    Canvas,
}
```

`#[derive(Copy)]`——因为全是 unit variant，NodeKind 从 `Clone`（有 payload 时必须）升级为
`Copy`（零成本拷贝）。match 分发不再需要引用或 clone。

---

impl NodeKind {
    /// Container content model: user-arrangeable children (div/button/a/p/span/ul/li/...).
    /// Used by layout (build taffy subtree) and render (batch DFS) to classify nodes.
    /// Single source of truth — adding a container variant only changes this method.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Self::Container
                | Self::TextBlock
                | Self::TextElement
                | Self::Label
                | Self::Button
                | Self::Link
                | Self::ListView
                | Self::ListItem
                | Self::Canvas
                | Self::Slot
                | Self::CustomElement
        )
    }

    /// Leaf: private internal structure, no user-arrangeable children.
    /// `!is_container()` — provided as a named method for readability at call sites.
    pub fn is_leaf(self) -> bool {
        !self.is_container()
    }

    /// Same as `is_container()` — semantic alias for "has children in content model".
    pub fn has_children(self) -> bool {
        self.is_container()
    }
}
```

### 1.6 谓词方法（reviewer 建议）

11 个容器变体 vs 11 个叶子变体的二元分类是 81 处 match 里最高频的模式
（layout 建不建 taffy 子树、render batch DFS 分流、build 遍不遍历 children）。
若每个站点都写 `Container | TextBlock | ... | CustomElement`，11 变体的 match arm 散在各处，
加一个新容器变体得改所有站点且漏一个只在那一处编译报错。谓词方法收成单一真相源：
改一处全改、加变体只改一个方法。

**不收谓词的场景**：dirty_text 门控（`matches!(k, NodeKind::TextNode)`）是具体变体行为
（只有 TextNode 需要文本重排版），不是容器/叶子分类——留 match arm。layout measure dispatch
（TextNode 查 text_contents、Image 查 image_srcs）也需具体变体 + side table 查表——留 match。

---

## 2. Node struct 拆分

### 2.1 拆分原则

按各 pass（process / rematch / solve / world_transforms / build）的访问模式分组，而非
"字段多就拆"。目标是热循环数据局部性：每帧三遍 DFS（§16），减少无关字段污染 cache line。

### 2.2 NodeFlags bitflags

4 个伪类源 bool -> bitflags（roadmap §3.1 明确要求）：

```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct NodeFlags: u8 {
        const HOVERED   = 1 << 0;
        const ACTIVE    = 1 << 1;
        const FOCUSED   = 1 << 2;
        const DISABLED  = 1 << 3;
        const CASCADED  = 1 << 4;  // cascaded_once 门控
    }
}
```

u8 足够（5 bit 用，留 3 bit 余量）。

### 2.3 NodeInteraction 子 struct

7 个交互态字段只有 process + rematch 碰，solve/world_transforms/build 零访问：

```rust
#[derive(Debug, Clone, Default)]
pub struct NodeInteraction {
    pub flags: NodeFlags,
    pub touchable: bool,
    pub draggable: bool,
    pub tabindex: Option<i32>,
}
```

旧 `cascaded_once: bool` 并入 flags 的 CASCADED bit。

### 2.4 拆分后 Node 形状

```rust
#[derive(Debug, Clone)]
pub struct Node {
    // ── 拓扑（每 pass 都读）──
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub kind: NodeKind,
    // ── 样式（cascade 原子单元，rematch + solve 高频）──
    pub style: ResolvedStyle,
    pub base_style: ResolvedStyle,
    pub classes: Vec<String>,
    pub id_attr: Option<String>,
    // ── 几何产物（layout 写，多 pass 读）──
    pub taffy_id: Option<taffy::NodeId>,
    pub layout_rect: Rect,
    pub clip_rect: Option<Rect>,
    // ── 交互态（仅 process + rematch 碰）──
    pub interaction: NodeInteraction,
    // ── 渲染脏（仅 build 碰）──
    pub dirty_mesh: bool,
    pub dirty_text: bool,
    pub reuse_key: u32,
    // ── 旧范式（退役中，控件束 WAI-ARIA 替代）──
    pub data_controller: Option<String>,
}
```

顶层从 23 个直接字段降到 15 个 + 1 个 interaction 子 struct。flags 从 5 个 bool 压成 1 字节。

### 2.5 有意不做

- **不拆成独立并行表（全 SoA）**。slotmap Key 64 bit + unsafe trait，SecondaryMap 不可行
  （node.rs 注释已论证）；style/geometry 是每节点必有 + 高频，内嵌 cache 优于 HashMap 查。
- **不拆 ResolvedStyle**。它是 cascade 原子单元，rematch 每帧全量读，拆了反而碎。
- **不拆拓扑/几何/渲染脏**。字段少（3-4 个），多一层间接不值。

---

## 3. Side table 组织 + 跨包传输

### 3.1 设计原则

旧 NodeKind 的叶子数据（Text content、Image src）从 enum payload 移出，进 Scene 持有的
稀疏表。模式统一：`HashMap<NodeId, T>`（和现有 anim/controllers 一致）。

### 3.2 跨包传输：TemplateNode 加叶子数据字段（🔴 硬伤修复）

**问题**：当前 content/src 全靠 `NodeKind::Text{content}` / `Image{src}` 的 payload 带
进 pkg.bin——`TemplateNode.kind: NodeKind` 整个序列化，payload 跟着进包。unit 化 NodeKind
后 payload 消失，content/src 无处存。而 **TemplateNode 才是跨 pkg.bin 序列化的那个**
（Node 运行时 struct 不进包）。instantiate 的 `create_node_from_template(scene, kind, style)`
只吃 kind，拿不出 content。

**修法**：TemplateNode 加两个叶子数据字段，序列化进包：

```rust
pub struct TemplateNode {
    // ... 既有字段（kind/style/parent_idx/classes/id_attr/draggable/tabindex/data_controller）...
    /// TextNode 的文本内容（仅 kind == TextNode 时 Some）。
    pub content: Option<String>,
    /// Image 的 src 路径（仅 kind == Image 时 Some）。
    pub src: Option<String>,
}
```

`Option<String>` bincode 序列化 = 1 byte tag（None/Some）+ 可选 string。大多数节点两个字段
都 None，每节点仅 +2 bytes 开销，可接受。

**instantiate 填充**：`Stage::instantiate` 循环 TemplateNode 建 live Node 时（stage.rs:597），
拿到 `create_node_from_template` 返回的 NodeId 后，事后填 side table：

```rust
let node_id = create_node_from_template(scene, tn.kind, tn.style.clone());
// 填叶子数据（unit 化后 content/src 不在 kind 里，从 TemplateNode 事后填）
if let Some(c) = &tn.content { scene.text_contents.insert(node_id, c.clone()); }
if let Some(s) = &tn.src     { scene.image_srcs.insert(node_id, s.clone()); }
// ... 既有 classes/id_attr/draggable/tabindex/data_controller 填充不变 ...
```

`create_node_from_template` 签名**不改**——保持 `(scene, kind, style) -> NodeId`。
content/src 由调用方（instantiate 循环）事后用返回的 NodeId 填 side table。

### 3.3 Spec-2 新增 side table（Scene 字段）

```rust
pub struct Scene {
    // ... 既有字段 ...
    /// TextNode 的文本内容（仅 TextNode 节点有条目）。
    pub text_contents: HashMap<NodeId, String>,
    /// Image 的 src 路径（仅 Image 节点有条目）。
    pub image_srcs: HashMap<NodeId, String>,
}
```

只有骨架链需要的两种（text_contents / image_srcs）在 Spec-2 实现。控件私有状态表
（slider value、dropdown selectedIndex、textfield value/光标）留到控件束阶段加——
Spec-2 只扩 enum 变体，不建控件状态表。

### 3.4 访问模式

- 读写：`scene.text_contents.get(&node_id)` / `.insert(node_id, content)`。
- 删节点联动：Scene::remove_node 时 `text_contents.remove(&id)` + `image_srcs.remove(&id)`
  （和 anim/controllers 同模式）。
- match 分发：`match node.kind { NodeKind::TextNode => { let c = &scene.text_contents[&node.id]; } }`。

### 3.5 数据不进 enum 的收益

- 改 content/src 不重建 NodeKind 变体（旧 `node.kind = NodeKind::Text{content:new}` ->
  `scene.text_contents.insert(id, new)`）。
- match 借用 `&node.kind` 是 Copy，不需要 clone。
- NodeKind 整个 enum 变 Copy（全 unit variant），热循环分发零 clone。

---

## 4. set-ness 持久化（Spec-2 前置）

### 4.1 问题回顾

spike 的 `InheritedSet(u16)` 是 dynamic.rs 的 transient local type——rematch 每帧实时算，
只在 dynamic cascade 写的属性上置 bit。打包期声明的可继承属性（baked 进 base_style）
拿不到 set-bit -> 生产环境继承 pass 会用父运行时值覆盖子节点的打包期声明值。

### 4.2 方案：inherited_set 进 ResolvedStyle

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InheritedSet(pub u16);

pub struct ResolvedStyle {
    // ... 既有字段 ...
    /// 可继承属性的显式声明标记（每 bit 对应一个可继承属性）。
    /// 打包期 bake：fence cascade -> apply_decl 时对可继承属性置对应 bit。
    /// 运行时 rematch：从 base_style.inherited_set 重起，动态规则额外 set 的继承属性合并。
    pub inherited_set: InheritedSet,
}
```

bit 映射沿用 spike 定义（dynamic.rs INH_* 常量）：

```rust
// 8 个可继承属性（与 spike 一致，fenced CSS 属性集可能后续扩展）
const INH_FONT_SIZE: u16       = 1 << 0;
const INH_COLOR: u16           = 1 << 1;
const INH_FONT_FAMILY: u16     = 1 << 2;
const INH_FONT_WEIGHT: u16     = 1 << 3;
const INH_TEXT_ALIGN: u16      = 1 << 4;
const INH_LINE_HEIGHT: u16     = 1 << 5;
const INH_LETTER_SPACING: u16  = 1 << 6;
const INH_WHITE_SPACE_NOWRAP: u16 = 1 << 7;
```

### 4.3 改动点

1. `InheritedSet` 从 dynamic.rs local struct 提升为 resolved.rs public type（+ Serialize/Deserialize）。
2. ResolvedStyle 加 `inherited_set` 字段，纳入 bincode 序列化（-> v17 形状变化）。
3. ResolvedStyle::default() 设 inherited_set = InheritedSet(0)（全 0 = 无显式声明）。
4. dynamic.rs 的 `rematch_pseudo_classes`：set_map 初始值从 `base_style.inherited_set` 读
   （而非 InheritedSet::default()），再 OR 动态规则额外 set 的 bit。
5. 现有 bincode roundtrip 测试覆盖新字段。

### 4.4 打包期 bake（本 spec 铺路，实际填在 ③）

Spec-2 只把字段加好、把 rematch 读取逻辑改对。打包期实际填 inherited_set 的代码在
③（cascade 收尾）或 ②（打包编排）时写——那时 fence 解析的 CSS 声明才会经 cascade
进 base_style。Spec-2 阶段 base_style 暂时全 default（inherited_set 全 0），不影响骨架链
测试（spike 已证明全 dynamic cascade 能跑通继承）。

---

## 5. pkg 格式升 v17

### 5.1 一刀切升级

`PKG_FORMAT_VERSION` 从 16 升到 17，`MIN == MAX == 17`（弃 v16，无迁移器，个人项目不兼容）。

> **Node 永远不进 pkg.bin**——进包的是 TemplateNode。Node struct 拆分
>（interaction 子 struct、flags 压缩）对 pkg 零影响，纯运行时改动。

v17 真正触发（均为 TemplateNode 序列化形状变化）：

1. **NodeKind enum 形状变**：5 变体 -> 22 unit variant + Copy。TemplateNode.kind 序列化
   形状变（旧 Text{content} 带 String payload，新 TextNode 是 unit 1 byte tag）。
2. **ResolvedStyle 加 inherited_set 字段**：bincode 布局变（TemplateNode.style + Node.style）。
3. **TemplateNode 新增 content/src 字段**：bincode 布局变（§3.2 硬伤修复带来的）。

三者叠加 = pkg.bin 不可读旧包，一刀切升 v17。

### 5.2 稳定性门

新增/更新 bincode 稳定性测试：
- NodeKind 全变体 roundtrip（确保 enum tag 映射稳定）。
- ResolvedStyle 含 inherited_set roundtrip（现有测试扩展）。
- NodeKind 序列化尺寸断言（unit variant = 1 byte tag）。
- TemplateNode 含 content/src roundtrip。

### 5.3 旧格式处理

- `loomgui_stage_load_html` / `loomgui_stage_set_rich_text` 等 P0 指出的过期 FFI 入口，
  在 Spec-2 里确认已从 FFI 导出删净（源码已删、dll 可能残留——Spec-2 前核实重编）。
- 旧 `showcase.pkg.bin` 已在 spike 阶段删除（commit ccfe800），无遗留。

---

## 6. 81 处 NodeKind:: match 迁移

### 6.1 影响面

20 文件，重灾 scene/dynamic.rs（28 处）、layout/mod.rs（16 处）。全 unit variant 后
编译器会报 non-exhaustive match，牵着逐处加分支。

### 6.2 迁移策略

- **容器类（Container/TextBlock/TextElement/Label/Button/Link/ListView/ListItem/Canvas/
  Slot/CustomElement）**：当前 `NodeKind::Container | Button` 的分支扩展到全容器类。
  大多数 layout/render 代码对容器类行为一致（建 taffy 节点、算几何、批合）。
- **叶子类（TextNode/Image/LineBreak/TextField/.../ProgressBar/Dropdown/OptionItem）**：
  当前 `NodeKind::Text | Image` 的分支扩展到全叶子类。控件叶子在 Spec-2 阶段行为与
  Image 类似（不建子 taffy 树、measure 或固定尺寸），但具体行为留到控件束实现。
- **TextNode**：替代旧 `Text { content }` 的所有匹配。content 从 destructure payload 改
  为查 `scene.text_contents[&node.id]`。

### 6.3 `children` 语义

`children: Vec<NodeId>` 留在 Node struct 上（不跟 kind 变体走）。"是否有 children"由 kind
语义决定（容器类有、叶子类无），建树/打包期验证叶子不收子节点。运行时不强制（避免每帧
检查开销），违约 = 打包期错误。

### 6.4 建树入口签名改动（🟡 硬伤配套）

content/src 不再在 NodeKind payload 里 -> 三个建树入口都要改传输通道：

**Scene::build entry tuple**（node.rs:305）：从 8-tuple 扩到 10-tuple：

```rust
pub fn build(entries: &[(
    Option<usize>,      // parent_idx
    NodeKind,
    ResolvedStyle,
    Vec<String>,        // classes
    Option<String>,     // id_attr
    bool,               // draggable
    Option<i32>,        // tabindex
    Option<String>,     // data_controller
    Option<String>,     // content  <- 新增（TextNode 文本）
    Option<String>,     // src      <- 新增（Image 路径）
)]) -> Scene
```

build 内部循环 insert 后，按 content/src 填 `scene.text_contents` / `scene.image_srcs`
（和 instantiate 事后填同模式）。spike mini-bridge（cascade_spike.rs 的 `SceneEntry` type alias）
跟着改。

**create_node_from_template**（dynamic.rs）：签名**不改**（保持 kind + style）。
content/src 由 instantiate 循环事后填（§3.2）。

**create_node**（dynamic.rs，runtime API / FFI 入口）：当前按 tag 字符串建节点
（"span" -> Text）。unit 化后 content 不进 kind，runtime 创建的 TextNode content 暂填空串
（`scene.text_contents.insert(id, String::new())`）。运行时改文本走后续 API（④ 后端对象层），
Spec-2 阶段 runtime create_node 不支持带初始 content（低频路径，可接受）。

---

## 7. 验收标准

### 7.1 编译通过

- `cargo build -p loomgui_core` + `cargo build -p loomgui_ffi_c`（FFI 不受影响——
  NodeId u32 ABI 不变，enum 扩变体不影响 C ABI）。
- `cargo build -p loomgui_pkg`（打包器——若它引用 NodeKind，需跟改）。

### 7.2 测试通过

- `cargo test -p loomgui_core`——现有测试全绿（81 处 match 迁移后）。
- `cargo test -p loomgui_fence`——spike 4 验收断言仍绿（mini-bridge 适配新 enum 后）。
- ResolvedStyle bincode roundtrip 含 inherited_set。
- NodeKind 全变体 bincode roundtrip + 尺寸断言。
- TemplateNode 含 content/src bincode roundtrip。

### 7.3 CI 门禁

- `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。
- feature-gate check（`--no-default-features --all-targets`）。

### 7.4 非验收（本 spec 不含）

- 端到端 HTML -> rect（那是 Spec-3 ② 之后，① 阶段 HTML 进不来 = "假绿"风险）。
- 控件行为（Slider/Toggle 等）——控件束阶段。
- SemanticKind -> NodeKind total 映射测试——桥 ② 实现（Spec-3）时加。

---

## 8. 范围外

| 项 | 归属 | 原因 |
|---|---|---|
| IrTree -> TemplateNode 桥 | Spec-3 ② | 需扩容后 enum 已到位 |
| 打包编排重建 | Spec-3 ② | 被删的 invoke fence -> write_package 编排 |
| cascade 全量产品化 | Spec-3 ③ | spike 产品接扩容后 enum |
| 后端对象层（C# 填充） | Spec-4 ④ | 依赖终点线1 |
| 控件状态表 | 控件束 | Spec-2 只扩 enum 变体 |
| RichText 退役功能替代 | 复合束文本模型 | display:block desugar 已退役 |
| data_controller 退役 | 控件束 WAI-ARIA | 不膨胀 Spec-2 范围 |

---

## 9. 实现顺序（建议）

1. **NodeFlags + InheritedSet 类型定义**（resolved.rs / node.rs）——零破坏。
2. **ResolvedStyle 加 inherited_set** + bincode roundtrip 测试——触发 v17 形状变化。
3. **NodeKind enum 扩容**（5 -> 22 unit variant）+ Default 改 Container——编译报错。
4. **Node struct 拆分**（interaction 子 struct + flags）——编译报错叠加（纯运行时，不影响 pkg）。
5. **TemplateNode 加 content/src 字段**（§3.2 硬伤修复）——序列化形状变。
6. **Scene 加 side table**（text_contents / image_srcs）+ build entry tuple 扩到 10-tuple。
7. **逐文件迁移 81 处 match**（编译器牵着走，从 dynamic.rs 开始）。Text{content} ->
   TextNode + `scene.text_contents[&id]` 查表。
8. **instantiate 循环填 side table**（stage.rs:597 -> 拿 NodeId 事后填 content/src，§3.2）。
9. **spike mini-bridge + 测试适配**（cascade_spike.rs 的 SceneEntry 扩到 10-tuple、Text{content} ->
   TextNode + side table）。
10. **cargo fmt + clippy + 全测试绿**。
11. **升 v17**（PKG_FORMAT_VERSION = 17，MIN=MAX=17）+ bincode 稳定性测试
    （NodeKind 全变体 / ResolvedStyle 含 inherited_set / TemplateNode 含 content-src）。
3. **NodeKind enum 扩容**（5 -> 22 unit variant）+ Default 改 Container + 谓词方法
   （`is_container()`/`is_leaf()`/`has_children()`，§1.6）——编译报错。
4. **Node struct 拆分**（interaction 子 struct + flags）——编译报错叠加（纯运行时，不影响 pkg）。
5. **TemplateNode 加 content/src 字段**（§3.2 硬伤修复）——序列化形状变。
6. **Scene 加 side table**（text_contents / image_srcs）+ build entry tuple 扩到 10-tuple。
7. **逐文件迁移 81 处 match**（编译器牵着走，从 dynamic.rs 开始）。
   容器/叶子分类用 `is_container()`/`is_leaf()` 谓词；Text{content} ->
   TextNode + `scene.text_contents[&id]` 查表。
8. **instantiate 循环填 side table**（stage.rs:597 -> 拿 NodeId 事后填 content/src，§3.2）。
9. **spike mini-bridge + 测试适配**（cascade_spike.rs 的 SceneEntry 扩到 10-tuple、Text{content} ->
   TextNode + side table）。
10. **cargo fmt + clippy + 全测试绿**。
11. **升 v17**（PKG_FORMAT_VERSION = 17，MIN=MAX=17）+ bincode 稳定性测试
    （NodeKind 全变体 / ResolvedStyle 含 inherited_set / TemplateNode 含 content-src）。

每步后 `cargo build -p loomgui_core` 验证编译，步骤 7 后 `cargo test` 验证全绿。
### 6.2 迁移策略

**优先用谓词方法处理容器/叶子二元分类**（§1.6）。现有最高频的模式是
`NodeKind::Container | Button => { ... }`（render batch DFS）——扩到全容器类时，
**直接换成 `node.kind.is_container()`**，不列 11 变体链。同理叶子分支用 `node.kind.is_leaf()`。
好处：加新容器变体只改 `is_container()` 一处，所有调用方自动跟上；不漏变体。

**具体变体行为仍走 match arm**（谓词不覆盖）：

- **dirty_text 门控**（dynamic.rs:84,143）：`matches!(k, NodeKind::Text { .. })` ->
  `matches!(k, NodeKind::TextNode)`。只有 TextNode 需要文本重排版，不是容器/叶子分类。
- **layout measure dispatch**（layout/mod.rs:152）：`match kind { Text{content} => ...,
  Image{src} => ..., _ => None }` -> `match kind { NodeKind::TextNode => { let c =
  &scene.text_contents[&node.id]; ... }, NodeKind::Image => { let s =
  &scene.image_srcs[&node.id]; ... }, _ => None }`。需具体变体 + side table 查表。
- **render mesh 生成**：Image 单纹理 quad、Container 纯背景——各变体的 payload 渲染逻辑，
  走 match arm 分发。

**控件叶子**（TextField/Slider/Toggle/.../ProgressBar）在 Spec-2 阶段行为与 Image 类似
（不建子 taffy 树、measure 或固定尺寸），但具体行为留到控件束实现。Spec-2 里它们落到
match 的 `_ => ...` 分支或 `is_leaf()` 后的默认叶子处理。
