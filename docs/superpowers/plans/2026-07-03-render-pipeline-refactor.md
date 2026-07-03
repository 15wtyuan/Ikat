# 渲染管线重构实现计划（tick 时序 + 变更检测机制）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修 B1（伪类反馈丢）+ 根治漏字段 dirty bug（坑 56/75/76）+ 把「transform/opacity 动画廉价」在 FFI 层兑现（滑动/动画只挪不重建 mesh）。

**Architecture:** 三支柱一条数据线。支柱1：tick 把 `rematch_pseudo_classes` 提到 `solve` 前。支柱2：`node_hash` 拆成正交的 `header_hash`（表头：world/alpha/材质）+ `payload_hash`（几何：全量 verts/uv/glyph），删采样。支柱3：两 hash 对比上帧 → 每节点一个 `ChangeLevel`（SKIP/HEADER/FULL），blob 加 1 字节列，C# 按级别决定"跳过 / 只挪 / 重建 mesh"。附带：alpha 从顶点色烘焙剥离成 `_Alpha` uniform，让 opacity 动画走 HEADER。

**Tech Stack:** Rust 2021（loomgui_core / loomgui_ffi_c）、Unity 6.5 URP（C#）、HLSL（LoomGUI-Unlit.shader）。测试：`cargo test`（core/ffi round-trip）+ Unity PlayMode（家里机验支柱3/alpha）。

## Global Constraints

- **Rust edition 2021**；依赖钉版本（taffy 0.5 / ttf-parser 0.20 / cssparser 0.34 / scraper 0.19 / slotmap 1.1 / csbindgen 1）。
- **FFI 边界 C-like enum 必须 `#[repr(uN)]`**（坑 34）；ABI struct 永远 `size_of` 断言。
- **FFI 入口绝不 panic**（坑 102）：scene=None 等状态优雅早返，不 `.expect`/`unwrap`。
- **csbindgen 不为 `#[repr(C)]` struct 生成 C# stub**，须手补镜像（坑 35）。本计划改的是 blob 字节布局（非 struct 签名），C# 侧手改 `FrameBlob.cs`。
- **闭环**：改 core/FFI 后重编 `.dll` + commit（坑 10）；本重构是纯 runtime（tick/dirty/blob/shader），改 .dll 即可，**不须重打 pkg**（base_style 未变，坑 66）。改 FFI 后 push 前查 dll 导出（坑 100）——本计划不改 FFI 函数签名（只改 blob 字节），但仍须重编 dll。
- **坐标系**：核心左上原点 y 向下，无 height-y 翻转。
- **用户只读中文**：问答/总结用中文；代码/commit 英文照旧。
- **blob VERSION** 每次改字节布局须 bump（当前 7 → 本计划 8）+ 同步 `FrameBlob.cs` ExpectedVersion。

---

## 关键设计决策（实现前已定稿，替代 spec §6 待定项）

- **D2 解法（比 spec 更精确）**：**hash 移到 merge 之后、按 `node_id` 键计算**。现状 `build_render_nodes` 在 merge 前按 scene 位置算 hash（render/mod.rs:285-289）。但 merge_meshes 把 N 个节点合成 1 个 anchor 节点，真正 emit 的是 merge 后的集合。故 hash 必须在 `merge_meshes` 之后、`thumb` 追加之后，遍历最终 `nodes`、以 `node_id` 为键存 `HashMap<u32, (u64,u64)>`。merged 节点移动时顶点烤进 mesh → payload_hash 变 → 自动 FULL，无需特判。
- **D3**：`prev` 表为空 或 当前帧出现的 node_id 在 prev 中缺失 → 该节点 FULL（无基线）。首帧全 FULL。
- **D1**：alpha 剥离成 `_Alpha` uniform（T6/T7），alpha → header_hash → HEADER。

---

## 文件结构

| 文件 | 职责 | 任务 |
|---|---|---|
| `loomgui_core/src/stage.rs` | tick 顺序（rematch 提前）；prev_hashes 类型 `Vec<u64>` → `HashMap<u32,(u64,u64)>` | T1, T4 |
| `loomgui_core/src/style/dynamic.rs` | `rematch_pseudo_classes` 删返回值 | T1 |
| `loomgui_core/src/render/dirty.rs` | 拆 `header_hash` + `payload_hash`（全量），删采样 | T2, T3, T7 |
| `loomgui_core/src/render/node.rs` | 删 `NodePayload::Unchanged`；加 `ChangeLevel` enum + RenderNode 字段 | T4 |
| `loomgui_core/src/render/mod.rs` | merge 后按 node_id 算双 hash → 定 change_level；alpha 归属 | T4, T7 |
| `loomgui_core/src/render/merge.rs` | merge_meshes 处理 payload 只剩 Mesh/Text（无 Unchanged）；DrawState key 加 alpha | T4, T9 |
| `loomgui_ffi_c/src/blob.rs` | VERSION 8；加 change_level 列；SKIP/HEADER 不写 arena；alpha 不烤顶点色 | T5, T6 |
| `loomgui_unity/Assets/LoomGUI/Shaders/LoomGUI-Unlit.shader` | `_Alpha` uniform | T6 |
| `loomgui_unity/Assets/LoomGUI/Runtime/FrameBlob.cs` | 21 列 + `ChangeLevel(i)` 读取；ExpectedVersion 8 | T8 |
| `loomgui_unity/Assets/LoomGUI/Runtime/MirrorPool.cs` | 三分支 SKIP/HEADER/FULL；`_Alpha` MPB | T8 |

---

## Task 1: 支柱1 — tick 时序重排（rematch 提到 solve 前）

**Files:**
- Modify: `loomgui_core/src/stage.rs:432-459`（tick_and_render 主体）
- Modify: `loomgui_core/src/style/dynamic.rs:186-231`（rematch 删返回值）
- Test: `loomgui_core/src/stage.rs`（新增 tick 时序集成测试）

**Interfaces:**
- Consumes: `solve`, `rematch_pseudo_classes`, `compute_world_transforms`, `refresh_content_sizes`（均已存在）
- Produces: `rematch_pseudo_classes(scene: &mut Scene)`（返回值从 `bool` 改为 `()`）；tick 新顺序 process → rematch → solve → refresh → compute → build

- [ ] **Step 1: 写失败测试（`:active{scale}` 当帧 world 含 scale）**

加到 `loomgui_core/src/stage.rs` 的 `#[cfg(all(test, feature = "parse"))] mod tests`（若无则新建）：

