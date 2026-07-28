//! 控件必须被 CSS 命中校验（端到端）。
//!
//! LoomGUI 控件（ProgressBar/Slider/Toggle/RadioButton）不带 UA 默认样式——
//! 写了控件标签却没匹配的 CSS 规则 = 运行时空白。本测试覆盖打包期校验：
//! 有匹配 CSS → 静默通过；完全无 CSS 命中 → `FenceControlWithoutCss` error + 教学。

use loomgui_fence::diagnostic::{DiagnosticCode, Severity};
use loomgui_fence::pipeline::parse_template;

/// 判定是否含「控件缺 CSS」诊断（按 code + message 含控件标签名）。
fn has_control_css_diag(result: &loomgui_fence::pipeline::ParsedTemplate, tag: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::FenceControlWithoutCss && d.message.contains(tag))
}

// ── ProgressBar ──

/// 裸 `<progress>` 无 CSS → error（控件空白）。
#[test]
fn progress_without_css_errors() {
    let html = r#"<progress value="70" max="100"></progress>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "progress"),
        "裸 progress 无 CSS 应报错: {:?}",
        result.diagnostics
    );
    // 必须是 error 级（空白控件是破坏性 bug，应阻断打包）
    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceControlWithoutCss
                && d.severity == Severity::Error
                && d.message.contains("progress")
        }),
        "应为 Error 级: {:?}",
        result.diagnostics
    );
}

/// `<progress>` + tag 选择器 CSS → 放行（控件已被命中）。
#[test]
fn progress_with_css_passes() {
    let html = r#"<style>progress{background:#ddd} .loom-fill{background:#4a9}</style><progress value="70"></progress>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress"),
        "progress + tag 选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}

/// 教学文案必须自包含 + 可操作（含控件标签 + CSS 提示 + loom-fill 引导）。
#[test]
fn progress_without_css_message_is_actionable() {
    let html = r#"<progress value="70"></progress>"#;
    let result = parse_template(html, "t.html");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::FenceControlWithoutCss)
        .expect("should emit control-css diagnostic");
    assert!(d.message.contains("progress"), "msg 应含标签名");
    assert!(d.message.contains("CSS"), "msg 应提 CSS");
    // 引导作者为 loom-* 内部子节点提供样式（控件内部视觉靠框架注入的 .loom-* class）
    assert!(
        d.message.contains("loom"),
        "msg 应引导 loom-* 子节点样式: {}",
        d.message
    );
}

// ── Slider (input[type=range]) ──

