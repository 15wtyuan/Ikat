#![cfg(feature = "parse")]

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
