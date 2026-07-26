//! bridge 提取控件 HTML 属性 → ControlInit 的契约测试。
//!
//! bridge() 在 IrTree→TemplateNode 翻译时，按 NodeKind 从 HTML 属性提取控件初始值
//! （value/max/min/step/checked/name），填进 TemplateNode.control_init，使其随 pkg.bin
//! 存活到运行时 instantiate。此文件覆盖四种控件 + ProgressBar 无 value 的 indeterminate 语义。

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
        r#"<style>progress,input[type="range"],input[type="checkbox"],input[type="radio"]{{background:#ddd}}</style>{html}"#
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
