//! 渐变渲染参数：CSS `Gradient`（解析期，% 未解析）→ 像素参数（渲染期按当帧 box 解析）。
//!
//! 同一套数学两处消费：
//! - Unity shader（program=6/7，GRADIENT 变体）：blob grad_params 列下发本结构，
//!   per-fragment 算 t + 分段 lerp；
//! - 文本渐变（background-clip:text）：`sample_gradient` 在 CPU 按字形角采样
//!   （与 shader 公式逐字对齐——改公式两侧必须同步）。
//!
//! 背景渐变统一走 program=6 per-fragment（顶点色路径只对 2 色正交 linear 精确，
//! 多 stop 是分段函数、radial 非 affine，顶点色均不可表达）。

use crate::style::resolved::{GradCoord, Gradient, RadialExtent};

/// 渐变像素参数（FFI grad_params 列布局，见 `to_bytes`）。Default 全零 = 无渐变
/// （由 program=6/7 门控启用，非渐变节点恒全零）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize)]
pub struct GradientParams {
    /// 0=linear, 1=radial。
    pub kind: u32,
    /// linear：CSS 角度（0deg=to top 顺时针）。调试/dump 用，shader 不读。
    pub angle_deg: f32,
    /// linear：梯度轴单位向量（屏幕 y 向下：0deg → (0,-1)）。
    pub dir: [f32; 2],
    /// linear：4 角在梯度轴上投影的 min 与 1/(max-min)（CSS 渐变线归一化）。
    pub t0: f32,
    pub inv_span: f32,
    /// radial：圆心（box 局部像素，左上原点）。
    pub center: [f32; 2],
    /// radial：椭圆半径像素（circle 时 rx==ry）。
    pub radii: [f32; 2],
    /// stop 数（1..=8；stops 定长 8 槽，未用槽零）。
    pub stop_count: u32,
    /// stops[8] × {r, g, b, a, pos}（straight RGBA + 0..1 定位）。
    pub stops: [[f32; 5]; 8],
}

impl GradientParams {
    /// 序列化定长（52B 头 + 8×20B stops = 208 字节，小端）。FFI blob 写出与
    /// C# 解析共用此布局（照 EffectBlock::to_bytes 先例）。
    pub const SIZE: usize = 208;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        let mut o = 0usize;
        macro_rules! wf {
            ($v:expr) => {
                buf[o..o + 4].copy_from_slice(&($v).to_le_bytes());
                o += 4;
            };
        }
        // kind/stop_count 按 f32 语义写（0.0/1.0、n.0），不当 u32 位模式——C# 列读成
        // float：位模式会读成 denormal（stop_count=2 → 2.8e-45 → (int)=0 → 单色渐变）。
        wf!(self.kind as f32);
        wf!(self.angle_deg);
        wf!(self.dir[0]);
        wf!(self.dir[1]);
        wf!(self.t0);
        wf!(self.inv_span);
        wf!(self.center[0]);
        wf!(self.center[1]);
        wf!(self.radii[0]);
        wf!(self.radii[1]);
        wf!(self.stop_count as f32);
        wf!(0f32); // reserved（8 字节对齐补位，恒 0）
        for s in &self.stops {
            for &v in s {
                wf!(v);
            }
        }
        debug_assert_eq!(o, Self::SIZE, "GradientParams 字段顺序/数量与 SIZE 不符");
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Self {
        assert_eq!(buf.len(), Self::SIZE, "grad_params 列定长 208B");
        let rf = |o: usize| f32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        let mut stops = [[0f32; 5]; 8];
        for (i, s) in stops.iter_mut().enumerate() {
            let base = 48 + i * 20;
            *s = [
                rf(base),
                rf(base + 4),
                rf(base + 8),
                rf(base + 12),
                rf(base + 16),
            ];
        }
        Self {
            kind: rf(0) as u32,
            angle_deg: rf(4),
            dir: [rf(8), rf(12)],
            t0: rf(16),
            inv_span: rf(20),
            center: [rf(24), rf(28)],
            radii: [rf(32), rf(36)],
            stop_count: rf(40) as u32,
            stops,
        }
    }
}

