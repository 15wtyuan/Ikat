# border 单边 longhand + border_width 减法重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `border-top/right/bottom/left` 单边属性正常渲染（修 showcase 标题分割线不显示），并删掉阻碍它的历史遗留字段 `ResolvedStyle.border_width`。

**Architecture:** 减法重构——删 `border_width` 单值字段，render 直读 `ts.border: Rect<LengthPercentage>` 四边 + 既有 `resolve_lp` helper（Text arm 先例）。`border_ring` 环拓扑不变（8 顶点），仅改 3 处：内轮廓公式四边独立、per-axis 比例钳制、per-edge 跳零宽边。mapping 加 4 个单边 longhand arm。颜色四边共享单色 `border_color`。

**Tech Stack:** Rust edition 2021 / taffy 0.5（`Rect<LengthPercentage>`、`Layout.border`）/ csbindgen FFI / bincode pkg。

**Spec:** `docs/superpowers/specs/2026-07-11-border-longhand-design.md`

## Global Constraints

- **Rust 依赖钉版本**：taffy 0.5、cssparser 0.34、scraper 0.19、slotmap 1.1、csbindgen 1。遇 API 不符查 `~/.cargo/registry/src/<crate>-<ver>/src/`，勿改版本。
- **CI 门禁**：每个 task 结束前跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`（全过才 commit，否则 CI 红）。
- **围栏门**：改 mapping/apply_decl 后跑 `cargo test -p loomgui_core fence_contract`。
- **parse-time 改动必重打 pkg**：mapping 改了 → `cargo run -p loomgui_pkg -- build showcase_project` 重打 pkg.bin（Task 5）。
- **Rust→Unity .dll 闭环**：核心 Rust 改动后 `cargo build -p loomgui_ffi_c --release` + 拷 dll（Task 5）。**拷贝时 Unity 必须关着**（锁 .dll）。
- **双机工作流**：公司机（本机 Windows）做核心 Rust + 重编 dll + 重打 pkg + commit；家里机 pull 后 PlayMode 验收。家里机也有 cargo 打包器。
- **代码注释上线品质**：自包含、说 WHY、**不引用坑号或内部暗语**（"坑 74"等只进 `docs/pitfalls.md`）。
- **commit message 英文**，结尾加 `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`。
- **坐标**：核心左上原点 y 向下；`border_ring` 产 design 坐标，y-flip 由后端根 Stage 处理。

---

## File Structure

| 文件 | 责任 | 本计划改动 |
|---|---|---|
| `loomgui_core/src/render/border.rs` | 边框环 mesh 生成 | 加 `BorderWidths` 类型；`border_ring` 改四边宽度 + 非均匀几何；更新/补单测 |
| `loomgui_core/src/render/mod.rs` | 渲染树构建 | border 调用点改读 `ts.border` 四边（`resolve_lp`）；删 `border_width` 读取 |
| `loomgui_core/src/style/mapping.rs` | CSS → ResolvedStyle | 加 `parse_border_width_color` helper + `BorderSide` + 4 单边 arm；简写 arm 用 helper；删 border_width 赋值 |
| `loomgui_core/src/style/resolved.rs` | IR 结构 | 删 `border_width` 字段 + Default + roundtrip 测试 |
| `loomgui_core/src/style/mapping/tests.rs` | mapping 单测 | border_width 断言改 `ts.border`；加单边 arm 测试 |
| `loomgui_core/src/render/tests.rs` | render 单测 | border 测试设值改 `ts.border` |
| `loomgui_core/tests/fence_contract.rs` | 围栏契约（权威真相源） | supported 表加单边 longhand；改 border_width 断言 |
| `loomgui_core/src/asset/mod.rs` | pkg 格式常量 | `PKG_FORMAT_VERSION` 15→16 + MIN/MAX |
| `docs/design/fence.md` | 围栏人类可读副本 | 补 border-top/right/bottom/left 条目 |

---

## Task 1: `border_ring` 改四边几何 + render 调用点改读 `ts.border`

**Files:**
- Modify: `loomgui_core/src/render/border.rs`（全文重写 `border_ring` + 加 `BorderWidths` + 更新测试）
- Modify: `loomgui_core/src/render/mod.rs` 的 border 调用块（约 411-429）
- Modify: `loomgui_core/src/render/tests.rs:120`

**Interfaces:**
- Consumes: `crate::scene::node::Rect`（既有）；render/mod.rs 既有私有 `fn resolve_lp(LengthPercentage) -> f32`（约 1129 行，Text arm 已在用）
- Produces: `pub struct BorderWidths { top, right, bottom, left: f32 }` + `impl BorderWidths { const fn all(v) }`；`pub fn border_ring(rect: &Rect, radii: &[(f32,f32);4], widths: BorderWidths, color: [f32;4]) -> (verts, uvs, colors, indices)`

- [ ] **Step 1: 写新的非均匀 `border_ring` 单测（先失败）**

在 `loomgui_core/src/render/border.rs` 的 `#[cfg(test)] mod tests` 末尾追加：

