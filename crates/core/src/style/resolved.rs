use serde::{Deserialize, Serialize};

/// Tracks which inherited CSS properties were explicitly declared (set-ness bitmask).
/// Each bit corresponds to one inheritable property (see INH_* constants in dynamic.rs).
/// Baked at package time into base_style; rematch reads it as the per-frame inheritance baseline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InheritedSet(pub u16);

use taffy::style::LengthPercentage;
use taffy::style::Style as TaffyStyle;
use taffy::FlexDirection;

/// CSS transition 声明（单属性）。prop: None = all（任一通道变化触发）。
/// 围栏先支持 opacity/color/background-color/all 映射到 TweenProp。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransitionSpec {
    /// None = all（动画任一变化的通道）
    pub prop: Option<crate::tween::TweenProp>,
    /// 动画时长（秒）
    pub duration: f32,
    /// easing 函数
    pub ease: crate::tween::Ease,
    /// 延迟（秒）
    pub delay: f32,
}

/// CSS overflow 轴模式。
/// `#[repr(u8)]` 保证 FFI/序列化稳定，`Default = Visible`。
/// Scroll/Auto 的物理/手势由 scroll 模块实现；本 enum 仅承载语义值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum OverflowMode {
    #[default]
    Visible = 0,
    Hidden = 1,
    Scroll = 2,
    Auto = 3,
}

/// CSS background-size 三档（围栏子集）。
/// `#[repr(u8)]` 保证序列化稳定；`Default = Stretch`（100% 语义，未设时拉伸填满，非 CSS auto）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum BackgroundSize {
    #[default]
    Stretch = 0, // 100% / 未设：UV 0..1 拉伸填满
    Cover = 1,   // 铺满裁剪（scale=max，UV 内收取子区中央）
    Contain = 2, // 完整放入留白（scale=min，UV 外扩，子区外透明透出底色）
}

/// 2 色线性渐变方向。围栏仅支持 4 正向（to right/left/top/bottom）；
/// 多 stop / 斜角度（45deg 等）由 mapping 静默忽略（apply_decl 返 false），与现有围栏外 CSS 语义一致。
/// `#[repr(u8)]` 保证 FFI/序列化稳定（与 BackgroundSize/OverflowMode 同模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum GradientDir {
    ToRight = 0,
    ToLeft = 1,
    ToTop = 2,
    ToBottom = 3,
}

/// 2 色线性渐变（背景填充）。color_a 是首 stop（渐变起点），color_b 是末 stop（终点）；
/// 起点/终点由 `dir` 决定（to right → 左为 a 右为 b，与 CSS 语义一致）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gradient2 {
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub dir: GradientDir,
}

/// LoomGUI display 旁路字段（与 taffy_style.display 解耦）。
///
/// （R1+ 围栏不允许 `display:block`——围栏外标签编译期报错。）
/// IR 层 block div 仍是 flex（taffy_style.display = Flex）——不用 taffy `Display::Block`，
/// 那会触发 taffy block 布局语义，违反"div 永远 flex"不变量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum DisplayMode {
    #[default]
    Flex = 0,
    Block = 1,
    None = 2,
}

/// CSS border-radius 单角半径。
/// (h, v) = (水平, 垂直) 半径，存 CSS 原始值（px/%），渲染期 resolve 成像素。
/// `/` 省略时 v = h（正圆角）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CornerRadius {
    pub h: LengthPercentage, // 水平半径
    pub v: LengthPercentage, // 垂直半径
}
impl Default for CornerRadius {
    fn default() -> Self {
        Self {
            h: LengthPercentage::Length(0.0),
            v: LengthPercentage::Length(0.0),
        }
    }
}

/// CSS border-radius 四角半径。corners 序 [TL, TR, BR, BL]（CSS 1~4 值展开序）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct BorderRadius {
    pub corners: [CornerRadius; 4],
}

impl BorderRadius {
    /// 解析四角为像素半径对 `(h, v)`，序 [TL, TR, BR, BL]（与 `mesh::rounded_rect` /
    /// `border::border_ring` 同约定）。百分比按 `(w, h)`（rect 宽/高）解析——水平半径
    /// 按宽、垂直半径按高（CSS border-radius 百分比语义）。
    pub fn as_corners(&self, w: f32, h: f32) -> [(f32, f32); 4] {
        let r = |lp: LengthPercentage, side: f32| match lp {
            LengthPercentage::Length(v) => v,
            LengthPercentage::Percent(p) => side * p,
        };
        [
            (r(self.corners[0].h, w), r(self.corners[0].v, h)),
            (r(self.corners[1].h, w), r(self.corners[1].v, h)),
            (r(self.corners[2].h, w), r(self.corners[2].v, h)),
            (r(self.corners[3].h, w), r(self.corners[3].v, h)),
        ]
    }
}

