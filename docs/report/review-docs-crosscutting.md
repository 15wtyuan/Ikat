# LoomGUI 文档与代码跨模块一致性审查报告

> 审查日期：2026-07-09
> 审查范围：`main-design.md` / `fence.md` / `pitfalls.md` / `CLAUDE.md` vs 实际代码
> 审查方法：逐条 reads → 对照代码验证关键声明 → 跨文档交叉比对

---

## 1. 关键数据不匹配（严重）

### 1.1 SOA 列数：文档写 22 列，代码实际 20 列

**文档原文**：
- `main-design.md:566` — "当前 22 列 / blob v9"
- `CLAUDE.md:91` — "每帧 Rust 产出一个 SOA 公共头（渲染节点公共字段，当前 22 列）"

**代码现实**：
- `loomgui_ffi_c/src/blob.rs:11,18` — `const VERSION: u32 = 10; // v10：text 塌进 mesh_arena，删 text_off/text_len 列 + text_arena`，`// 列名 + 每元素字节数。v10：删 text_off/text_len 列（22→20 列）`
- `FrameBlob.cs:16` — "v10：text_arena 已删，列 text_off/text_len 删除（22→20 列）"
- `blob.rs:43` — `let num_col_offsets = columns.len(); // 20`

**不一致**：blob 已升到 v10，text_arena/text_off/text_len 已删除，列数从 22 缩为 20。两份文档仍在描述 v9 的 22 列。

**严重级别**：**严重**。FFI 契约的列数是跨语言边界 ABI 的核心参数，文档与实际不符会直接导致后端开发者误算 header offset、读串列。

---

### 1.2 Node 类型层级：main-design.md 列 16 种，实际定义 5 种

**文档原文**（`main-design.md:165-174`）：
```
Node (基类)
├── Container
│   ├── Button
│   ├── List (虚拟化滚动列表)
│   ├── ComboBox / Slider / ProgressBar / Tree
├── Image
├── Text
├── RichText / TextInput / Loader / MovieClip / Graph
└── NativeHost
```

**代码现实**（`loomgui_core/src/scene/node.rs:62-79`）：
```rust
pub enum NodeKind {
    #[default]
    Container,
    Text { content: String },
    RichText { runs: Vec<...> },
    Image { src: String },
    Button,
}
```

**不一致**：文档列了约 16 种类型（含 List、ComboBox、Slider、ProgressBar、Tree、TextInput、Loader、MovieClip、Graph、NativeHost），实际 enum 只有 5 个变体。文档在 §5.2 底部注"RichText/TextInput/List 等是内部 NodeKind，不暴露为 HTML 标签"，但 List/ComboBox/Slider 等**根本没有对应的 NodeKind 变体**——它们不在代码中。

**严重级别**：**严重**。新成员看文档会以为框架已实现 16 种控件类型，实际只有 5 种。这是设计与实现的巨大鸿沟。

---

### 1.3 Node struct 字段定义与实际代码完全不同

**文档原文**（`main-design.md:179-198`）：
```rust
struct Node {
    id: NodeId,
    parent: Option<NodeId>,
    transform: Transform2D,         // 不存在
    style_size: SizeStyle,          // 不存在（已合并进 ResolvedStyle）
    measured_size: (f32, f32),      // 不存在（在 layout_rect 里）
    layout_rect: Rect,              // 存在
    alpha: f32, visible: bool,      // 不存在（在 style 里派生）
    touchable: bool, grayed: bool,  // grayed 不存在
    color_tint: Color,              // 不存在（在 style.text_color）
    base_style: ResolvedStyle,      // 存在
    style: ResolvedStyle,           // 存在
    dirty: DirtyFlags,              // 不存在（改为 dirty_mesh + dirty_text bool）
    children: Option<Vec<NodeId>>,  // 存在但非 Option（就是 Vec）
    sorting_order: i32,             // 不存在（走 sort_key 全局表）
    clip_rect: Option<Rect>,        // 存在
    // gears/gear_locked/controller  // 不存在（Gear 已砍，Controller 在 Scene 级）
}
```

