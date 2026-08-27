//! sync_animation_players（tick step g'）集成测试：class 声明式触发启停 player
//! （spec §5.2 class 触发 / §5.4 生命周期 / §6.3 fill-mode 完成态 / §6.4 多 animation）。
//!
//! 覆盖：加 class → rematch 后声明进 computed style → sync 启 player → 完成 fill both
//! 保留末值；声明消失 → 回收 player + 通道回 None；`animation: a .3s, b .5s` → 2 player
//! 同通道后者覆盖；同名 Completed 不重播；参数变 → reset 重跑；programmatic player
//! 不受 sync 管；backwards fill 启动即写首帧（delay 期不闪 base）；stage tick g' 接线。

use ikat_core::scene::animation::{update_all, KeyframePlayer};
use ikat_core::scene::{
    AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule, Node, NodeId, NodeKind,
    PlayerPlayState, Scene,
};
use ikat_core::stage::Stage;
use ikat_core::style::dynamic::{
    rematch_pseudo_classes, sync_animation_players, Combinator, Compound, Declaration, DynamicRule,
    ParsedSelector, ScopedRule, Specificity,
};
use ikat_core::style::resolved::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
};

fn assert_close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "expected {b} ± 1e-4, got {a}");
}

/// 构造 `.cls` 选择器（测试手搓，不走 fence parse）。
fn class_selector(cls: &str) -> ParsedSelector {
    ParsedSelector {
        raw: format!(".{cls}"),
        compound: vec![Compound {
            tag: None,
            classes: vec![cls.to_string()],
            id: None,
            combinator: Combinator::Descendant,
            pseudo_hover: false,
            pseudo_active: false,
            pseudo_disabled: false,
            pseudo_focus: false,
            pseudo_nth_child: None,
            attrs: Vec::new(),
        }],
        specificity: Specificity(0, 1, 0),
    }
}

/// 全局动态规则（scope_root = INVALID，跨作用域命中）。
fn push_rule(scene: &mut Scene, cls: &str, decl_prop: &str, decl_value: &str) {
    scene.dynamic_rules.entries.push(ScopedRule {
        rule: DynamicRule {
            selector: class_selector(cls),
            declarations: vec![Declaration {
                prop: decl_prop.to_string(),
                value: decl_value.to_string(),
            }],
        },
        scope_root: NodeId::INVALID,
    });
}

/// 单 Container 节点 Scene（from_nodes，id 由 slotmap 分配）。
fn scene_with_node() -> (Scene, NodeId) {
    let scene = Scene::from_nodes(
        vec![Node {
            kind: NodeKind::Container,
            ..Default::default()
        }],
        vec![],
    );
    let id = scene.get(scene.roots[0]).expect("root").id;
    (scene, id)
}

fn stop(selector: KeyframeStopSelector, props: AnimatableProps) -> KeyframeStop {
    KeyframeStop {
        selector,
        props,
        timing: None,
        hook: None,
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

/// 默认 spec：fadeIn .4s 单次 Normal Both Linear Running。
fn fade_spec() -> AnimationSpec {
    AnimationSpec {
        name: "fadeIn".into(),
        duration: 0.4,
        delay: 0.0,
        iteration_count: Some(1),
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::Both,
        timing_function: ikat_core::tween::Ease::Linear,
        play_state: AnimationPlayState::Running,
    }
}

/// 测 1（主流程，spec §5.2）：初始无 class 不启 player；加 class → rematch 后声明进
/// computed style → sync 启 player；推进 → fill both 完成保留末值且不重播；
/// 移除 class → 回收 player + 通道回 None（tween/base 接管）。
#[test]
fn class_add_starts_player_fill_both_retains_class_remove_reclaims() {
    let (mut scene, id) = scene_with_node();
    scene.keyframes.insert("fadeIn".into(), opacity_fade());
    push_rule(&mut scene, "fade", "animation", "fadeIn .4s both");

    // 初始无 class：rematch + sync 不启任何 player。
    rematch_pseudo_classes(&mut scene);
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 0, "无 class 时无 player");

    // 加 class → 下帧 rematch + sync 启 player。
    scene.get_mut(id).unwrap().classes.push("fade".to_string());
    rematch_pseudo_classes(&mut scene);
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "加 class 后启 player");
    let p = scene.players.values().next().unwrap();
    assert_eq!(p.node, id);
    assert_eq!(p.spec.name, "fadeIn");
    assert_eq!(p.play_state, PlayerPlayState::Playing);
    assert!(!p.programmatic, "class 触发的 player 非 programmatic");

    // 推进到完成：fill both → 保留末值 opacity=1.0，player 不回收。
    update_all(&mut scene, 0.2, &mut Vec::new());
    update_all(&mut scene, 0.2, &mut Vec::new());
    let p = scene.players.values().next().unwrap();
    assert_eq!(p.play_state, PlayerPlayState::Completed);
    assert_close(
        scene.anim.get(id).expect("anim").opacity.expect("opacity"),
        1.0,
    );

    // 声明仍在 + 参数未变 → 不重播（仍 1 个 Completed player）。
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "同名 Completed 不重播");
    assert_eq!(
        scene.players.values().next().unwrap().play_state,
        PlayerPlayState::Completed
    );

    // 移除 class → rematch + sync 回收 player，通道回 None。
    scene.get_mut(id).unwrap().classes.retain(|c| c != "fade");
    rematch_pseudo_classes(&mut scene);
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 0, "声明消失 → 回收 player");
    assert!(
        scene.anim.get(id).is_none(),
        "回收后通道回 None（tween/base 接管）"
    );
}

