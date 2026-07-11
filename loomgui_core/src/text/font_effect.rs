//! 文字效果配置 + effect_sig 指纹。
//!
//! SDF 改造前本模块还承担位图后处理（dilate/erode/gaussian_blur 等），挂在 atlas ensure
//! 内对 R8 位图做形态学操作产生 shadow/glow/stroke/blur 的独立字形槽。SDF 化后这一路径
//! 已废——文字效果改由 shader uniform 实现（atlas 只存一份 raw SDF，shader 按 effect
//! 参数在 GPU 端做距离场阈值/膨胀/描边）。
//!
//! 保留下来的内容：
//! - `FontEffect` 枚举：描述效果类型 + 参数，仍是 DSL → ResolvedStyle 的配置载体。
//! - `effect_sig`：把效果参数 hash 成稳定指纹，供后续 shader uniform 打包按 effect 组复用。
//! color 进 sig 以保证同参数不同色的效果在打包时各自占独立 uniform 槽。

/// 单个文字效果配置。Copy：参数全为值类型。
/// Serialize/Deserialize：text_effects 挂在 ResolvedStyle 上随 pkg.bin 序列化。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FontEffect {
    /// 阴影：shader 期按 (ox, oy) 偏移采样 + 距离阈值软化模拟 blur。
    Shadow {
        ox: f32,
        oy: f32,
        blur: f32,
        color: [f32; 4],
    },
    /// 描边：shader 期按距离阈值在字形外侧填描边色。
    Stroke { w: f32, color: [f32; 4] },
    /// 发光：shader 期按距离阈值外扩 + 颜色衰减晕开。
    Glow { w: f32, color: [f32; 4] },
    /// 模糊：shader 期按距离阈值软化整个字形。
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
/// 输入 = (discriminant, params)；per-size 不进 sig（size 不再进 GlyphKey）。
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
