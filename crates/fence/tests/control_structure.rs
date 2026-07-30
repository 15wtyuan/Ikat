//! 控件结构契约校验（Stage 6.8，端到端）。
//!
//! role 化重构后（spec §2.2），作者自写控件结构——可能漏写必需子节点。打包期
//! `FenceMissingControlChild` error 严格拦截，不依赖运行时 reparent 兜底。本测试覆盖
//! 端到端 parse_template：role 驱动控件缺必需子 → error；旧标签控件不触发。

use loomgui_fence::diagnostic::{DiagnosticCode, Severity};
use loomgui_fence::pipeline::parse_template;

fn struct_errors(html: &str) -> Vec<String> {
    let result = parse_template(html, "t.html");
    result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == DiagnosticCode::FenceMissingControlChild && d.severity == Severity::Error
        })
        .map(|d| d.message.clone())
        .collect()
}

// ── combobox / listbox ──

#[test]
fn combobox_missing_listbox_reports_error() {
    let msgs = struct_errors(r#"<div role="combobox"></div>"#);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    let m = &msgs[0];
    assert!(m.contains("combobox"));
    assert!(m.contains("listbox"));
}

#[test]
fn combobox_with_option_but_no_listbox_reports_error() {
    // option 直接挂 combobox（缺 listbox 中间层）→ 打包期报 error，不依赖运行时 reparent
    let msgs = struct_errors(r#"<div role="combobox"><div role="option">A</div></div>"#);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
}

#[test]
fn listbox_without_option_reports_error() {
    let msgs = struct_errors(r#"<div role="listbox"></div>"#);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("option"));
}

#[test]
fn combobox_full_structure_passes() {
    let msgs = struct_errors(
        r#"<div role="combobox"><div role="listbox"><div role="option">A</div></div></div>"#,
    );
    assert!(msgs.is_empty(), "{msgs:?}");
}

// ── slider / progressbar (data-slot) ──

#[test]
fn slider_missing_thumb_reports_error() {
    let msgs = struct_errors(r#"<div role="slider"></div>"#);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("thumb"));
}

#[test]
fn progressbar_missing_fill_reports_error() {
    let msgs = struct_errors(r#"<div role="progressbar"></div>"#);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("fill"));
}

#[test]
fn slider_with_thumb_passes() {
    let msgs = struct_errors(r#"<div role="slider"><div data-slot="thumb"></div></div>"#);
    assert!(msgs.is_empty(), "{msgs:?}");
}

#[test]
fn progressbar_with_fill_passes() {
    let msgs = struct_errors(r#"<div role="progressbar"><div data-slot="fill"></div></div>"#);
    assert!(msgs.is_empty(), "{msgs:?}");
}

// ── list ──

#[test]
fn list_missing_listitem_reports_error() {
    let msgs = struct_errors(r#"<div role="list"></div>"#);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("listitem"));
}

#[test]
fn list_with_listitem_passes() {
    let msgs = struct_errors(r#"<div role="list"><div role="listitem">A</div></div>"#);
    assert!(msgs.is_empty(), "{msgs:?}");
}

// ── 无必需子角色的控件（不校验）──

#[test]
fn controls_without_required_children_not_checked() {
    // textbox / spinbutton / switch / radio：裸节点不报结构 error
    let msgs = struct_errors(
        r#"<div role="textbox"></div><div role="spinbutton"></div><div role="switch"></div><div role="radio"></div>"#,
    );
    assert!(msgs.is_empty(), "{msgs:?}");
}

// ── 旧标签控件不触发（showcase 中间态）──

#[test]
fn legacy_control_tags_not_checked() {
    // select / progress / input：走 legacy tag 映射（无 role 属性）→ 不触发结构契约
    // （它们仍走 control_css_check，但本测试只过滤结构契约 code）
    let msgs = struct_errors(
        r#"<select><option value="a">A</option></select><progress value="1"></progress><input type="range">"#,
    );
    assert!(msgs.is_empty(), "旧标签不应触发结构契约: {msgs:?}");
}

// ── 必需子必须是直接子 ──

#[test]
fn required_child_must_be_direct() {
    // thumb 嵌在 wrapper 里不算直接子（spec §2.2 字面结构）
    let msgs = struct_errors(
        r#"<div role="slider"><div class="wrap"><div data-slot="thumb"></div></div></div>"#,
    );
    assert_eq!(msgs.len(), 1, "{msgs:?}");
}

// ── 教学文案自包含 ──

#[test]
fn missing_child_message_is_actionable() {
    let msgs = struct_errors(r#"<div role="combobox"></div>"#);
    let m = &msgs[0];
    // 必须告诉作者该写什么结构（含 role=listbox + 指向 spec §2.2 契约表）
    assert!(m.contains("role=\"listbox\""), "{}", m);
    assert!(m.contains("§2.2"), "{}", m);
}