```rust
#[test]
fn border_ring_single_side_bottom_only() {
    // border-bottom:1px：只底边有宽 → 只发底边 2 三角 = 6 索引；顶点固定 8（4 未引用）。
    let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
    let radii = [(0.0, 0.0); 4];
    let widths = BorderWidths { top: 0.0, right: 0.0, bottom: 1.0, left: 0.0 };
    let (verts, _u, _c, idx) = border_ring(&r, &radii, widths, [1.0; 4]);
    assert_eq!(verts.len(), 8, "顶点固定 8");
    assert_eq!(idx.len(), 6, "只底边 2 三角 = 6 索引，得 {}", idx.len());
}

#[test]
fn border_ring_asymmetric_four_sides() {
    // 四边各自宽度：内角 = (left, top) / (rw-right, top) / (rw-right, rh-bottom) / (left, rh-bottom)
    let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
    let radii = [(0.0, 0.0); 4];
    let widths = BorderWidths { top: 2.0, right: 3.0, bottom: 4.0, left: 5.0 };
    let (verts, _u, _c, idx) = border_ring(&r, &radii, widths, [1.0; 4]);
    let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
    let ys: Vec<f32> = verts.iter().map(|v| v[1]).collect();
    // inner TL = (5, 2), inner TR = (97, 2), inner BR = (97, 46), inner BL = (5, 46)
    assert!(xs.contains(&5.0) && xs.contains(&97.0), "内轮廓 x = left/right 缩进");
    assert!(ys.contains(&2.0) && ys.contains(&46.0), "内轮廓 y = top/bottom 缩进");
    assert_eq!(idx.len(), 24, "四边全 >0 → 24 索引");
}

#[test]
fn border_ring_opposite_sides_exceed_width_clamped() {
    // left+right > rw → per-axis 比例缩（CSS 语义）。left=80,right=40,rw=100 → scale=100/120
    let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
    let radii = [(0.0, 0.0); 4];
    let widths = BorderWidths { top: 0.0, right: 40.0, bottom: 0.0, left: 80.0 };
    let (verts, _u, _c, _idx) = border_ring(&r, &radii, widths, [1.0; 4]);
    // 钳后 left≈66.67, right≈33.33；inner TL.x = x+left ≈ 66.67, inner TR.x = x+rw-right ≈ 66.67
    // → inner 宽 ≈ 0（塌缩），但不交叉（无负坐标越界）
    let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
    assert!(xs.iter().all(|&x| x >= 0.0 && x <= 100.0), "钳制后坐标不越界");
    assert!((xs.iter().cloned().fold(f32::MAX, f32::min) - 0.0).abs() < 1e-3, "外轮廓 x=0 仍在");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core border_ring`
Expected: 编译失败——`BorderWidths` 未定义、`border_ring` 签名仍是 `width: f32`。

- [ ] **Step 3: 重写 `border.rs`（加 `BorderWidths` + 改 `border_ring`）**

把 `loomgui_core/src/render/border.rs` 顶部的 `border_ring` 函数（从 `pub fn border_ring(` 到它的结束 `}`）替换为下面两段。先在 `use` 之后加 `BorderWidths`：

```rust
/// 四边 border 宽度（像素，已 resolve）。命名防 parse_four 的 [t,r,b,l] 索引错位。
/// 仅作 border_ring 参数，不序列化、不进 ResolvedStyle。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BorderWidths {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl BorderWidths {
    /// 四边同值（均匀环，等价旧 border_ring 行为）。
    pub const fn all(v: f32) -> Self {
        Self { top: v, right: v, bottom: v, left: v }
    }
}
```

再把 `border_ring` 整体替换为：

