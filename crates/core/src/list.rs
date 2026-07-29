//! ListView 虚拟化内核：HeightCache + 可见区算法 + slot 池 + spacer 撑高 + anchoring。
//! side table 模式（照 scroll.rs / EditState），不塞进 Node。

use crate::scene::node::{NodeFlags, NodeId, NodeKind, Scene};

/// 预渲染缓冲项数（可见区前后各 BUFFER 项提前克隆 + bind，吸收滚动速度）。
pub const BUFFER: usize = 2;
/// 冷启动（首帧 layout_rect 全 0 → viewport.h=0）时实例化的项数。
pub const INITIAL_SLOTS: usize = 1 + 2 * BUFFER;

/// 每项高度缓存。未测项用 estimate（已测均值；无已测时=模板首次布局高）。
/// sum = 已测部分精确和 + 未测数 × estimate。朴素 O(n)，触发换 Fenwick 判据：sum 占 tick > 5%。
#[derive(Debug, Clone)]
pub struct HeightCache {
    pub known: Vec<Option<f32>>,
    pub estimate: f32,
}

impl HeightCache {
    pub fn new(item_count: usize, initial_estimate: f32) -> Self {
        Self {
            known: vec![None; item_count],
            estimate: initial_estimate,
        }
    }

    /// 改变项数。旧已知项保留（不收缩则 resize 填 None）。`initial_estimate` 仅在已测项清空时生效。
    pub fn resize(&mut self, item_count: usize, initial_estimate: f32) {
        self.known.resize(item_count, None);
        if self.known.is_empty() {
            self.estimate = initial_estimate;
        }
    }

    pub fn height_of(&self, i: usize) -> f32 {
        self.known
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(self.estimate)
    }

    pub fn set(&mut self, i: usize, h: f32) {
        if i < self.known.len() {
            self.known[i] = Some(h);
        }
        self.recompute_estimate();
    }

    /// 求和 [start..end)。已测精确 + 未测 × estimate。
    pub fn sum(&self, range: std::ops::Range<usize>) -> f32 {
        let mut total = 0.0;
        for i in range {
            total += self.height_of(i);
        }
        total
    }

    fn recompute_estimate(&mut self) {
        let known: Vec<f32> = self.known.iter().filter_map(|v| *v).collect();
        if !known.is_empty() {
            self.estimate = known.iter().sum::<f32>() / known.len() as f32;
        }
    }
}

impl Default for HeightCache {
    fn default() -> Self {
        Self {
            known: Vec::new(),
            estimate: 0.0,
        }
    }
}

/// 单个虚拟列表 slot（克隆出的实例根 + 它绑定的 item 序号）。
/// `node` 在 slots vec 中按 item_index 排序（克隆时按 visible 顺序 append）。
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub node: NodeId,
    pub item_index: usize,
}

/// ListView 运行时虚拟化状态。每 ul（NodeKind::ListView）一个槽。
///
/// - `slots`：当前实例化的 slot（克隆出的 li），按 item_index 升序（克隆 / 回收均保序）。
/// - `free`：回收的 slot 根 NodeId 池（下次克隆优先复用，避免 clone_subtree 开销）。
/// - `visible`：上帧算出的可见 item 区间 [start, end)。
/// - `pending_binds`：本帧新克隆待绑定的 (slot_node, item_index)，由 bind 阶段（Task 6）消费。
/// - `anchoring_active` / `dirty`：anchoring / 静默刷新标记（预留，Task 5+ 用）。
#[derive(Debug, Clone)]
pub struct ListState {
    pub item_count: usize,
    /// 克隆模板的游离根（enter_data_driven 备份的设计期 li）。
    pub template_root: Option<NodeId>,
    pub heights: HeightCache,
    pub slots: Vec<Slot>,
    pub free: Vec<NodeId>,
    pub visible: std::ops::Range<usize>,
    pub head_spacer: NodeId,
    pub tail_spacer: NodeId,
    pub pending_binds: Vec<(NodeId, usize)>,
    pub list_ordinal: u32,
    pub anchoring_active: bool,
    pub dirty: bool,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            item_count: 0,
            template_root: None,
            heights: HeightCache::default(),
            slots: Vec::new(),
            free: Vec::new(),
            visible: 0..0,
            head_spacer: NodeId::INVALID,
            tail_spacer: NodeId::INVALID,
            pending_binds: Vec::new(),
            list_ordinal: 0,
            anchoring_active: false,
            dirty: true,
        }
    }
}

/// 每 ListView 节点的虚拟化状态表（`HashMap<NodeId, ListState>`）。运行时态，不进 pkg。
/// 结构与访问约定同 `ScrollTable`/`AnimTable`（见 node.rs AnimTable doc：NodeId 不能直接当
/// SecondaryMap Key，用 HashMap 便租用 / 零转换）。enter_data_driven 填、remove_node 联动清。
#[derive(Debug, Clone, Default)]
pub struct ListTable(pub std::collections::HashMap<NodeId, ListState>);

impl ListTable {
    pub fn get(&self, id: NodeId) -> Option<&ListState> {
        self.0.get(&id)
    }
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut ListState> {
        self.0.get_mut(&id)
    }
    /// 删该节点 ListState（remove_node 联动调，防悬空 NodeId 残留）。
    pub fn remove(&mut self, id: NodeId) {
        self.0.remove(&id);
    }
}

/// 进入数据驱动模式：备份模板（兜底=第一个设计期 li）+ 建 spacer + 清空设计期 li + 建 ListState。
///
/// ul 高度必须 auto（否则虚拟化无法撑出可滚内容）；非 auto → Err。被祖先 flex 拉伸检测复杂，
/// 这里只检显式 height 非 auto，flex 拉伸留 Unity 真机诊断（可接受：spec §4 约定 ul 高度 auto）。
pub fn enter_data_driven(
    stage: &mut crate::stage::Stage,
    ul: NodeId,
    list_ordinal: u32,
) -> Result<(), String> {
    // 短期不可变借：校验 kind + height + 收集设计期 li（含模板候选）。
    // 不能跨 clone_subtree 持有 scene 借（clone_subtree 也要 &mut stage）。
    let (first_li, lis): (Option<NodeId>, Vec<NodeId>) = {
        let scene = stage.scene.as_ref().ok_or("no scene")?;
        if scene.get(ul).map(|n| n.kind) != Some(NodeKind::ListView) {
            return Err("enter_data_driven: node is not a ListView".into());
        }
        check_ul_height_auto(scene, ul)?;
        let ul_node = scene.get(ul).unwrap();
        let first_li = ul_node
            .children
            .iter()
            .copied()
            .find(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem));
        let lis: Vec<NodeId> = ul_node
            .children
            .iter()
            .copied()
            .filter(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem))
            .collect();
        (first_li, lis)
    };
    // 先 clone 模板（需 &mut stage，此时无 scene 借），再清空原 li。
    let template_root = if let Some(li) = first_li {
        let cloned = stage.clone_subtree(li)?;
        for li in &lis {
            stage.remove_node(*li);
        }
        Some(cloned)
    } else {
        return Err("ListView 无模板来源：无 <template>、无设计期 li、未设 ItemTemplate".into());
    };
    let head = stage.create_node("div", "")?;
    let tail = stage.create_node("div", "")?;
    configure_spacer(stage, head);
    configure_spacer(stage, tail);
    stage.append_child(ul, head)?;
    stage.append_child(ul, tail)?;
    let ls = ListState {
        item_count: 0,
        template_root,
        heights: HeightCache::new(0, 0.0),
        slots: Vec::new(),
        free: Vec::new(),
        visible: 0..0,
        head_spacer: head,
        tail_spacer: tail,
        pending_binds: Vec::new(),
        list_ordinal,
        anchoring_active: false,
        dirty: true,
    };
    stage.scene.as_mut().unwrap().lists.0.insert(ul, ls);
    Ok(())
}

