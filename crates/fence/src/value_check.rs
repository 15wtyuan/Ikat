//! 打包期 CSS 值域门（inline style 与 `<style>` 规则共用单一校验入口）。
//!
//! 原则：围栏外输入打包期报错，不静默降级。运行时 `apply_decl` 对部分通道解析失败
//! 仍返回 true（宽松吞值）：颜色 / overflow / filter / transform。浏览器先验里合法、
//! 运行时恒无效的声明（命名色 `red`、`overflow: clip`、`filter: blur()`、
//! `transform: skew()`）静默吞掉即「上线即坏」——此处统一提前到打包期报
//! FenceBadCssValue，并给出围栏内写法。
//!
//! 函数白名单与 core `style::mapping` 同步（parse_filter / func_to_matrix）；漂移由
//! fence 测试（doc_schema_sync 同目录）锁：改 core 白名单必须同步本表。

use crate::schema::css::{find_css_prop, CssValueParser};

/// `filter` 运行时实现的函数集（core mapping::parse_filter）。
const FILTER_FNS: &[&str] = &[
    "grayscale",
    "brightness",
    "contrast",
    "saturate",
    "hue-rotate",
    "invert",
    "sepia",
];

/// `transform` 运行时实现的函数集（core mapping::func_to_matrix）。
const TRANSFORM_FNS: &[&str] = &[
    "translate",
    "translateX",
    "translateY",
    "rotate",
    "scale",
    "scaleX",
    "scaleY",
];

/// 值域校验：返回 Some(错误消息) = 打包期报 FenceBadCssValue；None = 放行。
/// prop 未注册返回 None（unknown-prop 门由调用方负责）。覆盖宽松吞值通道
/// （Color/Overflow/Filter/Transform）+ BoxShadow 层数硬限（超过合成 node_id
/// 编码容量的层静默错渲染，委托 core parser 拒收）；Keyword 域走 [`keyword_error`]。
pub fn value_error(prop: &str, value: &str) -> Option<String> {
    // shorthand 域映射：overflow 简写（Replicate 到 -x/-y）值域与 longhand 同集。
    let parser = find_css_prop(prop).map(|s| &s.parser).or(match prop {
        "overflow" => Some(&CssValueParser::Overflow),
        _ => None,
    })?;
    let value = value.trim();
    // Raw parser 的定向值域门（无 parser 枚举污染）：transform-origin / per-stop timing。
    // core 解析器是唯一真相源，这里只借它判定合法性（防校验/解析两张表漂移）。
    match prop {
        "transform-origin" => {
            if loomgui_core::style::mapping::parse_transform_origin(value).is_some() {
                return None;
            }
            return Some(format!(
                "value \"{value}\" is not valid for CSS property \"transform-origin\"                  (expected: <length|%> × 2, or left|center|right / top|center|bottom keywords)"
            ));
        }
        "animation-timing-function" => {
            if loomgui_core::style::mapping::parse_ease(value).is_some() {
                return None;
            }
            return Some(format!(
                "value \"{value}\" is not valid for CSS property \"animation-timing-function\"                  (see fence.md 缓动函数全集: CSS keyword + cubic-bezier(x1,y1,x2,y2)                  + loom superset ease-in/out/in-out-back/elastic/bounce)"
            ));
        }
        _ => {}
    }
    match parser {
        CssValueParser::Color => {
            // core parse_color 认 #hex / rgb() / rgba()（含全透明 rgba(0,0,0,0)，显式
            // 清色可用）。命名色全通道恒无效；`transparent` 关键字仅 `color` 有 core
            // 拦截，其余颜色属性写它 = 静默不覆盖。
            let lowered = value.to_ascii_lowercase();
            let ok = loomgui_core::style::mapping::parse_color(value).is_some()
                || (prop == "color" && lowered == "transparent");
            if ok {
                return None;
            }
            let mut msg = format!(
                "value \"{value}\" is not a valid color for \"{prop}\" — \
                 use #rgb / #rrggbb / #rrggbbaa or rgb() / rgba()"
            );
            msg.push_str(if prop == "color" {
                "; named colors are outside the fence color subset"
            } else {
                "; named colors and `transparent` are outside the fence color subset \
                 (explicit clear: rgba(0,0,0,0) or #00000000)"
            });
            Some(msg)
        }
        CssValueParser::BoxShadow => {
            // 委托 core parse_box_shadow（与运行时同一真相源）：任一层语法非法、或层数
            // 超过合成 node_id 编码硬限（inset ≤ 8 / outer ≤ 4，超限层 id 撞相邻编码区
            // → 静默错渲染）都返 None——打包期报清，不静默降级。
            if loomgui_core::style::mapping::parse_box_shadow(value).is_some() {
                return None;
            }
            Some(format!(
                "value \"{value}\" is not a valid box-shadow for \"{prop}\" \
                 (layer limits: at most 8 inset and 4 outer layers; \
                 layers beyond the limits render incorrectly and are rejected)"
            ))
        }
        CssValueParser::NumberOrLength => {
            // line-height 三形：<number>（倍数）| <number>px（绝对）| normal。
            // em/% 等其余 CSS 形围栏外——此前值域不校验，`27px` 过 check 后被 core
            // mapping 剥 px 当 27 倍 → 单行高度 ×27（#65 高度爆炸）。
            let v = value.trim();
            let ok = v == "normal"
                || v.parse::<f32>().is_ok_and(|n| n.is_finite() && n >= 0.0)
                || v.strip_suffix("px").is_some_and(|p| {
                    p.trim()
                        .parse::<f32>()
                        .is_ok_and(|n| n.is_finite() && n >= 0.0)
                });
            (!ok).then(|| {
                format!(
                    "value \"{value}\" is not valid for CSS property \"line-height\" \
                     (allowed: a unitless multiplier like 1.6, a px length like 27px, or normal; \
                     em/% and other forms are outside the fence)"
                )
            })
        }
        CssValueParser::Overflow => {
            // 合法值与 core parse_overflow 同集。`clip` 等浏览器值运行时静默忽略
            // （等同 visible，无裁剪）——按值域外报错。
            let bad = value
                .split_whitespace()
                .find(|t| !matches!(*t, "visible" | "hidden" | "scroll" | "auto"));
            bad.map(|t| {
                format!(
                    "value \"{t}\" is not valid for CSS property \"{prop}\" \
                     (allowed: visible | hidden | scroll | auto)"
                )
            })
        }
        CssValueParser::Filter => {
            if value.is_empty() || value.eq_ignore_ascii_case("none") {
                return None;
            }
            let bad = value.split_whitespace().find(|tok| {
                let (name, _) = tok.split_once('(').unwrap_or((tok, ""));
                !FILTER_FNS.contains(&name.trim())
            });
            bad.map(|tok| {
                format!(
                    "value \"{tok}\" is not valid for CSS property \"{prop}\" \
                     (supported functions: {})",
                    FILTER_FNS.join(", ")
                )
            })
        }
        CssValueParser::Transform => {
            if value.is_empty() || value.eq_ignore_ascii_case("none") {
                return None;
            }
            // 括号感知分词：translate(10px, 5px) 参数含空格，不能 split_whitespace。
            let bad = split_transform_fns(value)
                .into_iter()
                .find(|name| !TRANSFORM_FNS.contains(&name.as_str()));
            bad.map(|name| {
                format!(
                    "value \"{name}(...)\" is not valid for CSS property \"{prop}\" \
                     (supported functions: {})",
                    TRANSFORM_FNS.join(", ")
                )
            })
        }
        _ => None,
    }
}

