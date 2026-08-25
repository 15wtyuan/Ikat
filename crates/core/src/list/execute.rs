use super::plan::PendingOps;
use super::pool::{encode_reuse_key, reorder_active_slots};
use super::state::Slot;
use crate::scene::node::{NodeFlags, NodeId, Scene};

/// execute 阶段：复用 parked slot（翻 display + 换绑）或克隆扩容、标 LOOKUP_SCOPE + reuse_key +
/// 入队 pending_binds + 写 spacer 高度。只借 scene（直接调 scene::dynamic 建树函数，
/// 不经 Stage 包装——避免与 plan_visible 的 &mut Scene 借用冲突）。
pub fn execute_visible(scene: &mut Scene, ops: Vec<PendingOps>) {
    for op in ops {
        execute_one(scene, op);
    }
}

fn execute_one(scene: &mut Scene, op: PendingOps) {
    let (template_root, list_ordinal, tail_spacer) = {
        let ls = match scene.lists.get(op.list_ul) {
            Some(ls) => ls,
            None => return,
        };
        (ls.template_root, ls.list_ordinal, ls.tail_spacer)
    };
    let tpl = match template_root {
        Some(t) => t,
        None => return,
    };
    for item_index in &op.to_bind {
        // 优先复用 parked slot（留挂 ul，只翻 display + 换绑，零克隆零重建）；
        // 同 item 的 parked slot 最优（内容本就对得上），否则任取一个。
        let parked_pos = scene.lists.get(op.list_ul).and_then(|ls| {
            ls.slots
                .iter()
                .position(|s| s.parked && s.item_index == *item_index)
                .or_else(|| ls.slots.iter().position(|s| s.parked))
        });
        let node = match parked_pos {
            Some(pos) => {
                let node = {
                    let ls = scene.lists.get_mut(op.list_ul).unwrap();
                    let s = &mut ls.slots[pos];
                    s.parked = false;
                    s.item_index = *item_index;
                    s.node
                };
                // 清 display 便签（而非写 display:block）——cascade 回落作者真实 display。
                let _ = crate::scene::dynamic::unset_inline_override(scene, node, "display");
                node
            }
            // 无 parked 可用 → 克隆扩容（高水位只增）。
            None => {
                let node = crate::scene::dynamic::clone_node_recursive(scene, tpl);
                // clone_node_recursive 不复制 inline_override / inline_set——grown slot 从模板
                // 的"干净态"开始，无 display:none 泄漏风险（对比 unpark 路径复用 parked slot 时
                // 显式 unset_inline_override 清 display 便签）。
                // 标 LOOKUP_SCOPE（不打 SCOPE_ROOT：slot 根 CSS 规则仍按页面根 scope 匹配）。
                if let Some(n) = scene.get_mut(node) {
                    n.interaction.flags.insert(NodeFlags::LOOKUP_SCOPE);
                }
                // ordinal = 新 slot 在 slots 的下标（slots 只增不减 → key 出生即定、永不旋转）。
                let ordinal = scene
                    .lists
                    .get(op.list_ul)
                    .map(|ls| ls.slots.len())
                    .unwrap_or(0);
                crate::scene::dynamic::set_reuse_key(
                    scene,
                    node,
                    encode_reuse_key(list_ordinal, ordinal),
                );
                // append 到 tail_spacer 之前（head/tail spacer 始终首位）。
                let _ = crate::scene::dynamic::insert_before(scene, op.list_ul, node, tail_spacer);
                if let Some(ls) = scene.lists.get_mut(op.list_ul) {
                    ls.slots.push(Slot {
                        node,
                        item_index: *item_index,
                        parked: false,
                    });
                }
                node
            }
        };
        if let Some(ls) = scene.lists.get_mut(op.list_ul) {
            ls.pending_binds.push((node, *item_index));
        }
    }
    // active slot 在 ul.children 里的顺序就是视觉顺序（CSS 流在 head/tail spacer 之间排）。
    // unpark 是就地复用（不搬运节点），被复用的 slot 会停在旧位——故每帧末重排一次，
    // 保证 active slot 按 item_index 升序。
    reorder_active_slots(scene, op.list_ul);
    let (head, tail) = {
        let ls = scene.lists.get_mut(op.list_ul).unwrap();
        ls.visible = op.new_visible;
        (ls.head_spacer, ls.tail_spacer)
    };
    set_spacer_height(scene, head, op.spacer_head_h);
    set_spacer_height(scene, tail, op.spacer_tail_h);
}

/// 写 spacer 高度（base_style + style 同步，标 dirty_mesh 触发重布局）。
fn set_spacer_height(scene: &mut Scene, spacer: NodeId, h: f32) {
    if let Some(n) = scene.get_mut(spacer) {
        let d = taffy::style::Dimension::length(h);
        n.base_style.taffy_style.size.height = d;
        n.style.taffy_style.size.height = d;
        n.dirty_mesh = true;
    }
}