```rust
/// 生成彩色边框 mesh：外轮廓（rect 4 角）减内轮廓（四边各自向内缩）的环形三角带。
///
/// - 非均匀宽度：每边梯形带宽 = 该边 widths；角是邻接梯形重叠区，不需额外几何。
/// - per-axis 比例钳制（CSS 浏览器语义）：对边和超过 rect 尺寸时等比缩，防内轮廓交叉。
/// - per-edge 跳零宽：width=0 的边不发三角（顶点仍 8 个——零宽邻边的内角被相邻非零边引用）。
/// - radii 遇 border-radius 当前直角退化（圆角留待 SDF task）。
/// - 返 SOA 四表（verts/uvs/colors/indices），uvs 全 0（纯色不采样）。
pub fn border_ring(
    rect: &Rect,
    radii: &[(f32, f32); 4],
    widths: BorderWidths,
    color: [f32; 4],
) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>) {
    let _ = radii; // 直角退化；圆角 SDF task 再处理
    let (x, y, rw, rh) = (rect.x, rect.y, rect.w, rect.h);
    if rw <= 0.0 || rh <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    if widths.top <= 0.0 && widths.right <= 0.0 && widths.bottom <= 0.0 && widths.left <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    // per-axis 比例钳制：对边和 > 尺寸 → 等比缩（只缩不放）
    let (mut t, mut r, mut b, mut l) = (widths.top, widths.right, widths.bottom, widths.left);
    let xsum = l + r;
    if xsum > rw && xsum > 0.0 {
        let s = rw / xsum;
        l *= s;
        r *= s;
    }
    let ysum = t + b;
    if ysum > rh && ysum > 0.0 {
        let s = rh / ysum;
        t *= s;
        b *= s;
    }
    // 外轮廓 4 角（TL,TR,BR,BL），内轮廓按四边独立缩进
    let outer = [[x, y], [x + rw, y], [x + rw, y + rh], [x, y + rh]];
    let inner = [
        [x + l, y + t],
        [x + rw - r, y + t],
        [x + rw - r, y + rh - b],
        [x + l, y + rh - b],
    ];
    let mut verts = Vec::with_capacity(8);
    verts.extend_from_slice(&outer);
    verts.extend_from_slice(&inner);
    let uvs = vec![[0.0, 0.0]; 8];
    let colors = vec![color; 8];
    // 每边 2 三角；width>0 才发（顶点固定 8）
    let widths_arr = [t, r, b, l];
    let mut indices = Vec::with_capacity(24);
    for i in 0..4 {
        if widths_arr[i] <= 0.0 {
            continue;
        }
        let ni = (i + 1) % 4;
        let (oi, oni) = (i as u32, ni as u32);
        let (ii, ini) = ((i + 4) as u32, (ni + 4) as u32);
        indices.extend_from_slice(&[oi, oni, ini, oi, ini, ii]);
    }
    (verts, uvs, colors, indices)
}
```

- [ ] **Step 4: 更新 `border.rs` 既有 6 个测试的 `border_ring` 调用签名**

旧测试都传 `width: f32`（如 `border_ring(&r, &radii, 5.0, [1.0; 4])`），改成 `BorderWidths::all(5.0)` 或 `BorderWidths::default()`。逐个改：

- `border_ring_zero_width_empty`：`border_ring(&r, &radii, 0.0, ...)` → `BorderWidths::default()`（全 0 → 空）。断言不变。
- `border_ring_rect_has_outer_and_inner_loops`：`5.0` → `BorderWidths::all(5.0)`。8 顶点 24 索引断言不变。
- `border_ring_inner_loop_inset_by_width`：`5.0` → `BorderWidths::all(5.0)`。内角 (5,5)/(95,5)/(95,45)/(5,45) 断言不变。
- `border_ring_degenerate_rect_empty`：`5.0` → `BorderWidths::all(5.0)`。空输出断言不变。
- `border_ring_width_clamped_to_half_rect`：**整测试替换**为 per-axis 钳制语义（删掉旧的"钳到短边一半"逻辑已不存在）：

```rust
#[test]
fn border_ring_width_clamped_per_axis() {
    // 四边同值超尺寸：100×50 rect，all(200) → x 方向 left+right=400>100 缩到 50+50，
    // y 方向 top+bottom=400>50 缩到 25+25。内轮廓不交叉、不越界。
    let r = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
    let radii = [(0.0, 0.0); 4];
    let (verts, _u, _c, _i) = border_ring(&r, &radii, BorderWidths::all(200.0), [1.0; 4]);
    let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
    let ys: Vec<f32> = verts.iter().map(|v| v[1]).collect();
    assert!(xs.contains(&50.0), "x 钳后 left=right=50 → inner x=50");
    assert!(ys.contains(&25.0), "y 钳后 top=bottom=25 → inner y=25");
    assert!(verts.iter().all(|v| v[0] >= 0.0 && v[0] <= 100.0 && v[1] >= 0.0 && v[1] <= 50.0));
}
```

- `border_ring_uvs_all_zero`：`5.0` → `BorderWidths::all(5.0)`。UV 全 0 断言不变。

- [ ] **Step 5: 改 `render/mod.rs` border 调用块改读 `ts.border`**

把 `loomgui_core/src/render/mod.rs` 中这段（`// 彩色边框激活` 注释下，约 411-429）：

