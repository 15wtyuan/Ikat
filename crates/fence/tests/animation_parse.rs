//! fence `animation`/`transition` 简写从「只校验」变成「解析存值」。
//!
//! 解析产 `Vec<AnimationSpec>` / `Vec<TransitionSpec>`（core 类型），bake 进
//! `base_style.animation` / `base_style.transition`。语义：首个 time=duration、
//! 次个=delay；ease 按对齐表映射（CSS 标准关键字 → 精确 bezier，#9 起 cubic-bezier()
//! 函数形 + loom 超集 keyword 也收）。

use loomgui_core::style::resolved::{AnimationDirection, AnimationFillMode, AnimationPlayState};
use loomgui_core::tween::{Ease, TweenProp};
use loomgui_fence::css_resolve::resolve_inline_styles;
use loomgui_fence::schema::css::{
    parse_animation_value, parse_transition_value, validate_animation_value,
};
use loomgui_fence::tree_builder::parse_html_to_ir;

#[test]
fn animation_full_shorthand() {
    // 全部子属性 + 顺序无关关键字（CSS 语义：name 必须首位，其余任意序）。
    let specs = parse_animation_value("fadeIn .4s .1s infinite alternate both ease");
    assert_eq!(specs.len(), 1);
    let s = &specs[0];
    assert_eq!(s.name, "fadeIn");
    assert!((s.duration - 0.4).abs() < 1e-6, "duration=.4");
    assert!((s.delay - 0.1).abs() < 1e-6, "delay=.1");
    assert_eq!(s.iteration_count, None, "infinite → None");
    assert_eq!(s.direction, AnimationDirection::Alternate);
    assert_eq!(s.fill_mode, AnimationFillMode::Both);
    assert_eq!(
        s.timing_function,
        Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0
        },
        "ease → 精确 bezier(0.25,0.1,0.25,1)（CSS Easing L1）"
    );
    assert_eq!(s.play_state, AnimationPlayState::Running);
}

#[test]
fn animation_multi_declaration_comma() {
    // 逗号多声明 → 2 个 AnimationSpec。
    let specs = parse_animation_value("a .3s, b .5s infinite");
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].name, "a");
    assert!((specs[0].duration - 0.3).abs() < 1e-6);
    assert_eq!(
        specs[0].iteration_count,
        Some(1),
        "CSS 默认 iteration-count=1"
    );
    assert_eq!(specs[1].name, "b");
    assert!((specs[1].duration - 0.5).abs() < 1e-6);
    assert_eq!(specs[1].iteration_count, None, "infinite → None");
}

#[test]
fn animation_first_time_duration_second_delay() {
    // 首个 time = duration，次个 time = delay（不再都当匿名 time）。
    let s = &parse_animation_value("fadeIn .1s .4s")[0];
    assert!((s.duration - 0.1).abs() < 1e-6, "首个 time 是 duration");
    assert!((s.delay - 0.4).abs() < 1e-6, "次个 time 是 delay");
}

#[test]
fn animation_ease_keywords_exact_css() {
    // CSS 标准关键字 → 精确 bezier（CSS Easing Functions L1 定义值；早期 Quad/Cubic
    // 幂函数近似已在 #9 废除）。
    let b = |p: [f32; 4]| Ease::CubicBezier {
        x1: p[0],
        y1: p[1],
        x2: p[2],
        y2: p[3],
    };
    let cases = [
        ("linear", Ease::Linear),
        ("ease", b([0.25, 0.1, 0.25, 1.0])),
        ("ease-in", b([0.42, 0.0, 1.0, 1.0])),
        ("ease-out", b([0.0, 0.0, 0.58, 1.0])),
        ("ease-in-out", b([0.42, 0.0, 0.58, 1.0])),
        ("step-start", Ease::Step { start: true }),
        ("step-end", Ease::Step { start: false }),
        // loom 超集（游戏 UI 刚需；fence.md 登记）
        ("ease-in-back", Ease::BackIn),
        ("ease-out-elastic", Ease::ElasticOut),
        ("ease-in-out-bounce", Ease::BounceInOut),
    ];
    for (kw, want) in cases {
        let s = &parse_animation_value(&format!("fadeIn .4s {kw}"))[0];
        assert_eq!(s.timing_function, want, "timing keyword {kw}");
    }
}

