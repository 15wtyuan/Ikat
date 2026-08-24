//! bridge 提取控件 ARIA/data-* 属性 → ControlInit 的契约测试。
//!
//! bridge() 在 IrTree→TemplateNode 翻译时，按 NodeKind 从 HTML 属性提取控件初始值
//! （value/max/min/step/checked/name/selected），填进 TemplateNode.control_init，使其随
//! pkg.bin 存活到运行时 instantiate。控件一律 role 驱动，初始值放在
//! ARIA（aria-valuenow/aria-checked/...）或 data-*（data-step/data-name）属性里。

use loomgui_core::asset::{ControlInit, TemplateNode};
use loomgui_core::scene::NodeKind;
use loomgui_pkg::bridge::bridge;

/// fence parse → bridge → 节点列表。单根契约下 [0] 即根元素。
/// 断言 diagnostics 为空（parse 干净），否则 bridge 行为无意义。
///
/// 注入一条匹配所有 role 控件的选择器，满足围栏 control-css 契约
/// （控件不带 UA 默认样式，须被 CSS 命中）。本测试文件聚焦 bridge 提取，
/// 非围栏校验本身。
fn run_bridge(html: &str) -> Vec<TemplateNode> {
    let wrapped = format!(
        r#"<style>[role="progressbar"],[role="slider"],[role="spinbutton"],[role="switch"],[role="radio"],[role="textbox"],[role="combobox"]{{background:#ddd;position:relative}} [role="slider"] [data-slot="thumb"],[role="progressbar"] [data-slot="fill"],[role="combobox"] [data-slot="value"],[role="option"],[role="listitem"],[role="tab"]{{background:#444}} [role="combobox"] [role="listbox"]{{display:none;position:absolute}}</style>{html}"#
    );
    let parsed = loomgui_fence::parse_template(&wrapped, "test.html");
    assert!(
        parsed.diagnostics.is_empty(),
        "parse diags (bridge 行为无意义): {:?}",
        parsed.diagnostics
    );
    bridge(&parsed).expect("bridge ok")
}

#[test]
fn bridge_extracts_progress_role_aria_attrs() {
    // <div role="progressbar" aria-valuenow/min/max> → Progress{value,max,false}.
    let html = r#"<div role="progressbar" aria-valuenow="780" aria-valuemin="0" aria-valuemax="1000"><div data-slot="fill"></div></div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::ProgressBar);
    let init = node.control_init.as_ref().expect("control_init set");
    assert!(matches!(
        init,
        ControlInit::Progress {
            value: 780.0,
            max: 1000.0,
            indeterminate: false
        }
    ));
}

#[test]
fn bridge_extracts_progress_role_indeterminate_when_no_valuenow() {
    // role=progressbar without aria-valuenow → indeterminate (mirrors HTML <progress>
    // semantics: value absent = spinning). aria-valuemin/max still read.
    let html = r#"<div role="progressbar" aria-valuemin="0" aria-valuemax="100"><div data-slot="fill"></div></div>"#;
    let node = &run_bridge(html)[0];
    let init = node.control_init.as_ref().expect("control_init set");
    assert!(matches!(
        init,
        ControlInit::Progress {
            value: 0.0,
            max: 100.0,
            indeterminate: true
        }
    ));
}

#[test]
fn bridge_extracts_slider_role_aria_and_data_attrs() {
    // role=slider: aria-valuenow/min/max + data-step.
    let html = r#"<div role="slider" aria-valuenow="50" aria-valuemin="0" aria-valuemax="100" data-step="5"><div data-slot="thumb"></div></div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::Slider);
    assert!(matches!(
        node.control_init,
        Some(ControlInit::Slider {
            value: 50.0,
            min: 0.0,
            max: 100.0,
            step: 5.0
        })
    ));
}

#[test]
fn bridge_extracts_slider_role_without_valuenow_is_none() {
    // role=slider without aria-valuenow → control_init=None (runtime default).
    let html = r#"<div role="slider" aria-valuemin="0" aria-valuemax="100"><div data-slot="thumb"></div></div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::Slider);
    assert!(
        node.control_init.is_none(),
        "Slider without aria-valuenow should yield control_init=None"
    );
}

