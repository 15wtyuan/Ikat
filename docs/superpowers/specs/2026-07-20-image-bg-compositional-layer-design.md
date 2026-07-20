> **⚠️ SUPERSEDED (2026-07-20):** The back-layer mechanism for image-bg described below was abandoned as YAGNI after a shader read revealed `BG_COMPOSITE` (program 2/4) already does source-over compositing (`tex over vcol`) for Container — Image just wasn't using it. The actual fix is ~5 lines (Image → BG_COMPOSITE when bg-color present), **not** a synthetic-id back-layer. Container is already correct (shader composites). Only the rename portion (`BOX_SHADOW_FLAG` → `BACK_LAYER_FLAG`, by semantics) survived. See `docs/superpowers/plans/2026-07-20-image-bg-color-via-bg-composite.md`.

# image-bg 合成层机制（back-layer）设计

> 2026-07-20。摸黑结束（Spec-4b）后 §4 视觉束第一个 tech-debt。
> 修：`<img>` / Container 的 background-color 不在 texture/背景图之下显示（紫底/底色透不出）。
> 通用机制：把 box-shadow 已有的"合成层"模式提取成显式 back-layer 机制，image-bg 复用，box-shadow refactor 到其上。

## 1. 问题与根因

HTML 语义：`background-color` 画在 `background-image` 之下；图的透明像素透出底色。当前实现两条路径都缺这层：

- **`NodeKind::Image`**（`render/mod.rs:481-507`）：只画一个 texture quad，完全跳过 `background-color`。spec4b card-img 的紫底没渲染。
- **Container**（`render/mod.rs:317-480`）：bg-color + 背景图 = **单个 quad**，bg-color 当**顶点色 tint** 背景图纹理（program 2 采样纹理 × 顶点色）。透明像素 → 透明结果，底色同样透不出。且 tint 行为非标准 CSS。

被 revert 的 `73e560e` 想给 Image 补 bg-color quad，但用了**同一个 node_id** —— Unity `MirrorPool` 按 node_id 去重（`MirrorPool.cs:75-77`，`poolKey = reuseKey != 0 ? reuseKey : id`）→ bg quad 被 texture quad 覆盖 → 仍无底色。**根因：bg 与 texture 共享 node_id，被后端去重。**

正确修法：给 bg-color 一个**合成 node_id**（与 primary 不同），Unity 建两个 GO，sort_key 保证 bg 绘在 texture 下。这正是 box-shadow 已有的模式。

## 2. 决策上下文（为什么是方案 A）

合成 node_id 的高位预算已**全占满**，无干净空闲位：

| 位 | 用途 |
|---|---|
| bits 24-27（high byte 1-15） | 文本跨页子页 index（`synth_text_node_id`） |
| bit 28（0x1000_0000） | `BOX_SHADOW_FLAG` |
| bit 29（0x2000_0000） | `H_THUMB_FLAG`（scrollbar 命中测试 sentinel） |
| bit 30（0x4000_0000） | `V_THUMB_FLAG`（scrollbar 命中测试 sentinel） |
| bits 29-31（high byte 232-255） | 富文本行内图（`INLINE_IMG_SYNTH_ID_BASE`） |

任何 high byte ≥ 16 的值都至少撞 shadow/H-thumb/V-thumb/inline-img 中的一个 flag。候选方案：

- **A（采纳）**：image-bg **复用 bit 28**，与 box-shadow 共用 back-layer 位。不同渲染路径产出，解码点对 image-bg 同样正确（都是"不合并的独立下层"）。唯一缺口：同一节点同时要 box-shadow + bg 底色（需 2 个 back-layer）会撞 id —— 记 tech-debt。
- ~~B~~（撤回）：原推荐"合并 H/V thumb 腾位"，但核实 thumb 可共存（双轴 overflow:auto 同产 v+h thumb）且是跨 `hit.rs`/`scroll.rs`/`input.rs` 的命中测试 sentinel，合并代价远超预期。
- **C（不做，YAGNI）**：重排整个合成 id 命名空间（4-bit layer-type 字段），每层独立槽位。要重迁文本子页/行内图/thumb 编码 + 跨多个解码点，本轮范围外。
- 注：原设想的"A′ 独立位"不成立——无干净独立位可选，要么退化成 A，要么是 C。