/// Keyword 值域（`<style>` 规则路径此前不校验）。`display: inline` 豁免硬错——
/// 它是围栏有意收进的关键字（运行时按 flex 处理），由 [`display_inline_warning`]
/// 出语义警告，不拦打包。
pub fn keyword_error(prop: &str, value: &str) -> Option<String> {
    let spec = find_css_prop(prop)?;
    if let CssValueParser::Keyword(allowed) = &spec.parser {
        let value = value.trim();
        if allowed.contains(&value) {
            return None;
        }
        if prop == "display" && value == "inline" {
            return None;
        }
        return Some(format!(
            "value \"{}\" is not valid for CSS property \"{}\" (allowed: {})",
            value,
            prop,
            allowed.join(" | ")
        ));
    }
    None
}

/// `display: inline` 语义警告文案（None = 不适用）。围栏没有 inline flow——inline
/// 运行时映射为 flex 容器（与浏览器 inline 收缩宽语义不同），显式声明多半是
/// 误用先验。
pub fn display_inline_warning(value: &str) -> Option<&'static str> {
    if value.trim() == "inline" {
        Some(
            "display:inline has no inline-flow layout in LoomGUI — \
             the element is laid out as a flex container (children become flex items). \
             Use display:flex explicitly, or display:none to hide.",
        )
    } else {
        None
    }
}

/// `transition` 引擎实际驱动的属性集（core emit_transition_requests 检测
/// BgColor/TextColor/Opacity/Transform 四通道——transform 走整矩阵 TRS 分解插值；
/// 布局属性走每帧离散 solve 无过渡）。
const TRANSITION_PROPS: &[&str] = &["background-color", "color", "opacity", "transform"];

/// transition 声明的属性域外警告（每条越界 spec 一条）。`all` 单独提示——它对
/// 支持通道有效，但其余属性（width/transform 等）静默 snap，浏览器先验会翻车。
pub fn transition_warnings(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for seg in value.split(',') {
        let seg = seg.trim();
        let Some(prop) = seg.split_whitespace().next() else {
            continue;
        };
        if prop.is_empty() || prop == "none" {
            continue;
        }
        if prop == "all" {
            out.push(
                "transition \"all\": only background-color / color / opacity / transform are \
                 transitioned in LoomGUI — all other properties (width, transform, \
                 filter, ...) change instantly"
                    .into(),
            );
        } else if !TRANSITION_PROPS.contains(&prop) {
            out.push(format!(
                "transition property \"{prop}\" has no runtime transition — \
                 only background-color / color / opacity / transform are animated; \
                 this property changes instantly"
            ));
        }
    }
    out
}

