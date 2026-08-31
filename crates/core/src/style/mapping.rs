use crate::scene::animation::TransformAnim;
use crate::style::color_filter::{self, IDENTITY};
use crate::style::resolved::{
    BackgroundSize, BorderRadius, BorderStyle, BoxShadow, CornerRadius, CursorStyle,
    DeferredLength, DisplayMode, GradCoord, Gradient, GradientStop, OverflowMode, OverflowWrap,
    RadialExtent, RadialShape, ResolvedStyle, SafeSide, SliceInsets, TextAlign, TextDecoration,
    TextSecurity, TextWrap, TransformOrigin, ViewportLen, ViewportUnit, WhiteSpace, WordBreak,
    GRADIENT_MAX_STOPS, MAX_INSET_SHADOW_LAYERS, MAX_OUTER_SHADOW_LAYERS,
};
use crate::transform::LenPct;
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
    // LengthPercentage 是 compact tagged pointer，变体匹配走 expand（taffy 0.14 正规 API，
    // 替代 0.12 期 into_raw+tag 手解）。calc 变体来自默认 feature 面，parse_lp 产不出，
    // 落 Length(0) 与旧兜底同义。
    match parse_lp(s).expand() {
        taffy::style::ExpandedLengthPercentage::Length(v) => Dimension::length(v),
        taffy::style::ExpandedLengthPercentage::Percent(v) => Dimension::percent(v),
        _ => Dimension::length(0.0),
    }
}

/// 视口相对单位（`vw`/`vh`/`vmin`/`vmax`）解析口。全长度属性通道可接（值域由围栏
/// CSS_PROPS 门控，core 侧按能力全放开——运行时动态规则的第二真相源不窄于围栏）。
fn try_viewport(s: &str) -> Option<ViewportLen> {
    let s = s.trim();
    const SUFFIXES: [(&str, ViewportUnit); 4] = [
        ("vw", ViewportUnit::Vw),
        ("vh", ViewportUnit::Vh),
        ("vmin", ViewportUnit::Vmin),
        ("vmax", ViewportUnit::Vmax),
    ];
    for (suffix, unit) in SUFFIXES {
        if let Some(num) = s.strip_suffix(suffix) {
            if let Ok(v) = num.trim().parse::<f32>() {
                return Some(ViewportLen { value: v, unit });
            }
        }
    }
    None
}

/// `env(safe-area-inset-top/right/bottom/left)` 四值（safe-area 之外的 env() 名
/// 不认——围栏拒、core 同口径静默 None）。大小写敏感（CSS env 名如此）。
fn try_env(s: &str) -> Option<SafeSide> {
    match s.trim() {
        "env(safe-area-inset-top)" => Some(SafeSide::Top),
        "env(safe-area-inset-right)" => Some(SafeSide::Right),
        "env(safe-area-inset-bottom)" => Some(SafeSide::Bottom),
        "env(safe-area-inset-left)" => Some(SafeSide::Left),
        _ => None,
    }
}

/// 延迟长度解析口：视口单位或 env()。
fn try_deferred(s: &str) -> Option<DeferredLength> {
    try_viewport(s)
        .map(DeferredLength::Viewport)
        .or_else(|| try_env(s).map(DeferredLength::SafeInset))
}

/// 尺寸族声明统一入口（width/height/min-*/max-*/flex-basis）：延迟长度进
/// `style.viewport` 平行槽（taffy 落 length(0) 占位，solve 期按 root/safe 换算覆写）；
/// px/%/auto 进 taffy 并清平行槽——CSS 级联后者胜出，px 覆写 vw 后 vw 必须失效。
fn apply_size_decl(vp: &mut Option<DeferredLength>, ts_slot: &mut Dimension, value: &str) {
    if let Some(v) = try_deferred(value) {
        *vp = Some(v);
        *ts_slot = Dimension::length(0.0);
    } else {
        *vp = None;
        *ts_slot = parse_dimension(value);
    }
}

/// min/max 槽专用：taffy 0.14 起 `Style.min_size/max_size` 分型为 LengthPercentageAuto
/// （size 槽仍是 Dimension）。复用 apply_size_decl 的视口/级联语义后换型落位——
/// Length/Percent 直映，auto 落 AUTO（min-width:auto / max-width:auto = 无约束）。
fn apply_minmax_decl(
    vp: &mut Option<DeferredLength>,
    ts_slot: &mut LengthPercentageAuto,
    value: &str,
) {
    let mut d = Dimension::auto();
    apply_size_decl(vp, &mut d, value);
    *ts_slot = match d.expand() {
        taffy::style::ExpandedDimension::Length(v) => LengthPercentageAuto::length(v),
        taffy::style::ExpandedDimension::Percent(v) => LengthPercentageAuto::percent(v),
        _ => LengthPercentageAuto::auto(),
    };
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

/// 单 token px 或延迟长度（视口单位/env()）。px 落 LengthPercentage + 清覆盖槽，
/// 延迟落 length(0) 占位 + 进覆盖槽。
fn parse_px_or_deferred(tok: &str) -> Option<(LengthPercentage, Option<DeferredLength>)> {
    let tok = tok.trim();
    if let Some(v) = try_deferred(tok) {
        return Some((LengthPercentage::length(0.0), Some(v)));
    }
    let px = tok
        .strip_suffix("px")
        .unwrap_or(tok)
        .trim()
        .parse::<f32>()
        .ok()?;
    Some((LengthPercentage::length(px), None))
}

/// padding 围栏 px/延迟长度 → ([t,r,b,l] taffy 值, [t,r,b,l] 延迟覆盖)。
/// 任一 token 皆非 → None（整条声明无效）。% 不开（padding px-only 基线保持，
/// 响应式走视口单位——票面拍板）。
fn parse_padding_four(s: &str) -> Option<([LengthPercentage; 4], [Option<DeferredLength>; 4])> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| parse_px_or_deferred(parts.get(i)?);
    let (v0, d0) = p(0)?;
    Some(match parts.len() {
        1 => ([v0, v0, v0, v0], [d0, d0, d0, d0]),
        2 => {
            let (v1, d1) = p(1)?;
            ([v0, v1, v0, v1], [d0, d1, d0, d1])
        }
        3 => {
            let (v1, d1) = p(1)?;
            let (v2, d2) = p(2)?;
            ([v0, v1, v2, v1], [d0, d1, d2, d1])
        }
        _ => {
            let (v1, d1) = p(1)?;
            let (v2, d2) = p(2)?;
            let (v3, d3) = p(3)?;
            ([v0, v1, v2, v3], [d0, d1, d2, d3])
        }
    })
}

/// gap 值对 [(row, col)]：1 token → 同值；2 token → (row, col)；更多 → None
/// （CSS gap 只收 1-2 值，整条无效——多 token 静默取前两值会造成预览/运行时口径分叉）。
fn parse_gap_pair(s: &str) -> Option<[(LengthPercentage, Option<DeferredLength>); 2]> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| parse_px_or_deferred(parts.get(i)?);
    match parts.len() {
        1 => {
            let v = p(0)?;
            Some([v, v])
        }
        2 => Some([p(0)?, p(1)?]),
        _ => None,
    }
}

