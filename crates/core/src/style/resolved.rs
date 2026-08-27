use serde::{Deserialize, Serialize};

/// Tracks which inherited CSS properties were explicitly declared (set-ness bitmask).
/// Each bit corresponds to one inheritable property (see INH_* constants in dynamic.rs).
/// Baked at package time into base_style; rematch reads it as the per-frame inheritance baseline.
/// u64 与 InlineSet 同位宽：INH_* bits 0-7 之后的新继承属性（overflow-wrap 等）落在 bits 33+
/// （bits 8-32 被 INLINE_* 非继承属性占用，复用会撞位）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InheritedSet(pub u64);

use taffy::style::LengthPercentage;
use taffy::style::Style as TaffyStyle;

/// CSS `white-space` 换行控制全集（#73）。三轴正交：
/// 空白折叠（Normal/Nowrap/PreLine 折、Pre/PreWrap 留）× 自动换行（Nowrap/Pre 关）
/// × 源换行保留（Pre/PreWrap/PreLine 留 `\n` 断行，Normal/Nowrap 折成空格）。
/// 消费点：`crate::text::layout::WrapControl`（measure_text/measure_rich_text 断行器）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

/// CSS `overflow-wrap`（#73）。`break-word`：词独行仍超行宽才逐字拆
/// （`normal` 词超宽 = 溢出，不拆——CSS 语义，浏览器一致）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OverflowWrap {
    #[default]
    Normal,
    BreakWord,
}

/// CSS `word-break`（#73）。`break-all`：任意字符间可断（拉丁词也逐字）；
/// `keep-all`：CJK 词内不断（只退到空格/标点边界断）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
}

/// CSS `text-wrap`（CSS Text 4 子集，#73）。只收 `nowrap`（关自动换行、
/// 保留 white-space 的空白语义）；`balance`/`stable`/`pretty` 围栏拒绝
/// （fence schema 值集即拒绝，deferred：text-align 替代不了的标题平衡换行狗粮场景再收）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextWrap {
    #[default]
    Normal,
    Nowrap,
}

/// CSS `-webkit-text-security` 的掩码形状（password 类输入的显示变换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextSecurity {
    /// 实心圆点 `●`（浏览器 disc 默认形态）。
    Disc,
    /// 空心圆 `○`。
    Circle,
    /// 实心方块 `■`。
    Square,
}

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

/// CSS `animation-direction`。`#[repr(u8)]` 保 FFI/序列化稳定，Default = Normal。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum AnimationDirection {
    #[default]
    Normal = 0,
    Reverse = 1,
    Alternate = 2,
    AlternateReverse = 3,
}

/// CSS `animation-fill-mode`。`#[repr(u8)]` 保 FFI/序列化稳定，Default = None。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum AnimationFillMode {
    #[default]
    None = 0,
    Forwards = 1,
    Backwards = 2,
    Both = 3,
}

/// CSS `animation-play-state`。`#[repr(u8)]` 保 FFI/序列化稳定，Default = Running。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum AnimationPlayState {
    #[default]
    Running = 0,
    Paused = 1,
}

/// CSS animation 声明（单条；`animation` 简写逗号分隔展开为多条）。
/// `name` 引用 Scene.keyframes 全局表（CSS `@keyframes` 全局查找语义）。
/// 镜像 TransitionSpec 模式：同 derive、同序列化路径（ResolvedStyle bincode blob）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationSpec {
    pub name: String,
    /// 动画时长（秒）
    pub duration: f32,
    /// 延迟（秒）
    pub delay: f32,
    /// None = infinite（CSS `iteration-count: infinite`）
    pub iteration_count: Option<u32>,
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
    /// easing 函数（复用 tween::Ease，含 steps() 的 Step 变体）
    pub timing_function: crate::tween::Ease,
    pub play_state: AnimationPlayState,
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

/// CSS `background-repeat`。默认 Repeat（CSS 初始值）——图小于盒时平铺填满；
/// NoRepeat 单张；RepeatX/RepeatY 仅横向/纵向。此前 core 渲染单张（等价 NoRepeat），
/// 与 CSS 默认 Repeat 分歧（标本馆 bg-contain HTML 平铺填盒、Unity 单张 80×80 根因）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum BackgroundRepeat {
    #[default]
    Repeat = 0,
    NoRepeat = 1,
    RepeatX = 2,
    RepeatY = 3,
}

/// 渐变 stop。`pos` 在解析期按 CSS 规则烘成 0..1 定位（首 0 / 末 1 / 中间相邻已定位
/// stop 的中点），并钳成单调不减——渲染层无需再处理默认位置。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub color: [f32; 4],
    pub pos: f32,
}