/// ul 高度必须 auto（虚拟化靠 spacer 撑出可滚高度，ul 自身被滚动容器裁切）。
/// taffy 0.12 的 size.height 是 `Dimension`，用 `is_auto()` 检测。
fn check_ul_height_auto(scene: &Scene, ul: NodeId) -> Result<(), String> {
    let n = scene.get(ul).ok_or("ul not found")?;
    if !n.base_style.taffy_style.size.height.is_auto() {
        return Err("数据驱动 ListView 高度必须为 auto（否则虚拟化无法撑出可滚内容）".into());
    }
    Ok(())
}

/// spacer 初始样式：flex-shrink:0（不被压缩）+ height:0 + padding-top:0.01px（阻断 margin collapsing）。
/// 直接改 base_style.taffy_style（运行时 create_node 的 css 参数虽经 apply_decl，但直接赋值更明确，
/// 避免 padding shorthand 的多值解析路径）。
fn configure_spacer(stage: &mut crate::stage::Stage, spacer: NodeId) {
    let scene = stage.scene.as_mut().unwrap();
    let n = scene.get_mut(spacer).unwrap();
    n.base_style.taffy_style.flex_shrink = 0.0;
    // padding 字段是 LengthPercentage（非 Auto）；size.height 是 Dimension（含 Auto 变体）。
    // taffy 0.12 用小写构造函数 `length(val)` / `auto()`。
    n.base_style.taffy_style.padding.top = taffy::style::LengthPercentage::length(0.01);
    n.base_style.taffy_style.size.height = taffy::style::Dimension::length(0.0);
    n.style = n.base_style.clone();
    n.dirty_mesh = true;
}

/// 计算可见项区间 [start, end)（含 BUFFER）。viewport.h==0 → 冷启动返 INITIAL_SLOTS。
/// top = scroll_pos.y - listview_offset（ul 相对 pane 的偏移）。
pub fn compute_visible_range(
    item_count: usize,
    scroll_pos_y: f32,
    listview_offset: f32,
    viewport_h: f32,
    heights: &HeightCache,
) -> std::ops::Range<usize> {
    if item_count == 0 {
        return 0..0;
    }
    if viewport_h <= 0.0 {
        return 0..INITIAL_SLOTS.min(item_count);
    } // 冷启动
      // 无已测高度（estimate<=0）：无法估算可见项数（每项高度 0 → 累积和永不达阈值 → 误判整列可见）。
      // 退化为冷启动定数，等首帧 solve + collect_heights 回填真实高度后下帧才走精准路径。
    if heights.estimate <= 0.0 {
        return 0..INITIAL_SLOTS.min(item_count);
    }
    let top = scroll_pos_y - listview_offset;
    // first = 首个顶边超过 top 的项（累积后判）。若全项顶边 ≤ top（内容短于视口），
    // 循环不 break，first 保持 0 → start 经 BUFFER 回退到 0，整列可见。
    let mut acc = 0.0;
    let mut first = 0usize;
    for i in 0..item_count {
        acc += heights.height_of(i);
        if acc > top {
            first = i;
            break;
        }
    }
    let target = top + viewport_h;
    let mut acc2 = 0.0;
    let mut last = item_count;
    for j in 0..item_count {
        acc2 += heights.height_of(j);
        if acc2 >= target {
            last = j + 1;
            break;
        }
    }
    let start = first.saturating_sub(BUFFER);
    let end = (last + BUFFER).min(item_count);
    start..end
}

// ── 数据驱动模式：可见区计算 + slot 池 + spacer 撑高 ───────────────────────

/// 设 ListView 的项数。重置 HeightCache 容量（保留已测高度）。
pub fn set_item_count(stage: &mut crate::stage::Stage, ul: NodeId, count: usize) {
    if let Some(scene) = stage.scene.as_mut() {
        if let Some(ls) = scene.lists.get_mut(ul) {
            ls.item_count = count;
            // 保留已测高度：resize 只扩缩 known vec，estimate 不变。
            // initial_estimate 取当前 estimate（无已测时 0.0，首帧 solve 后 Task 5 补真实模板高）。
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

/// 同帧推进虚拟化管线（spec §7）：立即跑一次 plan/execute，让本帧滚动后新进入可见区的
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

/// 滚动到指定 item（spec §7 ScrollToItem）。越界 index → Err（FFI 转 -1 → C# 抛 UIContractException）。
///
/// 时序：先设祖先 ScrollPane.scroll_pos 到目标偏移，**再** drain_now（plan+execute）——
/// plan_visible 读 scroll_pos 算可见区，故须先定 scroll_pos 才能让目标 item 的 slot
/// 进新可见区、同帧克隆 + 入 pending_binds 队列。binds 留队列给 C# DrainPendingBinds 消费
/// （core 不取——见 drain_now 文档）。
///
/// behavior：0=Instant（直接 snap+clamp），1=Smooth（走 ScrollPane 自维护的 cubic-out
/// tween，TweenProp 无 Scroll 变体——滚动容器物理独立于 GTween）。
///
/// 目标偏移 = `heights.sum(0..index)`（未测项用 estimate；下帧 collect_heights 回填后
/// anchoring 会修正偏差）。`set_pos` 按 overlap clamp——虚拟列表 content_size 由 driver 注入。
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
        .map(|ls| ls.heights.sum(0..index))
        .unwrap_or(0.0);
    // 设祖先 ScrollPane scroll_pos（保留 x，设 y）。animated=behavior==1（Smooth）。
    if let Some(pane) = pane {
        let scene = stage.scene.as_mut().ok_or("no scene")?;
        if let Some(st) = scene.scroll.get_mut(pane) {
            let x = st.scroll_pos.0;
            st.set_pos((x, target), behavior == 1);
        }
    }
    // drain_now（plan+execute）让新可见区的 slot 同帧克隆 + binds 入队。
    drain_now(stage, ul);
    Ok(())
}

/// 插入通知（spec §10 NotifyInserted）：在 `at` 处插入 `count` 项。heights.known 插入
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
    }
    ls.item_count += count;
    // 移位 + 重排队：item_index >= at 的 slot 移位后语义指向新 item，需重新 bind。
    // 收集移位 slot 的 (node, new_idx) 再 push（iter_mut 借 ls.slots 与 push ls.pending_binds 同借冲突）。
    let to_rebind: Vec<(NodeId, usize)> = ls
        .slots
        .iter()
        .filter(|s| s.item_index >= at)
        .map(|s| (s.node, s.item_index + count))
        .collect();
    for s in ls.slots.iter_mut() {
        if s.item_index >= at {
            s.item_index += count;
        }
    }
    ls.pending_binds.extend(to_rebind);
    ls.dirty = true;
    Ok(())
}

