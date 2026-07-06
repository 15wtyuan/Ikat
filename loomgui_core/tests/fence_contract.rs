//! 围栏契约测试 = LoomGUI 围栏权威真相源（docs/design/fence.md 是人类副本）。
//! 三类断言：
//!   A. 元素围栏：围栏外标签报错（parse_html），白名单接受。
//!   B. 支持属性：apply_decl 返回 true + ResolvedStyle 字段变化。
//!   C. 围栏外属性：apply_decl 返回 false + 布局字段不变（静默忽略）。
//!   D. 属性选择器围栏：parse_selector 接受 [attr]/[attr="val"]（Exists+Eq），
//!      优先级 class 桶，围栏外操作符保守降级为 Exists。
//! 改 apply_decl / FENCE_TAGS / selector 必须同步本测试 + fence.md。

use loomgui_core::parse::css::parse_css;
use loomgui_core::parse::dom::parse_html;
use loomgui_core::parse::selector::parse_selector;
use loomgui_core::style::mapping::apply_decl;
use loomgui_core::style::resolved::ResolvedStyle;
use taffy::Display;

// ── A. 元素围栏 ──────────────────────────────────────────────────

#[test]
fn fence_tags_whitelist_accepted() {
    // FENCE_TAGS = div/span/img/button。l-container 不在围栏内（用 div，与 div 同映射冗余）。
    for tag in ["div", "span", "img", "button"] {
        let html = format!("<{tag}></{tag}>");
        assert!(parse_html(&html).is_ok(), "<{tag}> 应被围栏接受");
    }
}

#[test]
fn fence_out_tags_rejected() {
    // 围栏外标签一律报错，不降级。l-container 不在围栏内（用 div）。
    for tag in ["video", "input", "b", "section", "p", "ul", "l-container"] {
        let html = format!("<{tag}></{tag}>");
        assert!(parse_html(&html).is_err(), "<{tag}> 应被围栏拒绝");
    }
}

// ── B. 支持属性生效（apply_decl 返回 true）──────────────────────

#[test]
fn supported_layout_props_return_true() {
    let cases = [
        ("display", "flex"),
        ("flex-direction", "row"),
        ("flex-wrap", "wrap"),
        ("gap", "10px"),
        ("justify-content", "center"),
        ("align-items", "center"),
        ("width", "100px"),
        ("padding", "8px"),
        ("margin", "4px"),
        ("aspect-ratio", "1.5"),
        ("order", "2"),
    ];
    for (prop, val) in cases {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(&mut s, prop, val),
            "支持属性 {prop}:{val} 应返回 true"
        );
    }
}

#[test]
fn supported_visual_props_return_true() {
    let cases = [
        ("background-color", "#5fb2c4"),
        ("background-image", "url(\"a.png\")"),
        ("background-size", "cover"),
        ("border-radius", "4px"),
        ("opacity", "0.5"),
        ("overflow", "hidden"),
        ("color", "#e0e0e0"),
        ("font-size", "16px"),
        ("font-weight", "700"),
        ("text-align", "center"),
        ("white-space", "nowrap"),
        ("transform", "rotate(45deg)"),
        ("pointer-events", "none"),
        ("filter", "grayscale(1)"),
        ("border-image-slice", "10"),
        ("transition", "opacity 0.3s ease 0s"),
    ];
    for (prop, val) in cases {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(&mut s, prop, val),
            "支持属性 {prop}:{val} 应返回 true"
        );
    }
}

#[test]
fn background_size_rejects_two_values() {
    // background-size 只认 cover/contain/100%，拒两值如 "100% 50%"。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "background-size", "100% 50%"),
        "background-size 两值应被拒（返回 false）"
    );
}

#[test]
fn display_grid_falls_to_flex() {
    // display:grid 走 mapping.rs 非 none 分支 → Flex，返回 true。
    // taffy 无 grid，grid 写了等于 flex，AI 不可预测 → fence.md 标"禁写 grid"。
    let mut s = ResolvedStyle::default();
    let ok = apply_decl(&mut s, "display", "grid");
    assert!(ok, "display:grid 走非 none 分支返回 true（落 Flex）");
    assert_eq!(
        s.taffy_style.display,
        Display::Flex,
        "display:grid 应落到 Flex（taffy 无 grid）"
    );
}

// ── C. 围栏外属性静默忽略（apply_decl 返回 false，布局字段不变）─────
// fence.md §2.4 / §3.3 标【推断·待测】转【实证】的关键项。
// AI 写了以为生效、实际无效 = 不可预测，围栏禁写，测试锁定"无效"行为。

#[test]
fn fence_out_props_return_false() {
    let cases: [(&str, &str); 9] = [
        ("float", "left"),
        ("align-content", "center"),
        ("cursor", "pointer"),
        ("clip-path", "circle(50%)"),
        ("background-position", "center"),
        ("background-repeat", "no-repeat"),
        ("transform-origin", "top left"),
        ("font-style", "italic"),
        ("border-style", "dashed"),
    ];
    for (prop, val) in cases {
        let mut s = ResolvedStyle::default();
        assert!(
            !apply_decl(&mut s, prop, val),
            "围栏外属性 {prop}:{val} 应返回 false（静默忽略）"
        );
    }
}

