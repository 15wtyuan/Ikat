use super::execute::execute_visible;
use super::plan::plan_visible;
use super::viewport::ancestor_pane;
use crate::scene::node::{NodeId, NodeKind, Scene};

/// 同帧推进虚拟化管线：立即跑一次 plan/execute，让本帧滚动后新进入可见区的
/// item 的 slot 同帧克隆，其 bind 入 `pending_binds` 队列等 C# `DrainPendingBinds` 消费。
///
/// **不取队列**——core 无法调业务 BindItem 回调；取队列是 C# `take_pending_binds` 的职责
/// （每 tick 开头跑一次）。若此处也 take，FFI 会丢掉返回的 Vec，刚克隆的 slot 永不 bind。
/// ScrollToItem / 首次 ItemCount 调用走此路径——避免目标 item 首帧以模板原样显示。
///
/// `ul` 仅作存在性校验；plan_visible 本就遍历所有 ListView（对其余 list 是幂等 no-op）。
pub fn drain_now(stage: &mut crate::stage::Stage, ul: NodeId) {
    let scene = stage.scene.as_mut().expect("scene");
    if scene.lists.get(ul).is_none() {
        return;
    }
    let ops = plan_visible(scene);
    execute_visible(scene, ops);
}

/// 滚动到指定 item（ScrollToItem）。越界 index → Err（FFI 转 -1 → C# 抛 UIContractException）。
///
/// 时序：先设祖先 ScrollPane.scroll_pos 到目标偏移，**再** drain_now（plan+execute）——
/// plan_visible 读 scroll_pos 算可见区，故须先定 scroll_pos 才能让目标 item 的 slot
/// 进新可见区、同帧克隆 + 入 pending_binds 队列。binds 留队列给 C# DrainPendingBinds 消费
/// （core 不取——见 drain_now 文档）。
///
/// behavior：0=Instant（直接 snap+clamp），1=Smooth（走 ScrollPane 自维护的 cubic-out
/// tween，TweenProp 无 Scroll 变体——滚动容器物理独立于 GTween）。
///
/// 目标偏移 = `heights.sum(0..index)`（未测项用 estimate）。Instant 路径同帧 drain_now
/// → 下帧 anchoring 即修正估计偏差。Smooth 路径的目标不是一次性的：tween 期间新可见
/// item 陆续测量、overlap 增长，一次性目标会停在过期边界——故设 `smooth_scroll_to`
/// 锚，每帧 tick 在 collect_heights 回填后由 [`recompute_smooth_scroll_targets`]
/// 按最新 heights 重算 tween 终点（用户滚轮/拖拽等接管清锚，见 scroll.rs）。
pub fn scroll_to_item(
    stage: &mut crate::stage::Stage,
    ul: NodeId,
    index: usize,
    behavior: u8,
) -> Result<(), String> {
    let pane = {
        let scene = stage.scene.as_ref().ok_or("no scene")?;
        if scene.get(ul).map(|n| n.kind) != Some(NodeKind::ListView) {
            return Err("scroll_to_item: node is not a ListView".into());
        }
        let ls = scene
            .lists
            .get(ul)
            .ok_or("scroll_to_item: ListView not in data-driven mode")?;
        if index >= ls.item_count {
            return Err("scroll_to_item: index out of range".into());
        }
        ancestor_pane(scene, ul)
    };
    // 算目标偏移（单独借，避免与下面 set_pos 的可变借重叠）。
    let target = stage
        .scene
        .as_ref()
        .ok_or("no scene")?
        .lists
        .get(ul)
        .map(|ls| ls.sum_heights(0..index))
        .unwrap_or(0.0);
    // 设祖先 ScrollPane scroll_pos（保留 x，设 y）。animated=behavior==1（Smooth）。
    if let Some(pane) = pane {
        let scene = stage.scene.as_mut().ok_or("no scene")?;
        if let Some(st) = scene.scroll.get_mut(pane) {
            let x = st.scroll_pos.0;
            st.set_pos((x, target), behavior == 1);
            if behavior == 1 {
                st.smooth_scroll_to = Some((ul, index));
            }
        }
    }
    drain_now(stage, ul);
    Ok(())
}

/// Smooth ScrollToItem 的每帧目标重算（tick 在 collect_heights 回填 + refresh_content_sizes
/// 之后调）：对每个持锚 pane，按最新 `heights.sum(0..index)` 重算 y 轴 tween 终点——
/// 变高列表滚动中 overlap 增长，一次性目标会停在过期边界（错位）。
///
/// 只对 `tweening[1] == 1`（程序滚动 tween 仍在推进）生效；tween 已结束/被接管
/// （滚轮/拖拽/物理已各自清锚，此处兜底）→ 清锚。change 更新后 advance 的
/// `start + change * cubic_out(t/dur)` 自动向新终点收敛。
pub fn recompute_smooth_scroll_targets(scene: &mut Scene) {
    let anchored: Vec<(NodeId, NodeId, usize)> = scene
        .scroll
        .0
        .iter()
        .filter_map(|(&pane, st)| st.smooth_scroll_to.map(|(ul, idx)| (pane, ul, idx)))
        .collect();
    for (pane, ul, index) in anchored {
        // 回填后按最新高度重算目标（lists 槽消失 → 清锚防悬空 NodeId 残留）。
        let Some(target) = scene.lists.get(ul).map(|ls| ls.sum_heights(0..index)) else {
            if let Some(st) = scene.scroll.get_mut(pane) {
                st.smooth_scroll_to = None;
            }
            continue;
        };
        if let Some(st) = scene.scroll.get_mut(pane) {
            if st.tweening[1] != 1 {
                st.smooth_scroll_to = None; // tween 已完成/被接管（清锚兜底）
                continue;
            }
            let t = target.clamp(0.0, st.overlap.1);
            st.tween_change.1 = t - st.tween_start.1;
        }
    }
}
