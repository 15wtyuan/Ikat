# SDF 文字效果 — Plan 2：effect 搬 shader 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 v1.8 文字效果（shadow/stroke/glow/blur）从已废的 bitmap 后处理路径搬到 SDF shader（单 quad 多 pass），恢复 Plan 1 降级的 showcase page_text C1-C4。

**Architecture:** 单一 SDF（Plan 1 产物）存 distance，effect 在 shader fragment 从 distance 单 pass 重建（对标 TMP_SDF_SSD.cginc）。effect 参数走新增 per-node `EffectBlock` 定长列（照 color_matrix 先例）→ FFI SOA → Unity MPB → shader uniform。顺手清掉 Plan 1 残留的文字 shadow/stroke 多 layer 骨架（保留 div box-shadow 共用的 BOX_SHADOW_FLAG）。

**Tech Stack:** Rust（loomgui_core / loomgui_ffi_c，edition 2021）、Unity HLSL（URP，LoomGUI-Unlit.shader ALPHA_MASK）、C#（FrameBlob.cs / MirrorPool.cs）。依赖钉版本（Plan 1 不变）。

## Global Constraints

（每个 task 的需求隐含以下约束，源自 spec `docs/superpowers/specs/2026-07-12-sdf-text-effects-design.md` + CLAUDE.md）

- **常量**：`SOURCE_SIZE = 48`、`SPREAD = 12`（Plan 1 已定，本 plan 不改）。
- **依赖钉版本**：`ab_glyph_rasterizer = "0.1"`、`etagere = "0.3"`、`ttf-parser = "0.20"`，不改版本。
- **Rust edition 2021**；FFI 入口绝不 panic（遇 None/越界优雅早返空帧，不 unwrap/expect）。
- **注释**：中文、说 WHY 不复述机制、**不引用内部编号或暗语**（"坑 N"、"对齐某 meta" 禁止）；代码/commit 英文。
- **改 Rust 后必须重编 + commit `.dll`**：`cargo build -p loomgui_ffi_c --release` → 拷贝到 `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Unity 关着）。本 plan Task 6 统一做（Task 1-5 改 Rust 的累积效果在 Task 6 一次性重编）。
- **测试门**：每 task 后 `cargo test -p loomgui_core` 全绿；不改围栏但跑 `cargo test -p loomgui_core --test fence_contract`。snapshot 用 `INSTA_UPDATE=always cargo test -p loomgui_core --test snapshot` 更新（Task 6）。
- **fmt/clippy**：每 task `cargo clippy -p loomgui_core --all-targets -- -D warnings` + `cargo fmt --all -- --check` 必过。
- **scope 边界**：本 plan 只做 effect 搬 shader + 清旧 layer。不改 base SDF（Plan 1 成果）、不改 8SSEDT、不改 GlyphKey、不改围栏标签/属性集。渐变字（`background-clip:text`）+ 装饰线（underline/strike）+ div box-shadow 不回归。
- **去 flags 约定**：`EffectBlock` 无 bitfield；effect 启用由参数隐含（`outline_width>0` / `underlay.color.a>0` / `glow_color.a>0` / `blur_width>0`）。shader 内 `if (param > 0.001)` 判断。
- **两机工作流**：公司机改 Rust + 重编 .dll；shader + C# 改动家里机拉代码即生效（shader 不重编 Rust；C# Unity 重编译）。

## File Structure

- **Modify** `loomgui_core/src/render/node.rs` — 新增 `EffectBlock` + `UnderlaySlot` struct（含 `to_bytes()` 序列化，DRY 供 blob + dirty 用）；`RenderNode` 加 `effect: EffectBlock` 字段。
- **Modify** `loomgui_core/src/render/mod.rs` — `pack_effects(&[FontEffect]) -> EffectBlock` 打包函数；`build_text_mesh` 启用 `_text_effects` 参数 + `TextMeshes.effect` 字段；`emit_text_node` 接 effect + 删 back/front layer push + 删 stroke 机制 + 改签名；删 `stroke_pairs`/`propagate_text_stroke_sort_keys`/`TEXT_STROKE_FRONT_FLAG`。
- **Modify** `loomgui_core/src/render/merge.rs` — 去掉 `TEXT_STROKE_FRONT_FLAG` 引用（只留 `BOX_SHADOW_FLAG`）。
- **Modify** `loomgui_core/src/render/dirty.rs` — `header_hash` 加 `effect` 采样。
- **Modify** `loomgui_core/src/render/tests.rs` — 新增 effect 打包 / mesh 产出 / dirty hash 测试。
- **Modify** `loomgui_core/tests/fence_contract.rs` — blob 列数 / VERSION 断言更新。
- **Modify** `loomgui_ffi_c/src/blob.rs` — columns 加 `effect_block` 列；`VERSION 10→11`；写出循环 extend effect_block bytes。
- **Modify** `loomgui_unity_package/Shaders/LoomGUI-Unlit.shader` — ALPHA_MASK fragment 单 pass 合成（face+outline+underlay×3+glow+blur）；新 Properties + CBUFFER uniforms。
- **Modify** `loomgui_unity_package/Runtime/FrameBlob.cs` — `EffectBlock(i)` 访问器（读 32 f32）。
- **Modify** `loomgui_unity_package/Runtime/MirrorPool.cs` — `program==1` 节点读 effect_block → MPB SetVector/SetFloat。
- **Modify** `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` — Task 6 重编拷贝。

---

### Task 1: `EffectBlock` struct + `pack_effects` 打包（core 数据层）

新增 `EffectBlock` / `UnderlaySlot`（render/node.rs，跟 RenderNode 一起）+ `pack_effects` 打包函数（render/mod.rs）。纯数据结构 + 纯函数，本 task **不接入** build_text_mesh / 不碰 FFI。

**Files:**
- Modify: `loomgui_core/src/render/node.rs`（加 EffectBlock + UnderlaySlot + to_bytes）
- Modify: `loomgui_core/src/render/mod.rs`（加 pack_effects 函数）
- Test: `loomgui_core/src/render/tests.rs`

**Interfaces:**
- Consumes: `crate::text::font_effect::FontEffect`（DSL 解析层已有，`Shadow{ox,oy,blur,color}` / `Stroke{w,color}` / `Glow{w,color}` / `Blur{w}`）。
- Produces: `EffectBlock`（含 `to_bytes() -> [u8; 128]`，供 Task 3 blob 序列化 + dirty hash 共用）；`pack_effects(&[FontEffect]) -> EffectBlock`（供 Task 2 build_text_mesh 调用）。

- [ ] **Step 1: 写失败测试**

`loomgui_core/src/render/tests.rs` 加（在现有 `mod tests` 内）：

```rust
    /// effect 打包：FontEffect → EffectBlock 槽位映射。
    /// Shadow→underlay（多重 ≤3，超 3 丢）、Stroke→outline、Glow→glow、Blur→blur。
    #[test]
    fn pack_effects_maps_to_slots() {
        use crate::render::node::{EffectBlock, UnderlaySlot};
        use crate::text::font_effect::FontEffect;
        let effects = vec![
            FontEffect::Shadow { ox: 3.0, oy: 0.0, blur: 0.0, color: [0., 0., 0., 1.] },
            FontEffect::Stroke { w: 2.0, color: [0., 0., 0., 1.] },
            FontEffect::Glow { w: 4.0, color: [0.37, 0.70, 0.77, 1.] },
            FontEffect::Blur { w: 2.0 },
        ];
        let eb = crate::render::pack_effects(&effects);
        // outline
        assert_eq!(eb.outline_width, 2.0);
        assert_eq!(eb.outline_color, [0., 0., 0., 1.]);
        // underlay[0] = 第一个 shadow
        assert_eq!(eb.underlay[0], UnderlaySlot { offset_x: 3.0, offset_y: 0.0, softness: 0.0, color: [0., 0., 0., 1.] });
        // underlay[1]/[2] 未填 = default（color.a=0 → shader 不启用）
        assert_eq!(eb.underlay[1].color[3], 0.0);
        assert_eq!(eb.underlay[2].color[3], 0.0);
        // glow / blur
        assert_eq!(eb.glow_power, 4.0);
        assert_eq!(eb.glow_color, [0.37, 0.70, 0.77, 1.]);
        assert_eq!(eb.blur_width, 2.0);
    }

    /// 多重 shadow：前 3 个填 underlay[0..3]，第 4 个丢弃（不 panic）。
    #[test]
    fn pack_effects_caps_shadows_at_three() {
        use crate::text::font_effect::FontEffect;
        let effects = vec![
            FontEffect::Shadow { ox: 2.0, oy: 2.0, blur: 0.0, color: [0., 0., 0., 1.] },
            FontEffect::Shadow { ox: 4.0, oy: 4.0, blur: 0.0, color: [0., 0., 0., 1.] },
            FontEffect::Shadow { ox: 6.0, oy: 6.0, blur: 0.0, color: [0., 0., 0., 1.] },
            FontEffect::Shadow { ox: 8.0, oy: 8.0, blur: 0.0, color: [0., 0., 0., 1.] }, // 超出，丢
        ];
        let eb = crate::render::pack_effects(&effects);
        assert_eq!(eb.underlay[0].offset_x, 2.0);
        assert_eq!(eb.underlay[1].offset_x, 4.0);
        assert_eq!(eb.underlay[2].offset_x, 6.0);
        // 没有 underlay[3]（只 3 槽）；第 4 个 shadow 被吞，不 panic 即可。
    }

    /// EffectBlock 默认 = 无 effect（全 0：outline_width=0 / color.a=0 / blur_width=0）。
    #[test]
    fn effect_block_default_is_no_effect() {
        use crate::render::node::EffectBlock;
        let eb = EffectBlock::default();
        assert_eq!(eb.outline_width, 0.0);
        assert_eq!(eb.outline_color[3], 0.0);
        assert_eq!(eb.underlay[0].color[3], 0.0);
        assert_eq!(eb.glow_color[3], 0.0);
        assert_eq!(eb.blur_width, 0.0);
    }

    /// to_bytes 往返：固定 128B，同 EffectBlock 产出同 bytes。
    #[test]
    fn effect_block_to_bytes_stable() {
        use crate::render::node::EffectBlock;
        let eb = EffectBlock::default();
        let bytes = eb.to_bytes();
        assert_eq!(bytes.len(), 128, "EffectBlock 序列化定长 128B");
        // 全 0 effect → 全 0 bytes
        assert!(bytes.iter().all(|&b| b == 0));
        // 同 effect 同 bytes（稳定性，供 dirty hash）
        let eb2 = EffectBlock::default();
        assert_eq!(eb.to_bytes(), eb2.to_bytes());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core pack_effects effect_block`
Expected: 编译失败（`EffectBlock` / `UnderlaySlot` / `pack_effects` 不存在）。

- [ ] **Step 3: 实现 `EffectBlock` + `UnderlaySlot`（node.rs）**

在 `loomgui_core/src/render/node.rs` 的 `RenderNode` struct 定义**之前**加：

```rust
/// 单个 underlay（shadow）槽：偏移采样 distance + softness（shadow blur 近似）+ 色。
/// color.a=0 → 该槽未启用（shader 据此跳过，effect 无 flags，参数隐含启用）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct UnderlaySlot {
    pub offset_x: f32,
    pub offset_y: f32,
    pub softness: f32,
    pub color: [f32; 4],
}