/// 删除通知（spec §10 NotifyRemoved）：删 [at, at+count) 项。越界（at+count > item_count）→ Err。
/// heights.known drain 该区间；item_count -= count；item_index 在 [at,end) 的 slot 回收
/// （从 slots 剔除 + 从 ul 子树 detach + 入 free 池，供下次克隆复用）；item_index > end
/// 的 slot.item_index -= count。dirty 置真。
///
/// 借用顺序：先快照待回收 slot 的 NodeId 与待移位的 (idx, delta)，再可变借 ls 做回收 +
/// 移位——避免在同一可变借里调 remove_child（它另借 scene）。
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
    let (to_recycle, to_shift): (Vec<NodeId>, Vec<(NodeId, usize)>) = {
        let ls = scene.lists.get_mut(ul).unwrap();
        let end = end.min(ls.heights.known.len());
        ls.heights.known.drain(at..end);
        ls.item_count -= count;
        let mut recycle = Vec::new();
        let mut shift = Vec::new();
        for s in ls.slots.iter() {
            if s.item_index >= at && s.item_index < end {
                recycle.push(s.node);
            } else if s.item_index >= end {
                shift.push((s.node, s.item_index - count));
            }
        }
        (recycle, shift)
    };
    // Phase B：可变借 ls —— 从 slots 剔除回收项 + 重写移位项 index + 重排队移位 slot（重新 bind）。
    {
        let ls = scene.lists.get_mut(ul).unwrap();
        ls.slots.retain(|s| !to_recycle.contains(&s.node));
        for s in ls.slots.iter_mut() {
            if let Some((_, new_idx)) = to_shift.iter().find(|(n, _)| *n == s.node) {
                s.item_index = *new_idx;
            }
        }
        // 移位 slot 现指向新 item_index → 重新 bind（业务数据跟到新序号）。
        ls.pending_binds.extend(to_shift);
        ls.dirty = true;
    }
    // Phase C：从 ul 子树 detach 回收 slot（remove_child 保 slotmap 槽，正是 free 池语义）+ 入 free。
    for node in &to_recycle {
        let _ = crate::scene::dynamic::remove_child(scene, ul, *node);
    }
    if let Some(ls) = scene.lists.get_mut(ul) {
        ls.free.extend(to_recycle);
    }
    Ok(())
}

/// 移动通知（spec §10 NotifyMoved）：把 `from` 项搬到 `to` 位置。heights.known 同步搬；
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
    // heights.known 搬移：remove(from).insert(to)。
    {
        let ls = scene.lists.get_mut(ul).unwrap();
        let v = ls.heights.known.remove(from);
        ls.heights.known.insert(to, v);
        // slot.item_index 重映射：原 from → to；
        //   from<to：原 (from,to] 的项前移 1（item_index-1）；
        //   from>to：原 [to,from) 的项后移 1（item_index+1）。
        // 同时收 集受影响 slot 重新 bind（item_index 变 → 业务数据需跟到新序号）。
        let mut to_rebind: Vec<(NodeId, usize)> = Vec::new();
        for s in ls.slots.iter_mut() {
            let i = s.item_index;
            if i == from {
                s.item_index = to;
                to_rebind.push((s.node, to));
            } else if from < to && i > from && i <= to {
                s.item_index = i - 1;
                to_rebind.push((s.node, s.item_index));
            } else if from > to && i >= to && i < from {
                s.item_index = i + 1;
                to_rebind.push((s.node, s.item_index));
            }
        }
        ls.pending_binds.extend(to_rebind);
        ls.dirty = true;
    }
    let _ = max;
    Ok(())
}

/// 刷新通知（spec §10 RefreshItems）：把 [start, start+count) 内**已物化**的 slot
/// 重新入 pending_binds 队列，让 C# 下帧重新 BindItem（业务数据刷新）。
/// 未物化的 slot（不在 slots 中）无法刷新——静默跳过（不报错）。越界（start >= item_count）→ Err。
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
            .filter(|s| s.item_index >= start && s.item_index < end)
            .map(|s| (s.node, s.item_index))
            .collect()
    };
    if let Some(ls) = scene.lists.get_mut(ul) {
        ls.pending_binds.extend(to_requeue);
    }
    Ok(())
}

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
            let slots: Vec<(NodeId, usize)> =
                ls.slots.iter().map(|s| (s.node, s.item_index)).collect();
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

/// plan 阶段的待执行操作（与 execute 解耦，避开 clone_subtree 的 &mut Stage 与
/// plan 的 &mut Scene 借用冲突）。单个 ListView 一条。
pub struct PendingOps {
    pub list_ul: NodeId,
    /// 本帧需新克隆的 item 序号（visible − 当前已有 slot）。
    pub to_clone: Vec<usize>,
    pub new_visible: std::ops::Range<usize>,
    pub spacer_head_h: f32,
    pub spacer_tail_h: f32,
}

/// plan 阶段：算可见区、回收离开的 slot 入 free 池、产待克隆 index 列表。**只借 scene**
/// （clone_subtree 不在此调）。tick_and_render 先调 plan_visible 再调 execute_visible。
pub fn plan_visible(scene: &mut Scene) -> Vec<PendingOps> {
    // 收集所有 ListView 节点的 NodeId（避免在借 scene.lists 时借 scene.nodes）。
    let uls: Vec<NodeId> = scene.lists.0.keys().copied().collect();
    let mut out = Vec::new();
    for ul in uls {
        if let Some(op) = plan_one(scene, ul) {
            out.push(op);
        }
    }
    out
}

