//! 控件必须被 CSS 命中校验（端到端）。
//!
//! LoomGUI 控件（role 驱动：progressbar/slider/switch/radio/textbox/...）不带 UA
//! 默认样式——写了控件却没匹配的 CSS 规则 = 运行时空白。本测试覆盖打包期校验：
//! 有匹配 CSS → 静默通过；完全无 CSS 命中 → `FenceControlWithoutCss` error + 教学。

use loomgui_fence::diagnostic::{DiagnosticCode, Severity};
use loomgui_fence::pipeline::parse_template;

/// 判定是否含「控件缺 CSS」诊断（按 code + message 含控件可读名片段）。
fn has_control_css_diag(result: &loomgui_fence::pipeline::ParsedTemplate, needle: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::FenceControlWithoutCss && d.message.contains(needle))
}

// ── ProgressBar (role=progressbar) ──

/// 裸 `role=progressbar` 无 CSS → error（控件空白）。
#[test]
fn progressbar_without_css_errors() {
    let html = r#"<div role="progressbar" aria-valuenow="70" aria-valuemax="100"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "progress bar"),
        "裸 progressbar 无 CSS 应报错: {:?}",
        result.diagnostics
    );
    // 必须是 error 级（空白控件是破坏性 bug，应阻断打包）
    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceControlWithoutCss
                && d.severity == Severity::Error
                && d.message.contains("progress bar")
        }),
        "应为 Error 级: {:?}",
        result.diagnostics
    );
}

/// `role=progressbar` + 属性选择器 CSS → 放行（控件已被命中）。
#[test]
fn progressbar_with_css_passes() {
    let html = r#"<style>[role="progressbar"]{background:#ddd}</style><div role="progressbar" aria-valuenow="70"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress bar"),
        "progressbar + CSS 不应报错: {:?}",
        result.diagnostics
    );
}

/// 教学文案必须自包含 + 可操作（含 data-slot 引导）。
#[test]
fn progressbar_without_css_message_is_actionable() {
    let html = r#"<div role="progressbar" aria-valuenow="70"></div>"#;
    let result = parse_template(html, "t.html");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::FenceControlWithoutCss)
        .expect("should emit control-css diagnostic");
    assert!(d.message.contains("progress bar"), "msg 应含控件名");
    assert!(d.message.contains("CSS"), "msg 应提 CSS");
    assert!(
        d.message.contains("data-slot=\"fill\""),
        "msg 应引导 data-slot=fill: {}",
        d.message
    );
    assert!(
        !d.message.contains(".loom-"),
        "msg 不应再引用已删除的 .loom-* 注入: {}",
        d.message
    );
}

// ── Slider (role=slider) ──