**代码现实**（`node.rs:89-139`）：
```rust
pub struct Node {
    pub id: NodeId,         // 存在
    pub parent: Option<NodeId>,  // 存在
    pub kind: NodeKind,     // 文档漏
    pub style: ResolvedStyle,    // 存在
    pub taffy_id: ...,      // 文档漏
    pub layout_rect: Rect,  // 存在
    pub clip_rect: Option<Rect>, // 存在
    pub children: Vec<NodeId>,   // 非 Option
    pub dirty_mesh: bool,   // 非 DirtyFlags
    pub dirty_text: bool,   // 非 DirtyFlags
    pub base_style: ResolvedStyle, // 存在
    pub classes: Vec<String>,     // 文档漏
    pub id_attr: Option<String>,  // 文档漏
    pub touchable: bool,    // 存在但不在 style 里
    pub hovered: bool,      // 文档漏
    pub active: bool,       // 文档漏
    pub disabled: bool,     // 文档漏
    pub draggable: bool,    // 文档漏
    pub tabindex: Option<i32>,    // 文档漏
    pub focused: bool,      // 文档漏
    pub reuse_key: u32,     // 文档漏
    pub data_controller: ...,     // 文档漏
    pub cascaded_once: bool,      // 文档漏
}
```

**不一致**：文档描述的 Node struct 字段与实际代码差异极大。`transform`/`style_size`/`measured_size`/`alpha`/`grayed`/`color_tint`/`DirtyFlags`/`sorting_order`/`gears` 在文档中存在但代码中不存在。`kind`/`taffy_id`/`classes`/`id_attr`/`hovered`/`active`/`disabled`/`draggable`/`tabindex`/`focused`/`reuse_key`/`data_controller`/`cascaded_once` 在代码中存在但文档中不存在。

**严重级别**：**严重**。Node 是框架的核心数据结构，文档与实际相差约 70% 的字段。新人看文档写的 Node 布局代码根本编不过。

---

### 1.4 tick 时序：main-design.md §14 与代码执行顺序矛盾

**文档原文**（`main-design.md:632-636`）：
```
c. layout dirty → taffy solve      ← 第3步
d. process 指针输入                 ← 第4步
e. ScrollPane 物理 + 消费 wheel    ← 第5步
f. process_keys                    ← 第6步
g. compute_world_transforms         ← 第7步
h. rematch_pseudo_classes           ← 第8步（在 compute 之后！）
```

**代码现实**（`stage.rs:716-793`）：
```
1. TweenManager.update
2. 消费 pending_focus_request
3. process（指针输入）              ← 3
4. scroll.update + wheel
5. process_keys
6. rematch_pseudo_classes           ← 6（在 solve 之前！）
7. transition drain
8. solve                            ← 8（在 rematch 之后！）
9. refresh_content_sizes
10. compute_world_transforms         ← 10（在 rematch 之后！）
11. build_render_nodes
```

**不一致**：文档把 `solve` 放在 `process` 之前（位置 3），把 `rematch` 放在 `compute` 之后（位置 8）；实际代码 `solve` 在 `rematch` 之后（位置 8）、`compute` 在 `rematch` 之后（位置 10）。这是**根本性的**顺序差异——`rematch` 在 `solve` 之前是整个架构的核心时序保证（伪类改布局属性当帧生效），文档的排序完全反了。

**严重级别**：**严重**。CLAUDE.md:82 正确描述了时序（"process → rematch → solve → refresh → compute → build"），`main-design.md` §14 是**旧时序**（坑 103 修之前的顺序），但文档未更新。两份重量级文档互相矛盾。

---

## 2. 文档内部矛盾

### 2.1 position:relative — main-design.md 说"非显式映射"，fence.md 说"实证已接受"

**main-design.md:86**：
```
`position:relative` 靠 taffy 默认 `Position::Relative` 生效
（非显式映射，写不写行为一致，无 inset 偏移）
```

**fence.md:71**：
```
| `position` | `relative`（默认）/`absolute`（v1.4-b 脱离流）... | mapping.rs:557-558 | 【实证】 |
```

**fence_contract.rs:189-195**：
```rust
fn position_relative_explicit() {
    let mut s = ResolvedStyle::default();
    s.taffy_style.position = taffy::style::Position::Absolute; // 先改成非默认
    assert!(apply_decl(&mut s, "position", "relative"));        // ← 返回 true！
    assert_eq!(s.taffy_style.position, taffy::style::Position::Relative);
}
```

**不一致**：main-design.md 说 `position:relative` 是"非显式映射"（靠 taffy 默认值生效），但 fence_contract.rs 测试证明 `apply_decl` 对 `position:relative` **返回 true**（显式接受）。v1.4-b 之后 position 已被显式加入围栏映射。main-design.md §3.2 的这段描述已过时。

**严重级别**：**中**。fence.md（人类可读围栏）和 fence_contract.rs（测试真相源）一致正确，只有 main-design.md §3.2 过时。但该节作为"设计哲学"被频繁引用。

### 2.2 pitfalls.md §1.1 position 描述与 fence_contract 矛盾

**pitfalls.md:19**：
```
LoomGUI 不映射 CSS position（apply_decl 无 position arm）
→ 所有节点 position 永远是 taffy 默认 Relative
```