#[test]
fn animation_cubic_bezier_functional_form() {
    // cubic-bezier(x1,y1,x2,y2)：x∈[0,1] 约束（y 可越界 overshoot）。
    let s = &parse_animation_value("fadeIn .4s cubic-bezier(.3,0,.7,1)")[0];
    assert_eq!(
        s.timing_function,
        Ease::CubicBezier {
            x1: 0.3,
            y1: 0.0,
            x2: 0.7,
            y2: 1.0
        }
    );
    // y 越界合法（overshoot 表达）
    let s = &parse_animation_value("fadeIn .4s cubic-bezier(.3,1.5,.7,-0.5)")[0];
    assert!(matches!(s.timing_function, Ease::CubicBezier { .. }));
    // x 越界拒：parse 侧未知 token 静默回落缺省（与默认值同形不可辨），真正的拦截在
    // validate 门（is_animation_keyword → parse_ease 判 false → 整条声明拒）。
    assert!(
        !validate_animation_value("fadeIn .4s cubic-bezier(-0.3,0,.7,1)"),
        "x 越界的 bezier 在 validate 门拒"
    );
    assert!(
        !validate_animation_value("fadeIn .4s cubic-bezier(.3,0)"),
        "参数个数错的 bezier 在 validate 门拒"
    );
    assert!(
        validate_animation_value("fadeIn .4s cubic-bezier(.3,0,.7,1)"),
        "合法 bezier 过门"
    );
}

#[test]
fn animation_iteration_count_integer() {
    let s = &parse_animation_value("fadeIn .4s 3")[0];
    assert_eq!(s.iteration_count, Some(3), "正整数 → Some(n)");
}

#[test]
fn animation_defaults_match_css_initial() {
    // 无关键字声明 → CSS initial：direction=normal / fill=none / play-state=running /
    // timing=ease（精确 bezier(0.25,0.1,0.25,1)；#9 前用 CubicOut 近似）/ iteration-count=1。
    let s = &parse_animation_value("fadeIn .4s")[0];
    assert_eq!(s.direction, AnimationDirection::Normal);
    assert_eq!(s.fill_mode, AnimationFillMode::None);
    assert_eq!(s.play_state, AnimationPlayState::Running);
    assert_eq!(
        s.timing_function,
        Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0
        },
        "CSS animation 默认 ease（精确 bezier）"
    );
    assert_eq!(s.iteration_count, Some(1));
    assert_eq!(s.delay, 0.0);
}

#[test]
fn animation_ms_and_play_state() {
    let s = &parse_animation_value("fadeIn 400ms paused")[0];
    assert!((s.duration - 0.4).abs() < 1e-6, "400ms → 0.4s");
    assert_eq!(s.play_state, AnimationPlayState::Paused);
}

#[test]
fn animation_none_is_empty() {
    assert!(parse_animation_value("none").is_empty(), "none = 无动画");
}

#[test]
fn animation_garbage_is_empty() {
    // 越界输入不 panic、不产 spec（围栏外由 validate 门拦，parse 只是防御）。
    assert!(parse_animation_value("").is_empty());
    assert!(parse_animation_value("123 2s").is_empty(), "数字开头 name");
}

#[test]
fn transition_prop_duration_ease_delay() {
    // prop + duration + ease + delay 全解析。
    let ts = parse_transition_value("opacity .3s ease .05s");
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].prop, Some(TweenProp::Opacity));
    assert!((ts[0].duration - 0.3).abs() < 1e-6);
    assert_eq!(
        ts[0].ease,
        Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        },
        "ease → 精确 bezier(0.25,0.1,0.25,1)"
    );
    assert!((ts[0].delay - 0.05).abs() < 1e-6);
}

