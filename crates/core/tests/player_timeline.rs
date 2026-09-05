//! KeyframePlayer 时间轴推进单测（spec §5.3 纯函数：无副作用，只算"当前应取的属性值 + 状态"）。
//!
//! 覆盖：backwards fill / ease 插值 / 完成判定 / fill 四态 / delay / direction 四态 /
//! iteration 边界 / TRS 分量级 lerp / color lerp / Step ease / 0 时长 / Paused 不推进。
//!
//! 全部确定性：固定 dt 累计推进，断言取值（spec §9.2 确定性断言策略）。

use yio_core::scene::{
    AnimatableProps, KeyframePlayer, KeyframeStop, KeyframeStopSelector, KeyframesRule, NodeId,
    PlayerFrame, PlayerPlayState, TransformAnim,
};
use yio_core::style::resolved::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
};
use yio_core::transform::LenPct;
use yio_core::tween::Ease;

fn assert_close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "expected {b} ± 1e-4, got {a}");
}

/// 默认 spec：fadeIn .4s 单次迭代 Normal None Linear Running。
fn spec() -> AnimationSpec {
    AnimationSpec {
        name: "fadeIn".into(),
        duration: 0.4,
        delay: 0.0,
        iteration_count: Some(1),
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        timing_function: Ease::Linear,
        play_state: AnimationPlayState::Running,
    }
}

/// 2-stop opacity 0→1 keyframes。
fn opacity_fade() -> KeyframesRule {
    KeyframesRule {
        name: "fadeIn".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(0.0),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    ..Default::default()
                },
            ),
        ],
    }
}

fn stop(selector: KeyframeStopSelector, props: AnimatableProps) -> KeyframeStop {
    KeyframeStop {
        selector,
        props,
        timing: None,
        hook: None,
    }
}

fn scale(v: [f32; 2]) -> AnimatableProps {
    AnimatableProps {
        transform: Some(TransformAnim {
            translate: None,
            scale: Some(v),
            rotate: None,
        }),
        ..Default::default()
    }
}

fn player(spec: AnimationSpec, keyframes: KeyframesRule) -> KeyframePlayer {
    KeyframePlayer::new(NodeId(0), spec, keyframes)
}

/// 测 1：首帧（dt=0）backwards fill 显 from 值。
#[test]
fn fade_in_first_frame_backwards_fill() {
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    s.timing_function = Ease::CubicOut;
    let mut p = player(s, opacity_fade());
    let f = p.advance(0.0);
    assert_close(f.props.opacity.expect("opacity override"), 0.0);
    assert!(!f.completed);
    assert_eq!(f.iteration_boundary, None);
    assert_eq!(f.play_state, PlayerPlayState::Playing);
}

/// 测 2：累计 0.2s → progress=0.5 → opacity=CubicOut(0.5)=0.875（2-stop 段=整段，ease 作用于段内）。
#[test]
fn fade_in_midpoint_cubic_out() {
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    s.timing_function = Ease::CubicOut;
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    let f = p.advance(0.2);
    assert_close(f.props.opacity.unwrap(), 0.875);
    assert!(!f.completed);
}

/// 测 3：累计 0.4s → opacity=1.0（fill both 保留末值），completed=true + 迭代边界。
#[test]
fn fade_in_completes_with_end_value() {
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    s.timing_function = Ease::CubicOut;
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    p.advance(0.2);
    let f = p.advance(0.2);
    assert_close(f.props.opacity.unwrap(), 1.0);
    assert!(f.completed);
    assert_eq!(f.play_state, PlayerPlayState::Completed);
    assert_eq!(f.iteration_boundary, Some(0), "末次迭代 0 本帧结束");
}

