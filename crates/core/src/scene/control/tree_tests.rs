//! Tree 控件（role=tree/treeitem，#8）单测：可见/文档序、选中派生、折叠剪枝、
//! APG 核心档键盘、激活（click/Enter 共用）。脚手架复刻 tablist 测试的
//! create_node_from_template + controls.ensure 路径（与 instantiate 产物同构）。

use super::*;
use crate::input::{
    EVT_EXPAND_CHANGED, EVT_SELECTION_CHANGED, KEY_DOWN, KEY_END, KEY_HOME, KEY_LEFT, KEY_RETURN,
    KEY_RIGHT, KEY_UP,
};
use crate::scene::dynamic::{append_child, create_node_from_template};
use crate::scene::node::{ControlState, NodeId, NodeKind, Scene};
use crate::style::resolved::ResolvedStyle;

/// 建测试树：tree > branchA(expanded) > [leafA1, leafA2]、branchB(collapsed) > [leafB1]。
/// branch 带 ControlState::TreeItem{expanded}，leaf 无控件态（与 instantiate 产物同构：
/// 打包期 branch 判定 = 有直接 treeitem 子，leaf 无 init）。
/// 返回 (tree, branch_a, leaf_a1, leaf_a2, branch_b, leaf_b1)。
fn make_tree(scene: &mut Scene) -> (NodeId, NodeId, NodeId, NodeId, NodeId, NodeId) {
    let mk = |scene: &mut Scene, kind: NodeKind| {
        create_node_from_template(scene, kind, ResolvedStyle::default(), None)
    };
    // 镜像 bridge 烘焙行为：全部 treeitem 持 TreeItem 态（leaf expanded 恒 false）。
    let ensure_item = |scene: &mut Scene, id: NodeId, expanded: bool| {
        scene
            .controls
            .ensure(id, ControlState::TreeItem { expanded });
    };
    let tree = mk(scene, NodeKind::Tree);
    scene
        .controls
        .ensure(tree, ControlState::Tree { selected: None });

    let branch_a = mk(scene, NodeKind::TreeItem);
    append_child(scene, tree, branch_a).unwrap();
    ensure_item(scene, branch_a, true);
    let leaf_a1 = mk(scene, NodeKind::TreeItem);
    append_child(scene, branch_a, leaf_a1).unwrap();
    ensure_item(scene, leaf_a1, false);
    let leaf_a2 = mk(scene, NodeKind::TreeItem);
    append_child(scene, branch_a, leaf_a2).unwrap();
    ensure_item(scene, leaf_a2, false);

    let branch_b = mk(scene, NodeKind::TreeItem);
    append_child(scene, tree, branch_b).unwrap();
    ensure_item(scene, branch_b, false);
    let leaf_b1 = mk(scene, NodeKind::TreeItem);
    append_child(scene, branch_b, leaf_b1).unwrap();
    ensure_item(scene, leaf_b1, false);
    (tree, branch_a, leaf_a1, leaf_a2, branch_b, leaf_b1)
}

#[test]
fn tree_leaf_hit_refines_to_leaf_not_parent_branch() {
    // 回归（#8）：leaf treeitem 无态时代，find_control_at 从叶子 label 上溯会跳过
    // 叶子落到父 branch——指针点叶子 = 激活父分组（选中父 + 误折叠）。全部条目持态
    // 后叶子命中自身。synth 侧的 branch-only 守卫由 style::dynamic 测试覆盖
    // （synth_treeitem_expanded_branch_only）。
    let mut scene = Scene::default();
    let (_tree, branch_a, leaf_a1, ..) = make_tree(&mut scene);
    // 叶子 label 子（非 treeitem 容器）上溯 → 必须是叶子自身。
    let label = create_node_from_template(
        &mut scene,
        NodeKind::Container,
        ResolvedStyle::default(),
        None,
    );
    append_child(&mut scene, leaf_a1, label).unwrap();
    assert_eq!(find_control_at(&scene, Some(label)), Some(leaf_a1));
    // branch 判定结构口径不因叶子持态而变。
    assert!(!is_branch(&scene, leaf_a1));
    assert!(is_branch(&scene, branch_a));
}

#[test]
fn tree_visible_and_document_order_items() {
    let mut scene = Scene::default();
    let (tree, branch_a, leaf_a1, leaf_a2, branch_b, leaf_b1) = make_tree(&mut scene);
    // 文档序（先序 DFS，含折叠隐藏的）：A, a1, a2, B, b1——bridge 烘焙初始选中序号的口径。
    assert_eq!(
        tree_items_document_order(&scene, tree),
        vec![branch_a, leaf_a1, leaf_a2, branch_b, leaf_b1]
    );
    // 可见序（branchB 折叠，子树跳过）：A, a1, a2, B——键盘 roving 的口径。
    assert_eq!(
        visible_tree_items(&scene, tree),
        vec![branch_a, leaf_a1, leaf_a2, branch_b]
    );
}

