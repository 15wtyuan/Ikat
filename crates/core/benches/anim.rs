//! #9 动画并发基准：TweenManager 池化推进 + KeyframePlayer update_all 的稳态帧成本。
//!
//! 形状：N 节点各挂 1 tween + 1 keyframes player（opacity 0→1 循环），逐帧 update 推进。
//! 关注点：池化后 spawn/recycle 稳态零分配（tween churn 场景）+ 每 tick 推进的
//! 每节点成本。跑法：`cargo bench -p yio_core`；`cargo test` 不执行 bench（CI 零负担）。

// bench 是独立 crate，不吃 lib root 的 allow——此处同款放行（Node 字段多、default
// 起步只改两字段，struct literal 全字段列反而藏噪声）。
#![allow(clippy::field_reassign_with_default)]
use criterion::{criterion_group, criterion_main, Criterion};
use yio_core::scene::animation::{
    update_all, AnimatableProps, KeyframePlayer, KeyframeStop, KeyframeStopSelector, KeyframesRule,
};
use yio_core::scene::node::{Node, NodeKind, Rect, Scene};
use yio_core::style::resolved::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
};
use yio_core::tween::{Ease, TweenProp, TweenSpec};

fn spec() -> AnimationSpec {
    AnimationSpec {
        name: "fade".into(),
        duration: 1.0,
        delay: 0.0,
        iteration_count: None, // infinite：永不完成 → 稳态推进
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        timing_function: Ease::Linear,
        play_state: AnimationPlayState::Running,
    }
}

fn rule() -> KeyframesRule {
    KeyframesRule {
        name: "fade".into(),
        stops: vec![
            KeyframeStop {
                selector: KeyframeStopSelector::From,
                props: AnimatableProps {
                    opacity: Some(0.0),
                    ..Default::default()
                },
                timing: None,
                hook: None,
            },
            KeyframeStop {
                selector: KeyframeStopSelector::To,
                props: AnimatableProps {
                    opacity: Some(1.0),
                    ..Default::default()
                },
                timing: None,
                hook: None,
            },
        ],
    }
}

fn scene_with_players(n: usize) -> Scene {
    let mut nodes: Vec<Node> = Vec::with_capacity(n);
    for _ in 0..n {
        let mut node = Node::default();
        node.kind = NodeKind::Container;
        node.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        nodes.push(node);
    }
    let mut scene = Scene::from_nodes(nodes, vec![]);
    let r = rule();
    let s = spec();
    for id in scene.roots.clone() {
        let mut p = KeyframePlayer::new(id, s.clone(), r.clone());
        p.programmatic = true; // sync 语义无关本 bench（只测 update_all 推进）
        scene.players.insert(p);
    }
    scene
}

fn bench_update_all(c: &mut Criterion) {
    for n in [100usize, 1000] {
        let mut scene = scene_with_players(n);
        let mut group = c.benchmark_group(format!("anim/update_all/players={n}"));
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_function("advance_16ms", |b| {
            b.iter(|| {
                update_all(&mut scene, 0.016, &mut Vec::new());
            })
        });
        group.finish();
    }
}

fn bench_tween_update(c: &mut Criterion) {
    for n in [100usize, 1000] {
        let mut scene = scene_with_players(n); // 节点复用（tween 写同批节点）
        let mut mgr = yio_core::tween::TweenManager::new();
        for id in scene.roots.clone() {
            mgr.tween(
                id,
                TweenSpec {
                    prop: TweenProp::Opacity,
                    start: [0.0; 8],
                    end: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    ease: Ease::Linear,
                    delay: 0.0,
                    duration: 1e9, // 永不完成 → 稳态推进
                    tag: 0,
                    repeat: 0,
                    yoyo: false,
                    shadow: None,
                },
            );
        }
        let mut group = c.benchmark_group(format!("anim/tween_update/active={n}"));
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_function("advance_16ms", |b| {
            b.iter(|| {
                mgr.update(0.016, &mut scene, &mut Vec::new());
            })
        });
        group.finish();
    }
}

/// spawn→完成→回池→再 spawn 的 churn 稳态：池化后应无每轮分配（pool 命中）。
fn bench_tween_churn(c: &mut Criterion) {
    let (mut scene, _) = {
        let mut n = Node::default();
        n.kind = NodeKind::Container;
        n.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let scene = Scene::from_nodes(vec![n], vec![]);
        let id = scene.roots[0];
        (scene, id)
    };
    let id = scene.roots[0];
    let mut mgr = yio_core::tween::TweenManager::new();
    let mut group = c.benchmark_group("anim/tween_churn");
    group.bench_function("spawn_complete_recycle", |b| {
        b.iter(|| {
            mgr.tween(
                id,
                TweenSpec {
                    prop: TweenProp::Opacity,
                    start: [0.0; 8],
                    end: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    ease: Ease::Linear,
                    delay: 0.0,
                    duration: 0.016,
                    tag: 0,
                    repeat: 0,
                    yoyo: false,
                    shadow: None,
                },
            );
            mgr.update(0.016, &mut scene, &mut Vec::new()); // 完成即回池
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_update_all,
    bench_tween_update,
    bench_tween_churn
);
criterion_main!(benches);