#[test]
fn bridge_extracts_switch_role_aria_checked() {
    // role=switch maps to Toggle; aria-checked is a tri-state string ("true"/"false").
    let on = &run_bridge(r#"<div role="switch" aria-checked="true"></div>"#)[0];
    assert_eq!(on.kind, NodeKind::Toggle);
    assert!(matches!(
        on.control_init,
        Some(ControlInit::Toggle { checked: true })
    ));

    let off = &run_bridge(r#"<div role="switch" aria-checked="false"></div>"#)[0];
    assert!(matches!(
        off.control_init,
        Some(ControlInit::Toggle { checked: false })
    ));
}

#[test]
fn bridge_extracts_radio_role_aria_checked_and_data_name() {
    // role=radio: aria-checked + data-name (data-name carries the radio group —
    // ARIA has no group-name attribute, so data-name is the contract).
    let html = r#"<div role="radio" aria-checked="true" data-name="gender"></div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::RadioButton);
    assert!(matches!(
        node.control_init,
        Some(ControlInit::Radio {
            checked: true,
            ref name
        }) if name == "gender"
    ));

    // aria-checked="false" → unchecked, still records the radio.
    let html2 = r#"<div role="radio" aria-checked="false" data-name="gender"></div>"#;
    let node2 = &run_bridge(html2)[0];
    assert!(matches!(
        node2.control_init,
        Some(ControlInit::Radio {
            checked: false,
            ref name
        }) if name == "gender"
    ));
}

#[test]
fn bridge_extracts_textfield_role_aria_placeholder_and_text_content() {
    // role=textbox (no aria-multiline) → TextField. ARIA has no textbox-value
    // attribute, so the value comes from element text content; placeholder and
    // maxlength come from aria-placeholder / data-maxlength.
    let html = r#"<div role="textbox" aria-placeholder="name" data-maxlength="20">bob</div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::TextField);
    match &node.control_init {
        Some(ControlInit::TextField(e)) => {
            assert_eq!(e.value, "bob");
            assert_eq!(e.placeholder, "name");
            assert_eq!(e.max_length, 20);
            assert!(!e.readonly);
        }
        other => panic!("expected TextField, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_textfield_role_aria_readonly() {
    // aria-readonly="true" → readonly flag (tri-state string like aria-checked).
    let html = r#"<div role="textbox" aria-readonly="true"></div>"#;
    let node = &run_bridge(html)[0];
    match &node.control_init {
        Some(ControlInit::TextField(e)) => assert!(e.readonly),
        other => panic!("expected TextField, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_textarea_role_aria_multiline_value_from_text() {
    // role=textbox + aria-multiline="true" → TextArea; value from element text
    // content (HTML <textarea> semantics), placeholder/maxlength from aria/data.
    let html = r#"<div role="textbox" aria-multiline="true" aria-placeholder="body" data-maxlength="500">hello</div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::TextArea);
    match &node.control_init {
        Some(ControlInit::TextArea(e)) => {
            assert_eq!(e.value, "hello");
            assert_eq!(e.placeholder, "body");
            assert_eq!(e.max_length, 500);
            assert!(!e.readonly);
        }
        other => panic!("expected TextArea, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_numberfield_role_spinbutton_aria_attrs() {
    // role=spinbutton → NumberField. edit.value from aria-valuenow; min/max/step
    // from aria-valuemin/aria-valuemax/data-step.
    let html = r#"<div role="spinbutton" aria-valuenow="32" aria-valuemin="1" aria-valuemax="64" data-step="1"></div>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::NumberField);
    match &node.control_init {
        Some(ControlInit::NumberField {
            edit,
            min,
            max,
            step,
        }) => {
            assert_eq!(edit.value, "32");
            assert_eq!(*min, 1.0);
            assert_eq!(*max, 64.0);
            assert_eq!(*step, 1.0);
        }
        other => panic!("expected NumberField, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_dropdown_role_aria_selected_index() {
    // role=combobox > role=listbox > role=option. Options live inside a listbox
    // popup (a structural requirement), so the bridge must walk the subtree, not
    // just direct children. aria-selected="true" on the 3rd option → index 2.
    let html = r#"<div role="combobox"><div data-slot="value">A</div><div role="listbox"><div role="option">A</div><div role="option">B</div><div role="option" aria-selected="true">C</div></div></div>"#;
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown {
            selected_index: 2,
            ..
        })
    ));
}

#[test]
fn bridge_extracts_dropdown_role_no_aria_selected_defaults_zero() {
    // No aria-selected on any option → default first option (index 0).
    let html = r#"<div role="combobox"><div data-slot="value">A</div><div role="listbox"><div role="option">A</div><div role="option">B</div></div></div>"#;
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown {
            selected_index: 0,
            ..
        })
    ));
}

#[test]
fn bridge_extracts_dropdown_option_values() {
    // Per-option `value` content attribute, declaration order, absent → None slot
    // (runtime falls back to the option text). Same subtree walk as selected_index.
    let html = r#"<div role="combobox"><div data-slot="value">A</div><div role="listbox"><div role="option" value="en">English</div><div role="option">中文</div><div role="option" value="ja" aria-selected="true">日本語</div></div></div>"#;
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    match &sel.control_init {
        Some(ControlInit::Dropdown {
            selected_index: 2,
            option_values,
        }) => {
            assert_eq!(option_values.len(), 3);
            assert_eq!(option_values[0].as_deref(), Some("en"));
            assert_eq!(option_values[1], None, "absent value → None slot");
            assert_eq!(option_values[2].as_deref(), Some("ja"));
        }
        other => panic!("expected Dropdown init, got {other:?}"),
    }
}

#[test]
fn bridge_extracts_dropdown_selected_index_ignores_whitespace_text_children() {
    // 多行 HTML：option 之间夹着空白 Text 节点（fence 只剥顶层空白，in-element 保留）。
    // selected_index 必须是「第几个 option」，而非「children 里的第几个」——否则
    // option_b 会被误算成 index 3（2 个前置空白 Text + option_a 占 child 下标 0..3），
    // 而它实际是第 2 个 option（index 1）。回归 bug：旧实现用 children 的 enumerate 下标。
    let html = "<div role=\"combobox\"><div data-slot=\"value\">B</div><div role=\"listbox\">\n  <div role=\"option\">A</div>\n  <div role=\"option\" aria-selected=\"true\">B</div>\n</div></div>";
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown {
            selected_index: 1,
            ..
        })
    ));
}
