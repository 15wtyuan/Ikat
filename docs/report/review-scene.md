# Scene 模块深度代码审查

## 概述

审查范围：`loomgui_core/src/scene/` 全部 4 个文件（node.rs / dynamic.rs / transform.rs / mod.rs），共约 2000 行。Scene 是项目核心数据结构，承载节点树、布局几何、渲染中间状态、动画覆盖、滚动状态、Controller 状态机。审查重点是数据结构正确性、内存安全、接口稳定性、清理完整性。

---

## 发现列表

### 1. `controllers` HashMap 未在 `remove_node` 中清除

- **文件**：`loomgui_core/src/scene/dynamic.rs:287-322`
- **严重级别**：中（内存泄漏 + 潜在状态残留）

**问题**：`remove_node` 联动清理了 `anim.clear_node(id)`（第 308 行）、`scroll.remove(id)`（第 309 行）、`tweens.kill_node(id)`（第 310 行）、`focused_node`（第 313-315 行），但**漏掉了 `scene.controllers.remove(&id)`**。

```rust
// 第 307-315 行：
scene.anim.clear_node(id);
scene.scroll.remove(id);
tweens.kill_node(id);
// ⚠️ 缺少: scene.controllers.remove(&id);
if scene.focused_node == Some(id) {
    scene.focused_node = None;
}
```

**影响**：
- `controllers` 是由 `set_selected_index` 懒注册的 `HashMap<NodeId, Controller>`（`node.rs:468-470`），条目建后永不清除
- 删掉的挂载节点在 HashMap 中留下孤儿条目：NodeId 的 gen 部分变了所以不会被新节点撞 key，无法触发误匹配——当前是"安全泄漏"而非 correctness bug
- 但多 Controller 场景反复删/建挂载点 → HashMap 持续膨胀，无回收路径
- 一旦 slotmap 12-bit gen 回绕（同一 index slot 删/建 4096+ 次），旧 gen 的 NodeId 可能与早期 entries 的 key 碰撞

**修复方向**：在第 309 行后加 `scene.controllers.remove(&id);`。递归删子时应同样处理（子节点也有可能是挂载点）。

---

### 2. Node struct 字段膨胀——23 字段混杂多项职责

- **文件**：`loomgui_core/src/scene/node.rs:90-139`
- **严重级别**：低（可维护性，非正确性）

**分析**：Node struct 共 23 个字段，按职责可分层：

| 层 | 字段 | 数量 |
|----|------|------|
| 树结构 | `id`, `parent`, `children` | 3 |
| 核心语义 | `kind`, `style`, `base_style` | 3 |
| 布局桥 | `taffy_id`, `layout_rect`, `clip_rect` | 3 |
| 脏标志 | `dirty_mesh`, `dirty_text` | 2 |
| 选择器源 | `classes`, `id_attr` | 2 |
| 交互状态 | `touchable`, `hovered`, `active`, `disabled`, `draggable`, `tabindex`, `focused` | 7 |
| 功能特性 | `reuse_key`, `data_controller`, `cascaded_once` | 3 |

**后来硬加迹象明显**：
- `draggable`（`node.rs:120`）："opt-in 可拖拽"语义，与 `touchable` 有重叠但独立字段
- `reuse_key`（`node.rs:129`）："运行时字段（不进 pkg）"——注释自曝这是临时性字段，不应该和持久字段混在一起
- `cascaded_once`（`node.rs:138`）：纯内部状态机标志，注释"是否已 cascade 过至少一次"
- `data_controller`（`node.rs:134`）：Controller 挂载点声明，仅匹配器回溯时用到

**问题**：
- Node 作为全场景热数据，每个字段每 tick 都可能被遍历。混合职责增加缓存行压力
- `Default` 实现（`node.rs:145-171`）手工列出 23 个默认值，新增字段时须同步更新——这是典型 boilerplate 高发点
- 交互状态 7 个字段只有少数节点（当前 hover/active/focused 链）同时为非默认值

**修复方向**：不必立即重构。如果 Node struct 继续增长，考虑拆出 `InteractionState` 子结构（hovered/active/focused/disabled）、拆出 `DragState`（draggable），用 `Option<Box<InteractionState>>` 减少常态存储开销。阈值：再增 3 个以上运行时字段时执行。

---