/// 按当帧 box（w/h 像素，左上原点）把 CSS 渐变解析成像素参数。
pub fn resolve_gradient(g: &Gradient, w: f32, h: f32) -> GradientParams {
    let mut p = GradientParams::default();
    let stops_src = g.stops();
    p.stop_count = stops_src.len() as u32;
    for (i, s) in stops_src.iter().enumerate().take(8) {
        p.stops[i] = [s.color[0], s.color[1], s.color[2], s.color[3], s.pos];
    }
    match g {
        Gradient::Linear { angle_deg, .. } => {
            p.kind = 0;
            p.angle_deg = *angle_deg;
            let rad = angle_deg.to_radians();
            // CSS 0deg=to top；屏幕 y 向下 → 方向 = (sin θ, −cos θ)。
            p.dir = [rad.sin(), -rad.cos()];
            // 渐变线归一化：4 角投影到梯度轴，t ∈ [0,1] 覆盖整个 box（CSS 规范算法）。
            let corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
            let mut tmin = f32::MAX;
            let mut tmax = f32::MIN;
            for c in &corners {
                let t = c[0] * p.dir[0] + c[1] * p.dir[1];
                tmin = tmin.min(t);
                tmax = tmax.max(t);
            }
            p.t0 = tmin;
            p.inv_span = 1.0 / (tmax - tmin).max(1e-6);
        }
        Gradient::Radial {
            extent,
            shape,
            center,
            ..
        } => {
            p.kind = 1;
            let resolve_c = |c: &GradCoord, side: f32| match *c {
                GradCoord::Pct(v) => v * side,
                GradCoord::Px(v) => v,
            };
            let (cx, cy) = (resolve_c(&center[0], w), resolve_c(&center[1], h));
            p.center = [cx, cy];
            // CSS 尺寸关键字按 box 解析；corner 关键字 = 对应 side 椭圆缩放穿过角点。
            let side = |far: bool, side: f32, c: f32| {
                let d1 = c.abs();
                let d2 = (side - c).abs();
                if far {
                    d1.max(d2)
                } else {
                    d1.min(d2)
                }
            };
            let is_circle = *shape == crate::style::resolved::RadialShape::Circle;
            let (mut rx, mut ry) = match *extent {
                RadialExtent::ClosestSide => {
                    let (sx, sy) = (side(false, w, cx), side(false, h, cy));
                    // circle 单一半径 = 四边最近距离（逐轴椭圆值曾让 circle 渲染成椭圆）。
                    if is_circle {
                        let r = sx.min(sy);
                        (r, r)
                    } else {
                        (sx, sy)
                    }
                }
                RadialExtent::FarthestSide => {
                    let (sx, sy) = (side(true, w, cx), side(true, h, cy));
                    if is_circle {
                        let r = sx.max(sy);
                        (r, r)
                    } else {
                        (sx, sy)
                    }
                }
                RadialExtent::ClosestCorner | RadialExtent::FarthestCorner => {
                    let far = matches!(*extent, RadialExtent::FarthestCorner);
                    let (sx, sy) = (side(far, w, cx), side(far, h, cy));
                    // 4 角中最近/最远角的欧氏距离。
                    let mut best_d = if far { f32::MIN } else { f32::MAX };
                    for corner in [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]] {
                        let d = ((corner[0] - cx).powi(2) + (corner[1] - cy).powi(2)).sqrt();
                        if (far && d > best_d) || (!far && d < best_d) {
                            best_d = d;
                        }
                    }
                    if is_circle {
                        // circle corner = 圆心到最近/最远角距离（单值）。
                        (best_d, best_d)
                    } else {
                        // CSS（css-images-3，Chrome 实测对齐）：ellipse corner 关键字 =
                        // 逐轴 side 距离 × √2 —— 椭圆精确穿过该角（corner 在归一化
                        // (1/√2,1/√2) 处，模长恰 1）。曾误用 f=角距/√(sx²+sy²) 缩放，
                        // 居中盒算出 f=1 → farthest-corner 塌成 farthest-side。
                        (sx * std::f32::consts::SQRT_2, sy * std::f32::consts::SQRT_2)
                    }
                }
                RadialExtent::Explicit(a, b) => {
                    let r1 = a.unwrap_or(0.0);
                    match b {
                        Some(r2) => (r1, r2),
                        None => (r1, r1), // 单长度 = 正圆
                    }
                }
            };
            rx = rx.max(1e-4);
            ry = ry.max(1e-4);
            p.radii = [rx, ry];
        }
    }
    p
}