/// 测 2（spec §6.4 多 animation）：`animation: a .3s, b .5s` → 2 个 player；
/// update_all 按声明序写，同通道后声明覆盖前者。
#[test]
fn multi_animation_creates_one_player_per_declaration() {
    let (mut scene, id) = scene_with_node();
    // a: opacity 0→1 (.3s)；b: opacity 0.2→0.6 (.5s)，同通道。
    scene.keyframes.insert(
        "a".into(),
        KeyframesRule {
            name: "a".into(),
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
        },
    );
    scene.keyframes.insert(
        "b".into(),
        KeyframesRule {
            name: "b".into(),
            stops: vec![
                stop(
                    KeyframeStopSelector::From,
                    AnimatableProps {
                        opacity: Some(0.2),
                        ..Default::default()
                    },
                ),
                stop(
                    KeyframeStopSelector::To,
                    AnimatableProps {
                        opacity: Some(0.6),
                        ..Default::default()
                    },
                ),
            ],
        },
    );
    push_rule(
        &mut scene,
        "multi",
        "animation",
        "a .3s linear, b .5s linear",
    );
    scene.get_mut(id).unwrap().classes.push("multi".to_string());
    rematch_pseudo_classes(&mut scene);
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 2, "每条声明一个 player");
    let mut names: Vec<String> = scene
        .players
        .values()
        .map(|p| p.spec.name.clone())
        .collect();
    names.sort();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);

    // dt=0.25：a progress=0.833→0.833；b progress=0.5→0.4。b 后写 → 0.4 赢。
    update_all(&mut scene, 0.25, &mut Vec::new());
    assert_close(
        scene.anim.get(id).expect("anim").opacity.expect("opacity"),
        0.4,
    );
}

/// 测 3：同名 Completed player（含 fill none 的结束标记）视为已启动，
/// sync 不重播。
#[test]
fn same_name_completed_player_not_replayed() {
    let (mut scene, id) = scene_with_node();
    // 声明已存在于 computed style（rematch 后形态）。
    scene.get_mut(id).unwrap().style.animation = vec![fade_spec()];
    // 已有 Completed player（模拟已播完的 fill both / fill none 结束标记）。
    let mut p = KeyframePlayer::new(id, fade_spec(), opacity_fade());
    p.play_state = PlayerPlayState::Completed;
    scene.players.insert(p);

    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "同名 Completed 不重播");
    assert_eq!(
        scene.players.values().next().unwrap().play_state,
        PlayerPlayState::Completed
    );
}

/// 测 4：声明参数变（duration 改）→ kill 旧 + 重跑（elapsed 归零、spec 更新）。
#[test]
fn param_change_restarts_player() {
    let (mut scene, id) = scene_with_node();
    scene.keyframes.insert("fadeIn".into(), opacity_fade());
    // 旧 player：duration 0.4，已跑 0.1s。
    scene
        .players
        .insert(KeyframePlayer::new(id, fade_spec(), opacity_fade()));
    // 声明变 duration 0.8。
    let mut new_spec = fade_spec();
    new_spec.duration = 0.8;
    scene.get_mut(id).unwrap().style.animation = vec![new_spec.clone()];

    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "kill 旧 + 重跑，仍 1 个 player");
    let p = scene.players.values().next().unwrap();
    assert_eq!(p.spec.duration, 0.8);
    assert_eq!(p.elapsed, 0.0, "重跑从 0 开始");
    assert_eq!(p.play_state, PlayerPlayState::Playing);
    assert_eq!(p.iteration, 0);
}

/// 测 5：programmatic player（node.Play 建）不受 sync 管——
/// 声明消失不回收，靠 Stop/句柄；声明同名出现也不视为"已启动"。
#[test]
fn programmatic_player_not_managed_by_sync() {
    let (mut scene, id) = scene_with_node();
    scene.keyframes.insert("fadeIn".into(), opacity_fade());
    let mut p = KeyframePlayer::new(id, fade_spec(), opacity_fade());
    p.programmatic = true;
    scene.players.insert(p);

    // 无声明：sync 不回收 programmatic player。
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "无声明不回收 programmatic");
    assert!(scene.players.values().next().unwrap().programmatic);

    // 声明同名出现：programmatic 不算已存在 → sync 另建 class player（2 个并存）。
    scene.get_mut(id).unwrap().style.animation = vec![fade_spec()];
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 2, "programmatic 与 class 触发各自独立");
    assert!(
        scene.players.values().any(|q| !q.programmatic),
        "sync 建出 class 触发的 player"
    );

    // 声明消失：programmatic 保留（靠 Stop 回收）。
    scene.get_mut(id).unwrap().style.animation.clear();
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "声明消失不回收 programmatic");
    assert!(scene.players.values().next().unwrap().programmatic);
}