/// 裸 `role=slider` 无 CSS → error。
#[test]
fn slider_without_css_errors() {
    let html = r#"<div role="slider" aria-valuenow="50"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "slider"),
        "裸 slider 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `role=slider` + class 选择器 CSS → 放行。
#[test]
fn slider_with_css_passes() {
    let html = r#"<style>.vol { display:block; background:#ddd }</style><div role="slider" aria-valuenow="50" class="vol"><div data-slot="thumb"></div></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "slider"),
        "slider + class CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── Toggle (role=switch) ──

/// 裸 `role=switch` 无 CSS → error。
#[test]
fn toggle_without_css_errors() {
    let html = r#"<div role="switch" aria-checked="true"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "toggle"),
        "裸 switch 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `role=switch` + CSS 命中 → 放行。
#[test]
fn toggle_with_css_passes() {
    let html = r#"<style>[role="switch"]{width:24px;height:24px}</style><div role="switch" aria-checked="true"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "toggle"),
        "switch + 属性选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── RadioButton (role=radio) ──

/// 裸 `role=radio` 无 CSS → error。
#[test]
fn radio_without_css_errors() {
    let html = r#"<div role="radio" aria-checked="true" data-name="grp"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "radio"),
        "裸 radio 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `role=radio` + class 选择器 CSS → 放行。
#[test]
fn radio_with_css_passes() {
    let html = r#"<style>.opt{display:block;width:20px;height:20px}</style><div role="radio" aria-checked="true" data-name="grp" class="opt"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "radio"),
        "radio + class CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── 选择器形态覆盖 ──

/// 后代选择器命中控件也算（`.bar [role="progressbar"]`）。
#[test]
fn descendant_selector_on_control_counts() {
    let html = r#"<style>.bar [role="progressbar"]{background:#ddd}</style><div class="bar"><div role="progressbar" aria-valuenow="1"></div></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress bar"),
        "后代选择器命中控件应放行: {:?}",
        result.diagnostics
    );
}

/// id 选择器命中控件也算（`#hp`）。
#[test]
fn id_selector_on_control_counts() {
    let html = r#"<style>#hp{background:#ddd}</style><div role="progressbar" aria-valuenow="1" id="hp"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress bar"),
        "id 选择器命中控件应放行: {:?}",
        result.diagnostics
    );
}

/// 非控件元素（div）无 CSS 不报（只管控件）。
#[test]
fn non_control_no_css_not_flagged() {
    let html = r#"<div>hi</div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceControlWithoutCss),
        "div 无 CSS 不应报控件校验: {:?}",
        result.diagnostics
    );
}

/// 同一控件被多条规则命中（含 :hover 伪类）→ 仍放行（用户在样式控件）。
#[test]
fn pseudo_class_rule_still_counts() {
    let html = r#"<style>[role="progressbar"]{background:#ddd} [role="progressbar"]:hover{background:#fff}</style><div role="progressbar" aria-valuenow="1"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress bar"),
        "[role=progressbar]{{}} 已命中，:hover 规则不影响: {:?}",
        result.diagnostics
    );
}

// ── TextField / TextArea (role=textbox) ──
//
// 文本输入控件同样不带 UA 默认样式：浏览器给 textbox 套自带外观
// （边框/底色/光标），但 LoomGUI core 无 UA 表——打包后运行时空白。

/// 裸 `role=textbox` 无 CSS → error。
#[test]
fn text_input_without_css_errors() {
    let html = r#"<div role="textbox"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "text field"),
        "裸 textbox 无 CSS 应报错: {:?}",
        result.diagnostics
    );
    // 必须是 error 级（空白文本框是破坏性 bug，应阻断打包）
    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceControlWithoutCss
                && d.severity == Severity::Error
                && d.message.contains("text field")
                && d.message.contains("CSS")
        }),
        "应为 Error 级且 message 含 text field/CSS: {:?}",
        result.diagnostics
    );
}

/// `role=textbox` + 属性选择器 CSS → 放行。
#[test]
fn text_input_with_css_passes() {
    let html = r#"<style>[role="textbox"]{background:#fff;border:1px solid #888;caret-color:#000}</style><div role="textbox"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "text field"),
        "textbox + 属性选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}

/// 教学文案：文本框应引导 background/border + caret-color（输入光标可见）。
#[test]
fn text_input_without_css_message_suggests_caret_color() {
    let html = r#"<div role="textbox"></div>"#;
    let result = parse_template(html, "t.html");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::FenceControlWithoutCss)
        .expect("should emit control-css diagnostic");
    assert!(d.message.contains("text field"), "msg 应含控件名");
    assert!(d.message.contains("CSS"), "msg 应提 CSS");
    assert!(
        d.message.contains("caret-color"),
        "msg 应建议 caret-color: {}",
        d.message
    );
}

/// `role=textbox` + aria-multiline → text area（仍走同一 textbox 校验路径）。
#[test]
fn textarea_without_css_errors() {
    let html = r#"<div role="textbox" aria-multiline="true"></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "text field"),
        "裸 aria-multiline textbox 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}