```rust
#[cfg(all(test, feature = "parse"))]
mod tick_order_tests {
    use super::*;

    /// 支柱1：rematch 提到 solve/compute 前后，:active{scale} 当帧 world 即含缩放。
    /// 回归 B1：旧顺序 compute 在 rematch 前 → 当帧 world 无 scale。
    #[test]
    fn active_scale_visible_same_frame() {
        let (fp, _) = crate::stage::tests_font_path();
        let mut stage = Stage::new(&fp, (200.0, 200.0)).expect("stage");
        stage.load_inline_for_test(
            "<div id=\"b\" class=\"btn\">x</div>",
            ".btn{width:100px;height:100px;} .btn:active{transform:scale(0.5);}",
        ).expect("load");
        // 首帧建立
        stage.tick_and_render();
        // 找到 btn NodeId，置 active
        let scene = stage.scene.as_mut().unwrap();
        let bid = scene.nodes.values().find(|n| n.id_attr.as_deref() == Some("b")).unwrap().id;
        scene.get_mut(bid).unwrap().active = true;
        // 本帧 tick：rematch 应在 compute 前生效 → world 2×2 非 identity
        stage.tick_and_render();
        let scene = stage.scene.as_ref().unwrap();
        let wm = scene.world_transforms[bid.index()];
        assert!((wm[0] - 0.5).abs() < 1e-3, "active scale 当帧进 world：m_a=0.5，实={}", wm[0]);
    }
}
```

> 注：`Stage::new` 签名 / `tests_font_path` helper 若不存在，按 stage.rs 现有测试模式调整（见 stage.rs 现有 test helper）。`load_inline_for_test` 已存在（stage.rs:478）。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core --features parse tick_order_tests::active_scale_visible_same_frame`
Expected: FAIL（`m_a=1.0` — 旧顺序 compute 在 rematch 前，world 无 scale）

- [ ] **Step 3: 重排 tick 顺序**

`stage.rs` tick_and_render 中，将现有 §432-464 段按新序排列。把 `rematch_pseudo_classes(scene);`（现 459）移到 `solve(...)` 之前，`compute_world_transforms` 保留在 scroll 之后：

```rust
// 3. process（仲裁 + 拖拽写 scroll_pos；hit_test 读【上帧】world_transforms）
let input = std::mem::take(&mut self.pending_input);
let mut ptr_out = self.pointer_state.process(scene, &input);
out.append(&mut ptr_out);
// 4. scroll.update
let wheels = std::mem::take(&mut self.pending_wheel);
for w in &wheels { crate::scroll::apply_wheel_to_hit(scene, *w); }
crate::scroll::advance_all(dt, scene);
// 5. 键盘
let keys = std::mem::take(&mut self.pending_keys);
crate::input::process_keys(scene, &keys, &mut out);
self.last_events = out;
// 6. 伪类重匹配（提到 solve 前：改 taffy_style/transform/colors，本帧全部消费）
rematch_pseudo_classes(scene);
// 7. solve（读 rematch 后的 taffy_style → layout_rect）
solve(scene, &self.font, self.root_size, &self.image_sizes);
// 8. content_size 填充
crate::scroll::refresh_content_sizes(scene);
// 9. compute_world_transforms（读 rematch 后 transform + scroll_pos → world）
crate::scene::transform::compute_world_transforms(scene);
// 10. build
let (frame, new_hashes) = build_render_nodes(scene, &self.font, &self.prev_node_hashes, &self.image_sizes);
self.prev_node_hashes = new_hashes;
frame
```

> ⚠ `tween.update` 与 `pending_focus_request` 仍在最前（§423-431 不动）。注意 `self.last_events = out;` 移到 rematch 前（events 在 process/scroll/keys 后已收集完）。

- [ ] **Step 4: 删 rematch 返回值**

`dynamic.rs:186`：签名 `pub fn rematch_pseudo_classes(scene: &mut Scene) -> bool` → `pub fn rematch_pseudo_classes(scene: &mut Scene)`。删除 `let mut any_layout_dirty = false;`（189）、`layout_changed` 计算与累积（223-228）、`any_layout_dirty` 返回（230）。保留写 `node.style = new_style;`。

```rust
// dynamic.rs rematch 尾部改为：
        let node = scene.get_mut(node_id).expect("live node");
        node.style = new_style;
    }
}
```

删除 dynamic.rs 测试中依赖返回值的断言：`rematch_layout_dirty_when_size_changes`（323）改为只验 `style.taffy_style.size.width` 被改（删 `assert!(changed)`）；`rematch_no_dirty_when_only_visual_changes`（341）、`hover_pseudo_changes_background_color`（291 的 `assert!(!changed)`）、`focus_pseudo_matches_focused_node`（440）、三个 `background_*_change_is_visual`（509/527/546）——删各自 `let changed =` 与 `assert!(...changed...)` 行，保留 style 断言。

- [ ] **Step 5: 跑测试确认通过 + 无回归**

Run: `cargo test -p loomgui_core --features parse`
Expected: PASS（新测试通过；scroll/hit/tween 相关测试无回归）

- [ ] **Step 6: Commit**

```bash
git add loomgui_core/src/stage.rs loomgui_core/src/style/dynamic.rs
git commit -m "$(cat <<'EOF'
refactor(core): 支柱1 tick 时序——rematch 提到 solve 前（修 B1/坑103）

rematch 改 taffy_style/transform/colors 三类，旧顺序排在 solve/compute 后 →
:hover{border}/:active{scale} 当帧丢。提到 solve 前，三类当帧全生效。
删 rematch any_layout_dirty 返回值（solve 每帧全量，无需 dirty 驱动）。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: 支柱2 — payload_hash 全量（删采样）

**Files:**
- Modify: `loomgui_core/src/render/dirty.rs:11-89`（node_hash → payload_hash 全量）
- Test: `loomgui_core/src/render/dirty.rs` tests

**Interfaces:**
- Consumes: `RenderNode`, `NodePayload::{Mesh, Text}`（node.rs）
- Produces: `pub fn payload_hash(rn: &RenderNode) -> u64`（仅几何：Mesh 全量 verts/uvs/colors/indices/image_path/program/color_matrix；Text 全量 font_size/color/所有 glyph codepoint+x+y）

- [ ] **Step 1: 写失败测试（"hello"→"helps" 全量必变）**

替换 dirty.rs 现有 text hash 测试区，加：

```rust
#[test]
fn payload_hash_full_text_no_collision() {
    // "hello"→"helps"：首字 h/5 字/首字坐标同——旧采样 hash 撞。全量必变。
    let a = text_rn_content(16.0, [1.0;4], &[104,101,108,108,111]); // hello
    let b = text_rn_content(16.0, [1.0;4], &[104,101,108,112,115]); // helps
    assert_ne!(payload_hash(&a), payload_hash(&b), "全量 codepoint → hash 变");
}
```

