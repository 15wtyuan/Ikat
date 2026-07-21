# Phase1 Spec-P2(视觉装饰:圆角边框 + linear 渐变角度)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐骨架链两处视觉缺口:(A) 让 `border_ring` 真正消费 `border-radius` 画圆角边框(当前背景圆角但边框直角,边角突出 bug);(B) 让 `linear-gradient` 支持任意角度(`45deg` 等,当前仅 4 正向)。

> **本次执行范围(2026-07-21 用户确认)**:Task 1-2(A 圆角边框,无 pkg bump,纯运行时几何)。Task 3-5(linear 角度)defer——linear 一次 pkg bump(21→22)+ v1.8 spec 修订只换 45deg 2 色不划算,等真需要多 stop 时合并做(角度+多 stop+corner 一次 bump)。plan 保留 Task 3-5 作 future linear 的现成设计,不在本轮执行。

**Architecture:** A = 在 `mesh.rs` 抽两个公共圆角 helper(`radius_scale` + `corner_arc_pts`,`rounded_rect` 重构共用),`border.rs::border_ring` 改用"外圆角轮廓 + 内圆角轮廓(半径=外−insets)配对三角带"(内角直角时内外同分段形成 infill 扇),`box_shadow_quad` 改用 `rounded_rect`。B = `Gradient2` 加 `angle: Option<f32>` 字段(serde 变 → pkg 21→22),`parse_linear_gradient` 解析 `<deg>`/`to <dir>`,`gradient_corner_colors` 加 CSS 渐变线角度算法(2 色 per-vertex 双线性 = 正确 affine 色场)。验收仿 spec4b headless(Rust render 层断言 + C# HeadlessTests,不碰 Unity)。

**Tech Stack:** Rust 2021、taffy 0.12(P1 已升)、bincode、csbindgen、xtask;C# xUnit HeadlessTests。

## Global Constraints

(摘自 spec `2026-07-21-phase1-blitz-refactor-design.md` §5.3/5.5 + CLAUDE.md,每个 task 隐含遵守)

- **本机是唯一编码机**:改 Rust 后必 `cargo build -p loomgui_ffi_c --release` + cp dll 到 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` + commit dll(Unity 必关着拷)。
- **改 serde 字段必 bump pkg + 重打**:Task 3 改 `Gradient2`(加 `angle` 字段)→ `asset/mod.rs` `PKG_FORMAT_VERSION` 21→22 + `MIN=MAX=22` + 重打所有工作区包 + dll + GUI exe(CLAUDE.md「改 parse-time 逻辑必重打 pkg」)。
- **pkg 格式一刀切**:`MIN_VERSION = MAX_VERSION`(无后向兼容,无迁移器)。
- **围栏手搓 CSS**:不引入 stylo/cssparser。linear 多 stop / corner 方向 / radial / conic 本 plan 不做(见 Deferred)。
- **不动成熟资产**:渲染批合(`render/mesh`+`batch`)、滚动物理、文本自绘光栅、FFI ABI。`mesh::rounded_rect` 是本 plan 的 DRY 重构对象(抽 helper),不改其三角扇填充语义。
- **两台机串行**:本机 headless 验为主;圆角/渐变视觉几何在 Rust render 层断言 + 浏览器人工对比;Unity 视觉验收推后(家里机)。
- **push 前**:`cargo fmt --all --check` + `cargo clippy --all-targets -D warnings` + `cargo test -p loomgui_fence` + `cargo test --workspace`。
- **SDD 防漂移**:Task 3 改 `Gradient2` 后,搜 docs/ 是否引用旧字段名;改 `mesh::rounded_rect` 内部后确认 render/tests.rs 现有断言不破。

---

## File Structure

| 文件 | 改动 | 责任 |
|---|---|---|
| `crates/core/src/render/mesh.rs` | 抽 `pub fn radius_scale` + `pub fn corner_arc_pts`;`rounded_rect` 重构调 `radius_scale`(DRY,行为不变) | 圆角几何公共 helper |
| `crates/core/src/render/border.rs` | `border_ring` 改圆角环(outline 配对三角带);`box_shadow_quad` 改用 `rounded_rect` | A 主战场 |
| `crates/core/src/render/tests.rs` | 加圆角 border / 圆角 box-shadow 回归 | A 测试 |
| `crates/core/src/style/resolved.rs` | `Gradient2` 加 `angle: Option<f32>` 字段 | B 数据结构(serde 变) |
| `crates/core/src/style/mapping.rs` | `parse_linear_gradient_2` → `parse_linear_gradient`(解析 `<deg>` + 保留 `to <dir>`) | B 解析 |
| `crates/core/src/style/mapping/tests.rs` | 加角度渐变解析测试 | B 测试 |
| `crates/core/src/render/mod.rs` | `gradient_corner_colors` 加 CSS 渐变线角度算法;`gradient_glyph_colors` angle fallback | B 渲染 |
| `crates/core/src/asset/mod.rs` | `PKG_FORMAT_VERSION` 21→22 | B pkg bump |
| `crates/fence/src/schema/css.rs` + `docs/design/fence.md` | `Gradient2` 描述扩 `<deg>` 角度语法 | 围栏镜像 |
| `docs/superpowers/specs/2026-07-08-v1.8-text-effects-decoration-design.md` | line 60/98 修订:linear 角度移出围栏外 | 防漂移 |
| `showcase/spec4b/p2-visual-acceptance.html` | 新建 P2 验收页(圆角 border + 角度渐变) | 验收基准 |
| `tests/dotnet/LoomGUI.HeadlessTests/VisualDecorationTests.cs` | 新建 P2 验收 test | C# headless 验收 |

---

### Task 1: mesh.rs 抽公共圆角 helper(radius_scale + corner_arc_pts)

**Files:**
- Modify: `crates/core/src/render/mesh.rs:81-105`(`rounded_rect` 的 CSS 缩放段抽成 `radius_scale`)+ 新增 `corner_arc_pts`
- Test: `crates/core/src/render/tests.rs`

**Interfaces:**
- Produces: `pub fn radius_scale(radii: &[(f32,f32);4], w: f32, h: f32) -> f32`、`pub fn corner_arc_pts(corner: [f32;2], rx: f32, ry: f32, center: [f32;2], start: f32, seg: u32) -> Vec<[f32;2]>`。Task 2 `border_ring` 消费这两个。

- [ ] **Step 1: 加 `radius_scale`(从 rounded_rect 抽出,行为不变)**

在 `crates/core/src/render/mesh.rs` `rounded_rect` 之前插入:
```rust
/// CSS border-radius 邻角和缩放因子(只缩不放,防负)。两邻角半径和 ≤ 边长,等比缩。
/// `rounded_rect`(背景填充)与 `border::border_ring`(边框环)共用,保证两者圆角一致。
pub fn radius_scale(radii: &[(f32, f32); 4], w: f32, h: f32) -> f32 {
    let (tl, tr, br, bl) = (radii[0], radii[1], radii[2], radii[3]);
    1.0_f32
        .min(w / (tl.0 + tr.0).max(1e-6))
        .min(w / (bl.0 + br.0).max(1e-6))
        .min(h / (tl.1 + bl.1).max(1e-6))
        .min(h / (tr.1 + br.1).max(1e-6))
}

/// 单角圆弧顶点序列(seg+1 个点,沿 start→start+π/2)。
/// - 圆角(rx>0 且 ry>0):弧上 seg+1 个点(末段锁 start+π/2 保精度,照 fgui)。
/// - 直角(rx≤0 或 ry≤0):产 seg+1 个 corner 点(重复),供 `border_ring` 环带配对——
///   外圆内方时内角直角但与外角同分段,自动形成角内 infill 扇。
pub fn corner_arc_pts(
    corner: [f32; 2],
    rx: f32,
    ry: f32,
    center: [f32; 2],
    start: f32,
    seg: u32,
) -> Vec<[f32; 2]> {
    if rx <= 0.0 || ry <= 0.0 {
        return vec![corner; seg as usize + 1];
    }
    let delta = std::f32::consts::FRAC_PI_2 / seg as f32;
    (0..=seg)
        .map(|j| {
            let a = if j == seg {
                start + std::f32::consts::FRAC_PI_2
            } else {
                start + delta * j as f32
            };
            [center[0] + a.cos() * rx, center[1] + a.sin() * ry]
        })
        .collect()
}
```

- [ ] **Step 2: rounded_rect 重构用 radius_scale(DRY,行为不变)**

`crates/core/src/render/mesh.rs:94-105` 原内联缩放:
```rust
// 原:
let (tl, tr, br, bl) = (radii[0], radii[1], radii[2], radii[3]);
let scale = 1.0_f32
    .min(w / (tl.0 + tr.0).max(1e-6))
    .min(w / (bl.0 + br.0).max(1e-6))
    .min(h / (tl.1 + bl.1).max(1e-6))
    .min(h / (tr.1 + br.1).max(1e-6));
let scale_r =
    |r: (f32, f32)| -> (f32, f32) { ((r.0 * scale).max(0.0), (r.1 * scale).max(0.0)) };
```
改为:
```rust
let scale = radius_scale(radii, w, h);
let scale_r =
    |r: (f32, f32)| -> (f32, f32) { ((r.0 * scale).max(0.0), (r.1 * scale).max(0.0)) };
```

- [ ] **Step 3: 加 helper 单测(直角退化产重复点、圆角分段数)**

`crates/core/src/render/tests.rs` 加:
```rust
#[test]
fn corner_arc_pts_sharp_returns_repeated_corner() {
    let pts = crate::render::mesh::corner_arc_pts(
        [10.0, 20.0], 0.0, 5.0, [0.0, 0.0], 0.0, 3,
    );
    assert_eq!(pts.len(), 4, "seg+1 个点");
    assert!(pts.iter().all(|&p| p == [10.0, 20.0]), "全落 corner");
}

#[test]
fn corner_arc_pts_round_arc_endpoints() {
    // start=0(右方向),seg=2:0, π/4, π/2
    let pts = crate::render::mesh::corner_arc_pts(
        [0.0, 0.0], 10.0, 10.0, [0.0, 0.0], 0.0, 2,
    );
    assert_eq!(pts.len(), 3);
    assert!((pts[0][0] - 10.0).abs() < 1e-4 && pts[0][1].abs() < 1e-4, "起点 (rx,0)");
    assert!((pts[2][1] - 10.0).abs() < 1e-4, "末点 y=ry");
}

#[test]
fn radius_scale_clamps_oversized() {
    // 两邻角和 > 边长 → 等比缩
    let s = crate::render::mesh::radius_scale(&[(60.0, 60.0), (60.0, 60.0), (0.0, 0.0), (0.0, 0.0)], 100.0, 100.0);
    assert!(s < 1.0 && s > 0.0, "缩放因子 ∈ (0,1)");
}
```

- [ ] **Step 4: 跑 test 验证 helper + rounded_rect 重构不破**

Run: `cargo test -p loomgui_core render::tests`
Expected: PASS(含新 3 个 + 现有 `container_radius_uses_rounded_rect` 等不破)。

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt --all && cargo clippy -p loomgui_core -- -D warnings`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/render/mesh.rs crates/core/src/render/tests.rs
git commit -m "refactor(render): extract radius_scale + corner_arc_pts helpers from rounded_rect"
```

---

### Task 2: border_ring 圆角化(消费 radii)+ box_shadow_quad 圆角

**Files:**
- Modify: `crates/core/src/render/border.rs:36-91`(`border_ring`)+ `border.rs:99-115`(`box_shadow_quad`)
- Test: `crates/core/src/render/tests.rs`

**Interfaces:**
- Consumes: Task 1 的 `mesh::radius_scale` + `mesh::corner_arc_pts`。
- Produces: `border_ring` 与 `box_shadow_quad` 消费 `radii` 参数(签名不变,删 `let _ = radii`)。`render/mod.rs:438-440` / `mod.rs:646-655` 调用点不变。

**几何模型(实现必读)**:
- 外轮廓 = 外圆角矩形周边顶点(每角 `corner_arc_pts`,外半径经 `radius_scale`)。
- 内轮廓 = 内圆角矩形周边顶点。**内半径 = 外半径 − 邻边 insets**(per-corner per-axis,钳 0);**内角圆心 = 外角圆心**(因 `inner_rect 角 + inner_radii = (rect 角+insets) + (外半径−insets) = rect 角 + 外半径 = 外圆心`)。
- 内外角**同分段** `seg = max(seg_of(外), seg_of(内), 2)`。内角直角(内半径≤0)时 `corner_arc_pts` 产 seg+1 个内角点 → 与外角弧顶点配对形成角内 infill 扇(外圆内方自动正确)。
- 环带三角:每对邻顶点 `(i, i+1)` 配 2 三角 `[外i, 外i+1, 内i+1] + [外i, 内i+1, 内i]`。

- [ ] **Step 1: 写失败 test(border_ring 圆角时顶点数 > 8)**

`crates/core/src/render/tests.rs` 加:
```rust
#[test]
fn border_ring_rounded_has_more_vertices_than_sharp() {
    use crate::render::border::{border_ring, BorderWidths};
    use crate::scene::node::Rect;
    let rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
    let w = BorderWidths::all(4.0);
    let sharp = border_ring(&rect, &[(0.0,0.0);4], w, [1.0;4]);
    let round = border_ring(&rect, &[(20.0,20.0);4], w, [1.0;4]);
    assert_eq!(sharp.0.len(), 8, "直角环 8 顶点(现状)");
    assert!(round.0.len() > 8, "圆角环顶点数 > 8(弧分段), got {}", round.0.len());
    assert_eq!(round.0.len() % 2, 0, "外+内轮廓等长");
}

#[test]
fn border_ring_rounded_inner_radius_clamps() {
    // 外半径 5 < border width 10 → 内半径钳 0(内角直角),外圆内方
    use crate::render::border::{border_ring, BorderWidths};
    use crate::scene::node::Rect;
    let rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
    let (verts, _, _, _) = border_ring(&rect, &[(5.0,5.0);4], BorderWidths::all(10.0), [1.0;4]);
    assert!(verts.len() > 8, "外圆角分段 + 内角点 infill");
}

#[test]
fn border_ring_zero_width_returns_empty() {
    use crate::render::border::{border_ring, BorderWidths};
    use crate::scene::node::Rect;
    let rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
    let (v, _, _, _) = border_ring(&rect, &[(20.0,20.0);4], BorderWidths::all(0.0), [1.0;4]);
    assert!(v.is_empty(), "全零宽早期返回");
}
```

- [ ] **Step 2: 跑 test 验证失败**

Run: `cargo test -p loomgui_core render::tests::border_ring_rounded`
Expected: FAIL(`border_ring_rounded_has_more_vertices_than_sharp`:圆角仍 8 顶点,因 `let _ = radii`)。

- [ ] **Step 3: 改写 border_ring 为圆角环**

`crates/core/src/render/border.rs:36-91` 整个 `border_ring` 函数体替换为(签名不变,删 `let _ = radii`):
```rust
pub fn border_ring(
    rect: &Rect,
    radii: &[(f32, f32); 4],
    widths: BorderWidths,
    color: [f32; 4],
) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>) {
    let (x, y, rw, rh) = (rect.x, rect.y, rect.w, rect.h);
    if rw <= 0.0
        || rh <= 0.0
        || (widths.top <= 0.0 && widths.right <= 0.0 && widths.bottom <= 0.0 && widths.left <= 0.0)
    {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    // per-axis width 钳制:对边和 > 尺寸等比缩(只缩不放)
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
    // 外半径 CSS 邻角缩放(与 rounded_rect 共用 → 背景圆角与边框圆角一致)
    let scale = crate::render::mesh::radius_scale(radii, rw, rh);
    let sr = |rad: (f32, f32)| ((rad.0 * scale).max(0.0), (rad.1 * scale).max(0.0));
    let (tl, tr, br, bl) = (sr(radii[0]), sr(radii[1]), sr(radii[2]), sr(radii[3]));
    // 内半径 = 外半径 − 邻边 insets(per-corner per-axis,钳 0)。内角直角(≤0)时外圆内方。
    let itl = ((tl.0 - l).max(0.0), (tl.1 - t).max(0.0));
    let itr = ((tr.0 - r).max(0.0), (tr.1 - t).max(0.0));
    let ibr = ((br.0 - r).max(0.0), (br.1 - b).max(0.0));
    let ibl = ((bl.0 - l).max(0.0), (bl.1 - b).max(0.0));

    use std::f32::consts::{FRAC_PI_2, PI};
    // 外角 (rx,ry,center,start,corner);内角 center = 外 center(见 plan 几何推导)
    let outer_cfg: [(f32, f32, [f32; 2], f32, [f32; 2]); 4] = [
        (tl.0, tl.1, [x + tl.0, y + tl.1], PI, [x, y]),
        (tr.0, tr.1, [x + rw - tr.0, y + tr.1], -FRAC_PI_2, [x + rw, y]),
        (br.0, br.1, [x + rw - br.0, y + rh - br.1], 0.0, [x + rw, y + rh]),
        (bl.0, bl.1, [x + bl.0, y + rh - bl.1], FRAC_PI_2, [x, y + rh]),
    ];
    // 内角 corner = rect 角 + (inset_x, inset_y);center 复用外角 center
    let inner_corner = [
        [x + l, y + t],
        [x + rw - r, y + t],
        [x + rw - r, y + rh - b],
        [x + l, y + rh - b],
    ];
    let inner_radii = [itl, itr, ibr, ibl];

    let seg_of = |rx: f32, ry: f32| {
        if rx <= 0.0 || ry <= 0.0 {
            1u32
        } else {
            ((PI * rx.max(ry) / 4.0).ceil() as i32 + 1).max(2) as u32
        }
    };
    let mut outer_pts: Vec<[f32; 2]> = Vec::new();
    let mut inner_pts: Vec<[f32; 2]> = Vec::new();
    for i in 0..4 {
        let (orx, ory, oc, os, ocorner) = outer_cfg[i];
        let (irx, iry) = inner_radii[i];
        // 内外同分段:外圆内方时内角直角,corner_arc_pts 产 seg+1 个内角点 → infill 扇
        let seg = seg_of(orx, ory).max(seg_of(irx, iry)).max(2);
        outer_pts.extend(crate::render::mesh::corner_arc_pts(ocorner, orx, ory, oc, os, seg));
        inner_pts.extend(crate::render::mesh::corner_arc_pts(
            inner_corner[i], irx, iry, oc, os, seg,
        ));
    }
    let n = outer_pts.len();
    debug_assert_eq!(n, inner_pts.len(), "内外轮廓等长(同分段)");
    let mut verts = Vec::with_capacity(2 * n);
    verts.extend_from_slice(&outer_pts);
    verts.extend_from_slice(&inner_pts);
    let uvs = vec![[0.0, 0.0]; 2 * n];
    let colors = vec![color; 2 * n];
    // 环带三角带:每对邻顶点 2 三角。零宽边处内外重合 → 退化三角(GPU 免费,不另跳过)。
    let mut indices: Vec<u32> = Vec::with_capacity(6 * n);
    for i in 0..n as u32 {
        let ni = if i + 1 < n as u32 { i + 1 } else { 0 };
        indices.extend_from_slice(&[i, ni, n + ni, i, n + ni, n + i]);
    }
    (verts, uvs, colors, indices)
}
```
同步更新 `border_ring` 的 doc 注释(删"radii 遇 border-radius 当前直角退化",改述圆角环模型)。

- [ ] **Step 4: 改 box_shadow_quad 用 rounded_rect(圆角外扩)**

`crates/core/src/render/border.rs:99-115` 替换 `box_shadow_quad` 函数体(签名不变,删 `let _ = radii`):
```rust
pub fn box_shadow_quad(
    rect: &Rect,
    radii: &[(f32, f32); 4],
    spread: f32,
    color: [f32; 4],
) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>) {
    let outer = Rect {
        x: rect.x - spread,
        y: rect.y - spread,
        w: rect.w + 2.0 * spread,
        h: rect.h + 2.0 * spread,
    };
    if outer.w <= 0.0 || outer.h <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    // 圆角随 spread 外扩(CSS box-shadow:每角半径 + spread)。rounded_rect 内部再 CSS 缩放。
    let spread_radii = [
        (radii[0].0 + spread, radii[0].1 + spread),
        (radii[1].0 + spread, radii[1].1 + spread),
        (radii[2].0 + spread, radii[2].1 + spread),
        (radii[3].0 + spread, radii[3].1 + spread),
    ];
    crate::render::mesh::rounded_rect(&outer, color, &spread_radii, [0.0, 0.0], [0.0, 0.0])
}
```
保留原 doc 的 ponytail 注释(无 blur / 无 alpha falloff,升级路径不变)。

- [ ] **Step 5: 跑 test 验证圆角 border 通过**

Run: `cargo test -p loomgui_core render::tests::border_ring`
Expected: PASS(3 个新 test 绿)。

- [ ] **Step 6: 端到端测(border + border-radius 共存,边框圆角进 mesh)**

`crates/core/src/render/tests.rs` 加(仿现有 `build_container_with_border_emits_border_node`):
```rust
#[test]
fn container_border_with_radius_emits_rounded_border() {
    // border-radius:20px + border:4px solid red → 边框环顶点数 > 直角环 12(背景4+边框8)
    let mut n = container_node(/* 见 render/tests.rs:42-50 现有工厂 */);
    crate::style::mapping::apply_decl(&mut n.style, "border-radius", "20px");
    crate::style::mapping::apply_decl(&mut n.style, "border", "4px solid #ff0000");
    let mut scene = crate::scene::Scene::from_nodes(vec![n], vec![]);
    let fonts = test_font_table().expect("need test font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = crate::render::build_render_nodes(
        &scene, &fonts, &std::collections::HashMap::new(),
        &empty_sizes(), &mut test_glyph_atlas(),
    );
    let mesh = frame.nodes[0].payload.expect_mesh();
    // 背景 rounded_rect(中心+弧) + 边框圆角环(外+内轮廓),总顶点 > 直角态 12
    assert!(mesh.verts.len() > 12, "圆角 border+bg 顶点 > 12, got {}", mesh.verts.len());
    assert!(mesh.colors.contains(&[1.0, 0.0, 0.0, 1.0]), "红色边框顶点存在");
}
```
> 注:`expect_mesh()` / `container_node` 工厂签名以 `render/tests.rs` 现有代码为准(若 payload 取值方式不同,按现有 `build_container_with_border_emits_border_node` 模式调整)。

Run: `cargo test -p loomgui_core render::tests::container_border_with_radius`
Expected: PASS。

- [ ] **Step 7: 全 workspace test + fmt + clippy**

Run: `cargo test --workspace && cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: PASS(现有 border/bg 测试不破)。