/// radial 渐变的尺寸。关键字在渲染期按 box 解析；显式长度（单长度=正圆、双长度=椭圆）
/// 已是像素。围栏子集不含 `<percentage>` 尺寸（CSS 规范允许但游戏 UI 无场景）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RadialExtent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    Explicit(Option<f32>, Option<f32>),
}

/// radial 圆心单轴坐标。Pct 按 box 尺寸解析（CSS `at 82% -12%`），Px 为像素。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GradCoord {
    Pct(f32),
    Px(f32),
}

/// CSS 渐变（背景填充 + background-clip:text 文本渐变共用）。
/// Linear 的方向在解析期归一化为角度（0deg=to top 顺时针；`to right`=90）。
/// 渲染期按当帧 box 解析成像素参数（`render::gradient::GradientParams`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Gradient {
    Linear {
        angle_deg: f32,
        stops: Vec<GradientStop>,
    },
    Radial {
        extent: RadialExtent,
        /// circle 收敛为单一半径（closest/farthest 语义与椭圆逐轴不同）。
        /// pkg 走 bincode（非自描述）：字段增删必 bump PKG_FORMAT_VERSION，
        /// `#[serde(default)]` 对 bincode 反序列化不生效（无缺字段路径），
        /// 仅对 JSON 类自描述格式有用。
        #[serde(default)]
        shape: RadialShape,
        center: [GradCoord; 2],
        stops: Vec<GradientStop>,
    },
}

/// radial 渐变形状（CSS `circle` / `ellipse`；缺省 ellipse）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RadialShape {
    #[default]
    Ellipse,
    Circle,
}

/// 渐变 stop 上限（FFI grad_params 列定长 8 槽；超出打包期拒收）。
pub const GRADIENT_MAX_STOPS: usize = 8;

impl Gradient {
    /// stop 列表访问器（linear/radial 同构）。
    pub fn stops(&self) -> &[GradientStop] {
        match self {
            Gradient::Linear { stops, .. } | Gradient::Radial { stops, .. } => stops,
        }
    }
}

/// LoomGUI display 旁路字段（与 taffy_style.display 并行设置）。
///
/// `display_mode` 让内部 Strategy 选择（Block vs Flex scrolling/text alignment
/// 分支）不依赖 taffy 模式枚举。`taffy_style.display` 同步设置——block
/// 标签和 `display:block` 都走 taffy `Display::Block`（真 CSS 块流，垂直堆叠且
/// 忽略子元素 flex-grow）。inline 走 Flex Row，none 走 None。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum DisplayMode {
    #[default]
    Flex = 0,
    Block = 1,
    None = 2,
}

/// 作者声明的 `position` 值（区别于 taffy 枚举的关键：taffy `Position::Relative` 是
/// 布局默认值，「声明了 relative」与「从未声明」在 taffy 侧不可区分）。布局层用它在
/// 建树时识别 positioned 节点——absolute 子项的包含块 = 最近 positioned 祖先
/// （CSS 浏览器语义；声明 relative/absolute 即成为后代的包含块候选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum PositionDeclared {
    #[default]
    Static = 0,
    Relative = 1,
    Absolute = 2,
}

/// 视口相对单位（`vw`/`vh`/`vmin`/`vmax`）。分母 = Stage root_size（画布），
/// 区别于 `%`（相对父容器）。solve 时按当帧 root_size 换算成 px 再喂 taffy——
/// taffy CompactLength 只有 length/percent/auto 三 tag，装不下第四种，故走
/// 平行字段（同 `position_declared` 先例）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewportUnit {
    Vw,
    Vh,
    Vmin,
    Vmax,
}

/// 一个视口相对长度，如 `2.5vh`。`value` 为百分数值（2.5vh 存 2.5）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewportLen {
    pub value: f32,
    pub unit: ViewportUnit,
}

impl ViewportLen {
    /// 按画布尺寸换算成 px：vw/vh 取对应维，vmin/vmax 取两维较小/较大者。
    pub fn resolve(&self, root: (f32, f32)) -> f32 {
        let v = self.value / 100.0;
        match self.unit {
            ViewportUnit::Vw => v * root.0,
            ViewportUnit::Vh => v * root.1,
            ViewportUnit::Vmin => v * root.0.min(root.1),
            ViewportUnit::Vmax => v * root.0.max(root.1),
        }
    }
}

/// 节点的视口相对长度声明集。全部 `None` = 无视口单位（Default）。
/// 约束：声明 px/% 会清掉同通道的视口覆盖（CSS 级联后者胜出语义），
/// 由 `apply_decl` 各臂维护；布局建树时 `apply` 用当帧 root_size 覆写 taffy 副本。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ViewportStyle {
    pub width: Option<ViewportLen>,
    pub height: Option<ViewportLen>,
    pub min_width: Option<ViewportLen>,
    pub min_height: Option<ViewportLen>,
    pub max_width: Option<ViewportLen>,
    pub max_height: Option<ViewportLen>,
    pub flex_basis: Option<ViewportLen>,
    /// inset 四边 [top, right, bottom, left]
    pub inset: [Option<ViewportLen>; 4],
    /// margin 四边 [top, right, bottom, left]
    pub margin: [Option<ViewportLen>; 4],
}

