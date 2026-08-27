//! #45 必需子节点 CSS 命中校验的集成测试（独立文件——lib 内嵌测试位曾出现
//! Windows 增量编译行号/断言错位，隔离后稳定复现与验证）。

use ikat_fence::control_css_check::check_control_css;
use ikat_fence::diagnostic::{DiagnosticCode, LineMap};
use ikat_fence::pipeline::parse_template;

fn check(html: &str) -> Vec<ikat_fence::diagnostic::Diagnostic> {
    let result = parse_template(html, "t.html");
    check_control_css(
        &result.tree,
        &result.dynamic_rules,
        "t.html",
        &LineMap::new(html),
    )
}

#[test]
fn thumb_without_css_errors_even_when_control_matched() {
    // #45 核心：控件本体有 CSS 命中，但 thumb 子无任何规则 → 可拖不可见的
    // 隐形滑块头，打包期报 FenceControlChildWithoutCss。
    let diags = check(
        r#"<div role="slider"><div data-slot="thumb"></div></div><style>[role=slider]{background-color:#222;width:100px;height:4px}</style>"#,
    );
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, DiagnosticCode::FenceControlChildWithoutCss);
    assert!(
        diags[0].message.contains("data-slot=\"thumb\""),
        "{}",
        diags[0].message
    );
}

#[test]
fn required_children_all_matched_clean() {
    // 控件 + 全部必需子都有规则命中 → 零诊断。
    let diags = check(
        r#"<div role="slider"><div data-slot="thumb"></div></div><style>[role=slider]{background-color:#222;width:100px;height:4px}[role=slider] [data-slot=thumb]{background-color:#fff;width:16px;height:16px}</style>"#,
    );
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn every_option_instance_needs_css() {
    // option 多实例逐个查：两个 option 只有一个被命中 → 未命中的那个报错
    //（存在一个被命中的不算过——每个列表行都需要样式）。
    let diags = check(
        r#"<div role="listbox"><div role="option" class="a">A</div><div role="option">B</div></div><style>.a{background-color:#333}</style>"#,
    );
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, DiagnosticCode::FenceControlChildWithoutCss);
}

#[test]
fn template_blueprint_listitem_needs_css() {
    // template 蓝图内的 listitem 同样查（蓝图无 CSS，克隆体也无）。
    let diags = check(
        r#"<div role="list"><template><div role="listitem">A</div></template></div><style>[role=list]{background-color:#111}</style>"#,
    );
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, DiagnosticCode::FenceControlChildWithoutCss);
    assert!(
        diags[0].message.contains("role=\"listitem\""),
        "{}",
        diags[0].message
    );
}

#[test]
fn combobox_value_slot_missing_is_structure_error() {
    // #45 附带：combobox 漏写 data-slot=value（选中值显示区）→ 6.8 结构门 error。
    let result = parse_template(
        r#"<div role="combobox"><div role="listbox"><div role="option">A</div></div></div>"#,
        "t.html",
    );
    let missing: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::FenceMissingControlChild)
        .collect();
    assert_eq!(missing.len(), 1, "{}", result.diagnostics.len());
    assert!(
        missing[0].message.contains("value"),
        "{}",
        missing[0].message
    );
}