/// CSS border-image-slice 四边切片量（源图像素）。top/right/bottom/left。
/// None = 无九宫格切片；Some = 四条切片线距各边距离。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SliceInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// CSS transform 解析产物。内部存 Affine2 矩阵（非分解字段）——这样单节点
/// `scale(2,1) rotate(45deg)` 的复合剪切在解析期就保留，不因提取字段丢失。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LocalTransform {
    pub matrix: crate::transform::Affine2,
}
impl Default for LocalTransform {
    fn default() -> Self {
        Self {
            matrix: crate::transform::IDENTITY,
        }
    }
}
impl LocalTransform {
    pub fn is_identity(&self) -> bool {
        crate::transform::is_identity(&self.matrix)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedStyle {
    /// taffy 布局字段（flex/padding/margin/size/min/max/gap/position 等）
    pub taffy_style: TaffyStyle,
    /// CSS display 的 LoomGUI 旁路标记（与 taffy_style.display 解耦）。
    pub display_mode: DisplayMode,
    /// 视觉字段（不进 taffy，渲染层消费）
    pub background_color: Option<[f32; 4]>, // rgba 0..1
    /// CSS background-image url 路径（已去 url() 包裹 + 引号），None = 无背景图。
    pub background_image: Option<String>,
    /// CSS background-size 模式。默认 Stretch。
    pub background_size: BackgroundSize,
    /// CSS `background: linear-gradient(...)` 2 色渐变（4 正向）。None=纯色背景。
    /// 渐变与 background_image 互斥渲染（gradient 走 quad_gradient 顶点色插值，无纹理采样）。
    pub background_gradient: Option<Gradient2>,
    /// CSS `background-clip: text` / `-webkit-background-clip: text`。
    /// 三件套文案（background:linear-gradient + background-clip:text + color:transparent 推荐）
    /// 触发渐变字形渲染：字形 quad 顶点色用 gradient 4 角色（替代 run.color）。
    /// 缺 gradient 时静默回退普通文本（无渐变）。默认 false（不裁剪到 text）。
    pub background_clip_text: bool,
    /// CSS border-radius 四角半径。默认全 0（直角）。
    pub border_radius: BorderRadius,
    pub border_color: Option<[f32; 4]>,
    pub opacity: f32,
    /// overflow 两轴模式。Default 双轴 Visible。
    pub overflow_x: OverflowMode,
    pub overflow_y: OverflowMode,
    pub color: [f32; 4],
    pub font_size: f32,
    pub font_family: Option<String>,
    pub font_weight: u16,
    pub text_align: TextAlign,
    pub line_height: f32, // 单位倍数（1.5 = 1.5x font-size），0 = normal
    pub letter_spacing: f32,
    pub white_space_nowrap: bool,
    /// flex 顺序（CSS `order`）。taffy 0.5 Style 无此字段，存在这里由
    /// layout 在 flex 排序前消费。默认 0 = DOM 顺序。
    pub order: i32,
    /// pointer-events:auto=true / none=false（命中门控）。默认 true。
    pub touchable: bool,
    /// CSS transform 解析产物（Affine2 矩阵，含多函数复合剪切）。默认 identity。
    pub transform: crate::style::LocalTransform,
    /// CSS filter → 4×5 颜色矩阵（行主序，20 float）。None=无 filter。
    /// grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia → fgui 预设矩阵。
    pub color_filter: Option<[f32; 20]>,
    /// CSS border-image-slice 四边切片（源图像素）。None=无九宫格。
    pub border_image_slice: Option<SliceInsets>,
    /// CSS box-shadow 几何近似（无 blur）。独立 RenderNode 画在节点下层。None=无投影。
    pub box_shadow: Option<BoxShadow>,
    /// CSS transition 声明。None=未设（默认无过渡动画）。
    pub transition: Vec<TransitionSpec>,
    /// 文字效果（text-shadow / -webkit-text-stroke / font-effect 等）。
    /// CSS INHERITED 属性：父元素声明则作用于所有后代文字，故挂 style 而非 per-run。
    /// build 期按 effect 类型分 Back/Front 层注入字形渲染。空 = 无效果。
    pub text_effects: Vec<crate::text::font_effect::FontEffect>,
    /// Which inherited CSS properties were explicitly declared (set-ness bitmask).
    ///
    /// Intended to be baked at package time into base_style (rematch seeds from this
    /// each frame). **Bake wiring incomplete (Spec-2 spike):** fence `css_resolve` does
    /// not yet set bits for inline inherited declarations, so runtime
    /// `propagate_inherited` overwrites inline inherited props (color/font-size/...)
    /// with the parent value. Spec-3 unifies this with the fence schema `inherited`
    /// flag and wires the bake. See TODO in `css_resolve.rs`.
    pub inherited_set: InheritedSet,
}

/// box-shadow 几何近似（无 blur，真实 blur 推 v1.14+ 离屏 RT）。
/// MVP 用 spread=0（偏移+颜色硬边投影）；圆角阴影随圆角 SDF task 补。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoxShadow {
    pub ox: f32,
    pub oy: f32,
    pub spread: f32,
    pub color: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        // div 永远是 flex 容器，默认 flex-direction: column。
        // taffy Style::DEFAULT 是 Row，这里改默认为 Column。
        // CSS 显式声明 flex-direction 时，style::mapping::apply_decl 的对应分支
        // 无条件覆盖 ts.flex_direction——故显式声明永远胜出（写在 row 即 row）。
        let mut taffy_style = TaffyStyle::DEFAULT;
        taffy_style.flex_direction = FlexDirection::Column;
        Self {
            taffy_style,
            display_mode: DisplayMode::Flex,
            background_color: None,
            background_image: None,
            background_size: BackgroundSize::Stretch,
            background_gradient: None,
            background_clip_text: false,
            border_radius: BorderRadius::default(),
            border_color: None,
            opacity: 1.0,
            overflow_x: OverflowMode::Visible,
            overflow_y: OverflowMode::Visible,
            color: [0.0, 0.0, 0.0, 1.0],
            font_size: 16.0,
            font_family: None,
            font_weight: 400,
            text_align: TextAlign::Left,
            line_height: 0.0,
            letter_spacing: 0.0,
            white_space_nowrap: false,
            order: 0,
            touchable: true,
            transform: LocalTransform::default(),
            color_filter: None,
            border_image_slice: None,
            box_shadow: None,
            transition: Vec::new(),
            text_effects: Vec::new(),
            inherited_set: InheritedSet::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as_corners_resolves_length_and_percent() {
        // TL=10px（正圆角），TR=50%（水平按宽 200→100，垂直按高 100→50）。
        let mut br = BorderRadius::default();
        br.corners[0] = CornerRadius {
            h: LengthPercentage::Length(10.0),
            v: LengthPercentage::Length(10.0),
        };
        br.corners[1] = CornerRadius {
            h: LengthPercentage::Percent(0.5),
            v: LengthPercentage::Percent(0.5),
        };
        let c = br.as_corners(200.0, 100.0);
        assert_eq!(c[0], (10.0, 10.0), "TL px 不变");
        assert_eq!(c[1], (100.0, 50.0), "TR 50% → h=200×0.5=100, v=100×0.5=50");
        assert_eq!(c[2], (0.0, 0.0), "BR 默认 0");
    }

    #[test]
    fn default_is_sane() {
        let s = ResolvedStyle::default();
        assert_eq!(s.opacity, 1.0);
        assert_eq!(s.font_size, 16.0);
        assert_eq!(
            s.overflow_x,
            OverflowMode::Visible,
            "overflow_x 默认 Visible"
        );
        assert_eq!(
            s.overflow_y,
            OverflowMode::Visible,
            "overflow_y 默认 Visible"
        );
        // div 默认 flex-direction: column（taffy DEFAULT 是 row，必须显式覆盖）
        assert_eq!(s.taffy_style.flex_direction, taffy::FlexDirection::Column);
    }

    #[test]
    fn resolved_style_bincode_roundtrip_preserves_all_fields() {
        // 构造一个各字段都非默认的 ResolvedStyle（覆盖 taffy 字段 + 视觉字段）。
        let mut s = ResolvedStyle::default();
        s.taffy_style.flex_direction = taffy::FlexDirection::Row;
        s.taffy_style.padding = taffy::geometry::Rect::length(7.0_f32);
        s.background_color = Some([0.1, 0.2, 0.3, 0.4]);
        s.border_color = Some([0.5, 0.6, 0.7, 0.8]);
        s.opacity = 0.5;
        s.overflow_x = OverflowMode::Hidden;
        s.overflow_y = OverflowMode::Hidden;
        s.color = [1.0, 0.0, 0.0, 1.0];
        s.font_size = 48.0;
        s.font_family = Some("DejaVuSans".to_string());
        s.font_weight = 700;
        s.text_align = TextAlign::Center;
        s.line_height = 1.5;
        s.letter_spacing = 2.0;
        s.white_space_nowrap = true;
        s.order = 5;
        s.background_image = Some("icons/home.png".to_string());
        s.background_size = BackgroundSize::Cover;
        s.border_radius = BorderRadius {
            corners: [
                CornerRadius {
                    h: LengthPercentage::Length(12.0),
                    v: LengthPercentage::Length(12.0),
                },
                CornerRadius {
                    h: LengthPercentage::Length(0.0),
                    v: LengthPercentage::Length(0.0),
                },
                CornerRadius {
                    h: LengthPercentage::Percent(0.25),
                    v: LengthPercentage::Percent(0.25),
                },
                CornerRadius {
                    h: LengthPercentage::Length(4.0),
                    v: LengthPercentage::Length(2.0),
                },
            ],
        };
        s.transition = vec![TransitionSpec {
            prop: Some(crate::tween::TweenProp::Opacity),
            duration: 0.3,
            ease: crate::tween::Ease::Linear,
            delay: 0.0,
        }];
        s.text_effects = vec![crate::text::font_effect::FontEffect::Shadow {
            ox: 2.0,
            oy: 2.0,
            blur: 4.0,
            color: [0.0, 0.0, 0.0, 1.0],
        }];

        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");

        assert_eq!(back, s, "全字段经 bincode round-trip 应相等");
    }

    #[test]
    fn background_image_size_default() {
        let s = ResolvedStyle::default();
        assert_eq!(s.background_image, None, "默认无背景图");
        assert_eq!(
            s.background_size,
            BackgroundSize::Stretch,
            "默认 Stretch（100% 语义）"
        );
    }

    #[test]
    fn background_size_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        s.background_size = BackgroundSize::Contain;
        s.background_image = Some("a.png".into());
        let bytes = bincode::serialize(&s).unwrap();
        let back: ResolvedStyle = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.background_size, BackgroundSize::Contain);
        assert_eq!(back.background_image.as_deref(), Some("a.png"));
        assert_eq!(back, s, "新字段 round-trip 全等");
    }

