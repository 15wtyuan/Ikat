//! 动画事件 emit 集成测试（spec §7.1 事件流 / §7.5 事件 struct / §7.4 OnKey 注册）。
//!
//! 覆盖：START（首帧一次）/ END（完成转变帧一次）/ ITERATION（迭代边界；count=1 完成
//! 只发 END）/ KEY（OnKey 百分比跨越，同 iteration 防重，下一 iteration 重新可发）/
//! HOOK（@loom-hook stop 跨越）；reverse/delay 首帧不误发；OnKey 与同百分点 hook 独立触发；
//! 事件 payload 的 player_key 编码可还原。

use loomgui_core::event::{
    EVT_ANIMATION_END, EVT_ANIMATION_HOOK, EVT_ANIMATION_ITERATION, EVT_ANIMATION_KEY,
    EVT_ANIMATION_START,
};
use loomgui_core::input::EventRecord;
use loomgui_core::scene::animation::{
    player_key_from_u64, register_on_key, update_all, KeyframePlayer, PlayerKey,
};
use loomgui_core::scene::{
    AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule, Node, NodeId, NodeKind,
    PlayerPlayState, Scene,
};
use loomgui_core::style::resolved::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
};
use loomgui_core::tween::Ease;

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

fn stop(
    selector: KeyframeStopSelector,
    props: AnimatableProps,
    hook: Option<&str>,
) -> KeyframeStop {
    KeyframeStop {
        selector,
        props,
        hook: hook.map(str::to_owned),
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
                None,
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    ..Default::default()
                },
                None,
            ),
        ],
    }
}

/// 3-stop keyframes，50% stop 带 @loom-hook "half"。
fn hook_fade() -> KeyframesRule {
    KeyframesRule {
        name: "fadeIn".into(),
        stops: vec![
            stop(
                KeyframeStopSelector::From,
                AnimatableProps {
                    opacity: Some(0.0),
                    ..Default::default()
                },
                None,
            ),
            stop(
                KeyframeStopSelector::Percent(50),
                AnimatableProps {
                    opacity: Some(0.5),
                    ..Default::default()
                },
                Some("half"),
            ),
            stop(
                KeyframeStopSelector::To,
                AnimatableProps {
                    opacity: Some(1.0),
                    ..Default::default()
                },
                None,
            ),
        ],
    }
}

/// 单 Container 节点的 Scene（id 由 slotmap 分配）。
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

fn insert_player(
    scene: &mut Scene,
    node: NodeId,
    s: AnimationSpec,
    kf: KeyframesRule,
) -> PlayerKey {
    scene.players.insert(KeyframePlayer::new(node, s, kf))
}

/// 直接调 update_all 推进一帧（等价 tick step b 的 player 段），返回本帧事件。
fn tick(scene: &mut Scene, dt: f32) -> Vec<EventRecord> {
    let mut out = Vec::new();
    update_all(scene, dt, &mut out);
    out
}

fn tick_n(scene: &mut Scene, dt: f32, n: usize) -> Vec<EventRecord> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(tick(scene, dt));
    }
    all
}

fn by_type(evs: &[EventRecord], ty: u8) -> Vec<&EventRecord> {
    evs.iter().filter(|e| e.event_type == ty).collect()
}

/// name 的 EventStrTable 索引（click_count+pad 24-bit 小端）。
fn name_idx(rec: &EventRecord) -> u32 {
    rec.click_count as u32 | ((rec.pad[0] as u32) << 8) | ((rec.pad[1] as u32) << 16)
}

fn name_of<'a>(scene: &'a Scene, rec: &EventRecord) -> &'a str {
    scene.event_strs.get(name_idx(rec)).unwrap_or("")
}

/// player_key 的 EventStrTable 载荷（hook_name 索引，f32 bits）。
fn hook_name_of<'a>(scene: &'a Scene, rec: &EventRecord) -> &'a str {
    scene.event_strs.get(rec.y.to_bits()).unwrap_or("")
}

/// 测 1：fadeIn .4s 单次——首帧 START（一次）；完成帧 END（一次）；全程无 ITERATION。
#[test]
fn start_once_then_end_on_completion() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.fill_mode = AnimationFillMode::Both;
    insert_player(&mut scene, node, s, opacity_fade());
    let all = tick_n(&mut scene, 0.1, 5); // t=0.1..0.5，t=0.4 完成

    let starts = by_type(&all, EVT_ANIMATION_START);
    assert_eq!(starts.len(), 1, "START 只发一次");
    assert_eq!(starts[0].node_id, node.0);
    assert_eq!(name_of(&scene, starts[0]), "fadeIn");

    let ends = by_type(&all, EVT_ANIMATION_END);
    assert_eq!(ends.len(), 1, "END 只发一次（完成转变帧）");
    assert_eq!(ends[0].node_id, node.0);
    assert_eq!(name_of(&scene, ends[0]), "fadeIn");

    assert!(
        by_type(&all, EVT_ANIMATION_ITERATION).is_empty(),
        "count=1 完成只发 END"
    );
    assert!(by_type(&all, EVT_ANIMATION_KEY).is_empty());
    assert!(by_type(&all, EVT_ANIMATION_HOOK).is_empty());
}