**fence_contract.rs:193** 证明 `apply_decl` **有** position arm 且接受 `relative`/`absolute`。

**不一致**：pitfalls.md 的这份 taffy API 参考写于 v1.4-b 之前。v1.4-b 后 `apply_decl` 加了 position arm，但 pitfall 文本未更新。

**严重级别**：**中**。对"position 进不进 layout"的正确理解需要知道 position 现在是显式映射，而不是靠 taffy 默认值。

### 2.3 main-design.md §14 tick 时序 vs CLAUDE.md tick 时序冲突

- `CLAUDE.md:82` — `process(hit 用上帧 world) → rematch_pseudo_classes → solve → refresh_content → compute_world_transforms → build`（正确）
- `main-design.md:632-636` — `solve → process → ... → compute → rematch → build`（错误/旧版）

两份都是项目级权威文档，但给出了完全不同的 tick 时序。CLAUDE.md 的时序与 `stage.rs:716-793` 实际代码一致，main-design.md 的时序不一致。

**严重级别**：**严重**。新成员面对两份矛盾的文档不知道该信哪个。且 main-design.md 作为"设计契约"，其描述的时序是**错的**（rematch 在 compute 之后的话伪类改 transform 当帧丢）。

---

## 3. 已过时但被引用为"权威"的段落

### 3.1 main-design.md §5.2—5.3 Node 类型层级和数据结构

整个 §5.2、§5.3 是早期设计草稿，Node 类型列了 16 种、Node struct 字段与实际完全不对应。但在 CLAUDE.md 等处被引用为架构核心文档。

### 3.2 main-design.md §10.2 Transition【拆 v1.5-b，本期未实现】

§10.2 标为未实现但在 §10.3 Controller 之前以"本节描述当前设计"口吻出现，混淆读者。§10.4 Gear 标注【砍】但保留完整 API 描述，v1.5 Controller 已用 CSS 属性选择器替代 Gear。

### 3.3 main-design.md §11 资源/包系统

§11 开头自述"§11.2-11.5 仍是早期设计草稿，与当前实现有差距"，但位置处于主设计文档核心章节，读者容易跳过开头声明直接读内容。§11.4 的 `TextureView` 引用计数模型已替换为 §11.3 描述的 path-only 模型。

### 3.4 main-design.md §8.1 文本测量/渲染分离

§8.1 描述"文本 mesh 在后端生成"已被 §8.2 末尾的勘误推翻（v1.6 字体搬进核心），但 §8.1 本身没有作废标记（仅 §8 开头加了"v1.6 演进前瞻"笼统声明）。

### 3.5 main-design.md §13.3 blob v9 vs 实际 blob v10

§13.3 整个段的列描述是基于 v9（22 列，含 text_off/text_len）。实际已升 v10（20 列，text 塌进 mesh_arena）。段中列出的 `text_off`、`text_len` 列在代码中已不存在。

### 3.6 CLAUDE.md 多处引用"当前 22 列"

CLAUDE.md:91 "当前 22 列" — 实际 20 列（v10）。

---

## 4. 架构不变量遵守情况检查

### 4.1 transform 不进 taffy ✅ 遵守

代码中 transform 存在 `ResolvedStyle.transform`（`style/resolved.rs`），layout 层不解引用它，taffy solve 只用 `taffy_style` 字段。设计正确。

### 4.2 每帧一次 solve ✅ 遵守

`stage.rs:779` — `solve(scene, &self.fonts, self.root_size, &self.image_sizes)` 在 tick_and_render 中仅调用一次。

### 4.3 代际 NodeId ✅ 遵守

`node.rs:12-58` — NodeId(pub u32) 高 20bit index + 低 12bit gen，`remove_node` gen++，slotmap 桥接。设计正确。

### 4.4 单一动画时钟 ✅ 遵守

`stage.rs:731` — `self.tweens.update(dt, scene, &mut out)` 是唯一时钟调用点。Controller/transition 都通过提交/kill tween 来驱动。

### 4.5 渲染树 = 瞬态 Vec<RenderNode> ✅ 遵守

`stage.rs:788-794` — `build_render_nodes` 产 FrameData → C# 拷贝后在 Rust 下帧 reset。

### 4.6 核心不知引擎对象 ✅ 遵守

所有 FFI 接口返回整数 id + 扁平 byte buffer，不从 Rust 引 Unity 对象。

---

## 5. pitfalls.md "已修"坑残留检查

### 5.1 坑 103（tick 时序）— 已修 ✅

`stage.rs:716` tick_and_render 开头注释 "支柱1重排——rematch 提到 solve 前"，代码证实 rematch 在 solve 之前。正确修。

### 5.2 坑 67（双测量不一致）— 已修 ✅