#[test]
fn tree_selection_sets_and_derives_aria() {
    let mut scene = Scene::default();
    let (tree, branch_a, leaf_a1, ..) = make_tree(&mut scene);
    let mut out = Vec::new();
    set_tree_selected(&mut scene, tree, leaf_a1, &mut out);
    assert_eq!(
        out.iter()
            .filter(|e| e.event_type == EVT_SELECTION_CHANGED)
            .count(),
        1,
        "净变选中 → 发 SelectionChanged@tree"
    );
    assert_eq!(tree_item_selected(&scene, leaf_a1), Some(true));
    assert_eq!(tree_item_selected(&scene, branch_a), Some(false));
    // 重复设同项：净变为零不发（HTML change 语义，镜像 tablist）。
    out.clear();
    set_tree_selected(&mut scene, tree, leaf_a1, &mut out);
    assert!(out.is_empty(), "重复选中不发事件");
}

#[test]
fn tree_collapse_prunes_and_expand_restores_display() {
    let mut scene = Scene::default();
    let (_, branch_a, leaf_a1, leaf_a2, ..) = make_tree(&mut scene);
    // branchA 展开 → 子无 display 覆写（bit 清，回落作者 CSS）。
    sync_control_visuals(&mut scene, branch_a, 0.0);
    assert_eq!(
        scene.get(leaf_a1).unwrap().inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
        0,
        "expanded → 不覆写 display"
    );
    // 折叠 → 直接 treeitem 子 display:none（嵌套深层随 display 剪枝语义走）。
    scene
        .controls
        .ensure(branch_a, ControlState::TreeItem { expanded: false });
    sync_control_visuals(&mut scene, branch_a, 0.0);
    for leaf in [leaf_a1, leaf_a2] {
        assert_eq!(
            scene.get(leaf).unwrap().inline_override.taffy_style.display,
            taffy::Display::None,
            "collapsed → 直接子 display:none"
        );
    }
    // 再展开 → 清覆写（bit 清，回落作者 CSS）。
    scene
        .controls
        .ensure(branch_a, ControlState::TreeItem { expanded: true });
    sync_control_visuals(&mut scene, branch_a, 0.0);
    assert_eq!(
        scene.get(leaf_a1).unwrap().inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
        0,
        "re-expanded → 覆写 bit 清、子可见"
    );
}

#[test]
fn tree_activate_selects_and_toggles_branch() {
    let mut scene = Scene::default();
    let (tree, branch_a, leaf_a1, ..) = make_tree(&mut scene);
    let mut out = Vec::new();
    activate_tree_item(&mut scene, branch_a, &mut out);
    // 激活 branch：选中 + 展开互切（true → false）+ 各发一次事件。
    assert_eq!(tree_item_selected(&scene, branch_a), Some(true));
    assert!(matches!(
        scene.controls.get(branch_a),
        Some(ControlState::TreeItem { expanded: false })
    ));
    assert_eq!(
        out.iter()
            .filter(|e| e.node_id == branch_a.0 && e.event_type == EVT_EXPAND_CHANGED)
            .count(),
        1,
        "branch 激活发一次 ExpandChanged@item"
    );
    // leaf 激活：只选中，无展开事件。
    out.clear();
    activate_tree_item(&mut scene, leaf_a1, &mut out);
    assert_eq!(tree_item_selected(&scene, leaf_a1), Some(true));
    assert!(
        out.iter()
            .filter(|e| e.event_type == EVT_EXPAND_CHANGED)
            .count()
            == 0,
        "leaf 激活不发 ExpandChanged"
    );
    let _ = tree;
}

