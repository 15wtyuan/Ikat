use crate::style::color_filter::{self, IDENTITY};
use crate::style::resolved::{
    BackgroundSize, BorderRadius, BoxShadow, CornerRadius, DisplayMode, Gradient2, GradientDir,
    OverflowMode, ResolvedStyle, SliceInsets, TextAlign,
};
use taffy::geometry::{Rect, Size};
use taffy::style::{Dimension, LengthPercentage, LengthPercentageAuto};

/// px → Dimension::Length(f32)；% → LengthPercentage::Percent；auto → Auto
pub fn parse_length(s: &str) -> LengthPercentageAuto {
    let s = s.trim();
    if s == "auto" {
        return LengthPercentageAuto::Auto;
    }
    parse_lp(s).into()
}

pub fn parse_lp(s: &str) -> LengthPercentage {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        if let Ok(v) = pct.trim().parse::<f32>() {
            return LengthPercentage::Percent(v / 100.0);
        }
    }
    if let Some(px) = s.strip_suffix("px") {
        if let Ok(v) = px.trim().parse::<f32>() {
            return LengthPercentage::Length(v);
        }
    }
    // 裸数字当 px
    if let Ok(v) = s.parse::<f32>() {
        return LengthPercentage::Length(v);
    }
    LengthPercentage::Length(0.0)
}

pub fn parse_dimension(s: &str) -> Dimension {
    let s = s.trim();
    if s == "auto" {
        return Dimension::Auto;
    }
    match parse_lp(s) {
        LengthPercentage::Length(v) => Dimension::Length(v),
        LengthPercentage::Percent(v) => Dimension::Percent(v),
    }
}