```rust
                if !has_image {
                    if let Some(border_col) = n.style.border_color {
                        if n.style.border_width > 0.0 {
                            let br = crate::render::border::border_ring(
                                rect,
                                &radii,
                                n.style.border_width,
                                border_col,
                            );
                            if !br.3.is_empty() {
                                let base = v.len() as u32;
                                v.extend_from_slice(&br.0);
                                uvc.extend_from_slice(&br.1);
                                col.extend_from_slice(&br.2);
                                idx.extend(br.3.iter().map(|i| i + base));
                            }
                        }
                    }
                }
```

替换为：

```rust
                if !has_image {
                    if let Some(border_col) = n.style.border_color {
                        let bw = &n.style.taffy_style.border;
                        let widths = crate::render::border::BorderWidths {
                            top: resolve_lp(bw.top),
                            right: resolve_lp(bw.right),
                            bottom: resolve_lp(bw.bottom),
                            left: resolve_lp(bw.left),
                        };
                        if widths.top > 0.0 || widths.right > 0.0
                            || widths.bottom > 0.0 || widths.left > 0.0
                        {
                            let br = crate::render::border::border_ring(
                                rect,
                                &radii,
                                widths,
                                border_col,
                            );
                            if !br.3.is_empty() {
                                let base = v.len() as u32;
                                v.extend_from_slice(&br.0);
                                uvc.extend_from_slice(&br.1);
                                col.extend_from_slice(&br.2);
                                idx.extend(br.3.iter().map(|i| i + base));
                            }
                        }
                    }
                }
```

（`resolve_lp` 是本文件既有私有 fn，约 1129 行，Text arm 已在用——无需新增 import。）

- [ ] **Step 6: 改 `render/tests.rs:120` 测试设值**

把 `loomgui_core/src/render/tests.rs` 的 `build_container_with_border_emits_border_node` 测试里这行：

```rust
    n.style.border_width = 4.0;
```

替换为（设四边等宽 4px 的 `ts.border`，等价旧均匀环——回归保护）：

```rust
    n.style.taffy_style.border = taffy::geometry::Rect::length(4.0_f32);
```

（`taffy::geometry::Rect::length` 是 taffy 0.5 既有构造——四边同值。`resolved.rs:311` padding 已用同款。）

- [ ] **Step 7: 跑测试确认通过**

Run: `cargo test -p loomgui_core border_ring && cargo test -p loomgui_core build_container_with_border`
Expected: PASS（含新 3 个非均匀测试 + 更新后的旧测试 + container border 端到端）。

- [ ] **Step 8: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 全过（无 warning）。若有 `field_reassign_with_default` 之类可辩护 lint，按 CLAUDE.md 在 crate root `#![allow]` 加（带理由注释）——但本 task 预期无需。

- [ ] **Step 9: Commit**

```bash
git add loomgui_core/src/render/border.rs loomgui_core/src/render/mod.rs loomgui_core/src/render/tests.rs
git commit -m "refactor(border): border_ring takes per-side widths, render reads ts.border

border_ring signature width:f32 -> BorderWidths; inner outline insets
per-side; per-axis ratio clamp (CSS semantics); skip zero-width edges.
render reads ts.border four sides via existing resolve_lp (Text arm
precedent). Uniform four-side case unchanged (regression-safe).
border_width field still exists (removed in follow-up commit)."
```

（结尾加 trailer：`Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`——所有 commit 同。）

---

## Task 2: mapping 加 4 个单边 longhand arm

**Files:**
- Modify: `loomgui_core/src/style/mapping.rs`（加 helper + `BorderSide` + 4 arm；简写 arm 用 helper）
- Modify: `loomgui_core/src/style/mapping/tests.rs`（加单边 arm 测试）

**Interfaces:**
- Consumes: 既有 `parse_color(tok) -> Option<[f32;4]>`、`taffy::style::LengthPercentage`
- Produces: `fn parse_border_width_color(value) -> Option<(f32, Option<[f32;4]>)>`；`enum BorderSide`；apply_decl 新 4 arm `border-top/right/bottom/left`

- [ ] **Step 1: 写单边 longhand 解析测试（先失败）**

在 `loomgui_core/src/style/mapping/tests.rs` 末尾追加：