/// 裸 `<input type="range">` 无 CSS → error。
#[test]
fn slider_without_css_errors() {
    let html = r#"<input type="range" value="50">"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 range 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="range">` + class 选择器 CSS → 放行。
#[test]
fn slider_with_css_passes() {
    let html = r#"<style>.vol { display:block; background:#ddd } input[type="range"]{width:200px}</style><input type="range" class="vol" value="50">"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "range + class CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── Toggle (checkbox) ──

/// 裸 `<input type="checkbox">` 无 CSS → error。
#[test]
fn toggle_without_css_errors() {
    let html = r#"<input type="checkbox" checked>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 checkbox 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="checkbox">` + CSS 命中 → 放行。
#[test]
fn toggle_with_css_passes() {
    let html = r#"<style>input[type="checkbox"]{width:24px;height:24px}</style><input type="checkbox" checked>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "checkbox + 属性选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── RadioButton ──

/// 裸 `<input type="radio">` 无 CSS → error。
#[test]
fn radio_without_css_errors() {
    let html = r#"<input type="radio" name="grp">"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 radio 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="radio">` + class 选择器 CSS → 放行。
#[test]
fn radio_with_css_passes() {
    let html = r#"<style>.opt{display:block;width:20px;height:20px}</style><input type="radio" name="grp" class="opt">"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "radio + class CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── 选择器形态覆盖 ──

/// 后代选择器命中控件也算（`.bar progress`）。
#[test]
fn descendant_selector_on_control_counts() {
    let html = r#"<style>.bar progress{background:#ddd}</style><div class="bar"><progress value="1"></progress></div>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress"),
        "后代选择器命中控件应放行: {:?}",
        result.diagnostics
    );
}

/// id 选择器命中控件也算（`#hp`）。
#[test]
fn id_selector_on_control_counts() {
    let html = r#"<style>#hp{background:#ddd}</style><progress id="hp" value="1"></progress>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress"),
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

/// 同一 progress 被多条规则命中（含 :hover 伪类）→ 仍放行（用户在样式控件）。
#[test]
fn pseudo_class_rule_still_counts() {
    let html = r#"<style>progress{background:#ddd} progress:hover{background:#fff}</style><progress value="1"></progress>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "progress"),
        "progress{{}} 已命中，:hover 规则不影响: {:?}",
        result.diagnostics
    );
}

// ── TextField (input[type=text] / bare input) ──
//
// 文本输入控件同样不带 UA 默认样式：浏览器给 <input>/textarea 套自带外观
// （边框/底色/光标），但 LoomGUI core 无 UA 表——打包后运行时空白。本组覆盖
// Stage 6.7 校验扩到文本控件后的行为（Task 17）。

/// 裸 `<input type="text">` 无 CSS → error。
#[test]
fn text_input_without_css_errors() {
    let html = r#"<input type="text" value="x">"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 text input 无 CSS 应报错: {:?}",
        result.diagnostics
    );
    // 必须是 error 级（空白文本框是破坏性 bug，应阻断打包）
    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceControlWithoutCss
                && d.severity == Severity::Error
                && d.message.contains("input")
                && d.message.contains("CSS")
        }),
        "应为 Error 级且 message 含 input/CSS: {:?}",
        result.diagnostics
    );
}

/// 裸 `<input>`（默认 type=text）无 CSS → error。
#[test]
fn bare_input_without_css_errors() {
    let html = r#"<input value="x">"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 input（默认 text）无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="text">` + tag 选择器 CSS → 放行。
#[test]
fn text_input_with_css_passes() {
    let html = r#"<style>input{background:#fff;border:1px solid #888;caret-color:#000}</style><input type="text" value="x">"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "text input + tag 选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="text">` + 属性选择器 CSS → 放行（属性选择器也算命中）。
#[test]
fn text_input_with_attr_selector_passes() {
    let html = r#"<style>input[type="text"]{background:#fff}</style><input type="text" value="x">"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "text input + 属性选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}

/// 教学文案：文本框无 .loom-* 子节点，应引导 background/border + caret-color
/// （而非 progress/slider 的 loom-fill/loom-thumb）。
#[test]
fn text_input_without_css_message_suggests_caret_color() {
    let html = r#"<input type="text" value="x">"#;
    let result = parse_template(html, "t.html");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::FenceControlWithoutCss)
        .expect("should emit control-css diagnostic");
    assert!(d.message.contains("input"), "msg 应含标签名");
    assert!(d.message.contains("CSS"), "msg 应提 CSS");
    // 文本框靠 caret-color 可见（输入光标），框架不注入 loom-* 子节点
    assert!(
        d.message.contains("caret-color"),
        "msg 应建议 caret-color: {}",
        d.message
    );
}

// ── PasswordField (input[type=password]) ──

/// 裸 `<input type="password">` 无 CSS → error。
#[test]
fn password_without_css_errors() {
    let html = r#"<input type="password">"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 password input 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="password">` + CSS 命中 → 放行。
#[test]
fn password_with_css_passes() {
    let html = r#"<style>input[type="password"]{background:#fff}</style><input type="password">"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "password + CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── SearchField (input[type=search]) ──

/// 裸 `<input type="search">` 无 CSS → error。
#[test]
fn search_without_css_errors() {
    let html = r#"<input type="search">"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "input"),
        "裸 search input 无 CSS 应报错: {:?}",
        result.diagnostics
    );
}

/// `<input type="search">` + CSS 命中 → 放行。
#[test]
fn search_with_css_passes() {
    let html = r#"<style>input[type="search"]{background:#fff}</style><input type="search">"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "input"),
        "search + CSS 不应报错: {:?}",
        result.diagnostics
    );
}

// ── TextArea (textarea) ──

/// 裸 `<textarea>` 无 CSS → error。
#[test]
fn textarea_without_css_errors() {
    let html = r#"<textarea></textarea>"#;
    let result = parse_template(html, "t.html");
    assert!(
        has_control_css_diag(&result, "textarea"),
        "裸 textarea 无 CSS 应报错: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::FenceControlWithoutCss
                && d.severity == Severity::Error
                && d.message.contains("textarea")
        }),
        "应为 Error 级: {:?}",
        result.diagnostics
    );
}

/// `<textarea>` + tag 选择器 CSS → 放行。
#[test]
fn textarea_with_css_passes() {
    let html = r#"<style>textarea{background:#fff}</style><textarea></textarea>"#;
    let result = parse_template(html, "t.html");
    assert!(
        !has_control_css_diag(&result, "textarea"),
        "textarea + tag 选择器 CSS 不应报错: {:?}",
        result.diagnostics
    );
}