/// margin 围栏 px/%/auto/延迟长度 → ([t,r,b,l] taffy 值, [t,r,b,l] 延迟覆盖)。
/// 任一 token 皆非（em/rem/keyword）→ None。兑现 fence 承诺：
/// `margin:10%` → Percent，`margin:auto` → Auto（居中），`margin:0 auto` →
/// top/bottom Length(0)、left/right Auto；延迟 token 进覆盖槽 + taffy 落 0 占位。
fn parse_margin_four(s: &str) -> Option<([LengthPercentageAuto; 4], [Option<DeferredLength>; 4])> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| -> Option<(LengthPercentageAuto, Option<DeferredLength>)> {
        let x = parts.get(i)?.trim();
        if let Some(v) = try_deferred(x) {
            return Some((LengthPercentageAuto::length(0.0), Some(v)));
        }
        if x == "auto" {
            return Some((LengthPercentageAuto::auto(), None));
        }
        if let Some(pct) = x.strip_suffix('%') {
            return Some((
                LengthPercentageAuto::percent(pct.parse::<f32>().ok()? / 100.0),
                None,
            ));
        }
        let px = x
            .strip_suffix("px")
            .unwrap_or(x)
            .trim()
            .parse::<f32>()
            .ok()?;
        Some((LengthPercentageAuto::length(px), None))
    };
    Some(match parts.len() {
        1 => {
            let (v, vp) = p(0)?;
            ([v, v, v, v], [vp, vp, vp, vp])
        }
        2 => {
            let (a, va) = p(0)?;
            let (b, vb) = p(1)?;
            ([a, b, a, b], [va, vb, va, vb])
        }
        3 => {
            let (a, va) = p(0)?;
            let (b, vb) = p(1)?;
            let (c, vc) = p(2)?;
            ([a, b, c, b], [va, vb, vc, vb])
        }
        _ => {
            let (a, va) = p(0)?;
            let (b, vb) = p(1)?;
            let (c, vc) = p(2)?;
            let (d, vd) = p(3)?;
            ([a, b, c, d], [va, vb, vc, vd])
        }
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

/// 解析 border-radius 1~4 值（每值 px/%/延迟长度）→ ([TL,TR,BR,BL] taffy 值,
/// [TL,TR,BR,BL] 延迟覆盖)。与 parse_four 同序。任一值非法（auto/inherit/initial/
/// 非数字）→ None（CSS：整条声明无效）。
fn parse_radius_group(s: &str) -> Option<([LengthPercentage; 4], [Option<DeferredLength>; 4])> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let p = |i: usize| -> Option<(LengthPercentage, Option<DeferredLength>)> {
        let tok = parts.get(i)?.trim();
        if tok == "auto" || tok == "inherit" || tok == "initial" {
            return None;
        }
        if let Some(v) = try_deferred(tok) {
            return Some((LengthPercentage::length(0.0), Some(v)));
        }
        // parse_lp 对垃圾（如 "abc"）静默返回 Length(0)，需额外校验：
        // 合法 token = 裸数字 / 数字px / 数字%
        let num_part = tok.trim_end_matches("px").trim_end_matches('%');
        if num_part.trim().parse::<f32>().is_err() {
            return None;
        }
        Some((parse_lp(tok), None))
    };
    let (v0, d0) = p(0)?;
    Some(match parts.len() {
        1 => ([v0, v0, v0, v0], [d0, d0, d0, d0]),
        2 => {
            let (v1, d1) = p(1)?;
            ([v0, v1, v0, v1], [d0, d1, d0, d1])
        }
        3 => {
            let (v1, d1) = p(1)?;
            let (v2, d2) = p(2)?;
            ([v0, v1, v2, v1], [d0, d1, d2, d1])
        }
        _ => {
            let (v1, d1) = p(1)?;
            let (v2, d2) = p(2)?;
            let (v3, d3) = p(3)?;
            ([v0, v1, v2, v3], [d0, d1, d2, d3])
        }
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
    } else if s.len() == 4 {
        // CSS Color Module Level 4 `#rgba`：3 位 hex 色的 alpha 简写（#000a = 黑色 α=0xaa/ff）。
        // 与 3 位 hex 同构（digit d → d*17），末位为 alpha。补全 3/8 位 hex 之间的缺口——
        // box-shadow / overlay 常用半透明色，缺失会令合法 CSS 被围栏拒收。
        let r = u8::from_str_radix(&s[0..1], 16).ok()?;
        let g = u8::from_str_radix(&s[1..2], 16).ok()?;
        let b = u8::from_str_radix(&s[2..3], 16).ok()?;
        let a = u8::from_str_radix(&s[3..4], 16).ok()?;
        Some([
            (r * 17) as f32 / 255.0,
            (g * 17) as f32 / 255.0,
            (b * 17) as f32 / 255.0,
            (a * 17) as f32 / 255.0,
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

/// 解析 `linear-gradient(...)` / `radial-gradient(...)` 声明值（含函数名）。
/// 成功 → `Gradient`；围栏外（conic / repeating-* / 坏语法 / 超 8 stops）→ None
/// （apply_decl 返 false，inline style 打包期报错，`<style>` 规则运行时忽略）。
/// fence 的 `<style>` 探针也走此函数（单一解析真相源）。
pub fn parse_gradient(value: &str) -> Option<Gradient> {
    let v = value.trim();
    if let Some(inner) = v
        .strip_prefix("linear-gradient(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return parse_linear_gradient(inner);
    }
    if let Some(inner) = v
        .strip_prefix("radial-gradient(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return parse_radial_gradient(inner);
    }
    None
}

/// 按顶层逗号切分（括号内逗号不属于分隔符——rgba(...) / cubic-bezier(...) 内含逗号）。
/// animation/transition 多声明分割与 fence validate 门共用（防函数参数被切成独立声明）。
pub fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts
}

/// 按顶层空白切分（括号内空白属于当前 token——`rgba(0, 0, 0, 0.5) 60%` 切成两 token）。
fn split_top_level_ws(s: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

/// 线性方向 → CSS 角度（0deg=to top，顺时针）。角点方向（to top right）defer → None。
fn linear_dir_angle(tok: &str) -> Option<f32> {
    match tok {
        "to top" => Some(0.0),
        "to right" => Some(90.0),
        "to bottom" => Some(180.0),
        "to left" => Some(270.0),
        _ => tok
            .strip_suffix("deg")
            .and_then(|n| n.trim().parse::<f32>().ok()),
    }
}

/// stop 位置：`60%` → 0.6；`0` → 0（CSS 唯一合法裸数）。其他裸数/单位 → None。
fn stop_pos(tok: &str) -> Option<f32> {
    if let Some(pct) = tok.strip_suffix('%') {
        return pct.trim().parse::<f32>().ok().map(|v| v / 100.0);
    }
    if tok == "0" {
        return Some(0.0);
    }
    None
}

/// stop 色：hex/rgb()/rgba() + `transparent` 关键字（渐变专属——parse_color 全局收
/// transparent 会令 schema 默认值 `background-color:transparent` 表示层漂移 None→Some）。
/// 其余命名色围栏外。
fn stop_color(tok: &str) -> Option<[f32; 4]> {
    if tok.eq_ignore_ascii_case("transparent") {
        return Some([0.0, 0.0, 0.0, 0.0]);
    }
    parse_color(tok)
}

/// 解析 stop 列表（每项 `color [pos]`），烘默认位置 + 钳单调。1..=8 项，否则 None。
fn parse_gradient_stops(parts: &[&str]) -> Option<Vec<GradientStop>> {
    if parts.is_empty() || parts.len() > GRADIENT_MAX_STOPS {
        return None;
    }
    let mut raw: Vec<([f32; 4], Option<f32>)> = Vec::with_capacity(parts.len());
    for p in parts {
        let toks = split_top_level_ws(p);
        if toks.is_empty() || toks.len() > 2 {
            return None;
        }
        let color = stop_color(&toks[0])?;
        let pos = match toks.get(1) {
            Some(t) => Some(stop_pos(t)?),
            None => None,
        };
        raw.push((color, pos));
    }
    // CSS 默认位置算法：全缺省 → 0..1 等分；否则首缺省 0 / 末缺省 1 /
    // 中段缺省取两侧最近已定位 stop 的等分插值。随后钳单调不减（乱序 stop 提到前 stop 位置）。
    let n = raw.len();
    let mut pos: Vec<f32> = raw.iter().map(|(_, p)| p.unwrap_or(f32::NAN)).collect();
    if n == 1 {
        pos[0] = if pos[0].is_nan() { 0.0 } else { pos[0] };
    } else if pos.iter().all(|v| v.is_nan()) {
        for (k, v) in pos.iter_mut().enumerate() {
            *v = k as f32 / (n - 1) as f32;
        }
    } else {
        // 前导缺省 → 0（首个已定位 stop 之前）。
        for v in pos.iter_mut() {
            if v.is_nan() {
                *v = 0.0;
            } else {
                break;
            }
        }
        // 末尾缺省 → 1。
        for v in pos.iter_mut().rev() {
            if v.is_nan() {
                *v = 1.0;
            } else {
                break;
            }
        }
        // 中段缺省 run：[i..j) 夹在已定位的 i-1 与 j 之间等分。
        let mut i = 0;
        while i < n {
            if pos[i].is_nan() {
                let mut j = i;
                while j < n && pos[j].is_nan() {
                    j += 1;
                }
                let a = if i > 0 { pos[i - 1] } else { 0.0 };
                let b = if j < n { pos[j] } else { 1.0 };
                let run = j - i;
                for (k, v) in pos.iter_mut().enumerate().take(j).skip(i) {
                    *v = a + (b - a) * (k - i + 1) as f32 / (run + 1) as f32;
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
    let mut prev = f32::NEG_INFINITY;
    let stops = raw
        .into_iter()
        .zip(pos)
        .map(|((color, _), mut p)| {
            p = p.max(prev);
            prev = p;
            GradientStop { color, pos: p }
        })
        .collect();
    Some(stops)
}

/// 解析 `linear-gradient` 内部串（已去函数名/外括号）。
/// `[angle | to-dir]? , stop [, stop]*`；缺省方向 = to bottom（CSS 默认 180deg）。
fn parse_linear_gradient(inner: &str) -> Option<Gradient> {
    let parts = split_top_level_commas(inner);
    if parts.len() < 2 {
        return None; // 至少 1 stop；纯 `linear-gradient(color)` 不合法
    }
    let (angle_deg, stop_parts) = match linear_dir_angle(parts[0]) {
        Some(a) => (a, &parts[1..]),
        None => (180.0, &parts[..]),
    };
    let stops = parse_gradient_stops(stop_parts)?;
    Some(Gradient::Linear { angle_deg, stops })
}

fn radial_size_keyword(tok: &str) -> Option<RadialExtent> {
    match tok {
        "closest-side" => Some(RadialExtent::ClosestSide),
        "farthest-side" => Some(RadialExtent::FarthestSide),
        "closest-corner" => Some(RadialExtent::ClosestCorner),
        "farthest-corner" => Some(RadialExtent::FarthestCorner),
        _ => None,
    }
}

/// 长度 token（`100px` / 裸数）→ px。
fn grad_px(tok: &str) -> Option<f32> {
    tok.strip_suffix("px")
        .unwrap_or(tok)
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
}

/// radial 坐标：`82%` → Pct(0.82)；`-12%` → Pct(-0.12)；`40px`/裸数 → Px。
fn grad_coord(tok: &str) -> Option<GradCoord> {
    if let Some(pct) = tok.strip_suffix('%') {
        return pct
            .trim()
            .parse::<f32>()
            .ok()
            .map(|v| GradCoord::Pct(v / 100.0));
    }
    grad_px(tok).map(GradCoord::Px)
}

/// 解析 radial 配置段（`circle closest-side at 82% -12%`）。
/// shape / size 任意序（CSS `||` 语法），`at` 后恰 2 坐标。None = 不是合法配置段。
fn parse_radial_config(s: &str) -> Option<(RadialShape, RadialExtent, [GradCoord; 2])> {
    let toks = split_top_level_ws(s);
    if toks.is_empty() {
        return None;
    }
    let mut shape: Option<&str> = None;
    let mut extent: Option<RadialExtent> = None;
    let mut len1: Option<f32> = None;
    let mut len2: Option<f32> = None;
    let mut center = [GradCoord::Pct(0.5), GradCoord::Pct(0.5)];
    let mut i = 0usize;
    while i < toks.len() {
        let t = toks[i].as_str();
        if t == "circle" || t == "ellipse" {
            if shape.is_some() {
                return None;
            }
            shape = Some(t);
            i += 1;
        } else if let Some(kw) = radial_size_keyword(t) {
            if extent.is_some() || len1.is_some() {
                return None;
            }
            extent = Some(kw);
            i += 1;
        } else if t == "at" {
            // at 后必须恰有 2 个坐标（get 越界 / 坐标不可解析 → 整体 None）。
            let cx = toks.get(i + 1).and_then(|t| grad_coord(t))?;
            let cy = toks.get(i + 2).and_then(|t| grad_coord(t))?;
            center = [cx, cy];
            i += 3;
            if i != toks.len() {
                return None; // at 坐标必须是段尾
            }
        } else {
            let v = grad_px(t)?;
            if extent.is_some() || len1.is_some() && len2.is_some() {
                return None;
            }
            if len1.is_none() {
                len1 = Some(v);
            } else {
                len2 = Some(v);
            }
            i += 1;
        }
    }
    // shape 与显式长度的合法性：circle 恰 1 长度（或 0）；ellipse 0/2 长度。
    match shape {
        Some("circle") if len2.is_some() => return None,
        Some("ellipse") if len1.is_some() != len2.is_some() => return None,
        _ => {}
    }
    let extent = match (extent, len1, len2) {
        (Some(kw), None, None) => kw,
        (None, None, None) => RadialExtent::FarthestCorner,
        (None, Some(a), b) => RadialExtent::Explicit(Some(a), b),
        _ => return None, // 尺寸关键字与显式长度混用 → 非法
    };
    let shape = match shape {
        Some("circle") => RadialShape::Circle,
        _ => RadialShape::Ellipse,
    };
    Some((shape, extent, center))
}

/// 解析 `radial-gradient` 内部串（已去函数名/外括号）。
/// `[shape || size]? [at position]? , stop [, stop]*`；缺省 = ellipse farthest-corner 50% 50%。
fn parse_radial_gradient(inner: &str) -> Option<Gradient> {
    let parts = split_top_level_commas(inner);
    if parts.len() < 2 {
        return None;
    }
    let (shape, extent, center, stop_parts) = match parse_radial_config(parts[0]) {
        Some((sh, e, c)) => (sh, e, c, &parts[1..]),
        None => (
            RadialShape::Ellipse,
            RadialExtent::FarthestCorner,
            [GradCoord::Pct(0.5), GradCoord::Pct(0.5)],
            &parts[..],
        ),
    };
    let stops = parse_gradient_stops(stop_parts)?;
    Some(Gradient::Radial {
        extent,
        shape,
        center,
        stops,
    })
}

use crate::style::resolved::LocalTransform;
use crate::transform::{self, Affine2};

/// 解析 CSS `transform` 声明值为累积 Affine2 矩阵。
/// 支持 translate/translateX/translateY(px)/rotate(deg)/scale/scaleX/scaleY(num[,num])；
/// skew/matrix()/%/3D 静默跳过。一轴变体（translateX/Y、scaleX/Y）与 `parse_transform_trs`
/// （关键帧路径）保持一致——showcase CSS 用 `translateY(-6px)` 表达 hover 上浮。
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
                let x = parse_len_pct(parts[0])?;
                let y = if let Some(y) = parts.get(1) {
                    parse_len_pct(y)?
                } else {
                    LenPct::ZERO
                };
                out.translate = Some([x, y]);
            }
            "translateX" => {
                if parts.len() != 1 {
                    return None;
                }
                out.translate = Some([parse_len_pct(parts[0])?, LenPct::ZERO]);
            }
            "translateY" => {
                if parts.len() != 1 {
                    return None;
                }
                out.translate = Some([LenPct::ZERO, parse_len_pct(parts[0])?]);
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

/// 解析 `<length-percentage>` 单值 → LenPct 混合长度（keyframes translate 通道，#77）。
/// 收 `12px` / `12` / `50%` / 混合 calc 语法不收（DSL 侧单形即可表达 AI 常用写法）。
/// 百分比存储描述符、采样写入期按节点尺寸解析（见 animation.rs compose_transform）。
pub fn parse_len_pct(v: &str) -> Option<crate::transform::LenPct> {
    let v = v.trim();
    if let Some(pct) = v.strip_suffix('%') {
        let pct = pct.trim().parse::<f32>().ok()?;
        return Some(crate::transform::LenPct { px: 0.0, pct });
    }
    let px = v.trim_end_matches("px").trim().parse::<f32>().ok()?;
    Some(crate::transform::LenPct { px, pct: 0.0 })
}

/// 解析 CSS `transform-origin` 值：1-2 个 `<length|%>|left|center|right|top|bottom>`。
/// 单值 → y 缺省 center(50%)（CSS 语义）。全非法 → None（apply_decl 报值错）。
/// 例：`50% 50%`（=default）/ `left top` / `10px 20px` / `center`。
pub fn parse_transform_origin(value: &str) -> Option<TransformOrigin> {
    let origin_kw = |t: &str| -> Option<crate::transform::LenPct> {
        match t {
            "left" | "top" => Some(crate::transform::LenPct { px: 0.0, pct: 0.0 }),
            "center" => Some(crate::transform::LenPct { px: 0.0, pct: 50.0 }),
            "right" | "bottom" => Some(crate::transform::LenPct {
                px: 0.0,
                pct: 100.0,
            }),
            _ => parse_len_pct(t),
        }
    };
    let parts: Vec<&str> = value.split_whitespace().collect();
    match parts.len() {
        1 => {
            let x = origin_kw(parts[0])?;
            Some(TransformOrigin {
                x,
                y: crate::transform::LenPct { px: 0.0, pct: 50.0 },
            })
        }
        2 => Some(TransformOrigin {
            x: origin_kw(parts[0])?,
            y: origin_kw(parts[1])?,
        }),
        _ => None,
    }
}

fn iter_transform_funcs(s: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
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
        i += 1;
        let args_start = i;
        while i < bytes.len() && bytes[i] != b')' {
            i += 1;
        }
        let args = &s[args_start..i];
        if i < bytes.len() {
            i += 1;
        }
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
        // 一轴便捷写法（CSS 标准）：translateX/Y 只动单轴，另一轴 0。
        "translateX" => {
            let x = parse_px(parts.first().copied().unwrap_or("0"))?;
            Some(transform::from_translate(x, 0.0))
        }
        "translateY" => {
            let y = parse_px(parts.first().copied().unwrap_or("0"))?;
            Some(transform::from_translate(0.0, y))
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
        // 一轴缩放：另一轴保持 1。
        "scaleX" => {
            let sx = parts.first().copied().unwrap_or("1").parse::<f32>().ok()?;
            Some(transform::from_scale(sx, 1.0))
        }
        "scaleY" => {
            let sy = parts.first().copied().unwrap_or("1").parse::<f32>().ok()?;
            Some(transform::from_scale(1.0, sy))
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

/// padding-top/right/bottom/left 单边 longhand：设 ts.padding 对应边 + 延迟覆盖槽，
/// 不动其他三边。px/延迟长度（同 padding 简写口径）；皆非 → false。
fn apply_padding_side(style: &mut ResolvedStyle, side: Side, value: &str) -> bool {
    let ([v, _, _, _], [d, _, _, _]) = match parse_padding_four(value) {
        Some(f) => f,
        None => return false,
    };
    let lp = v;
    let idx = match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    };
    style.viewport.padding[idx] = d;
    let ts = &mut style.taffy_style;
    match side {
        Side::Top => ts.padding.top = lp,
        Side::Right => ts.padding.right = lp,
        Side::Bottom => ts.padding.bottom = lp,
        Side::Left => ts.padding.left = lp,
    }
    true
}

/// margin 单边声明：与 apply_padding_side 同构，但走 parse_margin_four（支持 px/%/auto/视口单位）。
/// 只设指定边，其余边保持不动（不重置四边）。
fn apply_margin_side(style: &mut ResolvedStyle, side: Side, value: &str) -> bool {
    let ([v, _, _, _], [vp, _, _, _]) = match parse_margin_four(value) {
        Some(f) => f,
        None => return false,
    };
    let idx = match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    };
    style.viewport.margin[idx] = vp;
    let ts = &mut style.taffy_style;
    match side {
        Side::Top => ts.margin.top = v,
        Side::Right => ts.margin.right = v,
        Side::Bottom => ts.margin.bottom = v,
        Side::Left => ts.margin.left = v,
    }
    true
}

/// inset 单边写入（top/right/bottom/left longhand 与 `inset` 简写共用）。
/// idx 序 [top, right, bottom, left]。px/%/auto/延迟长度同 longhand 值域。
fn apply_inset_side(style: &mut ResolvedStyle, idx: usize, value: &str) -> bool {
    let lp = if let Some(v) = try_deferred(value) {
        style.viewport.inset[idx] = Some(v);
        taffy::style::LengthPercentageAuto::length(0.0)
    } else {
        style.viewport.inset[idx] = None;
        if let Some(px) = parse_px(value) {
            taffy::style::LengthPercentageAuto::length(px)
        } else if let Some(pct) = value.trim().strip_suffix('%') {
            match pct.trim().parse::<f32>() {
                Ok(v) => taffy::style::LengthPercentageAuto::percent(v / 100.0),
                Err(_) => return false,
            }
        } else if value.trim() == "auto" {
            taffy::style::LengthPercentageAuto::auto()
        } else {
            return false;
        }
    };
    let ts = &mut style.taffy_style;
    match idx {
        0 => ts.inset.top = lp,
        1 => ts.inset.right = lp,
        2 => ts.inset.bottom = lp,
        _ => ts.inset.left = lp,
    }
    true
}

/// 把一条 declaration 应用到 style（覆盖对应字段）。返回是否被识别。
pub fn apply_decl(style: &mut ResolvedStyle, prop: &str, value: &str) -> bool {
    let ts = &mut style.taffy_style;
    match prop.trim() {
        "width" => {
            apply_size_decl(&mut style.viewport.width, &mut ts.size.width, value);
            true
        }
        "height" => {
            apply_size_decl(&mut style.viewport.height, &mut ts.size.height, value);
            true
        }
        "min-width" => {
            apply_minmax_decl(&mut style.viewport.min_width, &mut ts.min_size.width, value);
            true
        }
        "min-height" => {
            apply_minmax_decl(
                &mut style.viewport.min_height,
                &mut ts.min_size.height,
                value,
            );
            true
        }
        "max-width" => {
            apply_minmax_decl(&mut style.viewport.max_width, &mut ts.max_size.width, value);
            true
        }
        "max-height" => {
            apply_minmax_decl(
                &mut style.viewport.max_height,
                &mut ts.max_size.height,
                value,
            );
            true
        }
        "padding" => {
            let ([t, r, b, l], d) = match parse_padding_four(value) {
                Some(v) => v,
                None => return false,
            };
            style.viewport.padding = d;
            ts.padding = Rect {
                left: l,
                right: r,
                top: t,
                bottom: b,
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
            let ([t, r, b, l], vp) = match parse_margin_four(value) {
                Some(v) => v,
                None => return false,
            };
            style.viewport.margin = vp;
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
            let (h, dh) = match parse_radius_group(h_group) {
                Some(g) => g,
                None => return false,
            };
            let (v, dv) = match parse_radius_group(v_group) {
                Some(g) => g,
                None => return false,
            };
            style.viewport.border_radius = [dh, dv];
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
            let [(row, drow), (col, dcol)] = match parse_gap_pair(value) {
                Some(v) => v,
                None => return false,
            };
            style.viewport.row_gap = drow;
            style.viewport.column_gap = dcol;
            ts.gap = Size {
                width: col,
                height: row,
            };
            true
        }
        // CSS `gap` longhand：row-gap 对应纵向间距（gap.height），column-gap 横向（gap.width），
        // 与上方 `gap` shorthand 拆分语义一致。px/延迟长度（裸数字与 px 后缀等价，
        // 否则 default `0` 会被静默拒）。
        "row-gap" => {
            let (v, d) = match value
                .split_whitespace()
                .next()
                .and_then(parse_px_or_deferred)
            {
                Some(v) => v,
                None => return false,
            };
            style.viewport.row_gap = d;
            ts.gap.height = v;
            true
        }
        "column-gap" => {
            let (v, d) = match value
                .split_whitespace()
                .next()
                .and_then(parse_px_or_deferred)
            {
                Some(v) => v,
                None => return false,
            };
            style.viewport.column_gap = d;
            ts.gap.width = v;
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
            apply_size_decl(&mut style.viewport.flex_basis, &mut ts.flex_basis, value);
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
        "border-image-slice" => match parse_slice(value) {
            Some(ins) => {
                style.border_image_slice = Some(ins);
                true
            }
            None => false,
        },
        "background-color" => {
            style.background_color = parse_color(value);
            true
        }
        "background-image" => {
            // `background-image: <gradient>` 走渐变；否则走现有 url() 解析。
            // 渐变与 url() 互斥（gradient 走 program=6 渐变 shader，无纹理采样）。
            if let Some(g) = parse_gradient(value.trim()) {
                style.background_gradient = Some(g);
                return true;
            }
            style.background_image = parse_url(value);
            style.background_image.is_some()
        }
        "background" => {
            // `background` shorthand：按 CSS 优先级依次试 gradient → url() → 纯色。
            // 三者互斥（gradient 走渐变 shader；url() 走纹理；纯色写 background_color）。
            let v = value.trim();
            if let Some(g) = parse_gradient(v) {
                style.background_gradient = Some(g);
                return true;
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
                "100%" | "stretch" => BackgroundSize::Stretch,
                _ => return false, // 围栏外值（auto/px/两值）静默忽略
            };
            true
        }
        "background-repeat" => {
            style.background_repeat = match value.trim() {
                "repeat" => crate::style::resolved::BackgroundRepeat::Repeat,
                "no-repeat" => crate::style::resolved::BackgroundRepeat::NoRepeat,
                "repeat-x" => crate::style::resolved::BackgroundRepeat::RepeatX,
                "repeat-y" => crate::style::resolved::BackgroundRepeat::RepeatY,
                _ => return false,
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
        "text-decoration" => {
            // 值集 none|underline（围栏 schema 已拒 line-through 等越界值，此处按
            // border-style 同款兜底映射：未知值落 None，不报错——打包期门负责报错）。
            style.text_decoration = match value.trim() {
                "underline" => TextDecoration::Underline,
                _ => TextDecoration::None,
            };
            true
        }
        "cursor" => {
            // 值集 auto|default|none|pointer（#93；围栏 schema 已拒越界值）。
            // Auto = UA 默认（pressable 控件悬停手型，runtime cursor_intent 决策）；
            // 显式声明恒压 UA 行为。
            style.cursor = match value.trim() {
                "pointer" => CursorStyle::Pointer,
                "default" => CursorStyle::System,
                "none" => CursorStyle::Hidden,
                _ => CursorStyle::Auto,
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
        // Ikat 私有属性（CSS 用 ::selection 伪元素，围栏无伪元素选择器，故平铺 prop）。
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
        // Ikat 私有属性（CSS 用 ::placeholder 伪元素，围栏无伪元素选择器，故平铺 prop）。
        // None = render/layout 回退到 color 折半（对齐浏览器 ::placeholder UA 默认）。
        // 不可解析色静默落 None（与 selection-color 同口径，不报错）。
        "placeholder-color" => {
            style.placeholder_color = parse_color(value);
            true
        }
        // CSS -webkit-text-security：password 类输入的掩码显示（disc/circle/square）。
        // `none`（CSS 初始值）与不可识别值 → None（不掩码）。
        "-webkit-text-security" => {
            style.text_security = match value.trim() {
                "disc" => Some(TextSecurity::Disc),
                "circle" => Some(TextSecurity::Circle),
                "square" => Some(TextSecurity::Square),
                _ => None,
            };
            true
        }
        "font-size" => {
            // 延迟长度（视口单位/env()）进平行槽，solve 期按 root/safe 解析后再继承传播
            //（px 字段此刻是占位值）；px 声明清槽——级联后者胜出。
            if let Some(d) = try_deferred(value) {
                style.viewport.font_size = Some(d);
            } else if let Some(v) = parse_px(value) {
                style.font_size = v;
                style.viewport.font_size = None;
            }
            true
        }
        "font-family" => {
            style.font_family = first_font_family(value);
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
            // CSS 三形：`1.5`（倍数，继承为倍数）/ `27px`（绝对值，继承为 px）/
            // `normal`（= 0 哨兵）。倍数形进 line_height 槽、px 形进 line_height_px 槽，
            // 消费点统一走 effective_line_height()。此前实现剥 px 后裸 parse——
            // `27px` 被当 27 倍 → 单行 17×27=459px（#65 高度爆炸根因）。
            let v = value.trim();
            if let Some(px) = v.strip_suffix("px").map(str::trim) {
                match px.parse::<f32>() {
                    Ok(n) if n.is_finite() && n >= 0.0 => {
                        style.line_height = 0.0; // 后声明完胜：px 形清倍数槽，不留 stale
                        style.line_height_px = Some(n);
                        true
                    }
                    _ => false,
                }
            } else if v == "normal" {
                style.line_height = 0.0;
                style.line_height_px = None;
                true
            } else {
                match v.parse::<f32>() {
                    Ok(n) if n.is_finite() && n >= 0.0 => {
                        style.line_height = n;
                        style.line_height_px = None;
                        true
                    }
                    _ => false,
                }
            }
        }
        "letter-spacing" => {
            // 同 font-size：延迟长度进槽，px 落字段清槽。
            if let Some(d) = try_deferred(value) {
                style.viewport.letter_spacing = Some(d);
            } else {
                let Some(v) = parse_px(value) else {
                    return false;
                };
                style.letter_spacing = v;
                style.viewport.letter_spacing = None;
            }
            true
        }
        "white-space" => {
            // #73 换行控制全集：五值映射（围栏 Keyword 值集先行校验，这里是运行时
            // 动态规则/inline 的第二真相源）。未识别值返 false 拒收（不静默降级）。
            style.white_space = match value.trim() {
                "normal" => WhiteSpace::Normal,
                "nowrap" => WhiteSpace::Nowrap,
                "pre" => WhiteSpace::Pre,
                "pre-wrap" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                _ => return false,
            };
            true
        }
        "overflow-wrap" => {
            style.overflow_wrap = match value.trim() {
                "normal" => OverflowWrap::Normal,
                "break-word" => OverflowWrap::BreakWord,
                _ => return false,
            };
            true
        }
        "word-break" => {
            style.word_break = match value.trim() {
                "normal" => WordBreak::Normal,
                "break-all" => WordBreak::BreakAll,
                "keep-all" => WordBreak::KeepAll,
                _ => return false,
            };
            true
        }
        "text-wrap" => {
            // balance/stable/pretty 围栏拒绝（schema 值集）；此处同构拒收。
            style.text_wrap = match value.trim() {
                "normal" => TextWrap::Normal,
                "nowrap" => TextWrap::Nowrap,
                _ => return false,
            };
            true
        }
        "aspect-ratio" => {
            // CSS <ratio>: `auto` | <number> | <number>/<number>。taffy 原生消费
            // width/height 比值（Some 时约束缺省轴）。不可解析值返 false 让围栏报错，
            // 不静默降级（避免 `16/9` 被吞 → 节点缺高度不可见却无诊断）。
            let v = value.trim();
            if v.eq_ignore_ascii_case("auto") {
                ts.aspect_ratio = None;
                return true;
            }
            let ratio = match v.split_once('/') {
                Some((a, b)) => {
                    let (a, b): (f32, f32) = match (a.trim().parse(), b.trim().parse()) {
                        (Ok(a), Ok(b)) => (a, b),
                        _ => return false,
                    };
                    if b == 0.0 {
                        return false;
                    }
                    a / b
                }
                None => match v.parse::<f32>() {
                    Ok(n) => n,
                    Err(_) => return false,
                },
            };
            ts.aspect_ratio = Some(ratio);
            true
        }
        "order" => {
            // taffy Style 无 order 字段；存进 ResolvedStyle.order，
            // 由 layout 在 flex 排序前消费。非法值降级为 0。
            style.order = value.trim().parse::<i32>().unwrap_or(0);
            true
        }
        "z-index" => {
            // 层叠序：绘制/命中分层（scene::stacking::paint_order 消费），
            // 不进 layout。与 order 正交：order 管 flex 排列，z-index 管盖上关系。
            // 非法值降级 0（fence 打包期已拦；运行时 StyleSheet 逃生舱宽松，同 order 策略）。
            // z_declared 同步置位：CSS 画序里「声明的 z」创建 stacking context
            // （flex item 上 z-index 即使 static 也生效）——stacking::classify 消费。
            style.z_index = value.trim().parse::<i32>().unwrap_or(0);
            style.z_declared = true;
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
        "transform-origin" => match parse_transform_origin(value) {
            Some(o) => {
                style.transform_origin = o;
                true
            }
            None => false,
        },
        "position" => {
            // absolute 围栏内（脱离流）；relative 显式；fixed/sticky 围栏外静默忽略。
            // position_declared 与 ts.position 并行——布局层识别「声明了 relative」需要
            // 它（taffy 的 Relative 是默认值，分不清声明与否）。
            match value.trim() {
                "absolute" => {
                    ts.position = taffy::style::Position::Absolute;
                    style.position_declared = crate::style::resolved::PositionDeclared::Absolute;
                    true
                }
                "relative" => {
                    ts.position = taffy::style::Position::Relative;
                    style.position_declared = crate::style::resolved::PositionDeclared::Relative;
                    true
                }
                "static" => {
                    // 显式回退初始值：taffy 侧 Relative 就是 in-flow（其默认），declared 归 Static。
                    ts.position = taffy::style::Position::Relative;
                    style.position_declared = crate::style::resolved::PositionDeclared::Static;
                    true
                }
                _ => false, // fixed/sticky/其他 → 围栏外
            }
        }
        "inset" => {
            // inset 简写（#110）：四边同域 longhand 的 1~4 值展开（与 margin 同值域
            // px/%/auto/延迟长度、同 [t,r,b,l] 展开序），逐边走 top/right/bottom/left。
            let parts: Vec<&str> = value.split_whitespace().collect();
            if parts.is_empty() || parts.len() > 4 {
                return false;
            }
            let sides: [&str; 4] = match parts.len() {
                1 => [parts[0]; 4],
                2 => [parts[0], parts[1], parts[0], parts[1]],
                3 => [parts[0], parts[1], parts[2], parts[1]],
                _ => [parts[0], parts[1], parts[2], parts[3]],
            };
            for (idx, side) in sides.iter().enumerate() {
                if !apply_inset_side(style, idx, side) {
                    return false;
                }
            }
            true
        }
        "top" | "right" | "bottom" | "left" => {
            // inset 四边。px 写 Length；% 按含块解析（绝对定位居中写法 top:50% 等的
            // 浏览器语义，百分比相对 containing block 尺寸，由 taffy solve 兑现）；
            // auto 保持默认（不写）。视口单位（bottom:2vh 贴画布底类写法）进平行槽。
            let idx = match prop.trim() {
                "top" => 0usize,
                "right" => 1,
                "bottom" => 2,
                "left" => 3,
                _ => unreachable!(),
            };
            apply_inset_side(style, idx, value)
        }
        "box-shadow" => {
            // 括号感知 tokenizer：多层 / inset / blur / spread / spaced rgba()。
            // 非法输入返 false（fence 委托 _=>{} 走 apply_decl，自动报 FenceBadCssValue）。
            match parse_box_shadow(value) {
                Some(list) if !list.is_empty() => {
                    style.box_shadow = list;
                    true
                }
                Some(_) => {
                    // "none" / 空 → 清空（合法，表示无阴影；覆盖任何先前声明）。
                    style.box_shadow = Vec::new();
                    true
                }
                None => false,
            }
        }
        "transition" => {
            style.transition = parse_transition(value);
            true
        }
        "animation" => {
            // class 规则运行时 rematch 走此 arm：动态规则的
            // animation 声明叠加进 computed style.animation，sync_animation_players
            // 据此启停 player。打包期 inline 走 fence 的 validate + parse（同一 parse_animation）。
            style.animation = parse_animation(value);
            true
        }
        "animation-name"
        | "animation-duration"
        | "animation-timing-function"
        | "animation-delay"
        | "animation-iteration-count"
        | "animation-direction"
        | "animation-fill-mode"
        | "animation-play-state" => apply_animation_longhand(style, prop, value.trim()),
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
            // 与 background: <gradient> + color:transparent（推荐）组合触发
            // per-glyph 渐变采样（build_text_mesh 内 gradient_glyph_colors）。
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
            // Ikat 私有 CSS：font-effect: glow(w color), blur(w)（逗号分隔多 effect）。
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
        "resize" => {
            // 围栏 noop（schema 注册表声明 accepted-as-noop）：textarea 拖拽手柄是
            // 浏览器 UI 概念，游戏 UI 无此交互——接受声明避免 prop 名报错，不消费。
            // 值域由 fence Keyword 门校验（none/both/horizontal/vertical）。
            true
        }
        _ => false, // 装饰属性静默忽略
    }
}

/// 解析 CSS `animation` 简写值 → AnimationSpec 列表（逗号分隔多声明展开为多条）。
///
/// 与 `parse_transition` 同构：core 是解析真相源（运行时 rematch 的 apply_decl "animation"
/// arm 调用），fence 打包期 inline 路径委托本函数（fence `parse_animation_value`），
/// 防两份解析器漂移（对齐表唯一真相源 = `css_ease_keyword`）。
///
/// 语义：首个 time=duration、次个 time=delay；ease 关键字按对齐表映射
/// （`ease`→CubicOut，`ease-in/out/in-out`→Quad*，`step-start/end`→Step）；缺省值 =
/// CSS initial（direction=normal / fill=none / play-state=running / iteration-count=1 /
/// timing=ease）。非法段（空 / `none` / 非法 name / 缺 duration）静默丢弃（filter_map）。
pub fn parse_animation(value: &str) -> Vec<crate::style::resolved::AnimationSpec> {
    use crate::style::resolved::AnimationSpec;
    split_top_level_commas(value)
        .into_iter()
        .filter_map(|decl| parse_one_animation(decl.trim()))
        .collect::<Vec<AnimationSpec>>()
}

/// animation-* 长划（单值子集）：写入 `style.animation` 全部既有 spec 的对应字段；
/// 无既有 spec 时创建一条 initial spec。空 name 的 spec 不被 sync_animation_players
/// 建成 player（见该函数 name 守卫）——长划先于 animation-name 时声明惰性，name
/// 到位才启播（CSS「无 name 不播」）。`animation-name: none` 清空列表。
/// 值非法返 false（fence inline 路径报 FenceBadCssValue）。逗号列表不收（简写专属）。
fn apply_animation_longhand(
    style: &mut crate::style::resolved::ResolvedStyle,
    prop: &str,
    value: &str,
) -> bool {
    use crate::style::resolved::{
        AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
    };
    if value.is_empty() || value.contains(',') {
        return false;
    }
    if prop == "animation-name" && value.eq_ignore_ascii_case("none") {
        style.animation.clear();
        return true;
    }
    if style.animation.is_empty() {
        style.animation.push(AnimationSpec {
            name: String::new(),
            duration: 0.0,
            delay: 0.0,
            iteration_count: Some(1),
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
            timing_function: crate::tween::Ease::CubicOut,
            play_state: AnimationPlayState::Running,
        });
    }
    let mut ok = true;
    for spec in &mut style.animation {
        match prop {
            "animation-name" => {
                if is_valid_animation_name(value) {
                    spec.name = value.to_string();
                } else {
                    ok = false;
                }
            }
            "animation-duration" | "animation-delay" => match parse_time_seconds(value) {
                Some(secs) => {
                    if prop == "animation-duration" {
                        spec.duration = secs;
                    } else {
                        spec.delay = secs;
                    }
                }
                None => ok = false,
            },
            "animation-timing-function" => match parse_ease(value) {
                Some(e) => spec.timing_function = e,
                None => ok = false,
            },
            "animation-iteration-count" => {
                if value.eq_ignore_ascii_case("infinite") {
                    spec.iteration_count = None;
                } else if value.chars().all(|c| c.is_ascii_digit()) && !value.is_empty() {
                    match value.parse::<u32>() {
                        Ok(n) => spec.iteration_count = Some(n),
                        Err(_) => ok = false,
                    }
                } else {
                    ok = false;
                }
            }
            "animation-direction" => match value.to_ascii_lowercase().as_str() {
                "normal" => spec.direction = AnimationDirection::Normal,
                "reverse" => spec.direction = AnimationDirection::Reverse,
                "alternate" => spec.direction = AnimationDirection::Alternate,
                "alternate-reverse" => spec.direction = AnimationDirection::AlternateReverse,
                _ => ok = false,
            },
            "animation-fill-mode" => match value.to_ascii_lowercase().as_str() {
                "none" => spec.fill_mode = AnimationFillMode::None,
                "forwards" => spec.fill_mode = AnimationFillMode::Forwards,
                "backwards" => spec.fill_mode = AnimationFillMode::Backwards,
                "both" => spec.fill_mode = AnimationFillMode::Both,
                _ => ok = false,
            },
            "animation-play-state" => match value.to_ascii_lowercase().as_str() {
                "running" => spec.play_state = AnimationPlayState::Running,
                "paused" => spec.play_state = AnimationPlayState::Paused,
                _ => ok = false,
            },
            _ => unreachable!("arm 匹配已限定 8 个长划 prop"),
        }
    }
    ok
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
        // CSS 缺省 timing = ease（精确 bezier；#9 前用 CubicOut 近似）
        timing_function: crate::tween::Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        },
        play_state: AnimationPlayState::Running,
    };
    let mut time_count = 0;
    for tok in tokens {
        if let Some(secs) = parse_time_seconds(tok) {
            // 首个 time = duration，次个 time = delay。
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
        } else if let Some(e) = parse_ease(tok) {
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
/// 防 ease 对齐表漂移）。
pub fn parse_transition(value: &str) -> Vec<crate::style::resolved::TransitionSpec> {
    split_top_level_commas(value)
        .into_iter()
        .filter_map(parse_one_transition)
        .collect()
}

/// 解析单个 transition spec（逗号分隔的一段）。
/// 空格切 token：prop 关键字（all/opacity/color/background-color/transform）→ TweenProp 映射；
/// time（`<n>s`/`<n>ms`）首遇 = duration、次遇 = delay；其余 = ease 关键字（对齐表）。
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
    // CSS 缺省 timing-function = ease（精确 bezier；#9 前用 CubicOut 近似）
    let mut ease = Ease::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    let mut time_count = 0;
    for t in tokens {
        match t {
            "all" => prop = None,
            "opacity" => prop = Some(TweenProp::Opacity),
            "color" => prop = Some(TweenProp::TextColor),
            "background-color" => prop = Some(TweenProp::BgColor),
            "transform" => prop = Some(TweenProp::Transform),
            // #10 layout/box-shadow 通道
            "width" => prop = Some(TweenProp::Width),
            "height" => prop = Some(TweenProp::Height),
            "flex-grow" => prop = Some(TweenProp::FlexGrow),
            "box-shadow" => prop = Some(TweenProp::BoxShadow),
            _ => {
                if let Some(secs) = parse_time_seconds(t) {
                    // 首遇 = duration，次遇 = delay（CSS 语义；time_count 防 0s duration 被吞）
                    if time_count == 0 {
                        duration = secs;
                    } else {
                        delay = secs;
                    }
                    time_count += 1;
                } else if let Some(e) = parse_ease(t) {
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

/// CSS timing-function 关键字 → Ease（唯一真相源）。
///
/// `pub` 供 fence 委托（transition 侧经 `parse_transition`、animation 侧直接调用），
/// 打包期与运行时共用一张表，防双份白名单漂移。本表按小写精确匹配；fence animation
/// 侧 validate 门大小写不敏感，查表前自行 lowercase（见 fence css.rs `ease_from_keyword`）。
pub fn css_ease_keyword(kw: &str) -> Option<crate::tween::Ease> {
    use crate::tween::{Ease, EASE_BEZIER, EASE_IN_BEZIER, EASE_IN_OUT_BEZIER, EASE_OUT_BEZIER};
    let b = |p: [f32; 4]| Ease::CubicBezier {
        x1: p[0],
        y1: p[1],
        x2: p[2],
        y2: p[3],
    };
    Some(match kw {
        // CSS 标准关键字（精确 bezier，CSS Easing Functions L1 定义值）
        "linear" => Ease::Linear,
        "ease" => b(EASE_BEZIER),
        "ease-in" => b(EASE_IN_BEZIER),
        "ease-out" => b(EASE_OUT_BEZIER),
        "ease-in-out" => b(EASE_IN_OUT_BEZIER),
        "step-start" => Ease::Step { start: true },
        "step-end" => Ease::Step { start: false },
        // ikat 超集 keyword（非标，游戏 UI 刚需；fence.md 登记）。命名照 CSS keyword 惯例
        // （ease-in-back），GSAP/Unity 先验同族。
        "ease-in-back" => Ease::BackIn,
        "ease-out-back" => Ease::BackOut,
        "ease-in-out-back" => Ease::BackInOut,
        "ease-in-elastic" => Ease::ElasticIn,
        "ease-out-elastic" => Ease::ElasticOut,
        "ease-in-out-elastic" => Ease::ElasticInOut,
        "ease-in-bounce" => Ease::BounceIn,
        "ease-out-bounce" => Ease::BounceOut,
        "ease-in-out-bounce" => Ease::BounceInOut,
        // 幂函数族：CSS 无对应 keyword（历史内部值，transition/ScrollTo 等运行时 API 消费）
        "quad-in" => Ease::QuadIn,
        "quad-out" => Ease::QuadOut,
        "quad-in-out" => Ease::QuadInOut,
        "cubic-in" => Ease::CubicIn,
        "cubic-out" => Ease::CubicOut,
        "cubic-in-out" => Ease::CubicInOut,
        _ => return None,
    })
}

/// 解析 timing-function 值全形：`cubic-bezier(x1,y1,x2,y2)` 函数形 + keyword 全集
/// （`css_ease_keyword`）。x1/x2 须 ∈[0,1]（CSS 有效性；y 不限）。非法 → None。
/// 消费方：`animation`/`animation-timing-function` 解析 + keyframes stop 内 timing。
pub fn parse_ease(value: &str) -> Option<crate::tween::Ease> {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix("cubic-bezier(") {
        let inner = rest.strip_suffix(')')?;
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() != 4 {
            return None;
        }
        let n: Vec<f32> = parts
            .iter()
            .map(|p| p.parse::<f32>().ok())
            .collect::<Option<_>>()?;
        if !(0.0..=1.0).contains(&n[0]) || !(0.0..=1.0).contains(&n[2]) {
            return None;
        }
        return Some(crate::tween::Ease::CubicBezier {
            x1: n[0],
            y1: n[1],
            x2: n[2],
            y2: n[3],
        });
    }
    css_ease_keyword(v)
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

/// CSS `box-shadow`：括号深度 0 按逗号切多层，每层走 [`parse_one_box_shadow`]。
/// `none` / 空 → 空 Vec（合法，表示无阴影）；任一层非法 → None（apply_decl 据此返 false，
/// fence 委托链自动报 FenceBadCssValue）。括号深度计数保证 `rgba(r,g,b,a)` 内部逗号不分层。
/// 层数硬限（render 合成 node_id high-byte 编码区大小）：inset ≤ [`MAX_INSET_SHADOW_LAYERS`]、
/// outer ≤ [`MAX_OUTER_SHADOW_LAYERS`]，任一超限 → None（超限层 id 会撞相邻编码区，静默
/// 错渲染，宁可整条拒收）。
pub fn parse_box_shadow(value: &str) -> Option<Vec<BoxShadow>> {
    // CSS keywords are case-insensitive (matches how `inset` is matched below).
    if value.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut layers: Vec<String> = vec![String::new()];
    let mut depth = 0;
    for ch in value.chars() {
        match ch {
            '(' => {
                depth += 1;
                layers.last_mut().unwrap().push(ch);
            }
            ')' => {
                depth -= 1;
                layers.last_mut().unwrap().push(ch);
            }
            ',' if depth == 0 => layers.push(String::new()),
            _ => layers.last_mut().unwrap().push(ch),
        }
    }
    let mut out = Vec::new();
    for layer in &layers {
        // trim() 使纯空白层（如尾随逗号）→ ""，parse_one_box_shadow 返 None → 整体非法。
        let bs = parse_one_box_shadow(layer.trim())?;
        out.push(bs);
    }
    if out.iter().filter(|s| s.inset).count() > MAX_INSET_SHADOW_LAYERS
        || out.iter().filter(|s| !s.inset).count() > MAX_OUTER_SHADOW_LAYERS
    {
        return None;
    }
    Some(out)
}

/// 解析单层 box-shadow。tokenize 时括号内不切空白（保护 `rgba(95, 180, 212, 0.5)`
/// 不被拆成多 token）。token 顺序自由：`inset`（前/后置均可）、≥2 个数值（ox oy [blur] [spread]）、
/// 可选 color。color 省略 → 默认半透明黑（CSS currentColor 围栏不追）。
/// 非 inset / 非 color / 非数值 → None；ox/oy 缺省 → None（CSS 强制 ≥2 数值）。
/// blur < 0 不合法（CSS spec），clamp 到 0 防运行时 σ 为负。
fn parse_one_box_shadow(s: &str) -> Option<BoxShadow> {
    let mut tokens: Vec<String> = vec![String::new()];
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                tokens.last_mut().unwrap().push(ch);
            }
            ')' => {
                depth -= 1;
                tokens.last_mut().unwrap().push(ch);
            }
            c if c.is_whitespace() && depth == 0 => tokens.push(String::new()),
            _ => tokens.last_mut().unwrap().push(ch),
        }
    }
    let tokens: Vec<&str> = tokens
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let mut inset = false;
    let mut nums: Vec<f32> = Vec::new();
    let mut color: Option<[f32; 4]> = None;
    for t in &tokens {
        if t.eq_ignore_ascii_case("inset") {
            inset = true;
            continue;
        }
        if let Some(c) = parse_color(t) {
            color = Some(c);
            continue;
        }
        // 数值（剥 "px"；box-shadow 长度单位固定 px，围栏外单位交 fence 报错）。
        match t.trim_end_matches("px").parse::<f32>() {
            Ok(v) => nums.push(v),
            Err(_) => return None, // 非 inset/color/数值 = 非法 token
        }
    }
    if nums.len() < 2 {
        return None; // CSS 强制 ox oy
    }
    let ox = nums[0];
    let oy = nums[1];
    let blur = *nums.get(2).unwrap_or(&0.0);
    let spread = *nums.get(3).unwrap_or(&0.0);
    let color = color.unwrap_or([0.0, 0.0, 0.0, 0.3]);
    Some(BoxShadow {
        ox,
        oy,
        spread,
        blur: blur.max(0.0),
        color,
        inset,
    })
}

/// 解析单个 Ikat 私有 font-effect：`glow(w color)` / `blur(w)`。
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

/// font-family 逗号列表取首个 family 名（MVP 每节点单字体）：剥引号、trim。
/// 整串存（如 `"JetBrainsMono",monospace`）会让 FontTable 精确匹配必失配 → 回落默认
/// 字体，等宽/像素字体 specimen 全部失效。泛型关键字（monospace/serif）无注册映射，
/// 不命中同样回落默认（围栏内 CSS 由 workspace 注册字体表决定可用字体）。
fn first_font_family(value: &str) -> Option<String> {
    let first = value.split(',').next()?.trim();
    let unquoted = first.trim_matches(|c| c == '"' || c == '\'');
    if unquoted.is_empty() {
        None
    } else {
        Some(unquoted.to_string())
    }
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