fn plan_one(scene: &mut Scene, ul: NodeId) -> Option<PendingOps> {
    // Phase A：单次不可变借完成所有只读计算——可见区（Copy 的 Range）+ spacer 高度 + gap。
    // spacer 高需 heights.sum，故一并在此算出，避免后续跨可变借再 clone heights。
    let (scroll_y, viewport_h, ul_y) = {
        let (sy, vh) = ancestor_scroll_viewport(scene, ul);
        let uy = scene.get(ul).map(|n| n.layout_rect.y).unwrap_or(0.0);
        (sy, vh, uy)
    };
    let (visible, spacer_head_h, spacer_tail_h) = {
        let ls = scene.lists.get(ul)?;
        let gap = if matches!(
            scene.get(ul).unwrap().base_style.taffy_style.display,
            taffy::Display::Flex
        ) {
            crate::render::resolve_lp(scene.get(ul).unwrap().base_style.taffy_style.gap.height)
        } else {
            0.0
        };
        let visible = compute_visible_range(ls.item_count, scroll_y, ul_y, viewport_h, &ls.heights);
        // Gap accounting for flex+gap uls. The virtualized ul's flex children are
        // [head_spacer, slot, ..., slot, tail_spacer], so a visible slot sits one gap
        // past the head spacer. For a visible slot (bound to item visible.start) to match
        // the non-virtualized reference position (item[k].top = sum(0..k) + k*gap), the
        // head spacer must reserve sum(0..visible.start) + (visible.start - 1)*gap:
        //   slot.top = head_spacer.h + gap = sum + (count-1)*gap + gap = sum + count*gap. ✓
        // The tail is symmetric. count = hidden items in each spacer's range. count=0 →
        // saturating_sub(1)=0 → no gap contribution (spacer empty). Block uls have gap=0
        // so this is a no-op for them.
        let head_count = visible.start;
        let tail_count = ls.item_count.saturating_sub(visible.end);
        let spacer_head_h = (ls.heights.sum(0..visible.start)
            + (head_count.saturating_sub(1) as f32) * gap)
            .max(0.0);
        let spacer_tail_h = (ls.heights.sum(visible.end..ls.item_count)
            + (tail_count.saturating_sub(1) as f32) * gap)
            .max(0.0);
        (visible, spacer_head_h, spacer_tail_h)
    };
    // Phase B：可变借回收离开的 slot。被回收的 NodeId 仅暂存（不在此 detach——detach 需借 scene
    // 建树函数，与本处 ls 可变借冲突），待 Phase A/B 借释放后再处理。to_clone 也在此算出。
    let new_set: std::collections::HashSet<usize> = visible.clone().collect();
    let (to_clone, to_free): (Vec<usize>, Vec<NodeId>) = {
        let ls = scene.lists.get_mut(ul)?;
        let mut keep_slots = Vec::new();
        let mut to_free = Vec::new();
        for s in ls.slots.drain(..) {
            if new_set.contains(&s.item_index) {
                keep_slots.push(s);
            } else {
                to_free.push(s.node);
            }
        }
        ls.slots = keep_slots;
        // 待克隆 = visible − 当前已有 slot indices。
        let have: std::collections::HashSet<usize> =
            ls.slots.iter().map(|s| s.item_index).collect();
        let to_clone = visible.clone().filter(|i| !have.contains(i)).collect();
        (to_clone, to_free)
    };
    // Phase C：摘除被回收的 slot——必须从场景树移出（parent=None + 出 ul.children），
    // 否则复用时 insert_before 因 child 已有 parent 返 Err 而被吞掉，slot 停在旧位、顺序漂移。
    // remove_child 保留 slotmap 槽（NodeId 仍 live），正是 free 池“存活 NodeId 池”语义。
    for node in &to_free {
        let _ = crate::scene::dynamic::remove_child(scene, ul, *node);
    }
    if let Some(ls) = scene.lists.get_mut(ul) {
        ls.free.extend(to_free);
    }
    Some(PendingOps {
        list_ul: ul,
        to_clone,
        new_visible: visible,
        spacer_head_h,
        spacer_tail_h,
    })
}

/// execute 阶段：clone slot + insert_before tail_spacer + 标 LOOKUP_SCOPE + reuse_key +
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
    for item_index in &op.to_clone {
        // 优先从 free 池复用（避免 clone 开销）；取不到才 clone_node_recursive。
        let node = scene.lists.get_mut(op.list_ul).and_then(|ls| ls.free.pop());
        let node = match node {
            Some(n) => n,
            None => crate::scene::dynamic::clone_node_recursive(scene, tpl),
        };
        // 标 LOOKUP_SCOPE（不打 SCOPE_ROOT：spec §6.2，slot 根 CSS 规则仍按页面根 scope 匹配）。
        if let Some(n) = scene.get_mut(node) {
            n.interaction.flags.insert(NodeFlags::LOOKUP_SCOPE);
        }
        // reuse_key 编码：((list_ordinal+1)<<16)|(slot_idx)。恒 ≠ 0（list_ordinal+1 ≥ 1）。
        let slot_idx = scene
            .lists
            .get(op.list_ul)
            .map(|ls| ls.slots.len())
            .unwrap_or(0);
        crate::scene::dynamic::set_reuse_key(scene, node, encode_reuse_key(list_ordinal, slot_idx));
        // append 到 tail_spacer 之前（head/tail spacer 始终首位）。
        let _ = crate::scene::dynamic::insert_before(scene, op.list_ul, node, tail_spacer);
        if let Some(ls) = scene.lists.get_mut(op.list_ul) {
            ls.slots.push(Slot {
                node,
                item_index: *item_index,
            });
            ls.pending_binds.push((node, *item_index));
        }
    }
    // 写 spacer 高度 + 记录本帧 visible。
    let (head, tail) = {
        let ls = scene.lists.get_mut(op.list_ul).unwrap();
        ls.visible = op.new_visible;
        (ls.head_spacer, ls.tail_spacer)
    };
    set_spacer_height(scene, head, op.spacer_head_h);
    set_spacer_height(scene, tail, op.spacer_tail_h);
}

/// reuse_key 编码：高 16 bit = list_ordinal+1（0 保留表“无 key”），低 16 bit = slot_idx。
/// 恒 ≠ 0（list_ordinal+1 ≥ 1）。场景级全局命名空间（同 ordinal 的 slot 跨帧复用）。
fn encode_reuse_key(list_ordinal: u32, slot_idx: usize) -> u32 {
    ((list_ordinal + 1) << 16) | ((slot_idx as u32) & 0xFFFF)
}

/// 沿祖先链找最近滚动容器，返 (scroll_pos.y, viewport.h)。无祖先 ScrollPane → (0,0)
/// （viewport.h=0 触发冷启动 → INITIAL_SLOTS），保证无滚动容器的测试也能实例化初始 slot。
fn ancestor_scroll_viewport(scene: &Scene, node: NodeId) -> (f32, f32) {
    let mut cur = scene.get(node).and_then(|n| n.parent);
    while let Some(pid) = cur {
        if let Some(st) = scene.scroll.get(pid) {
            return (st.scroll_pos.1, st.viewport_size.1);
        }
        cur = scene.get(pid).and_then(|n| n.parent);
    }
    (0.0, 0.0)
}

