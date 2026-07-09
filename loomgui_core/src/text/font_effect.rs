//! 字形位图后处理（v1.8 FontEffect）：描边 / 发光 / blur / 阴影。
//!
//! 挂在 atlas `ensure` 内 `rasterize_glyph` 之后：按 `GlyphKey.effect_sig` 查 effect
//! 配置，对 R8 alpha 位图做 dilate/erode/gaussian_blur，输出可能扩边界的新位图。
//! atlas 按 (font, glyph_id, size_px, effect_sig) 分槽缓存——同字形不同 effect 自动
//! 分槽。effect_sig=0 表示无后处理（v1.6 既有路径，恒不走本模块）。
//!
//! 位图只存 alpha——颜色（Shadow/Stroke/Glow 的 color）不在此处理，由 build 期 vertex
//! color 提供（premultiplied alpha 约定）。color 仅参与 sig 区分，使同参数不同色的
//! 效果分槽（保证语义独立；alpha 位图复用是后续优化点）。
//!
//! 对标 RmlUi ConvolutionFilter（取 max=dilation / 累加 sum=高斯），但用 etagere 增量
//! atlas 规避其全量重建纹理的缺陷——effect 字形走同一条增量 allocate 路径，旧槽不 repack。

/// 单个文字效果配置。Copy：参数全为值类型，atlas effect 表按值存。
/// Serialize/Deserialize：text_effects 挂在 ResolvedStyle 上随 pkg.bin 序列化。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FontEffect {
    /// 阴影：build 期偏移 quad + 高斯 blur。位图层只 blur（offset 不进位图）。
    Shadow {
        ox: f32,
        oy: f32,
        blur: f32,
        color: [f32; 4],
    },
    /// 描边：内侧吃字（erode）。描边色填充 erode 后的边界环。
    Stroke { w: f32, color: [f32; 4] },
    /// 发光：dilate 膨胀亮边 + 高斯 blur 晕开。
    Glow { w: f32, color: [f32; 4] },
    /// 模糊：可分离高斯两 pass。
    Blur { w: f32 },
}

/// f32 不 impl Hash，逐分量按 to_bits 进 hash（与 effect_sig 内其它 f32 一致）。
fn hash_color<H: std::hash::Hasher>(h: &mut H, color: &[f32; 4]) {
    use std::hash::Hash;
    for c in color {
        c.to_bits().hash(h);
    }
}

