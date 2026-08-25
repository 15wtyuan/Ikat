use super::viewport::ancestor_pane;
use crate::scene::node::{NodeId, Scene};

/// 每帧 solve 后、refresh_content_sizes 前调：把已实例化 slot 的真实 margin-box 高
/// 回填 HeightCache，并按 head 区间总高变化做 scroll anchoring 补偿。
///
/// **margin box**：`layout_rect.h` 是 border box，不含 margin。`li { margin-bottom:8px }`
/// 极常见，漏计会让 spacer 求和系统性偏小、anchoring delta 跟着偏 → 滚回头漂移。
/// 故 `height_of(i) = layout_rect.h + margin_top + margin_bottom`。
///
/// **scroll anchoring**：本帧回填（含 recompute_estimate）若改变 `visible.start` 之前
/// 区间（head spacer 覆盖范围）的高度总和，delta≠0 → 同帧把祖先 ScrollPane.scroll_pos.y
/// += delta（用户视角内容不动，滚动条长度悄然修正）。补偿点 solve 之后、refresh_content_sizes
/// 之前；scroll_pos 只被 compute_world_transforms 消费，不触发二次 solve。
/// anchoring_active 标记本帧是否发生补偿，供 refresh_content_sizes 的 clamp 分支豁免清 tween。
pub fn collect_heights(scene: &mut Scene) {
    // 快照各 list 的 (slot_node, item_index) + head 区间旧总和 + 祖先 pane。
    // 在回填前取 old_head_sum（sum 借 heights 不可变；set 借可变——先快照再循环写）。
    let lists: Vec<(NodeId, Vec<(NodeId, usize)>, f32, Option<NodeId>)> = scene
        .lists
        .0
        .iter()
        .map(|(ul, ls)| {
            // 跳过 parked slot：display:none → layout_rect.h 恒 0，且 item_index 是 stale
            // 复用参考——快照它会把 0.0 盖到别的（可能可见的）item 的缓存高度上。
            let slots: Vec<(NodeId, usize)> = ls
                .slots
                .iter()
                .filter(|s| !s.parked)
                .map(|s| (s.node, s.item_index))
                .collect();
            let old_head_sum = ls.heights.sum(0..ls.visible.start);
            let pane = ancestor_pane(scene, *ul);
            (*ul, slots, old_head_sum, pane)
        })
        .collect();
    for (ul, slots, old_head_sum, pane) in lists {
        // 回填前重置 anchoring_active（反映「本帧」是否补偿）。
        if let Some(ls) = scene.lists.get_mut(ul) {
            ls.anchoring_active = false;
        }
        for (node, idx) in slots {
            // margin box = border box h + margin top + bottom（解析后的 px 值）。
            // LengthPercentageAuto 的 Auto 分支返 0（auto margin 在 flex 主轴由布局决定，
            // slot 已 solve 完，layout_rect.h 不含 auto margin 的贡献——auto margin 对
            // 列表高度无影响，按 0 计与 CSS 块/flex 流一致）。
            let h = scene
                .get(node)
                .map(|n| {
                    let ts = &n.base_style.taffy_style;
                    n.layout_rect.h
                        + resolve_margin_px(ts.margin.top)
                        + resolve_margin_px(ts.margin.bottom)
                })
                .unwrap_or(0.0);
            if let Some(ls) = scene.lists.get_mut(ul) {
                ls.heights.set(idx, h);
            }
        }
        // anchoring：回填后 head 区间总高变化 → 补偿祖先 pane scroll_pos.y。
        let (new_head_sum, visible_start) = match scene.lists.get(ul) {
            Some(ls) => (ls.heights.sum(0..ls.visible.start), ls.visible.start),
            None => continue,
        };
        let delta = new_head_sum - old_head_sum;
        if delta.abs() > 0.001 && visible_start > 0 {
            if let Some(pane) = pane {
                if let Some(st) = scene.scroll.get_mut(pane) {
                    st.scroll_pos.1 += delta;
                    if let Some(ls) = scene.lists.get_mut(ul) {
                        ls.anchoring_active = true;
                    }
                }
            }
        }
    }
}

/// 把 taffy `LengthPercentageAuto` 解析为 px：Length→值，Percent/Auto→0。
/// margin box 回填用——auto margin 对列表高度无贡献（flex 主轴由布局决定，已 solve 完），
/// percent margin 在列表场景极罕见且无父尺寸上下文，按 0 计（同 render::resolve_lp 对 padding 的处理）。
fn resolve_margin_px(m: taffy::style::LengthPercentageAuto) -> f32 {
    if m.is_auto() {
        0.0
    } else {
        let cl = m.into_raw();
        if cl.tag() == taffy::style::CompactLength::LENGTH_TAG {
            cl.value()
        } else {
            0.0 // percent / 其他：无上下文，按 0
        }
    }
}
