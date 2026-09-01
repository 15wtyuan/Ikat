//! Tree 复合控件（`role="tree"` / `role="treeitem"`，#8）。镜像 [`super::tablist`] 的全套
//! 机制（ControlState + synth aria + roving 键盘），差异在**嵌套**：treeitem 任意深度嵌套，
//! 条目集不是「直接子按 DOM 序」而是子树 DFS 序，且折叠的 branch 子树不可见。
//!
//! 语义对齐 WAI-ARIA APG Tree View（单选树）：
//! - 选中：单值，存 `ControlState::Tree{selected}`（NodeId——展开/折叠使可见序漂移，
//!   Node 身份稳定；对照 TabList 用 index 因 tab 是平层）。焦点移动即选中（APG 单选树
//!   缺省模型，无 manual 变体——TabList 的 data-activation 二态对树无先验，不发明）。
//! - 展开/折叠：branch 条目自身 `ControlState::TreeItem{expanded}`，折叠 = 对直接
//!   treeitem 子写 display:none（镜像 TabList panel 显隐）；leaf 无态。
//! - 键盘：Up/Down 可见项间移动、Right 展开/进首子项、Left 折叠/回父项、Home/End
//!   首末可见项、Enter/Space 激活（选中 + branch 折叠展开互切）。APG 核心档全收；
//!   typeahead 是 APG optional，defer（触发判据见 #8 票）。
//! - 条目身份判定用 NodeKind（fence 把 role=treeitem 烙成 kind，类型层真相），不走
//!   role 字符串查表（TabList 用 role_of 是因 Tab 无 ControlState 需旁证；TreeItem 有）。

use crate::input::{
    focus_node, EventRecord, EVT_EXPAND_CHANGED, EVT_SELECTION_CHANGED, KEY_DOWN, KEY_END,
    KEY_HOME, KEY_LEFT, KEY_RETURN, KEY_RIGHT, KEY_SPACE, KEY_UP,
};
use crate::scene::node::{ControlState, NodeId, NodeKind, Scene};

/// 从 `item` 沿父链上溯找最近的 Tree 容器节点。treeitem 到其 Tree 之间可能隔着任意
/// 深度的嵌套 treeitem；限深防环。item 不 live / 无 Tree 祖先 → None。
pub(crate) fn tree_owner(scene: &Scene, item: NodeId) -> Option<NodeId> {
    let mut cur = scene.get(item)?.parent?;
    for _ in 0..100_000 {
        let n = scene.get(cur)?;
        if n.kind == NodeKind::Tree {
            return Some(cur);
        }
        cur = n.parent?;
    }
    None
}

/// branch 判定：有直接 treeitem 子（展开/折叠语义只对 branch 成立；leaf 恒 false）。
fn is_branch(scene: &Scene, item: NodeId) -> bool {
    !direct_treeitem_children(scene, item).is_empty()
}

/// node 的直接 treeitem 子（children 先 clone 保证借用安全）。
fn direct_treeitem_children(scene: &Scene, node: NodeId) -> Vec<NodeId> {
    scene
        .get(node)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| scene.get(c).is_some_and(|n| n.kind == NodeKind::TreeItem))
        .collect()
}

/// 全量条目（先序 DFS 文档序，**含折叠隐藏的**）。bridge 烘焙
/// `ControlInit::Tree{selected_item}` 序号与 `resolve_tree_initial_selection` 解析共用
/// 本口径——两侧必须同步演化（bridge 侧见 packer bridge.rs tree 分支）。
pub fn tree_items_document_order(scene: &Scene, tree: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    walk_all(scene, tree, &mut out);
    out
}

fn walk_all(scene: &Scene, node: NodeId, out: &mut Vec<NodeId>) {
    let children = scene
        .get(node)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for c in children {
        if scene.get(c).is_some_and(|n| n.kind == NodeKind::TreeItem) {
            out.push(c);
            walk_all(scene, c, out);
        } else {
            // 非 treeitem 中间层（作者包裹容器）也下钻，保持与 bridge IR 遍历同口径。
            walk_all(scene, c, out);
        }
    }
}

/// 可见条目（先序 DFS，跳过折叠 branch 的子树）——键盘 roving / aria-selected 合成的
/// 「可见序」口径。展开 branch 的子树与 leaf 之外的中间层照常下钻。
pub(crate) fn visible_tree_items(scene: &Scene, tree: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    walk_visible(scene, tree, &mut out);
    out
}

fn walk_visible(scene: &Scene, node: NodeId, out: &mut Vec<NodeId>) {
    let children = scene
        .get(node)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for c in children {
        if scene.get(c).is_some_and(|n| n.kind == NodeKind::TreeItem) {
            out.push(c);
            let expanded = matches!(
                scene.controls.get(c),
                Some(ControlState::TreeItem { expanded: true })
            );
            if expanded {
                walk_visible(scene, c, out);
            }
        } else {
            walk_visible(scene, c, out);
        }
    }
}