/// 文字效果参数块（per-text-node）。定长，序列化进 FFI SOA 的 effect_block 列（照
/// color_matrix 先例）。无 flags：effect 启用由参数隐含（outline_width>0 /
/// underlay.color.a>0 / glow_color.a>0 / blur_width>0）。Default 全 0 = 无 effect。
///
/// 槽位对标 TextMeshPro（_Outline*/_Underlay*/_Glow*）；多重 shadow 扩展为 underlay[3]
/// （TMP underlay 单槽）。blur 是 LoomGUI 私有近似（TMP 无整字高斯 blur）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct EffectBlock {
    pub outline_width: f32,
    pub outline_color: [f32; 4],
    pub underlay: [UnderlaySlot; 3],
    pub glow_power: f32,
    pub glow_color: [f32; 4],
    pub blur_width: f32,
}

impl EffectBlock {
    /// 序列化定长（32 × f32 = 128 字节，小端）。字段顺序固定，FFI blob 写出与 C# 解析、
    /// dirty hash 共用此方法（DRY）。字段顺序 = outline / underlay[3] / glow / blur。
    pub const SIZE: usize = 128;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        let mut o = 0usize;
        macro_rules! wf {
            ($v:expr) => {
                buf[o..o + 4].copy_from_slice(&($v).to_le_bytes());
                o += 4;
            };
        }
        wf!(self.outline_width);
        for &c in &self.outline_color {
            wf!(c);
        }
        for slot in &self.underlay {
            wf!(slot.offset_x);
            wf!(slot.offset_y);
            wf!(slot.softness);
            for &c in &slot.color {
                wf!(c);
            }
        }
        wf!(self.glow_power);
        for &c in &self.glow_color {
            wf!(c);
        }
        wf!(self.blur_width);
        debug_assert_eq!(o, Self::SIZE, "EffectBlock 字段顺序/数量与 SIZE 不符");
        buf
    }
}
```

- [ ] **Step 4: 实现 `pack_effects`（mod.rs）**

在 `loomgui_core/src/render/mod.rs`（`build_text_mesh` 函数附近，定义在模块级 `pub(crate)` 可见）加：

```rust
/// 把节点 text_effects 打包成定长 EffectBlock（供 build_text_mesh → RenderNode.effect）。
/// 映射：Shadow→underlay 槽（多重 ≤3，超 3 丢弃；shadow blur→softness、ox/oy→offset）、
/// Stroke→outline（CSS 单值，后到覆盖）、Glow→glow（w→power，起点值，验收精调）、Blur→blur。
/// 同类型多值：shadow 抢 underlay 空槽；stroke/glow/blur 后到覆盖先到。
pub(crate) fn pack_effects(effects: &[crate::text::font_effect::FontEffect]) -> EffectBlock {
    use crate::text::font_effect::FontEffect;
    let mut eb = EffectBlock::default();
    let mut underlay_idx = 0usize;
    for e in effects {
        match e {
            FontEffect::Shadow { ox, oy, blur, color } => {
                if underlay_idx < eb.underlay.len() {
                    eb.underlay[underlay_idx] = UnderlaySlot {
                        offset_x: *ox,
                        offset_y: *oy,
                        softness: *blur,
                        color: *color,
                    };
                    underlay_idx += 1;
                }
                // 超 3 个 shadow：静默丢弃（CSS text-shadow 多重尾部，FFI 邻近不 panic）。
            }
            FontEffect::Stroke { w, color } => {
                eb.outline_width = *w;
                eb.outline_color = *color;
            }
            FontEffect::Glow { w, color } => {
                eb.glow_power = *w;
                eb.glow_color = *color;
            }
            FontEffect::Blur { w } => {
                eb.blur_width = *w;
            }
        }
    }
    eb
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_core pack_effects effect_block`
Expected: 4 passed。

- [ ] **Step 6: clippy/fmt**

Run: `cargo clippy -p loomgui_core --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 无 warning、无 diff。修到干净。

- [ ] **Step 7: Commit**

```bash
git add loomgui_core/src/render/node.rs loomgui_core/src/render/mod.rs loomgui_core/src/render/tests.rs
git commit -m "feat(render): add EffectBlock struct + pack_effects (FontEffect -> slots)"
```

---

### Task 2: `RenderNode.effect` + `build_text_mesh` 启用 + 清旧 layer 骨架

把 EffectBlock 接入渲染树：`RenderNode` 加 effect 字段；`build_text_mesh` 启用 `_text_effects` 参数 + `TextMeshes.effect` carry；`emit_text_node` 把 effect 塞进 base/子页 RenderNode；删 Plan 1 残留的文字 back/front layer 产出 + stroke 机制（保留 div box-shadow 共用的 BOX_SHADOW_FLAG）。

**Files:**
- Modify: `loomgui_core/src/render/node.rs`（RenderNode 加 effect 字段）
- Modify: `loomgui_core/src/render/mod.rs`（TextMeshes.effect；build_text_mesh 启用 _text_effects；emit_text_node 接 effect + 清 layer + 改签名；删 stroke 机制）
- Modify: `loomgui_core/src/render/merge.rs`（去 TEXT_STROKE_FRONT_FLAG 引用）
- Test: `loomgui_core/src/render/tests.rs`

**Interfaces:**
- Consumes: Task 1 的 `EffectBlock` / `pack_effects`。
- Produces: `RenderNode.effect: EffectBlock`（供 Task 3 blob 序列化）；`TextMeshes.effect`（emit_text_node 读）；`build_text_mesh` 产 base 只（无 back/front layer mesh）。

- [ ] **Step 1: 写失败测试**

`loomgui_core/src/render/tests.rs` 加：

```rust
    /// build_text_mesh 产 base 字形 mesh + 把 text_effects 打包进 TextMeshes.effect。
    /// （构造参照现有 build_text_* 测试的 fixture 模式。）
    #[test]
    fn build_text_packs_effects_into_meshes() {
        use crate::text::font_effect::FontEffect;
        let (atlas, layout, rect) = build_text_fixture(48.0); // 现有 helper（见 build_text_snaps_quad_to_integer_pixel）
        let mut atlas = atlas;
        let effects = vec![FontEffect::Shadow { ox: 3.0, oy: 3.0, blur: 0.0, color: [0., 0., 0., 1.] }];
        let m = build_text_mesh(&layout, &mut atlas, &fonts(), &rect, &effects, None, false);
        // effect 进了 TextMeshes.effect
        assert_eq!(m.effect.underlay[0].offset_x, 3.0);
        assert_eq!(m.effect.underlay[0].offset_y, 3.0);
        // base 非空（字形 quad）
        assert!(!m.base.is_empty(), "base 字形 mesh 仍产出");
    }

    /// 清旧 layer：build_text_mesh 不再产 back/front_layers（即使有 shadow/stroke effect）。
    #[test]
    fn build_text_no_longer_emits_effect_layers() {
        use crate::text::font_effect::FontEffect;
        let (atlas, layout, rect) = build_text_fixture(48.0);
        let mut atlas = atlas;
        // 声明 shadow + stroke（旧路径会产 back + front layer mesh）
        let effects = vec![
            FontEffect::Shadow { ox: 2.0, oy: 2.0, blur: 0.0, color: [0., 0., 0., 1.] },
            FontEffect::Stroke { w: 2.0, color: [0., 0., 0., 1.] },
        ];
        let m = build_text_mesh(&layout, &mut atlas, &fonts(), &rect, &effects, None, false);
        assert!(m.back_layers.is_empty(), "back_layers 已废（effect 改 shader）");
        assert!(m.front_layers.is_empty(), "front_layers 已废（effect 改 shader）");
    }
```

（`build_text_fixture` / `fonts()` 是 `render/tests.rs` 现有 helper——参照 `build_text_snaps_quad_to_integer_pixel`（Plan 1 加）的构造模式。若该测试未抽 helper，本 task 顺手抽一个 `build_text_fixture(font_size) -> (GlyphAtlas, TextLayout, Rect)` 供两条测试复用。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core build_text_packs_effects build_text_no_longer`
Expected: 编译失败（`TextMeshes.effect` 字段不存在；`build_text_mesh` 的 `_text_effects` 未消费 → `back_layers`/`front_layers` 仍被旧逻辑填，断言失败）。

- [ ] **Step 3: `RenderNode` 加 effect 字段（node.rs）**

`loomgui_core/src/render/node.rs` 的 `RenderNode` struct，在 `reuse_key` 后、`payload` 前加：

```rust
    pub reuse_key: u32,
    /// 文字效果参数（SDF effect：outline/underlay×3/glow/blur）。非文字节点 = default（全 0，
    /// shader 纯 face）。进 FFI effect_block 列 + header_hash（effect 变 = Header 级，只更 MPB）。
    pub effect: EffectBlock,
    pub payload: NodePayload,
```

同步：所有 `RenderNode { ... }` 构造点（node.rs 测试 + dirty.rs 测试 `mesh_rn` + render/mod.rs 各 push 点 + blob/tests.rs）补 `effect: EffectBlock::default()`（Task 2 Step 6 统一处理被波及构造点）。

- [ ] **Step 4: `TextMeshes.effect` + `build_text_mesh` 启用（mod.rs）**

`loomgui_core/src/render/mod.rs`：

`TextMeshes` struct（原 line 1173）加字段（同步删 `back_layers` / `front_layers` 字段，见 Step 5）：

```rust
struct TextMeshes {
    base: Vec<(u32, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>)>,
    /// 节点 text_effects 打包成的 effect 块（build_text_mesh 期算，emit 期塞进 base/子页
    /// RenderNode.effect）。同一文字节点所有页共享同一 effect 配置。
    effect: EffectBlock,
}
```

`build_text_mesh` 签名（原 line 1213）：`_text_effects` 去前缀下划线 → `text_effects`，函数体开头加打包：

```rust
fn build_text_mesh(
    layout: &crate::text::layout::TextLayout,
    atlas: &mut GlyphAtlas,
    fonts: &crate::text::layout::FontTable,
    rect: &crate::scene::node::Rect,
    text_effects: &[crate::text::font_effect::FontEffect],
    background_gradient: Option<crate::style::resolved::Gradient2>,
    background_clip_text: bool,
) -> TextMeshes {
    use std::collections::BTreeMap;
    let effect = pack_effects(text_effects); // effect 一次打包，base/子页共享
    let pad = crate::text::atlas::GLYPH_PAD as f32;
    // ... base_pages 构建逻辑不变 ...
```

`build_text_mesh` 末尾返回值（原返回 `TextMeshes { base, back_layers, front_layers }`）改为 `TextMeshes { base, effect }`（删 back/front_layers，见 Step 5）。

- [ ] **Step 5: 删 back/front layer 产出（mod.rs）**

`loomgui_core/src/render/mod.rs`：

1. `TextMeshes` struct 删 `back_layers` / `front_layers` 字段（Step 4 已加 `effect`，现删旧两字段）。
2. 删 `build_text_mesh` 内构建 `shadow_pages` / `glow_pages` / `stroke_pages` / `blur_pages` 的所有代码段（Plan 1 Task 3 后这些应已删；若残留，本 step 确认删净——`build_text_mesh` 只产 `base_pages`）。
3. 删 `build_text_mesh` 末尾 `TextMeshes { base, back_layers: ..., front_layers: ... }` 中的 back/front 字段构造，改为 `TextMeshes { base, effect }`。

- [ ] **Step 6: `emit_text_node` 接 effect + 删 layer push + 改签名（mod.rs）**

`emit_text_node`（原 line 1528+）：

1. 签名删 `shadow_pairs: &mut Vec<(u32, u32)>` 和 `stroke_pairs: &mut Vec<(u32, u32)>` 参数（文字不再产 back/front layer，不需这两收集器）。新签名末尾只剩必要参数（`nodes` / `id_to_pos` / `meshes` / `n` / `node_id` / `text_primary_id` / `parent_id` / `alpha` / `color_tint` / `wm` / `register_id_map`）。
2. 解构 `TextMeshes`（原 line 1543）：`let TextMeshes { base, effect } = meshes;`（删 back/front_layers 解构）。
3. 删 back layer push 循环（原 line 1548-1583，`for (si, ...) in back_layers.iter().enumerate()` 整块）。
4. 删 front layer push 循环（原 line 1675-1705+，`for (si, ...) in front_layers.iter().enumerate()` 整块）。
5. base 首页 RenderNode（原 line 1621）+ 子页 RenderNode（原 line 1648）+ 空文本占位 RenderNode（原 line 1589）的构造，每个加 `effect` 字段：

```rust
        nodes.push(RenderNode {
            node_id: text_primary_id, // 或 sub_id / 占位 id
            // ... 现有字段 ...
            reuse_key: n.reuse_key, // 或 0（子页）
            effect, // ← 新增：同一文字节点所有页共享 effect
            payload: NodePayload::Mesh { /* ... */ },
        });
```

（占位空文本节点也带 `effect`——空文本无 quad，effect 不生效，但字段必须填以满足 struct。）

- [ ] **Step 7: 删 stroke 机制 + caller 适配（mod.rs）**

`loomgui_core/src/render/mod.rs`：

1. 删常量 `TEXT_STROKE_FRONT_FLAG`（原 line 37）。
2. 删函数 `propagate_text_stroke_sort_keys`（原 line 959）。
3. `emit` 主循环（原 line 271-273）删 `let mut stroke_pairs: Vec<(u32, u32)> = Vec::new();` 声明。
4. `emit_text_node` 的两处 caller（原 line 558-580 Text arm、646-708 RichText arm）：删调用里传 `&mut shadow_pairs` / `&mut stroke_pairs` 的两个参数。
5. `emit` 末尾（原 line 815）删 `propagate_text_stroke_sort_keys(&mut nodes, &stroke_pairs);` 调用。
6. 保留 `shadow_pairs` 声明 + `propagate_box_shadow_sort_keys`（div box-shadow 共用，原 line 814）。

- [ ] **Step 8: `merge.rs` 去 `TEXT_STROKE_FRONT_FLAG`**

`loomgui_core/src/render/merge.rs:26`：

```rust
// 原：if rn.node_id & (crate::render::BOX_SHADOW_FLAG | crate::render::TEXT_STROKE_FRONT_FLAG) != 0 {
// 改：只留 BOX_SHADOW_FLAG（div box-shadow 合成节点；文字 stroke front 已废）。
    if rn.node_id & crate::render::BOX_SHADOW_FLAG != 0 {
```

- [ ] **Step 9: 修被波及测试 + RenderNode 构造点**

全局 `RenderNode { ... }` 构造点补 `effect: EffectBlock::default()`：
- `render/node.rs` 测试模块的 RenderNode 构造
- `render/dirty.rs` 测试 `mesh_rn`（原 line 94-117）
- `render/mod.rs` 所有 `nodes.push(RenderNode { ... })`（box-shadow :775 / image / mesh 各 arm——非文字节点 effect=default）
- `render/tests.rs` 各 RenderNode 构造
- `loomgui_ffi_c/src/blob/tests.rs` 各 RenderNode 构造

`render/tests.rs` 里旧 back/front_layers / stroke_pairs / `propagate_text_stroke_sort_keys` / `TEXT_STROKE_FRONT_FLAG` 相关测试（原 line 4059-4168 `propagate_text_stroke_sort_keys` 回归测试、原 line 2590-2601 `BOX_SHADOW_FLAG` 边界测试保留但确认仍过）：
- 删 `propagate_text_stroke_sort_keys` 相关测试（机制已废）。
- `synth_text_node_id_roundtrip`（原 line 2582）保留（synth_text_node_id 仍用于跨页子页 + 行内图），但其中 `page=16 触发 BOX_SHADOW_FLAG` 注释保留（BOX_SHADOW_FLAG 还在）；去掉对 TEXT_STROKE_FRONT_FLAG 的引用（若有）。

- [ ] **Step 10: 跑全核心测试**

Run: `cargo test -p loomgui_core`
Expected: 全绿。`cargo test -p loomgui_core --test fence_contract` 也绿（不改围栏）。snapshot 可能因 back/front_layers 删除而 FAIL——预期，Task 6 用 `INSTA_UPDATE=always` 统一更新（本 task 先不更 snapshot，确认失败属预期即可）。

- [ ] **Step 11: clippy/fmt**

Run: `cargo clippy -p loomgui_core --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 干净。注意 `EffectBlock::default()` 在大量构造点是重复——若 clippy 报 `field_reassign_with_default` 等，按 CLAUDE.md 在 crate root `#![allow]` 加可辩护注释（构造点 struct 字面量补默认 effect 是合法模式）。

- [ ] **Step 12: Commit**

```bash
git add loomgui_core/src/render/node.rs loomgui_core/src/render/mod.rs loomgui_core/src/render/merge.rs loomgui_core/src/render/dirty.rs loomgui_core/src/render/tests.rs loomgui_ffi_c/src/blob/tests.rs
git commit -m "refactor(render): wire EffectBlock into RenderNode; drop text effect layers"
```

---

### Task 3: FFI `effect_block` 列 + `header_hash` 采样 + 断言

effect 参数进 FFI：blob.rs SOA 加 `effect_block` 定长列（VERSION v10→v11，列数 +1）；dirty.rs `header_hash` 加 effect 采样（effect 变 = Header 级）；fence_contract 列数/VERSION 断言更新。

**Files:**
- Modify: `loomgui_ffi_c/src/blob.rs`（columns + VERSION + 写出循环）
- Modify: `loomgui_core/src/render/dirty.rs`（header_hash + effect）
- Modify: `loomgui_core/tests/fence_contract.rs`（断言更新）
- Test: `loomgui_ffi_c/src/blob/tests.rs` + `loomgui_core/src/render/dirty.rs` 测试

**Interfaces:**
- Consumes: Task 1 的 `EffectBlock::to_bytes()`（128B）、Task 2 的 `RenderNode.effect`。
- Produces: blob `effect_block` 列（第 21 列，index 20）；`header_hash` 含 effect。

- [ ] **Step 1: 写失败测试**

`loomgui_ffi_c/src/blob/tests.rs` 加：

```rust
    /// effect_block 列：非文字节点写全 0，文字节点写 EffectBlock bytes。
    #[test]
    fn blob_writes_effect_block_column() {
        use loomgui_core::render::node::{EffectBlock, UnderlaySlot, BlendMode, ChangeLevel, MaskContext, NodePayload, RenderNode};
        use loomgui_core::transform::IDENTITY;
        let mut effect = EffectBlock::default();
        effect.outline_width = 2.0;
        let nodes = vec![
            RenderNode { node_id: 1, parent_id: None, visible: true, alpha: 1.0,
                color_tint: [1.0;4], world_matrix: IDENTITY, blend: BlendMode::Normal,
                mask_context: MaskContext(0), sort_key: 0, change_level: ChangeLevel::Full,
                reuse_key: 0, effect: EffectBlock::default(),
                payload: NodePayload::Mesh { verts: vec![], uvs: vec![], colors: vec![],
                    indices: vec![], image_path: None, program: 0, color_matrix: [0.0;20] } },
            RenderNode { node_id: 2, parent_id: None, visible: true, alpha: 1.0,
                color_tint: [1.0;4], world_matrix: IDENTITY, blend: BlendMode::Normal,
                mask_context: MaskContext(0), sort_key: 0, change_level: ChangeLevel::Full,
                reuse_key: 0, effect,
                payload: NodePayload::Mesh { verts: vec![], uvs: vec![], colors: vec![],
                    indices: vec![], image_path: None, program: 1, color_matrix: [0.0;20] } },
        ];
        let frame = loomgui_core::render::FrameData { nodes, clips: vec![] };
        let blob = super::build_blob(&frame);
        // effect_block 列 offset = ColOff(20)；节点 0（全 0）+ 节点 1（outline_width=2.0）
        // 第 21 列（index 20）——读节点 1 的 effect_block 首个 f32 = outline_width = 2.0。
        // （ColOff 读取逻辑同 ColorMatrix 测试；具体偏移从 blob header 列 offset 表读。）
        // 见 blob/tests.rs 现有读列模式（如读 color_matrix 的测试）。
        // 断言：节点 1 effect_block[0..4] = 2.0f32 LE；节点 0 effect_block 全 0。
        // （精确读法照 blob/tests.rs 现有 helper。）
    }
```

`loomgui_core/src/render/dirty.rs` 测试模块加：

```rust
    #[test]
    fn header_hash_includes_effect() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.effect.outline_width = 2.0; // effect 变
        assert_ne!(header_hash(&a), header_hash(&b), "effect 变 → header_hash 变（HEADER 级）");
    }

    #[test]
    fn payload_hash_ignores_effect() {
        let a = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        let mut b = mesh_rn(Some("a.png"), 1.0, [1.0; 4]);
        b.effect.outline_width = 2.0;
        assert_eq!(payload_hash(&a), payload_hash(&b), "effect 不进 payload_hash（非几何）");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core header_hash_includes_effect payload_hash_ignores && cargo test -p loomgui_ffi_c blob_writes_effect`
Expected: dirty 测试 FAIL（effect 变 header_hash 不变）；blob 测试 FAIL（无 effect_block 列）。

- [ ] **Step 3: blob.rs 加 effect_block 列 + VERSION**

`loomgui_ffi_c/src/blob.rs`：

1. `VERSION`（原 line 11）：`const VERSION: u32 = 10;` → `const VERSION: u32 = 11;`，注释更新：`// v11：加 effect_block 列（SDF effect 参数，照 color_matrix 先例），列数 20→21。`
2. `columns` 数组（原 line 21-42）末尾（`reuse_key` 后）加：

```rust
        ("reuse_key", 4),     // v9：渲染复用键
        ("effect_block", 128), // v11：SDF effect 参数块（EffectBlock::SIZE，照 color_matrix 先例）
```

3. `col_bufs` vec（原 line 164-185）末尾加：

```rust
        ("reuse_key", &col_reuse_key),
        ("effect_block", &col_effect_block), // v11：effect 参数列
```

4. 列 buffer 声明（原 line 60-79）加：`let mut col_effect_block = Vec::<u8>::new();`
5. 写出循环（原 line 81-162，`for rn in nodes`）末尾加：

```rust
        col_effect_block.extend_from_slice(&rn.effect.to_bytes());
```

（`EffectBlock::to_bytes()` 产 128B，非文字节点全 0、文字节点有值。照 color_matrix 写出模式。）

- [ ] **Step 4: dirty.rs `header_hash` 加 effect**

`loomgui_core/src/render/dirty.rs` `header_hash`（原 line 65-84），在 `rn.parent_id.hash(...)` 后、`h.finish()` 前加：

```rust
    rn.effect.to_bytes().hash(&mut h); // effect 参数（SDF effect）：变 → Header 级只更 MPB
```

（`to_bytes` 返回 `[u8; 128]`，`[u8; N]` impl Hash，直接 hash。）

- [ ] **Step 5: fence_contract 断言更新**

`loomgui_core/tests/fence_contract.rs`：搜索 blob 列数 / VERSION 断言（如 `assert_eq!(列数, 20)` 或 `VERSION == 10`），改为 21 / 11。若无现成断言，加一条：

```rust
    /// blob SOA 列数 = 21（v11：加 effect_block 列）。
    #[test]
    fn blob_column_count() {
        // 读 blob header 列 offset 表数量，断言 = 21。
        // （照 fence_contract 现有 blob 结构断言模式。）
        assert_eq!(/* 列数读取 */, 21);
    }
```

- [ ] **Step 6: 跑测试确认通过**

Run:
```
cargo test -p loomgui_core header_hash_includes_effect payload_hash_ignores
cargo test -p loomgui_ffi_c blob_writes_effect
cargo test -p loomgui_core --test fence_contract
```
Expected: 全绿。

- [ ] **Step 7: clippy/fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 干净。

- [ ] **Step 8: Commit**

```bash
git add loomgui_ffi_c/src/blob.rs loomgui_ffi_c/src/blob/tests.rs loomgui_core/src/render/dirty.rs loomgui_core/tests/fence_contract.rs
git commit -m "feat(ffi): add effect_block SOA column (v11) + header_hash samples effect"
```

---

### Task 4: shader ALPHA_MASK 单 pass 合成

shader fragment 从只算 face 改为单 pass 合成 face + outline + underlay×3 + glow（blur=face softness）。新增 per-renderer uniform（Properties + CBUFFER）。无 Rust 单测——靠编译通过 + Task 6 家里 PlayMode 视觉验收（shader 不可单测）。

**Files:**
- Modify: `loomgui_unity_package/Shaders/LoomGUI-Unlit.shader`

**Interfaces:**
- Consumes: R8 atlas（`.r` = encoded distance）、新 effect uniforms（MirrorPool Task 5 设 MPB）。
- Produces: ALPHA_MASK fragment 单 pass 合成多 effect。

- [ ] **Step 1: 加 Properties + CBUFFER（effect uniforms）**

`LoomGUI-Unlit.shader` `Properties{}` 块（`_GradientScale` 后）加：

```hlsl
        // SDF 文字效果（per-renderer MPB，program=1 ALPHA_MASK 用；参数=0 = 该 effect 不启用）。
        _OutlineWidth("Outline Width", Float) = 0
        _OutlineColor("Outline Color", Color) = (0,0,0,0)
        _UnderlayOffset0("Underlay0 Offset", Vector) = (0,0,0,0)   // xy=像素偏移
        _UnderlaySoftness0("Underlay0 Softness", Float) = 0
        _UnderlayColor0("Underlay0 Color", Color) = (0,0,0,0)
        _UnderlayOffset1("Underlay1 Offset", Vector) = (0,0,0,0)
        _UnderlaySoftness1("Underlay1 Softness", Float) = 0
        _UnderlayColor1("Underlay1 Color", Color) = (0,0,0,0)
        _UnderlayOffset2("Underlay2 Offset", Vector) = (0,0,0,0)
        _UnderlaySoftness2("Underlay2 Softness", Float) = 0
        _UnderlayColor2("Underlay2 Color", Color) = (0,0,0,0)
        _GlowPower("Glow Power", Float) = 0
        _GlowColor("Glow Color", Color) = (0,0,0,0)
        _BlurWidth("Blur Width", Float) = 0
```

`CBUFFER_START(UnityPerMaterial)`（`_GradientScale` 后）加对应声明：

```hlsl
                float _OutlineWidth;
                half4 _OutlineColor;
                float2 _UnderlayOffset0; float _UnderlaySoftness0; half4 _UnderlayColor0;
                float2 _UnderlayOffset1; float _UnderlaySoftness1; half4 _UnderlayColor1;
                float2 _UnderlayOffset2; float _UnderlaySoftness2; half4 _UnderlayColor2;
                float _GlowPower;
                half4 _GlowColor;
                float _BlurWidth;
```

- [ ] **Step 2: 改 ALPHA_MASK fragment（单 pass 合成）**

把 ALPHA_MASK 分支（原 line 109-123）从纯 face 改为单 pass 合成：

```hlsl
                #if defined(ALPHA_MASK)
                // SDF：tex.r 是 encoded distance（中心 0.5、inside>0.5）。
                float2 uvDx = ddx(i.uv);
                float2 uvDy = ddy(i.uv);
                float pxSize = rsqrt(abs(uvDx.x * uvDy.y - uvDx.y * uvDy.x));
                float scale = pxSize * (1.3333 * _GradientScale) / _MainTex_TexelSize.z;
                float d = tex.r;
                float threshold = 0.5 - _FaceDilate * 0.5;
                // blur 近似：软化 face 过渡带（SDF 无整字高斯 blur，偏硬，验收接受）。
                if (_BlurWidth > 0.001) scale /= 1.0 + _BlurWidth * scale;
                float face = saturate((d - threshold) * scale + 0.5);

                // 单 pass 合成（对标 TMP_SDF_SSD.cginc）：underlay 画 face 下 → face → outline 覆盖边缘 → glow 晕 face 外。
                float3 rgb = vcol.rgb;
                float a = face * vcol.a;
                // underlay×3（偏移 uv 重采 d，over 合成画 face 下）
                #define UNDERLAY_PASS(idx) \
                    if (_UnderlayColor##idx.a > 0.001) { \
                        float du = tex2D(_MainTex, i.uv + _UnderlayOffset##idx / _MainTex_TexelSize.xy).r; \
                        float ls = scale / (1.0 + _UnderlaySoftness##idx * scale); \
                        float um = saturate((du - threshold) * ls + 0.5); \
                        float ua = _UnderlayColor##idx.a * um; \
                        rgb = lerp(rgb, _UnderlayColor##idx.rgb, ua * (1.0 - a)); \
                        a += ua * (1.0 - a); \
                    }
                UNDERLAY_PASS(0)
                UNDERLAY_PASS(1)
                UNDERLAY_PASS(2)
                #undef UNDERLAY_PASS
                // outline（face ± width 环形）
                if (_OutlineWidth > 0.001) {
                    float outer = saturate((d - threshold + _OutlineWidth) * scale + 0.5);
                    float inner = saturate((d - threshold - _OutlineWidth) * scale + 0.5);
                    rgb = lerp(rgb, _OutlineColor.rgb, saturate((outer - inner) * _OutlineColor.a));
                }
                // glow（face 外 distance power 衰减）
                if (_GlowColor.a > 0.001) {
                    float gm = 1.0 - saturate((d - threshold) * scale + 0.5);
                    float ga = pow(gm, _GlowPower) * _GlowColor.a;
                    rgb = lerp(rgb, _GlowColor.rgb, ga * (1.0 - a));
                    a += ga * (1.0 - a);
                }
                half4 col = half4(rgb, a);
```

（`_UnderlayOffset##idx / _MainTex_TexelSize.xy`：像素偏移转 uv 偏移。`_MainTex_TexelSize.xy` = 1/atlasSize。）

- [ ] **Step 3: 本地验证**

Run: `cargo test -p loomgui_core`
Expected: 全绿（shader 不影响 Rust 测试）。shader 语法靠 Unity 控制台编译（家里机拉代码后验，Task 6）。

本 task 后视觉预期：effect uniform 全默认 0（C# Task 5 才设 MPB）→ 所有 `if (param > 0.001)` 不进 → 纯 face，无 effect（不回归 Plan 1）。

- [ ] **Step 4: Commit**

```bash
git add loomgui_unity_package/Shaders/LoomGUI-Unlit.shader
git commit -m "feat(shader): single-pass SDF text effects (outline/underlay/glow/blur)"
```

---

### Task 5: `FrameBlob.cs` 访问器 + `MirrorPool` MPB

C# 后端读 effect_block 列 → MirrorPool 给 program==1 节点设 effect uniform MPB。无 Rust 单测——靠 Task 6 家里 PlayMode 视觉验收。

**Files:**
- Modify: `loomgui_unity_package/Runtime/FrameBlob.cs`
- Modify: `loomgui_unity_package/Runtime/MirrorPool.cs`

**Interfaces:**
- Consumes: blob effect_block 列（Task 3，第 21 列 index 20，128B/节点）。
- Produces: per-renderer MPB 设 effect uniforms（shader Task 4 消费）。

- [ ] **Step 1: `FrameBlob.cs` 加 `EffectBlock(i)` 访问器**

`loomgui_unity_package/Runtime/FrameBlob.cs`（`ColorMatrix` 访问器，原 line 84-91，旁边）加：

```csharp
        /// v11：effect_block 列（第 21 列，index 20）。32 × f32 = 128B/节点。
        /// flatten 顺序：outline_width(1) outline_color(4) underlay[3](3×7=21)
        ///               glow_power(1) glow_color(4) blur_width(1)。
        /// 非 text 节点全 0 = 无 effect。MirrorPool program==1 时读此 → MPB。
        public float[] EffectBlock(int i) {
            int off = ColOff(20) + i * 128;
            float[] eb = new float[32];
            for (int j = 0; j < 32; j++) {
                eb[j] = BitConverter.ToSingle(_buf, off + j * 4);
            }
            return eb;
        }
```

（`ColOff(20)`：第 21 列 offset。`ColOff` 现有 helper 读列 offset 表，加列后自动含 index 20。确认 `ColOff` 支持 index 20——若它硬编码列数，改用列 offset 表读取。）

- [ ] **Step 2: `MirrorPool.cs` 设 effect MPB**

`loomgui_unity_package/Runtime/MirrorPool.cs`（`_CF` MPB 段，原 line 203-212，`ro.Mpb.SetFloat("_Alpha", alpha)` 前）加：

```csharp
            // SDF 文字效果（program=1 ALPHA_MASK）：读 effect_block 列 → MPB。
            // 非 text 节点不设（material 默认全 0 = 纯 face）。
            if (blob.Program(i) == 1) {
                float[] eb = blob.EffectBlock(i);
                ro.Mpb.SetFloat("_OutlineWidth", eb[0]);
                ro.Mpb.SetVector("_OutlineColor", new Vector4(eb[1], eb[2], eb[3], eb[4]));
                for (int s = 0; s < 3; s++) {
                    int b = 5 + s * 7; // underlay 槽起点：[5],[12],[19]
                    ro.Mpb.SetVector("_UnderlayOffset" + s, new Vector4(eb[b], eb[b + 1], 0, 0));
                    ro.Mpb.SetFloat("_UnderlaySoftness" + s, eb[b + 2]);
                    ro.Mpb.SetVector("_UnderlayColor" + s, new Vector4(eb[b + 3], eb[b + 4], eb[b + 5], eb[b + 6]));
                }
                ro.Mpb.SetFloat("_GlowPower", eb[26]);
                ro.Mpb.SetVector("_GlowColor", new Vector4(eb[27], eb[28], eb[29], eb[30]));
                ro.Mpb.SetFloat("_BlurWidth", eb[31]);
            }
```

（flatten offset 验证：outline_width=eb[0]，outline_color=eb[1..4]，underlay[0]=eb[5..11]（off_x/off_y/softness/color[4]），underlay[1]=eb[12..18]，underlay[2]=eb[19..25]，glow_power=eb[26]，glow_color=eb[27..30]，blur_width=eb[31]。共 32。✓）

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity_package/Runtime/FrameBlob.cs loomgui_unity_package/Runtime/MirrorPool.cs
git commit -m "feat(unity): read effect_block column into per-renderer MPB (text effects)"
```

---

### Task 6: 重编 `.dll` + snapshot 更新 + 家里验收

Plan 2 收尾：重编 FFI .dll + 拷贝（Task 1-3 累积 Rust 改动）+ 更新 snapshot + fmt/clippy 全量门 + 家里 PlayMode 验收 C1-C4。

**Files:**
- Modify: `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`（重编拷贝）
- Modify: `loomgui_core/tests/snapshots/*`（INSTA_UPDATE）

- [ ] **Step 1: 重编 .dll + 拷贝**

Run:
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
（**Unity 必须关着**，否则 .dll 被锁。）验证 `md5sum target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` 两文件 md5 一致（防 stale .dll）。

- [ ] **Step 2: 更新 snapshot**

Run: `INSTA_UPDATE=always cargo test -p loomgui_core --test snapshot`
review 新 snapshot：变化应来自 back/front_layers 删除（少 mesh layer 节点）+ effect_block 列新增（blob 结构）。无意义的 diff 确认是预期。

再 `cargo test -p loomgui_core --test snapshot` 确认全绿。

- [ ] **Step 3: fmt/clippy 全量门**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -p loomgui_core
cargo test -p loomgui_core --test fence_contract
cargo test -p loomgui_ffi_c
```
Expected: 全绿。

- [ ] **Step 4: Commit .dll + snapshot**

```bash
git add loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll loomgui_core/tests/snapshots/
git commit -m "feat(text): rebuild dll + update snapshots for SDF text effects"
```

- [ ] **Step 5: 家里 PlayMode 验收（两机工作流）**

家里机拉代码 → Unity PlayMode 加载 showcase page_text：
- ✅ **C1 text-shadow**：硬边 `3px 3px #000`、柔光 `0 0 12px #5fb2c4`（blur→softness 精调）、多重 `2/4/6px` ×3（underlay×3）—— 对齐 HTML 预览。
- ✅ **C2 -webkit-text-stroke**：`2px #000`、`2px #5fb2c4`（outline 环形）。
- ✅ **C3 font-effect:glow**：`glow(4 #5fb2c4)`、`glow(6 #ff6b6b)`（face 外 power 衰减；w→power 映射精调）。
- ⚠️ **C4 font-effect:blur**：`blur(2)`、`blur(4)`（face softness 近似，偏硬接受；softness 系数精调）。
- ✅ **不回归**：page_image 3.1-3.3 字细（Plan 1 成果）、渐变字、装饰线、**div box-shadow**（v1.8，BOX_SHADOW_FLAG 保留）、无 effect 纯文字（effect_block 全 0 = 纯 face）。
- 调参：shadow blur→softness 系数、glow w→power 映射、blur softness 在 shader（Task 4）调，shader 改即时（不重编 Rust）。若 effect 参数错位/丢字段，先 `dump_showcase_text` 量化 core effect_block，确认 core 对再查 Unity MPB（双机调试方法论）。

验收通过 → Plan 2 完成，C1-C4 恢复。

---

## Self-Review（写计划后自查）

**1. Spec 覆盖**：
- §2.1 单 quad 多 pass → Task 4 shader fragment（单 pass 合成）+ Task 2 删 layer。✅
- §2.2 effect_block SOA 列 → Task 1 EffectBlock + Task 3 blob 列。✅
- §2.3 去 flags 参数隐含 → Task 4 shader `if (param > 0.001)` + Task 1 EffectBlock 无 flags。✅
- §2.4 `_FaceDilate` 保持 material 默认 → 不进 effect_block（Task 1 字段无 face_dilate）；shader Task 4 保留现有 `_FaceDilate` material 默认。✅
- §2.5 BOX_SHADOW_FLAG 共用保留 → Task 2 Step 7 保留 shadow_pairs + propagate_box_shadow_sort_keys，只删 stroke 机制；Task 2 Step 8 merge.rs 只去 TEXT_STROKE_FRONT_FLAG。✅
- §2.6 effect 进 header_hash → Task 3 Step 4 dirty.rs header_hash + effect。✅
- §3.1 数据流 → Task 1-5 端到端。✅
- §3.2 effect_block 字段布局 → Task 1 EffectBlock（outline/underlay×3/glow/blur）。✅
- §3.3 core 改动 + 清旧 layer → Task 2。✅
- §3.4 shader fragment → Task 4。✅
- §3.5 FFI 改动 → Task 3（blob/dirty）+ Task 5（FrameBlob/MirrorPool）。✅
- §4 测试 + 验收 → 各 task 测试 + Task 6 PlayMode。✅
- §5 风险（shader 精调 / 多重 shadow MPB / 编码一致性 / 清 layer 边界 / 两机）→ Task 6 Step 5 验收调参 + Global Constraints。✅

**2. Placeholder 扫描**：
- 无 TBD/TODO。
- shader 数学（glow w→power、blur softness、shadow blur→softness）是起点值——spec §5 已声明需验收精调，非 placeholder（Task 6 Step 5 明确调参项）。
- Task 2 Step 9 "全局 RenderNode 构造点补 effect:default"——清单完整（node.rs/dirty.rs/mod.rs/tests.rs/blob/tests.rs）。✅
- Task 3 Step 5 fence_contract 断言——给了"若无现成断言则加"的兜底，非空。✅

**3. 类型一致**：
- `EffectBlock` 字段（Task 1）：outline_width/outline_color/underlay[3]/glow_power/glow_color/blur_width。
- `to_bytes()`（Task 1）= 128B，flatten 顺序 outline/underlay[3]/gllow/blur。
- blob.rs（Task 3）用 `rn.effect.to_bytes()` → 128B 列。✅
- dirty.rs（Task 3）用 `rn.effect.to_bytes().hash()`。✅
- FrameBlob.cs（Task 5）`ColOff(20) + i*128` 读 32 f32——flatten offset 与 to_bytes 一致（eb[0]=outline_width ... eb[31]=blur_width）。✅
- MirrorPool.cs（Task 5）flatten offset：underlay 槽起点 [5]/[12]/[19]（= 5 + s*7），glow_power=eb[26]，glow_color=eb[27..30]，blur=eb[31]。验证：5 + 3*7 = 26（underlay 占 eb[5..26]），glow eb[26..31]，blur eb[31]。共 32。✅
- shader Task 4 uniform 名（`_UnderlayOffset0/1/2` 等）= MirrorPool Task 5 SetVector 名。✅
- `pack_effects`（Task 1）→ `build_text_mesh`（Task 2 Step 4）调用 → `TextMeshes.effect`（Task 2）→ emit RenderNode.effect（Task 2 Step 6）。✅

**4. Task 边界**：每 task 自带测试周期（Task 1/2/3 core 单测；Task 4/5 编译 + Task 6 PlayMode）。Task 2 是最大（接入 + 清旧 layer），但内聚（一次 render 层重构：build_text_mesh 产 base+effect 不产 layer，emit 带 effect 不 push layer），拆开会割裂。可接受。