/// 测 2：infinite——每个 iteration 边界 emit ITERATION{刚结束的迭代序号}；无 END。
#[test]
fn iteration_events_on_every_boundary_infinite() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.duration = 1.0;
    s.iteration_count = None;
    insert_player(&mut scene, node, s, opacity_fade());
    let all = tick_n(&mut scene, 0.3, 8); // t=0.3..2.4，边界 @1.2(0 结束)、@2.4(1 结束)

    let iters = by_type(&all, EVT_ANIMATION_ITERATION);
    assert_eq!(iters.len(), 2);
    assert_eq!(iters[0].y.to_bits(), 0, "迭代 0 结束");
    assert_eq!(iters[1].y.to_bits(), 1, "迭代 1 结束");
    assert_eq!(name_of(&scene, iters[0]), "fadeIn");
    assert!(
        by_type(&all, EVT_ANIMATION_END).is_empty(),
        "infinite 无 END"
    );
}

/// 测 3：count=2——非完成边界 ITERATION(0)；完成帧跨界（iteration>1）发 ITERATION(1) + END。
#[test]
fn iteration_emitted_on_completion_when_count_gt_1() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.iteration_count = Some(2);
    insert_player(&mut scene, node, s, opacity_fade());
    let all = tick_n(&mut scene, 0.1, 8); // t=0.1..0.8，边界 @0.4、完成 @0.8

    let iters = by_type(&all, EVT_ANIMATION_ITERATION);
    assert_eq!(iters.len(), 2, "两个边界各一次");
    assert_eq!(iters[0].y.to_bits(), 0);
    assert_eq!(iters[1].y.to_bits(), 1);
    assert_eq!(by_type(&all, EVT_ANIMATION_END).len(), 1);
    assert_eq!(by_type(&all, EVT_ANIMATION_START).len(), 1);
}

/// 测 4：OnKey(.5) 注册——progress 跨越 .5 时 emit KEY{percent:.5}；同 iteration 不重复。
#[test]
fn key_event_on_percent_crossing() {
    let (mut scene, node) = scene_with_1_node();
    let k = insert_player(&mut scene, node, spec(), opacity_fade());
    register_on_key(&mut scene, k, 0.5);
    let all = tick_n(&mut scene, 0.1, 5);

    let keys = by_type(&all, EVT_ANIMATION_KEY);
    assert_eq!(keys.len(), 1, "同 iteration 只发一次");
    assert_eq!(keys[0].node_id, node.0);
    assert_eq!(keys[0].y, 0.5);
    assert_eq!(name_of(&scene, keys[0]), "fadeIn");
}

/// 测 5：@loom-hook "half" 在 50% stop——跨越 0.5 时 emit HOOK{name:"half"}。
#[test]
fn hook_event_on_stop_crossing() {
    let (mut scene, node) = scene_with_1_node();
    insert_player(&mut scene, node, spec(), hook_fade());
    let all = tick_n(&mut scene, 0.1, 5);

    let hooks = by_type(&all, EVT_ANIMATION_HOOK);
    assert_eq!(hooks.len(), 1, "同 iteration 只发一次");
    assert_eq!(hooks[0].node_id, node.0);
    assert_eq!(hook_name_of(&scene, hooks[0]), "half");
    assert_eq!(name_of(&scene, hooks[0]), "fadeIn");
}

/// 测 6：infinite + OnKey(.5)——下一 iteration 重新跨越 → 再发（fired_keys 按 iteration 清）。
#[test]
fn key_fires_again_in_next_iteration() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.duration = 1.0;
    s.iteration_count = None;
    let k = insert_player(&mut scene, node, s, opacity_fade());
    register_on_key(&mut scene, k, 0.5);
    let all = tick_n(&mut scene, 0.3, 8);
    // 跨越：t=0.6 (0.3→0.6)、t=1.5 (0.2→0.5)。t=1.2 是边界帧（0.9→0.2，新 iteration 从 0 起）。

    let keys = by_type(&all, EVT_ANIMATION_KEY);
    assert_eq!(keys.len(), 2, "每 iteration 各一次");
    assert_eq!(keys[0].y, 0.5);
    assert_eq!(keys[1].y, 0.5);
}