impl ViewportStyle {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// 把视口声明按 `root` 换算后覆写到 taffy 副本上（solve 建树期调用；
    /// 只动有声明的通道，未声明通道保持 taffy 原值）。
    pub fn apply(&self, style: &mut TaffyStyle, root: (f32, f32)) {
        let dim =
            |v: &Option<ViewportLen>| v.map(|v| taffy::style::Dimension::length(v.resolve(root)));
        if let Some(v) = dim(&self.width) {
            style.size.width = v;
        }
        if let Some(v) = dim(&self.height) {
            style.size.height = v;
        }
        if let Some(v) = dim(&self.min_width) {
            style.min_size.width = v;
        }
        if let Some(v) = dim(&self.min_height) {
            style.min_size.height = v;
        }
        if let Some(v) = dim(&self.max_width) {
            style.max_size.width = v;
        }
        if let Some(v) = dim(&self.max_height) {
            style.max_size.height = v;
        }
        if let Some(v) = dim(&self.flex_basis) {
            style.flex_basis = v;
        }
        let lpa = |v: &Option<ViewportLen>| {
            v.map(|v| taffy::style::LengthPercentageAuto::length(v.resolve(root)))
        };
        if let Some(v) = lpa(&self.inset[0]) {
            style.inset.top = v;
        }
        if let Some(v) = lpa(&self.inset[1]) {
            style.inset.right = v;
        }
        if let Some(v) = lpa(&self.inset[2]) {
            style.inset.bottom = v;
        }
        if let Some(v) = lpa(&self.inset[3]) {
            style.inset.left = v;
        }
        if let Some(v) = lpa(&self.margin[0]) {
            style.margin.top = v;
        }
        if let Some(v) = lpa(&self.margin[1]) {
            style.margin.right = v;
        }
        if let Some(v) = lpa(&self.margin[2]) {
            style.margin.bottom = v;
        }
        if let Some(v) = lpa(&self.margin[3]) {
            style.margin.left = v;
        }
    }
}

