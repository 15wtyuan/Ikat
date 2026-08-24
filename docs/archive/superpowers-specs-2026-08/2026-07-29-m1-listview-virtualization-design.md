# M1 · ListView 虚拟化 设计

> 日期：2026-07-29　里程碑：M1（复合束第一硬骨头）
> 相关：`docs/roadmap/milestones.md` M1、`docs/design/public-api.md` §8、`docs/design/main-design.md`

## 1. 目标与判据

`ul → ListView`、`li → ListItem`，把虚拟化（slot 池化 / 可见区裁剪 / 不等高补偿 / content_size / reuse_key）整层吸收进框架，driver 不再手写虚拟列表。

**本质判据**：`ItemCount=1000` 与 `ItemCount=10000` 的 render node 数**相等**（≈ 可见项数），不随总项数增长。

## 2. 现状（勘察结论）

- fence 已有 `ul→SemanticKind::ListView`、`li→ListItem`、`template→Template`（`crates/fence/src/schema/tag.rs:123-126`）；`ol` 已被移出围栏，故本 spec 只处理 `ul`。
- `NodeKind::ListView/ListItem` 已存在（`crates/core/src/scene/node.rs:91-115`）。
- 虚拟化内核为零；C# `ListView` 全 `throw NE()`（`unity/package/Runtime/Public/LoomGUI.Nodes.cs:2329-2349`）。
- 滚动基建齐全：`ScrollPaneState`（`crates/core/src/scroll.rs:71`）、`refresh_content_sizes`（scroll.rs:553）在 solve 之后跑；`content_size_overridden` 逃生舱存在。
- `instantiate(pkg, component)`（`crates/core/src/stage.rs:693`）只能克隆**包级**组件，无场景内子树克隆。
- **`<template>` 当前根本不进场景树**：`crates/packer/pkg/src/bridge.rs:34` 打包期 `is_in_template_subtree(ir_idx, parsed) → continue` 整个子树跳过；`map_semantic` 对 `SemanticKind::Template` 直接返 `Err`（bridge.rs:123）；`NodeKind` 无 `Template` 变体。故「克隆 `<template>` 子树」在现状下无源可克 —— 见 §6.1。
- **`NodeFlags::SCOPE_ROOT` 是双语义 flag**（`crates/core/src/scene/node.rs:23` 注释原文：「`Get<T>` 查找边界 **+** CSS dynamic_rules 作用域隔离都据此判定」），直接给 slot 根打此位会让页面 CSS 对 item 全部失效 —— 见 §6.2。
- `PKG_FORMAT_VERSION = 26`（`crates/core/src/asset/mod.rs:23-25`，MIN=MAX=26）。
- tick 入口 `Stage::tick_and_render`（`crates/core/src/stage.rs:818`）。

## 3. 关键决策

| 决策 | 选择 | 理由 |
|---|---|---|
| 虚拟化层次 | **全在 Rust core** | 跨引擎共享核心是差异化卖点；bind 过桥频率低（仅进出可见区） |
| item 定位 | **可见 item 走正常 CSS 流 + 头/尾 spacer** | 对 CSS 语义透明（gap/margin/padding 天然生效），不在 core 复刻盒模型；护城河 = 布局可预测 |
| 不等高 | **估算 + 实测回填 + scroll anchoring** | 一次做完，不分步 |
| bind 过桥 | **pending 队列 + 帧首排空**（无跨 FFI 同步回调） | 异常安全，符合攒批回写范式，不破坏「每帧一次 solve」 |
| 模板克隆 | **新增 `clone_subtree`（场景级）** | 复用 instantiate 的重建逻辑；以后 slot / Custom Element 共用。**注意：`clone_subtree` 本身不动 pkg，但让 `<template>` 有东西可克隆必须动 pkg（见下行）** |
| `<template>` 支持 | **本轮收进 pkg，认 v26→v27 bump** | 模板来源三档完整；bump 成本一次付清（Custom Element / slot 后续复用同一通道）|
| API 范围 | 见 §9 | YAGNI；删 SelectedIndex/SelectionChanged |