/// 1~4 值展开四向（top right bottom left）
pub fn parse_four(s: &str) -> [f32; 4] {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| -> f32 {
        parts
            .get(i)
            .and_then(|x| x.strip_suffix("px").unwrap_or(x).trim().parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    match parts.len() {
        1 => {
            let v = p(0);
            [v, v, v, v]
        }
        2 => {
            let v = p(0);
            let h = p(1);
            [v, h, v, h]
        }
        3 => [p(0), p(1), p(2), p(1)],
        _ => [p(0), p(1), p(2), p(3)],
    }
}

/// 解析 CSS filter 函数链 → 4×5 矩阵（None=filter:none 或空）。
/// 函数：grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia。
/// 多函数 = 矩阵相乘（左到右，CSS 顺序）。
fn parse_filter(value: &str) -> Option<[f32; 20]> {
    let v = value.trim();
    if v == "none" || v.is_empty() {
        return None;
    }
    let mut acc = IDENTITY;
    let mut any = false;
    for func in v.split_whitespace() {
        // func 形如 "grayscale(1)" / "hue-rotate(90deg)"
        let (name, arg) = match func.split_once('(') {
            Some((n, rest)) => {
                let arg = rest.trim_end_matches(')');
                (n.trim(), arg.trim())
            }
            None => continue, // 无括号函数（罕见）跳过
        };
        let m = match name {
            "grayscale" => {
                // grayscale(x): x∈[0,1]，x=1 完全灰化。sat = -x → saturate(1-x)
                let x = parse_number(arg).unwrap_or(1.0);
                color_filter::saturate(1.0 - x)
            }
            "brightness" => color_filter::brightness(parse_number(arg).unwrap_or(1.0)),
            "contrast" => color_filter::contrast(parse_number(arg).unwrap_or(1.0)),
            "saturate" => color_filter::saturate(parse_number(arg).unwrap_or(1.0)),
            "hue-rotate" => {
                // 90deg → 90.0
                let deg = arg
                    .trim_end_matches("deg")
                    .trim()
                    .parse::<f32>()
                    .unwrap_or(0.0);
                color_filter::hue_rotate(deg)
            }
            "invert" => {
                let x = parse_number(arg).unwrap_or(1.0);
                if x >= 0.5 {
                    color_filter::invert()
                } else {
                    IDENTITY
                }
            }
            "sepia" => {
                // sepia(1) = 棕褐 tint 预设（完整 Tint 矩阵待补，先用 grayscale 占位，spec §9 风险）
                // ponytail: sepia 完整 Tint 矩阵实现期补，先用 grayscale 占位
                color_filter::grayscale()
            }
            _ => continue,
        };
        acc = color_filter::concat(&m, &acc); // 新 preset 左乘（fgui ConcatValues: newPreset × _matrix）
        any = true;
    }
    if any {
        Some(acc)
    } else {
        None
    }
}

/// parse_number: 解析 "1" / "1.2" / "50%" → f32（% 暂存比例，渲染期 resolve）。
fn parse_number(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        p.trim().parse::<f32>().ok().map(|v| v / 100.0)
    } else {
        s.parse::<f32>().ok()
    }
}

/// 解析 border-image-slice 4 值（CSS 4 值缩写同 margin）。
/// px 存像素，% 存比例（渲染期 resolve 乘源图边）。
fn parse_slice(value: &str) -> Option<SliceInsets> {
    let nums: Vec<f32> = value
        .split_whitespace()
        .map(|tok| {
            if let Some(p) = tok.strip_suffix('%') {
                p.parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                tok.parse::<f32>().ok()
            }
        })
        .collect::<Option<Vec<_>>>()?;
    let (t, r, b, l) = match nums.len() {
        1 => (nums[0], nums[0], nums[0], nums[0]),
        2 => (nums[0], nums[1], nums[0], nums[1]),
        3 => (nums[0], nums[1], nums[2], nums[1]),
        4 => (nums[0], nums[1], nums[2], nums[3]),
        _ => return None,
    };
    Some(SliceInsets {
        top: t,
        right: r,
        bottom: b,
        left: l,
    })
}

/// 解析 border-radius 1~4 值（每值 px 或 %）→ [LengthPercentage;4]（TL,TR,BR,BL）。
/// 与 parse_four 同序，但保留 %。任一值非法（auto/inherit/initial/非 px-% 数字）→ None
/// （CSS：整条声明无效）。
fn parse_radius_group(s: &str) -> Option<[LengthPercentage; 4]> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| -> Option<LengthPercentage> {
        let tok = parts.get(i)?.trim();
        if tok == "auto" || tok == "inherit" || tok == "initial" {
            return None;
        }
        // parse_lp 对垃圾（如 "abc"）静默返回 Length(0)，需额外校验：
        // 合法 token = 裸数字 / 数字px / 数字%
        let num_part = tok.trim_end_matches("px").trim_end_matches('%');
        if num_part.trim().parse::<f32>().is_err() {
            return None;
        }
        Some(parse_lp(tok))
    };
    let v0 = p(0)?;
    Some(match parts.len() {
        1 => [v0, v0, v0, v0],
        2 => [v0, p(1)?, v0, p(1)?],
        3 => [v0, p(1)?, p(2)?, p(1)?],
        _ => [v0, p(1)?, p(2)?, p(3)?],
    })
}

pub fn parse_color(s: &str) -> Option<[f32; 4]> {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    } else {
        None
    }
}