- [ ] **Step 8: 重编 dll(运行时几何改,dll 要更新;Task 2 无 pkg bump)**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
(Unity 必关着拷。)

- [ ] **Step 9: Commit**

```bash
git add crates/core/src/render/border.rs crates/core/src/render/tests.rs unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "feat(render): border_ring consumes border-radius (rounded ring) + box_shadow rounded"
```

---

### Task 3: linear-gradient 任意角度(Gradient2 加 angle + pkg 21→22)

**Files:**
- Modify: `crates/core/src/style/resolved.rs:56-70`(`Gradient2` 加字段)
- Modify: `crates/core/src/style/mapping.rs:302-337`(`parse_linear_gradient_2` → `parse_linear_gradient`)+ `:711-735`(调用点)
- Modify: `crates/core/src/render/mod.rs:66-75`(`gradient_corner_colors` 加角度)+ `:81-123`(`gradient_glyph_colors` angle fallback)
- Modify: `crates/core/src/asset/mod.rs:20-22`(`PKG_FORMAT_VERSION` 21→22)
- Test: `crates/core/src/style/mapping/tests.rs`、`crates/core/src/render/tests.rs`、`crates/core/src/style/resolved.rs`(bincode round-trip)

**Interfaces:**
- Produces: `Gradient2 { color_a, color_b, dir, angle: Option<f32> }`。`angle=Some(deg)` 时背景渐变走 CSS 渐变线角度算法;`angle=None` 走原 `dir` 4 正向(行为不变,向后兼容)。

