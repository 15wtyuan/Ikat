// == CssPropSpec ==

/// Compile-time schema entry for one CSS property.
///
/// Three orthogonal dimensions model CSS in the fence:
/// - CssPropSpec (this struct): name, default, inheritance, parser.
/// - CssValueParser: how a raw string value is validated.
/// - ShorthandSpec: how a shorthand expands to longhand props.
#[derive(Debug)]
pub struct CssPropSpec {
    pub name: &'static str,
    pub default: &'static str,
    pub inherited: bool,
    pub parser: CssValueParser,
}

// == CssValueParser ==

/// Value parser tag identifying how a CSS value is parsed/validated.
#[derive(Debug, Clone, PartialEq)]
pub enum CssValueParser {
    Keyword(&'static [&'static str]),
    Length,
    LengthPercent,
    LengthPercentAuto,
    Color,
    Number,
    Integer,
    FourSidedPx,
    FourSidedMargin,
    BorderRadius,
    Transform,
    Overflow,
    Filter,
    BoxShadow,
    TextShadow,
    Transition,
    Animation,
    Gradient2,
    TextEffect,
    TextStroke,
    BackgroundClipText,
    Url,
    Raw,
}

// == ShorthandSpec ==

/// Compile-time schema entry for a CSS shorthand.
#[derive(Debug)]
pub struct ShorthandSpec {
    pub name: &'static str,
    pub expands_to: &'static [&'static str],
    pub kind: ShorthandKind,
}

/// How a shorthand expands to longhand properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShorthandKind {
    Box,
    Replicate,
    FallThrough,
    BorderShorthand,
    BackgroundShorthand,
}

// == CSS_PROPS registry ==