/// 解析 CSS `background-image: url(...)` 值，提取括号内路径（去可选引号 + 首尾空格）。
/// 支持 `url(x)` / `url("x")` / `url('x')` / `url( x )`。非 url() 格式或空 → None。
pub fn parse_url(value: &str) -> Option<String> {
    let v = value.trim();
    let inner = v.strip_prefix("url(")?.strip_suffix(")")?;
    let inner = inner.trim();
    let len = inner.len();
    if len == 0 {
        return None;
    }
    // 去首尾配对引号
    let path = if len >= 2
        && ((inner.starts_with('"') && inner.ends_with('"'))
            || (inner.starts_with('\'') && inner.ends_with('\'')))
    {
        &inner[1..len - 1]
    } else {
        inner
    };
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// 解析 `linear-gradient(...)` 内部串（已去外层 `linear-gradient(` `)`）。
///
/// 围栏子集：方向仅 `to right/left/top/bottom` 4 正向 + 恰好 2 色 stop（用 `parse_color`，
/// 即 6 位 hex）。多 stop / 斜角度（`45deg` 等）/未知方向/不可解析色 → 返 `false`
/// （apply_decl 静默忽略，与 clip-path 等围栏外 CSS 同模式——CSS 合法但 LoomGUI 不支持该形态，
/// 渲染时不绘渐变，AI 不可预测性弱于报错）。
///
/// 形如：`"to right, #ff0000, #0000ff"` → `Gradient2 { a=red, b=blue, dir=ToRight }`。
fn parse_linear_gradient_2(style: &mut ResolvedStyle, inner: &str) -> bool {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    // 至少 3 段：方向 + 2 色。多于 3 段（多 stop）拒收。
    if parts.len() != 3 {
        return false;
    }
    let dir = match parts[0] {
        "to right" => GradientDir::ToRight,
        "to left" => GradientDir::ToLeft,
        "to top" => GradientDir::ToTop,
        "to bottom" => GradientDir::ToBottom,
        _ => return false, // 斜角度 / 未知方向 → 围栏外
    };
    let color_a = match parse_color(parts[1]) {
        Some(c) => c,
        None => return false,
    };
    let color_b = match parse_color(parts[2]) {
        Some(c) => c,
        None => return false,
    };
    style.background_gradient = Some(Gradient2 {
        color_a,
        color_b,
        dir,
    });
    true
}

use crate::style::resolved::LocalTransform;
use crate::transform::{self, Affine2};

/// 解析 CSS `transform` 声明值为累积 Affine2 矩阵。
/// 支持 translate(px,px)/rotate(deg)/scale(num[,num])；skew/matrix()/%/3D 静默跳过。
/// 多函数从左到右 = 矩阵左乘累积（CSS 语义：最左函数最外层）。
pub fn parse_transform(value: &str) -> LocalTransform {
    let mut m = transform::IDENTITY;
    for (name, args) in iter_transform_funcs(value.trim()) {
        if let Some(fm) = func_to_matrix(name, args) {
            m = transform::mul(&m, &fm);
        }
    }
    LocalTransform { matrix: m }
}

/// 拆 "translate(10px,20px) rotate(45deg)" → [("translate","10px,20px"),("rotate","45deg")]。
fn iter_transform_funcs(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 跳空白
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'-') {
            i += 1;
        }
        let name = &s[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'(' {
            break;
        }
        i += 1; // skip '('
        let args_start = i;
        while i < bytes.len() && bytes[i] != b')' {
            i += 1;
        }
        let args = &s[args_start..i];
        if i < bytes.len() {
            i += 1;
        } // skip ')'
        if !name.is_empty() {
            out.push((name, args));
        }
    }
    out
}

/// 单函数 → Affine2。围栏外函数返 None（跳过）。
fn func_to_matrix(name: &str, args: &str) -> Option<Affine2> {
    let parts: Vec<&str> = args.split(',').map(|p| p.trim()).collect();
    match name {
        "translate" => {
            // translate 只支持 px，拒 %
            let x = parse_px(parts.first().copied().unwrap_or("0"))?;
            let y = parse_px(parts.get(1).copied().unwrap_or("0"))?;
            Some(transform::from_translate(x, y))
        }
        "rotate" => {
            let deg = parts.first().copied().unwrap_or("0");
            let deg = deg.trim_end_matches("deg").trim().parse::<f32>().ok()?;
            Some(transform::from_rotate(deg.to_radians()))
        }
        "scale" => {
            let sx = parts.first().copied().unwrap_or("1").parse::<f32>().ok()?;
            let sy = parts
                .get(1)
                .copied()
                .unwrap_or(&sx.to_string())
                .parse::<f32>()
                .ok()?;
            Some(transform::from_scale(sx, sy))
        }
        _ => None, // skew/matrix3d/... 围栏外
    }
}

