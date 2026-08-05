//! player.update → NodeAnim 写入 + tick step b 优先级集成测试（spec §5.1 / §6）。
//!
//! 覆盖：update_all 写 opacity/transform（SRT 合成顺序）/bg_color/text_color；
//! tick step b 中 player 在 tween 之后写（animation > transition 同通道优先级）；
//! Completed + fill forwards/both 每帧续写末值不回收；fill none/backwards 完成即清
//! 自己持有的通道（tween/base 接管）且 player 保留 Completed 态（防 sync 重启）；
//! Stopped 清通道 + 回收；Paused 位置保持；全 None TRS 不 override。

use loomgui_core::scene::animation::{update_all, KeyframePlayer};
use loomgui_core::scene::{
    AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule, Node, NodeId, NodeKind,
    PlayerPlayState, Scene, TransformAnim,
};
use loomgui_core::stage::Stage;
use loomgui_core::style::resolved::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
};
use loomgui_core::transform::{from_scale, from_translate, Affine2Ext};
use loomgui_core::tween::{Ease, TweenProp};

fn assert_close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "expected {b} ± 1e-4, got {a}");
}

fn assert_close_msg(a: f32, b: f32, msg: &str) {
    assert!((a - b).abs() < 1e-4, "{msg}: expected {b} ± 1e-4, got {a}");
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

fn stop(selector: KeyframeStopSelector, props: AnimatableProps) -> KeyframeStop {
    KeyframeStop {
        selector,
        props,
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

/// 2-stop opacity + translate 组合（slide-in 风格）。
fn slide_fade() -> KeyframesRule {
    KeyframesRule {
        name: "slideIn".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(0.0),
                    transform: Some(TransformAnim {
                        translate: Some([0.0, 20.0]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    transform: Some(TransformAnim {
                        translate: Some([0.0, 0.0]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        ],
    }
}

/// transform-only keyframes（测 owned-channel 精确清除用）。
fn translate_only() -> KeyframesRule {
    KeyframesRule {
        name: "slide".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    transform: Some(TransformAnim {
                        translate: Some([0.0, 20.0]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    transform: Some(TransformAnim {
                        translate: Some([0.0, 0.0]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        ],
    }
}

/// 单 Container 节点的 Scene（from_nodes，id 由 slotmap 分配）。
fn scene_with_1_node() -> (Scene, NodeId) {
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

fn insert_player(scene: &mut Scene, node: NodeId, s: AnimationSpec, kf: KeyframesRule) {
    scene.players.insert(KeyframePlayer::new(node, s, kf));
}

/// 直接调 update_all 推进一帧（等价 tick step b 的 player 段）。
fn tick(scene: &mut Scene, dt: f32) {
    update_all(scene, dt, &mut Vec::new());
}

/// 测 1：update_all 写 opacity + transform；transform 按 SRT 合成
/// （T(10,0)∘S(2,1)：点先 scale 再 translate，(1,0)→(12,0)）。
#[test]
fn update_all_writes_opacity_and_transform_srt_order() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    let kf = KeyframesRule {
        name: "combo".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(0.0),
                    transform: Some(TransformAnim {
                        translate: Some([10.0, 0.0]),
                        scale: Some([2.0, 1.0]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    transform: Some(TransformAnim {
                        translate: Some([10.0, 0.0]),
                        scale: Some([2.0, 1.0]),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
        ],
    };
    insert_player(&mut scene, node, s, kf);
    tick(&mut scene, 0.2);
    let anim = scene.anim.get(node).expect("player 写 anim");
    assert_close(anim.opacity.expect("opacity"), 0.5);
    let m = anim.transform.expect("transform");
    let (x, y) = m.apply_point(1.0, 0.0);
    assert_close(x, 12.0);
    assert_close(y, 0.0);
    // 对照：若实现误用 S∘T（先平移再缩放），(1,0) → T → (11,0) → S → (22,0)——
    // 断言 22.0 证明本测试能抓住合成顺序错误。
    let wrong_order = from_scale(2.0, 1.0).mul(from_translate(10.0, 0.0));
    let (wx, _) = wrong_order.apply_point(1.0, 0.0);
    assert_close_msg(wx, 22.0, "对照：S∘T 顺序会给 (22,0)");
}

/// 测 1b：mid-anim 合成（translate lerp 0,20→0,0，中点 = T(0,10) 纯平移）。
#[test]
fn update_all_composes_lerped_translate() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    insert_player(&mut scene, node, s, slide_fade());
    tick(&mut scene, 0.2);
    let anim = scene.anim.get(node).expect("player 写 anim");
    assert_close(anim.opacity.unwrap(), 0.5);
    let m = anim.transform.expect("transform");
    assert!(
        m.is_pure_translation(),
        "translate-only 动画合成为纯平移矩阵"
    );
    let (x, y) = m.apply_point(0.0, 0.0);
    assert_close(x, 0.0);
    assert_close(y, 10.0);
}

/// 测 2（Stage 集成）：tick step b 中 player 在 tween 之后写 → animation 覆盖 transition 同通道。
#[test]
fn tick_step_b_animation_overrides_transition_same_channel() {
    let font = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/DejaVuSans.ttf"
    ))
    .unwrap();

    // 场景 A：tween（transition opacity .3→.7 .2s）+ player（animation opacity 0→1 .4s）
    // dt=0.2：tween 完成写 0.7，player 后写 0.5 → 断言 0.5（player 赢）。
    let mut stage = Stage::new((200.0, 200.0)).unwrap();
    stage.register_font("DejaVu", font.clone(), true).unwrap();
    let node = stage.create_node("div", "").unwrap();
    stage.tweens.tween(
        node,
        TweenProp::Opacity,
        [0.3, 0.0, 0.0, 0.0],
        [0.7, 0.0, 0.0, 0.0],
        Ease::Linear,
        0.0,
        0.2,
        0,
    );
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    {
        let scene = stage.scene.as_mut().unwrap();
        // g' sync_animation_players 只管有声明（computed style.animation）的 player——
        // 手工插入的 player 须有声明，否则下个 tick 的 g' 视为"声明消失"回收。
        // 声明须进 base_style：rematch 每帧从 base_style 重起 cascade 重建 style，
        // 直接写 style.animation 会被 rematch 覆盖（打包期 inline/静态规则即此路径）。
        scene.get_mut(node).unwrap().base_style.animation = vec![s.clone()];
        insert_player(scene, node, s, opacity_fade());
    }
    stage.advance_time(0.2);
    stage.tick_and_render();
    let anim = stage.scene.as_ref().unwrap().anim.get(node).expect("anim");
    assert_close_msg(anim.opacity.expect("opacity"), 0.5, "player 后写覆盖 tween");

    // 场景 B 对照：无 player 时 tween 值可见（0.7）——证明场景 A 的 0.5 确实来自 player 覆盖。
    let mut stage = Stage::new((200.0, 200.0)).unwrap();
    stage.register_font("DejaVu", font, true).unwrap();
    let node = stage.create_node("div", "").unwrap();
    stage.tweens.tween(
        node,
        TweenProp::Opacity,
        [0.3, 0.0, 0.0, 0.0],
        [0.7, 0.0, 0.0, 0.0],
        Ease::Linear,
        0.0,
        0.2,
        0,
    );
    stage.advance_time(0.2);
    stage.tick_and_render();
    let anim = stage.scene.as_ref().unwrap().anim.get(node).expect("anim");
    assert_close_msg(
        anim.opacity.expect("opacity"),
        0.7,
        "无 player 时 tween 值可见",
    );
}

/// 测 3：Completed + fill forwards 每帧续写末值，player 不回收。
#[test]
fn completed_forwards_keeps_writing_end_value_and_player_retained() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Forwards;
    insert_player(&mut scene, node, s, opacity_fade());
    tick(&mut scene, 0.2);
    tick(&mut scene, 0.2);
    assert_eq!(scene.players.len(), 1, "forwards 完成后不回收");
    let p = scene.players.values().next().unwrap();
    assert_eq!(p.play_state, PlayerPlayState::Completed);
    assert_close(scene.anim.get(node).unwrap().opacity.unwrap(), 1.0);
    // 后续每帧持续写末值。
    tick(&mut scene, 0.1);
    assert_close(scene.anim.get(node).unwrap().opacity.unwrap(), 1.0);
    assert_eq!(scene.players.len(), 1);
}

/// 测 4：Completed + fill none 清掉 player 自己持有的通道（tween/base 接管），
/// player 保留在 Completed 态（sync_animation_players 靠它识别"已结束"，防声明仍在时重启）。
#[test]
fn completed_fill_none_clears_owned_channels_player_retained() {
    let (mut scene, node) = scene_with_1_node();
    insert_player(&mut scene, node, spec(), opacity_fade());
    tick(&mut scene, 0.2);
    tick(&mut scene, 0.2);
    assert_eq!(
        scene.players.len(),
        1,
        "Completed player 保留（防 sync 重启）"
    );
    let p = scene.players.values().next().unwrap();
    assert_eq!(p.play_state, PlayerPlayState::Completed);
    assert!(
        scene.anim.get(node).is_none(),
        "fill none 完成：通道回 None → tween/base 接管"
    );
    // 后续 tick 保持（不再写回）。
    tick(&mut scene, 0.1);
    assert!(scene.anim.get(node).is_none());
}

/// 测 4b：owned-channel 精确清除——player 只持有 transform，不动 tween 在写的 opacity。
#[test]
fn fill_none_completion_only_clears_player_owned_channels() {
    let (mut scene, node) = scene_with_1_node();
    insert_player(&mut scene, node, spec(), translate_only());
    // 模拟 tween 在写 opacity（同节点另一通道）。
    scene.anim.ensure(node).opacity = Some(0.42);
    tick(&mut scene, 0.2);
    tick(&mut scene, 0.2);
    let anim = scene.anim.get(node).expect("opacity 通道仍由 tween 持有");
    assert_close_msg(anim.opacity.unwrap(), 0.42, "非 player 通道不受影响");
    assert!(anim.transform.is_none(), "player 自己的通道已清");
}

/// 测 4c（T7 review Important）：多 animation 共享通道——fill-none 完成清通道带掩码，
/// 完成帧保留同节点其他活跃 player 本帧已写的值（不闪 base）；独占通道照常回 None。
#[test]
fn fill_none_completion_keeps_other_players_shared_channel_value() {
    let (mut scene, node) = scene_with_1_node();
    // pulse：opacity 0.2→1.0 (.6s, fill forwards)，长动画本测试内不完成。
    let mut pulse = spec();
    pulse.name = "pulse".into();
    pulse.duration = 0.6;
    pulse.fill_mode = AnimationFillMode::Forwards;
    insert_player(
        &mut scene,
        node,
        pulse,
        KeyframesRule {
            name: "pulse".into(),
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
                        opacity: Some(1.0),
                        ..Default::default()
                    },
                ),
            ],
        },
    );
    // flash：opacity 0.9→0.3 + translate 0→10（.2s, fill none）——与 pulse 共享 opacity、
    // 独占 transform。插入在后（槽序靠后）：本帧 pulse 先写、flash 完成清——修前
    // 无条件清会把 pulse 本帧已写的值一起清掉（一帧闪 base）。
    let mut flash = spec();
    flash.name = "flash".into();
    flash.duration = 0.2;
    flash.fill_mode = AnimationFillMode::None;
    insert_player(
        &mut scene,
        node,
        flash,
        KeyframesRule {
            name: "flash".into(),
            stops: vec![
                stop(
                    KeyframeStopSelector::From,
                    AnimatableProps {
                        opacity: Some(0.9),
                        transform: Some(TransformAnim {
                            translate: Some([0.0, 10.0]),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
                stop(
                    KeyframeStopSelector::To,
                    AnimatableProps {
                        opacity: Some(0.3),
                        transform: Some(TransformAnim {
                            translate: Some([0.0, 0.0]),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
            ],
        },
    );

    // flash 完成帧（0.2s）：pulse 先写 opacity = 0.2 + 0.8*(0.2/0.6) = 0.4667；
    // flash 完成清通道 → 共享 opacity 保留（掩码），独占 transform 回 None。
    tick(&mut scene, 0.2);
    assert_eq!(
        scene.players.len(),
        2,
        "两个 player 都保留（flash 为 Completed 结束标记）"
    );
    let anim = scene.anim.get(node).expect("pulse 仍在写");
    assert_close_msg(
        anim.opacity.expect("opacity"),
        0.2 + 0.8 * (0.2 / 0.6),
        "共享通道保留 pulse 本帧值（不闪 base）",
    );
    assert!(
        anim.transform.is_none(),
        "flash 独占通道回 None（base 接管）"
    );

    // 下一帧：pulse 继续写（0.3s → 0.6），flash 惰性（Completed 不写不清）。
    tick(&mut scene, 0.1);
    let anim = scene.anim.get(node).expect("anim");
    assert_close_msg(anim.opacity.unwrap(), 0.6, "pulse 持续可见");
}

/// 测 5：Stopped（显式 Stop 标记）→ 清通道 + 从 players 表回收。
#[test]
fn stopped_player_clears_channels_and_is_removed() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    insert_player(&mut scene, node, s, opacity_fade());
    tick(&mut scene, 0.2);
    assert_close(scene.anim.get(node).unwrap().opacity.unwrap(), 0.5);
    let key = scene.players.keys().next().unwrap();
    scene.players.get_mut(key).unwrap().play_state = PlayerPlayState::Stopped;
    tick(&mut scene, 0.1);
    assert!(scene.players.is_empty(), "Stopped 回收");
    assert!(scene.anim.get(node).is_none(), "Stopped 清通道回 base");
}

/// 测 6：Paused 位置保持（elapsed 不推进，仍写当前帧值）。
#[test]
fn paused_player_holds_position() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    insert_player(&mut scene, node, s, opacity_fade());
    tick(&mut scene, 0.2);
    assert_close(scene.anim.get(node).unwrap().opacity.unwrap(), 0.5);
    let key = scene.players.keys().next().unwrap();
    scene.players.get_mut(key).unwrap().play_state = PlayerPlayState::Paused;
    tick(&mut scene, 0.2);
    assert_close_msg(
        scene.anim.get(node).unwrap().opacity.unwrap(),
        0.5,
        "暂停期间取值不变",
    );
    assert_eq!(scene.players.len(), 1);
}

/// 测 7：全 None TRS → NodeAnim.transform 不写（不 override base transform）。
#[test]
fn all_none_transform_is_not_written() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    let kf = KeyframesRule {
        name: "edge".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(0.0),
                    transform: Some(TransformAnim::default()),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    transform: Some(TransformAnim::default()),
                    ..Default::default()
                },
            ),
        ],
    };
    insert_player(&mut scene, node, s, kf);
    tick(&mut scene, 0.2);
    let anim = scene.anim.get(node).expect("opacity 已写");
    assert_close(anim.opacity.unwrap(), 0.5);
    assert!(
        anim.transform.is_none(),
        "全 None TRS 不 override transform"
    );
}

/// 测 8：bg_color / text_color 通道直写。
#[test]
fn update_all_writes_bg_and_text_color() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    let kf = KeyframesRule {
        name: "colors".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    bg_color: Some([1.0, 0.0, 0.0, 1.0]),
                    text_color: Some([0.0, 0.0, 0.0, 1.0]),
                    ..Default::default()
                },
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    bg_color: Some([0.0, 0.0, 1.0, 1.0]),
                    text_color: Some([1.0, 1.0, 1.0, 1.0]),
                    ..Default::default()
                },
            ),
        ],
    };
    insert_player(&mut scene, node, s, kf);
    tick(&mut scene, 0.2);
    let anim = scene.anim.get(node).expect("anim");
    for (got, want) in anim.bg_color.unwrap().iter().zip([0.5, 0.0, 0.5, 1.0]) {
        assert_close(*got, want);
    }
    for (got, want) in anim.text_color.unwrap().iter().zip([0.5, 0.5, 0.5, 1.0]) {
        assert_close(*got, want);
    }
}

/// 测 9：fill none + 完成后通道回 None，tween 值重新可见（spec §6.3 下帧 tween/base 接管）。
/// 真实 tick 顺序：tween.update 先写 → update_all 后写/清。完成转变帧清一次后 player 惰性，
/// 下帧 tween 的写入不再被清。
#[test]
fn fill_none_completion_hands_back_to_tween_next_frame() {
    let (mut scene, node) = scene_with_1_node();
    insert_player(&mut scene, node, spec(), opacity_fade());
    tick(&mut scene, 0.2);
    assert_close(scene.anim.get(node).unwrap().opacity.unwrap(), 0.5);
    // 完成转变帧：tween 先写（0.2）→ player 完成清一次 → 本帧 None（base）。
    scene.anim.ensure(node).opacity = Some(0.2);
    tick(&mut scene, 0.4);
    assert!(
        scene.anim.get(node).is_none(),
        "完成帧通道回 None（下帧起 tween 接管）"
    );
    // 下帧：tween 再写 → Completed player 惰性不再清 → tween 值可见。
    scene.anim.ensure(node).opacity = Some(0.2);
    tick(&mut scene, 0.1);
    let anim = scene.anim.get(node).expect("tween 值接管");
    assert_close_msg(anim.opacity.unwrap(), 0.2, "player 完成回退后 tween 值可见");
}
