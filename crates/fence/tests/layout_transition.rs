//! #10 layout 动画围栏面：
//! - keyframes 停靠点端点校验（auto 拒 / 异域拒 / 同域过）
//! - transition 元素级端点扫描（静态可见端点同域显式；伪类变体进池）
//! - transition 白名单扩展（width/height/flex-grow/box-shadow 不再警告）

use loomgui_fence::diagnostic::Severity;
use loomgui_fence::parse_template;

fn errors(html: &str) -> Vec<String> {
    let parsed = parse_template(html, "test.html");
    parsed
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

fn warnings(html: &str) -> Vec<String> {
    let parsed = parse_template(html, "test.html");
    parsed
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.message.clone())
        .collect()
}

// —— @keyframes 停靠点端点 ——

#[test]
fn keyframes_width_auto_endpoint_rejected() {
    let errs = errors(
        "<div style=\"width:100px\"></div><style>@keyframes grow { from { width:0px } to { width:auto } }</style>",
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("width") && m.contains("auto")),
        "auto 端点应 error: {errs:?}"
    );
}

#[test]
fn keyframes_width_cross_domain_rejected() {
    let errs = errors(
        "<div style=\"width:100px\"></div><style>@keyframes grow { from { width:0px } to { width:50% } }</style>",
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("domains") || m.contains("ONE domain")),
        "异域混合应 error: {errs:?}"
    );
}

#[test]
fn keyframes_width_same_domain_passes() {
    let errs = errors(
        "<div style=\"width:100px\"></div><style>@keyframes grow { from { width:0px } to { width:200px } }</style>",
    );
    assert!(
        !errs.iter().any(|m| m.contains("width")),
        "px↔px 同域应放行: {errs:?}"
    );
}

// —— transition 元素级端点扫描 ——

#[test]
fn transition_height_px_pair_passes() {
    // base 100px + hover 0px：同域 → 无 error。
    let errs = errors(
        "<div class=\"p\" style=\"transition:height .3s\"></div><style>.p { height:100px } .p:hover { height:0px }</style>",
    );
    assert!(
        !errs.iter().any(|m| m.contains("height")),
        "同域 height 端点放行: {errs:?}"
    );
}

#[test]
fn transition_height_auto_endpoint_rejected() {
    // hover 端 auto（伪类变体在扫描池——伪类是运行时开关，规则结构上作用到该元素；
    // 对照：`.p.open` 这类「运行时 add_class 组合」静态结构不匹配，归 core snap 兜底）。
    let errs = errors(
        "<div class=\"p\" style=\"transition:height .3s\"></div><style>.p { height:100px } .p:hover { height:auto }</style>",
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("height") && m.contains("auto")),
        "auto 伪类端点应 error: {errs:?}"
    );
}

#[test]
fn transition_width_cross_domain_rejected() {
    let errs = errors(
        "<div class=\"p\" style=\"transition:width .3s\"></div><style>.p { width:50vw } .p:hover { width:80% }</style>",
    );
    assert!(
        errs.iter()
            .any(|m| m.contains("ONE domain") || m.contains("domains differ")),
        "vw↔% 异域应 error: {errs:?}"
    );
}

#[test]
fn transition_no_layout_prop_not_scanned() {
    // transition 只报 opacity（非 layout 通道）→ 不触发端点扫描（width 域可任意）。
    let errs = errors(
        "<div class=\"p\" style=\"transition:opacity .3s\"></div><style>.p { width:50vw } .p:hover { width:80% }</style>",
    );
    assert!(
        !errs.iter().any(|m| m.contains("domain")),
        "非 layout 通道不扫描端点: {errs:?}"
    );
}

#[test]
fn transition_all_covers_layout_channels() {
    // transition:all 覆盖 width/height → 端点进扫描池。
    let errs = errors(
        "<div class=\"p\" style=\"transition:all .3s\"></div><style>.p { height:100px } .p:hover { height:auto }</style>",
    );
    assert!(
        errs.iter().any(|m| m.contains("auto")),
        "all 覆盖 height → auto 端点应 error: {errs:?}"
    );
}

// —— 白名单扩展（warning 消失）——

#[test]
fn transition_layout_props_no_longer_warned() {
    let warns =
        warnings("<div style=\"transition:width .3s, box-shadow .2s, flex-grow .4s\"></div>");
    assert!(
        warns
            .iter()
            .all(|m| !m.contains("has no runtime transition")),
        "width/box-shadow/flex-grow 已是支持通道，不再警告: {warns:?}"
    );
}

#[test]
fn transition_unknown_prop_still_warned() {
    let warns = warnings("<div style=\"transition:margin .3s\"></div>");
    assert!(
        warns
            .iter()
            .any(|m| m.contains("has no runtime transition")),
        "margin 仍域外: {warns:?}"
    );
}