/// overflow 值 → OverflowMode。未知值返回 None（宽松忽略，不报错）。
fn parse_overflow(value: &str) -> Option<OverflowMode> {
    match value.trim() {
        "visible" => Some(OverflowMode::Visible),
        "hidden" => Some(OverflowMode::Hidden),
        "scroll" => Some(OverflowMode::Scroll),
        "auto" => Some(OverflowMode::Auto),
        _ => None,
    }
}

/// 把一条 declaration 应用到 style（覆盖对应字段）。返回是否被识别。
pub fn apply_decl(style: &mut ResolvedStyle, prop: &str, value: &str) -> bool {
    let ts = &mut style.taffy_style;
    match prop.trim() {
        "width" => {
            ts.size.width = parse_dimension(value);
            true
        }
        "height" => {
            ts.size.height = parse_dimension(value);
            true
        }
        "min-width" => {
            ts.min_size.width = parse_dimension(value);
            true
        }
        "min-height" => {
            ts.min_size.height = parse_dimension(value);
            true
        }
        "max-width" => {
            ts.max_size.width = parse_dimension(value);
            true
        }
        "max-height" => {
            ts.max_size.height = parse_dimension(value);
            true
        }
        "padding" => {
            let [t, r, b, l] = parse_four(value);
            ts.padding = Rect {
                left: LengthPercentage::Length(l),
                right: LengthPercentage::Length(r),
                top: LengthPercentage::Length(t),
                bottom: LengthPercentage::Length(b),
            };
            true
        }
        "margin" => {
            let [t, r, b, l] = parse_four(value);
            ts.margin = Rect {
                left: LengthPercentageAuto::Length(l),
                right: LengthPercentageAuto::Length(r),
                top: LengthPercentageAuto::Length(t),
                bottom: LengthPercentageAuto::Length(b),
            };
            true
        }
        "border" | "border-width" => {
            let [t, r, b, l] = parse_four(value);
            ts.border = Rect {
                left: LengthPercentage::Length(l),
                right: LengthPercentage::Length(r),
                top: LengthPercentage::Length(t),
                bottom: LengthPercentage::Length(b),
            };
            // 同时填视觉 border_width（取 top 作为单值，渲染描边用）
            style.border_width = t;
            true
        }
        "border-radius" => {
            // 语法：<len>{1,4} [ / <len>{1,4} ]?  —— / 前水平半径，/ 后垂直半径（省略=同水平）
            let (h_group, v_group) = match value.split_once('/') {
                Some((h, v)) => (h, v),
                None => (value, value), // 无 / → 垂直 = 水平（正圆角）
            };
            let h = match parse_radius_group(h_group) {
                Some(g) => g,
                None => return false,
            };
            let v = match parse_radius_group(v_group) {
                Some(g) => g,
                None => return false,
            };
            style.border_radius = BorderRadius {
                corners: [
                    CornerRadius { h: h[0], v: v[0] }, // TL
                    CornerRadius { h: h[1], v: v[1] }, // TR
                    CornerRadius { h: h[2], v: v[2] }, // BR
                    CornerRadius { h: h[3], v: v[3] }, // BL
                ],
            };
            true
        }
        "gap" => {
            let f = parse_four(value);
            ts.gap = Size {
                width: LengthPercentage::Length(f[1]),
                height: LengthPercentage::Length(f[0]),
            };
            true
        }
        "flex-direction" => {
            ts.flex_direction = match value.trim() {
                "row" => taffy::FlexDirection::Row,
                "row-reverse" => taffy::FlexDirection::RowReverse,
                "column-reverse" => taffy::FlexDirection::ColumnReverse,
                _ => taffy::FlexDirection::Column,
            };
            true
        }
        "flex-wrap" => {
            ts.flex_wrap = match value.trim() {
                "wrap" => taffy::FlexWrap::Wrap,
                _ => taffy::FlexWrap::NoWrap,
            };
            true
        }
        "justify-content" => {
            ts.justify_content = Some(parse_justify(value));
            true
        }
        "align-items" => {
            ts.align_items = Some(parse_align(value));
            true
        }
        "align-self" => {
            ts.align_self = Some(parse_align(value));
            true
        }
        "flex-grow" => {
            ts.flex_grow = value.trim().parse::<f32>().unwrap_or(0.0);
            true
        }
        "flex-shrink" => {
            ts.flex_shrink = value.trim().parse::<f32>().unwrap_or(1.0);
            true
        }
        "flex-basis" => {
            ts.flex_basis = parse_dimension(value);
            true
        }
        "display" => {
            match value.trim() {
                "none" => {
                    ts.display = taffy::Display::None;
                    style.display_mode = DisplayMode::None;
                }
                "block" => {
                    // block：taffy 仍 Flex（守铁律），仅旁路字段标记供打包器 desugar 识别。
                    ts.display = taffy::Display::Flex;
                    style.display_mode = DisplayMode::Block;
                }
                _ => {
                    ts.display = taffy::Display::Flex;
                    style.display_mode = DisplayMode::Flex;
                }
            }
            true
        }
        "filter" => {
            // CSS filter 函数链：grayscale(1) brightness(1.2) ... → 矩阵相乘
            style.color_filter = parse_filter(value);
            true
        }
        "border-image-slice" => {
            // 4 值上右下左（CSS 4 值缩写同 margin）；px 存像素，% 存比例（渲染期 resolve 乘源图边）
            match parse_slice(value) {
                Some(ins) => {
                    style.border_image_slice = Some(ins);
                    true
                }
                None => false,
            }
        }
        "background-color" => {
            style.background_color = parse_color(value);
            true
        }
        "background-image" => {
            // `background-image: linear-gradient(...)` 走 2 色渐变；否则走现有 url() 解析。
            // 渐变与 url() 互斥（gradient 走 quad_gradient 顶点色，无纹理采样）。
            let v = value.trim();
            if let Some(rest) = v
                .strip_prefix("linear-gradient(")
                .and_then(|s| s.strip_suffix(')'))
            {
                return parse_linear_gradient_2(style, rest);
            }
            style.background_image = parse_url(value);
            style.background_image.is_some()
        }
        "background" => {
            // `background` shorthand：仅识别 `linear-gradient(...)` 形态。
            // 其他 shorthand 值（纯色、url()）围栏不支持——纯色须写 `background-color`，
            // 图片须写 `background-image`。围栏外值静默返 false（与 clip-path 等同模式）。
            let v = value.trim();
            if let Some(rest) = v
                .strip_prefix("linear-gradient(")
                .and_then(|s| s.strip_suffix(')'))
            {
                return parse_linear_gradient_2(style, rest);
            }
            false
        }
        "background-size" => {
            style.background_size = match value.trim() {
                "cover" => BackgroundSize::Cover,
                "contain" => BackgroundSize::Contain,
                "100%" => BackgroundSize::Stretch,
                _ => return false, // 围栏外值（auto/px/两值）静默忽略
            };
            true
        }
        "border-color" => {
            style.border_color = parse_color(value);
            true
        }
        "opacity" => {
            style.opacity = value
                .trim()
                .trim_end_matches('%')
                .parse::<f32>()
                .unwrap_or(1.0)
                .min(1.0);
            true
        }
        "overflow" => {
            // shorthand：双轴同值。未知值宽松忽略（不动既有字段，仍返回 true）。
            if let Some(m) = parse_overflow(value) {
                style.overflow_x = m;
                style.overflow_y = m;
            }
            true
        }
        "overflow-x" => {
            // longhand：单轴 x。后于 shorthand apply 即覆盖（CSS 同 specificity 源序后写者胜）。
            if let Some(m) = parse_overflow(value) {
                style.overflow_x = m;
            }
            true
        }
        "overflow-y" => {
            if let Some(m) = parse_overflow(value) {
                style.overflow_y = m;
            }
            true
        }
        "color" => {
            if let Some(c) = parse_color(value) {
                style.color = c;
            }
            true
        }
        "font-size" => {
            style.font_size = parse_px(value).unwrap_or(style.font_size);
            true
        }
        "font-family" => {
            style.font_family = Some(value.trim().to_string());
            true
        }
        "font-weight" => {
            style.font_weight = value.trim().parse::<u16>().unwrap_or(400);
            true
        }
        "text-align" => {
            style.text_align = match value.trim() {
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => TextAlign::Left,
            };
            true
        }
        "line-height" => {
            style.line_height = value
                .trim()
                .trim_end_matches("px")
                .parse::<f32>()
                .unwrap_or(0.0);
            true
        }
        "letter-spacing" => {
            style.letter_spacing = parse_px(value).unwrap_or(0.0);
            true
        }
        "white-space" => {
            style.white_space_nowrap = value.trim() == "nowrap";
            true
        }
        "aspect-ratio" => {
            if let Ok(v) = value.trim().parse::<f32>() {
                ts.aspect_ratio = Some(v);
            }
            true
        }
        "order" => {
            // taffy 0.5 Style 无 order 字段；存进 ResolvedStyle.order，
            // 由 layout 在 flex 排序前消费。非法值降级为 0。
            style.order = value.trim().parse::<i32>().unwrap_or(0);
            true
        }
        "pointer-events" => {
            // auto/默认=true（可命中），none=false（跳过自身，继续测子——CSS 语义）
            style.touchable = value.trim() != "none";
            true
        }
        "transform" => {
            style.transform = parse_transform(value);
            true
        }
        "position" => {
            // absolute 围栏内（脱离流）；relative 显式；fixed/sticky 围栏外静默忽略。
            match value.trim() {
                "absolute" => {
                    ts.position = taffy::style::Position::Absolute;
                    true
                }
                "relative" => {
                    ts.position = taffy::style::Position::Relative;
                    true
                }
                _ => false, // fixed/sticky/其他 → 围栏外
            }
        }
        "top" | "right" | "bottom" | "left" => {
            // inset 四边。auto 保持默认（不写）；px 写 Length。
            if let Some(px) = parse_px(value) {
                let lp = taffy::style::LengthPercentageAuto::Length(px);
                match prop {
                    "top" => ts.inset.top = lp,
                    "right" => ts.inset.right = lp,
                    "bottom" => ts.inset.bottom = lp,
                    "left" => ts.inset.left = lp,
                    _ => unreachable!(),
                }
                true
            } else if value.trim() == "auto" {
                // auto 显式置回默认（覆盖之前的 px 值）
                let lp = taffy::style::LengthPercentageAuto::Auto;
                match prop {
                    "top" => ts.inset.top = lp,
                    "right" => ts.inset.right = lp,
                    "bottom" => ts.inset.bottom = lp,
                    "left" => ts.inset.left = lp,
                    _ => unreachable!(),
                }
                true
            } else {
                false // 非法值（% 等围栏外）静默忽略
            }
        }
        "box-shadow" => {
            // CSS: ox oy [blur] [spread] color。blur 静默忽略，spread 解析。
            // ponytail: blur 静默忽略（真实 blur 需离屏 RT，排 v1.14+）。
            let parts: Vec<&str> = value.split_whitespace().collect();
            if parts.len() < 3 {
                return false;
            }
            // parse_number 不剥 "px" 后缀，此处手动剥。
            let ox = parse_number(parts[0].trim_end_matches("px")).unwrap_or(0.0);
            let oy = parse_number(parts[1].trim_end_matches("px")).unwrap_or(0.0);
            // 第 3 段可能是 blur（数值）或 color；若是数值且后一段也数值则分别为 blur+spread。
            let mut color_idx = 2;
            let mut spread_val = 0.0f32;
            if parts[2].trim_end_matches("px").parse::<f32>().is_ok() {
                if parts.len() < 4 {
                    return false;
                }
                if parts[3].trim_end_matches("px").parse::<f32>().is_ok() {
                    // parts[3] is spread
                    spread_val = parse_number(parts[3].trim_end_matches("px")).unwrap_or(0.0);
                    color_idx = 4;
                    if parts.len() < 5 {
                        return false;
                    }
                } else {
                    color_idx = 3;
                }
            }
            let color = parts
                .get(color_idx)
                .and_then(|s| parse_color(s))
                .unwrap_or([0.0, 0.0, 0.0, 0.3]);
            style.box_shadow = Some(BoxShadow {
                ox,
                oy,
                spread: spread_val,
                color,
            });
            true
        }
        "transition" => {
            style.transition = parse_transition(value);
            true
        }
        _ => false, // 装饰属性静默忽略
    }
}