## 4. 数据模型

side table 模式（照 `EditState`/`ScrollTable`），不塞进 `Node`。新文件 `crates/core/src/list.rs`：

```rust
struct ListState {
    item_count: usize,
    template_root: Option<NodeId>,   // 游离态模板根，clone_subtree 的源
    heights: HeightCache,
    slots: Vec<Slot>,                // 已实例化的 item 子树
    free: Vec<NodeId>,               // 空闲 slot 池
    visible: Range<usize>,
    head_spacer: NodeId,
    tail_spacer: NodeId,
    pending_binds: Vec<(NodeId, usize)>,
    dirty: DirtyFlags,
}
struct Slot { node: NodeId, item_index: usize }
```

`ListTable: SecondaryMap<NodeId, ListState>`。**表中有条目 = 数据驱动模式**，这就是 public-api 所说「隐式锁定」的实现，不需要额外 mode 枚举。静态 ul 完全不碰这套。

**树形状**（虚拟化开启后 ListView 的真实子节点）：

```
<ul>                  ← ListView
  [head spacer]       ← Container，height = heights.sum(0..visible.start)
  <li slot> ...       ← 仅可见区 + buffer
  [tail spacer]       ← height = heights.sum(visible.end..item_count)
</ul>
```

spacer 是普通 Container，`flex-shrink:0` + 显式 height，**不带 class、不参与 cascade**（用户 CSS 无法命中）。

**gap 修正（按 display 分支，不能无条件扣）**：ul 的 `DisplayDefault` 是 **Block**（`tag.rs:267`），P1 C2 后走 taffy 真 block 流。实证：taffy 0.12.2 `compute/block.rs` **完全不实现 gap**（全文唯一一处 "gap" 是注释 `with zero inter-item gap`）。故：

- **ul 为 Block（默认）**：spacer 与 item 间**无** gap，**不做任何 gap 扣减**（盲扣会令 spacer 偏矮、滚动条与内容不符）；
- **用户显式写 `display:flex` 的 ul**：才读 `ResolvedStyle.row_gap` 做扣减（spacer 高度 > 0 且两侧有兄弟时各扣一个 gap）。

**heights 语义 = margin box（不是 border box）**：`layout_rect` 是 border box，不含 margin。而 `li { margin-bottom: 8px }` 是极常见写法，若 heights 漏计 margin 则 spacer 求和**系统性偏小**、anchoring 的 delta 也跟着偏 → 滚回头必漂移（直接打脸 §11 的「无漂移」断言）。故回填时 `height_of(i) = layout_rect.h + margin_top + margin_bottom`（解析后的像素值）。

**margin 折叠约定**：block 流下 taffy 有 margin collapsing（`can_be_collapsed_through`），spacer 的存在会改变相邻 item 的折叠行为。约定：**spacer 声明 `padding-top: 0.01px`（或等效阻断手段）使其不可被折叠穿透**，令 spacer 高度严格等于所设值、不被相邻 margin 吞掉。实现时需单测锁死此行为（混合 margin 数据集）。

**与 ScrollPane 的关系**：ListView 自己不滚。滚动仍由祖先 `overflow:auto` 容器负责，ListView 只是其内容。spacer 撑满全量高度 ⇒ `refresh_content_sizes` 的自然计算即正确，**不使用 `set_content_size` 逃生舱**（旧 driver 补丁随本 spec 退役）。找不到祖先 ScrollPane 时退化为全量渲染 + 一次性运行时警告（不报错——短列表无滚动容器合法）。

