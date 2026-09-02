use super::pool::reorder_active_slots;
use crate::scene::node::{NodeId, Scene};

/// 插入通知（NotifyInserted）：在 `at` 处插入 `count` 项。heights.known 插入
/// `count` 个 None（新项未测）；item_count += count；slot.item_index >= at 的 +count
/// （保持物化 slot 与逻辑项的映射）。越界（at > item_count）→ Err。
/// dirty 置真，让下帧 plan_visible 按新 item_count / 可见区重算 spacer + 复用 slot。
pub fn notify_inserted(
    scene: &mut Scene,
    ul: NodeId,
    at: usize,
    count: usize,
) -> Result<(), String> {
    let ls = scene
        .lists
        .get_mut(ul)
        .ok_or("notify_inserted: ListView not in data-driven mode")?;
    if at > ls.item_count {
        return Err("notify_inserted: at out of range".into());
    }
    for _ in 0..count {
        ls.heights.known.insert(at, None);
        // 新项模板未知（C# 若设 selector 会在 Notify 后立刻重推 [at, count) 覆盖）。
        ls.template_ids.insert(at, ls.default_bp as u16);
    }
    ls.item_count += count;
    // 移位 + 重排队：item_index >= at 的 slot 移位后语义指向新 item，需重新 bind。
    // 收集移位 slot 的 (node, new_idx) 再 push（iter_mut 借 ls.slots 与 push ls.pending_binds 同借冲突）。
    // parked slot 的 item_index 只是复用参考（stale），不可入 bind 队列——否则驱动会对
    // 一个 display:none 的隐形 slot 跑 BindItem（无谓回调 + 业务数据写进看不见的节点）。
    let to_rebind: Vec<(NodeId, usize)> = ls
        .slots
        .iter()
        .filter(|s| s.item_index >= at && !s.parked)
        .map(|s| (s.node, s.item_index + count))
        .collect();
    for s in ls.slots.iter_mut() {
        if s.item_index >= at {
            s.item_index += count;
        }
    }
    ls.pending_binds.extend(to_rebind);
    ls.dirty = true;
    // 移位后 active slot 的物理顺序须仍按 item_index 升序。
    reorder_active_slots(scene, ul);
    Ok(())
}

/// 删除通知（NotifyRemoved）：删 [at, at+count) 项。越界（at+count > item_count）→ Err。
/// heights.known drain 该区间；item_count -= count；item_index 在 [at,end) 的 slot 就地 park
/// （留挂 ul + display:none，供下次可见区复用）；item_index > end 的 slot.item_index -= count。
/// dirty 置真。
///
/// 借用顺序：先快照待 park slot 的 NodeId 与待移位的 (idx, delta)，再可变借 ls 做标记 +
/// 移位——避免在同一可变借里调 set_inline_override（它另借 scene）。
pub fn notify_removed(
    scene: &mut Scene,
    ul: NodeId,
    at: usize,
    count: usize,
) -> Result<(), String> {
    let end = {
        let ls = scene
            .lists
            .get(ul)
            .ok_or("notify_removed: ListView not in data-driven mode")?;
        let end = at + count;
        if at >= ls.item_count || end > ls.item_count {
            return Err("notify_removed: range out of bounds".into());
        }
        end
    };
    // Phase A：可变借 ls —— drain heights + 算回收 / 移位分区（记 NodeId）。
    // 用 HashSet/HashMap 而非 Vec 线性查：slots 是只增的高水位池，Phase B 的成员判定
    // 必须 O(1)，否则每次 notify_removed 都是 O(高水位²)。
    // 待重排队的 bind 只收 active slot——parked slot 的 item_index 是 stale 复用参考，
    // 入队会让驱动对隐形（display:none）slot 跑 BindItem。
    let (to_recycle, to_shift, shift_binds): (
        std::collections::HashSet<NodeId>,
        std::collections::HashMap<NodeId, usize>,
        Vec<(NodeId, usize)>,
    ) = {
        let ls = scene.lists.get_mut(ul).unwrap();
        let end = end.min(ls.heights.known.len());
        ls.heights.known.drain(at..end);
        ls.template_ids.drain(at..end.min(ls.template_ids.len()));
        ls.item_count -= count;
        let mut recycle = std::collections::HashSet::new();
        let mut shift = std::collections::HashMap::new();
        let mut binds = Vec::new();
        for s in ls.slots.iter() {
            if s.item_index >= at && s.item_index < end {
                recycle.insert(s.node);
            } else if s.item_index >= end {
                let new_idx = s.item_index - count;
                shift.insert(s.node, new_idx);
                if !s.parked {
                    binds.push((s.node, new_idx));
                }
            }
        }
        (recycle, shift, binds)
    };
    // Phase B：可变借 ls —— park 回收项 + 重写移位项 index + 重排队移位的 active slot。
    {
        let ls = scene.lists.get_mut(ul).unwrap();
        // 回收 = 就地 park（slot 永驻 slots vec 与 ul 子树，只标休眠）。
        for s in ls.slots.iter_mut() {
            if to_recycle.contains(&s.node) {
                s.parked = true;
            } else if let Some(&new_idx) = to_shift.get(&s.node) {
                s.item_index = new_idx;
            }
        }
        // 移位 slot 现指向新 item_index → 重新 bind（业务数据跟到新序号）。
        ls.pending_binds.extend(shift_binds);
        ls.dirty = true;
    }
    // Phase C：不再 detach——离开可见区的 slot 就地 park（留挂 ul + display:none 便签），
    // NodeId/parent/reuse_key 全保留，下次进可见区只翻 display + 换绑。
    for node in &to_recycle {
        let _ = crate::scene::dynamic::set_inline_override(scene, *node, "display:none");
    }
    // park/shift 后 active slot 的物理顺序须仍按 item_index 升序（ul.children 即视觉顺序）。
    reorder_active_slots(scene, ul);
    Ok(())
}

