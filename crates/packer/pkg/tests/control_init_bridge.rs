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
