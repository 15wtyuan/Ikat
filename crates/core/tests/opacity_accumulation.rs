//! opacity 父级累积（spec §3.3，CSS opacity 语义：子整体乘父 alpha）。
//!
//! build_render_nodes 产出的 RenderNode.alpha 必须是「父累积 × 自身 own」，而非自身 alone：
//! 父节点动画 opacity（T6 player 写 NodeAnim.opacity，或 CSS style.opacity）时整子树必须跟着
//! 淡——否则父淡出、子仍全亮（pre-existing 行为缺陷，本测试锁定修正）。

use loomgui_core::render::build_render_nodes;
use loomgui_core::scene::node::{Node, NodeId, NodeKind, Rect, Scene};
use loomgui_core::scene::transform::compute_world_transforms;
use loomgui_core::text::atlas::GlyphAtlas;
use loomgui_core::text::layout::FontTable;

fn assert_close(a: f32, b: f32, msg: &str) {
    assert!((a - b).abs() < 1e-5, "{msg}: expected {b}, got {a}");
}

/// Container 节点（own opacity 由调用方在 style / anim 上设）。
/// id/parent 由 `Scene::from_nodes` 的 edges 推，不需预填。
fn container_node(rect: Rect) -> Node {
    Node {
        kind: NodeKind::Container,
        layout_rect: rect,
        ..Default::default()
    }
}

/// 建 frame（空字体表 / 空图集：纯 Container 树无文本、无图，无需真实资源）。
fn frame(scene: &mut Scene) -> loomgui_core::render::FrameData {
    compute_world_transforms(scene);
    let fonts = FontTable::new();
    build_render_nodes(
        scene,
        &fonts,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        &mut GlyphAtlas::new(),
    )
    .0
}

fn root_id(s: &Scene) -> NodeId {
    s.roots[0]
}

fn child_id(s: &Scene, parent: NodeId, idx: usize) -> NodeId {
    s.get(parent).unwrap().children[idx]
}

/// 断言这些 node_id 的所有 RenderNode alpha == 期望。容合批（同 DrawState 相邻节点
/// merge 后子 node_id 被锚吞）：节点集里至少一个存在、每个存在者都携带累积值。
fn assert_all_alphas(
    frame: &loomgui_core::render::FrameData,
    ids: &[u32],
    expected: f32,
    msg: &str,
) {
    let found: Vec<f32> = frame
        .nodes
        .iter()
        .filter(|rn| ids.contains(&rn.node_id))
        .map(|rn| rn.alpha)
        .collect();
    assert!(!found.is_empty(), "{msg}: 无这些 node_id 的 RenderNode");
    for a in found {
        assert_close(a, expected, msg);
    }
}

#[test]
fn child_accumulates_parent_anim_opacity() {
    // 父 CSS opacity=1.0，T6 player 写 anim.opacity=0.5（生产路径写法：anim.ensure）；
    // 子 own=1.0 → 累积：父 0.5（1.0×0.5），子 0.5（0.5×1.0）。
    let mut s = Scene::from_nodes(
        vec![
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            }),
        ],
        vec![(0, 1)],
    );
    let pid = root_id(&s);
    let cid = child_id(&s, pid, 0);
    s.anim.ensure(pid).opacity = Some(0.5);
    let frame = frame(&mut s);
    // 父与子累积相等（0.5）→ 同 DrawState，可能合批（子 node_id 并入父锚）；集合断言。
    assert_all_alphas(
        &frame,
        &[pid.0, cid.0],
        0.5,
        "父 anim.opacity=0.5 → 父与子 alpha 均 0.5（子吃父累积）",
    );
}

#[test]
fn child_accumulates_parent_style_opacity() {
    // 父 CSS style.opacity=0.5（无 anim，own 源 = style）、子 own=1.0 → 子 0.5。
    let mut s = Scene::from_nodes(
        vec![
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            }),
        ],
        vec![(0, 1)],
    );
    let pid = root_id(&s);
    let cid = child_id(&s, pid, 0);
    s.get_mut(pid).unwrap().style.opacity = 0.5;
    let frame = frame(&mut s);
    assert_all_alphas(
        &frame,
        &[pid.0, cid.0],
        0.5,
        "父 style.opacity=0.5 → 父与子 alpha 均 0.5",
    );
}

