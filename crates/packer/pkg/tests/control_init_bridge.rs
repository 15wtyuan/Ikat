//! bridge 提取控件 HTML 属性 → ControlInit 的契约测试。
//!
//! bridge() 在 IrTree→TemplateNode 翻译时，按 NodeKind 从 HTML 属性提取控件初始值
//! （value/max/min/step/checked/name/selected），填进 TemplateNode.control_init，使其随
//! pkg.bin 存活到运行时 instantiate。此文件覆盖所有控件 NodeKind 的提取契约
//! （progress/slider/toggle/radio/text/textarea/dropdown/number）+ ProgressBar 无 value
//! 的 indeterminate 语义。

use loomgui_core::asset::{ControlInit, TemplateNode};
use loomgui_core::scene::NodeKind;
use loomgui_pkg::bridge::bridge;

/// fence parse → bridge → 首节点。单根契约下 [0] 即根元素。
/// 断言 diagnostics 为空（parse 干净），否则 bridge 行为无意义。
///
/// 注入一条匹配所有控件的 `<style>` 规则，满足围栏 control-css 契约
/// （控件不带 UA 默认样式，须被 CSS 命中）。本测试文件聚焦 bridge 提取，
/// 非围栏校验本身。
fn run_bridge(html: &str) -> Vec<TemplateNode> {
    let wrapped = format!(
        r#"<style>progress,input[type="range"],input[type="checkbox"],input[type="radio"],input[type="text"],input[type="password"],input[type="search"],input[type="number"],textarea,select,option{{background:#ddd}}</style>{html}"#
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
fn bridge_extracts_progress_attrs() {
    let html = r#"<progress value="70" max="100"></progress>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::ProgressBar);
    let init = node.control_init.as_ref().expect("control_init set");
    assert!(matches!(
        init,
        ControlInit::Progress {
            value: 70.0,
            max: 100.0,
            indeterminate: false
        }
    ));
}

#[test]
fn bridge_extracts_progress_indeterminate_when_no_value() {
    // 无 value 属性的 <progress> 视为 indeterminate（HTML 语义：浏览器同样把无 value 的
    // progress 渲染为旋转动画）。value 缺省 0.0，max 缺省 100.0，indeterminate=true。
    let html = r#"<progress max="100"></progress>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::ProgressBar);
    // ProgressBar 即便无 value 也必须产 ControlInit（不能 None）——否则 indeterminate 语义丢失。
    let init = node.control_init.as_ref().expect("control_init set");
    assert!(matches!(
        init,
        ControlInit::Progress {
            value: 0.0,
            max: 100.0,
            indeterminate: true
        }
    ));

    // 无 value 无 max 同样 indeterminate，max 走缺省 100.0。
    let node2 = &run_bridge(r#"<progress></progress>"#)[0];
    let init2 = node2.control_init.as_ref().expect("control_init set");
    assert!(matches!(
        init2,
        ControlInit::Progress {
            value: 0.0,
            max: 100.0,
            indeterminate: true
        }
    ));
}

#[test]
fn bridge_extracts_slider_attrs() {
    let html = r#"<input type="range" min="0" max="100" step="5" value="50">"#;
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
fn bridge_extracts_slider_without_value_is_none() {
    // Slider 无 value 属性 → control_init=None（运行时 instantiate 用默认值兜底）。
    let html = r#"<input type="range" min="0" max="100">"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::Slider);
    assert!(
        node.control_init.is_none(),
        "Slider without value should yield control_init=None (runtime default fallback)"
    );
}

#[test]
fn bridge_extracts_checkbox_attrs() {
    let html = r#"<input type="checkbox" checked>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::Toggle);
    assert!(matches!(
        node.control_init,
        Some(ControlInit::Toggle { checked: true })
    ));
}

#[test]
fn bridge_extracts_checkbox_unchecked() {
    // 无 checked 属性 → Toggle{checked:false}（仍产 ControlInit，显式记录未勾选状态）。
    let html = r#"<input type="checkbox">"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::Toggle);
    assert!(matches!(
        node.control_init,
        Some(ControlInit::Toggle { checked: false })
    ));
}

#[test]
fn bridge_extracts_radio_name() {
    let html = r#"<input type="radio" name="grp" checked>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::RadioButton);
    assert!(matches!(
        node.control_init,
        Some(ControlInit::Radio {
            checked: true,
            ref name
        }) if name == "grp"
    ));
}

#[test]
fn bridge_extracts_text_attrs() {
    let html = r#"<input type="text" value="bob" placeholder="name" maxlength="20">"#;
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
fn bridge_extracts_textarea_attrs() {
    let html = r#"<textarea placeholder="body" maxlength="500">hello</textarea>"#;
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
fn bridge_extracts_password_attrs_as_textfield() {
    // <input type="password"> is web-only; it folds to a plain TextField (games
    // self-implement masking). Its value/placeholder/maxlength/readonly still
    // extract via the shared TextField path.
    let html =
        r#"<input type="password" value="secret" placeholder="pwd" maxlength="32" readonly>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::TextField);
    match &node.control_init {
        Some(ControlInit::TextField(e)) => {
            assert_eq!(e.value, "secret");
            assert_eq!(e.placeholder, "pwd");
            assert_eq!(e.max_length, 32);
            assert!(e.readonly);
        }
        other => panic!("expected TextField, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_search_attrs_as_textfield() {
    // <input type="search"> is web-only; it folds to a plain TextField.
    let html = r#"<input type="search" value="query" placeholder="search..." maxlength="100">"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::TextField);
    match &node.control_init {
        Some(ControlInit::TextField(e)) => {
            assert_eq!(e.value, "query");
            assert_eq!(e.placeholder, "search...");
            assert_eq!(e.max_length, 100);
            assert!(!e.readonly);
        }
        other => panic!("expected TextField, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_dropdown_selected_index_from_option_selected() {
    // <select> 的 option[selected] 决定初始选中索引（这里第 2 项 selected → index 1）。
    let html = r#"<select id="s"><option value="a">A</option><option value="b" selected>B</option></select>"#;
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown { selected_index: 1 })
    ));
}

#[test]
fn bridge_extracts_dropdown_selected_index_ignores_whitespace_text_children() {
    // 多行 HTML：option 之间夹着空白 Text 节点（fence 只剥顶层空白，in-element 保留）。
    // selected_index 必须是「第几个 option」，而非「children 里的第几个」——否则
    // option_b 会被误算成 index 3（2 个前置空白 Text + option_a 占 child 下标 0..3），
    // 而它实际是第 2 个 option（index 1）。回归 bug：旧实现用 children 的 enumerate 下标。
    let html = "<select id=\"s\">\n  <option value=\"a\">A</option>\n  <option value=\"b\" selected>B</option>\n</select>";
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown { selected_index: 1 })
    ));
}

#[test]
fn bridge_extracts_dropdown_no_selected_defaults_to_zero() {
    // 无 option 带 selected → 默认首项（index 0）。
    let html =
        r#"<select id="s"><option value="a">A</option><option value="b">B</option></select>"#;
    let nodes = run_bridge(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown { selected_index: 0 })
    ));
}

#[test]
fn bridge_extracts_number_field_min_max_step_value() {
    // <input type="number"> 的 value/min/max/step 全部从属性提取。
    let html = r#"<input type="number" value="5" min="0" max="10" step="2">"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::NumberField);
    match &node.control_init {
        Some(ControlInit::NumberField {
            edit,
            min,
            max,
            step,
        }) => {
            assert_eq!(edit.value, "5");
            assert_eq!(*min, 0.0);
            assert_eq!(*max, 10.0);
            assert_eq!(*step, 2.0);
        }
        other => panic!("expected NumberField, got {:?}", other),
    }
}

// == role-driven extraction (Task 8.5) ==
//
// role-driven `<div role="...">` controls carry init values in ARIA
// (`aria-valuenow`/`aria-checked`/...) and `data-*` (`data-step`/`data-name`)
// attributes, because the fence forbids plain attributes on `<div>`. These
// tests lock the bridge's ARIA-first extraction; the legacy-tag tests above
// lock the fallback path that stays live until Task 7 retires the tags.

/// Like `run_bridge`, but the injected `<style>` targets role selectors so the
/// control-css contract is satisfied for `<div role="...">` controls. The legacy
/// helper's tag selectors (`progress`/`input[...]`) do not match divs.
fn run_bridge_role(html: &str) -> Vec<TemplateNode> {
    let wrapped = format!(
        r#"<style>[role="progressbar"],[role="slider"],[role="spinbutton"],[role="switch"],[role="radio"],[role="textbox"],[role="combobox"]{{background:#ddd}}</style>{html}"#
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
    let node = &run_bridge_role(html)[0];
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
    let node = &run_bridge_role(html)[0];
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
    let node = &run_bridge_role(html)[0];
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
    // role=slider without aria-valuenow → control_init=None (runtime default),
    // matching the legacy `<input type=range>` no-value contract.
    let html = r#"<div role="slider" aria-valuemin="0" aria-valuemax="100"><div data-slot="thumb"></div></div>"#;
    let node = &run_bridge_role(html)[0];
    assert_eq!(node.kind, NodeKind::Slider);
    assert!(
        node.control_init.is_none(),
        "Slider without aria-valuenow should yield control_init=None"
    );
}

#[test]
fn bridge_extracts_switch_role_aria_checked() {
    // role=switch maps to Toggle; aria-checked is a tri-state string ("true"/"false").
    let on = &run_bridge_role(r#"<div role="switch" aria-checked="true"></div>"#)[0];
    assert_eq!(on.kind, NodeKind::Toggle);
    assert!(matches!(
        on.control_init,
        Some(ControlInit::Toggle { checked: true })
    ));

    let off = &run_bridge_role(r#"<div role="switch" aria-checked="false"></div>"#)[0];
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
    let node = &run_bridge_role(html)[0];
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
    let node2 = &run_bridge_role(html2)[0];
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
    let node = &run_bridge_role(html)[0];
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
    let node = &run_bridge_role(html)[0];
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
    let node = &run_bridge_role(html)[0];
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
    let node = &run_bridge_role(html)[0];
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
    let html = r#"<div role="combobox"><div role="listbox"><div role="option">A</div><div role="option">B</div><div role="option" aria-selected="true">C</div></div></div>"#;
    let nodes = run_bridge_role(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown { selected_index: 2 })
    ));
}

#[test]
fn bridge_extracts_dropdown_role_no_aria_selected_defaults_zero() {
    // No aria-selected on any option → default first option (index 0).
    let html = r#"<div role="combobox"><div role="listbox"><div role="option">A</div><div role="option">B</div></div></div>"#;
    let nodes = run_bridge_role(html);
    let sel = nodes
        .iter()
        .find(|n| n.kind == NodeKind::Dropdown)
        .expect("Dropdown node missing");
    assert!(matches!(
        sel.control_init,
        Some(ControlInit::Dropdown { selected_index: 0 })
    ));
}

#[test]
fn bridge_aria_preferred_over_legacy_when_both_present() {
    // When both the ARIA source and the legacy plain attribute are present,
    // ARIA wins. Uses <progress> (which legally carries both the global
    // aria-valuenow and its content-attr value) to lock the precedence.
    let html =
        r#"<progress value="999" aria-valuenow="50" max="1000" aria-valuemax="100"></progress>"#;
    let node = &run_bridge(html)[0];
    assert_eq!(node.kind, NodeKind::ProgressBar);
    let init = node.control_init.as_ref().expect("control_init set");
    assert!(matches!(
        init,
        ControlInit::Progress {
            value: 50.0,
            max: 100.0,
            indeterminate: false
        }
    ));
}
