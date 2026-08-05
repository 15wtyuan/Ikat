use crate::scene::animation::TransformAnim;
use crate::style::color_filter::{self, IDENTITY};
use crate::style::resolved::{
    BackgroundSize, BorderRadius, BorderStyle, BoxShadow, CornerRadius, DisplayMode, Gradient2,
    GradientDir, OverflowMode, ResolvedStyle, SliceInsets, TextAlign,
};
use taffy::geometry::{Rect, Size};
use taffy::style::{Dimension, LengthPercentage, LengthPercentageAuto};

/// px → Dimension::length(f32)；% → LengthPercentage::percent；auto → auto()
pub fn parse_length(s: &str) -> LengthPercentageAuto {
    let s = s.trim();
    if s == "auto" {
        return LengthPercentageAuto::auto();
    }
    parse_lp(s).into()
}

pub fn parse_lp(s: &str) -> LengthPercentage {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        if let Ok(v) = pct.trim().parse::<f32>() {
            return LengthPercentage::percent(v / 100.0);
        }
    }
    if let Some(px) = s.strip_suffix("px") {
        if let Ok(v) = px.trim().parse::<f32>() {
            return LengthPercentage::length(v);
        }
    }
    // 裸数字当 px
    if let Ok(v) = s.parse::<f32>() {
        return LengthPercentage::length(v);
    }
    LengthPercentage::length(0.0)
}

pub fn parse_dimension(s: &str) -> Dimension {
    let s = s.trim();
    if s == "auto" {
        return Dimension::auto();
    }
    // taffy 0.12：LengthPercentage 是 pub struct(CompactLength) tagged pointer，
    // 内字段私有无法 match 变体——用 into_raw + tag 解构（length/percent 二选一）。
    let lp = parse_lp(s);
    let cl = lp.into_raw();
    match cl.tag() {
        taffy::style::CompactLength::LENGTH_TAG => Dimension::length(cl.value()),
        taffy::style::CompactLength::PERCENT_TAG => Dimension::percent(cl.value()),
        _ => Dimension::length(0.0),
    }
}

/// 1~4 值展开四向（top right bottom left）
/// 解析 1~4 值 px（含裸数字）→ [t,r,b,l]。任一 token 非 px（%/em/rem/auto/keyword）→ None。
/// px-only 属性（padding/border-width/gap）用它：非 px 让 apply_decl 返 false（围栏外静默忽略），
/// 不能静默落 0 还返 true——AI 写 `padding:10%` 期望间距在。
pub fn parse_four(s: &str) -> Option<[f32; 4]> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| -> Option<f32> {
        parts
            .get(i)
            .and_then(|x| x.strip_suffix("px").unwrap_or(x).trim().parse::<f32>().ok())
    };
    Some(match parts.len() {
        1 => {
            let v = p(0)?;
            [v, v, v, v]
        }
        2 => [p(0)?, p(1)?, p(0)?, p(1)?],
        3 => [p(0)?, p(1)?, p(2)?, p(1)?],
        _ => [p(0)?, p(1)?, p(2)?, p(3)?],
    })
}

/// margin 围栏 px/%/auto → [t,r,b,l]。任一 token 非 px/%/auto（em/rem/keyword）→ None。
/// 兑现 fence 承诺：`margin:10%` → Percent，`margin:auto` → Auto（居中），
/// `margin:0 auto` → top/bottom Length(0)、left/right Auto。
fn parse_margin_four(s: &str) -> Option<[LengthPercentageAuto; 4]> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| -> Option<LengthPercentageAuto> {
        let x = parts.get(i)?.trim();
        if x == "auto" {
            return Some(LengthPercentageAuto::auto());
        }
        if let Some(pct) = x.strip_suffix('%') {
            return Some(LengthPercentageAuto::percent(
                pct.parse::<f32>().ok()? / 100.0,
            ));
        }
        let px = x
            .strip_suffix("px")
            .unwrap_or(x)
            .trim()
            .parse::<f32>()
            .ok()?;
        Some(LengthPercentageAuto::length(px))
    };
    Some(match parts.len() {
        1 => {
            let v = p(0)?;
            [v, v, v, v]
        }
        2 => [p(0)?, p(1)?, p(0)?, p(1)?],
        3 => [p(0)?, p(1)?, p(2)?, p(1)?],
        _ => [p(0)?, p(1)?, p(2)?, p(3)?],
    })
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
                let x = parse_number(arg).unwrap_or(1.0).clamp(0.0, 1.0);
                // CSS invert(amount) = lerp from identity toward full invert by x.
                // Full invert: diag = -1, offset = 1. Lerp: diag = 1-2x, offset = x.
                let mut m = IDENTITY;
                m[0] = 1.0 - 2.0 * x;
                m[6] = 1.0 - 2.0 * x;
                m[12] = 1.0 - 2.0 * x;
                m[4] = x;
                m[9] = x;
                m[14] = x;
                m
            }
            "sepia" => color_filter::sepia(),
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