### 3. `create_node` / `create_root` 未 resize 并行数组

- **文件**：`loomgui_core/src/scene/dynamic.rs:59-108`
- **严重级别**：低（延迟修复）

**问题**：`create_node`（第 97 行 `scene.nodes.insert`）可能触发 slotmap 扩容，但 `text_layouts` 和 `rich_fragments` 的容量未同步扩张。若 driver 在 `build_render_nodes` 之前调 `create_node` 且 slotmap 扩容，render 按新 index 读 `text_layouts` 会越界。

```rust
// dynamic.rs:97-99 — insert 后未 resize text_layouts / rich_fragments
let key = scene.nodes.insert(node);
let id = NodeId::from_key(key);
scene.nodes.get_mut(key).unwrap().id = id;
```

**当前缓解因素**：
- `build_render_nodes` 用 `.get(idx)` 读 `text_layouts`（`render/mod.rs:354`），越界返 None → fallback 重测文本，不 panic
- `layout::solve` 每帧重建 `text_layouts = vec![None; capacity + 1]`（`layout/mod.rs:216`），所以新容量会在下帧同步
- 典型使用顺序：driver `create_node`（tick 间）→ 下 tick `solve`（重建 text_layouts）→ `build_render_nodes`（安全读）

**修复方向**：在 `create_node`、`create_node_from_template`、`create_root` 的 insert 后加 `resize_with` 对齐容量（与 `Scene::build` 第 392-393 行保持一致）：

```rust
scene.text_layouts.resize_with(scene.nodes.capacity() + 1, || None);
scene.rich_fragments.resize_with(scene.nodes.capacity() + 1, || None);
```

这样即使同一 tick 内 render 读到新 index 也不会越界。

---

### 4. `base_style` 注释与 `set_style` 行为矛盾

- **文件**：`loomgui_core/src/scene/node.rs:105-106`，`dynamic.rs:264-271`
- **严重级别**：低（文档误导）

**问题**：`node.rs:105-106` 注释写着"打包期 resolve_styles 产物（不变，rematch 基线）"，暗示 `base_style` 是不可变的。但实际上 `dynamic.rs:264-271` 的 `set_style` 会直接修改 `base_style`：

```rust
// node.rs:105-106
pub base_style: ResolvedStyle, // 打包期 resolve_styles 产物（不变，rematch 基线）
```

```rust
// dynamic.rs:266-268
pub fn set_style(scene: &mut Scene, node: NodeId, css: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    apply_css(&mut n.base_style, css);  // ← 直接改 base_style
    n.dirty_mesh = true;
```

**影响**："不变"注释会误导维护者以为 `base_style` 是 write-once 的，在并发/缓存场景下可能做出错误假设。

**修复方向**：改注释为"打包期 resolve_styles 产物（rematch 基线；runtime 可通过 set_style 改）"。

---

### 5. `compute_world_transforms` pivot 仅支持 center 原点

- **文件**：`loomgui_core/src/scene/transform.rs:26,39-46`
- **严重级别**：信息（设计约束，非 bug）

**分析**：pivot 硬编码为 box center `(w/2, h/2)`（第 26 行）：

```rust
let pivot = (lr.w / 2.0, lr.h / 2.0);
```

公式正确：
```
local = T(rel) ∘ T(pivot) ∘ m ∘ T(-pivot)
```
其中 `m` 是 CSS transform matrix（或 anim override），`rel` 是相对父布局 rect 原点的偏移。世界坐标累积含父滚动 offset `T(-scroll_pos)`。

**约束**：不支持 CSS `transform-origin` 属性（CSS 默认是 50% 50% = center，此代码与默认值一致）。但如果围栏将来扩展支持 `transform-origin: top left` 等值，此处需改为从 `style.transform_origin` 读 pivot。当前围栏不支持此属性，无实际 bug。

---

### 6. `rec` 函数每层递归 clone children（可优化但不紧急）

- **文件**：`loomgui_core/src/scene/transform.rs:61-64`，`dynamic.rs:290`
- **严重级别**：低（性能微优化）

**问题**：`compute_world_transforms` 的递归函数 `rec` 在每层执行 `node.children.clone()`（第 61 行），避免持有 scene 不可变借时递归再借 scene。

