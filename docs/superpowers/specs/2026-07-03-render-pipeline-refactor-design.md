# 渲染管线重构设计（tick 时序 + 变更检测机制）

> 2026-07-03。修 B1（伪类反馈丢），根治漏字段类 dirty bug（坑 56/75/76），把 CLAUDE.md
> 「transform 是渲染层、位置动画廉价」不变量在 FFI 层真正兑现。
>
> 一次重构，三根支柱，同一条数据线：`stage.rs`（时序）→ `dirty.rs`（变更检测）
> → `render/mod.rs`（emit 级别）→ `blob.rs`（FFI 变更级别列）→ `MirrorPool.cs`（按级别执行）。

---

## 0. 背景：为什么这块「越做越复杂」

三块（tick 时序 / 伪类 rematch / render emit）不是各自的 bug，而是**同一个反哲学模式**的三个化身：

- CLAUDE.md 立了三条不变量：「所有布局帧末一致」「每帧一次 solve」「全量重算」。
- 但实现里**偷偷做了局部增量**：
  - tick 把 rematch 排在 solve/compute **之后**（等于"改了布局却不重算"）——坑 103 / B1。
  - dirty hash **只采样部分字段**（首字 codepoint、verts[0]/[2]、UV 摘要）——每加一个视觉字段漏一次，坑 56/75/76。
- 复杂来自偏离哲学。**回归哲学 = 简洁。**

本设计不加机制去打补丁，而是**删掉两处偷懒**（时序补丁区 + hash 采样补丁区），并把变更检测提成一个**正交的、可扩展的机制**。

---

## 1. 支柱一：tick 顺序 = 显式依赖拓扑

### 1.1 问题

`rematch_pseudo_classes` 每帧从 `base_style` 重建 `node.style`，产出**三类**变化，各有不同消费者：

| rematch 改的字段 | 消费者 | 当前消费者位置 | 结果 |
|---|---|---|---|
| `taffy_style`（`:hover{border/width}`） | `solve` | rematch **之前**(stage.rs:435) | ✗ 本帧丢，返回值 `any_layout_dirty` 还被扔掉 |
| `transform`（`:active{scale}`） | `compute_world_transforms` | rematch **之前**(stage.rs:456) | ✗ 本帧丢（B1 主症状） |
| `colors`（`:hover{bg}`） | `build_render_nodes` | rematch **之后**(stage.rs:464) | ✓ 唯一正常 |

外部 AI 诊断建议「把 compute 挪到 rematch 后」——**只修了 transform 一类，`:hover{border}` 仍丢**（solve 还在 rematch 前）。

### 1.2 设计：rematch 提到最前，消费者依次在后

规则一句话：**用到某产物前，先把它算好。** 状态输入（hover/active/focus，由 hit_test 用**上帧** world 产，1 帧延迟为已认可设计）排最前，然后严格按"产→读"排：

```
新 tick 顺序（loomgui_core/src/stage.rs tick_and_render）：
  ① tween.update            （写 anim override 表；须在 solve/compute 前）
  ② pending_focus_request
  ③ process（仲裁 + 拖拽写 scroll_pos；hit_test 读【上帧】world_transforms）
  ④ scroll.update（wheel + 惯性）
  ⑤ process_keys
  ⑥ rematch_pseudo_classes  ← 提到这里：按本帧 hover/active/focus 改 node.style（三类字段）
  ⑦ solve                   （读 taffy_style → layout_rect）  ✓ :hover{border/width} 生效
  ⑧ refresh_content_sizes   （solve 后填 content_size/overlap）
  ⑨ compute_world_transforms（读 transform + scroll_pos → world）✓ :active{scale} 生效
  ⑩ build_render_nodes      （读 visual + world → RenderNode）  ✓ :hover{bg} 生效
```

**改动**：把 rematch 从 ⑦(旧 459) 移到 solve 之前；删掉 `rematch_pseudo_classes` 的 `any_layout_dirty` 返回值（不再需要——solve 每帧全量，rematch 在它前面，无需 dirty 驱动）。

### 1.3 副作用核查（均已读代码确认低风险）