#[test]
fn transition_all_is_none_prop() {
    // all → prop=None（任一通道变化触发）。
    let ts = parse_transition_value("all .2s linear");
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].prop, None);
    assert!((ts[0].duration - 0.2).abs() < 1e-6);
    assert_eq!(ts[0].ease, Ease::Linear);
    assert_eq!(ts[0].delay, 0.0);
}

#[test]
fn transition_color_prop_mapping() {
    // CSS 语义：color=文字色（TextColor 通道）、background-color=背景色（BgColor 通道）。
    let ts = parse_transition_value("color .3s");
    assert_eq!(ts[0].prop, Some(TweenProp::TextColor));
    let ts = parse_transition_value("background-color .3s");
    assert_eq!(ts[0].prop, Some(TweenProp::BgColor));
}

#[test]
fn transition_multi_declaration_comma() {
    let ts = parse_transition_value("opacity .3s, background-color .2s ease-in");
    assert_eq!(ts.len(), 2);
    assert_eq!(ts[0].prop, Some(TweenProp::Opacity));
    assert_eq!(ts[1].prop, Some(TweenProp::BgColor));
    assert_eq!(
        ts[1].ease,
        Ease::CubicBezier {
            x1: 0.42,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        },
        "ease-in → 精确 bezier(0.42,0,1,1)"
    );
}

#[test]
fn transition_missing_prop_defaults_to_all() {
    // 缺 prop → None（all）。
    let ts = parse_transition_value(".3s ease");
    assert_eq!(ts[0].prop, None);
    assert!((ts[0].duration - 0.3).abs() < 1e-6);
}

#[test]
fn transition_defaults_match_css_initial() {
    // CSS transition-timing-function 初始值 = ease（精确 bezier；#9 前用 CubicOut 近似）。
    let ts = parse_transition_value("opacity .3s");
    assert_eq!(
        ts[0].ease,
        Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0
        }
    );
    assert_eq!(ts[0].delay, 0.0);
}

#[test]
fn transition_ms_duration() {
    let ts = parse_transition_value("opacity 400ms");
    assert!((ts[0].duration - 0.4).abs() < 1e-6, "400ms → 0.4s");
}

#[test]
fn transition_garbage_is_noop_spec() {
    // 零校验宽松语义（保持现状：transition 值不报错）——未知 token 忽略，产默认 spec。
    let ts = parse_transition_value("bogus .3s");
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].prop, None);
    assert!((ts[0].duration - 0.3).abs() < 1e-6);
    assert!(parse_transition_value("").is_empty(), "空值 → 空 Vec");
}

#[test]
fn inline_animation_bakes_into_base_style() {
    // css_resolve 的 Animation arm：合法值存进 styles[idx].animation（不再 continue 丢弃）。
    let (tree, _) = parse_html_to_ir(r#"<div style="animation: fadeIn .4s both"></div>"#);
    let styles = resolve_inline_styles(&tree);
    let id = tree.roots[0];
    let anim = &styles[id.0].animation;
    assert_eq!(anim.len(), 1, "animation 应解析存值");
    assert_eq!(anim[0].name, "fadeIn");
    assert_eq!(anim[0].fill_mode, AnimationFillMode::Both);
}

#[test]
fn inline_transition_bakes_into_base_style() {
    // css_resolve 的 Transition arm：合法值存进 styles[idx].transition。
    let (tree, _) = parse_html_to_ir(r#"<div style="transition: opacity .3s ease .05s"></div>"#);
    let styles = resolve_inline_styles(&tree);
    let id = tree.roots[0];
    let ts = &styles[id.0].transition;
    assert_eq!(ts.len(), 1, "transition 应解析存值");
    assert_eq!(ts[0].prop, Some(TweenProp::Opacity));
    assert_eq!(
        ts[0].ease,
        Ease::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        }
    );
}