/// 解析 flex basis token：length(px) | percent(%) | auto。
/// 供 flex shorthand 的 `[g, sh]`（歧义二 token）和 `[g, sh, b]`（三 token）共用，
/// 保证两分支 basis 解析同口径。无单位/未知关键字 → None（调用方 return false，不静默降级）。
fn parse_flex_basis(tok: &str) -> Option<Dimension> {
    let tok = tok.trim();
    if tok == "auto" {
        return Some(Dimension::auto());
    }
    // length(px) 或 percent(%)：先抽出数值与单位标记。
    let (num, is_px) = tok
        .strip_suffix("px")
        .map(|s| (s.trim(), true))
        .or_else(|| tok.strip_suffix('%').map(|s| (s.trim(), false)))?;
    let v = num.parse::<f32>().ok()?;
    Some(if is_px {
        Dimension::length(v)
    } else {
        Dimension::percent(v / 100.0)
    })
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
    // CSS 函数式颜色：rgb() / rgba()（现代写法统一用 rgb；rgba 为 legacy 别名）。
    // AI 与设计稿导出常写 rgba()——原先只认 hex 导致静默丢色。
    // 支持：rgb(r,g,b) / rgba(r,g,b,a) / rgb(r g b) / rgb(r g b / a)，
    // 分量可 0-255 整数或百分比，alpha 为 0..1。
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    {
        return parse_rgb_inner(rest);
    }
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    } else if s.len() == 8 {
        // CSS Color Module Level 4 `#rrggbbaa`：末 2 位 hex 为 alpha（aa=ff 不透明）。
        // 接受 StyleMirror flush 的 8 位形式（与 6 位 hex aa=ff 等价），确保 color round-trip。
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        let a = u8::from_str_radix(&s[6..8], 16).ok()?;
        Some([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ])
    } else if s.len() == 3 {
        // CSS 3 位 hex：每数字重复（#rgb → #rrggbb，如 #888 = #888888）。
        // digit d → d*17（d*16+d）：0→0、f→255，与 6 位展开一致。
        let r = u8::from_str_radix(&s[0..1], 16).ok()?;
        let g = u8::from_str_radix(&s[1..2], 16).ok()?;
        let b = u8::from_str_radix(&s[2..3], 16).ok()?;
        Some([
            (r * 17) as f32 / 255.0,
            (g * 17) as f32 / 255.0,
            (b * 17) as f32 / 255.0,
            1.0,
        ])
    } else {
        None
    }
}

/// 解析 rgb()/rgba() 括号内分量。接受两种现代/legacy 语法：
/// - legacy：`r,g,b` 或 `r,g,b,a`（逗号分隔，a 仅 rgba）
/// - CSS Color 4：`r g b` 或 `r g b / a`（空格分隔，斜杠前缀 alpha）
/// 分量：0-255 整数 或 0%-100%（255≡1.0）。alpha：0..1 浮点（也接受百分比）。
/// 不可解析（分量数不对、值越界、空）→ None（静默忽略，与 hex 同模式）。
fn parse_rgb_inner(inner: &str) -> Option<[f32; 4]> {
    // 斜杠分隔 alpha（CSS 4）："r g b / a" → ("r g b", Some("a"))
    let (color_part, alpha_part) = if let Some((c, a)) = inner.split_once('/') {
        (c, Some(a.trim()))
    } else {
        (inner, None)
    };
    // 分量按逗号或空白切（legacy 逗号 / CSS4 空格混用也能收）。
    let comps: Vec<&str> = color_part
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|p| !p.trim().is_empty())
        .collect();
    // legacy rgba(r,g,b,a)：无斜杠但 4 个逗号分量 → 第 4 个是 alpha。
    let (rgb, legacy_alpha) = match comps.len() {
        3 => (comps.as_slice(), None),
        4 if alpha_part.is_none() => (&comps[0..3], Some(comps[3])),
        _ => return None,
    };
    let r = parse_component(rgb[0])?;
    let g = parse_component(rgb[1])?;
    let b = parse_component(rgb[2])?;
    // alpha 优先级：斜杠 > legacy 第 4 参 > 缺省 1.0。
    let a = match alpha_part {
        Some(a) => parse_alpha(a)?,
        None => match legacy_alpha {
            Some(a) => parse_alpha(a)?,
            None => 1.0,
        },
    };
    Some([r, g, b, a])
}

/// 颜色分量解析：整数 0-255 或百分比 0%-100%（% → /255 归一）。
fn parse_component(p: &str) -> Option<f32> {
    let p = p.trim();
    if let Some(pct) = p.strip_suffix('%') {
        let v: f32 = pct.trim().parse().ok()?;
        Some((v / 100.0).clamp(0.0, 1.0))
    } else {
        let v: f32 = p.parse().ok()?;
        Some((v / 255.0).clamp(0.0, 1.0))
    }
}

