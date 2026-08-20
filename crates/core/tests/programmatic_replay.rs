//! `play_programmatic` 重播语义回归：重复 Play = 确定性从头重播。
//!
//! 修复前缺陷：同节点同名 programmatic player 不回收——旧 Completed+fill-both player
//! 每帧续写末值且 `sync_animation_players` 永不回收 programmatic player，重复 Play 叠加
//! 写同通道且 player 无限累积，第二次起视觉上"静默无效"。

use loomgui_core::scene::animation::{play_programmatic, update_all};
use loomgui_core::scene::{
    AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule, Node, NodeId, NodeKind,
    PlayerPlayState, Scene, TransformAnim,
};

/// 2-stop translate-only keyframes（Field Notes N11 形态：keyframes 仅 transform）。
fn lunge() -> KeyframesRule {
    let stop = |sel, tx| KeyframeStop {
        selector: sel,
        props: AnimatableProps {
            transform: Some(TransformAnim {
                translate: Some([tx, 0.0]),
                ..Default::default()
            }),
            ..Default::default()
        },
        hook: None,
    };
    KeyframesRule {
        name: "lunge".into(),
        stops: vec![
            stop(KeyframeStopSelector::From, 0.0),
            stop(KeyframeStopSelector::To, 100.0),
        ],
    }
}

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

fn anim_translate_x(scene: &Scene, node: NodeId) -> f32 {
    let a = scene.anim.0.get(&node).expect("anim entry");
    let m = a.transform.expect("transform channel");
    m[4]
}

fn prog_count(scene: &Scene, node: NodeId, name: &str) -> usize {
    scene
        .players
        .values()
        .filter(|p| p.programmatic && p.node == node && p.spec.name == name)
        .count()
}

fn tick(scene: &mut Scene, dt: f32) {
    let mut out = Vec::new();
    update_all(scene, dt, &mut out);
}

/// 完整播完一轮后再次 Play：首帧立即回起点、player 不累积、推进正常。
#[test]
fn replay_after_completion_restarts_from_beginning() {
    let (mut scene, node) = scene_with_node();
    scene.keyframes.insert("lunge".into(), lunge());

    let _ = play_programmatic(&mut scene, node, "lunge").expect("first play");
    assert_eq!(anim_translate_x(&scene, node), 0.0, "首帧 = from 值");
    tick(&mut scene, 2.0); // 默认 1s 单次 + fill both → 终态
    assert_eq!(anim_translate_x(&scene, node), 100.0);

    let _ = play_programmatic(&mut scene, node, "lunge").expect("second play");
    assert_eq!(
        prog_count(&scene, node, "lunge"),
        1,
        "重复 Play 不得累积 player"
    );
    assert_eq!(
        anim_translate_x(&scene, node),
        0.0,
        "重播首帧必须立即回到 from（不被旧 player 末值压住）"
    );
    tick(&mut scene, 0.5); // ease cubic-out 中点值在 0..100 之间
    let mid = anim_translate_x(&scene, node);
    assert!(mid > 0.0 && mid < 100.0, "重播须实际推进，中途值 = {mid}");
    tick(&mut scene, 1.0);
    assert_eq!(anim_translate_x(&scene, node), 100.0);
}

/// 连续多次 Play（含未播完时打断）：始终至多一个同名 programmatic player。
#[test]
fn rapid_replay_keeps_single_player() {
    let (mut scene, node) = scene_with_node();
    scene.keyframes.insert("lunge".into(), lunge());

    for i in 0..5 {
        let _ = play_programmatic(&mut scene, node, "lunge").expect("play");
        tick(&mut scene, 0.3); // 半途打断
        assert_eq!(prog_count(&scene, node, "lunge"), 1, "round {i}");
    }
}

/// 同帧 Stop（标 Stopped，尚未过 update_all 回收）+ Play：去重路径直接移除旧 player，
/// 新 player 正常播放，下一帧 update_all 不双重清理。
#[test]
fn same_frame_stop_then_play_is_deterministic() {
    let (mut scene, node) = scene_with_node();
    scene.keyframes.insert("lunge".into(), lunge());

    let k1 = play_programmatic(&mut scene, node, "lunge").expect("first play");
    tick(&mut scene, 0.2);
    // FFI stop 的 core 语义：只标 Stopped，回收延迟到下一帧 update_all。
    scene.players.get_mut(k1).unwrap().play_state = PlayerPlayState::Stopped;

    let k2 = play_programmatic(&mut scene, node, "lunge").expect("second play");
    assert_ne!(k1, k2);
    assert_eq!(prog_count(&scene, node, "lunge"), 1);
    assert_eq!(anim_translate_x(&scene, node), 0.0);
    tick(&mut scene, 0.5);
    let mid = anim_translate_x(&scene, node);
    assert!(
        mid > 0.0 && mid < 100.0,
        "同帧 Stop+Play 后须正常推进，中途值 = {mid}"
    );
}

/// 不同名动画共存：去重只替换同名，不误伤并行的其他 programmatic player。
#[test]
fn different_name_players_coexist() {
    let (mut scene, node) = scene_with_node();
    scene.keyframes.insert("lunge".into(), lunge());
    let mut flash = lunge();
    flash.name = "flash".into();
    scene.keyframes.insert("flash".into(), flash);

    let _ = play_programmatic(&mut scene, node, "lunge").expect("lunge");
    let _ = play_programmatic(&mut scene, node, "flash").expect("flash");
    let _ = play_programmatic(&mut scene, node, "lunge").expect("lunge replay");
    assert_eq!(prog_count(&scene, node, "lunge"), 1);
    assert_eq!(prog_count(&scene, node, "flash"), 1);
    assert_eq!(scene.players.values().filter(|p| p.programmatic).count(), 2);
}