/// 测 4：infinite pulse scale 1↔1.1 .4s alternate（3-stop 三角）：
/// 0s→1.0 / .2s(iter0 p.5)→1.1 / .4s(iter1 p0→directed 1)→1.0 / .6s(iter1 p.5)→1.1。
#[test]
fn pulse_scale_alternate_infinite() {
    let mut s = spec();
    s.name = "pulse".into();
    s.iteration_count = None;
    s.direction = AnimationDirection::Alternate;
    let kf = KeyframesRule {
        name: "pulse".into(),
        stops: vec![
            stop(KeyframeStopSelector::From, scale([1.0, 1.0])),
            stop(KeyframeStopSelector::Percent(50), scale([1.1, 1.1])),
            stop(KeyframeStopSelector::To, scale([1.0, 1.0])),
        ],
    };
    let mut p = player(s, kf);
    let f0 = p.advance(0.0);
    let f1 = p.advance(0.2);
    let f2 = p.advance(0.2);
    let f3 = p.advance(0.2);
    for f in [&f0, &f1, &f2, &f3] {
        assert!(!f.completed, "infinite 永不完成");
    }
    assert_eq!(f0.props.transform.unwrap().scale.unwrap(), [1.0, 1.0]);
    assert_eq!(f1.props.transform.unwrap().scale.unwrap(), [1.1, 1.1]);
    assert_eq!(f2.props.transform.unwrap().scale.unwrap(), [1.0, 1.0]);
    assert_eq!(f3.props.transform.unwrap().scale.unwrap(), [1.1, 1.1]);
}

/// 测 5：reverse：directed = 1 - progress。t=0 显末值，t=.2 中点。
#[test]
fn reverse_mirrors_progress() {
    let mut s = spec();
    s.direction = AnimationDirection::Reverse;
    let mut p = player(s, opacity_fade());
    let f0 = p.advance(0.0);
    let f1 = p.advance(0.2);
    assert_close(f0.props.opacity.unwrap(), 1.0);
    assert_close(f1.props.opacity.unwrap(), 0.5);
    assert_close(p.last_progress, 0.5);
}

/// alternate-reverse：iter0 反向（directed=1-p），iter1 正向（directed=p）。
#[test]
fn alternate_reverse_starts_from_end() {
    let mut s = spec();
    s.iteration_count = None;
    s.direction = AnimationDirection::AlternateReverse;
    let mut p = player(s, opacity_fade());
    let f0 = p.advance(0.0); // iter0 p0 → directed 1
    let f1 = p.advance(0.2); // iter0 p.5 → directed .5
    let f2 = p.advance(0.2); // iter1 p0 → directed 0
    let f3 = p.advance(0.2); // iter1 p.5 → directed .5
    assert_close(f0.props.opacity.unwrap(), 1.0);
    assert_close(f1.props.opacity.unwrap(), 0.5);
    assert_close(f2.props.opacity.unwrap(), 0.0);
    assert_close(f3.props.opacity.unwrap(), 0.5);
}

/// 测 6：delay 0.1s fill both：elapsed<delay 显首帧；越过 delay 后 progress=(elapsed-delay)/duration。
#[test]
fn delay_backwards_fill_then_play() {
    let mut s = spec();
    s.delay = 0.1;
    s.fill_mode = AnimationFillMode::Both;
    let mut p = player(s, opacity_fade());
    let f0 = p.advance(0.05);
    let f1 = p.advance(0.1); // 累计 0.15 → anim_time .05 → progress .125
    assert_close(f0.props.opacity.unwrap(), 0.0);
    assert!(!f0.completed);
    assert_close(f1.props.opacity.unwrap(), 0.125);
}

/// fill none（默认）：delay 期间无 override（通道 None，回退 base）。
#[test]
fn delay_fill_none_no_override_during_delay() {
    let mut s = spec();
    s.delay = 0.1;
    let mut p = player(s, opacity_fade());
    let f = p.advance(0.05);
    assert_eq!(
        f.props,
        AnimatableProps::default(),
        "fill none 延迟期不写通道"
    );
}