```rust
/// border-top/right/bottom/left 单边 longhand：设 ts.border 对应边 + border_color，不动其他三边。
#[test]
fn apply_border_side_longhands_set_one_side_only() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-bottom", "1px solid #3a3f55"));
    let ts = &s.taffy_style.border;
    assert!(matches!(ts.bottom, LengthPercentage::Length(1.0)), "bottom 设了");
    assert!(matches!(ts.top, LengthPercentage::Length(0.0)), "top 不动（默认 0）");
    assert!(matches!(ts.left, LengthPercentage::Length(0.0)));
    assert!(matches!(ts.right, LengthPercentage::Length(0.0)));
    let c = s.border_color.expect("单边 color 解析");
    assert_eq!(c[0], 0x3a as f32 / 255.0);

    // 累积：再设 top，bottom 仍在
    assert!(apply_decl(&mut s, "border-top", "4px solid #e0e0e0"));
    assert!(matches!(s.taffy_style.border.top, LengthPercentage::Length(4.0)));
    assert!(matches!(s.taffy_style.border.bottom, LengthPercentage::Length(1.0)), "bottom 不被覆盖");
}

#[test]
fn apply_border_side_longhand_rejects_non_px() {
    // 非 px width → 整条 false（围栏外静默忽略），不碰任何字段
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "border-bottom", "1em solid red"));
    assert!(matches!(s.taffy_style.border.bottom, LengthPercentage::Length(0.0)), "失败不设值");
    assert!(s.border_color.is_none(), "失败不设 color");
}

#[test]
fn apply_border_side_longhand_optional_color() {
    // border-bottom:1px（无 color）→ 设宽度，不碰 border_color
    let mut s = ResolvedStyle::default();
    s.border_color = Some([0.5; 4]);
    assert!(apply_decl(&mut s, "border-bottom", "1px"));
    assert!(matches!(s.taffy_style.border.bottom, LengthPercentage::Length(1.0)));
    assert_eq!(s.border_color, Some([0.5; 4]), "无 color token 不覆盖");
}
```

（确认 `LengthPercentage` 在 tests.rs 已 import——既有 `apply_border_shorthand_sets_width_and_color` 测试已用 `LengthPercentage::Length`，所以已在 scope。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core apply_border_side`
Expected: FAIL——`border-bottom` arm 不存在，`apply_decl` 返 false。

- [ ] **Step 3: 加 `parse_border_width_color` helper + `BorderSide` + 单边处理 fn**

在 `loomgui_core/src/style/mapping.rs` 的 `apply_decl` 函数**之前**加：

```rust
/// 解析 border 宽度+颜色声明：`<width> <style>? <color>?`（CSS 简写语义，style 围栏外忽略）。
/// width 取首个 px token，color 取首个可解析颜色 token。width 缺失 → None（整条无效）。
fn parse_border_width_color(value: &str) -> Option<(f32, Option<[f32; 4]>)> {
    let mut w: Option<f32> = None;
    let mut color: Option<[f32; 4]> = None;
    for tok in value.split_whitespace() {
        if color.is_none() {
            if let Some(c) = parse_color(tok) {
                color = Some(c);
                continue;
            }
        }
        if w.is_none() {
            if let Some(px) = tok
                .strip_suffix("px")
                .and_then(|s| s.trim().parse::<f32>().ok())
            {
                w = Some(px);
            }
        }
    }
    Some((w?, color))
}

/// CSS border 四边（用于单边 longhand）。
enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
}

/// border-top/right/bottom/left 单边 longhand：设 ts.border 对应边 + border_color，不动其他三边。
fn apply_border_side(style: &mut ResolvedStyle, side: BorderSide, value: &str) -> bool {
    let Some((w, color)) = parse_border_width_color(value) else {
        return false;
    };
    let lp = LengthPercentage::Length(w);
    let ts = &mut style.taffy_style;
    match side {
        BorderSide::Top => ts.border.top = lp,
        BorderSide::Right => ts.border.right = lp,
        BorderSide::Bottom => ts.border.bottom = lp,
        BorderSide::Left => ts.border.left = lp,
    }
    if let Some(c) = color {
        style.border_color = Some(c);
    }
    true
}
```

- [ ] **Step 4: 简写 arm 用 helper（DRY）+ 加 4 个单边 arm**

把 `apply_decl` 里现有的 `"border" =>` arm（约 471-505，含手动 width/color token 循环）替换为：

```rust
        "border" => {
            // CSS 简写：四边同值。width + color 共用 parse_border_width_color。
            let Some((w, color)) = parse_border_width_color(value) else {
                return false;
            };
            let lp = LengthPercentage::Length(w);
            ts.border = Rect {
                left: lp,
                right: lp,
                top: lp,
                bottom: lp,
            };
            style.border_width = w;
            if let Some(c) = color {
                style.border_color = Some(c);
            }
            true
        }
```

（注意：`style.border_width = w;` 暂留——`border_width` 字段在 Task 3 删。这里保留赋值保证 Task 2 期间编译过。）

在 `"border" =>` arm 之后（`"border-width" =>` arm 之前）加 4 个单边 arm：

```rust
        "border-top" => apply_border_side(style, BorderSide::Top, value),
        "border-right" => apply_border_side(style, BorderSide::Right, value),
        "border-bottom" => apply_border_side(style, BorderSide::Bottom, value),
        "border-left" => apply_border_side(style, BorderSide::Left, value),
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_core apply_border`
Expected: PASS（新 3 个单边测试 + 既有简写/border-width 测试全过——简写改 helper 后行为不变）。

- [ ] **Step 6: 围栏门 + fmt + clippy**

Run: `cargo test -p loomgui_core fence_contract && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 全过。

