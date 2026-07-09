# LoomGUI 渲染管线深度代码审查报告

## 总览

审查范围：`loomgui_core/src/render/` 全部文件（mod.rs、node.rs、mesh.rs、batch.rs、merge.rs、dirty.rs、tests.rs），共 ~6100 行 Rust。追踪了从 `build_render_nodes` → `assign_sort_keys` → merge 的完整流程，并深入审查了 ChangeLevel 双 hash 机制和九宫格/圆角矩形几何生成。

---

## 发现列表（按严重程度排序）

### 🔴 严重

#### 1. `NodePayload` 是死枚举（只有 `Mesh` 一个变体）

- **位置**：`node.rs:47-57`
- **代码**：
  ```rust
  pub enum NodePayload {
      Mesh { ... },
  }
  ```
- **分析**：v1.6 把 Text 变体合并进 Mesh（用 `program=1` 区分）后，`NodePayload` 退化为单变体枚举。枚举包装给匹配/序列化带来无意义的 `NodePayload::Mesh { ... }` 包裹层，增加：
  - 所有消费者必须写 `match &rn.payload { NodePayload::Mesh { ... } => ... }` 而非直接访问字段
  - serde 序列化多一层 `"Mesh": { ... }` JSON 键（~20 bytes per node）
  - `#[allow(unreachable_patterns, irrefutable_let_patterns)]` 的 lint 放行（如 render/tests.rs:1, dirty.rs:86, merge.rs:129）——编译器已提示这是冗余的
- **修复方向**：将 `NodePayload` 改为 struct，字段直接挂在 `RenderNode` 上；或至少用 `#[serde(untagged)]` 省掉 JSON 外壳。如后续（v1.6 字体自绘）需要区分 Mesh/Text，届时再加回变体不迟。

#### 2. `program` 字段是 u32 魔数，零定义零类型安全

- **位置**：
  - `node.rs:54` — 字段声明：`program: u32`
  - `mod.rs:265-275` — Container/Button 分支设 program=0/2/3/4
  - `mod.rs:321` — Image 分支设 program=0/3
  - `mod.rs:459,479` — RichText bg 设 program=3/0
  - `mod.rs:525` — 行内图设 program=0
  - `mod.rs:948,979,1010` — Text mesh 设 program=1
  - `batch.rs:35` — `is_mergeable_mesh` 硬编码 `*program == 0`
  - `merge.rs:18` — `mesh_key` 硬编码 `*program == 0 || *program == 1`
- **分析**：
  - 值 0-4 分散在 5 个函数的 10+ 处硬编码，无 const/enum 定义。新增 shader 时必须 grep 所有出现点手工改。
  - 用 `u32` 浪费 3 字节/节点（值域只需 u8）。
  - `batch.rs` 的 mergeable 判断只认 program=0；`merge.rs` 的合批认 program=0|1——不一致紧耦合。
- **修复方向**：定义 `#[repr(u8)] enum ShaderProgram { Default=0, Text=1, BgComposite=2, Filter=3, FilterBg=4 }`，替换所有魔数。字段类型改为 `u8`，blob 列宽减 3 字节。

#### 3. `build_render_nodes` 函数体 ~490 行，嵌套最深 5 层

- **位置**：`mod.rs:123-609`
- **分析**：
  - 单函数内包含：display:none 剪枝、5 种 NodeKind match arm（Container/Button/Image/Text/RichText）、scrollbar thumb 合成、text 子页 sort_key 传播、双 hash change_level 定级
  - 最长嵌套路径：`for scene.nodes.values()` → `match n.kind` → `Container|Button` → `match background_image` → `match (has_slice, all_zero)` → `(true, false)` 调用 `nine_slice_rounded`。5 层深度，每个 arm 自身又 20-100 行。
  - Container arm（~110 行）和 RichText arm（~155 行）内各有一整段几何逻辑（border-radius 解析、九宫格分流、bg quad、行内图 push）完全可独立为函数。
  - 末尾 dirty hash 循环（`mod.rs:585-602`）本身就是职责独立的步骤，塞在同一函数内。
  - 函数签名 `build_render_nodes(scene, fonts, prev, image_sizes, atlas) -> (FrameData, HashMap, Vec<u32>, Vec<fragments>)` — 4 元组返回值，调用方必须解构 4 项。
