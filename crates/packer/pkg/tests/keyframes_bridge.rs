//! fence @keyframes → bridge → pkg.bin 往返契约测试（@loom-hook 锚点 + transform TRS 分解）。
//!
//! `ComponentTemplate.keyframes: Vec<KeyframesRule>`（pkg v30 序列化）落地但 bridge 曾
//! 静默丢弃 keyframes；本测试锁死「不再丢弃」：HTML 里的 @keyframes 必须进 pkg，且
//! `/* @loom-hook name */` 注释锚点挂在前一个 stop 上、stop 的 transform 按 TRS 分解
//! （translateY(20px) → translate [0,20]，不做矩阵合并）。

use loomgui_core::asset::read_package;
use loomgui_core::scene::animation::{AnimatableProps, KeyframeStopSelector, TransformAnim};
use loomgui_pkg::build::{pack_components, Component};

/// 单组件 HTML：@keyframes slideIn（from 带 hook 注释 + translateY，to 为终态）
/// + .card 挂 animation 简写（断言 base_style.animation 与 keyframes 名字连通）。
const HTML: &str = r#"<style>
@keyframes slideIn{from{opacity:0;transform:translateY(20px)}/* @loom-hook start */ to{opacity:1;transform:none}}
.card{animation:slideIn .4s both}
</style>
<div class="card">slide</div>"#;

#[test]
fn keyframes_survive_roundtrip_with_hook_and_trs() {
    let comps = vec![Component {
        name: "home".to_string(),
        src: HTML.to_string(),
        html_rel: "home.html".to_string(),
    }];
    let bytes = pack_components(&comps).unwrap().bytes;
    let pkg = read_package(&bytes).unwrap();
    let comp = pkg.components.get("home").expect("home component");

    // ① keyframes 不再被 bridge 丢弃：rule 名 + stop 结构完整进 pkg。
    assert_eq!(
        comp.keyframes.len(),
        1,
        "keyframes 不应为空（bridge 丢弃回归）"
    );
    let kf = &comp.keyframes[0];
    assert_eq!(kf.name, "slideIn");
    assert_eq!(kf.stops.len(), 2);
    assert_eq!(kf.stops[0].selector, KeyframeStopSelector::From);
    assert_eq!(kf.stops[1].selector, KeyframeStopSelector::To);

    // ② @loom-hook 锚点：注释写在 from 块后、to 前 → 挂在前一个 stop（from）上。
    assert_eq!(
        kf.stops[0].hook.as_deref(),
        Some("start"),
        "hook 应挂在 from stop（前一个 stop）"
    );
    assert_eq!(kf.stops[1].hook, None, "to stop 无锚点注释");

    // ③ transform TRS 分解：translateY(20px) → translate [0,20]，scale/rotate None。
    assert_eq!(
        kf.stops[0].props,
        AnimatableProps {
            opacity: Some(0.0),
            transform: Some(TransformAnim {
                translate: Some([0.0, 20.0]),
                scale: None,
                rotate: None,
            }),
            bg_color: None,
            text_color: None,
        },
        "from stop 的 props（opacity + translateY TRS 分解）"
    );
    // to: opacity 1 + transform:none → 空 TRS（各分量 None = 不参与插值）。
    assert_eq!(kf.stops[1].props.opacity, Some(1.0));
    assert_eq!(
        kf.stops[1].props.transform,
        Some(TransformAnim::default()),
        "transform:none 应为空 TRS 而非 None"
    );

    // ④ animation 简写与 keyframes 表同名连通。class 规则属于运行时动态级联表，
    // 因而在 pkg 中检查其声明已被保留；bridge 不把 animation 规则静默丢掉。
    assert!(comp.dynamic_rules.rules.iter().any(|rule| {
        rule.selector.raw == ".card"
            && rule
                .declarations
                .iter()
                .any(|decl| decl.prop == "animation" && decl.value == "slideIn .4s both")
    }));
}