- [ ] **Step 7: Commit**

```bash
git add loomgui_core/src/style/mapping.rs loomgui_core/src/style/mapping/tests.rs
git commit -m "feat(mapping): support border-top/right/bottom/left longhands

Per-side border longhands set ts.border.<side> + border_color, leaving
other sides untouched. Extract parse_border_width_color helper (shared
with border shorthand). Enables border-bottom divider rendering."
```

---

## Task 3: 删 `border_width` 字段 + PKG_FORMAT 16（破坏性变更）

**Files:**
- Modify: `loomgui_core/src/style/resolved.rs`（删字段 + Default + roundtrip 测试）
- Modify: `loomgui_core/src/style/mapping.rs`（删 2 处 `border_width` 赋值）
- Modify: `loomgui_core/src/style/mapping/tests.rs`（border_width 断言改 ts.border）
- Modify: `loomgui_core/tests/fence_contract.rs`（border_width 断言改）
- Modify: `loomgui_core/src/asset/mod.rs`（PKG_FORMAT_VERSION 15→16）

**Interfaces:**
- Consumes: Task 1（render 已不读 border_width）、Task 2（mapping 简写 arm 仍赋值 border_width，本 task 删）
- Produces: `ResolvedStyle` 无 `border_width` 字段；`PKG_FORMAT_VERSION = 16`

- [ ] **Step 1: 全仓库 grep 确认 `border_width` 残留点**

Run: `git grep -n border_width -- loomgui_core`
Expected 命中（删前）：`resolved.rs:172`（字段）、`resolved.rs:242`（Default）、`resolved.rs:314`（roundtrip）、`mapping.rs`（简写 arm 赋值 + border-width arm 赋值）、`mapping/tests.rs`（4 处断言）、`fence_contract.rs:121`（断言）。`render/` 应**零命中**（Task 1 已清）。把命中点逐一对应到下面 step。

- [ ] **Step 2: 删 `resolved.rs` 字段 + Default + roundtrip**

- 删字段定义（`resolved.rs:172`）：删除整行 `    pub border_width: f32,`
- 删 Default 赋值（`resolved.rs:242`）：删除整行 `            border_width: 0.0,`
- 删 roundtrip 测试（`resolved.rs:314`）：删除整行 `        s.border_width = 3.0;`。若 roundtrip 测试后续（324+）有 `assert_eq!(s.border_width, ...)` 断言也删（grep 确认）。

- [ ] **Step 3: 删 mapping.rs 2 处 `border_width` 赋值**

- `"border"` 简写 arm：删 `            style.border_width = w;`（Task 2 加的那行）
- `"border-width"` arm（约 507-521）：删注释里提 border_width 的行 + `            style.border_width = t;`。arm 保留 `ts.border = Rect {...}` 四边独立填值（`parse_four` 产 `[t,r,b,l]`），只删单值赋值。

`"border-width"` arm 改后应为：

```rust
        "border-width" => {
            let [t, r, b, l] = match parse_four(value) {
                Some(v) => v,
                None => return false,
            };
            ts.border = Rect {
                left: LengthPercentage::Length(l),
                right: LengthPercentage::Length(r),
                top: LengthPercentage::Length(t),
                bottom: LengthPercentage::Length(b),
            };
            true
        }
```

- [ ] **Step 4: 改 mapping/tests.rs 的 border_width 断言**

逐个改为断言 `ts.border`（既有测试 `apply_border_shorthand_sets_width_and_color` 已有 `ts.border` 四边断言，那些保留；只删/改 `border_width` 单值断言）：

- `apply_border_shorthand_sets_width_and_color`（552）：删 `assert_eq!(s.border_width, 1.0, "border 简写 width");`（558-562 已断言 ts.border 四边，覆盖了）。
- `apply_border_shorthand_token_order_and_optional_color`（570, 579）：`assert_eq!(s.border_width, 2.0)` → `assert!(matches!(s.taffy_style.border.top, LengthPercentage::Length(2.0)))`；579 同理改 `3.0`。
- `apply_border_width_property_leaves_color_untouched`（590）：`assert_eq!(s.border_width, 4.0)` → `assert!(matches!(s.taffy_style.border.top, LengthPercentage::Length(4.0)), "border-width 设四边")`。

- [ ] **Step 5: 改 fence_contract.rs:121 断言**

`border_shorthand_parses_width_and_color` 测试里：