pub static CSS_PROPS: &[CssPropSpec] = &[
    CssPropSpec {
        name: "width",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "height",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "min-width",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "min-height",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "max-width",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "max-height",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "padding-top",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "padding-right",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "padding-bottom",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "padding-left",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "margin-top",
        default: "0",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "margin-right",
        default: "0",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "margin-bottom",
        default: "0",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "margin-left",
        default: "0",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "display",
        default: "block",
        inherited: false,
        parser: CssValueParser::Keyword(&["block", "flex", "none", "inline"]),
    },
    CssPropSpec {
        name: "flex-direction",
        default: "row",
        inherited: false,
        parser: CssValueParser::Keyword(&["row", "row-reverse", "column", "column-reverse"]),
    },
    CssPropSpec {
        name: "flex-wrap",
        default: "nowrap",
        inherited: false,
        parser: CssValueParser::Keyword(&["nowrap", "wrap", "wrap-reverse"]),
    },
    CssPropSpec {
        name: "justify-content",
        default: "flex-start",
        inherited: false,
        parser: CssValueParser::Keyword(&[
            "flex-start",
            "center",
            "flex-end",
            "space-between",
            "space-around",
            "space-evenly",
        ]),
    },
    CssPropSpec {
        name: "align-items",
        default: "stretch",
        inherited: false,
        parser: CssValueParser::Keyword(&[
            "flex-start",
            "center",
            "flex-end",
            "stretch",
            "baseline",
        ]),
    },
    CssPropSpec {
        name: "align-content",
        default: "stretch",
        inherited: false,
        parser: CssValueParser::Keyword(&[
            "flex-start",
            "center",
            "flex-end",
            "stretch",
            "space-between",
            "space-around",
            "space-evenly",
        ]),
    },
    CssPropSpec {
        name: "align-self",
        default: "auto",
        inherited: false,
        parser: CssValueParser::Keyword(&[
            "auto",
            "flex-start",
            "center",
            "flex-end",
            "stretch",
            "baseline",
        ]),
    },
    CssPropSpec {
        name: "flex-grow",
        default: "0",
        inherited: false,
        parser: CssValueParser::Number,
    },
    CssPropSpec {
        name: "flex-shrink",
        default: "1",
        inherited: false,
        parser: CssValueParser::Number,
    },
    CssPropSpec {
        name: "flex-basis",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "gap",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "row-gap",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "column-gap",
        default: "0",
        inherited: false,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "position",
        default: "relative",
        inherited: false,
        parser: CssValueParser::Keyword(&["absolute", "relative"]),
    },
    CssPropSpec {
        name: "top",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "right",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "bottom",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "left",
        default: "auto",
        inherited: false,
        parser: CssValueParser::LengthPercentAuto,
    },
    CssPropSpec {
        name: "aspect-ratio",
        default: "auto",
        inherited: false,
        parser: CssValueParser::Number,
    },
    CssPropSpec {
        name: "order",
        default: "0",
        inherited: false,
        parser: CssValueParser::Integer,
    },
    CssPropSpec {
        name: "border-color",
        default: "transparent",
        inherited: false,
        parser: CssValueParser::Color,
    },
    CssPropSpec {
        name: "border-style",
        default: "none",
        inherited: false,
        parser: CssValueParser::Keyword(&["none", "solid", "dashed", "dotted", "double"]),
    },
    CssPropSpec {
        name: "border-radius",
        default: "0",
        inherited: false,
        parser: CssValueParser::BorderRadius,
    },
    CssPropSpec {
        name: "border-image-slice",
        default: "none",
        inherited: false,
        parser: CssValueParser::FourSidedPx,
    },
    CssPropSpec {
        name: "background-color",
        default: "transparent",
        inherited: false,
        parser: CssValueParser::Color,
    },
    CssPropSpec {
        name: "background-image",
        default: "none",
        inherited: false,
        parser: CssValueParser::Url,
    },
    CssPropSpec {
        name: "background-size",
        default: "stretch",
        inherited: false,
        parser: CssValueParser::Keyword(&["cover", "contain", "100%", "stretch"]),
    },
    CssPropSpec {
        name: "background-clip",
        default: "border-box",
        inherited: false,
        parser: CssValueParser::Keyword(&["border-box", "padding-box", "content-box", "text"]),
    },
    CssPropSpec {
        name: "-webkit-background-clip",
        default: "border-box",
        inherited: false,
        parser: CssValueParser::Keyword(&["border-box", "padding-box", "content-box", "text"]),
    },
    CssPropSpec {
        name: "opacity",
        default: "1",
        inherited: false,
        parser: CssValueParser::Number,
    },
    CssPropSpec {
        name: "overflow-x",
        default: "visible",
        inherited: false,
        parser: CssValueParser::Overflow,
    },
    CssPropSpec {
        name: "overflow-y",
        default: "visible",
        inherited: false,
        parser: CssValueParser::Overflow,
    },
    CssPropSpec {
        name: "color",
        default: "#000000",
        inherited: true,
        parser: CssValueParser::Color,
    },
    CssPropSpec {
        name: "box-shadow",
        default: "none",
        inherited: false,
        parser: CssValueParser::BoxShadow,
    },
    CssPropSpec {
        name: "pointer-events",
        default: "auto",
        inherited: false,
        parser: CssValueParser::Keyword(&["auto", "none"]),
    },
    // resize: 标准浏览器禁 textarea 拖拽手柄。core 不消费（noop），fence 接受避免报 prop 名错。
    CssPropSpec {
        name: "resize",
        default: "none",
        inherited: false,
        parser: CssValueParser::Keyword(&["none", "both", "horizontal", "vertical"]),
    },
    CssPropSpec {
        name: "transform",
        default: "none",
        inherited: false,
        parser: CssValueParser::Transform,
    },
    CssPropSpec {
        name: "filter",
        default: "none",
        inherited: false,
        parser: CssValueParser::Filter,
    },
    CssPropSpec {
        name: "font-size",
        default: "16px",
        inherited: true,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "font-family",
        default: "inherit",
        inherited: true,
        parser: CssValueParser::Raw,
    },
    CssPropSpec {
        name: "font-weight",
        default: "400",
        inherited: true,
        parser: CssValueParser::Integer,
    },
    CssPropSpec {
        name: "text-align",
        default: "left",
        inherited: true,
        parser: CssValueParser::Keyword(&["left", "center", "right"]),
    },
    CssPropSpec {
        name: "line-height",
        default: "0",
        inherited: true,
        parser: CssValueParser::Number,
    },
    CssPropSpec {
        name: "letter-spacing",
        default: "0",
        inherited: true,
        parser: CssValueParser::Length,
    },
    CssPropSpec {
        name: "white-space",
        default: "normal",
        inherited: true,
        parser: CssValueParser::Keyword(&["normal", "nowrap"]),
    },
    CssPropSpec {
        name: "text-shadow",
        default: "none",
        inherited: true,
        parser: CssValueParser::TextShadow,
    },
    CssPropSpec {
        name: "-webkit-text-stroke",
        default: "0 transparent",
        inherited: true,
        parser: CssValueParser::TextStroke,
    },
    CssPropSpec {
        name: "font-effect",
        default: "none",
        inherited: true,
        parser: CssValueParser::TextEffect,
    },
    CssPropSpec {
        name: "transition",
        default: "none",
        inherited: false,
        parser: CssValueParser::Transition,
    },
    // animation: name duration [easing] [iteration-count|infinite] [fill-mode] [direction] [play-state] [delay].
    // 对齐 public-api.md「动画定义全在 CSS」终态契约。runtime 驱动（@keyframes 表 + tween
    // 发射）在 §4 视觉束实现（v1.10）；本轮 fence 接受语法 + 静默忽略（不报错、不跑动画）。
    CssPropSpec {
        name: "animation",
        default: "none",
        inherited: false,
        parser: CssValueParser::Animation,
    },
];