**ul 高度必须为 auto（否则静默失效）**：`refresh_content_sizes`（`scroll.rs:600-621`）算的是 **pane 直接子节点**的 AABB。链路是 `pane > ul > spacer/slot`，所以 content 高 = `ul.layout_rect.h`。这**仅在 ul 高度 auto 时**等于 spacer+item 总和；一旦用户写 `height:100%`、或 pane 是 flex 容器使 ul stretch/grow，ul.h 被钉成 viewport 高 → content_size = viewport → overlap = 0 → **完全不能滚，且是静默失败**。

处置：进入数据驱动模式时**运行时检测**——若 ul 的解析高度非 auto（含被祖先 flex 拉伸：`align-items` 非 `flex-start` 且交叉轴为高，或 `flex-grow > 0`）→ 抛 `UIContractException`，消息明确指出「数据驱动 ListView 的高度必须为 auto，否则虚拟化无法撑出可滚内容」。不静默降级。

## 5. 高度缓存与可见区

```rust
struct HeightCache {
    known: Vec<Option<f32>>,   // len = item_count
    estimate: f32,             // 已测均值；无已测时 = 模板首次布局高
    known_sum: f32,
    known_count: usize,
}
```

- `height_of(i) = known[i].unwrap_or(estimate)`
- `sum(range) = 已测部分精确和 + 未测数 × estimate`
- **实现选择**：M1 用朴素 O(n) 求和 + 每帧缓存（10000 项一次加法循环 ≈ 微秒级）。Fenwick 树 defer，**触发判据：profile 显示 sum 占 tick > 5%**。

**回填**：solve 之后遍历 slots，读 `layout_rect.height` 写回 `known[i]`。

**scroll anchoring（不等高能否好用的关键）**：若本帧回填修正了 `visible.start` 之前区间（头 spacer 覆盖范围）的高度总和，delta ≠ 0 → **同帧把祖先 ScrollPane 的 `scroll_pos` 补偿 delta**。用户视角内容不动，滚动条长度悄然修正。补偿点在 solve 之后、`refresh_content_sizes` 之前；`scroll_pos` 只被 `compute_world_transforms` 消费，**不触发二次 solve，架构不变量安全**。

**anchoring 与 tween 的交互（必须显式处理）**：`refresh_content_sizes` 在 overlap 变化且 `scroll_pos` 越界时会 clamp **并 `st.tweening = [0, 0]` 静默杀掉正在跑的 tween**（`scroll.rs:640-647`）。而 `ScrollToItem(Smooth)` 正是 tween，不等高列表在 tween 期间不断回填 → content_size 变 → overlap 变 → 若补偿后瞬时越界，**平滑滚动会半途静默停住**。

处置：`ListState` 持 `anchoring_active` 标记，本帧发生过 anchoring 补偿时置位；`refresh_content_sizes` 的 clamp 分支在该标记下**仍 clamp 位置但不清 `tweening`**（几何变化源于虚拟化回填而非真实内容突变，tween 应继续）。同时 `ScrollToItem` 的 tween 目标值在每次回填后**重算**（目标项的累计偏移会随实测变化），否则平滑滚动会停在错位置。

**可见区计算**（帧首）—— `scroll_pos` **和 `layout_rect` 都是上一帧的**（与「hit 用上帧 world」同一时序哲学）：

```
viewport.h = pane.layout_rect.h                    // content_box_size 当前就是 border box 简化（scroll.rs:653-656）
                                                   // 本 spec 沿用同一简化，不引入第二套口径语义
listview_offset = ul.layout_rect.y - pane.layout_rect.y   // 同为 pane 坐标系下的 layout 值
top    = scroll_pos.y - listview_offset
first  = max i  where sum(0..i) <= top
last   = min j  where sum(0..j) >= top + viewport.h
visible = [first - BUFFER, last + BUFFER] ∩ [0, item_count)
```

**冷启动（首帧）**：首帧 ul / pane 的 `layout_rect` 全为默认 0 → `viewport.h = 0` → visible 空集 → 列表空白一帧。处置：`viewport.h == 0` 时视为**未就绪**，退化为「先实例化 `INITIAL_SLOTS`（= 1 + 2*BUFFER）项」而非空集，下帧 layout_rect 就绪后转入正常路径。保证首帧有内容且不会因 item_count 巨大而卡。