/// 解析 CSS `transition` 声明值 → TransitionSpec 列表。
///
/// 逗号分隔多 spec（如 `background-color 0.3s, color 0.3s`）。每段由 `parse_one_transition`
/// 解析。空输入返回空 Vec（未声明 transition）。
fn parse_transition(value: &str) -> Vec<crate::style::resolved::TransitionSpec> {
    value.split(',').filter_map(parse_one_transition).collect()
}

/// 解析单个 transition spec（逗号分隔的一段）。
/// 空格切 token：第 1 个 = 属性名（all/opacity/color/background-color）；含 's' 的 =
/// duration/delay（首遇 duration，次遇 delay）；其余 = ease 关键字。缺省补默认
/// （dur=0s, ease=Linear, delay=0s）。空段返回 None（被 filter_map 丢弃）。
fn parse_one_transition(part: &str) -> Option<crate::style::resolved::TransitionSpec> {
    use crate::style::resolved::TransitionSpec;
    use crate::tween::{Ease, TweenProp};
    let tokens: Vec<&str> = part.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let mut prop = None;
    let mut duration = 0.0f32;
    let mut delay = 0.0f32;
    let mut ease = Ease::Linear;
    for t in tokens {
        if t == "all" {
            prop = None;
        } else if t == "opacity" {
            prop = Some(TweenProp::Opacity);
        } else if t == "color" {
            prop = Some(TweenProp::TextColor);
        } else if t == "background-color" {
            prop = Some(TweenProp::BgColor);
        } else if t.ends_with('s') {
            let n = t.trim_end_matches('s').parse::<f32>().unwrap_or(0.0);
            if duration == 0.0 {
                duration = n;
            } else {
                delay = n;
            }
        } else {
            // ease 关键字（CSS 标准名 → 内 Ease 变体）
            ease = match t {
                "linear" => Ease::Linear,
                "ease" => Ease::QuadOut,
                "ease-in" => Ease::QuadIn,
                "ease-out" => Ease::QuadOut,
                "ease-in-out" => Ease::QuadInOut,
                _ => Ease::Linear,
            };
        }
    }
    Some(TransitionSpec {
        prop,
        duration,
        ease,
        delay,
    })
}

/// "10px" → 10.0；"10%" → None（拒 %）；"10" → 10.0（容错无单位）。
fn parse_px(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.ends_with('%') {
        return None;
    }
    s.trim_end_matches("px").trim().parse::<f32>().ok()
}

fn parse_justify(v: &str) -> taffy::JustifyContent {
    // JustifyContent 是 AlignContent 的类型别名（taffy 0.5），用全路径构造
    match v.trim() {
        "center" => taffy::AlignContent::Center,
        "flex-end" => taffy::AlignContent::FlexEnd,
        "space-between" => taffy::AlignContent::SpaceBetween,
        "space-around" => taffy::AlignContent::SpaceAround,
        "space-evenly" => taffy::AlignContent::SpaceEvenly,
        _ => taffy::AlignContent::FlexStart,
    }
}
fn parse_align(v: &str) -> taffy::AlignItems {
    match v.trim() {
        "center" => taffy::AlignItems::Center,
        "flex-end" => taffy::AlignItems::FlexEnd,
        "stretch" => taffy::AlignItems::Stretch,
        "baseline" => taffy::AlignItems::Baseline,
        _ => taffy::AlignItems::FlexStart,
    }
}

#[cfg(test)]
mod tests;
