# Layout 层代码审查报告

**文件**：`loomgui_core/src/layout/mod.rs`（706 行）
**审查日期**：2026-07-09
**审查范围**：taffy flexbox 与场景图的集成层——树构建、CSS→taffy 映射、measure 闭包、结果回写、错误处理、性能、注释

---

## 总体评价

代码结构清晰、注释质量高（模块级 doc 完整覆盖 API 边界和设计决策）、CSS→taffy 属性映射正确、Image measure 三档优先级逻辑严密、overflow 传播正确。未发现正确性 bug。发现若干性能浪费和琐碎改进点，无阻塞项。

---

## 发现列表

### 1. display:none 子树仍被整棵送入 taffy 树（性能浪费）

- **行号**：182–186, 122
- **代码片段**：
  ```rust
  let children_ids: Vec<taffy::NodeId> = node
      .children
      .iter()
      .map(|c| build(scene, tree, taffy_ids, *c, self_overflow, image_sizes))
      .collect();
  ```
- **问题分析**：`build` 闭包不检查 `style.display == taffy::Display::None`，对所有节点无条件递归构建子节点。虽然 taffy 对 `Display::None` 节点会内部跳过布局计算（产零尺寸），但构建 taffy 树的开销（节点插入、measure 闭包触达、layout 遍历）完全浪费。渲染层已通过 `collect_display_none_subtree` 正确过滤（`render/mod.rs:43`），layout 层应同步跳过。
- **修复方向**：在 `build` 开头检查 `node.style.display_mode == DisplayMode::None` 或 `style.display == taffy::Display::None`，直接创建叶子节点返回（不递归子节点），或更彻底地：在 `solve` 入口前由调用方（`stage.rs`）剪枝 display:none 子树。
- **严重级别**：中（性能，非正确性）

### 2. text_layouts "Some 优先"策略依赖 taffy 内部测量序

- **行号**：300–304
- **代码片段**：
  ```rust
  if let Some(sid) = taffy_to_scene.get(&nid) {
      let slot = &mut text_layouts[sid.index()];
      if slot.is_none() || known.width.is_some() {
          *slot = Some(layout.clone());
      }
  }
  ```
- **问题分析**：存储策略是"首次 Some(available) 测量结果覆盖 None 结果，后续 None 不覆盖已有 Some"。依赖 taffy 先调用 `None`（max-content 测），后调用 `Some(available)`（约束宽测）的内部实现顺序。若 taffy 未来改变测量序或只调用一次，短文本的换行结果可能丢失。注释已说明此依赖（"短文本 taffy 可能只传 None……长文本传 Some……一旦存了 Some，后续 None 不覆盖"），但仍属脆弱假设。
- **修复方向**：方案 A—改为"始终存最后一次测量结果"，由 taffy 的最后一次调用决定最终尺寸（taffy 的最终测量结果即为布局使用的值）。方案 B—保留当前策略但加一条：若最终 `layout.size.width` 与 `text_layouts` 中的 `text_width` 差异显著，用 `layout.size.width` 重新测量换行。
- **严重级别**：低-中（可维护性/鲁棒性）

### 3. 魔数 64×64 图像兜底尺寸

- **行号**：170
- **代码片段**：
  ```rust
  .unwrap_or((64.0, 64.0));
  ```
- **问题分析**：图像尺寸表缺失或 w/h=0 时的兜底值硬编码为 `(64.0, 64.0)`，在代码中（行 170）和多处测试注释中重复出现。无命名常量，无集中定义。
- **修复方向**：提取为文件级 `const IMAGE_FALLBACK_SIZE: (f32, f32) = (64.0, 64.0);`，测试中也引用同一常量。
- **严重级别**：低（可维护性）

### 4. 死字段 `Node.taffy_id`

- **行号**：`scene/node.rs:96`
- **代码片段**：
  ```rust
  pub taffy_id: Option<taffy::NodeId>,
  ```