```rust
    assert_eq!(s.border_width, 1.0);
```

替换为：

```rust
    assert!(
        matches!(s.taffy_style.border.top, LengthPercentage::Length(1.0)),
        "border 简写四边同宽 1px"
    );
```

- [ ] **Step 6: PKG_FORMAT_VERSION 15→16**

`loomgui_core/src/asset/mod.rs:23-25`：

```rust
pub const PKG_FORMAT_VERSION: u32 = 15; // v15：删 AssetManifest 段，图尺寸改走 atlas.json + set_image_sizes
pub(crate) const MIN_VERSION: u32 = 15;
pub(crate) const MAX_VERSION: u32 = 15;
```

替换为：

```rust
pub const PKG_FORMAT_VERSION: u32 = 16; // v16：删 ResolvedStyle.border_width 字段（render 改读 ts.border 四边）
pub(crate) const MIN_VERSION: u32 = 16;
pub(crate) const MAX_VERSION: u32 = 16;
```

`asset/tests.rs` 不用改——`old_version_pkg_rejected` 用 `MIN_VERSION - 1`、`read_rejects_too_new_version` 用 `MAX_VERSION + 1`、`header_is_20_bytes` 用 `PKG_FORMAT_VERSION` 符号，自动跟随 v16。

- [ ] **Step 7: 跑测试确认零残留 + 通过**

Run: `git grep -n border_width -- loomgui_core`
Expected: **零命中**（全清）。

Run: `cargo test -p loomgui_core`
Expected: PASS（含 fence_contract、mapping、render、resolved roundtrip、asset version 拒绝测试）。

- [ ] **Step 8: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 全过。

- [ ] **Step 9: Commit**

```bash
git add loomgui_core/src/style/resolved.rs loomgui_core/src/style/mapping.rs loomgui_core/src/style/mapping/tests.rs loomgui_core/tests/fence_contract.rs loomgui_core/src/asset/mod.rs
git commit -m "refactor(style): drop border_width field, bump PKG_FORMAT 15->16

border_width was a lossy single-value projection of ts.border that
prevented per-side border rendering. Removed; render reads ts.border
four sides directly (Task 1). bincode layout change -> PKG_FORMAT 16
rejects stale v15 pkgs (must re-pack)."
```

---

## Task 4: fence 文档/契约加单边 longhand 条目

**Files:**
- Modify: `loomgui_core/tests/fence_contract.rs`（supported 表加 4 单边 longhand）
- Modify: `docs/design/fence.md`（补 border-top/right/bottom/left 条目）

**Interfaces:** 无（文档/契约同步）

- [ ] **Step 1: fence_contract supported 表加单边 longhand**

`loomgui_core/tests/fence_contract.rs` 的 `supported_visual_props_return_true` 测试 cases 数组里（约 94-97 border 区），在 `("border-width", "2px"),` 之后加：

```rust
        ("border-top", "1px solid #3a3f55"),
        ("border-right", "1px solid #3a3f55"),
        ("border-bottom", "1px solid #3a3f55"),
        ("border-left", "1px solid #3a3f55"),
```

- [ ] **Step 2: 跑围栏门确认**

Run: `cargo test -p loomgui_core supported_visual_props_return_true`
Expected: PASS（4 单边 longhand 现返回 true）。

- [ ] **Step 3: fence.md 补条目**

`docs/design/fence.md` 的 border 区（约 line 69 `border` 简写条目附近），在 `border-width` 条目后加一行（参照既有 `border` 条目格式）：

```markdown
| `border-top`/`border-right`/`border-bottom`/`border-left` | 单边 longhand `<width> <style>? <color>?`：设 `ts.border` 对应边 + `border_color`（四边共享单色），不动其他三边。多单边声明累积 | mapping.rs `apply_border_side` | 【实证】 |
```

- [ ] **Step 4: Commit**

```bash
git add loomgui_core/tests/fence_contract.rs docs/design/fence.md
git commit -m "docs(fence): add border-top/right/bottom/left to supported subset"
```

---

## Task 5: 重编 .dll + 重打 pkg + PlayMode 验收（家里机）

**Files:**
- Modify: `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`（重建）
- Modify: `loomgui_unity/Assets/Bundles/ui/showcase.pkg.bin`（重打，PKG_FORMAT 16）

**Interfaces:** 无（构建产物）

> **前提**：Task 1-4 全 commit + push。家里机 pull 后做 PlayMode 验收。公司机做 dll + pkg 重打。

- [ ] **Step 1: 重编 .dll（公司机，Unity 关着）**

确认 Unity 未运行（锁 .dll）。然后：

Run: `cargo build -p loomgui_ffi_c --release`
Expected: 编译成功，产 `target/release/loomgui_ffi_c.dll`。

