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
- tick 入口 `Stage::tick_and_render`（`crates/core/src/stage.rs:818`）。

## 3. 关键决策

| 决策 | 选择 | 理由 |
|---|---|---|
| 虚拟化层次 | **全在 Rust core** | 跨引擎共享核心是差异化卖点；bind 过桥频率低（仅进出可见区） |
| item 定位 | **可见 item 走正常 CSS 流 + 头/尾 spacer** | 对 CSS 语义透明（gap/margin/padding 天然生效），不在 core 复刻盒模型；护城河 = 布局可预测 |
| 不等高 | **估算 + 实测回填 + scroll anchoring** | 一次做完，不分步 |
| bind 过桥 | **pending 队列 + 帧首排空**（无跨 FFI 同步回调） | 异常安全，符合攒批回写范式，不破坏「每帧一次 solve」 |
| 模板克隆 | **新增 `clone_subtree`（场景级）** | 复用 instantiate 的重建逻辑；不动 pkg 格式（免 bump / 免重出 GUI exe）；以后 slot / Custom Element 共用 |
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

**gap 修正**：taffy 会在 spacer 与相邻 item 之间也施加 `row_gap`。spacer 高度需扣掉相应 gap（当 spacer 高度 > 0 且两侧有兄弟时各扣一个 gap）。这是 core 唯一需要读 CSS 值的地方（读 `ResolvedStyle.row_gap`），不是重新实现盒模型。

**与 ScrollPane 的关系**：ListView 自己不滚。滚动仍由祖先 `overflow:auto` 容器负责，ListView 只是其内容。spacer 撑满全量高度 ⇒ `refresh_content_sizes` 的自然计算即正确，**不使用 `set_content_size` 逃生舱**（旧 driver 补丁随本 spec 退役）。找不到祖先 ScrollPane 时退化为全量渲染 + 一次性运行时警告（不报错——短列表无滚动容器合法）。

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

**可见区计算**（帧首，用上一帧 `scroll_pos`，与「hit 用上帧 world」同一时序哲学）：

```
top    = scroll_pos.y - listview_offset_in_content
first  = max i  where sum(0..i) <= top
last   = min j  where sum(0..j) >= top + viewport.h
visible = [first - BUFFER, last + BUFFER] ∩ [0, item_count)
```

`BUFFER = 2`，内部常量，不暴露为 API。

**slot 回收/复用**：新旧 visible 求差集——离开的 slot 入空闲池；进入的 index 优先从池取（取不到才 `clone_subtree`），配对时**优先复用同侧 slot**（滚动方向连续复用，减少 mesh churn）。复用即 `pending_binds.push((node, new_index))`。`reuse_key` 设为 **slot 序号**（非 item index），契合现有 `render/dirty.rs` header_hash 复用机制，令 Unity 侧 GameObject 稳定不重建。

## 6. 模板与 clone_subtree

`Stage::clone_subtree(src: NodeId) -> NodeId`（`crates/core/src/stage.rs`，与 `instantiate` 并列）：深拷贝 `kind` / `classes` / `id_attr` / `base_style` / 文本 / img src / 控件初值，递归子树，返回**游离**新根（不挂树，调用方 append）。

- **不克隆**：EditState、ScrollPaneState、tween 状态、事件订阅。克隆是结构 + 样式，不是运行时状态。
- **id 作用域**：克隆产生 N 份重复 id。每个 slot 根标记为 **scope root**，`Get<T>("id")` 在 ListItem 边界内停止（本 spec 至少实装 ListItem 边界那一部分，补上 `LoomGUI.Nodes.cs:190` 的 gap）。
- **`<template>` 源**：`display:none` 使其不渲染不布局，但**必须仍在场景树中**作为克隆源。打包期校验 template 根是单个 `li`（若 packer 侧缺此校验则补）。

**模板来源解析**（首次进入数据驱动时执行一次，结果存 `template_root`）：

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

**对策**：`BUFFER=2` 使 item 在进入可见区**前**即被克隆入队，正常滚速下 bind 早已完成，不可见。仅「瞬移式跳转」命中——因此 `ScrollToItem`（跨大距离）与首次 `ItemCount` 赋值**显式同帧强制排空** bind 队列。精确例外，非每帧特判。

## 8. FFI 契约

`crates/ffi/src/lib.rs` 新增（SOA / ptr+len 风格，enum 一律 `#[repr(u8)]`，ABI struct 加 `size_of` 断言）：

- `loomgui_list_set_item_count(stage, node, count) -> i32`
- `loomgui_list_set_template(stage, node, template_node) -> i32`
- `loomgui_list_refresh(stage, node, start, count) -> i32`（RefreshItem/RefreshItems 共用）
- `loomgui_list_notify(stage, node, op: u8, a: i32, b: i32) -> i32`（Inserted/Removed/Moved 三合一；core 侧搬移 heights、修正 slot.item_index、保持 scroll 锚定）
- `loomgui_list_scroll_to(stage, node, index, behavior: u8) -> i32`
- `loomgui_list_take_pending_binds(stage, out_nodes: *mut u32, out_indices: *mut i32, cap: u32, out_len: *mut u32) -> i32`（cap 不足则分批）
- `loomgui_list_drain_now(stage, node) -> i32`