- **问题分析**：grep 全项目 0 次引用。taffy 树每帧从零重建于 `solve()` 局部变量 `taffy_ids`（`Vec<Option<taffy::NodeId>>`），场景节点上的 `taffy_id` 从未被写入或读取。字段占 8 字节/节点，序列化/反序列化时产生无意义数据（pkg 存它浪费空间）。
- **修复方向**：移除字段（需同步改 `build_scene` / `Scene::build` / `Default for Node` 等构造点），确认无外部序列化兼容性需求。
- **严重级别**：低（死代码/浪费）

### 5. `taffy_ids` 和 `text_layouts` 按 `capacity()` 而非 `len()` 分配

- **行号**：111, 216
- **代码片段**：
  ```rust
  let mut taffy_ids: Vec<Option<taffy::NodeId>> = vec![None; scene.nodes.capacity() + 1];
  let mut text_layouts: Vec<Option<TextLayout>> = vec![None; scene.nodes.capacity() + 1];
  ```
- **问题分析**：注释说明"按容量分配是因为 remove_node 后 idx 不变但存活数减少"——用 `capacity` 保安全正确。但 slotmap 惰性收缩导致 `capacity` 可能远大于实际节点数（反复增删后未触发 compact），每帧分配大量 `None` 条目。实际场景节点数通常远小于 capacity 时，有内存/遍历浪费。
- **修复方向**：当前方案在正确性和简单性间已是合理取舍。届时若 profiling 显示 slotmap 膨胀严重，可改为"按 max(slotmap 最大 key, len) + 1 分配" 或定期 compact。
- **严重级别**：低（理论浪费，需 profiling 证实）

### 6. `taffy_to_scene` HashMap 过度构建

- **行号**：210–215
- **代码片段**：
  ```rust
  let mut taffy_to_scene: HashMap<taffy::NodeId, NodeId> = HashMap::new();
  for n in scene.nodes.values() {
      if let Some(tid) = taffy_ids[n.id.index()] {
          taffy_to_scene.insert(tid, n.id);
      }
  }
  ```
- **问题分析**：HashMap 为所有节点构建双向映射，但仅在 Text/RichText 的 measure 闭包中使用（存 text_layouts）。一个包含 5000 个 div 和 50 个 text 的场景，浪费 4950 条映射的内存和构建 CPU。此外，measure 闭包中按 taffy NodeId 做 HashMap 查表（含 hash + 探测）不如直接用 `taffy_ids` 反查：taffy NodeId 转换为 usize index 再查 `taffy_ids` 数组取 scene NodeId。但 taffy NodeId 是 slotmap 的 DefaultKey（64 bit），不能直接做数组索引。
- **修复方向**：场景节点数千量级，HashMap 开销可接受，暂不改。若 profiling 显示热点，可改为在 measure 闭包传一个并行数组 `scene_ids_by_taffy_index`（taffy 的 slotmap index → scene NodeId）。
- **严重级别**：低（理论浪费）

### 7. `write_back` 中 children vec 克隆

- **行号**：362
- **代码片段**：
  ```rust
  let kids = node.children.clone();
  for c in kids {
      write_back(scene, tree, taffy_ids, c, (x, y));
  }
  ```
- **问题分析**：克隆 `children: Vec<NodeId>` 仅为避开 `scene` 的借用冲突（`scene.get_mut` 借了 &mut scene，不能再 `scene.nodes.get(c).children`）。每次 solve 为每节点分配一次 Vec。可改为先收集 children 引用再遍历，或用 index 访问。
- **修复方向**：改为 `let kids: Vec<NodeId> = node.children.iter().copied().collect();`（语义等价），或结合 NodeId Copy 用 `let kids = node.children.clone()` 已是够轻量的方案。不改亦可。
- **严重级别**：低

### 8. 根节点 style 克隆-重组