- [ ] **Step 2: 拷 dll 入库**

Run（PowerShell）：`copy-item target\release\loomgui_ffi_c.dll loomgui_unity_package\Plugins\LoomGUI\loomgui_ffi_c.dll`
Expected: 拷贝成功。若报 `Device or resource busy` → Unity 还开着，关 Unity 重试。

- [ ] **Step 3: stale .dll 诊断（可选，验 dll 确实更新）**

Run: `md5sum target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Bash）或 `Get-FileHash` 两文件比对（PowerShell）。
Expected: 两个 hash 相等（= 拷贝成功、非 stale）。

- [ ] **Step 4: 重打 showcase pkg（PKG_FORMAT 16）**

Run: `cargo run -p loomgui_pkg -- build showcase_project`
Expected: 打包成功，产 `loomgui_unity/Assets/Bundles/ui/showcase.pkg.bin`（v16）。无 fence violation（border-bottom 现围栏内）。

- [ ] **Step 5: commit dll + pkg**

```bash
git add loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll loomgui_unity/Assets/Bundles/ui/showcase.pkg.bin
git commit -m "build: recompile dll + re-pack showcase (PKG_FORMAT 16, border longhands)"
```

- [ ] **Step 6: PlayMode 验收（家里机，pull 后）**

家里机 `git pull`，Unity 打开 showcase 场景，进 PlayMode。逐项确认（spec §9 R1-R5）：

- **R1**：图片页 §3/§3.8/§3.9 三个 `.sec-h` 标题下各有一条 `#3a3f55` 分割线（`border-bottom:1px solid`）。
- **R2**：图片页 `.header` 下分割线（`border-bottom:2px solid #3a3f55`）。
- **R3**：§3.1 border demo 的 `border-top:4px solid #e0e0e0` 只显示顶边（其余三边无）。
- **R4**：§3.1 其余 border demo（`border:2px solid` 等四边简写）仍四边环正常——零回归。
- **R5**：Console 无报错（尤其无 stale .dll 症状：全不渲 + Console 干净）。

**取证（R3 不符时）**：`cargo run -p loomgui_pkg --example dump_page_image`（或既有 `dump_render`）验单边 border 几何是否进 mesh_arena。

- [ ] **Step 7: 验收记录**

R1-R5 全过 → 本计划完成。若有不符，记症状到 `docs/pitfalls.md`（新坑号递增）并回到对应 Task 修。

---

## Self-Review 笔记（写 plan 后自检）

**Spec 覆盖**：
- §0 目标（单边 longhand + 删 border_width）→ Task 1-3 ✅
- §1.1 减法（render 读 ts.border）→ Task 1 Step 5 ✅
- §1.2 环拓扑不变 3 处改 → Task 1 Step 3 ✅
- §1.3 per-axis 钳制 → Task 1 Step 3（border_ring 内）+ 测试 Step 1/4 ✅
- §1.4 单色共享 → border_color 单值不变，贯穿 ✅
- §2 CSS 子集（单边 longhand 语法）→ Task 2 + Task 4 ✅
- §3 数据流 → Task 1 Step 5 ✅
- §4 解析（helper + 4 arm + 删赋值）→ Task 2 + Task 3 Step 3 ✅
- §5 几何（BorderWidths + 内轮廓 + 钳 + 跳零 + 早返 + winding）→ Task 1 ✅（winding 由 `border_ring_asymmetric_four_sides` 隐式验，若 PlayMode 发现缺角加显式朝向断言）
- §6 共存（bg-image 限制保留、border-image-slice 互斥）→ Task 1 `if !has_image` 保留 ✅
- §7 FFI 零改（payload_hash 覆盖）→ Task 5 重编 dll 即可，无 FFI struct 改 ✅
- §8 PKG_FORMAT 16 → Task 3 Step 6 ✅
- §9 验收 R1-R5 → Task 5 Step 6 ✅
- §10 测试 → Task 1（border_ring）+ Task 2（mapping）+ Task 3（fence/roundtrip）+ Task 4（supported 表）✅
- §11 限制/后续 → 非目标，无需 task ✅

**Placeholder 扫描**：无 TBD/TODO；所有代码块完整。

**类型一致性**：`BorderWidths` 全程一致（Task 1 定义、Task 1 Step 5/6 用）；`parse_border_width_color`（Task 2 定义、简写 arm 调）；`apply_border_side`（Task 2 定义、4 arm 调）；`resolve_lp`（既有、Task 1 复用）。

**注意**：Task 2 的简写 arm 暂留 `style.border_width = w;`（Task 3 删）——这是为保持 Task 2 期间编译过（字段还在），Task 3 删字段时同步删此行。两 task 顺序固定：Task 2 必须先于 Task 3。