- **修复方向**：拆为：
  - `build_container_mesh(node, scene, image_sizes) -> MeshData`
  - `build_image_mesh(node, image_sizes) -> MeshData`
  - `build_text_meshes(node, scene, fonts, atlas) -> Vec<RenderNode>`
  - `build_rich_text_meshes(node, scene, fonts, atlas) -> (Vec<RenderNode>, Vec<RichFragment>)`
  - `finalize_change_levels(&mut [RenderNode], prev) -> HashMap<u32, (u64,u64)>`
  - 主函数 `build_render_nodes` 退化为 ~40 行的编排器。

#### 4. `nine_slice_rounded` 不重用 `rounded_rect` 和 `nine_slice`，~465 行完全自包含

- **位置**：`mesh.rs:267-731`
- **分析**：
  - `nine_slice_rounded` 内部重写了一套弧扇生成逻辑（与 `rounded_rect` 的三角扇逻辑几乎一样：sides 计算、delta 步进、末段锁 `FRAC_PI_2`）和一套 push_quad 逻辑（与 `quad` 的 4-vert+6-index 完全一致），仅 UV 映射方式不同（分段 vs 线性）。
  - `quad` 函数（`mesh.rs:22-40`）和 `push_quad_uv` 闭包（`mesh.rs:352-378`）产出的顶点序/索引序完全一致（TL,TR,BR,BL + `[0,1,2,0,2,3]`）。
  - `nine_slice` 的 grid 构建逻辑（`mesh.rs:188-226`）被完全复制到 `nine_slice_rounded` 的 `sl_l/r/t/b` 切片线（`mesh.rs:287-294`）+ UV 切片线（`mesh.rs:301-304`）。
  - 这 ~465 行函数实际可拆为：复用 `nine_slice` 算 grid 位置和 UV 线 + 在每个角区插入弧扇（复用 `rounded_rect` 的弧生成）+ 对非角区发 quad。
- **修复方向**：提取 `make_arc_fan(center, rx, ry, start_angle) -> (verts, uvs, indices)` 供 `rounded_rect` 和 `nine_slice_rounded` 共用；提取 `make_quad(x0,x1,y0,y1) -> (verts, indices)` 消除 3 处重复的 4-vert+6-index 生成。三函数共享同一个 `arc_fan` 实现可减少约 150 行重复代码。

### 🟡 中等

#### 5. `payload_hash` 未覆盖 `parent_id` 字段

- **位置**：`dirty.rs:12-59`（payload_hash）、`dirty.rs:65-83`（header_hash）
- **代码**：
  ```rust
  // header_hash 覆盖：world_matrix, visible, alpha, sort_key, mask_context, color_tint, blend, reuse_key
  // payload_hash 覆盖：image_path, program, color_matrix, verts(re-based), uvs, colors, indices
  ```
- **分析**：`RenderNode.parent_id` 不在任一 hash 中。C# 侧的 MirrorPool 用 `node_id` keying，但 `parent_id` 变更可能影响 GO 层级（parenting）。如果同 node_id 换父节点（如 remove+add 同一 slot），header/payload 不变 → ChangeLevel::Skip → C# 不重新 parent。实际是否已发生待确认——当前测试未覆盖此路径。
- **修复方向**：将 `parent_id` 加入 `header_hash`（父节点变动通常只改 transform 层级，不重建 mesh）。或确认当前后端不依赖 `parent_id` 后在此字段上加 `#[doc(hidden)]` 注释说明不参与 diff。

#### 6. `propagate_text_sub_page_sort_keys` 的 O(n*m) 内环

- **位置**：`mod.rs:675-681`
- **代码**：
  ```rust
  for (primary_sk, n) in &shifts {
      for rn in nodes.iter_mut() {  // ← 每个 shift 扫全量 nodes
          if rn.sort_key > adjusted_sk {
              rn.sort_key += n;
          }
      }
  }
  ```