- **行号**：220–232
- **代码片段**：
  ```rust
  let root_style = taffy_tree.style(root_tid).unwrap().clone();
  taffy_tree
      .set_style(
          root_tid,
          Style {
              size: Size {
                  width: Dimension::Length(root_size.0),
                  height: Dimension::Length(root_size.1),
              },
              ..root_style
          },
      )
      .ok();
  ```
- **问题分析**：从 taffy 读回整个 Style 再 clone 重组，仅改 `size` 字段再写回。taffy 不提供部分更新 API，故只能整取-改-整设。taffy `Style` 约 200 字节，开销极小。注释已在 `solve` 文档（行 91 "root_size 是根节点固定尺寸"）说明意图。
- **修复方向**：无需改。若要极致优化，可 memory-access `TaffyTree` 内部（private）style 数组——不推荐。
- **严重级别**：信息（无问题）

### 9. Leaf 节点不验证 children 为空

- **行号**：188–194
- **代码片段**：
  ```rust
  let tid = if let Some(mctx) = ctx {
      tree.new_leaf_with_context(style, mctx).unwrap()
  } else {
      tree.new_with_children(style, &children_ids).unwrap()
  };
  ```
- **问题分析**：当 Text/Image/RichText 节点有 children 时，`children_ids` 非空，但 `new_leaf_with_context` 传入的 style 会被 taffy 拒绝（taffy leaf 不允许有 children）→ `unwrap()` panic。在正常流程中不会发生（parser 保证 Text/Image <RichText(block div)> 是叶子），但若动态树 API 不守契约会触发。
- **修复方向**：debug_assert 校验，或 `new_leaf` 前 assert `children_ids.is_empty()` 提供更清晰的 panic 信息。
- **严重级别**：信息（内部不变量，正常不会触发）

### 10. `unwrap()`/`expect()` 用法审查

- **行号**：121, 190, 193, 220, 351, 352, 356
- **分析**：
  - 行 121 `scene.get(id).expect("live node")` — 内部不变量，若破说明场景损坏，panic 合理
  - 行 190 `tree.new_leaf_with_context(style, mctx).unwrap()` — taffy 仅在 OOM/重复 id 时 Err，正常不会
  - 行 193 `tree.new_with_children(style, &children_ids).unwrap()` — 同上
  - 行 220 `taffy_tree.style(root_tid).unwrap().clone()` — 刚插入的 root，必存在
  - 行 351 `taffy_ids[id.index()].unwrap()` — 所有节点在 build 阶段已插入
  - 行 352 `tree.layout(tid).unwrap()` — compute_layout 已执行
  - 行 356 `scene.get_mut(id).expect("live node")` — 同上，递归遍历 live 树
- **结论**：所有 unwrap/expect 都有不变量保护，无滥用。唯一建议：行 190/193 的 unwrap 可加 expect 信息便于诊断。
- **严重级别**：信息（无问题）

---

## CSS 属性 → taffy 映射覆盖度审查

`ResolvedStyle.taffy_style` 在 cascade 阶段已填好所有 taffy 字段（`Style` 约 40+ 字段），layout 层直接 `clone()` 使用。唯一在此层额外映射的字段：

| CSS 属性 | taffy 字段 | 映射位置 | 正确性 |
|---------|-----------|---------|-------|
| `overflow-x/y` | `style.overflow.{x,y}` | 行 126–129 | 正确（Auto→Scroll 因 taffy 0.5 无 Auto） |
| `flex_shrink` | 子节点 `style.flex_shrink = 0.0` | 行 132–134 | 正确（overflow 容器子项不缩） |
| `width/height` | 根节点 `style.size` | 行 225–228 | 正确（覆盖为 root_size） |
| `display` | `style.display` | 已在 cascade 填（`mapping.rs:482`） | 正确（None→taffy 展平） |
| `position` | `style.position` | 已在 cascade 填 | 正确（relative/absolute 透传） |
| `flex_direction` | `style.flex_direction` | 已在 cascade 填（默认 Column） | 正确 |
| `gap`, `padding`, `margin`, `border` 等 | 各字段 | 已在 cascade 填 | 正确（无需 layout 层再映射） |