/// 测 7：OnKey(.5) 与 50% stop hook 同百分点——各自独立触发（fired_keys/fired_hooks 分离）。
#[test]
fn key_and_hook_same_percent_both_fire() {
    let (mut scene, node) = scene_with_1_node();
    let k = insert_player(&mut scene, node, spec(), hook_fade());
    register_on_key(&mut scene, k, 0.5);
    let all = tick_n(&mut scene, 0.1, 5);

    assert_eq!(by_type(&all, EVT_ANIMATION_KEY).len(), 1);
    assert_eq!(by_type(&all, EVT_ANIMATION_HOOK).len(), 1);
}

/// 测 8：reverse 动画首帧 prev 从 t=0 起点（directed=1.0）算——OnKey(.5) 不在首帧误发，
/// 在 directed 真正扫过 0.5（t=0.2）时发。
#[test]
fn reverse_no_spurious_key_on_first_frame() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.direction = AnimationDirection::Reverse;
    let k = insert_player(&mut scene, node, s, opacity_fade());
    register_on_key(&mut scene, k, 0.5);

    let t1 = tick(&mut scene, 0.1);
    assert!(
        by_type(&t1, EVT_ANIMATION_KEY).is_empty(),
        "首帧 directed=0.75 未扫过 0.5，不得误发"
    );
    let all = tick_n(&mut scene, 0.1, 3);
    let keys = by_type(&all, EVT_ANIMATION_KEY);
    assert_eq!(keys.len(), 1, "t=0.2 directed 扫过 0.5 时发");
}

/// 测 9：delay 期 progress 冻结不发 KEY；出 delay 帧从 t=0 起点检测。
#[test]
fn delay_frames_do_not_fire_keys() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.delay = 0.1;
    let k = insert_player(&mut scene, node, s, opacity_fade());
    register_on_key(&mut scene, k, 0.5);

    let all = tick_n(&mut scene, 0.1, 4);
    // t=0.1 出 delay（progress=0）；t=0.3 progress=0.5 跨越。
    let keys = by_type(&all, EVT_ANIMATION_KEY);
    assert_eq!(keys.len(), 1, "delay 期不误发，出 delay 后跨越才发");
    assert_eq!(by_type(&all, EVT_ANIMATION_START).len(), 1);
}

/// 测 10：register_on_key 重复注册同 pct 去重（不重复触发）。
#[test]
fn register_on_key_dedups() {
    let (mut scene, node) = scene_with_1_node();
    let mut s = spec();
    s.duration = 1.0;
    s.iteration_count = None;
    let k = insert_player(&mut scene, node, s, opacity_fade());
    register_on_key(&mut scene, k, 0.5);
    register_on_key(&mut scene, k, 0.5);
    let all = tick_n(&mut scene, 0.3, 8);

    assert_eq!(
        by_type(&all, EVT_ANIMATION_KEY).len(),
        2,
        "去重后每 iteration 仍各一次"
    );
}

/// 测 11：Paused 推进不产生任何事件（START 已发后）。
#[test]
fn paused_advance_emits_nothing() {
    let (mut scene, node) = scene_with_1_node();
    let k = insert_player(&mut scene, node, spec(), opacity_fade());
    let t1 = tick(&mut scene, 0.1);
    assert_eq!(by_type(&t1, EVT_ANIMATION_START).len(), 1);

    scene.players.get_mut(k).unwrap().play_state = PlayerPlayState::Paused;
    let rest = tick_n(&mut scene, 0.1, 2);
    assert!(rest.is_empty(), "Paused 帧不产任何事件");
}

/// 测 12：事件 payload 的 player_key 编码可还原（u64 拆 2×u32 → from_ffi 重建 = 原 key）。
#[test]
fn event_player_key_round_trips() {
    let (mut scene, node) = scene_with_1_node();
    let k = insert_player(&mut scene, node, spec(), opacity_fade());
    let all = tick_n(&mut scene, 0.1, 2);

    let start = by_type(&all, EVT_ANIMATION_START);
    assert!(!start.is_empty());
    let lo = start[0].touch_id as u32 as u64;
    let hi = start[0].x.to_bits() as u64;
    let decoded = player_key_from_u64((hi << 32) | lo);
    assert_eq!(decoded, k, "u64 编码还原 = 原 PlayerKey");
    assert!(
        scene.players.contains_key(decoded),
        "还原 key 在 players 表可查"
    );
}
