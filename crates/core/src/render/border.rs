//! 彩色边框：外轮廓减内轮廓环形三角带。无背景图时拼进 Container/Button 背景同一
//! Mesh payload（program=0 顶点色，单 draw call），边框三角序在背景之后——重叠的边框
//! 环区边框覆盖背景，内部仅背景。v1.8 修 border_color 死字段（resolved.rs 存了 render 零引用）。

use crate::scene::node::Rect;

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
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

/// 生成彩色圆角边框 mesh：外轮廓（外圆角矩形周边）减内轮廓（内圆角矩形周边）的环形三角带。
///
/// - 外半径经 CSS 邻角缩放（与 `mesh::rounded_rect` 共用 `radius_scale`），保证背景圆角
///   与边框圆角视觉一致。
/// - 内半径 = 外半径 − 邻边 insets（per-corner per-axis，钳 0）；内角圆心 = 外角圆心
///   （因 `inner_corner + inner_radius = (rect 角 + inset) + (外半径 − inset) = rect 角 + 外半径
///   = 外圆心`）。
/// - 内外角**同分段** `max(seg_outer, seg_inner, 2)`：内角直角（内半径≤0）时
///   `corner_arc_pts` 产 seg+1 个 corner 重复点，与外角弧顶点 1:1 配对 → 角内自动形成
///   infill 扇（外圆内方）。
/// - 环带三角：每对邻顶点 2 三角 `[外i, 外i+1, 内i+1] + [外i, 内i+1, 内i]`；零宽边处
///   内外重合 → 退化三角（GPU 免费，不另跳过）。
/// - per-axis 比例钳制（CSS 浏览器语义）：对边和超过 rect 尺寸时等比缩，防内轮廓交叉。
/// - 返 SOA 四表（verts/uvs/colors/indices），uvs 全 0（纯色不采样）。
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
    // per-axis width 钳制：对边和 > 尺寸等比缩（只缩不放）
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
    // 外半径 CSS 邻角缩放（与 rounded_rect 共用 → 背景圆角与边框圆角一致）
    let scale = crate::render::mesh::radius_scale(radii, rw, rh);
    let sr = |rad: (f32, f32)| ((rad.0 * scale).max(0.0), (rad.1 * scale).max(0.0));
    let (tl, tr, br, bl) = (sr(radii[0]), sr(radii[1]), sr(radii[2]), sr(radii[3]));

    // 直角 fast-path：四角全 0 → 8 顶点矩形环（无 per-corner 弧细分，零宽边跳过发三角）。
    // 走原 sharp 路径保 backward-compat（build_container_with_border_emits_border_node 等
    // 现有测试约定 sharp = 8 顶点 + 24 索引 / 零宽边跳过）。
    let all_sharp = [tl, tr, br, bl]
        .iter()
        .all(|&(rx, ry)| rx <= 0.0 || ry <= 0.0);
    if all_sharp {
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
        // 每边 2 三角；width>0 才发（顶点固定 8）。序 [top, right, bottom, left] 与
        // outer/inner 的 TL,TR,BR,BL 角序对齐：边 i 连角 i 与下一角 (i+1)%4。
        let widths_arr = [t, r, b, l];
        let mut indices = Vec::with_capacity(24);
        for (i, &w) in widths_arr.iter().enumerate() {
            if w <= 0.0 {
                continue;
            }
            let ni = (i + 1) % 4;
            let (oi, oni) = (i as u32, ni as u32);
            let (ii, ini) = ((i + 4) as u32, (ni + 4) as u32);
            indices.extend_from_slice(&[oi, oni, ini, oi, ini, ii]);
        }
        return (verts, uvs, colors, indices);
    }

    // 内半径 = 外半径 − 邻边 insets（per-corner per-axis，钳 0）。内角直角(≤0)时外圆内方。
    let itl = ((tl.0 - l).max(0.0), (tl.1 - t).max(0.0));
    let itr = ((tr.0 - r).max(0.0), (tr.1 - t).max(0.0));
    let ibr = ((br.0 - r).max(0.0), (br.1 - b).max(0.0));
    let ibl = ((bl.0 - l).max(0.0), (bl.1 - b).max(0.0));

    use std::f32::consts::{FRAC_PI_2, PI};
    // 外角 (rx, ry, center, start, corner)；内角 center 复用外角 center（见上方几何推导）
    let outer_cfg: [(f32, f32, [f32; 2], f32, [f32; 2]); 4] = [
        (tl.0, tl.1, [x + tl.0, y + tl.1], PI, [x, y]),
        (
            tr.0,
            tr.1,
            [x + rw - tr.0, y + tr.1],
            -FRAC_PI_2,
            [x + rw, y],
        ),
        (
            br.0,
            br.1,
            [x + rw - br.0, y + rh - br.1],
            0.0,
            [x + rw, y + rh],
        ),
        (
            bl.0,
            bl.1,
            [x + bl.0, y + rh - bl.1],
            FRAC_PI_2,
            [x, y + rh],
        ),
    ];
    // 内角 corner = rect 角 + (inset_x, inset_y)；center 复用外角 center
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
        // 内外同分段：外圆内方时内角直角，corner_arc_pts 产 seg+1 个内角点 → infill 扇
        let seg = seg_of(orx, ory).max(seg_of(irx, iry)).max(2);
        outer_pts.extend(crate::render::mesh::corner_arc_pts(
            ocorner, orx, ory, oc, os, seg,
        ));
        inner_pts.extend(crate::render::mesh::corner_arc_pts(
            inner_corner[i],
            irx,
            iry,
            oc,
            os,
            seg,
        ));
    }
    let n = outer_pts.len();
    debug_assert_eq!(n, inner_pts.len(), "内外轮廓等长(同分段)");
    let n_u32 = n as u32;
    let mut verts = Vec::with_capacity(2 * n);
    verts.extend_from_slice(&outer_pts);
    verts.extend_from_slice(&inner_pts);
    let uvs = vec![[0.0, 0.0]; 2 * n];
    let colors = vec![color; 2 * n];
    // 环带三角带：每对邻顶点 2 三角。零宽边处内外重合 → 退化三角(GPU 免费，不另跳过)。
    let mut indices: Vec<u32> = Vec::with_capacity(6 * n);
    for i in 0..n_u32 {
        let ni = if i + 1 < n_u32 { i + 1 } else { 0 };
        indices.extend_from_slice(&[i, ni, n_u32 + ni, i, n_u32 + ni, n_u32 + i]);
    }
    (verts, uvs, colors, indices)
}