**范围说明(诚实标注)**:
- ✅ 做:2 色 + 任意角度(`45deg` 等)+ 4 正向(向后兼容)。2 色 linear = affine 色场,4 角顶点色双线性插值 = **正确**(无需细分)。
- ❌ Defer(本 plan 不做,见 Deferred 段):linear 多 stop(>2 色,需细分 mesh,4 角双线性表达不了分段)、corner 方向(`to top right`,需按 rect 长宽比算角度)、text 渐变角度(`background-clip:text` + `45deg`,gradient_glyph_colors 遇 angle fallback 到 dir)。

- [ ] **Step 1: 写失败 test(45deg 渐变解析进 angle 字段)**

`crates/core/src/style/mapping/tests.rs` 加:
```rust
#[test]
fn parse_linear_gradient_angle_deg() {
    let mut s = crate::style::resolved::ResolvedStyle::default();
    assert!(crate::style::mapping::apply_decl(
        &mut s, "background", "linear-gradient(45deg, #ff0000, #0000ff)"
    ));
    let g = s.background_gradient.expect("gradient parsed");
    assert_eq!(g.color_a, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(g.color_b, [0.0, 0.0, 1.0, 1.0]);
    assert!((g.angle.unwrap() - 45.0).abs() < 1e-4, "angle=45deg");
}

#[test]
fn parse_linear_gradient_to_dir_still_works() {
    let mut s = crate::style::resolved::ResolvedStyle::default();
    assert!(crate::style::mapping::apply_decl(
        &mut s, "background", "linear-gradient(to right, #ff0000, #0000ff)"
    ));
    let g = s.background_gradient.unwrap();
    assert!(g.angle.is_none(), "to right → angle=None(用 dir)");
    assert_eq!(g.dir, crate::style::resolved::GradientDir::ToRight);
}

#[test]
fn parse_linear_gradient_multistop_rejected() {
    // 多 stop 需细分 mesh(P2 defer),围栏外静默忽略
    let mut s = crate::style::resolved::ResolvedStyle::default();
    assert!(!crate::style::mapping::apply_decl(
        &mut s, "background", "linear-gradient(to right, #ff0000, #00ff00, #0000ff)"
    ));
}
```