/// transform 值的括号感知函数名分词（含空格参数安全）。
fn split_transform_fns(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut name = String::new();
    for ch in value.chars() {
        match ch {
            '(' => {
                depth += 1;
                if depth == 1 {
                    out.push(name.trim().to_string());
                    name.clear();
                }
            }
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => name.push(ch),
            _ => {}
        }
    }
    out.into_iter().filter(|n| !n.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_named_and_transparent_rejected_outside_color() {
        assert!(value_error("background-color", "red").is_some());
        assert!(value_error("border-color", "blue").is_some());
        assert!(value_error("background-color", "transparent").is_some());
        // rgba 全透明是合法显式清色（core parse_color 认）。
        assert!(value_error("background-color", "rgba(0, 0, 0, 0)").is_none());
        // color 通道的 transparent 有 core 拦截。
        assert!(value_error("color", "transparent").is_none());
        assert!(value_error("background-color", "#ff0000").is_none());
        assert!(value_error("background-color", "rgba(160, 58, 42, 0.25)").is_none());
    }

    #[test]
    fn line_height_number_px_normal_forms() {
        // #65：三形合法。此前 Number 域不校验——`27px` 过 check 后被 core mapping
        // 剥 px 当 27 倍（单行高度 ×27）。
        assert!(value_error("line-height", "1.6").is_none());
        assert!(value_error("line-height", "27px").is_none());
        assert!(value_error("line-height", "normal").is_none());
        // em / % / 负数 / 杂串围栏外
        assert!(value_error("line-height", "1.5em").is_some());
        assert!(value_error("line-height", "150%").is_some());
        assert!(value_error("line-height", "-2").is_some());
        assert!(value_error("line-height", "auto").is_some());
    }

    #[test]
    fn overflow_clip_and_typos_rejected() {
        assert!(value_error("overflow", "clip").is_some());
        assert!(value_error("overflow", "visibl").is_some());
        assert!(value_error("overflow", "hidden auto").is_none());
        assert!(value_error("overflow-x", "scroll").is_none());
    }

    #[test]
    fn filter_unsupported_fns_rejected() {
        assert!(value_error("filter", "blur(5px)").is_some());
        assert!(value_error("filter", "drop-shadow(2px 2px 4px black)").is_some());
        assert!(value_error("filter", "none").is_none());
        assert!(value_error("filter", "grayscale(1) brightness(0.8)").is_none());
    }

    // box-shadow：语法 + 层数硬限都委托 core parse_box_shadow（运行时同一真相源）。
    // 层数超合成 node_id 编码容量（inset > 8 / outer > 4）的层静默错渲染 → 打包期拒收。
    fn shadow_layers(n: usize, prefix: &str) -> String {
        std::iter::repeat_n(format!("{prefix} 1px 1px #000"), n)
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn box_shadow_layer_cap_rejected() {
        assert!(value_error("box-shadow", "none").is_none());
        assert!(value_error("box-shadow", "0 8px 26px rgba(95, 180, 212, 0.5)").is_none());
        assert!(value_error("box-shadow", &shadow_layers(8, "inset 0")).is_none());
        assert!(value_error("box-shadow", &shadow_layers(4, "0")).is_none());
        let err = value_error("box-shadow", &shadow_layers(9, "inset 0")).unwrap();
        assert!(
            err.contains("8 inset"),
            "error names the layer limits: {err}"
        );
        assert!(value_error("box-shadow", &shadow_layers(5, "0")).is_some());
        assert!(
            value_error("box-shadow", "10px").is_some(),
            "syntax error rejected"
        );
    }

    #[test]
    fn transform_unsupported_fns_rejected() {
        assert!(value_error("transform", "skew(10deg)").is_some());
        assert!(value_error("transform", "matrix(1,0,0,1,0,0)").is_some());
        assert!(value_error("transform", "none").is_none());
        // 空格参数 + 混合链。
        assert!(value_error("transform", "translate(10px, 5px) rotate(45deg) scale(2)").is_none());
        assert!(value_error("transform", "translateX(10px) skewX(5deg)").is_some());
    }

    #[test]
    fn keyword_domain_checked_with_inline_exemption() {
        assert!(keyword_error("flex-direction", "sideways").is_some());
        assert!(keyword_error("flex-direction", "column").is_none());
        assert!(
            keyword_error("display", "inline").is_none(),
            "inline 豁免硬错"
        );
        assert!(keyword_error("display", "inline-block").is_some());
        assert!(display_inline_warning("inline").is_some());
        assert!(display_inline_warning("flex").is_none());
    }
}