```rust
// transform.rs:61-63
let kids = node.children.clone();
for c in kids {
    rec(scene, anim, c, world, worlds);
}
```

同样模式在 `remove_node` 中也有（`dynamic.rs:290`），但那里合理——需要边迭代边改 slotmap。

**为何是 micro-issue**：节点数通常在数千以内，每个 children Vec 只是 NodeId(u32) 的 Vec，clone 成本低。

**修复方向**：可以改 `rec` 为两阶段——先收集 DFS 序节点 ID，再 for 循环算 world matrix，消除所有 clone。不值得为当前规模立即做。

---

### 7. Scene 结构体全 pub 字段——无封装边界

- **文件**：`loomgui_core/src/scene/node.rs:258-298`
- **严重级别**：信息（架构特征，非 bug）

**问题**：Scene 的 14 个字段全部 `pub`，跨模块直接读写：

```rust
pub struct Scene {
    pub roots: Vec<NodeId>,
    pub nodes: SlotMap<DefaultKey, Node>,
    pub dynamic_rules: DynamicRuleTable,
    pub focused_node: Option<NodeId>,
    pub world_transforms: Vec<Affine2>,
    pub node_sort_keys: Vec<u32>,
    pub anim: AnimTable,
    pub scroll: ScrollTable,
    pub text_layouts: Vec<Option<TextLayout>>,
    pub rich_fragments: Vec<Option<Vec<RichFragment>>>,
    pub controllers: HashMap<NodeId, Controller>,
    pub pending_controller_events: Vec<ControllerChangedEvent>,
    pub pending_transitions: Vec<TransitionRequest>,
}
```

**后果**：
- `layout::solve` 直接写 `scene.text_layouts`（`layout/mod.rs:369`）
- `render::build_render_nodes` 直接读 `scene.text_layouts`、`scene.anim`、`scene.scroll`（`render/mod.rs:352-399`）
- `tick_and_render` 直接清 `scene.pending_controller_events`、写 `scene.node_sort_keys`（`stage.rs:726,795`）
- 任何模块都可能违反 Scene 不变量（如 `remove_node` 后残留 stale 索引）

**评价**：LoomGUI 是核心库而非面向外部 SDK，内部模块间 trust-based 访问可接受。但 Scene 成为"全局可变状态容器"（类似帧全局变量），新增维护者难追踪每个字段的 writer 集合。**当前不应加 getter/setter（违反 YAGNI），但若添加第 15 个并行数组字段，需认真考虑引入 Scene 不变量自动化检查**。

---

### 8. 12-bit gen 溢出风险——文档声称"足够"但无硬保护

- **文件**：`loomgui_core/src/scene/node.rs:39-41,52-58`
- **严重级别**：低（极端场景）

**分析**：`gen()` 取低 12 bit（`node.rs:39-41`），`to_key()` 将 12-bit gen 写回 slotmap version（`node.rs:56-57`）：

```rust
// node.rs:39-41
pub fn gen(self) -> u16 {
    (self.0 & 0xFFF) as u16
}
// node.rs:54-57
pub fn to_key(self) -> DefaultKey {
    let idx = (self.0 >> 12) as u64;
    let version = (self.0 & 0xFFF) as u64;
    DefaultKey::from(KeyData::from_ffi((version << 32) | idx))
}
```

**slotmap 内部行为**：`remove` 使该槽 version 递增；`from_ffi` 用 `version | 1` 强制奇数（slotmap 约定）。12-bit gen 值域 0-4095，但强制奇数后实际可用值仅 2048 个不同 odd 值。

**绕回场景**：同一 index 槽 2048 次 remove+insert 后 gen 回绕 → `to_key()` 重构的 version 与上一次同 index 的历史 NodeId 碰撞 → 旧 NodeId 误恢复为"有效"。需约 2000+ 次删/建同一槽，正常 UI 场景几乎不可能。

文档注释（`node.rs:24`）承认截断："version 截断 → slotmap.get 安全返 None"。准确但不完整—应补充回绕条件。

**修复方向**：不需要改代码。在 `to_key()` 注释中补一句：约 2048 次同槽删/建后 gen 回绕，正常 UI 不可能触发。

---

### 9. `build_render_nodes` 读 `text_layouts` 的 fallback 重测——消除双测量不一致的初衷被 fallback 路径削弱