`BUFFER = 2`，内部常量，不暴露为 API。

**slot 回收/复用**：新旧 visible 求差集——离开的 slot 入空闲池；进入的 index 优先从池取（取不到才 `clone_subtree`），配对时**优先复用同侧 slot**（滚动方向连续复用，减少 mesh churn）。复用即 `pending_binds.push((node, new_index))`。

**`reuse_key` 编码（必须显式避开两个撞车）**：`MirrorPool.cs:76-78` 的实现是 `poolKey = reuseKey != 0 ? reuseKey : id`，且 `_poolByReuse` 是**场景级全局单 dict**。故：

- **0 是 sentinel**（表示「不复用，按 node_id」），不得使用；
- **多个 ListView 不得撞车**（mail + inventory 同页就会踩）。

编码：`reuse_key = ((list_ordinal + 1) << 16) | (slot_idx & 0xFFFF)`。`list_ordinal` = 该 ListView 进入数据驱动模式的递增序号（存于 `ListTable`）。保证结果恒 ≠ 0。slot_idx 上限 65535（远超可见区需求），list_ordinal 越界时 debug_assert。

## 6. 模板与 clone_subtree

`Stage::clone_subtree(src: NodeId) -> NodeId`（`crates/core/src/stage.rs`，与 `instantiate` 并列）：深拷贝 `kind` / `classes` / `id_attr` / `base_style` / 文本 / img src / 控件初值，递归子树，返回**游离**新根（不挂树，调用方 append）。

- **side table 逐条判定**（权威清单取自 `remove_node` 的联动清理，`scene/dynamic.rs:528-533`；逐条写死，避免 P3 NumberField 漏 dispatch arm 那类缺口）：

| side table | 克隆？ | 理由 |
|---|---|---|
| `scene.controls`（ControlState）| **克隆模板的初值** | 模板含 Slider/Toggle 时，不克隆则控件无状态、渲染与命中均挂；克隆的是**模板原始值**（非上一个 item 的值）。slot 被复用时必须 **reset 回模板初值**再 bind |
| `scene.text_contents` | 克隆 | 模板文本是结构的一部分（BindItem 会覆盖）|
| `scene.image_srcs` | 克隆 | 同上 |
| `scene.scroll`（ScrollPaneState）| **不克隆** | 运行时滚动位置，新实例应从 0 起 |
| `scene.anim` / `tweens` | **不克隆** | 运行时动画状态 |
| EditState（编辑态）| **不克隆** | 光标/选区/composition 是运行时态 |
| `text_layouts` | **不克隆** | 派生缓存，solve 重建 |
| `focused_node` | **不克隆** | 全局单一焦点；**且 slot 回收时若焦点在其内部，需先清焦点**（否则焦点悬空到被复用的节点上）|
| 事件订阅 | **不克隆** | 用户在 BindItem 里自行绑定 |
- **id 作用域**：克隆产生 N 份重复 id。slot 根需作为 `Get<T>` 查找边界，但**不能**直接打 `SCOPE_ROOT` —— 见 §6.2。
- **`<template>` 源**：需先完成 §6.1 的 pkg 改造，template 才真实存在于场景树中。

### 6.1 `<template>` 进 pkg（阻断前置，v26 → v27）

现状下 template 子树在**打包期**就被丢弃，运行时无任何痕迹。本轮收进 pkg，改动如下：