- **hit_test 用上帧 world**（1 帧延迟语义，stage.rs:439 / hit.rs:85）：`process`(③) 仍在 `compute`(⑨) 之前，process 里读的 `scene.world_transforms` 仍是**上帧**值 → 语义不变。**零回归。**
- **scroll_pos 同帧进 world**（拖拽零延迟，spec v1d.5 §9.3）：`compute`(⑨) 仍在 `scroll.update`(④) 之后 → 不变。
- **tween 与 rematch 不打架**：tween 写独立 `anim` override 表（HashMap，tween.rs:222-230），rematch 写 `node.style`（从 base_style 重建）。compute/build 读取时是 `anim.xxx.unwrap_or(style.xxx)`（transform.rs:35-38 / render/mod.rs:136-137）。两条通道正交。**移动 rematch 不碰 tween。**
- **伪类改布局属性当帧 solve**：`:hover{width:200px}` 现在会当帧 solve 生效（浏览器本就此行为）。对 nav 按钮量级无所谓，不加任何 dirty 门控（符合「每帧一次 solve」哲学）。

### 1.4 rematch 内的 `.expect("live node")`（顺带）

`rematch_pseudo_classes`(dynamic.rs:206/222)、`compound_matches_with_state`(dynamic.rs:109) 用 `.expect`。这些在 core 内部（非 FFI 入口），node_id 来自 `scene.nodes.values()` 当帧收集，不会失活 → 保留（非 FFI 边界，坑 102 不适用）。

---

## 2. 支柱二：变更检测 = 全量 hash（删采样）

### 2.1 问题

`dirty.rs::node_hash` 手写采样：文字只 hash「首字 codepoint + glyph_count + 首字 x/y」，mesh 只 hash「verts[0]/[2] + uvs[0]/[2]」。后果：

- `"hello"` → `"helps"`：首字 h、5 字、首字坐标同 → **hash 撞** → 误判 Unchanged → 文字不更新（坑 56/75/76 一整类）。
- 每加一个视觉字段（program/color_matrix/UV/圆角顶点）就要补一次采样点 + 一个回归测试。**采样是补丁累积区。**

### 2.2 设计：payload 全量 hash

`node_hash` 的 payload 部分改为**全量**：所有 verts、所有 uvs、所有 glyph codepoint 一律 `to_le_bytes().hash()`，不再挑代表。

- **根治漏字段类 bug**：不漏了，就不可能因漏字段误判。
- **删代码**：删掉 verts.len 分支采样、首字采样、UV 摘要那 ~50 行，删一半"补漏字段"回归测试（`mesh_quad_size_change`/`text_first_glyph_pen_x` 等改为"全量必变"的少量测试）。
- **性能**：全量 hash 是纯 Rust CPU、读已在内存的数组。几百节点 × 几~几十顶点 = 几万次哈希加法 ≈ 几十微秒（16ms 预算的零头）。它跟 `build` 一样是「本帧本来就全算」的一部分，**不碰**跨 FFI 传输/Unity 重建那步（贵的那步）。

### 2.3 关键：拆成两个正交 hash（支柱三的地基）

不再是一个 `node_hash`，而是**两个**：

```rust
// loomgui_core/src/render/dirty.rs
pub fn header_hash(rn: &RenderNode) -> u64   // 表头轴：world_matrix, visible, alpha, sort_key,
                                             //         mask_context, color_tint, blend, program, color_matrix
pub fn payload_hash(rn: &RenderNode) -> u64  // 几何轴：mesh(verts/uvs/colors/indices/image_path 全量)
                                             //         / text(font_size/color/全量 glyph)
```

两轴**正交**——表头（廉价：改 GO transform / 材质）与几何（贵：UploadMesh + RecalculateBounds + Sprite 查询 + UV 重映射 / 文字光栅化）分离。这是支柱三分级的依据。

---

## 3. 支柱三：变更级别机制（表头/几何两轴 → 三级）

### 3.1 问题：现状是「要么全传要么全不传」，太粗

C# MirrorPool.cs:71-76 现状只有两档：

- **Unchanged(kind=0)**：`continue`，连 localPosition 都不更新。
- **Mesh(kind=1)/Text(kind=2)**：走**完整**重建（UploadMesh + RecalculateBounds + Sprite 查询 + UV 重映射 / 光栅化 + 设材质）。

滑动 / `:active` 缩放 / 位移动画时，world_matrix 变 → 旧 `node_hash` 变 → 判 kind=1 → **重建整个 mesh，尽管顶点一字节没变**。这是真实浪费，也是 CLAUDE.md「位置/缩放动画廉价」承诺在 FFI 层**没兑现**的地方。

### 3.2 反面：为什么不是「加个 kind=3 TransformOnly」

那是补丁。`payload_kind` 表示「节点是什么几何」（mesh/text），不该塞「这帧什么变了」。挑"位置变"一个组合特殊处理 → 下个人加 kind=4（只 alpha）、kind=5（只材质）……补丁堆积。**两件正交的事被搅在一起。**