C# 侧 `ListView` 投影持有 `Action<ListItem,int> BindItem`；`UIContext` 在 tick 前统一 `take_pending_binds` 并按 node 分发至对应 ListView。

## 9. 公共 API 范围

**实装**：`ItemCount`、`ItemTemplate`、`TemplateSelector`、`BindItem`、`ScrollToItem`、`RefreshItem`、`RefreshItems`、`NotifyInserted`、`NotifyRemoved`、`NotifyMoved`。

**保留签名 + `NE()`**：`ItemExitClass`（依赖 M2 的 `AnimationEnd`，M2 到位即实现，非死 API）。

**删除（破坏性变更）**：`SelectedIndex`、`SelectionChanged`。理由：
- 纯便利糖，零架构含量（用户在 BindItem 里按自己的选中模型贴 class 即可）；
- 虚拟化下是陷阱——选中态必须存于数据侧（item 会被回收），框架维护 index 等于替用户管影子状态，而真实需求常为多选/范围选/按 id 选，单 index 模型覆盖不了，用户会绕过它 → 死 API；
- 与 Dropdown 的 `SelectedIndex` 同名不同义：Dropdown 背后是 HTML `select/option` 的真选中语义，`ul/li` **无任何 HTML 选中语义**。凭空长出选中态违背「标准 HTML 语义决定行为」，污染文档与 AI 预测。

**未来落点**（写明以防日后被当作遗漏加回）：真需选中语义时走 `role="listbox"` / `aria-selected`，依赖 P4（Node attrs 存储 + attr_matches_node 扩展 + role dispatch）基建，**不得**把 index 塞回 ListView。

同步修改：`docs/design/public-api.md` §8 与判据 17、`unity/package/Runtime/Public/LoomGUI.Nodes.cs:2340-2341`、`tests/dotnet/LoomGUI.PublicApi` 相关引用。实现全为 `NE()`、无真实用户，实际零迁移成本。

## 10. 错误处理

一律 `UIContractException`（围栏外明确失败，不静默降级）：

- 数据驱动模式下 `AddChild`/`InsertChild`/`RemoveChild`/读 `Children`；
- ul 下多个 `<template>` 且未设 `TemplateSelector`；
- 无任何模板来源（无 template、无设计期 li、未设 ItemTemplate）；
- `ItemCount` 为负；`ScrollToItem`/`RefreshItem`/`NotifyMoved` index 越界。

**不抛**：无祖先 ScrollPane → 退化全量渲染 + 一次性运行时警告。

## 11. 测试

**core 单测**（`cargo test -p loomgui_core`）：HeightCache 求和/回填/估算收敛；可见区区间（buffer 裁剪、边界项、item_count=0/1）；slot 差集复用配对；`Notify*` 对 heights 与 slot.item_index 的搬移；anchoring 补偿量数值断言；`clone_subtree` 深拷贝完整性 + 不拷 EditState。

**headless 端到端**（`tests/dotnet/LoomGUI.HeadlessTests`）：

- **虚拟化本质断言**：`ItemCount=1000` 与 `ItemCount=10000` 的 render node 数**相等**（两数据点相等，非阈值）；
- content_size ≈ 全量高度，滚动条比例正确；
- 滚到底 → 末项可见、tail spacer 高度为 0；
- **不等高无漂移**：混合高度数据集，从头滚到底再滚回头，首项 y 与初始一致（anchoring 无累积漂移）；
- `NotifyInserted/Removed` 后滚动位置与可见内容不跳。

**Unity PlayMode**（defer 家里机）：mail 页（不等高）+ inventory 页（等高网格）真机滚动——复用无鬼影、无闪烁、GameObject 数稳定。

## 12. 交付顺序

1. `clone_subtree` + FFI + 单测
2. `ListState`/`ListTable`/`HeightCache` 骨架 + 可见区算法单测（纯逻辑，不接树）
3. 接树：spacer 生成、slot 池、tick 挂钩 → headless「不随总数增长」绿
4. `pending_binds` 队列 + FFI + C# 投影（ItemCount/ItemTemplate/TemplateSelector/BindItem）
5. 不等高：回填 + anchoring → headless「滚回头无漂移」绿
6. `ScrollToItem`/`RefreshItem(s)`/`Notify*` + 异常契约
7. 公共 API 缩减 + `docs/design/public-api.md` 同步 + dll 重编入库（`cargo build -p loomgui_ffi_c --release` → 拷 `unity/package/Plugins/LoomGUI/` → `cargo run -p xtask -- sync-bindings`）

## 13. 不在本 spec 内

- `ItemExitClass` 实现（等 M2 `AnimationEnd`）
- 选中语义（未来走 role/aria，P4 线）
- Fenwick 树优化（触发判据：sum 占 tick > 5%）
- **横向 / 网格虚拟化**：若 inventory 为 wrap 网格，按**行**虚拟化、行内全量；纯横向列表不在本轮范围。