/// alpha 分量解析：0..1 浮点（rgba 第 4 参），也接受百分比（50% → 0.5）。
fn parse_alpha(p: &str) -> Option<f32> {
    let p = p.trim();
    if let Some(pct) = p.strip_suffix('%') {
        let v: f32 = pct.trim().parse().ok()?;
        Some((v / 100.0).clamp(0.0, 1.0))
    } else {
        let v: f32 = p.parse().ok()?;
        Some(v.clamp(0.0, 1.0))
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
/// Parse a keyframe transform into its lossless TRS representation.
///
/// Keyframe transforms deliberately do not use the static-transform matrix path: the runtime
/// interpolates each component independently. The fence transform subset is translate/scale/
/// rotate, so this preserves every supported function without matrix decomposition. Any unknown
/// function or malformed argument returns `None` rather than silently dropping part of a value.
/// `translateX`/`translateY` are accepted as the one-axis CSS conveniences used by showcase CSS;
/// `none` is the identity transform and returns an empty `TransformAnim`.
/// The decomposition assumes the canonical T→R→S order: repeated components are last-wins,
/// and other composition orders (for example `rotate translate`) are not preserved.
pub fn parse_transform_trs(value: &str) -> Option<TransformAnim> {
    let value = value.trim();
    if value == "none" {
        return Some(TransformAnim::default());
    }
    let funcs = iter_transform_funcs(value);
    if funcs.is_empty() {
        return None;
    }
    let mut out = TransformAnim::default();
    for (name, args) in funcs {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        match name {
            "translate" => {
                if parts.len() > 2 || parts.is_empty() {
                    return None;
                }
                let x = parse_px(parts[0])?;
                let y = if let Some(y) = parts.get(1) {
                    parse_px(y)?
                } else {
                    0.0
                };
                out.translate = Some([x, y]);
            }
            "translateX" => {
                if parts.len() != 1 {
                    return None;
                }
                out.translate = Some([parse_px(parts[0])?, 0.0]);
            }
            "translateY" => {
                if parts.len() != 1 {
                    return None;
                }
                out.translate = Some([0.0, parse_px(parts[0])?]);
            }
            "scale" => {
                if parts.len() != 1 && parts.len() != 2 {
                    return None;
                }
                let sx = parts[0].parse::<f32>().ok()?;
                let sy = if let Some(y) = parts.get(1) {
                    y.parse::<f32>().ok()?
                } else {
                    sx
                };
                out.scale = Some([sx, sy]);
            }
            "rotate" => {
                if parts.len() != 1 {
                    return None;
                }
                let deg = parts[0]
                    .trim_end_matches("deg")
                    .trim()
                    .parse::<f32>()
                    .ok()?;
                out.rotate = Some(deg.to_radians());
            }
            _ => return None,
        }
    }
    Some(out)
}

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

/// 解析 border 简写值：`<width> <style>? <color>?`（CSS 标准简写语义）。
/// width 取首个 px token，style 取首个关键字（solid/dashed/dotted/double/none），
/// color 取首个可解析颜色 token。width 缺失 → None（整条无效）。未声明 style 时
/// style 默认 None（CSS：不画边框），调用方据此决定是否填 border_style。
fn parse_border_value(value: &str) -> Option<(f32, BorderStyle, Option<[f32; 4]>)> {
    let mut w: Option<f32> = None;
    let mut style: BorderStyle = BorderStyle::None; // 未声明 = CSS 默认 none
    let mut color: Option<[f32; 4]> = None;
    for tok in value.split_whitespace() {
        if color.is_none() {
            if let Some(c) = parse_color(tok) {
                color = Some(c);
                continue;
            }
        }
        // style 关键字优先于 width 判定，避免 "solid" 被 strip_suffix("px") 误漏。
        match tok {
            "solid" => {
                style = BorderStyle::Solid;
                continue;
            }
            "dashed" => {
                style = BorderStyle::Dashed;
                continue;
            }
            "dotted" => {
                style = BorderStyle::Dotted;
                continue;
            }
            "double" => {
                style = BorderStyle::Double;
                continue;
            }
            "none" => {
                style = BorderStyle::None;
                continue;
            }
            _ => {}
        }
        if w.is_none() {
            if let Some(px) = tok
                .strip_suffix("px")
                .and_then(|s| s.trim().parse::<f32>().ok())
            {
                w = Some(px);
            }
        }
    }
    Some((w?, style, color))
}

/// CSS 盒模型四边（border/padding 单边 longhand 共用）。
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// border-top/right/bottom/left 单边 longhand：设 ts.border 对应边 + border_color +
/// border_style，不动其他三边。
fn apply_border_side(style: &mut ResolvedStyle, side: Side, value: &str) -> bool {
    let Some((w, bstyle, color)) = parse_border_value(value) else {
        return false;
    };
    let lp = LengthPercentage::length(w);
    let ts = &mut style.taffy_style;
    match side {
        Side::Top => ts.border.top = lp,
        Side::Right => ts.border.right = lp,
        Side::Bottom => ts.border.bottom = lp,
        Side::Left => ts.border.left = lp,
    }
    if let Some(c) = color {
        style.border_color = Some(c);
    }
    style.border_style = bstyle;
    true
}

/// padding-top/right/bottom/left 单边 longhand：设 ts.padding 对应边，不动其他三边。
/// px-only（同 padding 简写）：复用 parse_four 的 px 解析，单 longhand 取首值；非 px → false。
fn apply_padding_side(style: &mut ResolvedStyle, side: Side, value: &str) -> bool {
    let [v, _, _, _] = match parse_four(value) {
        Some(f) => f,
        None => return false,
    };
    let lp = LengthPercentage::length(v);
    let ts = &mut style.taffy_style;
    match side {
        Side::Top => ts.padding.top = lp,
        Side::Right => ts.padding.right = lp,
        Side::Bottom => ts.padding.bottom = lp,
        Side::Left => ts.padding.left = lp,
    }
    true
}

/// margin 单边声明：与 apply_padding_side 同构，但走 parse_margin_four（支持 px/%/auto）。
/// 只设指定边，其余边保持不动（不重置四边）。
fn apply_margin_side(style: &mut ResolvedStyle, side: Side, value: &str) -> bool {
    let [v, _, _, _] = match parse_margin_four(value) {
        Some(f) => f,
        None => return false,
    };
    let ts = &mut style.taffy_style;
    match side {
        Side::Top => ts.margin.top = v,
        Side::Right => ts.margin.right = v,
        Side::Bottom => ts.margin.bottom = v,
        Side::Left => ts.margin.left = v,
    }
    true
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
            let [t, r, b, l] = match parse_four(value) {
                Some(v) => v,
                None => return false,
            };
            ts.padding = Rect {
                left: LengthPercentage::length(l),
                right: LengthPercentage::length(r),
                top: LengthPercentage::length(t),
                bottom: LengthPercentage::length(b),
            };
            true
        }
        "padding-top" => apply_padding_side(style, Side::Top, value),
        "padding-right" => apply_padding_side(style, Side::Right, value),
        "padding-bottom" => apply_padding_side(style, Side::Bottom, value),
        "padding-left" => apply_padding_side(style, Side::Left, value),
        "margin-top" => apply_margin_side(style, Side::Top, value),
        "margin-right" => apply_margin_side(style, Side::Right, value),
        "margin-bottom" => apply_margin_side(style, Side::Bottom, value),
        "margin-left" => apply_margin_side(style, Side::Left, value),
        "margin" => {
            let [t, r, b, l] = match parse_margin_four(value) {
                Some(v) => v,
                None => return false,
            };
            ts.margin = Rect {
                left: l,
                right: r,
                top: t,
                bottom: b,
            };
            true
        }
        "border" => {
            // CSS 简写：四边同值。width + style + color 共用 parse_border_value。
            let Some((w, bstyle, color)) = parse_border_value(value) else {
                return false;
            };
            let lp = LengthPercentage::length(w);
            ts.border = Rect {
                left: lp,
                right: lp,
                top: lp,
                bottom: lp,
            };
            if let Some(c) = color {
                style.border_color = Some(c);
            }
            style.border_style = bstyle;
            true
        }
        "border-top" => apply_border_side(style, Side::Top, value),
        "border-right" => apply_border_side(style, Side::Right, value),
        "border-bottom" => apply_border_side(style, Side::Bottom, value),
        "border-left" => apply_border_side(style, Side::Left, value),
        "border-width" => {
            let [t, r, b, l] = match parse_four(value) {
                Some(v) => v,
                None => return false,
            };
            ts.border = Rect {
                left: LengthPercentage::length(l),
                right: LengthPercentage::length(r),
                top: LengthPercentage::length(t),
                bottom: LengthPercentage::length(b),
            };
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
            let f = match parse_four(value) {
                Some(v) => v,
                None => return false,
            };
            ts.gap = Size {
                width: LengthPercentage::length(f[1]),
                height: LengthPercentage::length(f[0]),
            };
            true
        }
        // CSS `gap` longhand：row-gap 对应纵向间距（gap.height），column-gap 横向（gap.width），
        // 与上方 `gap` shorthand 拆分语义一致。复用 parse_four 的 px 解析（含裸数字），
        // 单 longhand 取首值——与 padding-* 单边 longhand 同口径（px-only：非 px 落 false）。
        // 裸数字（如 row-gap:0）须与 px 后缀等价，否则 default `0` 会被静默拒。
        "row-gap" => {
            let [v, _, _, _] = match parse_four(value) {
                Some(f) => f,
                None => return false,
            };
            ts.gap.height = LengthPercentage::length(v);
            true
        }
        "column-gap" => {
            let [v, _, _, _] = match parse_four(value) {
                Some(f) => f,
                None => return false,
            };
            ts.gap.width = LengthPercentage::length(v);
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
                "nowrap" => taffy::FlexWrap::NoWrap,
                "wrap" => taffy::FlexWrap::Wrap,
                _ => return false, // wrap-reverse 等未支持值不静默降级（schema 已拦）
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
        // `align-content` longhand：cross 轴多行内容对齐。
        // 不复用 parse_justify——后者服务于 justify-content，无 stretch 分支，会把
        // align-content 的 CSS 默认值 stretch 静默降级成 FLEX_START。这里独立 match
        // 覆盖 fence schema 列出的全部合法值（flex-start/center/flex-end/stretch/
        // space-between/space-around/space-evenly）。
        "align-content" => {
            // cross 轴多行内容对齐。fence schema 列出的合法值全覆盖；未知值返 false
            // （不静默降级成 FLEX_START，与 flex-wrap 同口径——拼写错误应报错而非吞）。
            ts.align_content = Some(match value.trim() {
                "flex-start" => taffy::AlignContent::FLEX_START,
                "center" => taffy::AlignContent::CENTER,
                "flex-end" => taffy::AlignContent::FLEX_END,
                "stretch" => taffy::AlignContent::STRETCH,
                "space-between" => taffy::AlignContent::SPACE_BETWEEN,
                "space-around" => taffy::AlignContent::SPACE_AROUND,
                "space-evenly" => taffy::AlignContent::SPACE_EVENLY,
                _ => return false,
            });
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
        // CSS `flex` shorthand：`<grow> <shrink>? <basis>?`。
        // 单 number（`flex:1`）→ grow=1/shrink=1/basis=0%（CSS 规范），
        // 单 length（`flex:100px`）→ grow=1/shrink=1/basis=length，
        // `none` → 0 0 auto，`initial` → 0 1 auto。未支持形态返 false（不静默降级）。
        // flex_basis 字段类型是 taffy Dimension（length/percent/auto 三态），非 LengthPercentageAuto。
        "flex" => {
            // CSS flex shorthand 语义（spec 用 `||` 可换序，但 fence 子集只支持标准顺序）：
            //   `none`=0 0 auto / `initial`=0 1 auto / `auto`=1 1 auto
            //   `<grow>` → grow=g,shrink=1,basis=0%
            //   `<grow> <shrink>` → 两 number
            //   `<grow> <basis>` → grow,number + basis(length/percent/auto)；shrink 默认 1
            //   `<grow> <shrink> <basis>` → 三值显式
            // 单值可以是 number 或 length（`flex:2` vs `flex:100px`）。
            // **不静默降级**：任何 token 解析失败 → return false（不 unwrap_or 吞值）。
            let toks: Vec<&str> = value.split_whitespace().collect();
            match toks.as_slice() {
                ["none"] => {
                    ts.flex_grow = 0.0;
                    ts.flex_shrink = 0.0;
                    ts.flex_basis = Dimension::auto();
                }
                ["initial"] => {
                    ts.flex_grow = 0.0;
                    ts.flex_shrink = 1.0;
                    ts.flex_basis = Dimension::auto();
                }
                ["auto"] => {
                    // CSS spec: `auto` ≡ `1 1 auto`（与 initial 对称的关键字）。
                    ts.flex_grow = 1.0;
                    ts.flex_shrink = 1.0;
                    ts.flex_basis = Dimension::auto();
                }
                [g] => {
                    // 单 number → basis=0%（CSS 规范），单 length → basis=该长度。
                    if let Ok(gv) = g.parse::<f32>() {
                        ts.flex_grow = gv;
                        ts.flex_shrink = 1.0;
                        ts.flex_basis = Dimension::percent(0.0);
                    } else if let Some(px) = g
                        .strip_suffix("px")
                        .and_then(|s| s.trim().parse::<f32>().ok())
                    {
                        ts.flex_grow = 1.0;
                        ts.flex_shrink = 1.0;
                        ts.flex_basis = Dimension::length(px);
                    } else {
                        return false;
                    }
                }
                [g, sh] => {
                    // 第二 token 歧义：number → shrink；length/percent/auto → basis（shrink 默认 1）。
                    // 防 `flex:1 50%` / `flex:1 auto` 被误当 shrink（旧实现静默吞值，basis 变 0%）。
                    let gv = match g.parse::<f32>() {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    ts.flex_grow = gv;
                    if let Ok(sv) = sh.parse::<f32>() {
                        ts.flex_shrink = sv;
                        ts.flex_basis = Dimension::percent(0.0);
                    } else if let Some(basis) = parse_flex_basis(sh) {
                        ts.flex_shrink = 1.0;
                        ts.flex_basis = basis;
                    } else {
                        return false;
                    }
                }
                [g, sh, b] => {
                    // 三值：grow(number) shrink(number) basis(length/percent/auto)。
                    // 不用 unwrap_or——解析失败返 false，让调用方/围栏报错（不静默降级）。
                    let gv = match g.parse::<f32>() {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let sv = match sh.parse::<f32>() {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let basis = match parse_flex_basis(b) {
                        Some(b) => b,
                        None => return false,
                    };
                    ts.flex_grow = gv;
                    ts.flex_shrink = sv;
                    ts.flex_basis = basis;
                }
                _ => return false,
            }
            true
        }
        "display" => {
            match value.trim() {
                "none" => {
                    ts.display = taffy::Display::None;
                    style.display_mode = DisplayMode::None;
                }
                "block" => {
                    // Real CSS block flow: taffy 0.12 Block mode stacks children
                    // vertically and, unlike Flex column, ignores flex-grow on
                    // children (they keep their explicit height). display_mode is
                    // still set so internal Strategy selection can branch on it.
                    ts.display = taffy::Display::Block;
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
            // `background` shorthand：按 CSS 优先级依次试 linear-gradient → url() → 纯色。
            // 三者互斥（gradient 走顶点色无采样；url() 走纹理；纯色写 background_color）。
            let v = value.trim();
            if let Some(rest) = v
                .strip_prefix("linear-gradient(")
                .and_then(|s| s.strip_suffix(')'))
            {
                return parse_linear_gradient_2(style, rest);
            }
            if v.starts_with("url(") {
                style.background_image = parse_url(v);
                return style.background_image.is_some();
            }
            if let Some(c) = parse_color(v) {
                style.background_color = Some(c);
                return true;
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
        "border-style" => {
            // CSS longhand：border-style:solid 等。未声明 border-style 时默认 None（不画）。
            style.border_style = match value.trim() {
                "solid" => BorderStyle::Solid,
                "dashed" => BorderStyle::Dashed,
                "dotted" => BorderStyle::Dotted,
                "double" => BorderStyle::Double,
                _ => BorderStyle::None,
            };
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
            // transparent 关键字 + rgba(0,0,0,0)（AI 常与 transparent 混用）都判透明。
            // parse_color 仅认 hex 形式（3/6/8 位），rgba() 函数走不到——此处显式拦截避免渐变字三件套
            // 里 color:rgba(0,0,0,0) 静默退化为不透明黑（AI 可预测性破坏）。
            let norm = value.trim().replace(' ', "").to_lowercase();
            if norm == "transparent" || norm == "rgba(0,0,0,0)" {
                style.color = [0.0, 0.0, 0.0, 0.0];
            } else if let Some(c) = parse_color(value) {
                style.color = c;
            }
            true
        }
        "caret-color" => {
            // CSS caret-color：文本框光标色。`auto`（CSS 初始值）= 用 color 属性 → None
            // （render arm unwrap_or(text_color) 兑现）。其他值走 parse_color（不可解析 → None，
            // 与 background-color 同口径：静默吞坏色值，不报错）。
            if value.trim() == "auto" {
                style.caret_color = None;
            } else {
                style.caret_color = parse_color(value);
            }
            true
        }
        // LoomGUI 私有属性（CSS 用 ::selection 伪元素，围栏无伪元素选择器，故平铺 prop）。
        // None = render 回退到缺省色（selection-background 蓝半透 / selection-color 白）。
        // 不可解析色静默落 None（与 background-color 同口径，不报错）。
        "selection-background" => {
            style.selection_background = parse_color(value);
            true
        }
        "selection-color" => {
            style.selection_color = parse_color(value);
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
            let Some(v) = parse_px(value) else {
                return false;
            };
            style.letter_spacing = v;
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
                let lp = taffy::style::LengthPercentageAuto::length(px);
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
                let lp = taffy::style::LengthPercentageAuto::auto();
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
        "animation" => {
            // class 规则运行时 rematch 走此 arm（spec §5.2 class 触发）：动态规则的
            // animation 声明叠加进 computed style.animation，sync_animation_players (g')
            // 据此启停 player。打包期 inline 走 fence 的 validate + parse（同一 parse_animation）。
            style.animation = parse_animation(value);
            true
        }
        "text-shadow" => {
            // CSS text-shadow: ox oy [blur] color，逗号分隔多阴影。
            // 每段 → FontEffect::Shadow{ox, oy, blur, color}，叠进 text_effects（INHERITED）。
            // blur 可省（默认 0 = 硬边投影）；color 必须可解析（parse_color 仅认 hex 3/6/8 位）。
            // 任一段非法 → 整条声明静默忽略（返 false，与围栏外 CSS 同模式）。
            let shadows = parse_text_shadow(value);
            if shadows.is_empty() {
                return false;
            }
            // text-shadow replaces only its own Shadow effects, composing with
            // font-effect / stroke (different properties compose, not wipe each other).
            style
                .text_effects
                .retain(|e| !matches!(e, crate::text::font_effect::FontEffect::Shadow { .. }));
            style.text_effects.extend(shadows);
            true
        }
        "-webkit-background-clip" | "background-clip" => {
            // 渐变字三件套之一：background-clip:text 将背景渐变裁剪到文字形状。
            // 与 background: linear-gradient + color:transparent（推荐）组合触发
            // per-glyph vertex gradient（build_text_mesh 内 gradient_corner_colors）。
            // 残缺（有 clip 无 gradient）静默回退普通文本，不报错。
            style.background_clip_text = value.trim() == "text";
            true
        }
        "-webkit-text-stroke" => {
            // CSS -webkit-text-stroke: w color（标准 CSS，fact-standard）。
            // → FontEffect::Stroke{w, color}，叠进 text_effects（INHERITED）。
            // 内侧吃字（erode），Front layer（描边在文字上方绘制）。
            // color 必须可解析（parse_color 仅认 hex 3/6/8 位，命名色静默拒）。
            let parts: Vec<&str> = value.split_whitespace().collect();
            if parts.len() < 2 {
                return false;
            }
            let w = match parse_number(parts[0].trim_end_matches("px")) {
                Some(v) => v,
                None => return false,
            };
            let color = match parse_color(parts[1]) {
                Some(c) => c,
                None => return false,
            };
            style
                .text_effects
                .push(crate::text::font_effect::FontEffect::Stroke { w, color });
            true
        }
        "font-effect" => {
            // LoomGUI 私有 CSS：font-effect: glow(w color), blur(w)（逗号分隔多 effect）。
            // glow = dilate + gaussian_blur Back layer（发光晕开），无偏移（居中）。
            // blur = gaussian_blur（可分离高斯两 pass）。
            // 未知 type（非 glow/blur）→ parse_font_effect 返 None → 不 push（静默忽略）。
            let prev_len = style.text_effects.len();
            for spec in value.split(',') {
                if let Some(eff) = parse_font_effect(spec.trim()) {
                    style.text_effects.push(eff);
                }
            }
            style.text_effects.len() > prev_len
        }
        _ => false, // 装饰属性静默忽略
    }
}

/// 解析 CSS `animation` 简写值 → AnimationSpec 列表（逗号分隔多声明展开为多条）。
///
/// 与 `parse_transition` 同构：core 是解析真相源（运行时 rematch 的 apply_decl "animation"
/// arm 调用），fence 打包期 inline 路径委托本函数（fence `parse_animation_value`），
/// 防两份解析器漂移（spec §8.2/§8.3 对齐表唯一真相源 = `css_ease_keyword`）。
///
/// 语义：首个 time=duration、次个 time=delay；ease 关键字按 §8.3 对齐表映射
/// （`ease`→CubicOut，`ease-in/out/in-out`→Quad*，`step-start/end`→Step）；缺省值 =
/// CSS initial（direction=normal / fill=none / play-state=running / iteration-count=1 /
/// timing=ease）。非法段（空 / `none` / 非法 name / 缺 duration）静默丢弃（filter_map）。
pub fn parse_animation(value: &str) -> Vec<crate::style::resolved::AnimationSpec> {
    use crate::style::resolved::AnimationSpec;
    value
        .split(',')
        .filter_map(|decl| parse_one_animation(decl.trim()))
        .collect::<Vec<AnimationSpec>>()
}

/// 单条 animation 声明（逗号分隔的一段）→ AnimationSpec。`none` / 空 / 非法 name → None。
/// 与 fence `validate_one_animation_decl`（打包期严格门）语义对齐；此处宽松（filter_map
/// 丢弃），运行时值已过打包期 validate，越界输入防御性返 None。
fn parse_one_animation(decl: &str) -> Option<crate::style::resolved::AnimationSpec> {
    use crate::style::resolved::{
        AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
    };
    if decl.is_empty() || decl.eq_ignore_ascii_case("none") {
        return None;
    }
    let mut tokens = decl.split_whitespace();
    let name = tokens.next()?;
    if !is_valid_animation_name(name) {
        return None;
    }
    // CSS initial 值起步；显式关键字覆盖对应字段。
    let mut spec = AnimationSpec {
        name: name.to_string(),
        duration: 0.0,
        delay: 0.0,
        iteration_count: Some(1), // CSS initial iteration-count = 1（None = infinite）
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        timing_function: crate::tween::Ease::CubicOut, // CSS animation 默认 ease（§8.3）
        play_state: AnimationPlayState::Running,
    };
    let mut time_count = 0;
    for tok in tokens {
        if let Some(secs) = parse_time_seconds(tok) {
            // 首个 time = duration，次个 time = delay（§8.2）。
            if time_count == 0 {
                spec.duration = secs;
            } else {
                spec.delay = secs;
            }
            time_count += 1;
        } else if tok.eq_ignore_ascii_case("infinite") {
            spec.iteration_count = None;
        } else if tok.chars().all(|c| c.is_ascii_digit()) {
            spec.iteration_count = tok.parse::<u32>().ok();
        } else if let Some(e) = css_ease_keyword(tok) {
            spec.timing_function = e;
        } else {
            match tok.to_ascii_lowercase().as_str() {
                "normal" => spec.direction = AnimationDirection::Normal,
                "reverse" => spec.direction = AnimationDirection::Reverse,
                "alternate" => spec.direction = AnimationDirection::Alternate,
                "alternate-reverse" => spec.direction = AnimationDirection::AlternateReverse,
                "none" => spec.fill_mode = AnimationFillMode::None,
                "forwards" => spec.fill_mode = AnimationFillMode::Forwards,
                "backwards" => spec.fill_mode = AnimationFillMode::Backwards,
                "both" => spec.fill_mode = AnimationFillMode::Both,
                "running" => spec.play_state = AnimationPlayState::Running,
                "paused" => spec.play_state = AnimationPlayState::Paused,
                _ => {} // 未知 token 忽略（validate 门已拦）
            }
        }
    }
    // 与 validate 一致：缺 time（duration）的声明无效。
    if time_count == 0 {
        return None;
    }
    Some(spec)
}

/// animation-name 接受 CSS 自定义标识符（字母/-/_/数字，非数字开头；不允许 `--` 前缀）。
/// 与 fence `is_valid_animation_name` 同语义（fence validate 门打包期拦，此处运行时防御）。
fn is_valid_animation_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    if first == '-' {
        // `-name` 允许；`--name` 是 CSS 变量，不是动画名。
        match chars.next() {
            Some('-') => return false,
            Some(c) if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') => return false,
            None => return false,
            _ => {}
        }
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 解析 CSS `transition` 声明值 → TransitionSpec 列表。
///
/// 逗号分隔多 spec（如 `background-color 0.3s, color 0.3s`）。每段由 `parse_one_transition`
/// 解析。空输入返回空 Vec（未声明 transition）。
///
/// `pub` 供 fence css_resolve 复用（打包期 inline 与运行时 rematch 同一真相源，
/// 防 spec §8.3 ease 对齐表漂移）。
pub fn parse_transition(value: &str) -> Vec<crate::style::resolved::TransitionSpec> {
    value.split(',').filter_map(parse_one_transition).collect()
}

/// 解析单个 transition spec（逗号分隔的一段）。
/// 空格切 token：prop 关键字（all/opacity/color/background-color）→ TweenProp 映射；
/// time（`<n>s`/`<n>ms`）首遇 = duration、次遇 = delay；其余 = ease 关键字（§8.3 对齐表）。
/// 缺省补默认（dur=0s, ease=CubicOut=CSS 初始 ease, delay=0s）。空段返回 None。
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
    let mut ease = Ease::CubicOut; // CSS transition 默认 timing-function = ease（§8.3）
    let mut time_count = 0;
    for t in tokens {
        match t {
            "all" => prop = None,
            "opacity" => prop = Some(TweenProp::Opacity),
            "color" => prop = Some(TweenProp::TextColor),
            "background-color" => prop = Some(TweenProp::BgColor),
            _ => {
                if let Some(secs) = parse_time_seconds(t) {
                    // 首遇 = duration，次遇 = delay（CSS 语义；time_count 防 0s duration 被吞）
                    if time_count == 0 {
                        duration = secs;
                    } else {
                        delay = secs;
                    }
                    time_count += 1;
                } else if let Some(e) = css_ease_keyword(t) {
                    ease = e;
                }
                // 未知 token 忽略（transition 零校验宽松语义，与 fence 一致）
            }
        }
    }
    Some(TransitionSpec {
        prop,
        duration,
        ease,
        delay,
    })
}

/// `<n>s` / `<n>ms` → 秒（None = 非 time token）。
fn parse_time_seconds(tok: &str) -> Option<f32> {
    if let Some(num) = tok.strip_suffix("ms") {
        return num.parse::<f32>().ok().map(|n| n / 1000.0);
    }
    tok.strip_suffix('s')?.parse::<f32>().ok()
}

/// CSS timing-function 关键字 → Ease（spec §8.3 对齐表；唯一真相源）。
///
/// `pub` 供 fence 委托（transition 侧经 `parse_transition`、animation 侧直接调用），
/// 打包期与运行时共用一张表，防双份白名单漂移。本表按小写精确匹配；fence animation
/// 侧 validate 门大小写不敏感，查表前自行 lowercase（见 fence css.rs `ease_from_keyword`）。
pub fn css_ease_keyword(kw: &str) -> Option<crate::tween::Ease> {
    use crate::tween::Ease;
    Some(match kw {
        "linear" => Ease::Linear,
        "ease" => Ease::CubicOut,
        "ease-in" => Ease::QuadIn,
        "ease-out" => Ease::QuadOut,
        "ease-in-out" => Ease::QuadInOut,
        "step-start" => Ease::Step { start: true },
        "step-end" => Ease::Step { start: false },
        _ => return None,
    })
}

/// 解析 CSS `text-shadow` 声明值 → FontEffect::Shadow 列表。
///
/// 逗号分隔多阴影；每段形如 `ox oy [blur] color`（CSS 标准语法）。blur 可省（默认 0），
/// color 必须是 hex 形式（3/6/8 位，parse_color 限制，命名色静默拒）。任一段非法 →
/// 返回空 Vec（apply_decl 据此返 false，整条声明静默忽略——CSS 一条声明全有或全无语义）。
fn parse_text_shadow(value: &str) -> Vec<crate::text::font_effect::FontEffect> {
    value.split(',').filter_map(parse_one_text_shadow).collect()
}

/// 解析单条 text-shadow spec（逗号分隔的一段）。
/// `2px 2px 4px #000` → Shadow{ox:2, oy:2, blur:4, color:black}；
/// `2px 2px #000` → blur=0（硬边投影）。
/// color 省略 → 默认黑（CSS currentColor 围栏不追，降级为黑保可见）；
/// color 存在但不可解析（命名色等）→ None（整条声明非法，CSS 全有或全无）。
fn parse_one_text_shadow(spec: &str) -> Option<crate::text::font_effect::FontEffect> {
    use crate::text::font_effect::FontEffect;
    let parts: Vec<&str> = spec.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let ox = parse_number(parts[0].trim_end_matches("px"))?;
    let oy = parse_number(parts[1].trim_end_matches("px"))?;
    // 第 3 段可能是 blur（数值）或 color；blur 省略时 color 在索引 2。
    // 检查与取值同用 parse_number（一致：均剥 "px" + 拒 '%'，避免检查用 f32::parse 而取值
    // 用 parse_number 的双路径不一致）。
    let (blur, color_idx) = match parts
        .get(2)
        .and_then(|s| parse_number(s.trim_end_matches("px")))
    {
        Some(b) => (b, 3),
        None => (0.0, 2),
    };
    // color 缺省（索引越界）→ 默认黑；color 存在但解析失败 → 整段非法。
    let color = match parts.get(color_idx).copied() {
        None => [0.0, 0.0, 0.0, 1.0],
        Some(c) => parse_color(c)?,
    };
    Some(FontEffect::Shadow {
        ox,
        oy,
        blur,
        color,
    })
}

/// 解析单个 LoomGUI 私有 font-effect：`glow(w color)` / `blur(w)`。
///
/// - `glow`：dilate 膨胀 + gaussian_blur 晕开，Back layer（居中），颜色可选（默认白）。
/// - `blur`：可分离高斯两 pass。
/// - 未知 type → None（apply_decl 静默跳过）。
fn parse_font_effect(s: &str) -> Option<crate::text::font_effect::FontEffect> {
    use crate::text::font_effect::FontEffect;
    let s = s.trim();
    if let Some(args) = s.strip_prefix("glow(").and_then(|x| x.strip_suffix(")")) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let w_raw = parts.first()?;
        if w_raw.contains('%') {
            return None; // font-effect width is px only, % meaningless
        }
        let w = parse_number(w_raw.trim_end_matches("px"))?;
        let color = parts
            .get(1)
            .and_then(|c| parse_color(c))
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        Some(FontEffect::Glow { w, color })
    } else if let Some(args) = s.strip_prefix("blur(").and_then(|x| x.strip_suffix(")")) {
        let w_raw = args.trim();
        if w_raw.contains('%') {
            return None; // font-effect width is px only, % meaningless
        }
        let w = parse_number(w_raw.trim_end_matches("px"))?;
        Some(FontEffect::Blur { w })
    } else {
        None // 未知 type → None，apply_decl 不 push
    }
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
    // JustifyContent 是 AlignContent 的类型别名，用全路径构造。
    // taffy 0.11+：对齐常量从 PascalCase 变体改成 SCREAMING_SNAKE 关联常量。
    match v.trim() {
        "center" => taffy::AlignContent::CENTER,
        "flex-end" => taffy::AlignContent::FLEX_END,
        "space-between" => taffy::AlignContent::SPACE_BETWEEN,
        "space-around" => taffy::AlignContent::SPACE_AROUND,
        "space-evenly" => taffy::AlignContent::SPACE_EVENLY,
        _ => taffy::AlignContent::FLEX_START,
    }
}
fn parse_align(v: &str) -> taffy::AlignItems {
    match v.trim() {
        "center" => taffy::AlignItems::CENTER,
        "flex-end" => taffy::AlignItems::FLEX_END,
        "stretch" => taffy::AlignItems::STRETCH,
        "baseline" => taffy::AlignItems::BASELINE,
        _ => taffy::AlignItems::FLEX_START,
    }
}

#[cfg(test)]
mod tests;