- **分析**：当有 k 个带子页的文本节点时，外循环 k 次，内循环全量 nodes（n 个）。时间复杂度 O(k*n)。k 在典型场景很小（跨页 text 罕见），当前无性能问题，但累积偏移逻辑本身可改写为单次扫描 O(n)。
- **修复方向**：预计算每个真节点位置处的累积偏移数组 `cum_shift[pos]`，然后单次遍历 nodes 应用偏移。ponytail：当前场景 k≪n，先不动，加 `// ponytail: O(k*n) loop, fix if text sub-pages become common`。

#### 7. `merge_batch` 硬写 `color_matrix: [0.0; 20]`

- **位置**：`merge.rs:123`
- **代码**：
  ```rust
  payload: NodePayload::Mesh { ..., color_matrix: [0.0; 20], }
  ```
- **分析**：合并后的 mesh 丢弃了源节点的 `color_matrix`。当前安全是因为 `mesh_key` 只允许 program=0|1 的节点合并，而 program=0|1 节点不会设 color_matrix（color_filter 使 program 变为 3 或 4）。但这是**隐式不变量**——`mesh_key` 不检查 color_matrix，全靠上游 `build_render_nodes` 的程序号分流保证。若未来 program 语义变化（如 program=0 也可带 filter），此处会静默丢数据。
- **修复方向**：在 `mesh_key` 中加入 color_matrix 比较（`color_matrix == [0.0; 20]` or `== [0.0; 20]`），确保仅零矩阵节点可合并；或将 color_matrix 纳入 merge key。至少加注释说明隐式不变量。

#### 8. `BlendMode` 也是死枚举（只有 `Normal`）

- **位置**：`node.rs:24-26`
- **代码**：
  ```rust
  pub enum BlendMode { Normal }
  ```
- **分析**：仅一个变体。`header_hash`（dirty.rs:77-79）中的 `match rn.blend { BlendMode::Normal => 0u8 }` 是冗余的。若未来加 `Additive`/`Multiply`，当前 hash 会漏（match 穷尽但新变体无 hash 变化，可能不会触发编译警告——编译器只在非穷尽 match 报错，但 hash 内部是穷尽的，新增变体后编译器会在 match 处报错要求补 arm，所以反而不容易漏）。
- **修复方向**：既然规范说未来会有 BlendMode，保留 enum 合理。但 `header_hash` 中 `match` 可改为 `#![deny(non_exhaustive_omitted_patterns)]` 之下的写法，或至少加 `#[allow(...)]` 注释说明。

#### 9. `reorder_for_batching` 每次分配临时 `order` Vec

- **位置**：`batch.rs:246`
- **代码**：
  ```rust
  let mut order: Vec<usize> = (0..nodes.len()).collect();
  ```
- **分析**：每帧分配一个 `Vec<usize>` 做排序，用后丢弃。渲染节点数通常 <1000，分配开销可忽略。但若帧率敏感（移动端 60fps），可复用预分配的 buffer。
- **修复方向**：ponytail——当前分配 <4KB/帧，留待 profiler 证明确为热点。可以加 `// ponytail: per-frame allocation, reuse if profiler shows GC pressure`。

### 🟢 轻微 / 建议

#### 10. `container_node` 测试 helper 复制 3 次

- **位置**：
  - `tests.rs:42-50` — `fn container_node(id, parent, rect, bg) -> Node`
  - `batch.rs:286-309` — `fn placeholder_rn(i) -> RenderNode`
  - `batch.rs:591-619` — `fn mesh_rn(path, rect, mask) -> RenderNode`
  - `batch.rs:764-777` — `fn mesh_rn_into_rn(id, path, _scene) -> RenderNode`
  - `merge.rs:135-169` — `fn mesh_node(id, path, sort_key, alpha, rect_off) -> RenderNode`
  - `dirty.rs:93-116` — `fn mesh_rn(path, alpha, color0) -> RenderNode`
