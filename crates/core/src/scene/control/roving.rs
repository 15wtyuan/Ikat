//! roving tabindex 通用积木：复合控件（TabList、将来的 Tree）方向键移动焦点的共享机制。
//!
//! WAI-ARIA roving tabindex：方向键在复合控件的 item（tab / treeitem）之间移动**焦点**；
//! 选中是否跟随由控件的激活模型决定（TabList 的 automatic / manual，见 ControlState）。
//! 焦点是唯一真相——种子取 scene.focused_node 落在本复合控件 item 集内的那个 item，
//! 焦点不在本控件内（首次按方向键 / 焦点在控件后代非 item 节点上）回落控件的
//! selected 项。激活模型分支归调用方（on_tablist_key / 将来的 Tree 键盘路由）。

use crate::scene::node::{NodeId, Scene};

/// 复合控件的 item 集：role 匹配的直接子按 DOM 序（与 aria-selected 合成 /
/// sync_control_visuals 同口径）。TabList 传 role=tab；Tree 复用时换自己的 role 常量。
pub(crate) fn roving_items(scene: &Scene, composite: NodeId, item_role: &str) -> Vec<NodeId> {
    scene
        .get(composite)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| scene.roles.role_of(c) == Some(item_role))
        .collect()
}

/// roving 步进：从 seed 沿 items 步进 delta，clamp 到 `[0, len-1]` **不 wrap**
///（对齐 Web tabs/tree 模式：到头停住，不回卷）。边缘处步进折返自身（焦点不动）。
/// items 空 → None。
pub(crate) fn roving_step(items: &[NodeId], seed: usize, delta: i64) -> Option<NodeId> {
    if items.is_empty() {
        return None;
    }
    let target = (seed as i64 + delta).max(0).min(items.len() as i64 - 1) as usize;
    items.get(target).copied()
}