#[test]
fn tree_keyboard_navigation_apg_core() {
    let mut scene = Scene::default();
    let (tree, branch_a, leaf_a1, leaf_a2, branch_b, leaf_b1) = make_tree(&mut scene);
    // 初始：焦点+选中在 branchA（roving tabindex：焦点在条目上）。
    scene.focused_node = Some(branch_a);
    scene.controls.ensure(
        tree,
        ControlState::Tree {
            selected: Some(branch_a),
        },
    );
    let mut out = Vec::new();
    // Down → leafA1（焦点+选中同步，APG 单选树焦点跟随模型）。
    assert!(on_tree_key(&mut scene, tree, KEY_DOWN, &mut out));
    assert_eq!(scene.focused_node, Some(leaf_a1));
    assert_eq!(tree_item_selected(&scene, leaf_a1), Some(true));
    // Home → 首个可见项（branchA）。
    assert!(on_tree_key(&mut scene, tree, KEY_HOME, &mut out));
    assert_eq!(scene.focused_node, Some(branch_a));
    // End → 末个可见项（branchB——折叠子树不进可见序）。
    assert!(on_tree_key(&mut scene, tree, KEY_END, &mut out));
    assert_eq!(scene.focused_node, Some(branch_b));
    // Right：branchB 折叠 → 展开（焦点不动）。
    assert!(on_tree_key(&mut scene, tree, KEY_RIGHT, &mut out));
    assert!(matches!(
        scene.controls.get(branch_b),
        Some(ControlState::TreeItem { expanded: true })
    ));
    assert_eq!(
        scene.focused_node,
        Some(branch_b),
        "折叠 branch 的 Right 只展开不移焦点"
    );
    // Right 再按：已展开 → 焦点/选中进首个子项。
    assert!(on_tree_key(&mut scene, tree, KEY_RIGHT, &mut out));
    assert_eq!(scene.focused_node, Some(leaf_b1));
    // Up → 回 branchB；Left：已展开 → 折叠。
    assert!(on_tree_key(&mut scene, tree, KEY_UP, &mut out));
    assert_eq!(scene.focused_node, Some(branch_b));
    assert!(on_tree_key(&mut scene, tree, KEY_LEFT, &mut out));
    assert!(matches!(
        scene.controls.get(branch_b),
        Some(ControlState::TreeItem { expanded: false })
    ));
    // Left 再按：折叠态 → 回父条目；branchB 是顶层（父是 Tree 容器）→ 无操作。
    assert!(on_tree_key(&mut scene, tree, KEY_LEFT, &mut out));
    assert_eq!(scene.focused_node, Some(branch_b));
    // Enter：激活（选中 + 展开）。
    assert!(on_tree_key(&mut scene, tree, KEY_RETURN, &mut out));
    assert!(matches!(
        scene.controls.get(branch_b),
        Some(ControlState::TreeItem { expanded: true })
    ));
    // Home + Down×2 → leafA2（可见序尾部可达性）。
    assert!(on_tree_key(&mut scene, tree, KEY_HOME, &mut out));
    assert!(on_tree_key(&mut scene, tree, KEY_DOWN, &mut out));
    assert!(on_tree_key(&mut scene, tree, KEY_DOWN, &mut out));
    assert_eq!(scene.focused_node, Some(leaf_a2));
}

#[test]
fn tree_keyboard_ignores_focus_not_on_treeitem() {
    let mut scene = Scene::default();
    let (tree, ..) = make_tree(&mut scene);
    // 焦点在 Tree 容器（非条目）→ 不路由（内嵌控件拥有按键的隔离哲学）。
    scene.focused_node = Some(tree);
    let mut out = Vec::new();
    assert!(!on_tree_key(&mut scene, tree, KEY_DOWN, &mut out));
}

#[test]
fn tree_item_level_counts_nesting() {
    let mut scene = Scene::default();
    let (tree, branch_a, leaf_a1, ..) = make_tree(&mut scene);
    assert_eq!(tree_item_level(&scene, branch_a), Some(1), "顶层 = 1");
    assert_eq!(tree_item_level(&scene, leaf_a1), Some(2), "二层 = 2");
    assert_eq!(tree_item_level(&scene, tree), None, "Tree 容器无 level");
}

#[test]
fn tree_resolve_initial_selection_ordinal() {
    let mut scene = Scene::default();
    let (tree, branch_a, leaf_a1, _leaf_a2, _branch_b, _leaf_b1) = make_tree(&mut scene);
    // 文档序 [branch_a, leaf_a1, leaf_a2, branch_b, leaf_b1]：序号 1 → leaf_a1。
    scene
        .control_inits
        .insert(tree, crate::asset::ControlInit::Tree { selected_item: 1 });
    resolve_tree_initial_selection(&mut scene, tree);
    assert_eq!(
        tree_item_selected(&scene, leaf_a1),
        Some(true),
        "文档序序号 1 解析成 leaf_a1 并置选中"
    );
    assert_eq!(tree_item_selected(&scene, branch_a), Some(false));
    // 序号越界 → clamp 末项（防御动态输入）。
    scene
        .control_inits
        .insert(tree, crate::asset::ControlInit::Tree { selected_item: 99 });
    resolve_tree_initial_selection(&mut scene, tree);
    assert_eq!(
        tree_item_selected(&scene, _leaf_b1),
        Some(true),
        "越界序号 clamp 到文档序末项"
    );
    // 无 ControlInit（防御）→ 不动。
    scene.control_inits.remove(&tree);
    resolve_tree_initial_selection(&mut scene, tree);
    assert_eq!(tree_item_selected(&scene, _leaf_b1), Some(true));
}
