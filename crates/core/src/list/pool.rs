use crate::scene::node::{NodeId, Scene};

/// reuse_key 编码：高 16 bit = list_ordinal+1（0 保留表“无 key”），低 16 bit = slot_idx。
/// 恒 ≠ 0（list_ordinal+1 ≥ 1）。场景级全局命名空间（同 ordinal 的 slot 跨帧复用）。
pub(super) fn encode_reuse_key(list_ordinal: u32, slot_idx: usize) -> u32 {
    ((list_ordinal + 1) << 16) | ((slot_idx as u32) & 0xFFFF)
}

/// 按 item_index 升序重排 ul.children 里的 **active** slot。
///
/// 池化模型下 slot 永不 detach，unpark 只翻 display + 换绑，节点会留在上次的物理位置。
/// 而 active slot 由 CSS 流在 head/tail spacer 之间依序排布——`ul.children` 顺序 **即视觉顺序**，
/// 不重排则被复用的 slot 渲染到错位（滞后的 item 翻到前面）。
///
/// parked slot（display:none）不占布局，物理位置任意——统一挡在 active 之后、tail spacer 之前。
/// head/tail spacer 位置不变（首子 / 末子）；非 slot 的意外子保序附在末尾。
/// 只改 `children` 排列，parent 不变（无需 remove_child/insert_before 的摘挂往返）。
pub(super) fn reorder_active_slots(scene: &mut Scene, ul: NodeId) {
    let (head, tail, active_rank, parked): (
        NodeId,
        NodeId,
        std::collections::HashMap<NodeId, usize>,
        std::collections::HashSet<NodeId>,
    ) = match scene.lists.get(ul) {
        Some(ls) => (
            ls.head_spacer,
            ls.tail_spacer,
            ls.slots
                .iter()
                .filter(|s| !s.parked)
                .map(|s| (s.node, s.item_index))
                .collect(),
            ls.slots
                .iter()
                .filter(|s| s.parked)
                .map(|s| s.node)
                .collect(),
        ),
        None => return,
    };
    let Some(ul_node) = scene.get_mut(ul) else {
        return;
    };
    let mut actives: Vec<NodeId> = Vec::with_capacity(active_rank.len());
    let mut parked_children: Vec<NodeId> = Vec::with_capacity(parked.len());
    let mut others: Vec<NodeId> = Vec::new();
    for &c in &ul_node.children {
        if c == head || c == tail {
            continue;
        }
        if active_rank.contains_key(&c) {
            actives.push(c);
        } else if parked.contains(&c) {
            parked_children.push(c);
        } else {
            others.push(c);
        }
    }
    // stable sort：同 item_index 的（不应出现）保持原相对序。
    actives.sort_by_key(|c| active_rank[c]);
    let mut new_children = Vec::with_capacity(ul_node.children.len());
    if ul_node.children.contains(&head) {
        new_children.push(head);
    }
    new_children.append(&mut actives);
    new_children.append(&mut parked_children);
    new_children.append(&mut others);
    if ul_node.children.contains(&tail) {
        new_children.push(tail);
    }
    ul_node.children = new_children;
}
