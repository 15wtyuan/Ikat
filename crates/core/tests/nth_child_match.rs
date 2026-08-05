//! `:nth-child` 运行时匹配（spec §8.5）：节点在父 `children` 的 1-based index `i`，
//! `a == 0` 时 `i == b`，否则 `(i - b) % a == 0 && (i - b) / a >= 0`。
//!
//! 选择器手构（core 不依赖 parse feature，测试与 runtime 同路径走 compound_matches_node）。

use loomgui_core::scene::{Node, NodeId, NodeKind, Scene};
use loomgui_core::style::dynamic::{
    compound_matches_node, Combinator, Compound, NthChildExpr, ParsedSelector, Specificity,
};

/// 手构一个仅含 `:nth-child(An+B)` 的 compound selector。
fn nth_selector(a: i32, b: i32) -> ParsedSelector {
    ParsedSelector {
        raw: format!(":nth-child({a}n+{b})"),
        compound: vec![Compound {
            tag: None,
            classes: Vec::new(),
            id: None,
            combinator: Combinator::Descendant,
            pseudo_hover: false,
            pseudo_active: false,
            pseudo_disabled: false,
            pseudo_focus: false,
            pseudo_nth_child: Some(NthChildExpr { a, b }),
            attrs: Vec::new(),
        }],
        specificity: Specificity(0, 1, 0),
    }
}

/// 空 Container 节点（struct update 语法，避开 field_reassign_with_default）。
fn container() -> Node {
    Node {
        kind: NodeKind::Container,
        ..Default::default()
    }
}

/// root 下挂 3 个 div 子节点。返 (scene, 3 个子 NodeId)。
fn scene_with_3_children() -> (Scene, Vec<NodeId>) {
    let nodes: Vec<Node> = std::iter::once(container())
        .chain((0..3).map(|_| container()))
        .collect();
    let edges: Vec<(usize, usize)> = (1..=3).map(|i| (0, i)).collect();
    let scene = Scene::from_nodes(nodes, edges);
    let root = scene.get(scene.roots[0]).expect("root");
    let kids: Vec<NodeId> = (0..3).map(|i| root.children[i]).collect();
    (scene, kids)
}

/// 断言 `sel` 对 3 子节点的命中集合（按 1-based 序）。
fn assert_matches(sel: &ParsedSelector, expect: &[usize]) {
    let (scene, kids) = scene_with_3_children();
    let actual: Vec<usize> = (0..3)
        .filter(|&i| compound_matches_node(&sel.compound[0], kids[i], &scene))
        .map(|i| i + 1)
        .collect();
    assert_eq!(actual, expect, "sel {:?}", sel.raw);
}

#[test]
fn nth_child_integer_matches_exact_position() {
    // :nth-child(2) = 0n+2 → 仅第 2 个子节点
    assert_matches(&nth_selector(0, 2), &[2]);
    assert_matches(&nth_selector(0, 3), &[3]);
    assert_matches(&nth_selector(0, 4), &[]); // 越界无命中
}

#[test]
fn nth_child_odd_even() {
    // odd = 2n+1 → 1/3；even = 2n → 2
    assert_matches(&nth_selector(2, 1), &[1, 3]);
    assert_matches(&nth_selector(2, 0), &[2]);
}

#[test]
fn nth_child_an_plus_b() {
    // 2n+1 = odd；n = 全部；2n-1 = 1/3
    assert_matches(&nth_selector(2, 1), &[1, 3]);
    assert_matches(&nth_selector(1, 0), &[1, 2, 3]);
    assert_matches(&nth_selector(2, -1), &[1, 3]);
    assert_matches(&nth_selector(3, 1), &[1]); // 3n+1 → 1,4,7...
    assert_matches(&nth_selector(2, 5), &[]); // 起点超出 → 无命中
}

#[test]
fn nth_child_root_node_never_matches() {
    // 根节点无父 → 不匹配任何 :nth-child（spec §8.5）
    let sel = nth_selector(0, 1);
    let (scene, _) = scene_with_3_children();
    assert!(!compound_matches_node(
        &sel.compound[0],
        scene.roots[0],
        &scene
    ));
    let sel_all = nth_selector(1, 0); // n 全匹配也排除根
    assert!(!compound_matches_node(
        &sel_all.compound[0],
        scene.roots[0],
        &scene
    ));
}