### 3.3 设计：变更级别（change level）作为独立的一轴

给每个节点每帧一个**变更级别**，与 payload_kind（是什么几何）正交。C# 按级别决定做多少事：

| 级别 | 判据（对比上帧两 hash） | C# 动作 |
|---|---|---|
| **SKIP** (0) | header_hash == 上帧 且 payload_hash == 上帧 | 保留 GO，什么都不碰（= 旧 Unchanged 语义）|
| **HEADER** (1) | 只 header_hash 变，payload_hash 不变 | 只更 localPosition/_ObjectMatrix/sortingOrder/材质，**绝不碰 mesh** |
| **FULL** (2) | payload_hash 变（几何变，通常也重设表头）| 重建 mesh（UploadMesh + RecalculateBounds + Sprite + UV 重映射 / 光栅化）+ 设表头 |

**可扩展性**：以后想让某个新廉价属性走快路，把它算进 `header_hash` 即可自动落 HEADER，**不加新变体**。这就是"机制"而非"补丁"。

### 3.4 core → build_render_nodes 产出级别

```rust
// render/mod.rs build_render_nodes：per-node 算两 hash，与上帧两 hash 比 → change_level。
// 首帧 / 结构变（prev 长度不符）→ 全部 FULL（无基线）。
// payload_kind 仍是 Mesh/Text（不再有 Unchanged 变体——「本帧没变」由 change_level=SKIP 表达）。
```

注意：**`NodePayload::Unchanged` 变体删除**。"本帧没变"从 payload 变体（把语义混进"是什么"）改为独立的 change_level=SKIP（放进"变了什么"轴）。payload 只剩 `Mesh | Text`，永远是真几何。level=SKIP 时 blob 仍写真 payload_kind（1/2）+ 表头，但 change_level 列标 0，C# 见 0 即跳过 arena 读取。

> ⚠️ 已知张力（spec 定稿点）：merge（render/merge.rs 合相邻同 DrawState mesh）与 SKIP/HEADER 的交互。merge 改变节点集合（N 个 → 1 个 merged），跨帧节点对齐会否破坏 hash 基线？见 §6 待定项 D2。

### 3.5 FFI：blob 加一个 change_level 列（1 字节）

`loomgui_ffi_c/src/blob.rs`（VERSION 7 → 8）：

- 新增列 `change_level`（u8，1B），列数 20 → 21。放在 `payload_kind` 之后。
- 表头 6 列（m_a..m_ty）+ 其余公共头**照旧每节点每帧全传**（已是现状，blob.rs:74-85）——所以 HEADER 级别所需的 world_matrix 数据**已在 blob 里**，无需新增数据，只需新增"级别"这个信号。
- **arena 写入优化**：level=SKIP 或 HEADER 的节点，mesh_arena/text_arena **不写**（mesh_off/len 占位 0）——省掉重的顶点/字形字节的拷贝。level=FULL 才写 arena。这是省 FFI 带宽的关键（旧版 kind=1 无条件写 arena）。
- `size_of` 断言 + header_len 计算随列数更新（坑 34）。csbindgen 重生成 + 手补 C# 镜像（FrameBlob 加 `ChangeLevel(i)` 读取，坑 35）。

### 3.6 C# MirrorPool 三分支

```csharp
// MirrorPool.cs Sync 主循环：读 change_level，三分支。
byte level = blob.ChangeLevel(i);
if (level == 0) { /* SKIP */ ro.Stale = false; continue; }          // = 旧 kind==0 分支
// level 1/2 都要 ensure RenderObj + 更新表头（localPosition/_ObjectMatrix/sortingOrder/材质）
UpdateHeader(ro, blob, i);                                          // 抽出现有 94-100 + 材质那段
if (level == 2) { /* FULL */ UploadMeshOrText(ro, blob, i); }       // 现有 119-197 的 mesh/text 重建
```

滑动/缩放/位移动画 → HEADER → 只 `localPosition = (Mtx,Mty)` / `_ObjectMatrix` 一行，不 UploadMesh。**兑现廉价承诺。**

### 3.7 为什么滑动天然是 HEADER（已核数据流）

blob.rs:104-105 对纯平移节点把顶点 re-base 成局部坐标（减 tx/ty），滚动偏移**全部**活在 Mtx/Mty（表头）。所以滚动逐帧：arena mesh 字节**完全相同** → payload_hash 不变 → HEADER。`:active{scale}` 同理（scale 进 _ObjectMatrix，顶点 box-local 稳定）。

