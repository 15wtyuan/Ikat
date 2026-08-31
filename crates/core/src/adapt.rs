//! 分辨率适配数学（设计分辨率 → 任意屏幕）。纯函数、无状态——引擎集成层
//! （Unity Driver / 未来 Godot 后端）每帧或屏幕变化时调 [`compute`]，拿
//! scale/root/offset 三件套去设根变换 + `Stage::set_root_size`，全引擎共享
//! 同一份策略实现（跨引擎行为一致 = 分辨率适配是框架承诺，不是各引擎良心）。
//!
//! 模型（web 启发，见 main-design §11.5）：
//! - **Letterbox**：contain——root 锁设计分辨率，取较小缩放比，safe 区内居中
//!   留黑边。布局永远按设计稿排（最可预测），屏幕长宽比不匹配时留白。
//! - **FitWidth / FitHeight**：拆黑边——锁定一维锚（宽或高按设计稿），另一维
//!   root 直接取真实屏幕换算值，布局重排（flex/% / vw-vh 声明流动）。无黑边、
//!   无裁切，px 不变形（缩放仍是均匀的）。
//!
//! safe-area：Fit 模式 root 贴物理全屏（背景满铺到物理边，unsafe 带被 root 覆盖），
//! 避让交给 CSS `env(safe-area-inset-*)`（unsafe 深度经 Stage viewport inset 暴露，
//! web viewport-fit=cover 语义）；Letterbox 以 safe 矩形为 contain 的框——root 全在
//! safe 内，env() 恒 0，黑边已让位、不重复避让。

/// 适配模式。数值即 FFI 侧 u32（ABI 稳定：只增不改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AdaptMode {
    /// contain：完整可见，safe 区内居中，留 letterbox 黑边
    Letterbox = 0,
    /// 宽锚：宽 = 设计宽，高重排（竖屏异形高常用）
    FitWidth = 1,
    /// 高锚：高 = 设计高，宽重排（横屏带鱼屏常用）
    FitHeight = 2,
}

impl AdaptMode {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Letterbox,
            1 => Self::FitWidth,
            2 => Self::FitHeight,
            _ => return None,
        })
    }
}