/// item 是否为所属 Tree 的当前选中项（跨节点派生，与 aria-selected synth 同源）。
/// item 非 TreeItem / 无 Tree 祖先 → None。
pub fn tree_item_selected(scene: &Scene, item: NodeId) -> Option<bool> {
    if scene.get(item)?.kind != NodeKind::TreeItem {
        return None;
    }
    let owner = tree_owner(scene, item)?;
    match scene.controls.get(owner) {
        Some(ControlState::Tree { selected }) => Some(*selected == Some(item)),
        _ => None,
    }
}

/// item 的层级（ARIA aria-level：顶层条目 = 1）。数 item 到 Tree 根之间的 TreeItem
/// 祖先数 +1；无 Tree 祖先（detached）→ None。结构派生不存态（嵌套深度即层级）。
pub fn tree_item_level(scene: &Scene, item: NodeId) -> Option<u32> {
    if scene.get(item)?.kind != NodeKind::TreeItem {
        return None;
    }
    let mut level = 1u32;
    let mut cur = scene.get(item)?.parent?;
    for _ in 0..100_000 {
        let n = scene.get(cur)?;
        if n.kind == NodeKind::Tree {
            return Some(level);
        }
        if n.kind == NodeKind::TreeItem {
            level += 1;
        }
        cur = n.parent?;
    }
    None
}

/// 设 Tree 选中项。仅净变才发 `EVT_SELECTION_CHANGED`@tree（镜像 set_tablist_selected_index
/// 的 HTML change 语义：重复点已选条目不发）。payload touch_id=0（事件只作「变了」信号，
/// 选中身份由 FFI get_tree_selected 读取——NodeId 64 位装不进 i32，不塞私货）。
/// tree 非 Tree 控件态 → no-op（防御）。
pub(super) fn set_tree_selected(
    scene: &mut Scene,
    tree: NodeId,
    item: NodeId,
    out: &mut Vec<EventRecord>,
) {
    let changed = matches!(
        scene.controls.get(tree),
        Some(ControlState::Tree { selected: cur }) if *cur != Some(item)
    );
    if let Some(ControlState::Tree { selected }) = scene.controls.get_mut(tree) {
        *selected = Some(item);
    }
    if changed {
        out.push(EventRecord {
            node_id: tree.0,
            event_type: EVT_SELECTION_CHANGED,
            click_count: 0,
            pad: [0, 0],
            touch_id: 0,
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
        });
    }
}

/// 设 branch 条目展开态。仅净变才发 `EVT_EXPAND_CHANGED`@item（touch_id=新态 1/0）。
/// leaf / 非控件态 → no-op。显隐剪枝在 sync_control_visuals 的 TreeItem 臂（本函数只改态）。
pub(super) fn set_treeitem_expanded(
    scene: &mut Scene,
    item: NodeId,
    expanded: bool,
    out: &mut Vec<EventRecord>,
) {
    let changed = matches!(
        scene.controls.get(item),
        Some(ControlState::TreeItem { expanded: cur }) if *cur != expanded
    );
    if let Some(ControlState::TreeItem { expanded: e }) = scene.controls.get_mut(item) {
        *e = expanded;
    }
    if changed {
        out.push(EventRecord {
            node_id: item.0,
            event_type: EVT_EXPAND_CHANGED,
            click_count: 0,
            pad: [0, 0],
            touch_id: expanded as i32,
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
        });
    }
}

/// 激活条目（click 与 Enter/Space 共用）：选中 + branch 折叠/展开互切（APG：branch 的
/// 激活行为 = 切换展开态，文件树惯例）。leaf 只选中。
pub(super) fn activate_tree_item(scene: &mut Scene, item: NodeId, out: &mut Vec<EventRecord>) {
    let Some(owner) = tree_owner(scene, item) else {
        return;
    };
    set_tree_selected(scene, owner, item, out);
    if is_branch(scene, item) {
        let next = !matches!(
            scene.controls.get(item),
            Some(ControlState::TreeItem { expanded: true })
        );
        set_treeitem_expanded(scene, item, next, out);
    }
}

/// instantiate 后置遍：把 `ControlInit::Tree{selected_item}`（bridge 侧先序文档序序号）
/// 解析成 NodeId 写入 ControlState::Tree.selected。空树保持 None；序号越界 clamp 到
/// 末项（bridge 产合法序号，防御动态改 pkg 的输入）。不发事件（初值不是交互）。
pub(crate) fn resolve_tree_initial_selection(scene: &mut Scene, tree: NodeId) {
    let Some(crate::asset::ControlInit::Tree { selected_item }) =
        scene.control_inits.get(&tree).cloned()
    else {
        return;
    };
    let items = tree_items_document_order(scene, tree);
    let picked = items
        .get((selected_item as usize).min(items.len().saturating_sub(1)))
        .copied();
    if let Some(ControlState::Tree { selected }) = scene.controls.get_mut(tree) {
        *selected = picked;
    }
}