/// CSS border-radius 单角半径。
/// (h, v) = (水平, 垂直) 半径，存 CSS 原始值（px/%），渲染期 resolve 成像素。
/// `/` 省略时 v = h（正圆角）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CornerRadius {
    pub h: LengthPercentage,
    pub v: LengthPercentage,
}
impl Default for CornerRadius {
    fn default() -> Self {
        Self {
            h: LengthPercentage::length(0.0),
            v: LengthPercentage::length(0.0),
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
        // taffy 0.12：LengthPercentage 是 pub struct(CompactLength) tagged pointer，
        // 内字段私有无法 match 变体——用 into_raw + tag 解构。
        let r = |lp: LengthPercentage, side: f32| {
            let cl = lp.into_raw();
            match cl.tag() {
                taffy::style::CompactLength::LENGTH_TAG => cl.value(),
                taffy::style::CompactLength::PERCENT_TAG => side * cl.value(),
                _ => 0.0,
            }
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

/// CSS border-style：控制边框线型。None=不渲染（CSS initial），其余=渲染对应线型。
/// 门控 render 层的 border 调用（None 时不画，对齐 CSS 规范默认值语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// CSS `transform-origin` 描述符：`<length-percentage>` × 2（x y），默认 50% 50%。
///
/// **延迟解析**：百分比相对节点布局尺寸，打包期无尺寸 → 存描述符，世界矩阵累计
/// （`compute_world_transforms`）按当帧 `layout_rect` 解析成 pivot 点。default 50% 50%
/// = 既有硬编码盒心 pivot，未声明时零行为变化。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransformOrigin {
    pub x: crate::transform::LenPct,
    pub y: crate::transform::LenPct,
}

impl Default for TransformOrigin {
    /// CSS 初始值 `50% 50%`（元素几何中心）。
    fn default() -> Self {
        TransformOrigin {
            x: crate::transform::LenPct { px: 0.0, pct: 50.0 },
            y: crate::transform::LenPct { px: 0.0, pct: 50.0 },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedStyle {
    /// taffy 布局字段（flex/padding/margin/size/min/max/gap/position 等）
    pub taffy_style: TaffyStyle,
    /// 作者声明的 position（Static=未声明）。与 taffy_style.position 并行设置——
    /// 见 [`PositionDeclared`]：布局层据此识别 absolute 子项的包含块候选。
    pub position_declared: PositionDeclared,
    /// 视口相对长度声明（vw/vh/vmin/vmax）。与 taffy_style 并行——taffy 装不下
    /// 第四种长度 tag，solve 建树期按 root_size 换算覆写（见 [`ViewportStyle`]）。
    pub viewport: ViewportStyle,
    /// CSS display 的 LoomGUI 旁路标记（与 taffy_style.display 解耦）。
    pub display_mode: DisplayMode,
    /// 视觉字段（不进 taffy，渲染层消费）
    pub background_color: Option<[f32; 4]>, // rgba 0..1
    /// CSS background-image url 路径（已去 url() 包裹 + 引号），None = 无背景图。
    pub background_image: Option<String>,
    /// CSS background-size 模式。默认 Stretch。
    pub background_size: BackgroundSize,
    /// CSS background-repeat（默认 Repeat）。render 对图小于盒时按此平铺。
    pub background_repeat: BackgroundRepeat,
    /// CSS `background: <gradient>` 渐变（linear/radial，多 stop + 任意角度）。
    /// None=纯色背景。与 background_image 互斥渲染（gradient 走 program=6 渐变 shader）。
    pub background_gradient: Option<Gradient>,
    /// CSS `background-clip: text` / `-webkit-background-clip: text`。
    /// 三件套文案（background:linear-gradient + background-clip:text + color:transparent 推荐）
    /// 触发渐变字形渲染：字形 quad 顶点色用 gradient 4 角色（替代 run.color）。
    /// 缺 gradient 时静默回退普通文本（无渐变）。默认 false（不裁剪到 text）。
    pub background_clip_text: bool,
    /// CSS border-radius 四角半径。默认全 0（直角）。
    pub border_radius: BorderRadius,
    pub border_color: Option<[f32; 4]>,
    /// CSS border-style：None=不画边框（CSS initial 值）。门控 border 渲染。
    pub border_style: BorderStyle,
    pub opacity: f32,
    /// overflow 两轴模式。Default 双轴 Visible。
    pub overflow_x: OverflowMode,
    pub overflow_y: OverflowMode,
    pub color: [f32; 4],
    /// CSS `caret-color`（TextField/TextArea 光标色）。None = 缺省回退到 `color`
    /// （render arm `unwrap_or(s.color)`），与 CSS `caret-color: auto` 语义一致。
    /// CSS INHERITED 属性（照 CSS 规范），打包期 bake 进 base_style。
    pub caret_color: Option<[f32; 4]>,
    /// CSS `selection-background`（选中文本的背景色）。None = 缺省回退蓝半透
    /// `[0,0,1,0.5]`（render arm fallback）。LoomGUI 私有属性
    /// （CSS 用 `::selection { background }`，围栏无伪元素选择器，故用平铺 prop）。
    pub selection_background: Option<[f32; 4]>,
    /// CSS `selection-color`（选中文本的文字色）。None = 缺省回退白色（render arm fallback）。
    /// 同 `selection_background`，LoomGUI 私有属性（CSS `::selection { color }`）。
    pub selection_color: Option<[f32; 4]>,
    /// 占位符渲染色（CSS `::placeholder { color }`，围栏无伪元素选择器，故平铺 prop）。
    /// None = 缺省把 `color` alpha 折半（对齐浏览器 ::placeholder UA 默认 ~opacity 0.5）。
    /// 颜色在 layout solve 期烘焙进缓存 TextLayout 的 per-run 色，故 layout 与 render 须
    /// 一致用此字段（见 `placeholder_render_color`）——render 单独改色会被缓存覆盖。
    pub placeholder_color: Option<[f32; 4]>,
    /// CSS `-webkit-text-security`（掩码显示，password 类输入）。None = 不掩码。
    /// 作用于文本控件的显示串（`display_value_masked`），不改变 value 本身。
    pub text_security: Option<TextSecurity>,
    pub font_size: f32,
    pub font_family: Option<String>,
    pub font_weight: u16,
    pub text_align: TextAlign,
    /// CSS `line-height` 的倍数形（`1.5` = 1.5×font-size）。0 = normal。
    pub line_height: f32,
    /// CSS `line-height` 的长度形（`27px`）：绝对行高，继承为 px 本身（CSS computed
    /// 语义），消费点按本元素 font_size 经 [`Self::effective_line_height`] 换算。
    /// None = 未声明长度形。两槽互斥：mapping 写其一，另一槽保持默认。
    pub line_height_px: Option<f32>,
    pub letter_spacing: f32,
    /// CSS `white-space`（#73 换行控制全集，继承）。
    pub white_space: WhiteSpace,
    /// CSS `overflow-wrap`（#73，继承）。
    pub overflow_wrap: OverflowWrap,
    /// CSS `word-break`（#73，继承）。
    pub word_break: WordBreak,
    /// CSS `text-wrap`（#73，继承，只收 nowrap）。
    pub text_wrap: TextWrap,
    /// flex 顺序（CSS `order`）。taffy Style 无此字段，存在这里由
    /// layout 在 flex 排序前消费。默认 0 = DOM 顺序。
    pub order: i32,
    /// 层叠序（CSS `z-index`）：只改同级兄弟间绘制/命中顺序（z 升序绘制、
    /// 子树整体移动），不改 flex 排列（那是 `order`）。默认 0。消费点：render
    /// DFS 子迭代 + open popup 追加循环 + hit `effective_draw_order`（三处同步）。
    pub z_index: i32,
    /// pointer-events:auto=true / none=false（命中门控）。默认 true。
    pub touchable: bool,
    /// CSS transform 解析产物（Affine2 矩阵，含多函数复合剪切）。默认 identity。
    pub transform: crate::style::LocalTransform,
    /// CSS `transform-origin`（#21 CSS 半边）：`<length|%>` × 2 描述符，默认 50% 50%
    /// （盒心 = 既有硬编码 pivot，零回归）。compute_world_transforms 按布局尺寸延迟解析。
    pub transform_origin: TransformOrigin,
    /// CSS filter → 4×5 颜色矩阵（行主序，20 float）。None=无 filter。
    /// grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia → fgui 预设矩阵。
    pub color_filter: Option<[f32; 20]>,
    /// CSS border-image-slice 四边切片（源图像素）。None=无九宫格。
    pub border_image_slice: Option<SliceInsets>,
    /// CSS box-shadow 列表（源序）。空 Vec = 无投影。每层独立 RenderNode：outer 画在节点
    /// 下层、inset 画在节点上层；blur>0 走 SDF 高斯边 shader。blur/inset/多层语义见各层渲染。
    pub box_shadow: Vec<BoxShadow>,
    /// CSS transition 声明。None=未设（默认无过渡动画）。
    pub transition: Vec<TransitionSpec>,
    /// CSS animation 声明（`animation` 简写，逗号分隔多声明）。name 引用 Scene.keyframes
    /// 全局表。空 = 无动画。与 transition 并列（base_style bake，进 pkg bincode blob）。
    pub animation: Vec<AnimationSpec>,
    /// 文字效果（text-shadow / -webkit-text-stroke / font-effect 等）。
    /// CSS INHERITED 属性：父元素声明则作用于所有后代文字，故挂 style 而非 per-run。
    /// build 期按 effect 类型分 Back/Front 层注入字形渲染。空 = 无效果。
    pub text_effects: Vec<crate::text::font_effect::FontEffect>,
    /// Which inherited CSS properties were explicitly declared (set-ness bitmask).
    ///
    /// Baked at package time into base_style (rematch seeds from this each frame).
    /// Fence `css_resolve` calls `loomgui_core::style::dynamic::inherited_bit` after
    /// `apply_decl` success and OR's the bit into this field, so runtime
    /// `propagate_inherited` respects inline inherited declarations and does not
    /// overwrite them with the parent value.
    pub inherited_set: InheritedSet,

    /// Which CSS properties were declared via inline `style="..."` at package time
    /// (INLINE_* bitmask, see `dynamic::inline_bit`). Baked into base_style.
    ///
    /// CSS cascade: an inline style attribute beats class rules. But fence bakes inline
    /// declarations into base_style, while `<style>` class rules become dynamic_rules
    /// applied later at runtime rematch — so without this bitmask a class rule would
    /// overwrite the inline value (priority inverted). rematch skips class declarations
    /// whose `inline_bit` is set here, restoring inline > class.
    pub inline_declared: u64,
}

/// box-shadow 每类层数硬限。render 合成节点把层类型+层内 idx 编码进 node_id high byte
/// （inset 36..=43、outer 44..=47，见 render::FRONT/BACK_SHADOW_SYNTH_BYTE），超限层的
/// id 会撞相邻编码区 → 错层序/漏 mask 传播。parse 层据此拒收超限声明（fence 打包期报错），
/// render push 兜底跳过超限层（运行时 inline override 注入不经打包期校验）。
pub const MAX_INSET_SHADOW_LAYERS: usize = 8;
pub const MAX_OUTER_SHADOW_LAYERS: usize = 4;

/// 单层 CSS box-shadow。多声明逗号分隔各成一层（`ResolvedStyle.box_shadow: Vec`，CSS 源序）。
/// blur=0 硬边（实心圆角矩形）；blur>0 走 SDF 高斯边（σ=blur/2 运行时算）。inset 画在节点内。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoxShadow {
    pub ox: f32,
    pub oy: f32,
    pub spread: f32,
    /// blur_radius（CSS px）。运行时 σ=blur/2。
    pub blur: f32,
    pub color: [f32; 4],
    pub inset: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// 占位符渲染色：声明了 `placeholder-color` 用之，否则把传入的 `text_color` alpha 折半
/// （对齐浏览器 ::placeholder UA 默认 ~opacity 0.5）。layout（控件 measure）与 render
/// （文本控件臂）共用，保证缓存 TextLayout 的 per-run 色与 render fallback 一致。
/// `text_color` 由调用方决定（layout 传 style.color；render 可传 anim 覆盖后的色）。
pub fn placeholder_render_color(declared: Option<[f32; 4]>, text_color: [f32; 4]) -> [f32; 4] {
    declared.unwrap_or_else(|| {
        [
            text_color[0],
            text_color[1],
            text_color[2],
            text_color[3] * 0.5,
        ]
    })
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        // CSS spec: flex-direction initial value is `row`. taffy Style::DEFAULT
        // is already Row — we used to override to Column for the legacy v1
        // "div is always a flex container" model, but that was removed (div
        // is real CSS block flow now). Keeping the Column default broke
        // containers whose display:flex comes from a <style> class rule (applied
        // at runtime rematch, past stage-4's row-override) — they stacked
        // vertically instead of flowing in a row. Default now matches the CSS
        // initial value (Row), same as taffy's own default.
        Self {
            taffy_style: TaffyStyle::DEFAULT,
            position_declared: PositionDeclared::Static,
            viewport: ViewportStyle::default(),
            // display fields (display_mode + taffy_style.display) here are Flex
            // (= taffy's own DEFAULT). They do NOT decide a real node's display:
            //   - packed (pkg) nodes: css_resolve bakes the tag's DisplayDefault
            //     into base_style at pack time.
            //   - runtime create_node: default_display_for_kind overrides these
            //     before apply_css (div→Block, button/img/span→Flex).
            // So this default only matters for hand-built test fixtures.
            display_mode: DisplayMode::Flex,
            background_color: None,
            background_image: None,
            background_size: BackgroundSize::Stretch,
            background_repeat: BackgroundRepeat::Repeat,
            background_gradient: None,
            background_clip_text: false,
            border_radius: BorderRadius::default(),
            border_color: None,
            border_style: BorderStyle::None,
            opacity: 1.0,
            overflow_x: OverflowMode::Visible,
            overflow_y: OverflowMode::Visible,
            color: [0.0, 0.0, 0.0, 1.0],
            caret_color: None,
            selection_background: None,
            selection_color: None,
            placeholder_color: None,
            text_security: None,
            font_size: 16.0,
            font_family: None,
            font_weight: 400,
            text_align: TextAlign::Left,
            line_height: 0.0,
            line_height_px: None,
            letter_spacing: 0.0,
            white_space: WhiteSpace::Normal,
            overflow_wrap: OverflowWrap::Normal,
            word_break: WordBreak::Normal,
            text_wrap: TextWrap::Normal,
            order: 0,
            z_index: 0,
            touchable: true,
            transform: LocalTransform::default(),
            transform_origin: TransformOrigin::default(),
            color_filter: None,
            border_image_slice: None,
            box_shadow: Vec::new(),
            transition: Vec::new(),
            animation: Vec::new(),
            text_effects: Vec::new(),
            inherited_set: InheritedSet::default(),
            inline_declared: 0,
        }
    }
}

impl ResolvedStyle {
    /// 有效行高倍数：px 形按本元素 font_size 换算（27px @17px → 1.588×），倍数形
    /// 原样返回。文本度量/渲染/光标的**唯一**取用入口——两槽并存（px 继承为 px、
    /// number 继承为 number，CSS computed 语义），直读 `line_height` 会漏 px 形。
    /// px≤0 或 font_size≤0 视为无效回退倍数槽。
    pub fn effective_line_height(&self) -> f32 {
        match self.line_height_px {
            Some(px) if px > 0.0 && self.font_size > 0.0 => px / self.font_size,
            _ => self.line_height,
        }
    }

    /// 静态文本断行控制（#73）：四个换行属性打包成断行器消费的 `WrapControl`。
    /// 文本控件（TextField/TextArea/NumberField）不走这里——空格折叠会破坏
    /// 光标字节↔布局 1:1 映射，控件侧用 `control_wrap_control` 冻结空白语义。
    pub fn wrap_control(&self) -> crate::text::layout::WrapControl {
        crate::text::layout::WrapControl {
            white_space: self.white_space,
            overflow_wrap: self.overflow_wrap,
            word_break: self.word_break,
            text_wrap: self.text_wrap,
        }
    }
}

/// 文本控件的断行控制：空白语义冻结为 pre 系（空格/换行原样保留——折叠会破坏
/// 光标字节↔布局 1:1 映射，CSS UA 对 input/textarea 的 white-space 也是 pre 系），
/// 换行开关尊重声明（white-space:nowrap / text-wrap:nowrap → 关自动换行）。
/// word-break/overflow-wrap 照常尊重（不断不删字符，光标映射不受影响）。
pub fn control_wrap_control(s: &ResolvedStyle) -> crate::text::layout::WrapControl {
    let mut wc = s.wrap_control();
    let wrap_off = matches!(s.white_space, WhiteSpace::Nowrap | WhiteSpace::Pre);
    wc.white_space = if wrap_off {
        WhiteSpace::Pre
    } else {
        WhiteSpace::PreWrap
    };
    wc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as_corners_resolves_length_and_percent() {
        // TL=10px（正圆角），TR=50%（水平按宽 200→100，垂直按高 100→50）。
        let mut br = BorderRadius::default();
        br.corners[0] = CornerRadius {
            h: LengthPercentage::length(10.0),
            v: LengthPercentage::length(10.0),
        };
        br.corners[1] = CornerRadius {
            h: LengthPercentage::percent(0.5),
            v: LengthPercentage::percent(0.5),
        };
        let c = br.as_corners(200.0, 100.0);
        assert_eq!(c[0], (10.0, 10.0), "TL px 不变");
        assert_eq!(c[1], (100.0, 50.0), "TR 50% → h=200×0.5=100, v=100×0.5=50");
        assert_eq!(c[2], (0.0, 0.0), "BR 默认 0");
    }

    #[test]
    fn border_style_defaults_to_none() {
        // CSS initial value of border-style is `none` (no border drawn). The render
        // layer gates border drawing on this field, so the default must be None to
        // match CSS semantics for nodes that declare no border-style.
        let s = ResolvedStyle::default();
        assert_eq!(s.border_style, BorderStyle::None);
    }

    #[test]
    fn border_style_bincode_roundtrip() {
        // border_style is a pkg field (ResolvedStyle is bincode-serialized into pkg.bin).
        // #[repr(u8)] keeps the on-disk layout stable and compact.
        for style in [
            BorderStyle::None,
            BorderStyle::Solid,
            BorderStyle::Dashed,
            BorderStyle::Dotted,
            BorderStyle::Double,
        ] {
            let mut s = ResolvedStyle::default();
            s.border_style = style;
            let bytes = bincode::serialize(&s).expect("serialize");
            let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
            assert_eq!(back.border_style, style, "{style:?} round-trip");
            assert_eq!(back, s, "全字段 round-trip 仍相等 ({style:?})");
        }
    }

    #[test]
    fn border_style_is_one_byte() {
        // FFI / 序列化稳定不变量：#[repr(u8)] enum 占 1 字节（与 OverflowMode 同模式）。
        assert_eq!(std::mem::size_of::<BorderStyle>(), 1);
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
        // flex-direction 默认 = CSS 初始值 row（= taffy DEFAULT）。
        assert_eq!(s.taffy_style.flex_direction, taffy::FlexDirection::Row);
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
        s.white_space = WhiteSpace::Nowrap;
        s.order = 5;
        s.background_image = Some("icons/home.png".to_string());
        s.background_size = BackgroundSize::Cover;
        s.border_radius = BorderRadius {
            corners: [
                CornerRadius {
                    h: LengthPercentage::length(12.0),
                    v: LengthPercentage::length(12.0),
                },
                CornerRadius {
                    h: LengthPercentage::length(0.0),
                    v: LengthPercentage::length(0.0),
                },
                CornerRadius {
                    h: LengthPercentage::percent(0.25),
                    v: LengthPercentage::percent(0.25),
                },
                CornerRadius {
                    h: LengthPercentage::length(4.0),
                    v: LengthPercentage::length(2.0),
                },
            ],
        };
        s.transition = vec![TransitionSpec {
            prop: Some(crate::tween::TweenProp::Opacity),
            duration: 0.3,
            ease: crate::tween::Ease::Linear,
            delay: 0.0,
        }];
        s.animation = vec![AnimationSpec {
            name: "fadeIn".into(),
            duration: 0.5,
            delay: 0.1,
            iteration_count: None,
            direction: AnimationDirection::AlternateReverse,
            fill_mode: AnimationFillMode::Both,
            timing_function: crate::tween::Ease::Step { start: true },
            play_state: AnimationPlayState::Paused,
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
    fn animation_enums_are_one_byte() {
        // FFI / 序列化稳定不变量：#[repr(u8)] enum 占 1 字节（与 OverflowMode 同模式）。
        assert_eq!(std::mem::size_of::<AnimationDirection>(), 1);
        assert_eq!(std::mem::size_of::<AnimationFillMode>(), 1);
        assert_eq!(std::mem::size_of::<AnimationPlayState>(), 1);
    }

    #[test]
    fn animation_enums_defaults_are_semantic() {
        // Default 按 CSS 语义：direction=Normal / fill-mode=None / play-state=Running。
        assert_eq!(AnimationDirection::default(), AnimationDirection::Normal);
        assert_eq!(AnimationFillMode::default(), AnimationFillMode::None);
        assert_eq!(AnimationPlayState::default(), AnimationPlayState::Running);
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
            assert_eq!(c.h, LengthPercentage::length(0.0), "默认水平半径 0");
            assert_eq!(c.v, LengthPercentage::length(0.0), "默认垂直半径 0");
        }
    }

    #[test]
    fn border_radius_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        // 非默认：TL=(8px,8px) 正圆角，TR=(10px,5px) 椭圆角
        s.border_radius = BorderRadius {
            corners: [
                CornerRadius {
                    h: LengthPercentage::length(8.0),
                    v: LengthPercentage::length(8.0),
                },
                CornerRadius {
                    h: LengthPercentage::length(10.0),
                    v: LengthPercentage::length(5.0),
                },
                CornerRadius {
                    h: LengthPercentage::percent(0.5),
                    v: LengthPercentage::percent(0.5),
                },
                CornerRadius {
                    h: LengthPercentage::length(0.0),
                    v: LengthPercentage::length(0.0),
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
        // linear（角度 + 多 stop）与 radial（extent + at 坐标 + 多 stop）经 bincode
        // round-trip 等值（pkg 字段，序列化稳定）。
        let cases = vec![
            Gradient::Linear {
                angle_deg: 137.0,
                stops: vec![
                    GradientStop {
                        color: [0.1, 0.2, 0.3, 0.4],
                        pos: 0.0,
                    },
                    GradientStop {
                        color: [0.5, 0.6, 0.7, 0.8],
                        pos: 1.0,
                    },
                ],
            },
            Gradient::Linear {
                angle_deg: 0.0,
                stops: vec![
                    GradientStop {
                        color: [1.0, 0.0, 0.0, 1.0],
                        pos: 0.0,
                    },
                    GradientStop {
                        color: [0.0, 1.0, 0.0, 0.5],
                        pos: 0.25,
                    },
                    GradientStop {
                        color: [0.0, 0.0, 1.0, 1.0],
                        pos: 1.0,
                    },
                ],
            },
            Gradient::Radial {
                extent: RadialExtent::Explicit(Some(1100.0), Some(560.0)),
                shape: RadialShape::Ellipse,
                center: [GradCoord::Pct(0.82), GradCoord::Pct(-0.12)],
                stops: vec![
                    GradientStop {
                        color: [0.37, 0.71, 0.83, 0.1],
                        pos: 0.0,
                    },
                    GradientStop {
                        color: [0.0; 4],
                        pos: 0.6,
                    },
                ],
            },
            Gradient::Radial {
                extent: RadialExtent::ClosestSide,
                shape: RadialShape::Circle,
                center: [GradCoord::Pct(0.5), GradCoord::Px(40.0)],
                stops: vec![GradientStop {
                    color: [0.9, 0.9, 0.1, 1.0],
                    pos: 0.5,
                }],
            },
        ];
        for g in cases {
            let mut s = ResolvedStyle::default();
            s.background_gradient = Some(g);
            let bytes = bincode::serialize(&s).expect("serialize");
            let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
            assert_eq!(
                back.background_gradient, s.background_gradient,
                "gradient round-trip"
            );
            assert_eq!(back, s, "全字段 round-trip 仍相等");
        }
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

    #[test]
    fn caret_selection_colors_default_none() {
        // 缺省 None：render arm 回退到 color（caret）/ 蓝半透（selection-bg）/ 白（selection-color）。
        let s = ResolvedStyle::default();
        assert!(
            s.caret_color.is_none(),
            "caret_color 默认 None（回退 color）"
        );
        assert!(
            s.selection_background.is_none(),
            "selection_background 默认 None（回退蓝半透）"
        );
        assert!(
            s.selection_color.is_none(),
            "selection_color 默认 None（回退白）"
        );
    }

    #[test]
    fn caret_selection_colors_bincode_roundtrip() {
        // pkg 字段：ResolvedStyle 经 bincode 进 pkg.bin。None / Some 都需 round-trip 稳定。
        let mut s = ResolvedStyle::default();
        s.caret_color = Some([0.1, 0.2, 0.3, 1.0]);
        s.selection_background = Some([0.0, 0.5, 0.0, 0.7]);
        s.selection_color = Some([1.0, 1.0, 0.0, 1.0]);
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.caret_color, s.caret_color);
        assert_eq!(back.selection_background, s.selection_background);
        assert_eq!(back.selection_color, s.selection_color);
        assert_eq!(back, s, "加字段后全字段 round-trip 仍相等");
    }
}