**决策：方案 A。** 把 bit 28 泛化成通用 `BACK_LAYER_FLAG`，image-bg 和 box-shadow 共用。多 back-layer per node（shadow+bg 共存）留待未来 C 重排。

## 3. 机制设计

### 3.1 `BACK_LAYER_FLAG`（bit 28 泛化重命名）

`render/mod.rs:35` `BOX_SHADOW_FLAG: u32 = 0x1000_0000` → 重命名 `BACK_LAYER_FLAG`（值不变）。语义从"div box-shadow 合成节点"泛化为"主节点的下层合成层"。

### 3.2 共享 pair 收集 + sort_key 传播

- `shadow_pairs: Vec<(u32, u32)>` → 泛化 `back_layer_pairs: Vec<(primary_id, synth_id)>`。
- `propagate_box_shadow_sort_keys`（`render/mod.rs:760`）→ 重命名 `propagate_back_layer_sort_keys`，逻辑不变：每个 back-layer 的 sort_key 设为其 primary 的 sort_key，primary 及其后节点后移一位，保持全序单调。image-bg back-layer 与 shadow back-layer 走同一条传播。

### 3.3 发射：per-feature 建网格，共享脚手架

mesh 几何因特性而异（box-shadow = offset shadow-color quad；image-bg = 同 rect 的 bg-color quad），**不抽过度通用的大参数 helper**（ponytail：10 参数 helper 是反模式）。共享的是脚手架：`sid = primary_id | BACK_LAYER_FLAG`、`sort_key:0`（待传播）、`mask_context:MaskContext(0)`、`change_level:Full`、`reuse_key:0`、push 在 primary 之前、`back_layer_pairs.push((primary_id, sid))`。

box-shadow 发射（`render/mod.rs:619-667`）改用 `BACK_LAYER_FLAG` + 共享 pairs；mesh 生成（`box_shadow_quad`）不动。

### 3.4 image-bg 发射（新）

- **Image 节点**（`render/mod.rs:481`）：若 `background_color` 的 alpha > 0，在 primary texture quad 之前 `emit_back_layer`：solid bg-color quad（program 0，color = bg_color，UV 全图），几何用 element rect（含 `border_radius` → `rounded_rect`）。primary texture quad 顶点色本就是白 `[1,1,1,1]`（`mod.rs:502`，现状不变），唯一新增是 back-layer。
- **Container**（`render/mod.rs:317`）：当 `has_image` 且 bg-color alpha > 0，发 bg-color back-layer（program 0，**full rect** `*rect`，含 rounded）+ image primary（program 2，**顶点色由 bg-color 改白**——bg-color 移到 back-layer，不再进 image quad 的 vertex color）。back-layer 用 full rect 而非 `draw_rect`（contain 调整只作用于图，不作用于底色）。

**发射条件**：bg-color alpha == 0 时不发 back-layer（默认透明 bg 无需层），保持当前单 quad 路径零开销。

## 4. 行为变化（Container）

| 场景 | 旧行为 | 新行为 |
|---|---|---|
| `<img>` + bg-color | 只画 texture（顶点色白），底色丢 | bg-color 下层 + texture 上层 |
| Container + bg-image + bg-color | 单 quad，image quad **顶点色 = bg-color**（底色走 vertex color 通道；可观测的 tint 取决于 Unity program-2 着色器是否乘顶点色，实现期确认） | bg-color 独立下层（full rect，纯色）+ image quad 顶点色白（图原色） |
| Container + bg-image（无 bg-color） | 单 quad 图 | 不变 |
| Container + 纯 bg-color（无图） | 单 quad 纯色 | 不变 |

Container 新行为对齐标准 CSS（`background-color` 永远在 `background-image` 下）。**这是有意的视觉行为变化**：现有 showcase 中"Container + bg-image + 非白 bg-color"的元素会从"底色混进 image quad"变成"原色图叠在独立底色层上"。家里机 Unity 验收需复核 showcase 各页视觉（确认 program-2 着色器的旧 tint 行为，并验证新分层正确）。