/// 沿祖先链找最近滚动容器 NodeId（anchoring 补偿 scroll_pos 用）。无则 None。
fn ancestor_pane(scene: &Scene, node: NodeId) -> Option<NodeId> {
    let mut cur = scene.get(node).and_then(|n| n.parent);
    while let Some(pid) = cur {
        if scene.scroll.get(pid).is_some() {
            return Some(pid);
        }
        cur = scene.get(pid).and_then(|n| n.parent);
    }
    None
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

/// 写 spacer 高度（base_style + style 同步，标 dirty_mesh 触发重布局）。
fn set_spacer_height(scene: &mut Scene, spacer: NodeId, h: f32) {
    if let Some(n) = scene.get_mut(spacer) {
        let d = taffy::style::Dimension::length(h);
        n.base_style.taffy_style.size.height = d;
        n.style.taffy_style.size.height = d;
        n.dirty_mesh = true;
    }
}

/// 构造测试用 Stage：场景含一个 ListView(ul) 根 + 一个 ListItem(li) 子。
/// 运行时 create_node 只支持 div/button/img/span，故 ListView/ListItem 须经
/// Scene::build 直接构造（同打包器入口），再注入 Stage。
#[cfg(test)]
fn stage_with_ul_li() -> (crate::stage::Stage, NodeId, NodeId) {
    use crate::scene::node::{NodeKind, Scene};
    use crate::style::resolved::ResolvedStyle;
    let mut s = crate::stage::Stage::new_for_test();
    let entries: [(
        Option<usize>,
        NodeKind,
        crate::style::resolved::ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
        Option<String>,
        Option<String>,
    ); 2] = [
        (
            None,
            NodeKind::ListView,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::ListItem,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
            None,
            None,
        ),
    ];
    let scene = Scene::build(&entries);
    let ul = scene.roots[0];
    let li = scene.get(ul).unwrap().children[0];
    s.scene = Some(scene);
    (s, ul, li)
}

/// 测试辅助：3 层树 pane(Container, overflow scroll) → ul(ListView) → li(ListItem)。
/// 用于 margin box / anchoring 测（需祖先 ScrollPane）。返 (stage, ul, li, pane)。
#[cfg(test)]
fn stage_with_pane_ul_li() -> (crate::stage::Stage, NodeId, NodeId, NodeId) {
    use crate::scene::node::{Node, NodeKind};
    use crate::style::resolved::OverflowMode;
    let pane_node = Node {
        kind: NodeKind::Container,
        style: crate::style::resolved::ResolvedStyle {
            overflow_y: OverflowMode::Scroll,
            ..Default::default()
        },
        ..Node::default()
    };
    let ul_node = Node {
        kind: NodeKind::ListView,
        ..Node::default()
    };
    let li = Node {
        kind: NodeKind::ListItem,
        ..Node::default()
    };
    let scene =
        crate::scene::node::Scene::from_nodes(vec![pane_node, ul_node, li], vec![(0, 1), (1, 2)]);
    let pane = scene.roots[0];
    let ul = scene.get(pane).unwrap().children[0];
    let li = scene.get(ul).unwrap().children[0];
    let mut s = crate::stage::Stage::new_for_test();
    s.scene = Some(scene);
    (s, ul, li, pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_cache_sum_with_mixed_known_estimate() {
        let mut hc = HeightCache::new(3, 20.0);
        hc.set(0, 10.0);
        hc.set(2, 30.0);
        approx_eq(hc.sum(0..3), 60.0);
    }

    #[test]
    fn height_cache_estimate_updates_to_known_mean() {
        let mut hc = HeightCache::new(5, 40.0);
        hc.set(0, 10.0);
        hc.set(1, 30.0);
        approx_eq(hc.estimate, 20.0);
        approx_eq(hc.sum(0..5), 100.0);
    }

    #[test]
    fn height_cache_sum_empty_range_zero() {
        let hc = HeightCache::new(10, 50.0);
        approx_eq(hc.sum(5..5), 0.0);
    }

    #[test]
    fn visible_range_basic() {
        let r = compute_visible_range(100, 0.0, 0.0, 100.0, &uniform_heights(100, 10.0));
        assert_eq!(r, 0..12);
    }

    #[test]
    fn visible_range_scrolled_mid() {
        let r = compute_visible_range(100, 50.0, 0.0, 100.0, &uniform_heights(100, 10.0));
        assert_eq!(r, 3..17);
    }

    #[test]
    fn visible_range_clamps_to_count() {
        let r = compute_visible_range(5, 50.0, 0.0, 100.0, &uniform_heights(5, 10.0));
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn visible_range_empty_count() {
        let r = compute_visible_range(0, 0.0, 0.0, 100.0, &HeightCache::new(0, 10.0));
        assert_eq!(r, 0..0);
    }

    #[test]
    fn visible_range_cold_start_viewport_zero() {
        let r = compute_visible_range(1000, 0.0, 0.0, 0.0, &uniform_heights(1000, 10.0));
        assert_eq!(r, 0..INITIAL_SLOTS);
    }

    fn uniform_heights(n: usize, h: f32) -> HeightCache {
        let mut hc = HeightCache::new(n, h);
        for i in 0..n {
            hc.set(i, h);
        }
        hc
    }

    fn approx_eq(a: f32, b: f32) {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }

    /// 断言所有 slot 都正确接在 ul 树上：每个 slot 的 parent==Some(ul)、且在 ul.children
    /// 中位于 head_spacer 之后 / tail_spacer 之前、按 item_index 严格递增（复用未 detach
    /// 会让被复用 slot 停在旧位、顺序乱掉）。同时检 ul.children 无重复 NodeId。
    fn assert_all_slots_well_parented(scene: &crate::scene::node::Scene, ul: NodeId) {
        let ls = scene.lists.get(ul).expect("list state");
        let head = ls.head_spacer;
        let tail = ls.tail_spacer;
        let ul_node = scene.get(ul).unwrap();
        // head/tail 始终首尾。
        assert_eq!(ul_node.children.first(), Some(&head), "head spacer first");
        assert_eq!(ul_node.children.last(), Some(&tail), "tail spacer last");
        // 无重复子。
        let mut seen = std::collections::HashSet::new();
        for &c in &ul_node.children {
            assert!(seen.insert(c), "duplicate child in ul.children");
        }
        // slot → item_index 映射。
        let item_of: std::collections::HashMap<NodeId, usize> =
            ls.slots.iter().map(|s| (s.node, s.item_index)).collect();
        // 逐 slot：parent 正确 + 在 head/tail 之间。并收集物理顺序的 item_index。
        let mut physical_order: Vec<usize> = Vec::new();
        for &c in &ul_node.children[1..ul_node.children.len() - 1] {
            let cn = scene.get(c).unwrap();
            assert_eq!(cn.parent, Some(ul), "slot parent must be ul");
            let idx = *item_of.get(&c).expect("child maps to a slot item");
            physical_order.push(idx);
        }
        // 物理顺序严格递增（复用未 detach 会让旧位 slot 的 item_index 乱序）。
        let mut sorted = physical_order.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            physical_order, sorted,
            "slot physical order must match sorted item_index (no drift)"
        );
    }

    #[test]
    fn enter_data_driven_creates_spacers_and_backups_li() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(ul_node.children.len(), 2, "ul has head+tail spacer only");
        let ls = scene.lists.get(ul).expect("list state created");
        assert!(
            ls.template_root.is_some(),
            "design-time li backed up as template"
        );
    }

    #[test]
    fn update_visible_instantiates_initial_slots() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 1000);
        // plan（借 scene）+ execute（借 scene）两阶段，同 tick_and_render 调法。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_ref().unwrap();
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(
            ul_node.children.len(),
            2 + crate::list::INITIAL_SLOTS,
            "2 spacers + INITIAL_SLOTS slots for cold-start count=1000"
        );
        // slot 根打 LOOKUP_SCOPE（不打 SCOPE_ROOT）
        let slot_node = scene.get(ul_node.children[2]).unwrap();
        assert!(
            slot_node
                .interaction
                .flags
                .contains(NodeFlags::LOOKUP_SCOPE),
            "slot root carries LOOKUP_SCOPE"
        );
        assert!(
            !slot_node.interaction.flags.contains(NodeFlags::SCOPE_ROOT),
            "slot root must NOT carry SCOPE_ROOT (CSS rules still apply)"
        );
    }

    /// 回收路径回归：滚后部分 slot 离开可见区→进 free 池，下一帧需复用时 insert_before
    /// 因未 detach 旧父会 Err 被吞、slot 停在旧位、ul.children 顺序漂移。此测模拟两次帧。
    #[test]
    fn update_visible_recycles_slots_across_frames() {
        use crate::scene::node::{Node, NodeKind};
        // 3 层树：scroll_ancestor(Container) → ul(ListView) → li(ListItem)。
        let ancestor = Node {
            kind: NodeKind::Container,
            ..Node::default()
        };
        let ul_node = Node {
            kind: NodeKind::ListView,
            ..Node::default()
        };
        let li = Node {
            kind: NodeKind::ListItem,
            ..Node::default()
        };
        let scene = crate::scene::node::Scene::from_nodes(
            vec![ancestor, ul_node, li],
            vec![(0, 1), (1, 2)],
        );
        let ancestor_id = scene.roots[0];
        let ul = scene.get(ancestor_id).unwrap().children[0];
        let mut s = crate::stage::Stage::new_for_test();
        s.scene = Some(scene);

        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 1000);
        // 给真实高度（避免 estimate=0 导致可见区退化为整列）：20px/项。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..1000 {
                ls.heights.set(i, 20.0);
            }
            // 滚动祖先视口高 100，scroll_y=0 → 第一帧可见 0..7（首项顶=0，+BUFFER）。
            let st = scene.scroll.ensure(ancestor_id);
            st.viewport_size = (1000.0, 100.0);
            st.scroll_pos = (0.0, 0.0);
        }

        // 第一帧：实例化初始 slot。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.slots.len(), 7, "first frame: visible 0..7 → 7 slots");
        assert_eq!(ls.free.len(), 0, "no recycle yet");
        assert_all_slots_well_parented(scene, ul);

        // 第二帧：滚下 100px（~5 项）→ 可见 3..12。items 0,1,2 离开→进 free 池，复用给 7,8,9。
        {
            let scene = s.scene.as_mut().unwrap();
            let st = scene.scroll.ensure(ancestor_id);
            st.scroll_pos = (0.0, 100.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.visible, 3..12, "second frame: scrolled to 3..12");
        assert_eq!(ls.slots.len(), 9, "second frame: 9 visible slots");
        assert_all_slots_well_parented(scene, ul);
    }

    /// template_root 是游离子树（parent=None、不在 roots）。remove_node(ul) 必须
    /// 随 ul 一并释放它，否则 ListState 条目清掉后成孤儿、slotmap 槽永久泄漏。
    #[test]
    fn remove_node_frees_template_root_subtree() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let template_root = s
            .scene
            .as_ref()
            .unwrap()
            .lists
            .get(ul)
            .unwrap()
            .template_root
            .expect("template backed up");
        // template_root 此时是游离节点（parent=None、不在 roots）。
        assert!(
            s.scene.as_ref().unwrap().get(template_root).is_some(),
            "template live before remove"
        );
        s.remove_node(ul);
        assert!(
            s.scene.as_ref().unwrap().get(ul).is_none(),
            "ul removed (slotmap slot freed)"
        );
        assert!(
            s.scene.as_ref().unwrap().get(template_root).is_none(),
            "template subtree freed (no leak)"
        );
        assert!(
            s.scene.as_ref().unwrap().lists.get(ul).is_none(),
            "list state entry removed"
        );
    }

    /// take_pending_binds：首次取回全部新克隆 slot 的 (node,item_index)，二次取空。
    /// C# tick 前调本函数逐条 BindItem，数据写回 core 后队列清空——保证每条 bind 仅触发一次。
    #[test]
    fn take_pending_binds_returns_new_slots_then_empty() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let binds = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        assert_eq!(binds.len(), crate::list::INITIAL_SLOTS);
        let binds2 = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        assert!(binds2.is_empty(), "second take empty");
    }

    /// collect_heights：solve 后把 slot 实际 layout_rect.h 回填 HeightCache，
    /// 下帧可见区算法用真实高度而非 estimate。等高版：直写 known[i]。
    #[test]
    fn collect_heights_writes_slot_layout_height() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 10);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 给每个 slot 一个伪造 layout_rect.h（绕过 solve，直写 layout_rect 验 collect 读对字段）。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            let slots: Vec<(NodeId, usize)> =
                ls.slots.iter().map(|s| (s.node, s.item_index)).collect();
            for (node, idx) in slots {
                let n = scene.get_mut(node).unwrap();
                n.layout_rect.h = (idx as f32) * 10.0 + 5.0;
            }
        }
        crate::list::collect_heights(s.scene.as_mut().unwrap());
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.heights.height_of(0), 5.0);
        assert_eq!(ls.heights.height_of(1), 15.0);
        assert_eq!(ls.heights.height_of(2), 25.0);
    }

    /// margin box 回填：li 带 margin-bottom:8px 时，height_of 应 = border-box h + margin。
    /// 回归锚点——漏计 margin 会让 spacer 求和系统性偏小、anchoring delta 跟着偏。
    #[test]
    fn collect_heights_uses_margin_box_not_border_box() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 10);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 伪造 slot：border-box h=20 + margin top=3 bottom=8 → margin box = 31。
        {
            let scene = s.scene.as_mut().unwrap();
            let slots: Vec<(NodeId, usize)> = scene
                .lists
                .get(ul)
                .unwrap()
                .slots
                .iter()
                .map(|s| (s.node, s.item_index))
                .collect();
            for (node, _idx) in slots {
                let n = scene.get_mut(node).unwrap();
                n.layout_rect.h = 20.0;
                let ts = &mut n.base_style.taffy_style;
                ts.margin.top = taffy::style::LengthPercentageAuto::length(3.0);
                ts.margin.bottom = taffy::style::LengthPercentageAuto::length(8.0);
            }
        }
        crate::list::collect_heights(s.scene.as_mut().unwrap());
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        approx_eq(ls.heights.height_of(0), 31.0);
        // 占位引用 pane（构造 helper 返回它；后续 anchoring 测也用）。
        let _ = pane;
    }

    /// anchoring 补偿：本帧回填修正了 estimate → head 区间（仍用 estimate 的未测项）
    /// 总和变化，delta≠0 → 同帧把祖先 ScrollPane.scroll_pos.y += delta（内容不动）。
    /// 触发路径：head 区间项未测（用 estimate），visible 区 slot 本帧首次实测 →
    /// recompute_estimate 改 estimate → head sum 随之变 → anchoring 补 delta。
    #[test]
    fn anchoring_compensates_head_height_delta() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        // 预置：所有项 estimate=20（全未测），滚到 visible.start≈10。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            ls.heights.estimate = 20.0; // 全未测，head 区用此 estimate
                                        // 视口高 100 → 滚到 scroll_y=200（~10 项）→ visible.start≈10。
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 100.0);
            st.scroll_pos = (0.0, 200.0);
        }
        // 第一帧 plan/execute 让 slot 物化（visible≈[8..18]，含 BUFFER）。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let visible_start = s
            .scene
            .as_ref()
            .unwrap()
            .lists
            .get(ul)
            .unwrap()
            .visible
            .start;
        // 记 head 区间当前总和（基于 estimate=20）。
        let head_before = s
            .scene
            .as_ref()
            .unwrap()
            .lists
            .get(ul)
            .unwrap()
            .heights
            .sum(0..visible_start);
        // 模拟 solve：物化的 slot（visible 区）实测高度=30（≠ estimate 20）。
        // collect_heights 会回填这些 → recompute_estimate 把 estimate 从 20 拉到 30
        // → head 区（仍全未测，用 estimate）总和从 20*vs 变 30*vs → delta=10*vs。
        {
            let scene = s.scene.as_mut().unwrap();
            let slots: Vec<(NodeId, usize)> = scene
                .lists
                .get(ul)
                .unwrap()
                .slots
                .iter()
                .map(|s| (s.node, s.item_index))
                .collect();
            for (node, _idx) in slots {
                let n = scene.get_mut(node).unwrap();
                n.layout_rect.h = 30.0;
            }
        }
        let scroll_y_before = s
            .scene
            .as_ref()
            .unwrap()
            .scroll
            .get(pane)
            .unwrap()
            .scroll_pos
            .1;
        crate::list::collect_heights(s.scene.as_mut().unwrap());
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        let head_after = ls.heights.sum(0..visible_start);
        let scroll_y_after = scene.scroll.get(pane).unwrap().scroll_pos.1;
        let delta = head_after - head_before;
        assert!(
            delta.abs() > 0.001,
            "head sum should have changed via estimate update: {delta}"
        );
        approx_eq(scroll_y_after - scroll_y_before, delta);
        assert!(
            ls.anchoring_active,
            "anchoring_active must be set this frame"
        );
    }

    /// display:flex + gap>0 时，head spacer 必须保留 (count-1)*gap 的项间 gap，
    /// 使首个可见 slot 的 y 与非虚拟化参考一致。回归旧 `sum - gap` 公式：
    /// 多个隐藏项时只扣一个 gap，系统性偏小（visible.start 越大偏越多）。
    ///
    /// 反例（旧公式错）：3 项 [10,10,10]，gap=5，visible.start=1。
    ///   参考：item[1].top = sum(0..1) + 1*gap = 10 + 5 = 15。
    ///   虚拟化：slot.top = head_spacer.h + gap。要 slot.top=15 → head_spacer.h=10。
    ///   旧 `sum-gap`=10-5=5（slot 在 10，偏 5）。新 `sum+(count-1)*gap`=10+0=10（正确）。
    #[test]
    fn flex_gap_spacer_head_matches_non_virtualized_reference() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        // 设 ul 为 display:flex + gap:5。base_style 是 plan_one 读的源（from_nodes 不从
        // style 拷贝 base_style，须显式设）。
        {
            let scene = s.scene.as_mut().unwrap();
            let n = scene.get_mut(ul).unwrap();
            n.base_style.taffy_style.display = taffy::Display::Flex;
            n.base_style.taffy_style.gap.height = taffy::style::LengthPercentage::length(5.0);
        }
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 10);
        // 每项实测高 10。预填 HeightCache（跳过 solve，直接给已知高度）。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..10 {
                ls.heights.set(i, 10.0);
            }
            // 视口高 30 → 约 3 项可见；滚到 scroll_y=55 → first=5 → visible.start=3
            // （BUFFER=2 回退）。start>1 才能检验多项 head 区的 (count-1)*gap。
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 30.0);
            st.scroll_pos = (0.0, 55.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        assert_eq!(ops.len(), 1, "one ListView planned");
        let op = &ops[0];
        assert!(
            op.new_visible.start > 1,
            "precondition: visible.start>1 to exercise multi-gap head region, got {}",
            op.new_visible.start
        );
        // 参考：item[visible.start].top = sum(0..start) + start*gap。
        // 虚拟化：slot.top = head_spacer.h + gap → head_spacer.h = sum + (start-1)*gap。
        let start = op.new_visible.start;
        let expected_head = (start * 10) as f32 + ((start - 1) as f32) * 5.0;
        approx_eq(op.spacer_head_h, expected_head);
        // 旧 `sum - gap` 会偏小：expected_head - (start*gap)。断言差异明显（start>1）。
        let old_wrong = (start * 10) as f32 - 5.0;
        assert!(
            (op.spacer_head_h - old_wrong).abs() > 0.01,
            "spacer_head_h {} must differ from old wrong `sum-gap` {} ",
            op.spacer_head_h,
            old_wrong
        );
    }

    // ── Task 7：scroll_to_item / notify_* / refresh_items ──────────────────

    /// ScrollToItem：跑一次虚拟化管线（plan+execute）让目标 item 的 slot 同帧物化 +
    /// pending_binds 入队；设祖先 ScrollPane.scroll_pos.y 到 item 偏移（Instant）。
    /// 断言：drain 后目标 slot 在 slots 中（binds 入队）；scroll_pos.y ≈ sum(0..index)。
    #[test]
    fn scroll_to_item_drains_pipeline_and_targets_index() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        // 每项 20px，视口 100 → 滚到 item 50 偏移 1000。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            // content_size/overlap 设大，让 set_pos 不 clamp 掉目标。
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 100.0);
            st.content_size = (1000.0, 2000.0);
            st.overlap = (0.0, 1900.0);
            st.scroll_pos = (0.0, 0.0);
        }
        crate::list::scroll_to_item(&mut s, ul, 50, 0).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        // drain 后 binds 入队（同帧物化）。
        assert!(
            !ls.pending_binds.is_empty(),
            "drain should have queued binds for the newly-visible slots"
        );
        // scroll_pos.y 落到 item 50 的累积偏移 = 50*20 = 1000。
        let scroll_y = scene.scroll.get(pane).unwrap().scroll_pos.1;
        approx_eq(scroll_y, 1000.0);
    }

    /// 越界 index → Err（FFI 转 -1 → C# 抛 UIContractException）。
    #[test]
    fn scroll_to_item_out_of_range_errs() {
        let (mut s, ul, _li, _pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        assert!(crate::list::scroll_to_item(&mut s, ul, 5, 0).is_err());
        assert!(crate::list::scroll_to_item(&mut s, ul, 100, 0).is_err());
    }

    /// NotifyInserted：在 at 插 count 项 → heights.known 在 at 插 count 个 None；
    /// slot.item_index >= at 的 +count。原 idx=2 的 slot 插入后变 idx=3。
    #[test]
    fn notify_inserted_shifts_heights_and_slot_indices() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        // 实例化 5 个 slot（冷启动 INITIAL_SLOTS=5，正好全覆盖）。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 全填已知高度，便于验插入后插的是 None。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..5 {
                ls.heights.set(i, 10.0);
            }
        }
        // 在 at=2 插 1 项。
        crate::list::notify_inserted(s.scene.as_mut().unwrap(), ul, 2, 1).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.item_count, 6);
        assert_eq!(ls.heights.known.len(), 6);
        // idx 2 现在是 None（新插入的未知项）。
        assert!(
            ls.heights.known[2].is_none(),
            "inserted slot is unknown height"
        );
        // 原 idx 0,1 保持 Some(10)；idx 3+ （原 2,3,4）保持 Some(10)（移位不丢值）。
        assert_eq!(ls.heights.known[0], Some(10.0));
        assert_eq!(ls.heights.known[3], Some(10.0));
        // slot.item_index >= 2 的 +1：原 [0,1,2,3,4] → [0,1,3,4,5]。
        let indices: Vec<usize> = ls.slots.iter().map(|s| s.item_index).collect();
        let mut sorted = indices.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![0, 1, 3, 4, 5],
            "slots shifted past insert point"
        );
    }

    /// NotifyRemoved：删 [at, at+count) → heights.known drain 该区间；item_count -= count；
    /// 该区间 slot 回收（出 slots、入 free），其余 >end 的 slot.item_index -= count。
    #[test]
    fn notify_removed_drains_range_and_recycles_slots() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        // 冷启动 INITIAL_SLOTS=5 → 物化 items 0..5 全集（无滚动容器，viewport.h=0）。
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let slot_count_before = s.scene.as_ref().unwrap().lists.get(ul).unwrap().slots.len();
        assert_eq!(
            slot_count_before, 5,
            "precondition: all 5 items instantiated"
        );
        // 删 [2, 4)（删 2 项）：item 2,3 的 slot 回收；item 4 的 slot.item_index 4→2。
        crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 2, 2).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.item_count, 3);
        assert_eq!(ls.heights.known.len(), 3);
        // 回收了 2 个 slot（item_index 原 2,3）。
        assert_eq!(
            ls.slots.len(),
            slot_count_before - 2,
            "removed-range slots recycled out of slots"
        );
        // 剩余 slot：item 0,1 不变 + item 4→2。物理顺序应 [0,1,2]。
        let mut indices: Vec<usize> = ls.slots.iter().map(|s| s.item_index).collect();
        indices.sort_unstable();
        assert_eq!(
            indices,
            vec![0, 1, 2],
            "indices after end shifted down by count"
        );
        // 回收的 slot 入 free 池（下次克隆优先复用，不 leak）。
        assert_eq!(ls.free.len(), 2, "recycled slots pushed to free pool");
    }

    /// NotifyMoved：from→to 搬一项，heights.known 同步搬，slot.item_index 重映射。
    #[test]
    fn notify_moved_remaps_height_and_slot_index() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 给 idx 1 一个独特高度，验搬移后跟到 to。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            ls.heights.set(1, 77.0);
        }
        // 把 item 1 搬到 3（前→后）。
        crate::list::notify_moved(s.scene.as_mut().unwrap(), ul, 1, 3).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.heights.known[3], Some(77.0), "height moved from→to");
        assert_eq!(
            ls.heights.known[1], None,
            "from slot now holds the shifted item"
        );
        // slot.item_index：原绑 1 的 slot 现绑 3；原绑 2,3 的 slot 各前移 1（→1,2）。
        let mut indices: Vec<usize> = ls.slots.iter().map(|s| s.item_index).collect();
        indices.sort_unstable();
        assert_eq!(
            indices,
            vec![0, 1, 2, 3, 4],
            "indices still cover full range"
        );
    }

    /// notify 越界 → Err（at > item_count / count 溢出）。
    #[test]
    fn notify_out_of_range_errs() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        assert!(crate::list::notify_inserted(s.scene.as_mut().unwrap(), ul, 6, 1).is_err());
        assert!(crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 0, 6).is_err());
        assert!(crate::list::notify_moved(s.scene.as_mut().unwrap(), ul, 5, 0).is_err());
    }

    /// refresh_items：把 [start, start+count) 内已物化的 slot 重新入 pending_binds 队列，
    /// 让 C# 下帧重新 BindItem（业务数据刷新）。未物化的不重复入队。
    #[test]
    fn refresh_items_requeues_visible_slots_in_range() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 10);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 清空首次 execute 产的 binds，只看 refresh 入队的。
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // 当前冷启动 visible = [0,5)，refresh [1,3) → 应入队 slot 绑 1,2。
        crate::list::refresh_items(s.scene.as_mut().unwrap(), ul, 1, 2).unwrap();
        let binds = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        let mut idxs: Vec<usize> = binds.iter().map(|(_, i)| *i).collect();
        idxs.sort_unstable();
        assert_eq!(
            idxs,
            vec![1, 2],
            "refresh re-queues only in-range instantiated slots"
        );
    }
}