**未映射的属性**：
- `order`（`ResolvedStyle.order`）— taffy 0.5 无此字段，渲染层按 DOM 序 + `Layout.order` 排序（行 20–21 注释说明）
- `display_mode: Block` — 仅作旁路标记供 desugar 识别（`resolved.rs:48-56`），taffy 侧仍为 Flex

**结论**：映射覆盖完整，无遗漏。

---

## 边界 case 分析

### position:absolute/relative

`taffy_style.position` 由 cascade 填空（`mapping.rs`），layout 层原样透传。taffy 0.5 原生支持 `Absolute`/`Relative`，absolute 元素脱离正常流、相对于含 `position != Static` 的最近祖先定位。CSS inset（top/right/bottom/left）在 taffy 中即 `Style.inset`，由 cascade 填写，layout 无需特殊处理。
结论：**正确**。

### overflow:Auto → Scroll 映射（行 46）

CSS `overflow:auto` 只在溢出时显示滚动条，`overflow:scroll` 总是显示。但在布局语义上，两者对 flexbox 的 `min-size` 默认行为相同（都设 min-size=0）。实际滚动条的显示与否由 `scroll` 模块的 `effective()` 函数运行时判断（`overflow_y != Visible && content > viewport`），不依赖此映射。
结论：**正确**。

### display:none 的布局行为

`display: none` → `taffy_style.display = Display::None`。taffy 对此节点的处理：不参与布局（不占空间），children 同样跳过。layout 回写时 `layout_rect` 为 taffy 返回的值（零尺寸）。渲染层额外过滤（`collect_display_none_subtree`）。
结论：**正确**，但性能可优化（见发现 1）。

### 根节点裁剪

Scene::build 按 `overflow_x/y != Visible` 决定 `clip_rect = Some(Rect::default())`。solve 的 write_back 用自身 border 框填充 clip_rect（行 359–361）。根节点的 clip 正确传播。
结论：**正确**。

### 空 Scene

`scene.roots.is_empty()` → 立即返回（行 105–107），避免 `roots[0]` 越界。文档注释说明"Stage 可能在 scene 未装内容时 tick"。
结论：**正确**。

---

## 性能热路径分析

| 操作 | 每帧开销 | 可优化 |
|------|---------|-------|
| taffy 树重建（全量） | O(n) 节点插入 + O(n·log n?) flexbox solve | 无（taffy 每次从零建，设计如此） |
| `taffy_to_scene` HashMap 构建 | O(n) 全节点遍历 + hash insert | 低优先级（见发现 6） |
| measure 闭包中 text_layout clone | O(k) k=文本节点数 | 低（TextLayout 较小，Arc 无必要） |
| write_back children clone | O(n) 每节点 1 次 Vec 分配 | 低（见发现 7） |
| `set_style` 根节点 style clone | 每帧 1 次 200B clone | 可忽略 |

---

## 总结

- **正确性**：无 bug。CSS→taffy 映射完整，overflow/position/display 处理正确，measure 闭包三档逻辑严密，错误处理合理。
- **性能**：3 处可优化点（display:none 子树、taffy_to_scene、children clone），均属低优先级。
- **可维护性**：1 处魔数、1 处脆弱假设、1 处死字段。
- **注释质量**：高。模块 doc 覆盖 taffy API 版本边界、生命周期约束、设计决策，内联注释准确。
- **测试覆盖**：7 个测试覆盖列布局默认值、Image 三档优先级、aspect ratio 推算。缺少 display:none 布局行为测试、overflow: scroll 容器 flex_shrink 测试。

**建议动作**（按优先级）：
1. display:none 子树跳过 taffy 树构建（发现 1）— 最大性能收益
2. 提取 64×64 魔数为常量（发现 3）
3. 移除死字段 `Node.taffy_id`（发现 4）
4. 其余为信息级，可择时处理