/// 测 7：Step ease：progress .5 → Step{start}=1.0（跳变在段起点）、Step{end}=0.0（段末跳变）。
#[test]
fn step_ease_jumps_at_segment_boundary() {
    for (ease, expected) in [
        (Ease::Step { start: true }, 1.0),
        (Ease::Step { start: false }, 0.0),
    ] {
        let mut s = spec();
        s.timing_function = ease;
        let mut p = player(s, opacity_fade());
        let f = p.advance(0.2); // progress .5 → segment local_t .5
        assert_close(f.props.opacity.unwrap(), expected);
    }
}

/// fill none：完成帧即回退 base（props 全 None），completed=true。
#[test]
fn fill_none_completion_returns_no_override() {
    let s = spec();
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    p.advance(0.2);
    let f = p.advance(0.2);
    assert!(f.completed);
    assert_eq!(f.props, AnimatableProps::default());
    assert_eq!(f.play_state, PlayerPlayState::Completed);
}

/// fill forwards：完成后每帧持续写末值（不回收），且不再报迭代边界。
#[test]
fn completed_forwards_keeps_end_value() {
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Forwards;
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    p.advance(0.2);
    let f1 = p.advance(0.2);
    let f2 = p.advance(0.2);
    assert!(f1.completed);
    assert!(f2.completed);
    assert_close(f1.props.opacity.unwrap(), 1.0);
    assert_close(f2.props.opacity.unwrap(), 1.0);
    assert_eq!(f2.iteration_boundary, None, "完成后不再报迭代边界");
}

/// Paused：跳过推进（elapsed 不变），返回当前时刻帧。
#[test]
fn paused_does_not_advance() {
    let s = spec();
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    let before = p.advance(0.2);
    p.play_state = PlayerPlayState::Paused;
    let during = p.advance(0.2);
    assert_eq!(before.props, during.props, "暂停期间取值不变");
    assert!(!during.completed);
    p.play_state = PlayerPlayState::Playing;
    let after = p.advance(0.2); // elapsed 仍是 0.2 → 累计 0.4 完成
    assert!(after.completed);
}

/// Stopped：同 Paused 不推进；恢复 Playing 从原位置继续。
#[test]
fn stopped_does_not_advance() {
    let s = spec();
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    p.play_state = PlayerPlayState::Stopped;
    let f = p.advance(0.2);
    assert!(!f.completed);
    assert_eq!(f.play_state, PlayerPlayState::Stopped);
    p.play_state = PlayerPlayState::Playing;
    let f2 = p.advance(0.2);
    assert!(
        !f2.completed,
        "Stopped 帧未推进 elapsed（仍 0.0）→ 累计仅 0.2，未到完成"
    );
}

/// 多迭代：.4s 完成 iter0（boundary=0），.8s 完成 iter1 并结束（boundary=1 + completed）。
#[test]
fn iteration_boundary_reported_on_crossing() {
    let mut s = spec();
    s.iteration_count = Some(2);
    s.fill_mode = AnimationFillMode::Both;
    let mut p = player(s, opacity_fade());
    p.advance(0.0);
    let f1 = p.advance(0.4);
    let f2 = p.advance(0.4);
    assert!(!f1.completed);
    assert_eq!(f1.iteration_boundary, Some(0));
    assert!(f2.completed);
    assert_eq!(f2.iteration_boundary, Some(1));
    assert_close(f2.props.opacity.unwrap(), 1.0);
}