`node.rs:279-284` — `text_layouts` 存 layout 测量结果，render 复用。正确修。

### 5.3 坑 56/75/76/105（dirty hash 采样漏字段）— 已修 ✅

`stage.rs:54` — `prev_node_hashes: HashMap<u32, (u64, u64)>`，双 hash (header + payload) 全量。已根治。

### 5.4 坑 102（FFI 入口 panic）— 已修 ✅

`stage.rs:717-721` — `let scene = match self.scene.as_mut() { Some(s) => s, None => return FrameData::default() }`。null scene 不 panic。

### 5.5 坑 32（implementer 改实现通过测试）— 已恢复 ✅

### 5.6 pitfalls.md §1.1 position 描述未更新 ⚠️

pitfalls.md:19 "LoomGUI 不映射 CSS position（apply_decl 无 position arm）"在 v1.4-b 后过时。见上文 2.2。

---

## 6. 反复出现的坑 — 设计层面问题

### 6.1 dirty hash 漏字段（坑 56 → 75 → 76 → 105 → 106）

经历了 5 轮迭代才做到全量 hash。根因：dirty/hash 机制设计为"选择性采样"（只 hash 部分字段），但渲染可见字段集合随功能增长持续膨胀，每次新增渲染路径（background-size UV / 圆角多顶点 / text align / scroll re-base）都会在 hash 摘要中漏字段。最终方案（双 hash 全量）证明：采样策略比全量 hash 更脆弱。**建议**：如果未来再加渲染字段，双 hash 全量模式应被文档标记为不可退化的设计决策。

### 6.2 跨层特性单测验不了（坑 131-133, 58, 59, 78, 88）

scroll、clip、Controller、filter、NativeHost 等跨 layout/render/blob/MirrorPool 多层的特性，其 bug 只在 PlayMode（实机运行）显现，Rust 单测全绿也无法暴露。根因是这些特性涉及 Rust→blob→C# MirrorPool→shader 完整链路，Rust 单测只能验证单一 crate 逻辑。**建议**：跨层特性加入"集成验收 check list"制度化（main-design.md 或 CLAUDE.md 加附录），新增跨层特性后强制跑 showcase PlayMode 逐项过。

### 6.3 Marshal.PtrToStructure 禁用声明与实际使用矛盾

**CLAUDE.md:91**：
```
禁 `Marshal.PtrToStructure`（IL2CPP struct 对齐坑）
```

**LoomEventHandler.cs:208,265**：
```csharp
var evt = System.Runtime.InteropServices.Marshal.PtrToStructure<LoomEvent>(ptr + i * recSize);
var evt = System.Runtime.InteropServices.Marshal.PtrToStructure<LoomControllerChangedEvent>(ptr + i * recSize);
```

**实际状态**：有注释标注"IL2CPP 移动端对齐坑届时换 Span+BinaryPrimitives"，是已知技术债而非隐藏违规。但 "禁" 字语义过强（暗示代码中不应存在），应改为 "当前在桌面 Mono 上使用 PtrToStructure，移动端 IL2CPP 上线前须换 Span+BinaryPrimitives"。

**严重级别**：**低**（文档夸大，代码有自知注释）。

---

## 7. 总结

| 类别 | 数量 | 严重 | 中 | 低 |
|------|------|------|----|----|
| 关键数据不匹配 | 4 | 4 | 0 | 0 |
| 文档内部矛盾 | 3 | 1 | 2 | 0 |
| 已过时仍被引用 | 6 | 0 | 6 | 0 |
| 架构不变量 | 6 | 0 | 0 | 0 |
| 坑残留 | 6 | 0 | 1 | 0 |
| 设计问题 | 3 | 0 | 1 | 1 |
| **合计** | **28** | **5** | **10** | **1** |

**最优先修**（影响新人 ramp-up + 跨团队协作）：
1. `main-design.md` §14 tick 时序 — 改为与 CLAUDE.md/stage.rs 一致的正确时序
2. `main-design.md` §5.2—5.3 Node 类型和 struct — 重写为与实际代码一致
3. `main-design.md` §13.3 blob 列 — 从 v9/22列 更新为 v10/20列
4. CLAUDE.md:91 "22 列" → "20 列"
5. pitfalls.md:19 position 描述 — 更新为 v1.4-b 后的显式映射

**次优先修**（消除歧义）：
6. `main-design.md` §3.2 position:relative 描述 — 与 fence.md 对齐
7. `main-design.md` §10.2/§10.4 未实现/已砍特性 — 加明确废弃标记或移除
8. CLAUDE.md Marshal.PtrToStructure — 从"禁"改为"桌面 Mono 当前使用，IL2CPP 前换"