/// box-shadow 几何近似：比 rect 外扩 spread 的圆角矩形（CSS box-shadow：每角半径 + spread）。
/// 无 blur（真实 blur 需离屏 RT，排 v1.14+）。独立 RenderNode 画在节点下层。
/// ponytail: 软边 alpha 衰减未实现 — 当前产纯色圆角 quad（rounded_rect 顶点同色），
///   无边缘 alpha falloff。升级路径：外 rect + inset vertex ring 带 alpha falloff →
///   v1.14+ 离屏 RT。
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
    // 圆角随 spread 外扩（CSS box-shadow：每角半径 + spread）。rounded_rect 内部再 CSS 缩放。
    let spread_radii = [
        (radii[0].0 + spread, radii[0].1 + spread),
        (radii[1].0 + spread, radii[1].1 + spread),
        (radii[2].0 + spread, radii[2].1 + spread),
        (radii[3].0 + spread, radii[3].1 + spread),
    ];
    crate::render::mesh::rounded_rect(&outer, color, &spread_radii, [0.0, 0.0], [0.0, 0.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_ring_zero_width_empty() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (v, _u, _c, i) = border_ring(&r, &radii, BorderWidths::default(), [1.0; 4]);
        assert!(v.is_empty() && i.is_empty(), "width=0 不生成边框");
    }

    #[test]
    fn border_ring_rect_has_outer_and_inner_loops() {
        // 直角矩形，width=5：外轮廓 4 角 + 内轮廓 4 角 = 8 顶点，
        // 环形三角带连外内 = 每边 2 三角 = 8 三角 = 24 索引。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (verts, _uvs, colors, indices) =
            border_ring(&r, &radii, BorderWidths::all(5.0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(verts.len(), 8, "直角矩形 4 外 + 4 内角 = 8 顶点");
        assert_eq!(indices.len(), 24, "4 边 × 2 三角 × 3 索引 = 24");
        assert!(
            colors.iter().all(|c| *c == [1.0, 0.0, 0.0, 1.0]),
            "全顶点边框色"
        );
    }

    #[test]
    fn border_ring_inner_loop_inset_by_width() {
        // 外角 (0,0)/(100,0)/(100,50)/(0,50)，width=5 → 内角 (5,5)/(95,5)/(95,45)/(5,45)
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (verts, _u, _c, _i) = border_ring(&r, &radii, BorderWidths::all(5.0), [1.0; 4]);
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        assert!(xs.contains(&5.0) && xs.contains(&95.0), "内轮廓 x 缩进 5");
    }

    #[test]
    fn border_ring_degenerate_rect_empty() {
        // 退化 rect（w=0）→ 空输出，不 panic。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (v, _u, _c, i) = border_ring(&r, &radii, BorderWidths::all(5.0), [1.0; 4]);
        assert!(v.is_empty() && i.is_empty(), "退化 rect 不生成边框");
    }

    #[test]
    fn border_ring_width_clamped_per_axis() {
        // 四边同值超尺寸：100×50 rect，all(200) → x 方向 left+right=400>100 缩到 50+50，
        // y 方向 top+bottom=400>50 缩到 25+25。内轮廓不交叉、不越界。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (verts, _u, _c, _i) = border_ring(&r, &radii, BorderWidths::all(200.0), [1.0; 4]);
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        let ys: Vec<f32> = verts.iter().map(|v| v[1]).collect();
        assert!(xs.contains(&50.0), "x 钳后 left=right=50 → inner x=50");
        assert!(ys.contains(&25.0), "y 钳后 top=bottom=25 → inner y=25");
        assert!(verts
            .iter()
            .all(|v| { (0.0..=100.0).contains(&v[0]) && (0.0..=50.0).contains(&v[1]) }));
    }

    #[test]
    fn border_ring_uvs_all_zero() {
        // 纯色边框不采样纹理，UV 全 0（program=0 顶点色 × 白 1×1 纹理 = 顶点色）。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (_v, uvs, _c, _i) = border_ring(&r, &radii, BorderWidths::all(5.0), [1.0; 4]);
        assert!(uvs.iter().all(|uv| *uv == [0.0, 0.0]), "UV 全 0");
    }

    #[test]
    fn box_shadow_spreads_outward() {
        // spread 生效：ox/oy 偏移由 caller 负责（offset rect 后再传 box_shadow_quad）。
        let r = Rect {
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 40.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (verts, _uvs, colors, _idx) = box_shadow_quad(&r, &radii, 5.0, [0.0, 0.0, 0.0, 0.5]);
        // 外扩 spread=5：角从 (10,10)/(90,10)/(90,50)/(10,50) → (5,5)/(95,5)/(95,55)/(5,55)
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        assert!(xs.contains(&5.0) && xs.contains(&95.0), "外扩 spread");
        // 边缘顶点 alpha 渐隐（MVP 纯色外扩 quad）
        assert!(colors.iter().all(|c| c[3] == 0.5));
    }

    #[test]
    fn box_shadow_degenerate_empty() {
        // rect w≤0 且 spread=0 时 outer 退化 → 空输出。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (v, _u, _c, i) = box_shadow_quad(&r, &radii, 0.0, [1.0; 4]);
        assert!(v.is_empty() && i.is_empty(), "退化 rect → 空输出");
    }

    #[test]
    fn border_ring_single_side_bottom_only() {
        // border-bottom:1px：只底边有宽 → 只发底边 2 三角 = 6 索引；顶点固定 8（4 未引用）。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let widths = BorderWidths {
            top: 0.0,
            right: 0.0,
            bottom: 1.0,
            left: 0.0,
        };
        let (verts, _u, _c, idx) = border_ring(&r, &radii, widths, [1.0; 4]);
        assert_eq!(verts.len(), 8, "顶点固定 8");
        assert_eq!(idx.len(), 6, "只底边 2 三角 = 6 索引，得 {}", idx.len());
    }

    #[test]
    fn border_ring_asymmetric_four_sides() {
        // 四边各自宽度：内角 = (left, top) / (rw-right, top) / (rw-right, rh-bottom) / (left, rh-bottom)
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let widths = BorderWidths {
            top: 2.0,
            right: 3.0,
            bottom: 4.0,
            left: 5.0,
        };
        let (verts, _u, _c, idx) = border_ring(&r, &radii, widths, [1.0; 4]);
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        let ys: Vec<f32> = verts.iter().map(|v| v[1]).collect();
        // inner TL = (5, 2), inner TR = (97, 2), inner BR = (97, 46), inner BL = (5, 46)
        assert!(
            xs.contains(&5.0) && xs.contains(&97.0),
            "内轮廓 x = left/right 缩进"
        );
        assert!(
            ys.contains(&2.0) && ys.contains(&46.0),
            "内轮廓 y = top/bottom 缩进"
        );
        assert_eq!(idx.len(), 24, "四边全 >0 → 24 索引");
    }

    #[test]
    fn border_ring_asymmetric_winding_consistent_ccw() {
        // 非对称宽度四边全发：每边 2 三角 = 8 三角。每三角形有符号面积须全同号
        // （一致 CCW winding），否则后端 back-face cull 会漏绘某条带。
        fn signed_area(verts: &[[f32; 2]], a: u32, b: u32, c: u32) -> f32 {
            let pa = verts[a as usize];
            let pb = verts[b as usize];
            let pc = verts[c as usize];
            0.5 * ((pb[0] - pa[0]) * (pc[1] - pa[1]) - (pc[0] - pa[0]) * (pb[1] - pa[1]))
        }
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let widths = BorderWidths {
            top: 2.0,
            right: 3.0,
            bottom: 4.0,
            left: 5.0,
        };
        let (verts, _u, _c, idx) = border_ring(&r, &radii, widths, [1.0; 4]);
        assert_eq!(idx.len() % 3, 0, "索引数是 3 的倍数");
        let mut signs = Vec::new();
        for tri in (0..idx.len()).step_by(3) {
            let area = signed_area(&verts, idx[tri], idx[tri + 1], idx[tri + 2]);
            if area.abs() > 1e-6 {
                signs.push(area.signum());
            }
        }
        assert!(!signs.is_empty(), "非对称宽度应有非退化三角形");
        let first = signs[0];
        assert!(
            signs.iter().all(|&s| s == first),
            "所有非退化三角形有符号面积同号（一致 winding），got signs={:?}",
            signs
        );
    }

    #[test]
    fn border_ring_opposite_sides_exceed_width_clamped() {
        // left+right > rw → per-axis 比例缩（CSS 语义）。left=80,right=40,rw=100 → scale=100/120
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let widths = BorderWidths {
            top: 0.0,
            right: 40.0,
            bottom: 0.0,
            left: 80.0,
        };
        let (verts, _u, _c, _idx) = border_ring(&r, &radii, widths, [1.0; 4]);
        // 钳后 left≈66.67, right≈33.33；inner TL.x = x+left ≈ 66.67, inner TR.x = x+rw-right ≈ 66.67
        // → inner 宽 ≈ 0（塌缩），但不交叉（无负坐标越界）
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        assert!(
            xs.iter().all(|&x| (0.0..=100.0).contains(&x)),
            "钳制后坐标不越界"
        );
        assert!(
            (xs.iter().cloned().fold(f32::MAX, f32::min) - 0.0).abs() < 1e-3,
            "外轮廓 x=0 仍在"
        );
    }
}