/// TRS 分量级 lerp：translate [0,20]→[0,0]；to 缺 scale/rotate 分量 → identity（[1,1]/0）lerp。
#[test]
fn transform_trs_component_lerp_with_identity() {
    let s = spec();
    let kf = KeyframesRule {
        name: "slide".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    transform: Some(TransformAnim {
                        translate: Some([
                            LenPct { px: 0.0, pct: 0.0 },
                            LenPct { px: 20.0, pct: 0.0 },
                        ]),
                        scale: Some([1.0, 1.0]),
                        rotate: Some(0.0),
                    }),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    transform: Some(TransformAnim {
                        translate: Some([
                            LenPct { px: 0.0, pct: 0.0 },
                            LenPct { px: 0.0, pct: 0.0 },
                        ]),
                        scale: None,
                        rotate: None,
                    }),
                    ..Default::default()
                },
            ),
        ],
    };
    let mut p = player(s, kf);
    p.advance(0.0);
    let f = p.advance(0.2); // progress .5（linear）
    let t = f.props.transform.expect("transform override");
    assert_eq!(
        t.translate,
        Some([LenPct::ZERO, LenPct { px: 10.0, pct: 0.0 }])
    );
    assert_eq!(
        t.scale,
        Some([1.0, 1.0]),
        "缺 scale 分量 → identity [1,1] lerp"
    );
    assert_eq!(t.rotate, Some(0.0), "缺 rotate 分量 → identity 0 lerp");
}

/// rotate 弧度 lerp：0 → π/2，中点 π/4。
#[test]
fn rotate_lerps_radians() {
    let s = spec();
    let kf = KeyframesRule {
        name: "spin".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    transform: Some(TransformAnim {
                        rotate: Some(0.0),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    transform: Some(TransformAnim {
                        rotate: Some(std::f32::consts::FRAC_PI_2),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        ],
    };
    let mut p = player(s, kf);
    p.advance(0.0);
    let f = p.advance(0.2);
    let rot = f.props.transform.unwrap().rotate.unwrap();
    assert_close(rot, std::f32::consts::FRAC_PI_4);
}

/// bg_color [f32;4] 逐通道 lerp。
#[test]
fn bg_color_lerps_per_channel() {
    let s = spec();
    let kf = KeyframesRule {
        name: "hue".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    bg_color: Some([1.0, 0.0, 0.0, 1.0]),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    bg_color: Some([0.0, 0.0, 1.0, 1.0]),
                    ..Default::default()
                },
            ),
        ],
    };
    let mut p = player(s, kf);
    p.advance(0.0);
    let f = p.advance(0.2);
    let c = f.props.bg_color.unwrap();
    for (got, want) in c.iter().zip([0.5, 0.0, 0.5, 1.0]) {
        assert_close(*got, want);
    }
}

/// duration=0：越过 delay 即完成，取末值（fill both）。
#[test]
fn zero_duration_completes_immediately() {
    let mut s = spec();
    s.duration = 0.0;
    s.fill_mode = AnimationFillMode::Both;
    let mut p = player(s, opacity_fade());
    let f = p.advance(0.1);
    assert!(f.completed);
    assert_close(f.props.opacity.unwrap(), 1.0);
}

/// 同 percent 重复 stop（fence 的 `from, 0%` 会展开出同位 stop）：后者胜（CSS 语义）。
#[test]
fn duplicate_percent_later_stop_wins() {
    let s = spec();
    let kf = KeyframesRule {
        name: "dup".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(0.0),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::Percent(0),
                AnimatableProps {
                    opacity: Some(0.3),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    ..Default::default()
                },
            ),
        ],
    };
    let mut p = player(s, kf);
    let f0 = p.advance(0.0);
    let f1 = p.advance(0.2);
    assert_close(
        f0.props.opacity.unwrap(),
        0.3, // 同位后者胜：首帧取 Percent(0) 的 0.3
    );
    assert_close(f1.props.opacity.unwrap(), 0.65); // 0.3→1 段中点 lerp
}

/// PlayerFrame 纯数据：默认帧 = 无 override + 未完成 + Playing。
#[test]
fn player_frame_default_is_empty() {
    let f = PlayerFrame::default();
    assert_eq!(f.props, AnimatableProps::default());
    assert!(!f.completed);
    assert_eq!(f.iteration_boundary, None);
    assert_eq!(f.play_state, PlayerPlayState::Playing);
}