- **文件**：`loomgui_core/src/render/mod.rs:352-369`，`scene/node.rs:280-284`
- **严重级别**：信息（设计一致性）

**问题**：`text_layouts` 的设计初衷（`node.rs:280-283`）是"layout 存，render 复用，消除双测量不一致"。但 `render/mod.rs:352-359` 在 `text_layouts[idx]` 为 None 时 fallback 重测：

```rust
// render/mod.rs:352-359
let mut layout = scene
    .text_layouts
    .get(n.id.index())
    .cloned()
    .flatten()
    .unwrap_or_else(|| {
        measure_text(content, s.font_size, ...)  // ← fallback 重测
    });
```

**场景**：动态建树 API 在 `create_node` 新建 Text 节点后同一 tick 内 render（text_layouts 未 resize 对齐 → `.get(idx)` 返 None → fallback 重测）。见发现 3。

**评价**：fallback 正确但让"双测量不一致"的原始 bug 可能以更隐蔽的方式重现（fallback 路径用 `rect.w` 测，layout 路径用 taffy 给的 max_width 测）。当前可接受—fallback 仅在动态建树的过渡帧触发。

---

### 10. `remove_node` 的 PointerState 清理被推迟到消费点

- **文件**：`loomgui_core/src/scene/dynamic.rs:316-318`
- **严重级别**：信息（有意为之，非 bug）

```rust
// dynamic.rs:316-318
// PointerState（Stage 层）的 down_node/hovered_chain/drag_target 等不在此清：
// 消费点（input.rs）全有 scene.get None-check 兜底，悬空 NodeId 仅向已删节点发 stale 事件
// （RollOut/DRAG_MOVE），无 panic；强清需把 pointer_state 传进 remove_node（改签名），YAGNI。
```

**评价**：符合项目 ponytail 原则。调用方（PointerState）所有对 scene.get 的调用都有 None-check，删除节点后仅产生 1 帧 stale 事件（如对已删节点发 RollOut），不会 panic。如果拖拽过程中删除节点，DragState 指向悬空 NodeId 仅持续到 `process` 发现 `scene.get` 返 None 并取消拖拽（1 帧）。**不引入耦合，设计正确。**

---

## 汇总

| # | 发现 | 文件 | 严重级别 | 修复优先级 |
|---|------|------|----------|-----------|
| 1 | controllers HashMap 未在 remove_node 中清除 | dynamic.rs | 中 | 立即 |
| 2 | Node 23 字段混职责 | node.rs | 低 | 观察 |
| 3 | create_node 未 resize 并行数组 | dynamic.rs | 低 | 近期 |
| 4 | base_style 注释说"不变"但 set_style 可改 | node.rs | 低 | 近期 |
| 5 | pivot 仅 center，不支持 transform-origin | transform.rs | 信息 | 不改 |
| 6 | 递归 clone children，可优化 | transform.rs | 低 | 不改 |
| 7 | Scene 全 pub 字段无封装 | node.rs | 信息 | 不改 |
| 8 | 12-bit gen 回绕风险 | node.rs | 低 | 不改 |
| 9 | text_layouts fallback 重测削弱设计初衷 | render/mod.rs | 信息 | 不改 |
| 10 | PointerState 悬挂推迟消费 | dynamic.rs | ✅ 正确 | 不改 |

### 核心评价

**NodeId 代际保护机制**：设计正确——20/12 分位 + slotmap 校验 + `from_key`/`to_key` 桥接——能满足 100 万节点、每槽 2048 次重用的正常 UI 需求。删除后旧句柄 `scene.get` 正确返 None。发现 8 的回绕风险是极端边界。

**remove_node 清理**：anim/scroll/tween/focused_node/parent 均已覆盖。controllers 遗漏（发现 1）是唯一实质缺陷。

**接口稳定性**：Scene 作为"帧全局状态容器"，全 pub 字段被多模块直接读写。当前比封装性更优先的是可读性——字段名就是文档。在各模块已熟悉契约的前提下，这个设计是合理的。

**compute_world_transforms**：公式正确，支持 transform + scroll offset + anim override 的完整组合。

**性能**：text_layouts 和 world_transforms 每帧全量重建（不可变重建，非增量更新），数百节点无感知。千级节点以上可考虑增量 compute，但当前不是瓶颈。