/// 移动通知（NotifyMoved）：把 `from` 项搬到 `to` 位置。heights.known 同步搬；
/// slot.item_index 重映射（from 的 → to；from<to 区间内的项后移，from>to 区间内的前移）。
/// 越界（from/to >= item_count）→ Err。
pub fn notify_moved(scene: &mut Scene, ul: NodeId, from: usize, to: usize) -> Result<(), String> {
    let max = {
        let ls = scene
            .lists
            .get(ul)
            .ok_or("notify_moved: ListView not in data-driven mode")?;
        let max = ls.item_count;
        if from >= max || to >= max {
            return Err("notify_moved: index out of range".into());
        }
        max
    };
    if from == to {
        return Ok(());
    }
    {
        let ls = scene.lists.get_mut(ul).unwrap();
        let v = ls.heights.known.remove(from);
        ls.heights.known.insert(to, v);
        // per-item 模板映射同搬（保持与 heights.known 的下标对齐）。
        if from < ls.template_ids.len() {
            let t = ls.template_ids.remove(from);
            ls.template_ids.insert(to.min(ls.template_ids.len()), t);
        }
        // slot.item_index 重映射：原 from → to；
        //   from<to：原 (from,to] 的项前移 1（item_index-1）；
        //   from>to：原 [to,from) 的项后移 1（item_index+1）。
        // 同时收 集受影响 slot 重新 bind（item_index 变 → 业务数据需跟到新序号）。
        // parked slot 的 item_index 是 stale 复用参考，不可入 bind 队列——否则驱动会对
        // 一个 display:none 的隐形 slot 跑 BindItem（无谓回调 + 业务数据写进看不见的节点）。
        let mut to_rebind: Vec<(NodeId, usize)> = Vec::new();
        for s in ls.slots.iter_mut() {
            let i = s.item_index;
            if i == from {
                s.item_index = to;
                if !s.parked {
                    to_rebind.push((s.node, to));
                }
            } else if from < to && i > from && i <= to {
                s.item_index = i - 1;
                if !s.parked {
                    to_rebind.push((s.node, s.item_index));
                }
            } else if from > to && i >= to && i < from {
                s.item_index = i + 1;
                if !s.parked {
                    to_rebind.push((s.node, s.item_index));
                }
            }
        }
        ls.pending_binds.extend(to_rebind);
        ls.dirty = true;
    }
    let _ = max;
    // 重映射后 active slot 的物理顺序须仍按 item_index 升序。
    reorder_active_slots(scene, ul);
    Ok(())
}

/// 刷新通知（RefreshItems）：把 [start, start+count) 内**当前 active**的 slot
/// 重新入 pending_binds 队列，让 C# 下帧重新 BindItem（业务数据刷新）。
/// 区间内无 active slot 的 item（不在可见区）无需刷新——静默跳过（不报错），它们进
/// 可见区时由 execute 的 unpark 路径重新 bind。越界（start >= item_count）→ Err。
pub fn refresh_items(
    scene: &mut Scene,
    ul: NodeId,
    start: usize,
    count: usize,
) -> Result<(), String> {
    let end = start + count;
    // 先快照匹配的 (node, idx)，再 push——避免 iter(ls.slots) 与 push(ls.pending_binds) 同借冲突。
    let to_requeue: Vec<(NodeId, usize)> = {
        let ls = scene
            .lists
            .get_mut(ul)
            .ok_or("refresh_items: ListView not in data-driven mode")?;
        if start >= ls.item_count {
            return Err("refresh_items: start out of range".into());
        }
        ls.slots
            .iter()
            // parked slot 的 item_index 是 stale 复用参考，可能恰好落在刷新区间内——入队会让
            // 驱动对一个 display:none 的隐形 slot 跑 BindItem（无谓回调 + 数据写进看不见的节点）。
            .filter(|s| !s.parked && s.item_index >= start && s.item_index < end)
            .map(|s| (s.node, s.item_index))
            .collect()
    };
    if let Some(ls) = scene.lists.get_mut(ul) {
        ls.pending_binds.extend(to_requeue);
    }
    Ok(())
}