pub static CSS_SHORTHANDS: &[ShorthandSpec] = &[
    ShorthandSpec {
        name: "padding",
        expands_to: &[
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        kind: ShorthandKind::Box,
    },
    ShorthandSpec {
        name: "margin",
        expands_to: &["margin-top", "margin-right", "margin-bottom", "margin-left"],
        kind: ShorthandKind::Box,
    },
    ShorthandSpec {
        name: "overflow",
        expands_to: &["overflow-x", "overflow-y"],
        kind: ShorthandKind::Replicate,
    },
    ShorthandSpec {
        name: "border",
        expands_to: &["border-color"],
        kind: ShorthandKind::BorderShorthand,
    },
    ShorthandSpec {
        name: "border-width",
        expands_to: &[],
        kind: ShorthandKind::Box,
    },
    ShorthandSpec {
        name: "border-top",
        expands_to: &[],
        kind: ShorthandKind::FallThrough,
    },
    ShorthandSpec {
        name: "border-right",
        expands_to: &[],
        kind: ShorthandKind::FallThrough,
    },
    ShorthandSpec {
        name: "border-bottom",
        expands_to: &[],
        kind: ShorthandKind::FallThrough,
    },
    ShorthandSpec {
        name: "border-left",
        expands_to: &[],
        kind: ShorthandKind::FallThrough,
    },
    ShorthandSpec {
        name: "background",
        expands_to: &[],
        kind: ShorthandKind::BackgroundShorthand,
    },
    ShorthandSpec {
        name: "flex",
        expands_to: &["flex-grow", "flex-shrink", "flex-basis"],
        kind: ShorthandKind::FallThrough,
    },
];

pub fn find_css_prop(name: &str) -> Option<&'static CssPropSpec> {
    CSS_PROPS.iter().find(|p| p.name == name)
}

pub fn find_shorthand(name: &str) -> Option<&'static ShorthandSpec> {
    CSS_SHORTHANDS.iter().find(|s| s.name == name)
}

/// 校验 `animation` 简写值的结构（轻量语法检查，捕捉拼写错误）。
///
/// 接受的最小子集（对齐 showcase 用法）：
/// - `none` —— 不跑动画
/// - `<name> <duration> [remainder...]` —— 至少 name + 一个 time 值（`<n>s` 或 `<n>ms`）；
///   remainder tokens 可任意顺序，每 token 须落入已知关键字类（easing / iteration-count /
///   fill-mode / direction / play-state / time）。
///
/// runtime 驱动在 §4 视觉束实现；本轮仅语法校验，不存值（apply_decl 不识别 `animation` →
/// resolve 阶段静默跳过）。语法合法但 runtime 不跑动画，符合「收到规则但 §4 没实现 → 静默忽略」。
pub fn validate_animation_value(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    if v.eq_ignore_ascii_case("none") {
        return true;
    }
    let mut tokens = v.split_whitespace();
    // 首 token = animation-name（标识符；不允许数字开头、不允许含特殊字符）
    let Some(name) = tokens.next() else {
        return false;
    };
    if !is_valid_animation_name(name) {
        return false;
    }
    // 至少跟一个 time（duration）。后续 token 需任一个匹配 time / 已知关键字。
    let mut saw_time = false;
    for tok in tokens {
        if is_time_token(tok) {
            saw_time = true;
        } else if !is_animation_keyword(tok) {
            return false;
        }
    }
    saw_time
}