1. **`NodeKind::Template` 新增**（`crates/core/src/scene/node.rs`）：**枚举末尾追加**，保证已有变体的 u8 判别值不变（node.rs:120-140 的映射同步追加）。
2. **bridge 不再跳过**（`crates/packer/pkg/src/bridge.rs:34`）：删 `is_in_template_subtree → continue`；`map_semantic` 的 `SemanticKind::Template` 从 `Err` 改为 `Ok(NodeKind::Template)`。template 及其子树正常序列化进 pkg。
3. **template 子树不参与布局/渲染/命中**：`NodeKind::Template` 强制 `display:none` 语义 —— layout 阶段滤出 taffy 树，render 不产 node，hit 不命中。**其子树也整体不参与**（不仅根节点）。这是它与普通 `display:none` 的关键差异需在实现时逐层校验。
4. **template 子树不参与 cascade**：rematch 跳过 Template 子树（模板是蓝图，不该因 hover/focus 而变）。克隆出的 slot 才参与。
5. **打包期校验**：template 根必须是单个 `li`（public-api 已规定；现 packer 无此校验，本轮补）。
6. **版本 bump**：`PKG_FORMAT_VERSION` 26 → 27，`MIN_VERSION`/`MAX_VERSION` 同步（`crates/core/src/asset/mod.rs:23-25`）。
7. **bump 连带链（坑 158 stale exe 同源，逐项勾）**：重编 release dll → 拷 `unity/package/Plugins/LoomGUI/` → `sync-bindings` → **重出 GUI exe 并拷 `unity/package/Editor/Tools/loomgui_gui.exe`**（tech-debt 已记「GUI exe 拷贝滞后」，本轮必须清）→ 重打 showcase pkg。

### 6.2 拆分 `SCOPE_ROOT` 双语义（阻断前置）

**问题**：`NodeFlags::SCOPE_ROOT`（node.rs:23）同时承担两个无关职责——（a）`Get<T>` 查找边界，（b）CSS scoped 规则隔离。页面 CSS 经 `instantiate`（stage.rs:779）包成 `ScopedRule { scope_root: 页面根 }`，rematch 按 `scope_root != node_scope → continue` 过滤（`style/dynamic.rs:513`）。若 slot 根打 `SCOPE_ROOT`，slot 内所有节点的 `node_scope` 变成 slot 根 ≠ 页面根 → **页面全部 class 规则对 item 不命中，item 裸奔无样式**。这是「per-task review 全绿、集成后 item 没样式」型的跨层缺口。

**修法**：拆成两个独立 flag。

- `SCOPE_ROOT`（`1 << 5`，语义收窄）：**仅** CSS scoped 规则隔离。仅页面根 / 组件实例化根打。slot 根**不打**。
- `LOOKUP_SCOPE`（`1 << 6`，新增）：**仅** `Get<T>` 查找边界。slot 根打此位；现有打 `SCOPE_ROOT` 的节点**同时**打 `LOOKUP_SCOPE`（保现有行为不变）。

**连带修改点**（逐个实现时核对读的是哪个 flag）：

| 位置 | 该读哪个 |
|---|---|
| `compute_scope_map`（`style/dynamic.rs:443`）| `SCOPE_ROOT`（CSS）|
| `parent_in_scope`（`style/dynamic.rs:433`）| `SCOPE_ROOT`（后代选择器边界）|
| rematch scope 校验（`style/dynamic.rs:513`）| `SCOPE_ROOT` |
| `remove_node` 的 `was_scope_root` 清理（`scene/dynamic.rs:506,550`）| **两者均需处理** |
| `Get<T>` / FFI 按 id 查找 | `LOOKUP_SCOPE` |

此拆分同时补上 `LoomGUI.Nodes.cs:190` 记的 scope 查找 gap。

### 6.3 模板来源解析

（首次进入数据驱动时执行一次，结果存 `template_root`）：

1. 用户显式 `ItemTemplate` / `TemplateSelector`；
2. ul 下恰好一个 `<template id>` → 自动采用；多个 → `UIContractException`；
3. 兜底：第一个设计期 `li` —— **先 clone_subtree 备份到游离态，再清空所有设计期 li**（顺序颠倒则模板丢失）。

