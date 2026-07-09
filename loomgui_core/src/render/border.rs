//! 彩色边框：外轮廓减内轮廓环形三角带。无背景图时拼进 Container/Button 背景同一
//! Mesh payload（program=0 顶点色，单 draw call），边框三角序在背景之后——重叠的边框
//! 环区边框覆盖背景，内部仅背景。v1.8 修 border_color 死字段（resolved.rs 存了 render 零引用）。

use crate::scene::node::Rect;

/// 生成彩色边框 mesh：外轮廓（rect + radii）减内轮廓（向内缩 width）的环形三角带。
///
/// - radii = [TL, TR, BR, BL]，每角 (h, v) 像素半径（与 mesh::rounded_rect 同约定）。
/// - width > 0 才生成；width ≤ 0 或 rect 退化 → 返空四表。
/// - 返 SOA 四表 (verts, uvs, colors, indices)，与 mesh::quad 同形，uvs 全 0（纯色，
///   不采样纹理）。
pub fn border_ring(
    rect: &Rect,
    radii: &[(f32, f32); 4],
    width: f32,
    color: [f32; 4],
) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>) {
    if width <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    // 钳到短边一半，防内轮廓交叉（width > 短边/2 时内轮廓翻转产生负面积）。
    let w = width.min(rect.w * 0.5).min(rect.h * 0.5);
    let (x, y, rw, rh) = (rect.x, rect.y, rect.w, rect.h);
    // ponytail: 圆角边框分段留 Task 5 圆角 SDF 一起补；此处有 radius 时按直角退化。
    // 升级路径：外/内轮廓各走 rounded_rect 圆弧顶点，环形带连两轮廓。
    let _ = radii;

    // 外轮廓 4 角（TL, TR, BR, BL），内轮廓 4 角（同序缩进 w）。
    let outer = [[x, y], [x + rw, y], [x + rw, y + rh], [x, y + rh]];
    let inner = [
        [x + w, y + w],
        [x + rw - w, y + w],
        [x + rw - w, y + rh - w],
        [x + w, y + rh - w],
    ];
    let mut verts = Vec::with_capacity(8);
    verts.extend_from_slice(&outer);
    verts.extend_from_slice(&inner);
    let uvs = vec![[0.0, 0.0]; 8]; // 纯色，不采样纹理
    let colors = vec![color; 8];
    // 每边 2 三角连 outer[i]/outer[i+1]/inner[i+1]/inner[i]（环形带）。
    let mut indices = Vec::with_capacity(24);
    for i in 0..4 {
        let ni = (i + 1) % 4;
        let (oi, oni) = (i as u32, ni as u32);
        let (ii, ini) = ((i + 4) as u32, (ni + 4) as u32);
        indices.extend_from_slice(&[oi, oni, ini, oi, ini, ii]);
    }
    (verts, uvs, colors, indices)
}

/// box-shadow 几何近似：比 rect 外扩 spread 的圆角 quad（直角退化），四周顶点 alpha 渐隐。
/// 无 blur（真实 blur 需离屏 RT，排 v1.14+）。独立 RenderNode 画在节点下层。
/// ponytail: 圆角阴影随圆角 SDF task 补，先直角外扩 quad；渐隐带 PlayMode 调参后补中间顶点。
pub fn box_shadow_quad(
    rect: &Rect,
    radii: &[(f32, f32); 4],
    spread: f32,
    color: [f32; 4],
) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>) {
    let _ = radii; // ponytail: 圆角阴影随圆角 SDF task 补，先直角
    let outer = Rect {
        x: rect.x - spread,
        y: rect.y - spread,
        w: rect.w + 2.0 * spread,
        h: rect.h + 2.0 * spread,
    };
    if outer.w <= 0.0 || outer.h <= 0.0 {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }
    crate::render::mesh::quad(&outer, color, [0.0, 0.0], [0.0, 0.0])
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
        let (v, _u, _c, i) = border_ring(&r, &radii, 0.0, [1.0; 4]);
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
        let (verts, _uvs, colors, indices) = border_ring(&r, &radii, 5.0, [1.0, 0.0, 0.0, 1.0]);
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
        let (verts, _u, _c, _i) = border_ring(&r, &radii, 5.0, [1.0; 4]);
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
        let (v, _u, _c, i) = border_ring(&r, &radii, 5.0, [1.0; 4]);
        assert!(v.is_empty() && i.is_empty(), "退化 rect 不生成边框");
    }

    #[test]
    fn border_ring_width_clamped_to_half_rect() {
        // width > rect 短边一半时钳到一半，防内轮廓交叉（100×50，width=200 → 钳到 25）。
        // 内轮廓 x = [25, 75]，y = [25, 25]（h=50 → 内轮廓高 0，但仍发 8 顶点不交叉）。
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let radii = [(0.0, 0.0); 4];
        let (verts, _u, _c, _i) = border_ring(&r, &radii, 200.0, [1.0; 4]);
        // 内轮廓 x 含 25/75（钳后 w=25），不含负数或越界值。
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        assert!(xs.contains(&25.0), "width 钳到 25 → 内轮廓 x=25");
        assert!(xs.contains(&75.0), "内轮廓 x=75");
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
        let (_v, uvs, _c, _i) = border_ring(&r, &radii, 5.0, [1.0; 4]);
        assert!(uvs.iter().all(|uv| *uv == [0.0, 0.0]), "UV 全 0");
    }

    #[test]
    fn box_shadow_spreads_outward() {
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
}