    #[test]
    fn border_radius_default_is_zero() {
        let s = ResolvedStyle::default();
        // 默认四角全 Length(0)（直角）
        for c in &s.border_radius.corners {
            assert_eq!(c.h, LengthPercentage::Length(0.0), "默认水平半径 0");
            assert_eq!(c.v, LengthPercentage::Length(0.0), "默认垂直半径 0");
        }
    }

    #[test]
    fn border_radius_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        // 非默认：TL=(8px,8px) 正圆角，TR=(10px,5px) 椭圆角
        s.border_radius = BorderRadius {
            corners: [
                CornerRadius {
                    h: LengthPercentage::Length(8.0),
                    v: LengthPercentage::Length(8.0),
                },
                CornerRadius {
                    h: LengthPercentage::Length(10.0),
                    v: LengthPercentage::Length(5.0),
                },
                CornerRadius {
                    h: LengthPercentage::Percent(0.5),
                    v: LengthPercentage::Percent(0.5),
                },
                CornerRadius {
                    h: LengthPercentage::Length(0.0),
                    v: LengthPercentage::Length(0.0),
                },
            ],
        };
        let bytes = bincode::serialize(&s).unwrap();
        let back: ResolvedStyle = bincode::deserialize(&bytes).unwrap();
        assert_eq!(
            back.border_radius, s.border_radius,
            "border_radius 经 bincode round-trip 应相等"
        );
    }

    #[test]
    fn default_touchable_is_true() {
        assert!(
            ResolvedStyle::default().touchable,
            "touchable 默认 true（pointer-events:auto）"
        );
    }

    #[test]
    fn touchable_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        s.touchable = false;
        let bytes = bincode::serialize(&s).unwrap();
        let back: ResolvedStyle = bincode::deserialize(&bytes).unwrap();
        assert!(!back.touchable);
        assert_eq!(back, s, "加字段后全字段 round-trip 仍相等");
    }

    #[test]
    fn local_transform_default_is_identity_matrix() {
        let t = LocalTransform::default();
        assert!(t.is_identity(), "默认 transform = identity 矩阵");
    }

    #[test]
    fn resolved_style_transform_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        s.transform = LocalTransform {
            matrix: crate::transform::from_rotate(0.5),
        };
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(
            back.transform.matrix, s.transform.matrix,
            "transform 经 bincode round-trip"
        );
    }

    #[test]
    fn overflow_default_is_visible_both_axes() {
        let s = ResolvedStyle::default();
        assert_eq!(s.overflow_x, OverflowMode::Visible);
        assert_eq!(s.overflow_y, OverflowMode::Visible);
    }

    #[test]
    fn overflow_mode_is_one_byte() {
        assert_eq!(std::mem::size_of::<OverflowMode>(), 1);
    }

    #[test]
    fn overflow_hidden_bincode_roundtrip() {
        // Hidden 经 bincode round-trip 不变（pkg 字段）
        let mut s = ResolvedStyle::default();
        s.overflow_x = OverflowMode::Hidden;
        s.overflow_y = OverflowMode::Scroll;
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.overflow_x, OverflowMode::Hidden);
        assert_eq!(back.overflow_y, OverflowMode::Scroll);
        assert_eq!(back, s, "overflow 字段 round-trip 全等");
    }

    #[test]
    fn color_filter_default_is_none() {
        let s = ResolvedStyle::default();
        assert!(s.color_filter.is_none(), "默认无 color_filter");
        assert!(s.border_image_slice.is_none(), "默认无 border_image_slice");
    }

    #[test]
    fn color_filter_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        // 非单位矩阵（grayscale 预设的前 4 值非默认）
        s.color_filter = Some([
            0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0, 0.0, 0.299, 0.587, 0.114, 0.0,
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ]);
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.color_filter, s.color_filter, "color_filter round-trip");
        assert_eq!(back, s, "加字段后全字段 round-trip 仍相等");
    }

    #[test]
    fn border_image_slice_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        s.border_image_slice = Some(SliceInsets {
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        });
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(
            back.border_image_slice, s.border_image_slice,
            "slice round-trip"
        );
        assert_eq!(back, s, "加字段后全字段 round-trip 仍相等");
    }

    #[test]
    fn background_gradient_default_is_none() {
        assert!(
            ResolvedStyle::default().background_gradient.is_none(),
            "默认无渐变"
        );
    }

    #[test]
    fn background_gradient_bincode_roundtrip() {
        // 4 方向 × 2 色经 bincode round-trip 等值（pkg 字段，序列化稳定）。
        for dir in [
            GradientDir::ToRight,
            GradientDir::ToLeft,
            GradientDir::ToTop,
            GradientDir::ToBottom,
        ] {
            let mut s = ResolvedStyle::default();
            s.background_gradient = Some(Gradient2 {
                color_a: [0.1, 0.2, 0.3, 0.4],
                color_b: [0.5, 0.6, 0.7, 0.8],
                dir,
            });
            let bytes = bincode::serialize(&s).expect("serialize");
            let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
            assert_eq!(
                back.background_gradient, s.background_gradient,
                "dir={dir:?} round-trip"
            );
            assert_eq!(back, s, "全字段 round-trip 仍相等");
        }
    }

    #[test]
    fn gradient_dir_is_one_byte() {
        // FFI / 序列化稳定不变量：#[repr(u8)] enum 占 1 字节。
        assert_eq!(std::mem::size_of::<GradientDir>(), 1);
    }

    #[test]
    fn background_clip_text_default_is_false() {
        assert!(
            !ResolvedStyle::default().background_clip_text,
            "默认 background_clip_text = false"
        );
    }

    #[test]
    fn background_clip_text_bincode_roundtrip() {
        for v in [false, true] {
            let mut s = ResolvedStyle::default();
            s.background_clip_text = v;
            let bytes = bincode::serialize(&s).expect("serialize");
            let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
            assert_eq!(
                back.background_clip_text, v,
                "background_clip_text round-trip {v}"
            );
            assert_eq!(back, s, "全字段 round-trip 仍相等 ({v})");
        }
    }

    #[test]
    fn inherited_set_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        s.inherited_set = InheritedSet(0b0000_0011); // font-size + color set
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.inherited_set, s.inherited_set);
        assert_eq!(back, s, "full round-trip equal");
    }
}