**`UITemplate` 重定义**：内部由「pkg + component path」扩为 enum `PackageComponent{pkg,path} | SceneSubtree{node_id}`；公共签名不变（仍是不透明句柄）。`Node.GetTemplate(id)` 从 `NE()` 实装为「按 id 找 `<template>` 返回 SceneSubtree 句柄」。`TemplateSelector` 返回的模板若与 slot 当前模板不同 → 弃用该 slot 重新克隆。

## 7. 帧时序

```
【C# 侧，tick 之前】
  drain_pending_binds()      ← 取 core 队列，逐条执行 BindItem(item, index)，数据写回 core
【core tick】
  tween.advance
  scroll.update
  process_keys / 事件         （hit 用上帧 world）
  *** list.update_visible()   ← 算可见区、回收/复用/克隆 slot、更新 spacer、入队 pending_binds
  rematch_pseudo_classes
  transition drain
  solve                      （一次）
  *** list.collect_heights()  ← 回填 known[i]；头部区间总高变化 → 补偿 scroll_pos（anchoring）
  refresh_content_sizes
  compute_world_transforms
  build_render_nodes
```

`update_visible` 在 solve **之前**（新克隆 slot 本帧即被布局，不闪空白），但其入队**下一帧**才 bind。故新进入可见区的 item 第一帧显示模板原样/上一复用者内容——这是无同步回调方案的固有代价。

**对策与诚实表述**：`BUFFER=2` 使 item 在进入可见区**前**即被克隆入队，慢速滚动下 bind 已完成。但一次滚轮 tick 常跨几十至上百 px，不等高列表里一帧跨 3~5 项很常见——**快速滚动下确实会出现一帧旧内容，本 spec 接受这个代价**（不声称「仅瞬移命中」）。自适应 BUFFER（按上帧滚速扩大预取）列为已知优化点，触发判据：Unity 真机快滚时旧内容肃动可见。

另外两个入口必须**同帧强制排空**：`ScrollToItem`（跨大距离）与首次 `ItemCount` 赋值。注意：**单纯排队是 no-op** —— `drain_now` 在 tick 外调用，此时目标区间的 slot 还未克隆（克隆发生在 tick 内的 `update_visible`），队列是空的。故 **`drain_now` 必须先同步跑一次 `update_visible`（含克隆/回收/spacer 更新）再排空**，否则就是个静默无效的 API。

## 8. FFI 契约

`crates/ffi/src/lib.rs` 新增（SOA / ptr+len 风格，enum 一律 `#[repr(u8)]`，ABI struct 加 `size_of` 断言）：

- `loomgui_list_set_item_count(stage, node, count) -> i32`
- `loomgui_list_set_template(stage, node, template_node) -> i32`
- `loomgui_list_refresh(stage, node, start, count) -> i32`（RefreshItem/RefreshItems 共用）
- `loomgui_list_notify(stage, node, op: u8, a: i32, b: i32) -> i32`（Inserted/Removed/Moved 三合一；core 侧搬移 heights、修正 slot.item_index、保持 scroll 锚定）
- `loomgui_list_scroll_to(stage, node, index, behavior: u8) -> i32`
- `loomgui_list_take_pending_binds(stage, out_nodes: *mut u32, out_indices: *mut i32, cap: u32, out_len: *mut u32) -> i32`（cap 不足则分批）
- `loomgui_list_drain_now(stage, node) -> i32`（**内部先跑 `update_visible` 再排空**，见 §7）

C# 侧 `ListView` 投影持有 `Action<ListItem,int> BindItem`；`UIContext` 在 tick 前统一 `take_pending_binds` 并按 node 分发至对应 ListView。

## 9. 公共 API 范围

**实装**：`ItemCount`、`ItemTemplate`、`TemplateSelector`、`BindItem`、`ScrollToItem`、`RefreshItem`、`RefreshItems`、`NotifyInserted`、`NotifyRemoved`、`NotifyMoved`。