## 5. 解码点（refactor 触及面）

- `render/batch.rs:38` `is_mergeable_mesh`：`BOX_SHADOW_FLAG` → `BACK_LAYER_FLAG`（image-bg back-layer 同样不合并，与 shadow 一致）。
- `render/merge.rs:26` `mesh_key`：同上 rename（back-layer 不进合并键）。
- `render/mod.rs:737` `is_text_sub_page`：已排除 high byte ≥ 16（back-layer 的 bit 28 = byte 16，天然排除）——**无需改**，但加断言守卫。
- box-shadow 现有测试（`render/tests.rs:2717` `box_shadow_emits_node_with_offset_and_sort_key`）：rename 后仍绿。

Unity 侧（`MirrorPool`）：**零改**。synthetic-id 模式已在 shadow 跑通，back-layer 的 distinct id 自动得独立 GO。

## 6. 方案 A 的限制（tech-debt）

单一 `BACK_LAYER_FLAG` 位 → 一个 primary 最多一个 back-layer。共存场景：

- **Container + box-shadow + bg-image + bg-color**：会同时要 shadow back-layer 和 bg-color back-layer，两者 `id | BACK_LAYER_FLAG` 撞 id → 去重掉一个。
- 处置：**box-shadow 优先**（不回归现有 shadow 行为），image-bg back-layer 在检测到同节点已有 shadow 时跳过（debug log）。底色在该罕见场景不显示。
- 真正修法 = 方案 C 重排（多 layer-type 槽位），本轮不做，记 §4 tech-debt。
- Image 节点不触发此限制（box-shadow 仅 container 发射，`mod.rs:623`）。

## 7. 测试

core render 单测（本机 headless，编码机验）：

1. **Image + bg-color**：产 2 个 RenderNode —— back-layer（`id | BACK_LAYER_FLAG`，program 0，color = bg-color）+ primary（program 2，顶点色白）。back-layer sort_key < primary。
2. **Image + 透明 bg-color**：仍 1 个 RenderNode（不发 back-layer）。
3. **Container + bg-image + bg-color**：2 个 RenderNode，back-layer full rect + primary image（白 tint）。
4. **box-shadow 回归**：`box_shadow_emits_node_with_offset_and_sort_key` rename 后绿；shadow node_id 仍 = `primary | BACK_LAYER_FLAG`。
5. **位碰撞守卫**：`BACK_LAYER_FLAG` 落在 high byte 16（≥16，被 `is_text_sub_page` 排除）的断言。
6. **共存放弃**：Container + shadow + bg-image+bg-color → 只发 shadow back-layer，不发 image-bg back-layer（方案 A 限制的可执行守卫）。

Unity 视觉验收（家里机）：spec4b-acceptance card-img 紫底出来（视觉第 5 门转绿）+ showcase 各页 Container+bg-image 视觉复核（行为变化）。

## 8. 范围 / 不做 / tech-debt

**做**：`BACK_LAYER_FLAG` 泛化 + 共享 back_layer_pairs/propagate；Image bg-color 下层；Container bg-image+bg-color 下层（行为变化）；box-shadow refactor；解码点 rename；上述测试。

**不做（tech-debt，记 §4）**：
- 多 back-layer per node（shadow+bg 共存）→ 方案 C 重排。
- Container + bg-image + border（边框需独立 draw call，`mod.rs:417` ponytail 注释已有）。
- 文本子页 / 行内图 / thumb 编码不动（语义不同）。
- border-under-image、glow 等未来层 → 同机制可扩，但每层一位需 C 重排后才不再撞。

## 9. 与 roadmap 的对接

- 消化 roadmap §4 tech-debt「card-img Image bg 合成 node_id 机制」（Spec-4b P3.4 视觉 1/5 未过项）。
- 视觉束首个落地项；不涉及 keyframes runtime 驱动（独立项，本 spec 不碰）。
- pkg.bin 格式不变（纯渲染期合成，无 schema 改动，无 dll 版本 bump 的 schema 因素——但渲染代码改了，.dll 须重编+commit）。