/// Tree 键盘路由（APG Tree View 核心档）。返回是否消费该键（消费 → 不发普通 keydown）。
/// 前置：焦点恰在某 treeitem 上（input.rs 路由入口保证；roving tabindex 模型里焦点
/// 落在条目上，treeitem 后代内嵌控件持有按键时本路由不触发——同 TabList 的隔离哲学，
/// 但 Tree 用「焦点自身是 treeitem」精确判，不用祖先链（树条目嵌套深，祖先链会误吞
/// 内嵌控件的键）。
///
/// - Up/Down：可见项间移动焦点 + 选中（clamp 不 wrap；边缘处键仍消费、净变为零不发事件）。
/// - Right：折叠 branch → 展开；已展开 branch → 焦点/选中进首个子项；leaf → 无操作（消费）。
/// - Left：已展开 branch → 折叠；折叠 branch/leaf → 焦点/选中回父条目；顶层 → 无操作（消费）。
/// - Home/End：首个/末个可见项。
/// - Enter/Space：激活（选中 + branch 互切展开）——见 [`activate_tree_item`]。
///
/// 焦点不在本 tree 的 treeitem / 无条目 → false（调用方走普通 keydown）。方向键由
/// collector 的 key repeat（#76）合成连发，本路由逐发步进——长按方向键在树里连续走。
pub(crate) fn on_tree_key(
    scene: &mut Scene,
    tree: NodeId,
    key_code: u32,
    out: &mut Vec<EventRecord>,
) -> bool {
    // 路由键集合之外的键不消费（普通字符键等透传给条目内容，如将来的 typeahead 入口）。
    let routed = matches!(
        key_code,
        KEY_UP | KEY_DOWN | KEY_LEFT | KEY_RIGHT | KEY_HOME | KEY_END | KEY_RETURN | KEY_SPACE
    );
    if !routed {
        return false;
    }
    let Some(focused) = scene.focused_node else {
        return false;
    };
    // 焦点必须是本 tree 子树内的 treeitem（input.rs 已保证；此处再守一道防误调）。
    if scene.get(focused).map(|n| n.kind) != Some(NodeKind::TreeItem)
        || tree_owner(scene, focused) != Some(tree)
    {
        return false;
    }
    let items = visible_tree_items(scene, tree);
    let Some(pos) = items.iter().position(|&i| i == focused) else {
        return false; // 焦点条目不可见（被折叠隐藏的瞬态）→ 不路由
    };
    // 移动到目标条目：焦点 + 选中同步（APG 单选树焦点跟随模型）。
    let move_to = |scene: &mut Scene, target: NodeId, out: &mut Vec<EventRecord>| {
        set_tree_selected(scene, tree, target, out);
        focus_node(scene, Some(target), out);
    };
    match key_code {
        KEY_UP => {
            if let Some(&t) = pos.checked_sub(1).and_then(|p| items.get(p)) {
                move_to(scene, t, out);
            }
            true
        }
        KEY_DOWN => {
            if let Some(&t) = items.get(pos + 1) {
                move_to(scene, t, out);
            }
            true
        }
        KEY_HOME => {
            if let Some(&t) = items.first() {
                move_to(scene, t, out);
            }
            true
        }
        KEY_END => {
            if let Some(&t) = items.last() {
                move_to(scene, t, out);
            }
            true
        }
        KEY_RIGHT => {
            if is_branch(scene, focused) {
                if matches!(
                    scene.controls.get(focused),
                    Some(ControlState::TreeItem { expanded: true })
                ) {
                    // 已展开：进首个子项（直接 treeitem 子的第一个）。
                    if let Some(child) = direct_treeitem_children(scene, focused).first().copied() {
                        move_to(scene, child, out);
                    }
                } else {
                    set_treeitem_expanded(scene, focused, true, out);
                }
            }
            true
        }
        KEY_LEFT => {
            if is_branch(scene, focused)
                && matches!(
                    scene.controls.get(focused),
                    Some(ControlState::TreeItem { expanded: true })
                )
            {
                set_treeitem_expanded(scene, focused, false, out);
            } else {
                // 折叠态/leaf：回父条目（顶层条目的父是 Tree 容器 → 无操作）。
                let parent_item = scene
                    .get(focused)
                    .and_then(|n| n.parent)
                    .filter(|&p| scene.get(p).is_some_and(|n| n.kind == NodeKind::TreeItem));
                if let Some(p) = parent_item {
                    move_to(scene, p, out);
                }
            }
            true
        }
        // Enter/Space：激活。
        KEY_RETURN | KEY_SPACE => {
            activate_tree_item(scene, focused, out);
            true
        }
        _ => unreachable!("routed set checked above"),
    }
}