- **分析**：6 个不同文件中各有构造 RenderNode 的测试工厂函数，功能高度重叠。不是 bugs，但增加了修改 RenderNode 字段时的维护面（需同时改 6 个地方）。
- **修复方向**：在 `render` 模块加 `#[cfg(test)] pub mod test_util` 集中提供 `mesh_rn_factory`，各测试文件 re-export。

#### 11. `src_size` fallback 64 是硬编码

- **位置**：`mod.rs:30-36`
- **代码**：
  ```rust
  fn src_size(image_sizes: &ImageSizeTable, path: &str) -> (f32, f32) {
      image_sizes.get(path)
          .filter(|(w, h)| *w != 0 && *h != 0)
          .map(|&(w, h)| (w as f32, h as f32))
          .unwrap_or((64.0, 64.0))
  }
  ```
- **分析**：64×64 是没有尺寸信息的图标的默认值，合理但应命名 `DEFAULT_SRC_SIZE: (f32, f32) = (64.0, 64.0)` 便于 grep/修改。

#### 12. `is_text_sub_page` 硬上限 4096 的 debug_assert 文档充分但无运行时兜底

- **位置**：`mod.rs:143-147`
- **代码**：
  ```rust
  debug_assert!(scene.nodes.values().all(|n| (n.id.0 >> 12) < 4096), ...);
  ```
- **分析**：注释写明 "release 继续（此时 is_text_sub_page 可能误判真节点为子页，sort_key 乱序）"。这是有意为之（debug 快速失败，release 不 panic），且测试 `node_index_4096_triggers_sub_page_collision`（`tests.rs:2196-2203`）已加哨兵。设计合理。

#### 13. 测试覆盖良好但 `build_render_nodes` 缺少直接单元测

- **位置**：`tests.rs` 全文件
- **分析**：现有 40+ 测试覆盖了 Container/Image/Text/RichText 各种组合、九宫格 UV、display:none 剪枝、change_level 三级、合批、scrollbar thumb、多页 text 子页传播等。但缺少：
  - 直接对 `build_render_nodes` 返回的 4 元组中 sort_keys buffer 的验证（现有测试通过 FrameData.nodes 间接测）
  - `parent_id` 是否随场景 parent 关系正确填充的回归测试
  - `build_render_nodes` 在节点数=0、全部 display:none、混合多种 kind 等极限场景的测试

---

## 架构总结

| 维度 | 评价 |
|------|------|
| 核心流程 | 清晰：display:none 剪枝 → 逐个节点构建 RenderNode → assign_sort_keys → text 子页传播 → reorder → merge → scrollbar thumb → dirty hash。每步职责明确。 |
| 几何生成 | quad/rounded_rect/nine_slice 三条路径正交正确。但 `nine_slice_rounded` 未复用前两者的基础图元，存在实质性代码重复。 |
| 变更检测 | 双 hash 机制精巧——payload_hash re-base 减世界坐标使位置变更只进 header_hash。覆盖完整（除 parent_id）。 |
| 合批条件 | merge key 含 (image_path, program, mask_context, alpha)，条件恰当。隐式依赖 program=0/1 不含 color_matrix 的不变量，建议显式化。 |
| 代码组织 | `build_render_nodes` 过长（~490 行 / 1022 行文件总量 = 48%），是最大的单点改进机会。 |
| 类型安全 | `program: u32` 魔数和单变体 `NodePayload` enum 是两个可立即修复的类型系统债务。 |

---

## 优先级排序修复方案

1. **将 `program: u32` 改为 `#[repr(u8)] enum ShaderProgram`**（影响 6 个文件，~20 处替换，无 behavioral change）
2. **拆分 `build_render_nodes` 为 5 个子函数**（按 NodeKind 分支 + change_level 计算，纯重组，风险低）
3. **消除 `NodePayload` enum（改为 struct）或标记 `#[non_exhaustive]`**（影响序列化格式，需同步 C# 侧 mirror）
4. **提取 `make_arc_fan` 供 rounded_rect + nine_slice_rounded 共用**（~150 行净减少）
5. **将 `parent_id` 加入 `header_hash`**（一行改动 + 回归测试）