/// animation-name 接受 CSS 自定义标识符（字母/-/_/数字，非数字开头；不允许 `--` 前缀）。
fn is_valid_animation_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    if first == '-' {
        // `-name` 允许；`--name` 是 CSS 变量，不是动画名
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

fn is_time_token(s: &str) -> bool {
    let stripped = s.strip_suffix("ms").or_else(|| s.strip_suffix("s"));
    let Some(num_part) = stripped else {
        return false;
    };
    !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// animation 简写中（除 name 与 time 外）的合法关键字（CSS animation-* 长属性的值域并集）。
fn is_animation_keyword(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        // iteration-count
        | "infinite"
        // timing-function（命名 + steps/cubic-bezier 的字面名不在此；多维函数式子交由 Raw 兜底）
        | "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end"
        // fill-mode
        | "none" | "forwards" | "backwards" | "both"
        // direction
        | "normal" | "reverse" | "alternate" | "alternate-reverse"
        // play-state
        | "running" | "paused"
        // iteration-count integer（任意无符号整数）
    ) || s.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css_rules::parse_style_block;

    #[test]
    fn known_css_props() {
        assert!(find_css_prop("width").is_some());
        assert!(find_css_prop("color").is_some());
        assert!(find_css_prop("display").is_some());
    }

    #[test]
    fn unknown_css_props() {
        assert!(find_css_prop("grid-template-columns").is_none());
        assert!(find_css_prop("cursor").is_none());
    }

    #[test]
    fn display_excludes_grid() {
        match &find_css_prop("display").unwrap().parser {
            CssValueParser::Keyword(kws) => assert!(!kws.contains(&"grid")),
            _ => panic!("expected Keyword"),
        }
    }

    #[test]
    fn shorthands_resolve() {
        assert!(find_shorthand("padding").is_some());
        assert!(find_shorthand("overflow").is_some());
        assert!(find_shorthand("background").is_some());
    }

    #[test]
    fn non_shorthand_returns_none() {
        assert!(find_shorthand("width").is_none());
    }

    #[test]
    fn overflow_is_replicate() {
        let sh = find_shorthand("overflow").unwrap();
        assert_eq!(sh.kind, ShorthandKind::Replicate);
        assert_eq!(sh.expands_to, &["overflow-x", "overflow-y"]);
    }

    #[test]
    fn animation_prop_registered() {
        // fence 终态契约（public-api.md §9：动画全在 CSS）——schema 注册是前提。
        let spec = find_css_prop("animation").expect("animation must be in CSS_PROPS");
        assert!(matches!(spec.parser, CssValueParser::Animation));
        assert!(!spec.inherited);
        assert_eq!(spec.default, "none");
    }

    #[test]
    fn animation_value_accepts_showcase_forms() {
        // 4 showcase HTML 用法 + none + 数字 iteration-count
        assert!(validate_animation_value("fadeIn .4s both"));
        assert!(validate_animation_value("shimmer 3s linear infinite"));
        assert!(validate_animation_value("charge 2s infinite alternate"));
        assert!(validate_animation_value("breathe 1.6s infinite"));
        assert!(validate_animation_value("none"));
        assert!(validate_animation_value("fadeIn 400ms ease-in 3"));
    }

    #[test]
    fn animation_value_rejects_garbage() {
        // 缺 duration / 数字开头 name / 空 / 未知关键字
        assert!(!validate_animation_value(""));
        assert!(!validate_animation_value("fadeIn")); // 缺 duration
        assert!(!validate_animation_value("123 2s")); // 数字开头 name
        assert!(!validate_animation_value("fadeIn 2s bogusKeyword"));
        assert!(!validate_animation_value("--custom 2s")); // CSS 变量前缀作 name
    }

    #[test]
    fn border_style_registered() {
        let spec = find_css_prop("border-style").expect("border-style must be in fence");
        let allowed = match &spec.parser {
            CssValueParser::Keyword(k) => k,
            _ => panic!("border-style must be Keyword parser"),
        };
        assert!(allowed.contains(&"none"));
        assert!(allowed.contains(&"solid"));
        assert_eq!(spec.default, "none");
        assert!(!spec.inherited);
    }

    #[test]
    fn resize_prop_accepted_as_noop() {
        // resize 进 CSS_PROPS（find_css_prop 命中），值 none/both/horizontal/vertical 接受
        assert!(find_css_prop("resize").is_some());
        // 通过 parse_style_block 验：含 resize:none 的规则不产 prop 名诊断
        let (_, _, diags) = parse_style_block("textarea { resize: none }");
        let resize_diag = diags.iter().find(|d| d.message.contains("resize"));
        assert!(resize_diag.is_none(), "resize 不该报 prop 名错：{diags:?}");
    }
}