- [ ] **Step 2: 跑 test 验证失败**

Run: `cargo test -p loomgui_core style::mapping::tests::parse_linear_gradient_angle`
Expected: FAIL(`apply_decl` 当前 sniff `linear-gradient(` 后调 `parse_linear_gradient_2`,45deg 落 `_ => return false`,返 false;`multistop_rejected` 反而会过,但 `angle_deg` 失败)。

- [ ] **Step 3: Gradient2 加 angle 字段**

`crates/core/src/style/resolved.rs:56-70`,在 `Gradient2` struct 末尾加字段:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gradient2 {
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub dir: GradientDir,
    /// CSS 渐变角度(deg):`linear-gradient(45deg, ...)`。None=用 `dir`(4 正向,向后兼容)。
    /// CSS 语义:0deg=to top,90deg=to right(顺时针递增)。
    pub angle: Option<f32>,
}
```
> ⚠️ 加字段 = bincode 格式变 → Task 3 Step 9 bump pkg 21→22 + 重打所有包。

全仓搜 `Gradient2 {` 构造点(`mapping.rs`、测试),补 `angle: None`(4 正向)或 `angle: Some(deg)`。`Default` 若 derive 了需补(Gradient2 当前无 Default,确认)。

- [ ] **Step 4: parse_linear_gradient 泛化(角度 + 4 正向,2 色)**

`crates/core/src/style/mapping.rs:302-337` 替换 `parse_linear_gradient_2` 为:
```rust
/// 解析 `linear-gradient(...)` 内部串(已去外层 `linear-gradient(` `)`)。
///
/// 围栏子集(P2 扩展):`to right/left/top/bottom`(4 正向,angle=None)+ `<deg>` 任意角度
/// + 恰好 2 色 stop。多 stop(>2 色)/corner 方向(`to top right`)/不可解析 → false
/// (围栏外静默忽略,与 clip-path 等同模式)。
fn parse_linear_gradient(style: &mut ResolvedStyle, inner: &str) -> bool {
    let parts = split_commas_top(inner);
    let mut idx = 0;
    let mut angle = None;
    let mut dir = None;
    if let Some(first) = parts.first() {
        let f = first.trim();
        if let Some(rest) = f.strip_prefix("to ") {
            dir = match rest.trim() {
                "right" => Some(GradientDir::ToRight),
                "left" => Some(GradientDir::ToLeft),
                "top" => Some(GradientDir::ToTop),
                "bottom" => Some(GradientDir::ToBottom),
                _ => return false, // corner(to top right 等)围栏外 defer
            };
            idx = 1;
        } else if let Some(deg) = f.strip_suffix("deg") {
            match deg.trim().parse::<f32>() {
                Ok(v) => {
                    angle = Some(v);
                    idx = 1;
                }
                Err(_) => return false,
            }
        }
    }
    let stops: Vec<[f32; 4]> = parts[idx..]
        .iter()
        .filter_map(|s| parse_color(s.trim()))
        .collect();
    if stops.len() != 2 {
        return false; // 多 stop 需细分 mesh(P2 defer)
    }
    style.background_gradient = Some(Gradient2 {
        color_a: stops[0],
        color_b: stops[1],
        dir: dir.unwrap_or(GradientDir::ToBottom),
        angle,
    });
    true
}

/// 顶层逗号 split(括号深度感知,不切 rgba()/嵌套 gradient() 内部逗号)。
fn split_commas_top(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}
```

- [ ] **Step 5: 更新调用点(parse_linear_gradient_2 → parse_linear_gradient)**

`crates/core/src/style/mapping.rs:719` 与 `:733` 两处 `parse_linear_gradient_2(style, rest)` 改为 `parse_linear_gradient(style, rest)`。

- [ ] **Step 6: gradient_corner_colors 加 CSS 渐变线角度算法**

`crates/core/src/render/mod.rs:60-75` 替换 `gradient_corner_colors`(签名加 `rw, rh`,因角度算法需 rect 尺寸):
```rust
/// 背景渐变 4 角色(TL,TR,BR,BL)。2 色 linear = affine 色场,4 角顶点色双线性插值正确。
/// - angle=None:4 正向(查表,向后兼容)。
/// - angle=Some(deg):CSS 渐变线算法——方向向量 (sin,−cos)(屏幕 y-down),
///   渐变线半长 |w/2·dx|+|h/2·dy|,每角 t = 投影/半长 ∈[0,1],lerp(color_a,color_b,t)。
fn gradient_corner_colors(
    g: crate::style::resolved::Gradient2,
    rw: f32,
    rh: f32,
) -> [[f32; 4]; 4] {
    let (a, b) = (g.color_a, g.color_b);
    let (dx, dy) = match g.angle {
        Some(deg) => {
            let r = deg.to_radians();
            (r.sin(), -r.cos())
        }
        None => {
            use crate::style::resolved::GradientDir as G;
            match g.dir {
                G::ToRight => (1.0, 0.0),
                G::ToLeft => (-1.0, 0.0),
                G::ToTop => (0.0, -1.0),
                G::ToBottom => (0.0, 1.0),
            }
        }
    };
    let half = (rw / 2.0 * dx.abs() + rh / 2.0 * dy.abs()).max(1e-6);
    let lerp = |t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ]
    };
    let corners = [
        (-rw / 2.0, -rh / 2.0), // TL
        (rw / 2.0, -rh / 2.0),  // TR
        (rw / 2.0, rh / 2.0),   // BR
        (-rw / 2.0, rh / 2.0),  // BL
    ];
    let mut out = [[0.0f32; 4]; 4];
    for (i, &(px, py)) in corners.iter().enumerate() {
        let t = ((px * dx + py * dy) / half * 0.5 + 0.5).clamp(0.0, 1.0);
        out[i] = lerp(t);
    }
    out
}
```

- [ ] **Step 7: 更新 gradient_corner_colors 调用点(传 rw,rh)**

`crates/core/src/render/mod.rs:360-367`(use_gradient 分支)原:
```rust
let g = n.style.background_gradient.expect("use_gradient 已校验");
crate::render::mesh::quad_gradient(
    &draw_rect,
    gradient_corner_colors(g),
    ...
)
```
改为:
```rust
let g = n.style.background_gradient.expect("use_gradient 已校验");
crate::render::mesh::quad_gradient(
    &draw_rect,
    gradient_corner_colors(g, draw_rect.w, draw_rect.h),
    ...
)
```

- [ ] **Step 8: gradient_glyph_colors angle fallback(text 渐变角度 defer)**

`crates/core/src/render/mod.rs:81-123` 的 `gradient_glyph_colors` 现按 `g.dir` 查表。在函数开头加 fallback:
```rust
fn gradient_glyph_colors(
    g: &crate::style::resolved::Gradient2,
    glyph_x: f32,
    glyph_advance: f32,
    line_width: f32,
    line_y: f32,
    line_height: f32,
    text_height: f32,
) -> [[f32; 4]; 4] {
    // ponytail: text 渐变角度(background-clip:text + 45deg)defer——斜角度 per-glyph
    // 投影需 text 块整体尺寸 + 多行处理,游戏 UI 渐变字通常水平/垂直。angle 时退化为
    // 最近 4 正向(按角度量化),视觉近似。升级路径:per-glyph 渐变线投影。
    let g = if g.angle.is_some() {
        let mut g = *g;
        g.dir = quantize_dir(g.angle.unwrap());
        g.angle = None;
        g
    } else {
        *g
    };
    // ... 原 match g.dir 逻辑不变
}
```
加 helper(在 gradient_glyph_colors 上方):
```rust
/// 角度量化到最近 4 正向(text 渐变 angle fallback 用)。
fn quantize_dir(deg: f32) -> crate::style::resolved::GradientDir {
    use crate::style::resolved::GradientDir as G;
    let d = ((deg % 360.0) + 360.0) % 360.0;
    match d {
        45.0..135.0 => G::ToRight,
        135.0..225.0 => G::ToBottom,
        225.0..315.0 => G::ToLeft,
        _ => G::ToTop,
    }
}
```

- [ ] **Step 9: pkg bump 21→22(Gradient2 serde 字段变)**

`crates/core/src/asset/mod.rs:20-22`:
```rust
pub const PKG_FORMAT_VERSION: u32 = 22;
pub const MIN_PKG_FORMAT_VERSION: u32 = 22;
pub const MAX_PKG_FORMAT_VERSION: u32 = 22;
```

- [ ] **Step 10: 跑解析 + round-trip + render test**

Run:
```bash
cargo test -p loomgui_core style::mapping::tests::parse_linear_gradient
cargo test -p loomgui_core style::resolved::tests::resolved_style_bincode_roundtrip
cargo test -p loomgui_core render::tests
```
Expected: PASS(angle 解析、bincode 含 angle 字段、现有 gradient render 测试不破)。
> 若 `resolved_style_bincode_roundtrip_preserves_all_fields` 未覆盖 `Gradient2.angle`,在该测试补 `angle: Some(45.0)` 断言(防字段漏序列化)。

- [ ] **Step 11: 重打所有工作区包 + 重编 dll + GUI exe(serde 变闭环)**

```bash
# 重编核心 + ffi
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
# 重打所有工作区包(showcase/spec4b 等,改 parse-time 必重打)
cargo run -p loomgui_pkg -- build showcase
# GUI exe 绑 fence crate(fence 若改了 schema);Task 3 未改 fence 可跳过,但 schema css.rs 描述在 Task 4 改 → Task 4 后重出 exe
```
> 验证 showcase 打包 exit 0、pkg.bin 可被 dump_*.rs 读(格式 22)。

- [ ] **Step 12: fmt + clippy + 全 workspace test + fence**

Run:
```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo test -p loomgui_fence
```
Expected: 全 PASS。

- [ ] **Step 13: Commit**

```bash
git add crates/core/src/style/resolved.rs crates/core/src/style/mapping.rs crates/core/src/render/mod.rs crates/core/src/asset/mod.rs crates/core/src/style/mapping/tests.rs crates/core/src/render/tests.rs unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "feat(style): linear-gradient arbitrary angle (Gradient2.angle) + pkg bump 21->22"
```

---

### Task 4: 围栏镜像 + v1.8 spec 修订(linear 角度进围栏)

**Files:**
- Modify: `crates/fence/src/schema/css.rs`(`Gradient2` parser 注释/描述)
- Modify: `docs/design/fence.md`(Gradient2 语法描述)
- Modify: `docs/superpowers/specs/2026-07-08-v1.8-text-effects-decoration-design.md`(line 60/98 修订)
- Test: `crates/fence` 现有 schema 测试

- [ ] **Step 1: fence.md 更新 Gradient2 语法**

`docs/design/fence.md` 找 `Gradient2` 行(§5.2 表,~line 250),原 `linear-gradient(to dir, hex, hex)` 改为:
```
| `Gradient2` | `linear-gradient(to <right\|left\|top\|bottom>, hex, hex)` 或 `linear-gradient(<deg>, hex, hex)` |
```
加一行注:`<deg>` = CSS 角度(0=to top,90=to right)。多 stop / corner 方向 / radial / conic 仍围栏外。

- [ ] **Step 2: schema css.rs 更新 Gradient2 parser 描述(若有 doc)**

`crates/fence/src/schema/css.rs:40`(`Gradient2` 枚举变体)补注释(若变体有 doc):
```rust
/// `linear-gradient(to <dir>, hex, hex)` 或 `linear-gradient(<deg>, hex, hex)`。
/// 2 色 + 4 正向或任意角度。多 stop / corner / radial / conic 围栏外。
Gradient2,
```

- [ ] **Step 3: v1.8 spec 修订(linear 角度移出围栏外)**

`docs/superpowers/specs/2026-07-08-v1.8-text-effects-decoration-design.md`:
- line ~60(§2.4"只支持 2 色 + 4 方向"):段末加注 `> P2(2026-07-21)扩展:2 色 + 任意角度(<deg>)已支持,见 phase1-p2 plan。多 stop / corner 仍 defer。`
- line ~92(围栏外静默忽略列表):若有 `linear-gradient 之外的渐变函数`,确认不含 linear 角度(angle 现已支持)。
- line ~98(编译期报错列表):`linear-gradient 多 stop 或斜角度` → 改为 `linear-gradient 多 stop(>2 色)或 corner 方向(to top right)`(斜角度 <deg> 已支持)。

- [ ] **Step 4: 加 fence schema 测试(angle 语法合法)**

`crates/fence` schema 测试(找现有 gradient 测试位置)加:
```rust
// linear-gradient(45deg, hex, hex) 围栏内合法
// linear-gradient(to right, hex, hex, hex) 多 stop 围栏外报错/静默(以现有 fence 行为为准)
```
> fence 对 `Gradient2` parser 的校验逻辑(若 `css_resolve.rs` 有 validator):确认 `<deg>` 形态不被拒。若 fence 无 Gradient2 validator(运行期 mapping.rs sniff),本步主要是 doc 同步,schema 测试可选。

- [ ] **Step 5: GUI exe 重出(fence 改动闭环)**

```bash
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)
cp crates/packer/gui/src-tauri/target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```
(GUI exe 静态链 fence crate,fence doc/schema 改后重出。若 Task 4 只改 doc 未改 fence 逻辑,exe 行为不变,但 pkg bump 时 stale exe 会误报,一并重出。)

- [ ] **Step 6: 跑 fence 测试 + fmt + clippy**

Run: `cargo test -p loomgui_fence && cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/fence/src/schema/css.rs docs/design/fence.md docs/superpowers/specs/2026-07-08-v1.8-text-effects-decoration-design.md unity/package/Editor/Tools/loomgui_gui.exe
git commit -m "docs(fence): linear-gradient <deg> angle enters fence subset (P2); revise v1.8 spec"
```

---

### Task 5: 验收页 + HeadlessTests(C# headless 验收)

**Files:**
- Create: `showcase/spec4b/p2-visual-acceptance.html`(圆角 border + 角度渐变最小例)
- Create: `tests/dotnet/LoomGUI.HeadlessTests/VisualDecorationTests.cs`(C# P/Invoke 断言)

**验收分层(诚实)**:
- C# HeadlessTests 验:computed style 进了字段(border-radius、background_gradient.angle)、layout rect。
- Rust render/tests.rs(已 Task 2/3 加):圆角 border 顶点、角度渐变 4 角色。
- 视觉(圆角/渐变好不好看):浏览器打开验收 HTML 人工对比(双方按标准 CSS 渲染,对得上 = 对)。Unity 视觉验收推后(家里机)。

- [ ] **Step 1: 写验收 HTML(浏览器 + HeadlessTests 共用)**

`showcase/spec4b/p2-visual-acceptance.html`(仿 `p1-block-acceptance.html` 极简):
```html
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>
  body { margin: 0; }
  .rounded-border {
    width: 200px; height: 120px;
    border: 6px solid #ff0000;
    border-radius: 24px;
    margin: 20px;
  }
  .angle-gradient {
    width: 200px; height: 120px;
    margin: 20px;
    background: linear-gradient(45deg, #ff0000, #0000ff);
  }
</style></head>
<body>
  <div class="rounded-border" id="rb"></div>
  <div class="angle-gradient" id="ag"></div>
</body>
</html>
```

- [ ] **Step 2: 重打验收页 pkg.bin**

```bash
# 把 p2-visual-acceptance.html 放进 showcase/spec4b 工作区,重打
cargo run -p loomgui_pkg -- build showcase/spec4b
# 把产出的 p2-visual-acceptance.pkg.bin 复制到 HeadlessTests fixtures(仿 p1-block.pkg.bin)
cp <output>/p2-visual-acceptance.pkg.bin tests/dotnet/LoomGUI.HeadlessTests/fixtures/ 2>$null
```
> 路径以 showcase 工作区布局为准(仿 P1 `p1-block.pkg.bin` 的产出与复制流程)。

- [ ] **Step 3: 写 HeadlessTests(断言 computed style)**

`tests/dotnet/LoomGUI.HeadlessTests/VisualDecorationTests.cs`(仿 `BlockLayoutTests.cs` 极简):
```csharp
using Xunit;
using LoomGUI.HeadlessTests.Harness;

public class VisualDecorationTests
{
    [Fact]
    public void BorderRadiusAndAngleGradient_EnterComputedStyle()
    {
        var (stage, ctx) = StageHarness.Create();
        try
        {
            // 加载 p2 验收 pkg + 字体 + Instantiate(仿 AcceptanceGateTests.InstantiateFixture)
            // ... 注册 DejaVuSans.ttf → LoadPackage → Instantiate("p2-visual-acceptance")
            // Tick 一次
            Native.loomgui_stage_tick(stage, 0.016f);

            // 圆角 border + 角度渐变进 computed style(以 ComputedNodeStyleRepr 暴露字段为准;
            // 若 border_radius/background_gradient 未暴露,此断言降级为"节点存在 + layout rect 非零")
            // 例:var rb = root.Get<Container>("rb");
            //     Assert.True(rb.Geometry.LayoutRect.Width > 0);
        }
        finally { StageHarness.Destroy(stage); }
    }
}
```
> 实现时先查 `ComputedNodeStyleRepr` 是否暴露 `border_radius` / `background_gradient`(勘察报告:当前 `ComputedNodeStyle` 仅 13 字段,不含这两项)。**若未暴露**:本 test 降级为"pkg 加载 + Instantiate + Tick 不 panic + layout rect 非零"(冒烟级),圆角/渐变的语义正确性靠 Rust render/tests.rs(已 Task 2/3 覆盖)。在 test 注释里标明这个分层。

- [ ] **Step 4: 跑 dotnet test**

Run(在 `tests/dotnet/LoomGUI.HeadlessTests/`):`dotnet test --filter "VisualDecoration"`
Expected: PASS。

- [ ] **Step 5: 浏览器人工对比(视觉验收)**

打开 `showcase/spec4b/p2-visual-acceptance.html` 于 Chrome:确认圆角 border(红框圆角,边角不突出)、45deg 渐变(左上红→右下蓝斜向)。LoomGUI 渲染(dump 或家里机 Unity)应一致。

- [ ] **Step 6: Commit**

```bash
git add showcase/spec4b/p2-visual-acceptance.html tests/dotnet/LoomGUI.HeadlessTests/VisualDecorationTests.cs tests/dotnet/LoomGUI.HeadlessTests/fixtures/p2-visual-acceptance.pkg.bin
git commit -m "test(p2): visual decoration acceptance (rounded border + angle gradient) headless"
```

---

## Deferred(本 plan 不做,roadmap 已标注)

| 项 | 原因 | 处置路标 |
|---|---|---|
| **G 多层 background** | 完全单层,多层=大重构(resolved+Vec/mapping/render/fence 6 文件 + 实现死枚举 `BackgroundShorthand` + 围栏修 `url(a),url(b)` 静默损坏 + 周期扩展算法)。游戏 UI 99% 单层 bg。blitz 靠 stylo 周期扩展,LoomGUI 手搓 | §4 视觉束 |
| **linear 多 stop(>2 色)** | 4 角顶点色双线性插值是 affine 色场,表达不了 stop 分段(例:red/yellow/blue 沿 to right,中间应 yellow,双线性给紫色)。需细分 mesh(沿渐变线切 N 段 sub-quad),angle+多 stop 组合还要斜细分 | follow-up plan(细分 mesh)+ §4 视觉束 |
| **linear corner 方向(`to top right`)** | 需按 rect 长宽比算渐变线角度(blitz `LineDirection::Corner`),比纯 `<deg>` 复杂,游戏 UI 少用 | §4 视觉束 |
| **text 渐变角度(`background-clip:text` + `45deg`)** | `gradient_glyph_colors` per-glyph 斜角度投影需 text 块整体尺寸 + 多行处理。本 plan fallback 到最近 4 正向 | §4 视觉束 |
| **radial / conic 渐变** | per-vertex 做不了(`\|p-c\|`/`atan2` 非 affine),要新 Unity shader program=5 + 1D LUT 纹理 + 后端同步,编码机验不了 shader(家里机串行)。v1.8 spec line 60 明文 defer | §4 视觉束(真有圆环 cooldown/光晕需求时) |
| **J ex/ch/ic 字体单位** | parse_lp 连 em/rem 都没支持;游戏 UI 不用;showcase 零使用。代价高(签名连锁 + FontTable 上下文 + ex fallback) | §4 或 YAGNI 永久 defer |

---

## Self-Review

**1. Spec 覆盖**:
- spec §5.3(A 圆角边框)→ Task 1+2(border_ring 消费 radii + box_shadow_quad)。✅
- spec §5.5(I linear)→ Task 3(任意角度 2 色)。✅ spec §5.5 提到的 radial/conic → Deferred(v1.8 已 defer,本 plan 维持)。
- spec §5.3 提"与 v1.2 旧算法关系待 plan 定"→ Task 2 决策:用 blitz 几何模型重写(内半径=insets、内外同分段 infill),不迁移 v1.2 旧 border 算法(v1.2 是背景 rounded_rect,保留;border_ring 是新增圆角)。✅
- spec §5.4(G)/§5.6(J)→ Deferred(roadmap 已标)。✅

**2. 占位符扫描**:Task 5 Step 2 pkg 路径、Step 3 ComputedNodeStyleRepr 字段标了"以实际为准/若未暴露降级"——这是诚实标注(ComputedNodeStyle 当前 13 字段不含 border_radius/background_gradient,勘察已证),非占位。其余 step 均有完整代码。

**3. 类型一致性**:
- `Gradient2` 字段:`color_a`/`color_b`/`dir`/`angle`——Task 3 定义,Task 3 Step 3 构造、Step 6/8 消费一致。✅
- `gradient_corner_colors` 签名 Task 3 Step 6 改 `(g, rw, rh)`、Step 7 调用点同步。✅
- `radius_scale`/`corner_arc_pts` Task 1 定义 pub、Task 2 border_ring 消费。✅
- `PKG_FORMAT_VERSION` 21→22 Task 3 Step 9。✅

**4. 风险**:
- **border_ring 圆角几何正确性**:内外同分段 infill 扇模型是推导的,Task 2 Step 3 的端到端测 + 浏览器人工对比(Task 5 Step 5)兜底。若 infill 扇有视觉瑕疵(角内镂空),补 corner_needs_infill 显式填充(blitz `css_box.rs:353-372`)。
- **pkg bump 闭环**:Task 3 改 serde 必重打所有包(Step 11),漏打 → HeadlessTests 加载旧 pkg 读 angle 字段错位。round-trip test(Step 10)自验字段完整性。
- **gradient + border-radius 共存**:`use_gradient` 门(mod.rs:343)仍要求 `all_zero`(gradient 不配圆角),本 plan 不解开(解开需 rounded_rect 支持 4 角色顶点,额外工作)。linear 角度 + 圆角容器 = 渐变退直角 quad(现状)。标 Deferred。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-21-phase1-p2-visual-decoration.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