#[test]
fn parent_times_child_opacity() {
    // 父 0.5 × 子 0.4 = 0.2（两级都非 1）。
    // 子累积 0.2 ≠ 父 0.5 → 不同 DrawState，各自独立 RenderNode，按 node_id 直断。
    let mut s = Scene::from_nodes(
        vec![
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            }),
        ],
        vec![(0, 1)],
    );
    let pid = root_id(&s);
    let cid = child_id(&s, pid, 0);
    s.get_mut(pid).unwrap().style.opacity = 0.5;
    s.get_mut(cid).unwrap().style.opacity = 0.4;
    let frame = frame(&mut s);
    assert_all_alphas(&frame, &[pid.0], 0.5, "父 alpha = own 0.5");
    assert_all_alphas(&frame, &[cid.0], 0.2, "子 alpha = 0.5×0.4 = 0.2");
}

#[test]
fn unity_parent_opacity_degrades_to_own() {
    // 父 1.0 × 子 0.5 = 0.5（无父累积退化验证：父透明时不改变子表现）。
    let mut s = Scene::from_nodes(
        vec![
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            }),
        ],
        vec![(0, 1)],
    );
    let pid = root_id(&s);
    let cid = child_id(&s, pid, 0);
    s.get_mut(cid).unwrap().style.opacity = 0.5;
    let frame = frame(&mut s);
    assert_all_alphas(&frame, &[cid.0], 0.5, "父 1.0 → 子 alpha = own 0.5");
}

#[test]
fn deep_chain_accumulates_multiple_levels() {
    // root 1.0 → mid 0.5 → leaf 0.5 → leaf 0.25（逐层乘）。
    // root 1.0 / mid 0.5 / leaf 0.25 全不同 → 无合批。
    let mut s = Scene::from_nodes(
        vec![
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            }),
        ],
        vec![(0, 1), (1, 2)],
    );
    let rid = root_id(&s);
    let mid = child_id(&s, rid, 0);
    let leaf = child_id(&s, mid, 0);
    s.get_mut(mid).unwrap().style.opacity = 0.5;
    s.get_mut(leaf).unwrap().style.opacity = 0.5;
    let frame = frame(&mut s);
    assert_all_alphas(&frame, &[rid.0], 1.0, "root alpha = 1.0");
    assert_all_alphas(&frame, &[mid.0], 0.5, "mid alpha = 1.0×0.5");
    assert_all_alphas(&frame, &[leaf.0], 0.25, "leaf alpha = 0.5×0.5");
}

#[test]
fn siblings_accumulate_independently() {
    // 父 0.5：子A own 0.6 → 0.3；子B own 0.4 → 0.2（兄弟互不串扰）。
    // 父 0.5 / A 0.3 / B 0.2 全不同 → 无合批，按 node_id 直断。
    let mut s = Scene::from_nodes(
        vec![
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            }),
            container_node(Rect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 80.0,
            }),
            container_node(Rect {
                x: 100.0,
                y: 0.0,
                w: 80.0,
                h: 80.0,
            }),
        ],
        vec![(0, 1), (0, 2)],
    );
    let pid = root_id(&s);
    let a = child_id(&s, pid, 0);
    let b = child_id(&s, pid, 1);
    s.get_mut(pid).unwrap().style.opacity = 0.5;
    s.get_mut(a).unwrap().style.opacity = 0.6;
    s.get_mut(b).unwrap().style.opacity = 0.4;
    let frame = frame(&mut s);
    assert_all_alphas(&frame, &[a.0], 0.3, "子A alpha = 0.5×0.6");
    assert_all_alphas(&frame, &[b.0], 0.2, "子B alpha = 0.5×0.4");
}