#[test]
fn position_absolute_breaks_flow() {
    // v1.4-b：position:absolute 现在生效（围栏内）。apply_decl 返回 true，
    // taffy_style.position = Absolute（脱离流），inset 写进 taffy_style.inset。
    let mut s = ResolvedStyle::default();
    let applied = apply_decl(&mut s, "position", "absolute");
    assert!(applied, "position:absolute 应返回 true（围栏内）");
    assert_eq!(
        s.taffy_style.position,
        taffy::style::Position::Absolute,
        "position:absolute → taffy Absolute（脱离流）"
    );
}

#[test]
fn position_relative_explicit() {
    // relative 显式设（虽靠默认，但显式更清晰）。
    let mut s = ResolvedStyle::default();
    s.taffy_style.position = taffy::style::Position::Absolute; // 先改成非默认
    assert!(apply_decl(&mut s, "position", "relative"));
    assert_eq!(s.taffy_style.position, taffy::style::Position::Relative);
}

#[test]
fn position_fixed_sticky_ignored() {
    // fixed/sticky 围栏外（静默忽略，保持默认 Relative）。
    for val in ["fixed", "sticky"] {
        let mut s = ResolvedStyle::default();
        assert!(
            !apply_decl(&mut s, "position", val),
            "position:{val} 围栏外 → false"
        );
        assert_eq!(
            s.taffy_style.position,
            taffy::style::Position::Relative,
            "position:{val} 不改默认 Relative"
        );
    }
}

#[test]
fn inset_top_writes_taffy_inset() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "top", "10px"));
    assert_eq!(
        s.taffy_style.inset.top,
        taffy::style::LengthPercentageAuto::Length(10.0)
    );
    // 未设的边保持 auto。
    assert_eq!(
        s.taffy_style.inset.bottom,
        taffy::style::LengthPercentageAuto::Auto
    );
}

#[test]
fn inset_four_sides() {
    let mut s = ResolvedStyle::default();
    apply_decl(&mut s, "top", "1px");
    apply_decl(&mut s, "right", "2px");
    apply_decl(&mut s, "bottom", "3px");
    apply_decl(&mut s, "left", "4px");
    assert_eq!(
        s.taffy_style.inset.top,
        taffy::style::LengthPercentageAuto::Length(1.0)
    );
    assert_eq!(
        s.taffy_style.inset.right,
        taffy::style::LengthPercentageAuto::Length(2.0)
    );
    assert_eq!(
        s.taffy_style.inset.bottom,
        taffy::style::LengthPercentageAuto::Length(3.0)
    );
    assert_eq!(
        s.taffy_style.inset.left,
        taffy::style::LengthPercentageAuto::Length(4.0)
    );
}

#[test]
fn inset_auto_keeps_default() {
    let mut s = ResolvedStyle::default();
    apply_decl(&mut s, "top", "10px");
    apply_decl(&mut s, "top", "auto"); // auto 显式置回默认 Auto（覆盖之前的 px 值）
    assert_eq!(
        s.taffy_style.inset.top,
        taffy::style::LengthPercentageAuto::Auto
    );
}

#[test]
fn transform_skew_does_not_apply() {
    // transform 只认 translate/rotate/scale，skew 显式跳过（mapping.rs:278）。
    // apply_decl("transform",...) 返回 true（进 match arm），但 transform 字段无变化。
    let mut s1 = ResolvedStyle::default();
    let applied = apply_decl(&mut s1, "transform", "skew(10deg,5deg)");
    assert!(
        applied,
        "skew 应进 transform arm 返回 true（no-op 但进 arm）"
    );
    let s2 = ResolvedStyle::default();
    assert_eq!(s1.transform, s2.transform, "skew 不应改变 transform 字段");
}

#[test]
fn at_rule_media_skipped_by_parser() {
    // @media 被 AtRuleParser 默认拒（parse/css.rs:58-63），整块跳过不报错。
    let css = "@media (min-width: 600px) { .a { width: 100px; } }";
    let sheet = parse_css(css).expect("parse_css 不应 panic");
    // @media 块被跳过，sheet 里无 .a 规则。
    assert!(
        sheet.rules.is_empty(),
        "@media 块应被跳过，规则不进 StyleSheet"
    );
}

#[test]
fn transition_prop_parsed() {
    // CSS transition 属性解析→TransitionSpec（prop:None=all, duration/delay/ease 各字段）
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "transition", "opacity 0.3s ease 0s"));
    let ts = s.transition.expect("transition 已设置");
    assert_eq!(ts.prop, Some(loomgui_core::tween::TweenProp::Opacity));
    assert!((ts.duration - 0.3).abs() < 1e-5, "duration=0.3s");
    assert_eq!(ts.ease, loomgui_core::tween::Ease::QuadOut);
    assert!((ts.delay - 0.0).abs() < 1e-5, "delay=0s");
}