**额外必须实装（review 捕获的漏项）**：

- **`ChildCount` 覆写**：public-api 规定数据驱动下 `ChildCount == ItemCount`，但 core 侧 ul 真实 children = 2 spacer + N slot。C# 不得直走 `get_child_count`，需在 `ListView` 覆写为返回 `ItemCount`。
- **`ListItem.Index`**（`LoomGUI.Nodes.cs:1270`，当前 `NE()`）：BindItem 后用户读 `item.Index` 是自然用法，本轮实装（读 slot 对应的 `item_index`）。

**保留签名 + `NE()`**：`ItemExitClass`（依赖 M2 的 `AnimationEnd`，M2 到位即实现，非死 API）。

**删除（破坏性变更）**：`SelectedIndex`、`SelectionChanged`。理由（按分量排序）：

1. **与 Dropdown 同名不同义，污染语义**：Dropdown 的 `SelectedIndex` 背后是 HTML `select/option` 的**真选中语义**，而 `ul/li` **无任何 HTML 选中语义**。凭空长出选中态违背「标准 HTML 语义决定行为」，是文档与 AI 预测的污染源；
2. **虚拟化下是陷阱**：选中态必须存于数据侧（item 会被回收），框架维护 index 等于替用户管影子状态，而真实需求常为多选/范围选/按 id 选，单 index 模型覆盖不了，用户会绕过它 → 死 API；
3. **纯便利糖，零架构含量**（用户在 BindItem 里按自己的选中模型贴 class 即可）。

已核实：`tests/dotnet` 与 `Runtime` 内 ListView 的这两个成员**零引用**；`SelectionChangedEvent` 结构体由 Dropdown 独立使用，**不受影响**。实现全为 `NE()`、无真实用户，零迁移成本。

**未来落点**（写明以防日后被当作遗漏加回）：真需选中语义时走 `role="listbox"` / `aria-selected`，依赖 P4（Node attrs 存储 + attr_matches_node 扩展 + role dispatch）基建，**不得**把 index 塞回 ListView。

同步修改：

- `docs/design/public-api.md` §8 与判据 17；**另 public-api.md:339 的「`ul/ol → ListView`」需改为仅 `ul`**（`ol` 已不在围栏）；
- `unity/package/Runtime/Public/LoomGUI.Nodes.cs:2340-2341`；
- `tests/dotnet/LoomGUI.PublicApi` 相关引用；
- **`docs/roadmap/milestones.md` 的 M1 退出判据**（当前写着 `ul/ol` 且要求实装 `SelectedIndex`，与本决策直接冲突，必须同步）。

## 10. 错误处理

一律 `UIContractException`（围栏外明确失败，不静默降级）：

- 数据驱动模式下 `AddChild`/`InsertChild`/`RemoveChild`/读 `Children`；
- ul 下多个 `<template>` 且未设 `TemplateSelector`；
- 无任何模板来源（无 template、无设计期 li、未设 ItemTemplate）；
- `ItemCount` 为负；`ScrollToItem`/`RefreshItem`/`NotifyMoved` index 越界。

**不抛**：无祖先 ScrollPane → 退化全量渲染 + 一次性运行时警告。

## 11. 测试

**core 单测**（`cargo test -p loomgui_core`）：HeightCache 求和/回填/估算收敛；可见区区间（buffer 裁剪、边界项、item_count=0/1、**首帧 viewport=0 冷启动**）；slot 差集复用配对；`Notify*` 对 heights 与 slot.item_index 的搬移；anchoring 补偿量数值断言；**`clone_subtree` 的 side table 覆盖测试**（逐条核对 §6 那张表：controls 克隆且复用时 reset、scroll/anim/EditState 不克隆）；**`reuse_key` 编码**（恒 ≠ 0、多 ListView 不撞）；**margin box 高度语义**（带 margin 的 li 求和正确）。