---

## 4. 字段 → hash → 级别 归属表（契约）

| 字段 | 归 | 变化触发级别 | 备注 |
|---|---|---|---|
| world_matrix (m_a..m_ty) | header_hash | HEADER | scroll/transform 动画主力 |
| visible | header_hash | HEADER | |
| alpha (节点 opacity) | **payload_hash** | **FULL** | ⚠ 例外，见下：alpha 烤进顶点色，须重写 arena |
| sort_key | header_hash | HEADER | 由结构决定，通常伴随几何变 |
| mask_context | header_hash | HEADER | |
| color_tint | header_hash | HEADER | |
| blend | header_hash | HEADER | |
| program | header_hash | HEADER | 只换材质 keyword，不碰几何 |
| color_matrix | header_hash | HEADER | filter 参数，走 MPB SetVector |
| mesh verts/uvs/colors/indices（全量）| payload_hash | FULL | |
| image_path | payload_hash | FULL | 换图触发 Sprite 查询 + UV 重映射 |
| text font_size/color/全量 glyph | payload_hash | FULL | |

**⚠ alpha 的例外（已知妥协，YAGNI）**：当前 alpha 被烤进顶点颜色（blob.rs:124 `c[3]*rn.alpha`）。这意味着 alpha 变会让 mesh_arena 字节变。但 payload_hash 是对 **RenderNode.payload**（core 层，烤 alpha 前）算的，不是对 blob arena 算——core 层 colors 不含 alpha 烘焙（那是 blob 阶段做的）。故 alpha 变 → header_hash 变（alpha 在表头列）→ HEADER。**但 blob arena 里的 colors 已烤 alpha → HEADER 不重写 arena → Unity 拿旧 alpha 的顶点色。**
→ **决策**：alpha 归 payload_hash，alpha 变落 FULL（重写 arena 才能刷新烤进的 alpha）。牺牲 opacity 动画的廉价性换正确性。opacity 动画非主力（主力是 transform），可接受。**待定项 D1** 记录另一路（把 alpha 从顶点色剥离、改走 MPB/顶点色分离）——不在本轮做。

---

## 5. 验收

| 支柱 | 验收方式 | 机器 |
|---|---|---|
| 1 tick 时序 | `cargo test -p loomgui_core`：新增测试——rematch 前置后 `:hover{width}` 当帧 layout_rect 变、`:active{scale}` 当帧 world 含 scale。离线可验。| 公司机 |
| 2 全量 hash | `cargo test`：`"hello"→"helps"` hash 必变；删采样后旧回归测试转"全量必变"。离线可验。| 公司机 |
| 3 变更级别 | blob round-trip 测试（change_level 列 + SKIP/HEADER 不写 arena）；C# 侧需 Unity PlayMode：滑动列表时 profiler 确认无 UploadMesh 调用、`:active` 缩放当帧可见、hover 变色刷新。| 家里机 |

**闭环**：改 core/FFI 后重编 `.dll` + commit（坑 10）；本设计纯 runtime（tick/dirty/blob），改 .dll 即可，**不须重打 pkg**（base_style 未变，坑 66）；改 FFI 签名后 push 前查 dll 导出（坑 100）。

---

## 6. 待定项（实现前定稿）

- **D1**（alpha 烘焙）：本轮 alpha → FULL（见 §4）。是否后续把 alpha 从顶点色剥离改走 MPB，让 opacity 动画走 HEADER——记 ledger，不在本轮。
- **D2**（merge × 级别）：merge_meshes 把 N 节点合成 1 个 merged 节点，merged 的 node_id/hash 跨帧如何对齐？若 merged 节点每帧重算 → 恒 FULL（merge 场景本就有内容变才 merge？）。**需在实现 T? 前读 merge.rs 定**：merged 节点走 FULL（保守正确），non-merged 走三级；或 merge 仅对 FULL 节点触发。倾向前者（简单正确）。
- **D3**（结构变基线）：`prev_hashes.len() != n_nodes` → 全 FULL（现状 baselined=false 已如此）。两 hash 表都按此重置。

---

## 7. 不做（YAGNI）

- 不加 `transform_dirty` flag（build 本就每帧重算 hash，不依赖 dirty flag）。
- 不为 alpha 剥离顶点色烘焙（D1 记 ledger）。
- 不加"只 sort_key 变""只材质变"等更细级别（HEADER 已覆盖所有廉价表头变化，一档够）。
- 不动 pkg.bin 格式（纯 runtime 重构）。