/// 适配输出。`scale` = 渲染均匀缩放比（screen_px = design_px * scale）；
/// `root` = 喂 Stage 的画布尺寸（设计单位）；`offset` = 设计原点 (0,0) 在屏幕
/// 像素系的落点（screen 系原点左上、y 向下——与 Unity Screen 一致）。Fit 模式
/// offset = (0,0)（root 贴物理原点，unsafe 避让走 CSS env()）。
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct AdaptResult {
    pub scale: f32,
    pub root_w: f32,
    pub root_h: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

/// 计算适配三件套。`safe` = 安全区矩形 (x, y, w, h) 屏幕像素，仅 Letterbox 消费
/// （contain 框）；Fit 模式忽略它（root 贴物理边，unsafe 深度走 env() 通道）。
/// 零宽高（编辑器未配屏等防御场景）自动退回全屏。design 非有限或 ≤0 时按
/// 1080×1920 兜底（与 Unity Driver 侧零向量兜底同值，双端一致）。
pub fn compute(
    design: (f32, f32),
    screen: (f32, f32),
    safe: (f32, f32, f32, f32),
    mode: AdaptMode,
) -> AdaptResult {
    let (dw, dh) =
        if design.0.is_finite() && design.1.is_finite() && design.0 > 0.0 && design.1 > 0.0 {
            design
        } else {
            (1080.0, 1920.0)
        };
    let (sw, sh) = screen;
    let (mut sx, mut sy, mut saw, mut sah) = safe;
    if !(saw.is_finite() && sah.is_finite()) || saw <= 0.0 || sah <= 0.0 {
        sx = 0.0;
        sy = 0.0;
        saw = sw;
        sah = sh;
    }
    match mode {
        AdaptMode::Letterbox => {
            let scale = (saw / dw).min(sah / dh);
            // rendered span（dw*scale × dh*scale）在 safe 矩形内居中
            let off_x = sx + (saw - dw * scale) * 0.5;
            let off_y = sy + (sah - dh * scale) * 0.5;
            AdaptResult {
                scale,
                root_w: dw,
                root_h: dh,
                offset_x: off_x,
                offset_y: off_y,
            }
        }
        AdaptMode::FitWidth => {
            let scale = sw / dw;
            AdaptResult {
                scale,
                root_w: dw,
                root_h: sh / scale,
                offset_x: 0.0,
                offset_y: 0.0,
            }
        }
        AdaptMode::FitHeight => {
            let scale = sh / dh;
            AdaptResult {
                scale,
                root_w: sw / scale,
                root_h: dh,
                offset_x: 0.0,
                offset_y: 0.0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.0);

    #[test]
    fn letterbox_equal_aspect_scales_uniformly() {
        // 设计 1080×1920 → 屏 1440×2560（等比 4:3×2）：整体放大，无黑边偏移
        let r = compute(
            (1080.0, 1920.0),
            (1440.0, 2560.0),
            FULL,
            AdaptMode::Letterbox,
        );
        assert_eq!(r.scale, 1440.0 / 1080.0);
        assert_eq!((r.root_w, r.root_h), (1080.0, 1920.0));
        assert_eq!((r.offset_x, r.offset_y), (0.0, 0.0));
    }

    #[test]
    fn letterbox_wider_screen_pillars() {
        // 设计 1920×1080 → 屏 2560×1080：高受限 scale=1，左右各 320 黑边
        let r = compute(
            (1920.0, 1080.0),
            (2560.0, 1080.0),
            FULL,
            AdaptMode::Letterbox,
        );
        assert_eq!(r.scale, 1.0);
        assert_eq!(r.offset_x, 320.0);
        assert_eq!(r.offset_y, 0.0);
    }

    #[test]
    fn fit_width_taller_root_no_bars() {
        // 设计 1080×1920 → 屏 1080×2340：宽锚 scale=1，root 高变 2340（重排），无偏移
        let r = compute(
            (1080.0, 1920.0),
            (1080.0, 2340.0),
            FULL,
            AdaptMode::FitWidth,
        );
        assert_eq!(r.scale, 1.0);
        assert_eq!((r.root_w, r.root_h), (1080.0, 2340.0));
        assert_eq!((r.offset_x, r.offset_y), (0.0, 0.0));
    }

    #[test]
    fn fit_width_shorter_root_squeezes() {
        // 设计 1080×1920 → 屏 1080×1600（更矮）：root 高 1600 < 设计 1920——
        // 写死 px 的页面可能溢出，flex/vh 声明收缩。这是 Fit 语义（重排非裁切）。
        let r = compute(
            (1080.0, 1920.0),
            (1080.0, 1600.0),
            FULL,
            AdaptMode::FitWidth,
        );
        assert_eq!(r.scale, 1.0);
        assert_eq!(r.root_h, 1600.0);
    }

    #[test]
    fn fit_height_wider_root() {
        // 设计 1920×1080 → 带鱼屏 2560×1080：高锚 scale=1，root 宽 2560
        let r = compute(
            (1920.0, 1080.0),
            (2560.0, 1080.0),
            FULL,
            AdaptMode::FitHeight,
        );
        assert_eq!(r.scale, 1.0);
        assert_eq!((r.root_w, r.root_h), (2560.0, 1080.0));
    }

    #[test]
    fn fit_modes_scale_with_nonunit_ratio() {
        // 设计 1080×1920 → 屏 1170×2532（iPhone 逻辑分辨率）：FitWidth scale=1170/1080，
        // root 高 = 2532/scale ≈ 2337.8（>1920，多出空间交给重排）
        let r = compute(
            (1080.0, 1920.0),
            (1170.0, 2532.0),
            FULL,
            AdaptMode::FitWidth,
        );
        assert!((r.scale - 1170.0 / 1080.0).abs() < 1e-6);
        assert!((r.root_h - 2532.0 / r.scale).abs() < 1e-3);
        assert!(r.root_h > 1920.0);
    }

    #[test]
    fn fit_modes_ignore_safe_area() {
        // 刘海屏：safe=(0,132,1080,2208)（顶部 132px 状态栏）→ Fit 模式贴物理边：
        // root 高按整屏 2340 算、offset=(0,0)，132px 带被 root 覆盖，避让交给
        // CSS env(safe-area-inset-*)（Stage viewport inset 通道）。
        let r = compute(
            (1080.0, 1920.0),
            (1080.0, 2340.0),
            (0.0, 132.0, 1080.0, 2208.0),
            AdaptMode::FitWidth,
        );
        assert_eq!(r.scale, 1.0);
        assert_eq!(r.root_h, 2340.0);
        assert_eq!((r.offset_x, r.offset_y), (0.0, 0.0));
        let r = compute(
            (1080.0, 1920.0),
            (1080.0, 2340.0),
            (12.0, 132.0, 1056.0, 2208.0),
            AdaptMode::FitHeight,
        );
        assert_eq!(r.root_h, 1920.0);
        assert!((r.root_w - 1080.0 * 1920.0 / 2340.0).abs() < 1e-3);
        assert_eq!((r.offset_x, r.offset_y), (0.0, 0.0));
    }

    #[test]
    fn letterbox_centers_inside_safe_area() {
        // letterbox 的 contain 框 = safe 矩形：span 在 safe 内居中
        let r = compute(
            (1920.0, 1080.0),
            (2560.0, 1200.0),
            (0.0, 60.0, 2560.0, 1080.0),
            AdaptMode::Letterbox,
        );
        assert_eq!(r.scale, 1.0);
        assert_eq!(r.offset_x, 320.0);
        assert_eq!(r.offset_y, 60.0);
    }

    #[test]
    fn zero_safe_area_falls_back_to_full_screen() {
        // Fit 模式已不吃 safe，回退门只有 Letterbox 消费——挂 Letterbox 验。
        let r = compute(
            (1080.0, 1920.0),
            (1080.0, 2340.0),
            (0.0, 0.0, 0.0, 0.0),
            AdaptMode::Letterbox,
        );
        let full = compute(
            (1080.0, 1920.0),
            (1080.0, 2340.0),
            FULL,
            AdaptMode::Letterbox,
        );
        assert_eq!(r, full);
    }

    #[test]
    fn invalid_design_falls_back() {
        let r = compute((0.0, 0.0), (1440.0, 2560.0), FULL, AdaptMode::Letterbox);
        assert_eq!((r.root_w, r.root_h), (1080.0, 1920.0));
    }

    #[test]
    fn mode_roundtrip_u32() {
        for m in [
            AdaptMode::Letterbox,
            AdaptMode::FitWidth,
            AdaptMode::FitHeight,
        ] {
            assert_eq!(AdaptMode::from_u32(m as u32), Some(m));
        }
        assert_eq!(AdaptMode::from_u32(3), None);
    }
}