加 helper（构造指定 codepoint 序列的 Text RenderNode，每 glyph x=i*10）：

```rust
fn text_rn_content(font_size: f32, color: [f32;4], cps: &[u32]) -> RenderNode {
    let glyphs: Vec<Glyph> = cps.iter().enumerate().map(|(i,&cp)| Glyph {
        glyph_id: 1, codepoint: cp, x: i as f32 * 10.0, y: 0.0,
        bearing_x: 0.0, bearing_y: 0.0,
    }).collect();
    let layout = TextLayout { text_width: 100.0, text_height: 20.0,
        lines: vec![Line { y: 0.0, height: 20.0, baseline: 16.0, width: 100.0,
            runs: vec![GlyphRun { font_size, glyphs }] }] };
    RenderNode {
        node_id: 0, parent_id: None, visible: true, alpha: 1.0, grayed: false,
        color_tint: [1.0;4], world_matrix: IDENTITY, blend: BlendMode::Normal,
        mask_context: MaskContext(0), sort_key: 0,
        payload: NodePayload::Text { layout, font_size, color, program: 1 },
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core payload_hash_full_text_no_collision`
Expected: FAIL（函数 `payload_hash` 未定义）

- [ ] **Step 3: 实现 payload_hash 全量**

在 dirty.rs 新增 `payload_hash`（保留旧 `node_hash` 暂不删，T4 再删调用点）。**只 hash 几何，不含 world/alpha/表头**：

```rust
/// 几何轴 hash：payload 全量（verts/uvs/colors/indices/image_path/program/color_matrix
/// 或 font_size/color/全量 glyph）。不含 world_matrix/alpha/sort/mask（那是 header_hash）。
/// 全量——不采样，根治漏字段类 bug（坑 56/75/76）。
pub fn payload_hash(rn: &RenderNode) -> u64 {
    let mut h = DefaultHasher::new();
    match &rn.payload {
        NodePayload::Mesh { verts, uvs, colors, indices, image_path, program, color_matrix } => {
            1u8.hash(&mut h); // 判别
            image_path.hash(&mut h);
            program.hash(&mut h);
            for &v in color_matrix.iter() { v.to_le_bytes().hash(&mut h); }
            for v in verts { v[0].to_le_bytes().hash(&mut h); v[1].to_le_bytes().hash(&mut h); }
            for u in uvs { u[0].to_le_bytes().hash(&mut h); u[1].to_le_bytes().hash(&mut h); }
            for c in colors { for &x in c.iter() { x.to_le_bytes().hash(&mut h); } }
            for &ix in indices { ix.hash(&mut h); }
        }
        NodePayload::Text { layout, font_size, color, program } => {
            2u8.hash(&mut h);
            font_size.to_le_bytes().hash(&mut h);
            program.hash(&mut h);
            for &v in color.iter() { v.to_le_bytes().hash(&mut h); }
            for line in &layout.lines {
                for run in &line.runs {
                    run.font_size.to_le_bytes().hash(&mut h);
                    for g in &run.glyphs {
                        g.codepoint.hash(&mut h);
                        g.x.to_le_bytes().hash(&mut h);
                        g.y.to_le_bytes().hash(&mut h);
                    }
                }
            }
        }
    }
    h.finish()
}
```

> ⚠ 此处 `match` 只有 Mesh/Text 两臂——依赖 T4 删除 `NodePayload::Unchanged`。若 T2 先做（Unchanged 尚存），临时加 `NodePayload::Unchanged => 0u64,` 一臂，T4 删。**本计划顺序 T2 在 T4 前，故加此临时臂**：

```rust
        NodePayload::Unchanged => { 0u64.hash(&mut h); } // 临时，T4 删除 Unchanged 变体后移除
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core payload_hash`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add loomgui_core/src/render/dirty.rs
git commit -m "$(cat <<'EOF'
feat(core): 支柱2 payload_hash 全量（根治漏字段 hash 碰撞坑56/75/76）

新增 payload_hash：几何轴全量 hash（所有 verts/uv/glyph codepoint），
不采样。"hello"→"helps" 不再撞 hash。旧 node_hash 暂存，T4 删调用点。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: 支柱2 — header_hash（表头轴）

**Files:**
- Modify: `loomgui_core/src/render/dirty.rs`（新增 header_hash）
- Test: `loomgui_core/src/render/dirty.rs` tests

**Interfaces:**
- Consumes: `RenderNode`
- Produces: `pub fn header_hash(rn: &RenderNode) -> u64`（world_matrix + visible + sort_key + mask_context + color_tint + blend；**alpha 暂不含**，T7 加）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn header_hash_world_matrix_change() {
    let a = mesh_rn(Some("a.png"), 1.0, [1.0;4]);
    let mut b = mesh_rn(Some("a.png"), 1.0, [1.0;4]);
    b.world_matrix = [1.0,0.0,0.0,1.0,5.0,0.0]; // tx=5
    assert_ne!(header_hash(&a), header_hash(&b), "world 变 → header_hash 变");
}

