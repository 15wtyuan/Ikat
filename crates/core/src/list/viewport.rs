use crate::scene::node::{NodeId, Scene};

/// ListView 的滚动视口来源。**自滚优先**：ul 自身带 ScrollPane（`overflow:auto/scroll`
/// 直接写在列表上）时用它自己的 scroll_pos/viewport——内容坐标原点就是 ul 内容盒，
/// 无祖先偏移可扣。否则沿祖先链找最近滚动容器（祖先滚动模式，如 mail 页外层列滚动）。
/// Some((sy, vh))：pane 视口（vh 可能 0 = 首帧未测，走冷启动）。None = 无任何
/// ScrollPane → plan_one 退化全量渲染 + 一次性警告（不再返回 (0,0) 假视口静默截断）。
pub(super) fn ancestor_scroll_viewport(scene: &Scene, node: NodeId) -> Option<(f32, f32)> {
    if let Some(st) = scene.scroll.get(node) {
        return Some((st.scroll_pos.1, st.viewport_size.1));
    }
    let mut cur = scene.get(node).and_then(|n| n.parent);
    while let Some(pid) = cur {
        if let Some(st) = scene.scroll.get(pid) {
            return Some((st.scroll_pos.1, st.viewport_size.1));
        }
        cur = scene.get(pid).and_then(|n| n.parent);
    }
    None
}

/// 无滚动容器的 warn-once：推 `Scene::warnings`（宿主每帧 drain 到引擎日志，如 Unity
/// Debug.LogWarning）。每列表只推一次（warned_no_pane 旗标防每帧刷屏）。
pub(super) fn warn_no_pane_once(scene: &mut Scene, ul: NodeId) {
    let warned = scene
        .lists
        .get(ul)
        .map(|ls| ls.warned_no_pane)
        .unwrap_or(true);
    if !warned {
        if let Some(ls) = scene.lists.get_mut(ul) {
            ls.warned_no_pane = true;
        }
        scene.warnings.push(format!(
            "ListView node {}: no scrollable ancestor (no overflow:auto/scroll pane) — \
             virtualization is disabled and every item renders up front; wrap the list in a \
             scroll pane or set overflow:auto on the list itself",
            ul.0
        ));
    }
}

/// 滚动容器 NodeId（anchoring 补偿 / scroll_to_item 设滚动用）。**自滚优先**
///（同 [`ancestor_scroll_viewport`]）：ul 自身可滚 → 返回 ul。无则 None。
pub(super) fn ancestor_pane(scene: &Scene, node: NodeId) -> Option<NodeId> {
    if scene.scroll.get(node).is_some() {
        return Some(node);
    }
    let mut cur = scene.get(node).and_then(|n| n.parent);
    while let Some(pid) = cur {
        if scene.scroll.get(pid).is_some() {
            return Some(pid);
        }
        cur = scene.get(pid).and_then(|n| n.parent);
    }
    None
}