#[test]
fn transition_all_and_color_parsed() {
    // all→None；color→TextColor；background-color→BgColor。
    for (val, expected_prop) in [
        ("all 0.2s linear 0s", None),
        (
            "color 0.5s ease-in 0.1s",
            Some(loomgui_core::tween::TweenProp::TextColor),
        ),
        (
            "background-color 0.4s ease-out 0s",
            Some(loomgui_core::tween::TweenProp::BgColor),
        ),
    ] {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(&mut s, "transition", val),
            "transition {val} 应返回 true"
        );
        let ts = s.transition.expect("transition 已设置");
        assert_eq!(ts.prop, expected_prop, "prop mismatch for {val}");
    }
}

#[test]
fn transition_defaults_for_omitted_tokens() {
    // 只写属性名 → duration=0, ease=Linear, delay=0 默认。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "transition", "opacity"));
    let ts = s.transition.expect("transition 已设置");
    assert_eq!(ts.prop, Some(loomgui_core::tween::TweenProp::Opacity));
    assert!((ts.duration - 0.0).abs() < 1e-5, "缺 duration 默认 0");
    assert_eq!(ts.ease, loomgui_core::tween::Ease::Linear);
    assert!((ts.delay - 0.0).abs() < 1e-5, "缺 delay 默认 0");
}

// ── D. 属性选择器围栏 ──
// 属性选择器 `[attr]` / `[attr="val"]` 已从围栏外升入围栏内（v1.5）。
// 仅 Exists + Eq 两操作符；围栏外操作符（~=, ^= 等）保守降级为 Exists 且丢弃值。
// 优先级在 class 桶（同 .class），非 tag 桶。

#[test]
fn parse_attr_selector_exists() {
    // [attr] → Exists 操作符，value=None。
    let s = parse_selector("[data-controller]").expect("应解析成功");
    assert_eq!(s.compound.len(), 1);
    assert_eq!(s.compound[0].attrs.len(), 1);
    let a = &s.compound[0].attrs[0];
    assert_eq!(a.name, "data-controller");
    assert!(matches!(a.op, loomgui_core::style::dynamic::AttrOp::Exists));
    assert!(a.value.is_none());
}

#[test]
fn parse_attr_selector_eq() {
    // [attr="val"] → Eq 操作符，value=字面值。
    let s = parse_selector(r#"[data-page="1"]"#).expect("应解析成功");
    let a = &s.compound[0].attrs[0];
    assert_eq!(a.name, "data-page");
    assert!(matches!(a.op, loomgui_core::style::dynamic::AttrOp::Eq));
    assert_eq!(a.value.as_deref(), Some("1"));
}

#[test]
fn attr_selector_specificity_class_bucket() {
    // 属性选择器优先级 = class 桶 (0,1,0)，非 tag 桶 (0,0,1)。
    let s = parse_selector("[data-controller]").expect("应解析成功");
    assert_eq!(
        s.specificity,
        loomgui_core::style::dynamic::Specificity(0, 1, 0),
        "[attr] specificity = (0,1,0)（class 桶）"
    );
}

#[test]
fn attr_selector_with_class_specificity_summed() {
    // [data-page="1"].panel → (0,2,0)：attr + class 各贡献一个 class 桶。
    let s = parse_selector(r#"[data-page="1"].panel"#).expect("应解析成功");
    assert_eq!(
        s.specificity,
        loomgui_core::style::dynamic::Specificity(0, 2, 0)
    );
}

#[test]
fn parse_attr_combined_tag_class_attr() {
    // div.panel[data-controller="tab"] → tag + class + attr 共存于同一 compound。
    let s = parse_selector(r#"div.panel[data-controller="tab"]"#).expect("应解析成功");
    let c = &s.compound[0];
    assert_eq!(c.tag.as_deref(), Some("div"));
    assert_eq!(c.classes, vec!["panel".to_string()]);
    assert_eq!(c.attrs.len(), 1);
    assert_eq!(c.attrs[0].name, "data-controller");
    assert!(matches!(
        c.attrs[0].op,
        loomgui_core::style::dynamic::AttrOp::Eq
    ));
    assert_eq!(c.attrs[0].value.as_deref(), Some("tab"));
}

#[test]
fn attr_selector_fence_out_op_degrades_to_exists() {
    // 围栏外操作符（~=, ^= 等）→ 保守降级为 Exists：name 去操作符，value 丢弃。
    // 不作解析错误（语义限定），属宽容降级。
    let s = parse_selector(r#"[attr~="val"]"#).expect("应解析成功");
    let a = &s.compound[0].attrs[0];
    assert_eq!(a.name, "attr");
    assert!(matches!(a.op, loomgui_core::style::dynamic::AttrOp::Exists));
    assert!(a.value.is_none(), "围栏外操作符丢弃 value");
}

#[test]
fn attr_selector_name_normalized_to_lowercase() {
    // HTML 属性名大小写不敏感 → parse 内小写归一。
    let s = parse_selector(r#"[DATA-CONTROLLER="Tab"]"#).expect("应解析成功");
    let a = &s.compound[0].attrs[0];
    assert_eq!(a.name, "data-controller");
    assert_eq!(a.value.as_deref(), Some("Tab"));
    // 值保留原样（大小写敏感），仅 name 归一。
}
