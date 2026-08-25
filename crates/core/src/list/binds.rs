use crate::scene::node::{NodeId, Scene};

/// 设 ListView 的项数。重置 HeightCache 容量（保留已测高度）。
pub fn set_item_count(stage: &mut crate::stage::Stage, ul: NodeId, count: usize) {
    if let Some(scene) = stage.scene.as_mut() {
        if let Some(ls) = scene.lists.get_mut(ul) {
            ls.item_count = count;
            // 保留已测高度：resize 只扩缩 known vec，estimate 不变。
            // initial_estimate 取当前 estimate（无已测时 0.0，首帧 solve 后补真实模板高）。
            ls.heights.resize(count, ls.heights.estimate);
            ls.dirty = true;
        }
    }
}

/// 取该 ListView 的 pending bind 队列（C# tick 前调，逐条 BindItem 后数据写回 core）。
/// `std::mem::take` 把队列内容搬出、原位置空——保证同一批 bind 不被重复消费。
/// 队列由 execute_visible 在克隆新 slot 时填充；无 ListState 条目则返空 Vec。
pub fn take_pending_binds(scene: &mut Scene, ul: NodeId) -> Vec<(NodeId, usize)> {
    scene
        .lists
        .get_mut(ul)
        .map(|ls| std::mem::take(&mut ls.pending_binds))
        .unwrap_or_default()
}

/// 取该 ListView 的 pending bind 队列前端的至多 `max` 条（`drain(..n)`），余量留下次调用。
/// 与 `take_pending_binds` 的全取不同：当调用方缓冲区（cap）装不下整队时，只取装得下的部分，
/// 余条留在队列里等下一帧再取——保证 cap 不足时不丢 bind。FFI `take_pending_binds` 走此路径。
pub fn drain_pending_binds_bounded(
    scene: &mut Scene,
    ul: NodeId,
    max: usize,
) -> Vec<(NodeId, usize)> {
    match scene.lists.get_mut(ul) {
        Some(ls) if max > 0 => {
            let n = ls.pending_binds.len().min(max);
            ls.pending_binds.drain(..n).collect()
        }
        _ => Vec::new(),
    }
}