#[test]
fn header_hash_ignores_payload() {
    // 几何变、表头不变 → header_hash 相等（payload 归 payload_hash）。
    let a = mesh_rn(Some("a.png"), 1.0, [1.0;4]);
    let mut b = mesh_rn(Some("a.png"), 1.0, [1.0;4]);
    if let NodePayload::Mesh { verts, .. } = &mut b.payload { verts[0] = [9.0,9.0]; }
    assert_eq!(header_hash(&a), header_hash(&b), "几何变不影响 header_hash");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core header_hash`
Expected: FAIL（`header_hash` 未定义）

- [ ] **Step 3: 实现 header_hash**

```rust
/// 表头轴 hash：world_matrix + visible + sort_key + mask_context + color_tint + blend。
/// 廉价属性——变了 C# 只需改 GO transform / 材质，不碰 mesh。alpha 见 T7（剥离后加入）。
pub fn header_hash(rn: &RenderNode) -> u64 {
    let mut h = DefaultHasher::new();
    for &v in rn.world_matrix.iter() { v.to_le_bytes().hash(&mut h); }
    rn.visible.hash(&mut h);
    rn.sort_key.hash(&mut h);
    rn.mask_context.0.hash(&mut h);
    for &v in rn.color_tint.iter() { v.to_le_bytes().hash(&mut h); }
    (match rn.blend { crate::render::node::BlendMode::Normal => 0u8 }).hash(&mut h);
    h.finish()
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core header_hash`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add loomgui_core/src/render/dirty.rs
git commit -m "$(cat <<'EOF'
feat(core): 支柱2 header_hash（表头轴，与 payload_hash 正交）

world_matrix/visible/sort_key/mask/color_tint/blend。廉价属性变→C#只改GO/材质。
alpha 待 T7 剥离顶点色后加入。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: 支柱3 core — ChangeLevel + 删 Unchanged + merge 后按 node_id 算级别

**Files:**
- Modify: `loomgui_core/src/render/node.rs:38-56`（删 Unchanged，加 ChangeLevel enum + RenderNode 字段）
- Modify: `loomgui_core/src/render/mod.rs:95-316`（build_render_nodes：删首帧 Unchanged 占位逻辑；merge 后按 node_id 算双 hash 定 change_level）
- Modify: `loomgui_core/src/render/merge.rs`（match 臂删 Unchanged）
- Modify: `loomgui_core/src/render/dirty.rs`（删旧 node_hash + 临时 Unchanged 臂）
- Modify: `loomgui_core/src/stage.rs`（prev_node_hashes 类型改 `HashMap<u32,(u64,u64)>`）
- Test: `loomgui_core/src/render/mod.rs` tests

**Interfaces:**
- Consumes: `payload_hash`, `header_hash`（T2/T3）
- Produces:
  - `pub enum ChangeLevel { Skip, Header, Full }`（`#[repr(u8)]`：Skip=0, Header=1, Full=2）
  - `RenderNode.change_level: ChangeLevel` 字段
  - `NodePayload` 只剩 `Mesh | Text`（删 Unchanged）
  - `build_render_nodes(scene, font, prev: &HashMap<u32,(u64,u64)>, image_sizes) -> (FrameData, HashMap<u32,(u64,u64)>)`
  - `Stage.prev_node_hashes: HashMap<u32,(u64,u64)>`

- [ ] **Step 1: 写失败测试（SKIP/HEADER/FULL 三级）**

加到 render/mod.rs tests：

```rust
#[test]
fn change_level_skip_header_full() {
    use crate::render::node::ChangeLevel;
    let mut scene = Scene::from_nodes(vec![container_node(
        0, None, Rect { x:0.0,y:0.0,w:10.0,h:10.0 }, Some([1.0,0.0,0.0,1.0]))], vec![]);
    let font = test_font().expect("font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    // 首帧：无基线 → FULL
    let (f1, h1) = build_render_nodes(&scene, &font, &std::collections::HashMap::new(), &empty_sizes());
    assert_eq!(f1.nodes[0].change_level, ChangeLevel::Full, "首帧 FULL");
    // 第二帧不变 → SKIP
    let (f2, h2) = build_render_nodes(&scene, &font, &h1, &empty_sizes());
    let n = f2.nodes.iter().find(|n| n.node_id == 0).unwrap();
    assert_eq!(n.change_level, ChangeLevel::Skip, "不变 → SKIP");
    // 第三帧只挪 world（平移）→ HEADER
    scene.get_mut(scene.roots[0]).unwrap().layout_rect.x = 50.0;
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (f3, h3) = build_render_nodes(&scene, &font, &h2, &empty_sizes());
    let n = f3.nodes.iter().find(|n| n.node_id == 0).unwrap();
    assert_eq!(n.change_level, ChangeLevel::Header, "只平移 → HEADER（payload 不变）");
    // 第四帧改背景色 → FULL
    scene.get_mut(scene.roots[0]).unwrap().style.background_color = Some([0.0,1.0,0.0,1.0]);
    let (f4, _) = build_render_nodes(&scene, &font, &h3, &empty_sizes());
    let n = f4.nodes.iter().find(|n| n.node_id == 0).unwrap();
    assert_eq!(n.change_level, ChangeLevel::Full, "颜色变 → FULL");
}
```

> ⚠ 第三帧「只平移 → HEADER」成立前提：纯平移节点 rect 用 world.tx（mod.rs:147-152），verts re-base 后不变 → payload_hash 不变。此测锁定这一点。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core --features parse change_level_skip_header_full`
Expected: FAIL（`ChangeLevel` / `change_level` 字段不存在）

- [ ] **Step 3: node.rs 删 Unchanged，加 ChangeLevel + 字段**

`node.rs`：
```rust
/// 帧级变更级别（与 payload_kind「是什么」正交，表示「这帧变了什么」）。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum ChangeLevel {
    Skip = 0,   // 表头+几何均未变：C# 保留 GO，不碰
    Header = 1, // 只表头变：C# 只改 GO transform/材质，不重建 mesh
    Full = 2,   // 几何变：C# 重建 mesh
}

#[derive(Debug, Clone, Serialize)]
pub enum NodePayload {
    // Unchanged 删除——"本帧没变"改由 ChangeLevel::Skip 表达（正交轴）
    Mesh { /* 原字段不变 */ verts: Vec<[f32;2]>, uvs: Vec<[f32;2]>, colors: Vec<[f32;4]>,
           indices: Vec<u32>, image_path: Option<String>, program: u32, color_matrix: [f32;20] },
    Text { layout: crate::text::layout::TextLayout, font_size: f32, color: [f32;4], program: u32 },
}
```
RenderNode 加字段 `pub change_level: ChangeLevel,`（放 payload 前）。

- [ ] **Step 4: 修所有 RenderNode 构造点 + match Unchanged 点**

编译错会指出所有点。逐一修：
- `render/mod.rs:111-124` 预分配 Unchanged 占位 → 删除整个预分配逻辑，改为直接 push 真节点（见 Step 5）。
- `render/mod.rs:60-84` thumb_render_node、mesh/text 构造 → 加 `change_level: ChangeLevel::Full`（thumb 每帧算，简单起见 Full；thumb 少无所谓）。
- `render/merge.rs:86` merge_batch 构造 RenderNode → 加 `change_level: ChangeLevel::Full`（merged 恒 Full，D2）；`mesh_key` 的 `_ => None` 臂已覆盖非 Mesh，无需改 Unchanged。
- `dirty.rs` 删旧 `node_hash` 函数 + 其测试中 `unchanged_returns_zero`；payload_hash 删临时 `Unchanged` 臂。
- `blob.rs` 的 `NodePayload::Unchanged` 臂（180-190）→ T5 处理（暂时编译错，T5 修）。**T4 先让 core 编译过，ffi crate 编译错留给 T5。**

- [ ] **Step 5: mod.rs build_render_nodes 重写 emit + merge 后算级别**

核心改动——hash 移到 merge 后按 node_id：

```rust
pub fn build_render_nodes(
    scene: &Scene, font: &Font,
    prev: &std::collections::HashMap<u32,(u64,u64)>,
    image_sizes: &ImageSizeTable,
) -> (FrameData, std::collections::HashMap<u32,(u64,u64)>) {
    // 1. 直接构造真节点（不再预分配 Unchanged 占位）。change_level 先占位 Full，末尾统一定级。
    let mut nodes: Vec<RenderNode> = Vec::new();
    for n in scene.nodes.values() {
        // ... 原 match n.kind 构造 Mesh/Text（同现有 157-282），但每个 RenderNode 加
        //     change_level: ChangeLevel::Full（占位，末尾重定）。
        nodes.push(rn);
    }
    // 2. batch / merge / thumb（顺序同现有 291-314）。
    let clips = batch::assign_sort_keys(scene, &mut nodes, &id_to_pos);
    let max_sort = nodes.iter().map(|n| n.sort_key).max().unwrap_or(0);
    batch::reorder_for_batching(scene, &mut nodes);
    let mut nodes = merge::merge_meshes(nodes);
    // ... thumb 追加（同现有 298-314）
    // 3. merge 后按 node_id 算双 hash → 定级别（D2/D3）。
    let mut new_hashes = std::collections::HashMap::with_capacity(nodes.len());
    for rn in &mut nodes {
        let hh = crate::render::dirty::header_hash(rn);
        let ph = crate::render::dirty::payload_hash(rn);
        rn.change_level = match prev.get(&rn.node_id) {
            None => ChangeLevel::Full,                       // D3 无基线
            Some(&(ph_hh, ph_ph)) => {
                if ph_ph != ph { ChangeLevel::Full }         // 几何变
                else if ph_hh != hh { ChangeLevel::Header }  // 只表头变
                else { ChangeLevel::Skip }
            }
        };
        new_hashes.insert(rn.node_id, (hh, ph));
    }
    (FrameData { nodes, clips }, new_hashes)
}
```

> ⚠ thumb 节点 node_id 是 sentinel（`nid | V_THUMB_FLAG`），每帧算 hash 会进 new_hashes，正常（下帧比对）。thumb 每帧 Full 无所谓（数量少）。
> ⚠ `id_to_pos`（原 104-109）batch 仍需要——保留构造，但基于初次 `nodes` 顺序。merge 后 nodes 变短，`id_to_pos` 只在 assign_sort_keys 用（merge 前），OK。

删除现有测试中依赖 `NodePayload::Unchanged` 的：`build_first_frame_all_emit_no_unchanged` / `build_static_frame_emits_unchanged` / `build_changed_frame_re_emits` / `build_reload_clears_baseline`——改写为断言 `change_level`（新测试 change_level_skip_header_full 已覆盖首帧 FULL + SKIP + FULL；补一个 reload → 全 FULL）。

- [ ] **Step 6: stage.rs prev_node_hashes 类型改 HashMap**

`stage.rs`：字段 `prev_node_hashes: Vec<u64>` → `std::collections::HashMap<u32,(u64,u64)>`；初始化 `Vec::new()` → `HashMap::new()`；`.clear()` 调用点（lib.rs:113 也有一处）不变（HashMap 也有 clear）。tick_and_render 调 build_render_nodes 传 `&self.prev_node_hashes`，接收 `new_hashes` 存回。

- [ ] **Step 7: 跑测试确认通过**

Run: `cargo test -p loomgui_core --features parse`
Expected: PASS（change_level 三级测试通过；无 Unchanged 引用；merge 测试仍过）

- [ ] **Step 8: Commit**

```bash
git add loomgui_core/src/render/node.rs loomgui_core/src/render/mod.rs \
        loomgui_core/src/render/merge.rs loomgui_core/src/render/dirty.rs \
        loomgui_core/src/stage.rs
git commit -m "$(cat <<'EOF'
feat(core): 支柱3 ChangeLevel 机制——删 Unchanged 变体，merge 后按 node_id 定级

NodePayload 删 Unchanged（"变了什么"移到正交的 ChangeLevel::Skip/Header/Full）。
hash 移到 merge 之后、按 node_id 键算（D2：merged 节点移动烤进 verts → 自动 Full）。
prev_node_hashes: Vec<u64> → HashMap<u32,(header,payload)>。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: 支柱3 FFI — blob change_level 列 + SKIP/HEADER 不写 arena

**Files:**
- Modify: `loomgui_ffi_c/src/blob.rs`（VERSION 8；加 change_level 列；arena 按级别写；删 Unchanged 臂）
- Test: `loomgui_ffi_c/src/blob.rs` tests

**Interfaces:**
- Consumes: `RenderNode.change_level: ChangeLevel`（T4）、`NodePayload::{Mesh,Text}`
- Produces: blob v8：21 列（change_level 作第 21 列 u8）；level!=Full 的节点 mesh_off/len=0（不写 arena）；header 6 列 + 公共头照常全写。

- [ ] **Step 1: 写失败测试（change_level round-trip + HEADER 不写 arena）**

blob.rs tests：

```rust
#[test]
fn change_level_column_round_trips() {
    use loomgui_core::render::node::ChangeLevel;
    let mut skip = mesh_node(0, None, 0.0, 0.0, 5.0, 5.0);
    skip.change_level = ChangeLevel::Skip;
    let mut header = mesh_node(1, None, 0.0, 0.0, 5.0, 5.0);
    header.change_level = ChangeLevel::Header;
    let mut full = mesh_node(2, None, 0.0, 0.0, 5.0, 5.0);
    full.change_level = ChangeLevel::Full;
    let blob = build_blob(&frame(&[skip, header, full]));
    let view = TestView::parse(&blob);
    assert_eq!(view.version(), 8, "VERSION=8");
    assert_eq!(view.change_level(0), 0, "Skip=0");
    assert_eq!(view.change_level(1), 1, "Header=1");
    assert_eq!(view.change_level(2), 2, "Full=2");
    // SKIP/HEADER 不写 arena → mesh_len=0；FULL 写 arena → mesh_len>0。
    assert_eq!(view.mesh_len_col(0), 0, "Skip 不写 arena");
    assert_eq!(view.mesh_len_col(1), 0, "Header 不写 arena");
    assert!(view.mesh_len_col(2) > 0, "Full 写 arena");
}
```

> `mesh_node` helper 已存在（blob.rs:297），但需加 `change_level` 字段（默认 Full）——同 Step 4 修所有构造点。TestView 加 `change_level(i)` + `mesh_len_col(i)` 读取（Step 3）。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_ffi_c change_level_column_round_trips`
Expected: FAIL（编译错：VERSION!=8 / change_level 方法缺）

- [ ] **Step 3: blob.rs 加列 + 按级别写 arena**

- `VERSION: u32 = 8;`
- `columns` 数组末尾加 `("change_level", 1)`（21 列）。加 `col_change_level` Vec + push。
- match 臂：删 `NodePayload::Unchanged`（180-190 整段删）。
- **arena 写入按级别门控**：Mesh/Text 臂顶部判断 `let write_arena = matches!(rn.change_level, ChangeLevel::Full);`。SKIP/HEADER 时 mesh_arena/text_arena 不 extend，col_mesh_off/len 写 0（同旧 Unchanged 占位逻辑）。仍写 change_level 列 + 公共头 + payload_kind（表明"是什么"）。

```rust
// 每节点循环顶部：
col_change_level.push(rn.change_level as u8);
let write_arena = matches!(rn.change_level, loomgui_core::render::node::ChangeLevel::Full);
match &rn.payload {
    NodePayload::Mesh { .. } => {
        col_kind.push(1);
        // ... path_idx/program/color_matrix 照写（表头信息，HEADER 也要）
        if write_arena {
            // ... 原 mesh_arena 写入（seg_off/verts/uvs/colors/indices）
            col_mesh_off/len = 实段;
        } else {
            col_mesh_off/len = 0;  // SKIP/HEADER 不写几何
        }
        col_text_off/len = 0;
    }
    NodePayload::Text { .. } => {
        col_kind.push(2);
        // ... program/color_matrix/path_idx 占位
        if write_arena { /* 写 text_arena */ } else { col_text_off/len = 0; }
        col_mesh_off/len = 0;
    }
}
```

- header_len 计算：列数 20→21，`num_col_offsets` 自动更新（用 `columns.len()`）。加 `col_change_level` 进 `col_bufs`。
- TestView 加：`fn change_level(&self,i)->u8 { self.buf[self.col_off[20] + i] }`；`fn mesh_len_col(&self,i)->u32`（读 col_off[14]）；col_off 数组 `[usize;21]`，parse 循环 `0..21`。

- [ ] **Step 4: 修 blob.rs 所有 mesh_node/unchanged_node helper**

`unchanged_node`（blob.rs:342）删除（无 Unchanged 变体）。所有 RenderNode 构造 helper 加 `change_level: ChangeLevel::Full`。用到 `unchanged_node` 的测试（`program_column_round_trips` 342/472、`blob_unchanged_kind_is_zero` 1027）改用 mesh_node 或删除 Unchanged 专项断言。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_ffi_c`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add loomgui_ffi_c/src/blob.rs
git commit -m "$(cat <<'EOF'
feat(ffi): 支柱3 blob v8 change_level 列 + SKIP/HEADER 不写 arena

第 21 列 change_level(u8)。SKIP/HEADER 节点 mesh/text arena 不写（省带宽），
公共头 6 矩阵列 + payload_kind 照常全传（HEADER 只挪所需数据已在）。删 Unchanged 臂。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: alpha 剥离 — shader `_Alpha` uniform + blob 不烤 alpha

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Shaders/LoomGUI-Unlit.shader`（Properties + CBUFFER + frag）
- Modify: `loomgui_ffi_c/src/blob.rs:117-129`（colors 不 `× rn.alpha`）
- Test: `loomgui_ffi_c/src/blob.rs`（验 colors.a 不再烤 alpha）

**Interfaces:**
- Consumes: 公共头 alpha 列（blob.rs:77，已传，shader 之前没用）
- Produces: blob mesh colors[].a = 原始 bg.a（不乘节点 alpha）；shader `col.a *= _Alpha`

- [ ] **Step 1: 写失败测试（blob colors.a 不烤 alpha）**

改现有 `mesh_colors_bake_alpha_not_tint`（blob.rs:571）语义——alpha 不再烤：

```rust
#[test]
fn mesh_colors_no_longer_bake_alpha() {
    // alpha 剥离后：colors.a = 原始 bg.a（不乘节点 alpha）。节点 alpha 走 _Alpha uniform。
    let blob = build_blob(&frame(&[mesh_node_tinted(0, [0.5;4], 0.5, [1.0,0.0,0.0,1.0])]));
    let view = TestView::parse(&blob);
    let colors = view.mesh_colors(0);
    assert_eq!(colors[0], [1.0, 0.0, 0.0, 1.0], "colors.a=原始1.0（alpha 0.5 不烤，走 uniform）");
    // alpha 列仍保留 0.5（供 C# SetPropertyBlock _Alpha）。
    let alpha_o = view.col_off[3];
    assert_eq!(f32::from_le_bytes(view.buf[alpha_o..alpha_o+4].try_into().unwrap()), 0.5);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_ffi_c mesh_colors_no_longer_bake_alpha`
Expected: FAIL（colors[0].a==0.5，旧代码烤了 alpha）

- [ ] **Step 3: blob.rs 删 alpha 烘焙**

blob.rs:120-124：
```rust
for c in colors {
    // alpha 剥离：colors 原样写，节点 alpha 走 _Alpha uniform（C# SetPropertyBlock）。
    mesh_arena.extend_from_slice(&c[0].to_le_bytes());
    mesh_arena.extend_from_slice(&c[1].to_le_bytes());
    mesh_arena.extend_from_slice(&c[2].to_le_bytes());
    mesh_arena.extend_from_slice(&c[3].to_le_bytes());
}
```

- [ ] **Step 4: shader 加 `_Alpha`**

`LoomGUI-Unlit.shader`：
- Properties（第 25 行后）加 `_Alpha ("Alpha", Float) = 1`
- CBUFFER（第 62 行 `_CFOff` 后）加 `float _Alpha;`
- frag：在 `#ifdef CLIPPED`（124 行）**之前**加 `col.a *= _Alpha;`

```hlsl
                // 节点 opacity（从顶点色剥离，per-renderer MPB）。alpha 剥离后 colors.a 不含节点 alpha。
                col.a *= _Alpha;
                #ifdef CLIPPED
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_ffi_c mesh_colors_no_longer_bake_alpha`
Expected: PASS

> ⚠ merged 节点：merge_batch 现在把 alpha 烤进 colors.a（merge.rs:74-78）。alpha 剥离后 merged 节点若含不同 alpha 的子节点，uniform 只能一个值——T9 处理（merge DrawState key 加 alpha）。T6 先让单节点 alpha 走 uniform；merge×alpha 由 T9 补齐。**T6 不改 merge.rs**（留 T9），故 merged 场景暂时 alpha 仍烤（T9 修）。

- [ ] **Step 6: Commit**

```bash
git add loomgui_ffi_c/src/blob.rs loomgui_unity/Assets/LoomGUI/Shaders/LoomGUI-Unlit.shader
git commit -m "$(cat <<'EOF'
feat: alpha 剥离成 _Alpha uniform（opacity 动画走 HEADER，不重建 mesh）

blob colors.a 不再 × 节点 alpha；shader frag col.a *= _Alpha（CLIPPED 前）。
套 _ObjM/_CF 现成 MPB 模式。merge×alpha 由 T9 补（DrawState key 加 alpha）。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: alpha 归 header_hash（HEADER 级别）

**Files:**
- Modify: `loomgui_core/src/render/dirty.rs`（header_hash 加 alpha）
- Test: `loomgui_core/src/render/dirty.rs`

**Interfaces:**
- Consumes: `RenderNode.alpha`
- Produces: header_hash 含 alpha → alpha 变落 HEADER（不再 FULL）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn header_hash_alpha_change() {
    let a = mesh_rn(Some("a.png"), 1.0, [1.0;4]);
    let b = mesh_rn(Some("a.png"), 0.5, [1.0;4]); // alpha 0.5
    assert_ne!(header_hash(&a), header_hash(&b), "alpha 变 → header_hash 变（HEADER）");
}

#[test]
fn payload_hash_ignores_alpha() {
    // alpha 归 header，payload_hash 不含 alpha（否则 alpha 变会误落 FULL）。
    let a = mesh_rn(Some("a.png"), 1.0, [1.0;4]);
    let b = mesh_rn(Some("a.png"), 0.5, [1.0;4]);
    assert_eq!(payload_hash(&a), payload_hash(&b), "payload_hash 不含 alpha");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core header_hash_alpha_change`
Expected: FAIL（header_hash 不含 alpha → 两者相等）

- [ ] **Step 3: header_hash 加 alpha**

dirty.rs `header_hash` 加一行（visible 后）：
```rust
    rn.alpha.to_le_bytes().hash(&mut h);
```
确认 `payload_hash`（T2）**不含** alpha（mesh_rn helper 构造的 colors 不依赖节点 alpha 字段——payload_hash 只 hash `colors` 数组值，节点 alpha 是独立字段，不进 payload_hash。测试 `payload_hash_ignores_alpha` 锁定）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core header_hash payload_hash`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add loomgui_core/src/render/dirty.rs
git commit -m "$(cat <<'EOF'
feat(core): alpha 归 header_hash → opacity 动画走 HEADER

alpha 剥离顶点色后（T6），alpha 变只需 SetPropertyBlock，落 HEADER 不重建 mesh。
payload_hash 不含 alpha（避免误 FULL）。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: C# — FrameBlob 21 列 + MirrorPool 三分支 + `_Alpha` MPB

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/FrameBlob.cs`（ExpectedVersion 8；21 列；ChangeLevel 读取；header offset +4）
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/MirrorPool.cs`（三分支；UpdateHeader 抽出；`_Alpha` MPB）
- Test: 家里机 Unity PlayMode（无离线 C# 测试框架）

**Interfaces:**
- Consumes: blob v8（change_level 列 @ col_off[20]；alpha 列 @ col_off[3]）
- Produces: MirrorPool 按 change_level 三分支；`_Alpha` 经 MPB 传每 renderer

- [ ] **Step 1: FrameBlob.cs 升 v8 + 加 ChangeLevel**

- `ExpectedVersion = 8`
- 列注释加 `20=change_level(u8)`
- header 布局：21 列后 arena header offset 全 +4（原 `12+20*4=92` → `12+21*4=96`）。逐个更新 `MeshArenaOff`/`TextArenaOff`/`TextArenaLen`/`ClipTableOff`/`ClipTableLen`/`PathTableOff`/`PathTableLen` 的常量（`20*4` → `21*4`）。
- 加读取：`public byte ChangeLevel(int i) => _buf[ColOff(20) + i];`
- header 注释里 header 长度 124 → 128（21 列多 4B）。

- [ ] **Step 2: MirrorPool.cs 三分支**

替换 Sync 主循环 §68-197。抽出 `UpdateHeader`（localPosition/rotation/scale + sortingOrder + 材质 + _ObjectMatrix + _Alpha）与 `UploadMeshOrText`（现 mesh/text 重建）：

```csharp
byte kind = blob.PayloadKind(i);
byte level = blob.ChangeLevel(i);   // 0=Skip 1=Header 2=Full
uint id = blob.NodeId(i);

if (level == 0) { // SKIP
    if (_pool.TryGetValue(id, out var ro0)) ro0.Stale = false;
    continue;
}
if (kind != 1 && kind != 2) continue; // 防御

if (!_pool.TryGetValue(id, out var ro)) {
    ro = NewRenderObj(root); ro.LastNodeId = id; _pool[id] = ro;
    level = 2; // 新建的 GO 无 mesh → 强制 FULL（无视 blob 的 HEADER）
}
ro.Stale = false;
ro.IsText = kind == 2;

UpdateHeader(ro, blob, i, root, mm, sprites, fallback, font); // localPos/材质/_ObjM/_Alpha/clip
if (level == 2) UploadMeshOrText(ro, blob, i, mm, sprites, fallback, font, fontDirty);
```

> ⚠ **HEADER 但 pool 无此 GO**（如上帧被 merge 掉、这帧单独出现）→ 上面 `level=2` 兜底强制 FULL，避免只挪空 mesh。

- [ ] **Step 3: `_Alpha` 进 MPB**

`UpdateHeader` 内（材质设置后），无条件 SetPropertyBlock `_Alpha`：

```csharp
// alpha 剥离顶点色（T6）：节点 alpha 走 _Alpha uniform（每 renderer MPB）。
ro.Mpb ??= new MaterialPropertyBlock();
ro.Mr.GetPropertyBlock(ro.Mpb);       // 保留已设的 _ObjM/_CF
ro.Mpb.SetFloat("_Alpha", blob.Alpha(i));
ro.Mr.SetPropertyBlock(ro.Mpb);
```

> ⚠ 现有 `SetObjectMatrix`/`SetColorFilterMatrix` 也用 ro.Mpb + SetPropertyBlock。合并成一次 GetPropertyBlock→set 多个→SetPropertyBlock，避免互相覆盖（MPB SetPropertyBlock 是整块替换）。重构这三处为「UpdateHeader 末尾一次性 GetPropertyBlock + set _ObjM/_CF/_Alpha + SetPropertyBlock」。

- [ ] **Step 4: 重编 dll + 拷贝（Unity 关闭下）**

Run:
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
```
Expected: 编译成功；`md5sum` 两 dll 一致（坑 10）。

- [ ] **Step 5: 家里机 Unity PlayMode 验收**

（家里机执行——公司机无法验）验收清单：
1. hover 变色刷新、`:active` 缩放当帧可见（支柱1）。
2. 滑动列表：Profiler 确认无逐帧 UploadMesh（支柱3 HEADER）。
3. opacity tween：Profiler 无 UploadMesh（alpha uniform）。
4. 图片/文字正常显示（v8 blob 解析正确）。

- [ ] **Step 6: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/FrameBlob.cs \
        loomgui_unity/Assets/LoomGUI/Runtime/MirrorPool.cs \
        loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "$(cat <<'EOF'
feat(unity): 支柱3 C# 三分支 SKIP/HEADER/FULL + _Alpha MPB

FrameBlob v8：21 列 + ChangeLevel(i)。MirrorPool 按级别三分支：
SKIP 保留 GO；HEADER 只更 localPosition/_ObjM/材质/_Alpha 不重建 mesh；FULL 重建。
新建/HEADER-无GO 兜底强制 FULL。alpha 走 _Alpha uniform。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: merge × alpha — DrawState key 加 alpha

**Files:**
- Modify: `loomgui_core/src/render/merge.rs:11-20`（mesh_key 加 alpha）、`60-85`（merge_batch 不烤 alpha）
- Test: `loomgui_core/src/render/merge.rs`

**Interfaces:**
- Consumes: `RenderNode.alpha`
- Produces: 不同 alpha 的节点不合并；merged 节点 colors.a 不烤 alpha（走 uniform，与 T6 一致）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn different_alpha_do_not_merge() {
    // alpha 剥离后：不同 alpha 不能合一个 draw call（uniform 单值）。
    let nodes = vec![
        mesh_node(1, Some("a.png"), 0, 1.0, 0.0),
        mesh_node(2, Some("a.png"), 1, 0.5, 100.0), // 不同 alpha
    ];
    let out = merge_meshes(nodes);
    assert_eq!(out.len(), 2, "不同 alpha → 不合并");
}

#[test]
fn same_alpha_still_merge_no_bake() {
    // 同 alpha 仍合并；但 colors.a 不再烤 alpha（走 uniform）。
    let nodes = vec![
        mesh_node(1, Some("a.png"), 0, 0.5, 0.0),
        mesh_node(2, Some("a.png"), 1, 0.5, 100.0),
    ];
    let out = merge_meshes(nodes);
    assert_eq!(out.len(), 1, "同 alpha 合并");
    if let NodePayload::Mesh { colors, .. } = &out[0].payload {
        for c in colors { assert!((c[3]-1.0).abs()<1e-6, "colors.a 不烤 alpha（原始1.0）"); }
    }
    assert!((out[0].alpha - 0.5).abs() < 1e-6, "merged.alpha=子 alpha（走 uniform）");
}
```

> `mesh_node` helper 的 colors 默认 `[1.0;4]`（merge.rs:125）——原始 a=1.0。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core different_alpha_do_not_merge same_alpha_still_merge_no_bake`
Expected: FAIL（现 mesh_key 不含 alpha → 合并；merge_batch 烤 alpha）

- [ ] **Step 3: mesh_key 加 alpha + merge_batch 不烤**

merge.rs `mesh_key` 返回类型加 alpha（用 bits 比较避免 f32 非 Eq）：
```rust
fn mesh_key(rn: &RenderNode) -> Option<(Option<String>, u32, u32, u32)> {
    match &rn.payload {
        NodePayload::Mesh { image_path, program, .. }
            if *program == 0 && crate::transform::is_pure_translation(&rn.world_matrix) =>
            Some((image_path.clone(), *program, rn.mask_context.0, rn.alpha.to_bits())),
        _ => None,
    }
}
```
`merge_batch`：colors 不烤 alpha（删 `col[3] *= alpha`），merged.alpha = 子 alpha（同 key 保证一致，取 `last.alpha`）：
```rust
    for &bi in batch {
        if let NodePayload::Mesh { verts:v, uvs:u, colors:c, indices:ix, .. } = &nodes[bi].payload {
            verts.extend_from_slice(v);
            uvs.extend_from_slice(u);
            colors.extend_from_slice(c);   // 不烤 alpha（走 uniform）
            for &ixv in ix { indices.push(ixv + base); }
            base += v.len() as u32;
        }
    }
    // ... RenderNode { alpha: last.alpha, change_level: ChangeLevel::Full, ... }
```
改 merge_batch 返回的 `alpha: 1.0` → `alpha: last.alpha`。更新现有测试 `two_same_drawstate_merge_into_one`（该测两节点 alpha 1.0/0.5 不同——改为都 1.0，或拆两测；断言 colors.a 不烤）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core --features parse`
Expected: PASS

- [ ] **Step 5: 重编 dll + Commit**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
git add loomgui_core/src/render/merge.rs loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "$(cat <<'EOF'
fix(core): merge×alpha——DrawState key 加 alpha，不同 alpha 不合并

alpha 剥离成 uniform（per-renderer 单值）后，不同 alpha 节点不能合一 draw call。
mesh_key 加 alpha.to_bits()；merge_batch 不再烤 alpha（走 _Alpha uniform）。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review 记录

**Spec 覆盖**：支柱1（T1）✓ 支柱2 全量 hash+双 hash（T2/T3/T7）✓ 支柱3 change_level 机制（T4/T5/T8）✓ alpha uniform §4.1（T6/T7/T8）✓ 颜色 rgb 不剥离 §4.2（不做，YAGNI）✓ merge×alpha D2（T9）✓ D2 hash 位置（T4）✓ D3 无基线（T4）✓。

**待实现者注意的顺序依赖**：
- T2 加临时 `Unchanged` 臂 → T4 删（已注明）。
- T4 让 ffi crate 编译错（blob Unchanged 臂）→ T5 修（已注明）。
- T6 单节点 alpha uniform，但 merge 仍烤 → T9 补齐（已注明）。
- T8 必须在 T4-T7 全完成后（依赖 v8 blob + change_level）。

**类型一致性**：`ChangeLevel { Skip=0, Header=1, Full=2 }` `#[repr(u8)]` 全程一致；`build_render_nodes` 新签名 `HashMap<u32,(u64,u64)>`（header, payload）在 T4 定义、T4 stage 消费；`header_hash`/`payload_hash` 命名 T2/T3 定义后一致。