**headless 端到端**（`tests/dotnet/LoomGUI.HeadlessTests`）：

- **虚拟化本质断言**：`ItemCount=1000` 与 `ItemCount=10000` 的 render node 数**相等**（两数据点相等，非阈值）；
- content_size ≈ 全量高度，滚动条比例正确；
- 滚到底 → 末项可见、tail spacer 高度为 0；
- **不等高无漂移**：混合高度数据集（**含 margin**），从头滚到底再滚回头，首项 y 与初始一致（anchoring 无累积漂移）；
- `NotifyInserted/Removed` 后滚动位置与可见内容不跳；
- **页面 CSS 对 item 生效**（验 §6.2 拆 flag 正确：页面级 `.item-title{color:red}` 命中克隆出的 slot 内节点）；
- **`ul` 非 auto 高度 → 抛 `UIContractException`**（不静默失效）；
- **`<template>` 不渲染不命中**（进了 pkg 但不产 render node、hit 不命中）。

**Unity PlayMode**（defer 家里机）：mail 页（不等高）+ inventory 页（等高网格）真机滚动——复用无鬼影、无闪烁、GameObject 数稳定。

## 12. 交付顺序

> **dll 重编纪律**：步骤 4/6 的 headless 断言已依赖新 FFI，故**每加一批 FFI 就得立即**重编 release dll + 拷 `unity/package/Plugins/LoomGUI/` + `cargo run -p xtask -- sync-bindings`，不得推到最后（坑 158 stale 链同源）。

0. **`<template>` 进 pkg**（§6.1）：`NodeKind::Template` 末尾追加 + bridge 不再跳过 + display:none / 不参与 cascade 语义 + 打包期根为 `li` 校验 + **v26→v27 bump 全链**（dll 重编 + sync-bindings + **GUI exe 重出并拷贝** + showcase 重打）
1. **拆 `SCOPE_ROOT` / `LOOKUP_SCOPE`**（§6.2）+ 四个连带点逐个核对 + 回归测试（现有页面 CSS 行为不变）
2. `clone_subtree` + side table 逐条判定 + FFI + 单测
3. `ListState`/`ListTable`/`HeightCache` 骨架 + 可见区算法单测（纯逻辑，不接树）
4. 接树：spacer 生成（含 margin 折叠阻断 + display 分支的 gap）、slot 池、`reuse_key` 编码、ul 高度校验、tick 挂钩 → headless「不随总数增长」绿
5. `pending_binds` 队列 + FFI + C# 投影（ItemCount/ItemTemplate/TemplateSelector/BindItem/ChildCount/ListItem.Index）
6. 不等高：margin box 回填 + anchoring + tween 交互豁免 → headless「滚回头无漂移」绿
7. `ScrollToItem`（含 drain_now 先跑 update_visible）/ `RefreshItem(s)` / `Notify*` + 异常契约
8. 公共 API 缩减 + public-api.md / milestones.md 同步 + 末次 dll 入库

## 13. 不在本 spec 内

- `ItemExitClass` 实现（等 M2 `AnimationEnd`）
- 选中语义（未来走 role/aria，P4 线）
- Fenwick 树优化（触发判据：sum 占 tick > 5%）
- 自适应 BUFFER（触发判据：真机快滚时旧内容肃动可见）
- **横向 / 网格虚拟化**：若 inventory 为 wrap 网格，按**行**虚拟化、行内全量；纯横向列表不在本轮范围。
- **结构伪类必须跳过 spacer（对未来的约束）**：现 core 无 `:nth-child`/`:first-child`，spacer 插入 ul 暂无害。但结构伪类在视觉束落地时，`li:nth-child(odd)` 的斑马纹会因头 spacer 占据第一个子位而整体错位，**且随滚动位置变化而闪烁**。实现结构伪类时：索引计算必须跳过 spacer，且虚拟化下应基于 **item_index** 而非真实子节点位置。