/// 测 6（spec §5.2 首帧 backwards fill）：`fadeIn .4s 1s backwards`（delay 1s）——
/// sync 启动即写首帧 0.0（不等下帧 update），delay 期持续显示首帧，不闪 base。
#[test]
fn backwards_fill_writes_first_frame_immediately_on_sync() {
    let (mut scene, id) = scene_with_node();
    scene.keyframes.insert("fadeIn".into(), opacity_fade());
    let mut s = fade_spec();
    s.delay = 1.0;
    s.fill_mode = AnimationFillMode::Backwards;
    scene.get_mut(id).unwrap().style.animation = vec![s];

    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1);
    assert_close(
        scene
            .anim
            .get(id)
            .expect("启动即写首帧")
            .opacity
            .expect("opacity"),
        0.0,
    );

    // delay 中推进：backwards fill 保持首帧值。
    update_all(&mut scene, 0.5, &mut Vec::new());
    assert_close(
        scene.anim.get(id).expect("anim").opacity.expect("opacity"),
        0.0,
    );
}

/// name 指定的 2-stop opacity keyframes（from→to，测 8 专用）。
fn opacity_ramp(name: &str, from: f32, to: f32) -> KeyframesRule {
    KeyframesRule {
        name: name.into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(from),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(to),
                    ..Default::default()
                },
            ),
        ],
    }
}

/// 测 8：sync 移除其一 player 时，共享通道保留另一 player 的值——
/// 回收掩码 = own ∩ ¬(剩余 player 持有)（dynamic.rs 已实现，本测试锁死防回归）。
#[test]
fn sync_remove_keeps_remaining_players_shared_channel_value() {
    let (mut scene, id) = scene_with_node();
    scene
        .keyframes
        .insert("a".into(), opacity_ramp("a", 0.0, 1.0));
    scene
        .keyframes
        .insert("b".into(), opacity_ramp("b", 0.2, 0.6));
    let spec_a = AnimationSpec {
        name: "a".into(),
        duration: 0.4,
        delay: 0.0,
        iteration_count: Some(1),
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        timing_function: ikat_core::tween::Ease::Linear,
        play_state: AnimationPlayState::Running,
    };
    let spec_b = AnimationSpec {
        name: "b".into(),
        duration: 0.4,
        delay: 0.0,
        iteration_count: Some(1),
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        timing_function: ikat_core::tween::Ease::Linear,
        play_state: AnimationPlayState::Running,
    };
    // 声明 a + b → sync 建 2 player；tick 一帧：两者都写 opacity（b 后写赢 = 0.4）。
    scene.get_mut(id).unwrap().style.animation = vec![spec_a.clone(), spec_b.clone()];
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 2);
    update_all(&mut scene, 0.2, &mut Vec::new());
    assert_close(
        scene.anim.get(id).expect("anim").opacity.expect("opacity"),
        0.4,
    );

    // 声明只剩 b → sync 移除 a → 共享通道 opacity 保留 b 的值（回收掩码不清）。
    scene.get_mut(id).unwrap().style.animation = vec![spec_b];
    sync_animation_players(&mut scene);
    assert_eq!(scene.players.len(), 1, "a 回收、b 保留");
    assert_eq!(scene.players.values().next().unwrap().spec.name, "b");
    assert_close(
        scene.anim.get(id).expect("anim").opacity.expect("opacity"),
        0.4,
    );
}

/// 测 7（stage 接线）：tick step g'（rematch 后、solve 前）调 sync_animation_players——
/// 加 class 后下一 tick 启 player，backwards 首帧同帧可见。
#[test]
fn tick_step_g_prime_starts_player_after_class_change() {
    let mut stage = Stage::new((200.0, 200.0)).unwrap();
    let node = stage.create_node("div", "").unwrap();
    {
        let scene = stage.scene.as_mut().unwrap();
        scene.keyframes.insert("fadeIn".into(), opacity_fade());
        push_rule(scene, "fade", "animation", "fadeIn .4s both linear");
    }
    stage.add_class(node, "fade").unwrap();
    stage.advance_time(0.05);
    stage.tick_and_render();
    {
        let scene = stage.scene.as_ref().unwrap();
        assert_eq!(scene.players.len(), 1, "g' 启 player");
        // 本 tick update_all（step b）在 sync（g'）前跑，player 未被推进 →
        // 首帧 0.0 已写（backwards 立即写，spec §5.2）。
        assert_close(
            scene
                .anim
                .get(node)
                .expect("anim")
                .opacity
                .expect("opacity"),
            0.0,
        );
    }
    // 下一 tick：update_all 推进 dt=0.05 → opacity = 0.05/0.4 = 0.125。
    stage.advance_time(0.05);
    stage.tick_and_render();
    let scene = stage.scene.as_ref().unwrap();
    assert_close(
        scene
            .anim
            .get(node)
            .expect("anim")
            .opacity
            .expect("opacity"),
        0.125,
    );
}