/// 在 box 局部坐标 (x, y) 采样渐变色（straight RGBA）。shader GRADIENT 分支的
/// CPU 镜像——公式变更两侧同步。插值走 premultiplied（CSS 渐变语义，
/// rgba→transparent 中点无灰边）。
pub fn sample_gradient(p: &GradientParams, x: f32, y: f32) -> [f32; 4] {
    let t = match p.kind {
        1 => {
            let (dx, dy) = (x - p.center[0], y - p.center[1]);
            ((dx / p.radii[0]).powi(2) + (dy / p.radii[1]).powi(2)).sqrt()
        }
        _ => ((x * p.dir[0] + y * p.dir[1]) - p.t0) * p.inv_span,
    }
    .clamp(0.0, 1.0);
    let n = (p.stop_count.clamp(1, 8)) as usize;
    // t 落在 stop[i].pos..=stop[i+1].pos 段内 → premultiplied lerp；段外（t 小于首/
    // 大于末，含 clamp 后的端点）取端 stop 色。
    let stops = &p.stops[..n];
    if t <= stops[0][4] || n == 1 {
        return [stops[0][0], stops[0][1], stops[0][2], stops[0][3]];
    }
    if t >= stops[n - 1][4] {
        let s = stops[n - 1];
        return [s[0], s[1], s[2], s[3]];
    }
    for w in stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t >= a[4] && t <= b[4] {
            let span = (b[4] - a[4]).max(1e-6);
            let f = ((t - a[4]) / span).clamp(0.0, 1.0);
            // premultiplied lerp 再反预乘（a=0 段 rgb 无意义，直通防 NaN）。
            let pa = [a[0] * a[3], a[1] * a[3], a[2] * a[3], a[3]];
            let pb = [b[0] * b[3], b[1] * b[3], b[2] * b[3], b[3]];
            let m = [
                pa[0] + (pb[0] - pa[0]) * f,
                pa[1] + (pb[1] - pa[1]) * f,
                pa[2] + (pb[2] - pa[2]) * f,
                pa[3] + (pb[3] - pa[3]) * f,
            ];
            if m[3] <= 1e-6 {
                return [0.0, 0.0, 0.0, 0.0];
            }
            return [m[0] / m[3], m[1] / m[3], m[2] / m[3], m[3]];
        }
    }
    [0.0, 0.0, 0.0, 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::resolved::{Gradient, GradientStop, RadialExtent};

    fn lin(angle: f32, stops: Vec<(f32, [f32; 4])>) -> Gradient {
        Gradient::Linear {
            angle_deg: angle,
            stops: stops
                .into_iter()
                .map(|(pos, color)| GradientStop { color, pos })
                .collect(),
        }
    }

    #[test]
    fn linear_to_right_span_is_box_width() {
        // to right（90deg）在 200x100 box：dir=(1,0)，投影 min=0 max=200。
        let p = resolve_gradient(
            &lin(
                90.0,
                vec![(0.0, [1.0, 0.0, 0.0, 1.0]), (1.0, [0.0, 0.0, 1.0, 1.0])],
            ),
            200.0,
            100.0,
        );
        assert_eq!(p.kind, 0);
        assert!((p.dir[0] - 1.0).abs() < 1e-6 && p.dir[1].abs() < 1e-6);
        assert!(p.t0.abs() < 1e-4);
        assert!((p.inv_span - 1.0 / 200.0).abs() < 1e-6);
    }

    #[test]
    fn linear_45deg_diagonal_span_matches_css() {
        // 45deg 在 100x100：dir=(√2/2, −√2/2)，投影范围 = ±100·√2/2（对角两角）。
        let p = resolve_gradient(
            &lin(45.0, vec![(0.0, [1.0; 4]), (1.0, [0.0; 4])]),
            100.0,
            100.0,
        );
        let half = 100.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((p.t0 + half).abs() < 1e-3, "t0 = -对角半投影, got {}", p.t0);
        assert!((p.inv_span - 1.0 / (2.0 * half)).abs() < 1e-5);
        // 45deg（向右上）渐变轴 = 左下→右上对角线：BL 角 t=0（起点色）、TR 角 t=1（终点色）；
        // TL/BR 角 t=0.5（对角中点，非端点——y 向下屏幕系 45deg 不经过它们）。
        assert_eq!(sample_gradient(&p, 0.0, 100.0), [1.0; 4], "BL = 起点");
        assert_eq!(sample_gradient(&p, 100.0, 0.0), [0.0; 4], "TR = 终点");
        let mid = sample_gradient(&p, 0.0, 0.0);
        assert!((mid[3] - 0.5).abs() < 1e-4, "TL = 对角中点 α=0.5");
    }

    #[test]
    fn radial_farthest_corner_default() {
        // 100x80 box 居中：farthest-corner 椭圆 = side×√2（Chrome 实测）：
        // rx=50√2≈70.71, ry=40√2≈56.57 —— 椭圆精确穿过最远角。
        let g = Gradient::Radial {
            extent: RadialExtent::FarthestCorner,
            shape: crate::style::resolved::RadialShape::Ellipse,
            center: [GradCoord::Pct(0.5), GradCoord::Pct(0.5)],
            stops: vec![GradientStop {
                color: [1.0, 0.0, 0.0, 1.0],
                pos: 0.0,
            }],
        };
        let p = resolve_gradient(&g, 100.0, 80.0);
        assert_eq!(p.kind, 1);
        assert!((p.center[0] - 50.0).abs() < 1e-4 && (p.center[1] - 40.0).abs() < 1e-4);
        assert!(
            (p.radii[0] - 50.0 * std::f32::consts::SQRT_2).abs() < 1e-3,
            "rx≈70.71, got {}",
            p.radii[0]
        );
        assert!(
            (p.radii[1] - 40.0 * std::f32::consts::SQRT_2).abs() < 1e-3,
            "ry≈56.57, got {}",
            p.radii[1]
        );
    }

    #[test]
    fn radial_closest_side_offset_center() {
        // 椭圆逐轴：圆心偏移 (30,40) 在 100x100 → rx=min(30,70)=30, ry=min(40,60)=40。
        let g = Gradient::Radial {
            extent: RadialExtent::ClosestSide,
            shape: crate::style::resolved::RadialShape::Ellipse,
            center: [GradCoord::Px(30.0), GradCoord::Px(40.0)],
            stops: vec![GradientStop {
                color: [1.0; 4],
                pos: 0.0,
            }],
        };
        let p = resolve_gradient(&g, 100.0, 100.0);
        assert!((p.radii[0] - 30.0).abs() < 1e-4);
        assert!((p.radii[1] - 40.0).abs() < 1e-4);
    }

    #[test]
    fn radial_circle_keyword_single_radius() {
        // circle + 关键字 = 单一半径（CSS 单值语义），不是逐轴椭圆：
        // - closest-side 120x80 居中：r = min(60,40) = 40
        // - farthest-side：r = max(60,40) = 60
        // - farthest-corner：r = 圆心到角距离 sqrt(60²+40²) ≈ 72.11
        let mk = |extent| Gradient::Radial {
            extent,
            shape: crate::style::resolved::RadialShape::Circle,
            center: [GradCoord::Pct(0.5), GradCoord::Pct(0.5)],
            stops: vec![GradientStop {
                color: [1.0; 4],
                pos: 0.0,
            }],
        };
        let p = resolve_gradient(&mk(RadialExtent::ClosestSide), 120.0, 80.0);
        assert!((p.radii[0] - 40.0).abs() < 1e-3 && (p.radii[1] - 40.0).abs() < 1e-3);
        let p = resolve_gradient(&mk(RadialExtent::FarthestSide), 120.0, 80.0);
        assert!((p.radii[0] - 60.0).abs() < 1e-3 && (p.radii[1] - 60.0).abs() < 1e-3);
        let p = resolve_gradient(&mk(RadialExtent::FarthestCorner), 120.0, 80.0);
        assert!((p.radii[0] - 72.111).abs() < 1e-2, "got {}", p.radii[0]);
    }

    #[test]
    fn radial_home_halo_params() {
        // home：1100x560 椭圆 at 82%,-12% 在 1920x1080 → cx=1574.4, cy=-129.6。
        let g = Gradient::Radial {
            extent: RadialExtent::Explicit(Some(1100.0), Some(560.0)),
            shape: crate::style::resolved::RadialShape::Ellipse,
            center: [GradCoord::Pct(0.82), GradCoord::Pct(-0.12)],
            stops: vec![
                GradientStop {
                    color: [0.373, 0.706, 0.831, 0.1],
                    pos: 0.0,
                },
                GradientStop {
                    color: [0.0; 4],
                    pos: 0.6,
                },
            ],
        };
        let p = resolve_gradient(&g, 1920.0, 1080.0);
        assert!((p.center[0] - 1574.4).abs() < 0.1);
        assert!((p.center[1] + 129.6).abs() < 0.1);
        assert!((p.radii[0] - 1100.0).abs() < 1e-4 && (p.radii[1] - 560.0).abs() < 1e-4);
        // 中心处 t=0 → 首 stop 色；椭圆边界外 t>0.6 → clamp 后末 stop（全透明）。
        let c = sample_gradient(&p, 1574.4, -129.6);
        assert!((c[3] - 0.1).abs() < 1e-4, "中心 alpha=0.1");
        let far = sample_gradient(&p, 0.0, 1080.0);
        assert_eq!(far, [0.0; 4], "远处 clamp 到 transparent");
    }

    #[test]
    fn sample_premultiplied_no_gray_fringe() {
        // rgba(255,0,0,1) → transparent：中点应为半透明纯红（premultiplied），
        // 直通 lerp 会得出暗红/灰（(127,0,0,0.5) 的 straight rgb=127——那是
        // premult 结果当 straight 用，浏览器按 premult 合成回纯红）。
        let p = resolve_gradient(
            &lin(
                90.0,
                vec![(0.0, [1.0, 0.0, 0.0, 1.0]), (1.0, [0.0, 0.0, 0.0, 0.0])],
            ),
            200.0,
            100.0,
        );
        let mid = sample_gradient(&p, 100.0, 50.0);
        assert!((mid[0] - 1.0).abs() < 1e-4, "rgb 保持纯红, got {mid:?}");
        assert!((mid[3] - 0.5).abs() < 1e-4, "alpha 中点 0.5");
    }

    #[test]
    fn sample_multi_stop_segments() {
        // 3 stop（0 / 0.5 / 1）：t=0.25 → 首段中点；t=0.75 → 次段中点。
        let p = resolve_gradient(
            &lin(
                90.0,
                vec![
                    (0.0, [1.0, 0.0, 0.0, 1.0]),
                    (0.5, [0.0, 1.0, 0.0, 1.0]),
                    (1.0, [0.0, 0.0, 1.0, 1.0]),
                ],
            ),
            100.0,
            100.0,
        );
        let a = sample_gradient(&p, 25.0, 0.0);
        assert!(
            (a[0] - 0.5).abs() < 1e-4 && (a[1] - 0.5).abs() < 1e-4,
            "{a:?}"
        );
        let b = sample_gradient(&p, 75.0, 0.0);
        assert!(
            (b[1] - 0.5).abs() < 1e-4 && (b[2] - 0.5).abs() < 1e-4,
            "{b:?}"
        );
    }

    #[test]
    fn params_bytes_roundtrip() {
        let mut p = resolve_gradient(
            &lin(
                137.0,
                vec![(0.0, [0.1, 0.2, 0.3, 0.4]), (0.7, [0.5, 0.6, 0.7, 0.8])],
            ),
            333.0,
            222.0,
        );
        p.stops[7] = [9.0, 8.0, 7.0, 6.0, 0.99]; // 未用槽也须 round-trip
        let back = GradientParams::from_bytes(&p.to_bytes());
        assert_eq!(back, p);
        assert_eq!(GradientParams::SIZE, 208);
    }

    #[test]
    fn default_params_zero() {
        // 非渐变节点列值恒全零（C# 侧按 program 门控，不读内容）。
        assert_eq!(GradientParams::default().to_bytes(), [0u8; 208]);
    }
}