/// 把一组 effect 配置 hash 成 64bit 指纹。same params → same sig；different → 极大概率不同。
/// 输入 = (discriminant, params)；per-size 不进 sig（size 已在 GlyphKey）。
/// f32 经 to_bits 进 hash（NaN 规范化由调用方保证；DSL 层不产生 NaN）。
pub fn effect_sig(effects: &[FontEffect]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for e in effects {
        std::mem::discriminant(e).hash(&mut h);
        match e {
            FontEffect::Shadow {
                ox,
                oy,
                blur,
                color,
            } => {
                ox.to_bits().hash(&mut h);
                oy.to_bits().hash(&mut h);
                blur.to_bits().hash(&mut h);
                hash_color(&mut h, color);
            }
            FontEffect::Stroke { w, color } => {
                w.to_bits().hash(&mut h);
                hash_color(&mut h, color);
            }
            FontEffect::Glow { w, color } => {
                w.to_bits().hash(&mut h);
                hash_color(&mut h, color);
            }
            FontEffect::Blur { w } => {
                w.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

/// 圆形 dilation：每个输出像素 = 邻域（radius 圆内）alpha 的 max。向外扩边界 ceil(radius)。
/// glow（膨胀亮边）用。
pub fn dilate(p: &[u8], w: u32, h: u32, radius: f32) -> (Vec<u8>, u32, u32) {
    convolve(p, w, h, radius, true)
}

/// 圆形 erosion：每个输出像素 = 邻域 alpha 的 min。画布同样扩边界（语义是亮区向内缩）。
/// stroke 内侧吃字用（dilation 反向）。
pub fn erode(p: &[u8], w: u32, h: u32, radius: f32) -> (Vec<u8>, u32, u32) {
    convolve(p, w, h, radius, false)
}

/// 描边环 = 原字形 - erode（内缩）：原亮区里被 erode 吃掉的边缘 r 宽像素 = 描边
/// （CSS -webkit-text-stroke 描边在字形边缘内侧）。erode 扩边界 ceil(radius)，原 p 在
/// eroded 位图的 (r, r) offset；环写在同尺寸（扩边界）位图，build 期填描边色。
fn stroke_ring(p: &[u8], w: u32, h: u32, radius: f32) -> (Vec<u8>, u32, u32) {
    let r = radius.ceil() as i32;
    if r <= 0 {
        return (p.to_vec(), w, h);
    }
    let (eroded, ew, eh) = erode(p, w, h, radius);
    let mut ring = vec![0u8; (ew * eh) as usize];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let orig = p[(y as u32 * w + x as u32) as usize];
            let ero = eroded[((y + r) as u32 * ew + (x + r) as u32) as usize];
            ring[((y + r) as u32 * ew + (x + r) as u32) as usize] = orig.saturating_sub(ero);
        }
    }
    (ring, ew, eh)
}

/// 圆形核形态学卷积：dilation=true 取邻域 max（亮区扩张），false 取 min（亮区内缩）。
/// 输出画布每边扩 `ceil(radius)` 像素，使原边界外的卷积结果有落点（dilate 亮边、
/// erode 后的边界环都在扩出区可见）。圆形核：dx²+dy² <= radius²。
///
/// 越界采样点按 0（暗）参与运算：dilate 取 max 时 0 不影响结果；erode 取 min 时触发
/// 内缩——亮区边缘因核触及越界 0 而被吃掉，这才是真正的 erosion（描边内侧吃字所需）。
/// 若改为跳过越界点，erode 会在扩出区留下假亮像素、且亮区不收缩。
fn convolve(p: &[u8], w: u32, h: u32, radius: f32, dilation: bool) -> (Vec<u8>, u32, u32) {
    let r = radius.ceil() as i32;
    if r <= 0 {
        return (p.to_vec(), w, h);
    }
    let nw = (w as i32 + 2 * r) as u32;
    let nh = (h as i32 + 2 * r) as u32;
    let mut out = vec![0u8; (nw * nh) as usize];
    let rf = radius * radius;
    let wi = w as i32;
    let hi = h as i32;
    for oy in 0..nh as i32 {
        for ox in 0..nw as i32 {
            // 初值：dilation 从 0 升（取 max），erosion 从 255 降（取 min）。
            let mut best: u8 = if dilation { 0 } else { 255 };
            for dy in -r..=r {
                for dx in -r..=r {
                    if (dx * dx + dy * dy) as f32 > rf {
                        continue;
                    }
                    let sx = ox - r + dx;
                    let sy = oy - r + dy;
                    // 越界按 0：见函数级注释——dilate 不受影响，erode 据此正确内缩。
                    let v = if sx < 0 || sy < 0 || sx >= wi || sy >= hi {
                        0
                    } else {
                        p[(sy as u32 * w + sx as u32) as usize]
                    };
                    best = if dilation { best.max(v) } else { best.min(v) };
                }
            }
            out[(oy as u32 * nw + ox as u32) as usize] = best;
        }
    }
    (out, nw, nh)
}

/// 可分离高斯 blur：水平 + 垂直两 pass。同尺寸（不扩边界，调用方按需 pad）。
/// 标准权重 exp(-x²/(2σ²))，3σ 截断，归一化。边界 clamp（边缘像素复制延伸）。
pub fn gaussian_blur(p: &[u8], w: u32, h: u32, sigma: f32) -> Vec<u8> {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return p.to_vec();
    }
    let r = (sigma * 3.0).ceil() as i32; // 3σ 截断
                                         // 1D 归一化权重。
    let two_sigma2 = 2.0 * sigma * sigma;
    let mut weights: Vec<f32> = (-r..=r)
        .map(|i| (-((i * i) as f32) / two_sigma2).exp())
        .collect();
    let sum: f32 = weights.iter().copied().sum();
    for wv in &mut weights {
        *wv /= sum;
    }
    let wi = w as i32;
    let hi = h as i32;
    // 水平 pass：行内卷积，结果存 f32 避免两次 round 误差累积。
    let mut tmp = vec![0.0f32; (w * h) as usize];
    for y in 0..hi {
        for x in 0..wi {
            let mut acc = 0.0;
            for (k, &wv) in weights.iter().enumerate() {
                let sx = (x + k as i32 - r).clamp(0, wi - 1);
                acc += p[(y as u32 * w + sx as u32) as usize] as f32 * wv;
            }
            tmp[(y as u32 * w + x as u32) as usize] = acc;
        }
    }
    // 垂直 pass：列内卷积，f32 → u8 收尾（钳到 [0,255]）。
    let mut out = vec![0u8; (w * h) as usize];
    for y in 0..hi {
        for x in 0..wi {
            let mut acc = 0.0;
            for (k, &wv) in weights.iter().enumerate() {
                let sy = (y + k as i32 - r).clamp(0, hi - 1);
                acc += tmp[(sy as u32 * w + x as u32) as usize] * wv;
            }
            out[(y as u32 * w + x as u32) as usize] = acc.clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// 位图四周外扩 `pad` 像素填 0。blur 前 pad，让高斯 halo 落在扩出区而不被原 bbox 边界
/// 截断（gaussian_blur 同尺寸不扩，否则柔光投影/glow 的光会硬切在字形 quad 内）。
fn pad_bitmap(p: &[u8], w: u32, h: u32, pad: u32) -> (Vec<u8>, u32, u32) {
    if pad == 0 {
        return (p.to_vec(), w, h);
    }
    let nw = w + 2 * pad;
    let nh = h + 2 * pad;
    let mut out = vec![0u8; (nw * nh) as usize];
    for y in 0..h {
        for x in 0..w {
            out[((y + pad) * nw + x + pad) as usize] = p[(y * w + x) as usize];
        }
    }
    (out, nw, nh)
}

/// blur 需要的外扩像素（3σ 截断，与 gaussian_blur 一致）。
fn blur_pad(sigma: f32) -> u32 {
    (sigma * 3.0).ceil().max(0.0) as u32
}

/// 对单字形 R8 位图应用一个 effect，返回后处理位图（可能扩边界）。
/// 颜色不在此处理（位图只存 alpha），颜色由 build 期 vertex color 提供。
/// - Shadow：blur>0 时 pad 外扩 + 高斯模糊（扩边界容纳 halo，offset 在 build 期偏移 quad）；blur=0 原样。
/// - Stroke：描边环 = 原字形 - erode（内侧吃字，边缘 r 宽环；扩边界）。
/// - Glow：dilate 膨胀亮边 + pad 外扩 + 高斯 blur 晕开。
/// - Blur：pad 外扩 + 高斯 blur（扩边界容纳 halo）。
pub fn apply_effect(p: &[u8], w: u32, h: u32, effect: &FontEffect) -> (Vec<u8>, u32, u32) {
    match effect {
        FontEffect::Shadow { blur, .. } => {
            if *blur > 0.0 {
                let sigma = *blur / 2.0;
                let (pp, pw, ph) = pad_bitmap(p, w, h, blur_pad(sigma));
                (gaussian_blur(&pp, pw, ph, sigma), pw, ph)
            } else {
                (p.to_vec(), w, h)
            }
        }
        FontEffect::Stroke { w: sw, .. } => stroke_ring(p, w, h, *sw),
        FontEffect::Glow { w: gw, .. } => {
            let (d, dw, dh) = dilate(p, w, h, *gw);
            let sigma = *gw / 2.0;
            let (pp, pw, ph) = pad_bitmap(&d, dw, dh, blur_pad(sigma));
            (gaussian_blur(&pp, pw, ph, sigma), pw, ph)
        }
        FontEffect::Blur { w: bw } => {
            let sigma = *bw / 2.0;
            let (pp, pw, ph) = pad_bitmap(p, w, h, blur_pad(sigma));
            (gaussian_blur(&pp, pw, ph, sigma), pw, ph)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_blur_preserves_size_and_range() {
        let w = 5u32;
        let h = 5u32;
        let mut px = vec![0u8; (w * h) as usize];
        for (i, b) in px.iter_mut().enumerate() {
            *b = if i % 2 == 0 { 200 } else { 0 };
        }
        let out = gaussian_blur(&px, w, h, 1.0);
        assert_eq!(out.len(), px.len(), "同尺寸");
        // 归一化权重 → 输出是输入的加权平均，不会超过输入最大值 200（无放大）。
        assert!(out.iter().all(|&v| v <= 200), "归一化：输出不超过输入 max");
    }

    #[test]
    fn gaussian_blur_spreads_bright_pixel() {
        // 单亮像素 blur 后能量扩散到四邻（归一化高斯权重非零）。
        let mut px = vec![0u8; 9];
        px[4] = 255; // 3×3 中心
        let out = gaussian_blur(&px, 3, 3, 1.0);
        assert!(out[1] > 0, "上邻接收 alpha");
        assert!(out[3] > 0, "左邻接收 alpha");
        assert!(out[5] > 0, "右邻接收 alpha");
        assert!(out[7] > 0, "下邻接收 alpha");
    }

    #[test]
    fn gaussian_blur_zero_sigma_is_identity() {
        let px = vec![10, 50, 90, 200, 0, 100, 30, 70, 255];
        let out = gaussian_blur(&px, 3, 3, 0.0);
        assert_eq!(out, px, "sigma=0 → 原样返回");
    }

    #[test]
    fn dilate_grows_shape() {
        // 3×3 中心 1 点，dilate radius=1 → 画布扩到 5×5，中心仍亮，十字邻域变亮。
        let px = vec![0, 0, 0, 0, 255, 0, 0, 0, 0];
        let (out, w, h) = dilate(&px, 3, 3, 1.0);
        let w = w as usize;
        assert_eq!((w as u32, h), (5, 5), "dilate 扩边界 ceil(radius)=1px 每边");
        assert_eq!(out[2 * w + 2], 255, "中心仍亮");
        assert!(out[2 * w + 1] > 0, "左邻变亮（圆形核覆盖）");
        assert!(out[2 * w + 3] > 0, "右邻变亮");
    }

    #[test]
    fn dilate_zero_radius_is_identity() {
        let px = vec![1, 2, 3, 4];
        let (out, w, h) = dilate(&px, 2, 2, 0.0);
        assert_eq!((w, h), (2, 2));
        assert_eq!(out, px);
    }

    #[test]
    fn erode_removes_isolated_pixel() {
        // 5×5 全暗 + 中心 1 亮像素。erode radius=1 → 亮像素邻域含 0，min=0 → 全暗。
        let mut px = vec![0u8; 25];
        px[12] = 255;
        let (out, w, h) = erode(&px, 5, 5, 1.0);
        assert_eq!((w, h), (7, 7), "erode 扩边界");
        assert!(out.iter().all(|&v| v == 0), "孤立亮像素被 erode 吃掉");
    }

    #[test]
    fn effect_sig_distinguishes_variants() {
        let s = effect_sig(&[FontEffect::Shadow {
            ox: 1.0,
            oy: 2.0,
            blur: 3.0,
            color: [0.; 4],
        }]);
        let st = effect_sig(&[FontEffect::Stroke {
            w: 1.0,
            color: [0.; 4],
        }]);
        let g = effect_sig(&[FontEffect::Glow {
            w: 1.0,
            color: [0.; 4],
        }]);
        let b = effect_sig(&[FontEffect::Blur { w: 1.0 }]);
        assert_ne!(s, st);
        assert_ne!(s, g);
        assert_ne!(s, b);
        assert_ne!(st, g);
        assert_ne!(st, b);
        assert_ne!(g, b);
    }

    #[test]
    fn effect_sig_same_params_same_hash() {
        let e1 = vec![FontEffect::Blur { w: 2.5 }];
        let e2 = vec![FontEffect::Blur { w: 2.5 }];
        assert_eq!(effect_sig(&e1), effect_sig(&e2), "同参数同 sig");
        let e3 = vec![FontEffect::Blur { w: 3.0 }];
        assert_ne!(effect_sig(&e1), effect_sig(&e3), "不同参数不同 sig");
    }

    #[test]
    fn effect_sig_distinguishes_color() {
        // color 进 sig：同 blur 不同色 → 不同 sig（保证语义独立）。
        let r = effect_sig(&[FontEffect::Shadow {
            ox: 0.0,
            oy: 0.0,
            blur: 2.0,
            color: [1.0, 0.0, 0.0, 1.0],
        }]);
        let b = effect_sig(&[FontEffect::Shadow {
            ox: 0.0,
            oy: 0.0,
            blur: 2.0,
            color: [0.0, 0.0, 1.0, 1.0],
        }]);
        assert_ne!(r, b, "不同 color 不同 sig");
    }

    #[test]
    fn effect_sig_empty_is_deterministic() {
        assert_eq!(effect_sig(&[]), effect_sig(&[]), "空切片确定性");
    }

    #[test]
    fn apply_effect_shadow_no_blur_is_clone() {
        let px = vec![10, 20, 30, 40];
        let (out, w, h) = apply_effect(
            &px,
            2,
            2,
            &FontEffect::Shadow {
                ox: 1.0,
                oy: 1.0,
                blur: 0.0,
                color: [1.; 4],
            },
        );
        assert_eq!((w, h), (2, 2));
        assert_eq!(
            out, px,
            "blur=0 → shadow 位图 = clone（offset 在 build 期）"
        );
    }

    #[test]
    fn apply_effect_shadow_with_blur_same_size() {
        let px = vec![255u8; 16];
        let (_out, w, h) = apply_effect(
            &px,
            4,
            4,
            &FontEffect::Shadow {
                ox: 1.0,
                oy: 1.0,
                blur: 2.0,
                color: [1.; 4],
            },
        );
        assert!(
            w > 4 && h > 4,
            "shadow blur 应扩边界容纳 halo（光不被切在 quad 内），实际 {w}×{h}"
        );
    }

    #[test]
    fn apply_effect_stroke_expands() {
        let px = vec![255u8; 9];
        let (out, w, h) = apply_effect(
            &px,
            3,
            3,
            &FontEffect::Stroke {
                w: 2.0,
                color: [1.; 4],
            },
        );
        // erode 扩边界 2*ceil(2)=4 → 7×7
        assert_eq!((w, h), (7, 7));
        assert_eq!(out.len(), 49);
    }

    #[test]
    fn stroke_emits_hollow_ring_not_solid_erode() {
        // 实心 3×3 亮块，Stroke 描边应是空心环（中心透明、边缘描边色），不是 erode 的
        // 内缩实心块（中心还亮）。CSS -webkit-text-stroke 描边在字形边缘内侧 r 宽。
        let px = vec![255u8; 9]; // 3×3 实心
        let (out, w, h) = apply_effect(
            &px,
            3,
            3,
            &FontEffect::Stroke {
                w: 1.0,
                color: [1.; 4],
            },
        );
        assert_eq!((w, h), (5, 5), "erode 扩边界 ceil(1)=1 → 5×5");
        // 原 3×3 中心 (1,1) → 扩边界位图 (2,2)。描边环中心透明（填色在 fill 之下）。
        let center = out[(2 * h as i32 + 2) as usize];
        assert_eq!(center, 0, "描边环中心应透明（非内缩实心块），实际 {center}");
        // 边缘环（原 3×3 角 (0,0) → 位图 (1,1)）应亮（描边）。
        let edge = out[(h as i32 + 1) as usize];
        assert_eq!(edge, 255, "描边环边缘应亮，实际 {edge}");
    }

    #[test]
    fn apply_effect_glow_expands() {
        let px = vec![255u8; 9];
        let (_out, w, h) = apply_effect(
            &px,
            3,
            3,
            &FontEffect::Glow {
                w: 1.0,
                color: [1.; 4],
            },
        );
        // dilate 扩 2*ceil(1)=2 → 5×5，blur 再 pad 扩边界 → > 5×5
        assert!(
            w > 5 && h > 5,
            "glow 的 blur 应扩边界容纳 halo，实际 {w}×{h}"
        );
    }

    #[test]
    fn apply_effect_blur_same_size() {
        let px = vec![255u8; 9];
        let (_out, w, h) = apply_effect(&px, 3, 3, &FontEffect::Blur { w: 2.0 });
        assert!(w > 3 && h > 3, "blur 应扩边界容纳 halo，实际 {w}×{h}");
    }

    #[test]
    fn blur_halo_extends_past_original_bounds() {
        // 单亮像素 blur 后 halo 应扩出原边界——否则柔光投影的光被切在 quad 内。
        let mut px = vec![0u8; 25]; // 5×5
        px[12] = 255; // 中心亮像素
        let (out, w, _h) = apply_effect(&px, 5, 5, &FontEffect::Blur { w: 4.0 });
        assert!(w > 5, "blur 应扩边界 ow={w}");
        let halo = out.iter().filter(|&&v| v > 0).count();
        assert!(halo > 1, "blur halo 应扩散到多像素，实际 {halo} 个非零");
    }
}
