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

/// 单个虚拟列表 slot（克隆出的实例根 + 它当前绑定的 item 序号）。
///
/// slot 从 `enter_data_driven` 预分配起永驻 ul 子树——离开可见区不 detach，只标 `parked`
/// （置 display:none 便签），保住 NodeId / parent / reuse_key，后端 GO 随之永驻不重建。
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub node: NodeId,
    /// 当前绑的 item；parked 时保留上次值（stale，仅作复用参考）。
    pub item_index: usize,
    /// true = 休眠（display:none override 已置，不占布局、不渲染）。
    pub parked: bool,
}

/// ListView 运行时虚拟化状态。每 ul（NodeKind::ListView）一个槽。
///
/// - `slots`：已实例化的 slot（克隆出的 li），高水位——只增不减，永不 detach。
/// - `visible`：上帧算出的可见 item 区间 [start, end)。
/// - `pending_binds`：本帧新绑定的 (slot_node, item_index)，由 bind 阶段（Task 6）消费。
/// - `anchoring_active` / `dirty`：anchoring / 静默刷新标记（预留，Task 5+ 用）。
#[derive(Debug, Clone)]
pub struct ListState {
    pub item_count: usize,
    /// 克隆模板的游离根（enter_data_driven 备份的设计期 li）。
    pub template_root: Option<NodeId>,
    pub heights: HeightCache,
    pub slots: Vec<Slot>,
    pub visible: std::ops::Range<usize>,
    pub head_spacer: NodeId,
    pub tail_spacer: NodeId,
    pub pending_binds: Vec<(NodeId, usize)>,
    pub list_ordinal: u32,
    pub anchoring_active: bool,
    pub dirty: bool,
    /// wrap 网格虚拟化（spec §311）：ul.style 为 flex-row+wrap 时 true，按行虚拟化、行内全量。
    /// 单列（含 flex-column、block）为 false，走原有 1D item 路径。
    pub grid: bool,
    /// 网格每行列数（grid=true 时首帧 solve 后测得；0=尚未测，退化为冷启动定数）。
    pub columns: usize,
    /// 行距 = item_h + gap_y（grid 测得 columns 时一并填）。单列不用。
    pub row_pitch: f32,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            item_count: 0,
            template_root: None,
            heights: HeightCache::default(),
            slots: Vec::new(),
            visible: 0..0,
            head_spacer: NodeId::INVALID,
            tail_spacer: NodeId::INVALID,
            pending_binds: Vec::new(),
            list_ordinal: 0,
            anchoring_active: false,
            dirty: true,
            grid: false,
            columns: 0,
            row_pitch: 0.0,
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
    // 短期不可变借：校验 kind + height + 解析模板来源（spec §6.3：<template> 子优先，
    // 兜底设计期 li）。不能跨 clone_subtree 持有 scene 借（clone_subtree 也要 &mut stage）。
    let (blueprint, all_children): (Option<NodeId>, Vec<NodeId>) = {
        let scene = stage.scene.as_ref().ok_or("no scene")?;
        if scene.get(ul).map(|n| n.kind) != Some(NodeKind::ListView) {
            return Err("enter_data_driven: node is not a ListView".into());
        }
        check_ul_height_auto(scene, ul)?;
        let ul_node = scene.get(ul).unwrap();
        // <template> 子（NodeKind::Template）：spec §6.3 要求恰好一个，多个是契约违反。
        let templates: Vec<NodeId> = ul_node
            .children
            .iter()
            .copied()
            .filter(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::Template))
            .collect();
        if templates.len() > 1 {
            return Err("ListView 下有多个 <template>：自动采用要求恰好一个（spec §6.3）".into());
        }
        // 蓝图 = <template> 内的首个 ListItem（packer 保留 template 子树，fence 已校验恰一个）。
        let blueprint = templates.first().and_then(|&tpl| {
            scene.get(tpl).and_then(|tn| {
                tn.children
                    .iter()
                    .copied()
                    .find(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem))
            })
        });
        // 兜底：ul 直接 ListItem 子（设计期 li 写法）。
        let blueprint = blueprint.or_else(|| {
            ul_node
                .children
                .iter()
                .copied()
                .find(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem))
        });
        (blueprint, ul_node.children.clone())
    };
    // 先 clone 蓝图到游离态（需 &mut stage，此时无 scene 借），再清空 ul 全部设计期子
    // （adopted <template> 子树 + 设计期 li + 标签间空白 TextNode），使 ul 仅剩 spacer+slot。
    let Some(bp) = blueprint else {
        return Err("ListView 无模板来源：无 <template>、无设计期 li、未设 ItemTemplate".into());
    };
    let template_root = stage.clone_subtree(bp)?;
    for child in &all_children {
        stage.remove_node(*child);
    }
    let head = stage.create_node("div", "")?;
    let tail = stage.create_node("div", "")?;
    configure_spacer(stage, head);
    configure_spacer(stage, tail);
    stage.append_child(ul, head)?;
    stage.append_child(ul, tail)?;
    // 预分配初始 batch：INITIAL_SLOTS 个 slot 现在就克隆好、挂在 head/tail spacer 之间，
    // 全部 parked（display:none）。slot 从此永驻 ul 子树，只翻 display + 换绑，永不 detach
    // ——后端 GO 随稳定 reuse_key 永驻，滞后一帧的重建闪烁随之消失。
    let mut slots = Vec::with_capacity(INITIAL_SLOTS);
    for ordinal in 0..INITIAL_SLOTS {
        let node = stage.clone_subtree(template_root)?;
        stage.insert_before(ul, node, tail)?;
        let scene = stage.scene.as_mut().ok_or("no scene")?;
        // LOOKUP_SCOPE（不打 SCOPE_ROOT：spec §6.2，slot 根 CSS 规则仍按页面根 scope 匹配）。
        if let Some(n) = scene.get_mut(node) {
            n.interaction.flags.insert(NodeFlags::LOOKUP_SCOPE);
        }
        // reuse_key 出生即定（ordinal = slots 下标，slots 只增不减 → key 永不旋转）。
        crate::scene::dynamic::set_reuse_key(scene, node, encode_reuse_key(list_ordinal, ordinal));
        crate::scene::dynamic::set_inline_override(scene, node, "display:none")?;
        slots.push(Slot {
            node,
            item_index: 0,
            parked: true,
        });
    }
    let ls = ListState {
        item_count: 0,
        template_root: Some(template_root),
        heights: HeightCache::new(0, 0.0),
        slots,
        visible: 0..0,
        head_spacer: head,
        tail_spacer: tail,
        pending_binds: Vec::new(),
        list_ordinal,
        anchoring_active: false,
        dirty: true,
        grid: false,
        columns: 0,
        row_pitch: 0.0,
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
///
/// `gap`：flex 列表项间 gap（px）。**必须计入**累积位——实际布局（spacer + taffy flex+gap）
/// 把 item i 放在 sum(h[0..i]) + i*gap，若此处漏算 gap 会低估值位置 → start 偏晚 →
/// 视口顶部空白（mail gap:12 实例：scroll 5000 时差 ~458px）。block ul gap=0 no-op。
pub fn compute_visible_range(
    item_count: usize,
    scroll_pos_y: f32,
    listview_offset: f32,
    viewport_h: f32,
    heights: &HeightCache,
    gap: f32,
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
    // first = 首个底边超过 top 的项（累积后判，含 gap）。若全项底边 ≤ top（内容短于视口），
    // 循环不 break，first 保持 0 → start 经 BUFFER 回退到 0，整列可见。
    // bottom(i) = sum(h[0..i+1]) + i*gap（i 个 gap 在 item i 之前）。
    let mut acc = 0.0;
    let mut first = 0usize;
    for i in 0..item_count {
        acc += heights.height_of(i);
        if acc + (i as f32) * gap > top {
            first = i;
            break;
        }
    }
    let target = top + viewport_h;
    let mut acc2 = 0.0;
    let mut last = item_count;
    for j in 0..item_count {
        acc2 += heights.height_of(j);
        if acc2 + (j as f32) * gap >= target {
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
///
/// TODO(Smooth recompute)：上面算出的 target 是一次性的——基于当前 heights 快照。
/// Instant 路径（behavior==0）同帧 drain_now → 下帧 anchoring 即修正，故正确。
/// 但 Smooth 路径把 target 喂给 ScrollPane 的 tween 后就不再重算：变高列表滚动过程中
/// 新可见项陆续测量、overlap 增长，tween 目标却停留在初始 overlap 边界，远距离 Smooth
/// 滚动会停在偏差位置。spec §5 要求 tween 期间按回填高度重算 target，当前未实现
/// （测试仅覆盖 Instant 路径）。
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

/// 删除通知（spec §10 NotifyRemoved）：删 [at, at+count) 项。越界（at+count > item_count）→ Err。
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

/// 刷新通知（spec §10 RefreshItems）：把 [start, start+count) 内**当前 active**的 slot
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

/// plan 阶段的待执行操作（与 execute 解耦，避开 clone_subtree 的 &mut Stage 与
/// plan 的 &mut Scene 借用冲突）。单个 ListView 一条。
pub struct PendingOps {
    pub list_ul: NodeId,
    /// 本帧需绑定的 item 序号（visible − 已有 active slot 绑的）。execute 优先 unpark 复用
    /// 池里的 parked slot，池空才克隆扩容——故是「待绑定」而非「待克隆」。
    pub to_bind: Vec<usize>,
    pub new_visible: std::ops::Range<usize>,
    pub spacer_head_h: f32,
    pub spacer_tail_h: f32,
}

/// 网格检测（一次性）：class 样式（如 .grid）在 solve 时才解析进 node.style，故在 plan_one
/// （首帧 solve 后）检测。首次命中 flex-row+wrap 时标 ls.grid 并把 spacer 改全宽（独占行，
/// 作纵向推力；否则进列流致后续行左偏）。后续帧 ls.grid 已 true 直接跳过。
fn ensure_grid_detected(scene: &mut Scene, ul: NodeId) {
    if scene.lists.get(ul).map(|ls| ls.grid).unwrap_or(false) {
        return;
    }
    let is_grid = scene
        .get(ul)
        .map(|n| {
            let ts = &n.style.taffy_style;
            ts.flex_direction == taffy::style::FlexDirection::Row
                && ts.flex_wrap == taffy::style::FlexWrap::Wrap
        })
        .unwrap_or(false);
    if !is_grid {
        return;
    }
    let spacers = scene
        .lists
        .get(ul)
        .map(|ls| (ls.head_spacer, ls.tail_spacer));
    if let Some(ls) = scene.lists.get_mut(ul) {
        ls.grid = true;
    }
    if let Some((h, t)) = spacers {
        for spacer in [h, t] {
            if let Some(n) = scene.get_mut(spacer) {
                let full = taffy::style::Dimension::percent(1.0);
                n.base_style.taffy_style.size.width = full;
                n.style.taffy_style.size.width = full;
                n.dirty_mesh = true;
            }
        }
    }
}

/// 测网格列数 + 行距（首帧 solve 后调）。slot0 已布局，从 ul.style 读 gap/padding。
/// 列数 = floor((content_w + gap_x) / (item_w + gap_x))；行距 = item_h + gap_y。
fn measure_grid(scene: &Scene, ul: NodeId, slot0: NodeId) -> Option<(usize, f32)> {
    let slot = scene.get(slot0)?;
    let uln = scene.get(ul)?;
    let ts = &uln.style.taffy_style;
    let gap_x = crate::render::resolve_lp(ts.gap.width);
    let gap_y = crate::render::resolve_lp(ts.gap.height);
    let pad_l = crate::render::resolve_lp(ts.padding.left);
    let pad_r = crate::render::resolve_lp(ts.padding.right);
    let content_w = (uln.layout_rect.w - pad_l - pad_r).max(0.0);
    let item_w = slot.layout_rect.w.max(1.0);
    let columns = ((content_w + gap_x) / (item_w + gap_x)).floor().max(1.0) as usize;
    let row_pitch = slot.layout_rect.h + gap_y;
    Some((columns, row_pitch))
}

/// 网格按行可见区 + spacer 高度（spec §311：按行虚拟化、行内全量）。
/// 行 r 占 [r*row_pitch, r*row_pitch+row_h]；BUFFER 行；spacer 高含 gap_y 补偿
/// （首 slot 行位置 = spacer_h + gap_y，对齐非虚拟基准 r*row_pitch）。
fn grid_visible_spacers(
    item_count: usize,
    columns: usize,
    row_h: f32,
    gap_y: f32,
    scroll_y: f32,
    ul_y: f32,
    viewport_h: f32,
) -> (std::ops::Range<usize>, f32, f32) {
    if item_count == 0 || columns == 0 {
        return (0..0, 0.0, 0.0);
    }
    let total_rows = item_count.div_ceil(columns);
    let row_pitch = row_h + gap_y;
    if viewport_h <= 0.0 || row_pitch <= 0.0 {
        // 冷启动 / 行距未就绪：前 BUFFER 行（整行），供下帧测量与全量填充。
        let r = BUFFER.min(total_rows);
        return (0..(r * columns).min(item_count), 0.0, 0.0);
    }
    let top = scroll_y - ul_y;
    let view_bottom = top + viewport_h;
    // first = 首个底边越过 top 的行（至少部分在视口内）。
    let mut first = 0usize;
    for r in 0..total_rows {
        if r as f32 * row_pitch + row_h > top {
            first = r;
            break;
        }
    }
    // last = 首个顶边抵达视口底的行（exclusive）。
    let mut last = total_rows;
    for r in 0..total_rows {
        if r as f32 * row_pitch >= view_bottom {
            last = r;
            break;
        }
    }
    let start_row = first.saturating_sub(BUFFER);
    let end_row = (last + BUFFER).min(total_rows);
    let start_item = start_row * columns;
    let end_item = (end_row * columns).min(item_count);
    let hidden_head = start_row;
    let hidden_tail = total_rows - end_row;
    let spacer_head_h =
        (hidden_head as f32 * row_h + hidden_head.saturating_sub(1) as f32 * gap_y).max(0.0);
    let spacer_tail_h =
        (hidden_tail as f32 * row_h + hidden_tail.saturating_sub(1) as f32 * gap_y).max(0.0);
    (start_item..end_item, spacer_head_h, spacer_tail_h)
}

/// 可见 item 区间 + head/tail spacer 高度。网格（grid）走按行路径；单列 / block 走 1D item 路径。
fn compute_visible_spacers(
    ls: &ListState,
    scene: &Scene,
    ul: NodeId,
    columns: usize,
    row_pitch: f32,
    scroll_y: f32,
    ul_y: f32,
    viewport_h: f32,
) -> (std::ops::Range<usize>, f32, f32) {
    if ls.grid {
        let gap_y = crate::render::resolve_lp(scene.get(ul).unwrap().style.taffy_style.gap.height);
        let row_h = (row_pitch - gap_y).max(0.0);
        return grid_visible_spacers(
            ls.item_count,
            columns,
            row_h,
            gap_y,
            scroll_y,
            ul_y,
            viewport_h,
        );
    }
    // 单列 1D（原路径）。
    let gap = if matches!(
        scene.get(ul).unwrap().base_style.taffy_style.display,
        taffy::Display::Flex
    ) {
        crate::render::resolve_lp(scene.get(ul).unwrap().base_style.taffy_style.gap.height)
    } else {
        0.0
    };
    let visible =
        compute_visible_range(ls.item_count, scroll_y, ul_y, viewport_h, &ls.heights, gap);
    // Gap accounting for flex+gap uls: [head_spacer, slot.., tail_spacer]，可见 slot 在 head spacer
    // 后一个 gap。为对齐非虚拟基准（item[k].top = sum(0..k) + k*gap），head spacer 须保留
    // sum(0..start) + (start-1)*gap：slot.top = spacer.h + gap = sum + count*gap。tail 对称。
    // count=0 → saturating_sub(1)=0（空 spacer 无 gap）。block ul 的 gap=0，本项 no-op。
    let head_count = visible.start;
    let tail_count = ls.item_count.saturating_sub(visible.end);
    let spacer_head_h =
        (ls.heights.sum(0..visible.start) + (head_count.saturating_sub(1) as f32) * gap).max(0.0);
    let spacer_tail_h = (ls.heights.sum(visible.end..ls.item_count)
        + (tail_count.saturating_sub(1) as f32) * gap)
        .max(0.0);
    (visible, spacer_head_h, spacer_tail_h)
}

/// plan 阶段：算可见区、把离开可见区的 slot 标 parked（就地休眠，不 detach）、产待绑定
/// item 列表（`to_bind`）。**只借 scene**（clone_subtree 不在此调），不建树、不入 bind 队列
/// ——那是 execute 的活。tick_and_render 先调 plan_visible 再调 execute_visible。
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
    // 网格检测（首帧 solve 后 style 已解析；一次性）。
    ensure_grid_detected(scene, ul);
    // Phase A：单次不可变借完成所有只读计算——可见区（Copy 的 Range）+ spacer 高度 + gap。
    // spacer 高需 heights.sum，故一并在此算出，避免后续跨可变借再 clone heights。
    let (scroll_y, viewport_h, ul_y) = {
        let (sy, vh) = ancestor_scroll_viewport(scene, ul);
        let uy = scene.get(ul).map(|n| n.layout_rect.y).unwrap_or(0.0);
        (sy, vh, uy)
    };
    let (visible, spacer_head_h, spacer_tail_h, measured) = {
        let ls = scene.lists.get(ul)?;
        // 网格且未测列数：据已布局 slot[0] + ul 几何测 columns/row_pitch（首帧 solve 后）。
        let measured = if ls.grid && ls.columns == 0 {
            ls.slots
                .first()
                .map(|s| s.node)
                .and_then(|s| measure_grid(scene, ul, s))
        } else {
            None
        };
        let (columns, row_pitch) = measured.unwrap_or((ls.columns, ls.row_pitch));
        let res = compute_visible_spacers(
            ls, scene, ul, columns, row_pitch, scroll_y, ul_y, viewport_h,
        );
        (res.0, res.1, res.2, measured)
    };
    // 测得则回写 ListState（上方块借已释放）。
    if let Some((c, rp)) = measured {
        if let Some(ls) = scene.lists.get_mut(ul) {
            ls.columns = c;
            ls.row_pitch = rp;
        }
    }
    // Phase B：可变借标记离开可见区的 slot（park 而非 detach）。display:none 便签在 Phase C
    // 写（set_inline_override 另借 scene，与本处 ls 可变借冲突）。to_bind 也在此算出。
    let new_set: std::collections::HashSet<usize> = visible.clone().collect();
    let (to_bind, to_park): (Vec<usize>, Vec<NodeId>) = {
        let ls = scene.lists.get_mut(ul)?;
        let mut to_park = Vec::new();
        for s in ls.slots.iter_mut() {
            if !new_set.contains(&s.item_index) && !s.parked {
                s.parked = true;
                to_park.push(s.node);
            }
        }
        // 待绑定 = visible − 已有 active slot 绑的 indices。parked slot 的 item_index 是 stale
        // 复用参考，不算「已绑」——它得等 execute unpark 后才重新 bind。
        let bound_items: std::collections::HashSet<usize> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| s.item_index)
            .collect();
        let to_bind = visible
            .clone()
            .filter(|i| !bound_items.contains(i))
            .collect();
        (to_bind, to_park)
    };
    // Phase C：给刚 park 的 slot 写 display:none 便签——留挂 ul（NodeId/parent/reuse_key 不变），
    // 同帧 rematch 拷进 node.style → taffy 跳、render 剪枝。不再有 detach/free 池。
    for node in &to_park {
        let _ = crate::scene::dynamic::set_inline_override(scene, *node, "display:none");
    }
    Some(PendingOps {
        list_ul: ul,
        to_bind,
        new_visible: visible,
        spacer_head_h,
        spacer_tail_h,
    })
}

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
                // 标 LOOKUP_SCOPE（不打 SCOPE_ROOT：spec §6.2，slot 根 CSS 规则仍按页面根 scope 匹配）。
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

/// 按 item_index 升序重排 ul.children 里的 **active** slot。
///
/// 池化模型下 slot 永不 detach，unpark 只翻 display + 换绑，节点会留在上次的物理位置。
/// 而 active slot 由 CSS 流在 head/tail spacer 之间依序排布——`ul.children` 顺序 **即视觉顺序**，
/// 不重排则被复用的 slot 渲染到错位（滞后的 item 翻到前面）。
///
/// parked slot（display:none）不占布局，物理位置任意——统一挡在 active 之后、tail spacer 之前。
/// head/tail spacer 位置不变（首子 / 末子）；非 slot 的意外子保序附在末尾。
/// 只改 `children` 排列，parent 不变（无需 remove_child/insert_before 的摘挂往返）。
fn reorder_active_slots(scene: &mut Scene, ul: NodeId) {
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
        let r = compute_visible_range(100, 0.0, 0.0, 100.0, &uniform_heights(100, 10.0), 0.0);
        assert_eq!(r, 0..12);
    }

    #[test]
    fn visible_range_counts_flex_gap_in_item_positions() {
        // 复现 mail 覆盖缺口：flex gap:12 把 item 撑开（item i 顶边 = sum(h[0..i]) + i*gap），
        // compute_visible_range 漏算 gap 会低估值位置 → start 偏晚 → 视口顶部空白。
        // 100 item × h75 + gap12，scroll 到 item 50 顶边（= 50*75 + 50*12 = 4350）。
        let h = uniform_heights(100, 75.0);
        let gap = 12.0_f32;
        let top = 50.0 * 75.0 + 50.0 * gap; // item 50 顶边
        let r = compute_visible_range(100, top, 0.0, 965.0, &h, gap);
        // item 50 顶边 == top，其底边(4425) > top → 部分可见 → first=50 → start=48(BUFFER)。
        // 漏 gap 时 first 会到 58（start 56）——这就是 live mail 顶部空白的根因。
        assert!(
            (48..=50).contains(&r.start),
            "gap 计入后 start 应 ~48，got {} (漏 gap 会给 56)",
            r.start
        );
    }

    #[test]
    fn visible_range_scrolled_mid() {
        let r = compute_visible_range(100, 50.0, 0.0, 100.0, &uniform_heights(100, 10.0), 0.0);
        assert_eq!(r, 3..17);
    }

    #[test]
    fn visible_range_clamps_to_count() {
        let r = compute_visible_range(5, 50.0, 0.0, 100.0, &uniform_heights(5, 10.0), 0.0);
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn visible_range_empty_count() {
        let r = compute_visible_range(0, 0.0, 0.0, 100.0, &HeightCache::new(0, 10.0), 0.0);
        assert_eq!(r, 0..0);
    }

    #[test]
    fn visible_range_cold_start_viewport_zero() {
        let r = compute_visible_range(1000, 0.0, 0.0, 0.0, &uniform_heights(1000, 10.0), 0.0);
        assert_eq!(r, 0..INITIAL_SLOTS);
    }

    #[test]
    fn grid_visible_full_rows_and_spacers() {
        // 200 项 × 5 列，row_h=120 gap_y=12（row_pitch=132）。视口 965 ≈ 7.3 行。
        // first=0，last=8（8*132=1056≥965），BUFFER→start 0 end 10 → 整 10 行 = 50 项。
        let (r, head, tail) = grid_visible_spacers(200, 5, 120.0, 12.0, 0.0, 0.0, 965.0);
        assert_eq!(r, 0..50, "full rows 0..10");
        approx_eq(head, 0.0);
        // tail = 30 hidden rows * 120 + 29 * 12 = 3948
        approx_eq(tail, 3948.0);
    }

    #[test]
    fn grid_visible_advances_by_rows_on_scroll() {
        // scroll=1000：first=7（7*132+120=1044>1000）→start_row 5；last=15→end_row 17。
        let (r, head, _tail) = grid_visible_spacers(200, 5, 120.0, 12.0, 1000.0, 0.0, 965.0);
        assert_eq!(r, 25..85, "rows 5..17 = items 25..85");
        // head = 5 rows * 120 + 4 * 12 = 648
        approx_eq(head, 648.0);
    }

    #[test]
    fn grid_visible_clamps_partial_last_row() {
        // 47 项 × 5 列 = 10 行（末行 2 项）。整页可见时 end 须 clamp 到 47（不超 item_count）。
        let (r, _h, _t) = grid_visible_spacers(47, 5, 120.0, 12.0, 0.0, 0.0, 965.0);
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 47, "end clamps to item_count (partial last row)");
    }

    #[test]
    fn grid_visible_cold_start_returns_buffer_rows() {
        // viewport<=0 → 冷启动返前 BUFFER 整行，spacer 为 0（供下帧测列数 + 全量填充）。
        let (r, head, tail) = grid_visible_spacers(200, 5, 120.0, 12.0, 0.0, 0.0, 0.0);
        assert_eq!(r, 0..(BUFFER * 5), "BUFFER rows worth of items");
        approx_eq(head, 0.0);
        approx_eq(tail, 0.0);
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
    /// 中位于 head_spacer 之后 / tail_spacer 之前。**active** slot 须按 item_index 严格递增
    /// （ul.children 顺序即 CSS 流的视觉顺序，复用后不重排会让 slot 渲染错位）；
    /// parked slot 是 display:none，不占布局，物理位置任意（spec §2.9）。
    /// 同时检 ul.children 无重复 NodeId。
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
        // active slot → item_index 映射（parked 的 item_index 是 stale 复用参考，不参与定序）。
        let active_of: std::collections::HashMap<NodeId, usize> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| (s.node, s.item_index))
            .collect();
        let all_slots: std::collections::HashSet<NodeId> =
            ls.slots.iter().map(|s| s.node).collect();
        // 逐 slot：parent 正确 + 在 head/tail 之间。并收集 active 的物理顺序。
        let mut physical_order: Vec<usize> = Vec::new();
        for &c in &ul_node.children[1..ul_node.children.len() - 1] {
            let cn = scene.get(c).unwrap();
            assert_eq!(cn.parent, Some(ul), "slot parent must be ul");
            assert!(all_slots.contains(&c), "child maps to a slot");
            if let Some(&idx) = active_of.get(&c) {
                physical_order.push(idx);
            }
        }
        // active slot 的物理顺序严格递增（unpark 就地复用后未重排会让顺序漂移、渲染错位）。
        let mut sorted = physical_order.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            physical_order, sorted,
            "active slot physical order must match sorted item_index (no drift)"
        );
    }

    #[test]
    fn enter_data_driven_creates_spacers_and_backups_li() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ul_node = scene.get(ul).unwrap();
        // 设计期子已清光：ul 下只剩 head/tail spacer + 预分配的 parked 初始 batch。
        assert_eq!(
            ul_node.children.len(),
            2 + INITIAL_SLOTS,
            "ul has spacers + pre-allocated parked batch only"
        );
        let ls = scene.lists.get(ul).expect("list state created");
        assert!(
            ls.template_root.is_some(),
            "design-time li backed up as template"
        );
    }

    /// 池化模型起点：`enter_data_driven` 预分配初始 batch —— INITIAL_SLOTS 个 slot 全部
    /// 克隆好并挂在 ul 上（head/tail spacer 之间），初始全 parked（display:none 便签已置）。
    /// 不再有 free 池（`ListState.free` 已删——本测能编译即证），slot 从生到死不 detach。
    ///
    /// display:none 是**便签层**（inline_override + inline_set bit），由下帧 rematch 拷进
    /// node.style 才真正生效；本测无 tick，故验便签位已置而非解析后的 style。
    #[test]
    fn enter_data_driven_pre_allocates_parked_slots() {
        use crate::style::dynamic::INLINE_DISPLAY;
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).expect("list state created");
        assert_eq!(ls.slots.len(), INITIAL_SLOTS, "pre-allocated initial batch");
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(
            ul_node.children.len(),
            2 + INITIAL_SLOTS,
            "ul = head spacer + INITIAL_SLOTS slots + tail spacer"
        );
        assert_eq!(
            ul_node.children.first(),
            Some(&ls.head_spacer),
            "head spacer first"
        );
        assert_eq!(
            ul_node.children.last(),
            Some(&ls.tail_spacer),
            "tail spacer last"
        );
        let mut keys = std::collections::HashSet::new();
        for slot in &ls.slots {
            let n = scene.get(slot.node).expect("slot node live");
            assert_eq!(
                n.parent,
                Some(ul),
                "slot attached under ul (never detached)"
            );
            assert!(slot.parked, "initial batch is all parked");
            assert_ne!(
                n.inline_set.0 & INLINE_DISPLAY,
                0,
                "display inline override bit set on parked slot"
            );
            assert_eq!(
                n.inline_override.taffy_style.display,
                taffy::Display::None,
                "parked slot's inline override value is display:none"
            );
            // 永久 ordinal：出生即定 key，不为 0（0 = MirrorPool 的“无 key”）、互不重复。
            assert_ne!(n.reuse_key, 0, "slot keyed at birth");
            assert!(
                keys.insert(n.reuse_key),
                "each slot has a distinct reuse_key"
            );
            assert!(
                n.interaction.flags.contains(NodeFlags::LOOKUP_SCOPE),
                "slot root carries LOOKUP_SCOPE"
            );
        }
    }

    /// 作者写 `<div role=list><template><div role=listitem>…</div></template></div>`：
    /// packer 把 `<template>` 保留为 NodeKind::Template 子（v27+），其下 ListItem 才是蓝图。
    /// enter_data_driven 须采用 template 内的 ListItem 作模板源（spec §6.3 step 2）。
    fn stage_with_ul_template_li() -> (crate::stage::Stage, NodeId) {
        use crate::scene::node::{Node, NodeKind};
        let ul = Node {
            kind: NodeKind::ListView,
            ..Node::default()
        };
        let tpl = Node {
            kind: NodeKind::Template,
            ..Node::default()
        };
        let li = Node {
            kind: NodeKind::ListItem,
            ..Node::default()
        };
        let scene = crate::scene::node::Scene::from_nodes(vec![ul, tpl, li], vec![(0, 1), (1, 2)]);
        let ul = scene.roots[0];
        let mut s = crate::stage::Stage::new_for_test();
        s.scene = Some(scene);
        (s, ul)
    }

    #[test]
    fn enter_data_driven_adopts_template_child() {
        let (mut s, ul) = stage_with_ul_template_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ul_node = scene.get(ul).unwrap();
        // ul 只剩 head/tail spacer + 预分配的 parked 初始 batch：adopted <template> 子树已清。
        assert_eq!(
            ul_node.children.len(),
            2 + INITIAL_SLOTS,
            "ul has spacers + pre-allocated parked batch only"
        );
        let ls = scene.lists.get(ul).expect("list state created");
        assert!(
            ls.template_root.is_some(),
            "template blueprint (ListItem inside <template>) adopted as template source"
        );
    }

    #[test]
    fn enter_data_driven_rejects_multiple_templates() {
        // spec §6.3：ul 下恰好一个 <template> 才自动采用；多个是契约违反。
        use crate::scene::node::{Node, NodeKind};
        let ul = Node {
            kind: NodeKind::ListView,
            ..Node::default()
        };
        let tpl1 = Node {
            kind: NodeKind::Template,
            ..Node::default()
        };
        let li1 = Node {
            kind: NodeKind::ListItem,
            ..Node::default()
        };
        let tpl2 = Node {
            kind: NodeKind::Template,
            ..Node::default()
        };
        let li2 = Node {
            kind: NodeKind::ListItem,
            ..Node::default()
        };
        let scene = crate::scene::node::Scene::from_nodes(
            vec![ul, tpl1, li1, tpl2, li2],
            vec![(0, 1), (1, 2), (0, 3), (3, 4)],
        );
        let ul = scene.roots[0];
        let mut s = crate::stage::Stage::new_for_test();
        s.scene = Some(scene);
        let err = crate::list::enter_data_driven(&mut s, ul, 0)
            .expect_err("multiple <template> should be rejected");
        assert!(err.contains("多个 <template>"), "got: {err}");
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

    /// 复用路径回归：滚后部分 slot 离开可见区→park，下一帧被 unpark 复用给新 item。
    /// unpark 是就地复用（不搬运节点），若不重排 ul.children，被复用 slot 会停在旧位——
    /// 而 active slot 由 CSS 流在 head/tail spacer 之间排布，物理顺序即视觉顺序，乱序 = 渲染错位。
    /// 此测模拟两次帧，每帧断言 active slot 仍按 item_index 升序。
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
        assert_all_slots_well_parented(scene, ul);

        // 第二帧：滚下 100px（~5 项）→ 可见 3..12。items 0,1,2 离开→park，被 unpark 复用给 7,8,9。
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
    /// 预分配的 parked slot 挂在 ul 下，同样须随 ul 递归释放（高水位池只在组件销毁时整批回收）。
    #[test]
    fn remove_node_frees_template_root_subtree() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let (template_root, slot_nodes) = {
            let ls = s.scene.as_ref().unwrap().lists.get(ul).unwrap();
            (
                ls.template_root.expect("template backed up"),
                ls.slots.iter().map(|s| s.node).collect::<Vec<_>>(),
            )
        };
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
        for node in slot_nodes {
            assert!(
                s.scene.as_ref().unwrap().get(node).is_none(),
                "pre-allocated parked slot freed with ul (no leak)"
            );
        }
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

    /// drain_pending_binds_bounded：队列超出 max 时只取前端 max 条，余量留下次调用。
    /// 这是 FFI cap 不足时的安全网——保证不丢 bind（余条留队列等下帧再取），
    /// 而非像 take_pending_binds 全取后在 cap 外丢掉。
    #[test]
    fn drain_pending_binds_bounded_leaves_remainder_for_next_call() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_mut().unwrap();
        let total = crate::list::INITIAL_SLOTS;
        // max 小于队列长度：只取 max 条，余条留队列。
        let first = crate::list::drain_pending_binds_bounded(scene, ul, 2);
        assert_eq!(first.len(), 2, "bounded drain respects max");
        // 余条仍在队列：再取剩下的。
        let rest = crate::list::drain_pending_binds_bounded(scene, ul, total);
        assert_eq!(rest.len(), total - 2, "remainder stays for next call");
        // 队列已空。
        let third = crate::list::drain_pending_binds_bounded(scene, ul, total);
        assert!(third.is_empty(), "queue drained");
        // 取出的合起来等于全队，无重复无丢失。
        let mut all: Vec<usize> = first.into_iter().chain(rest).map(|(_, idx)| idx).collect();
        all.sort();
        let expected: Vec<usize> = (0..total).collect();
        assert_eq!(all, expected, "no bind lost or duplicated");
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

    /// 回归：parked slot（display:none → layout_rect.h=0）不更新 HeightCache。
    /// parked slot 的 item_index 是 stale 复用参考——若不加跳过，会把 0.0 写成对应
    /// item 的 known 高度，污染下帧可见区计算（坑 182 侧效应）。
    #[test]
    fn collect_heights_skips_parked_slots() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 每 slot 给一个可区分的布局高度：item 0→10, 1→20, 2→30, 3→40, 4→50。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            let slots: Vec<(NodeId, usize)> =
                ls.slots.iter().map(|s| (s.node, s.item_index)).collect();
            for (node, idx) in slots {
                let n = scene.get_mut(node).unwrap();
                n.layout_rect.h = (idx as f32 + 1.0) * 10.0;
            }
        }
        // 首轮回填：缓存 5 项真实高度（10/20/30/40/50）。
        crate::list::collect_heights(s.scene.as_mut().unwrap());
        // 手动 park 第 3 个 slot（item_index=2），layout_rect.h 坠零（模拟 display:none 后 solve）。
        {
            let scene = s.scene.as_mut().unwrap();
            let node = scene.lists.get(ul).unwrap().slots[2].node;
            // 分两次可变借：先改 slot 状态，再改 node 的 layout_rect。
            scene.lists.get_mut(ul).unwrap().slots[2].parked = true;
            scene.get_mut(node).unwrap().layout_rect.h = 0.0;
        }
        // 二轮回填：parked 跳过 → known[2] 不应被污染为 0。
        crate::list::collect_heights(s.scene.as_mut().unwrap());
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert!(
            ls.heights.height_of(2) > 0.0,
            "parked slot should not overwrite height cache with zero"
        );
        assert_eq!(
            ls.heights.height_of(2),
            30.0,
            "parked slot should leave existing known height unchanged"
        );
        // 其余 active slot 真高度不变。
        assert_eq!(ls.heights.height_of(0), 10.0);
        assert_eq!(ls.heights.height_of(1), 20.0);
        assert_eq!(ls.heights.height_of(3), 40.0);
        assert_eq!(ls.heights.height_of(4), 50.0);
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

    /// NotifyRemoved（池化模型）：删 [at, at+count) → heights.known drain 该区间；
    /// item_count -= count；区间内 slot 就地 park（parked=true, display:none 便签），
    /// 永不 detach（parent 仍是 ul）；>end 的 slot.item_index -= count（移位）。
    /// slots.len() 不变（高水位只增不减）——不再有 free 池，parked slot 随时可翻醒复用。
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
        // 删 [2, 4)（删 2 项）：item 2,3 的 slot 就地 park；item 4 的 slot.item_index 4→2。
        crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 2, 2).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.item_count, 3);
        assert_eq!(ls.heights.known.len(), 3);
        // 高水位不变：slot 永驻 slots vec（parked 只标休眠，不 detach）。
        assert_eq!(
            ls.slots.len(),
            slot_count_before,
            "high-water: slots never shrink; parked slots stay in vec"
        );
        // 2 个 slot 被 park（原 items 2,3）。
        assert_eq!(
            ls.slots.iter().filter(|s| s.parked).count(),
            2,
            "two slots parked (items 2,3 removed)"
        );
        // active slot 覆盖 items 0,1,2。
        let mut active_indices: Vec<usize> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| s.item_index)
            .collect();
        active_indices.sort_unstable();
        assert_eq!(
            active_indices,
            vec![0, 1, 2],
            "active slots cover remaining items after shift"
        );
        // 所有 slot 的 parent 仍是 ul（无 detach）。
        for s in &ls.slots {
            assert_eq!(
                scene.get(s.node).unwrap().parent,
                Some(ul),
                "no detach on remove: every slot still parented to ul"
            );
        }
        // parked slot 已标 display:none 便签（inline_set 有 INLINE_DISPLAY bit）。
        for s in ls.slots.iter().filter(|s| s.parked) {
            let n = scene.get(s.node).unwrap();
            assert!(
                n.inline_set.0 & crate::style::dynamic::INLINE_DISPLAY != 0,
                "parked slot {:?} has display:none inline override set",
                s.node
            );
        }
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

    /// notify_removed：pooled-slot-lifecycle 模型下，删 item 不 detach slot——
    /// 受影响 slot 就地 park（parent 仍是 ul），item_index > end 的移位，parked slot
    /// 不入 pending_binds。slot 总数不变（高水位只增不减）。
    #[test]
    fn notify_removed_parks_not_detaches() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        // 冷启动 INITIAL_SLOTS=5（视口高度 0 → 退化为定数）。
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 清空 execute 产的 initial binds（只看 notify_removed 新增的）。
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // 5 个 slot 全 active，绑 items 0..5。
        assert_eq!(
            s.scene.as_ref().unwrap().lists.get(ul).unwrap().slots.len(),
            5,
            "precondition: 5 slots instantiated"
        );
        // 删 [3, 5)（删 items 3,4）。此时无 >end 的 slot 需移位（end=5 全覆盖）。
        crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 3, 2).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.item_count, 3, "item_count reduced by 2");
        // 所有 slot 的 parent 仍是 ul（无 detach）。
        for s in &ls.slots {
            assert_eq!(
                scene.get(s.node).unwrap().parent,
                Some(ul),
                "no detach on remove: slot still parented to ul"
            );
        }
        // 有两 slot 被 park（原 items 3,4）。
        assert_eq!(
            ls.slots.iter().filter(|s| s.parked).count(),
            2,
            "two slots parked (items 3,4 removed)"
        );
        // active slot 覆盖 items 0,1,2。
        let active_indices: Vec<usize> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| s.item_index)
            .collect();
        let mut sorted = active_indices.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2], "active slots cover remaining items");
        // 高水位不变：slots.len() == 5。
        let slot_count = ls.slots.len();
        assert_eq!(slot_count, 5, "high-water pool: slots never shrink");
        // 注：此场景无移位（count=5, end=5 全覆盖），故 notify_removed 不生 bind。
        // parked slot 的 stale idx=3/4 未入 pending_binds。
    }

    /// notify_inserted：池化模型下，插入 item 只做 index 移位，不 detach slot。
    #[test]
    fn notify_inserted_shifts_indices_no_detach() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 在 at=2 插入 2 项。
        crate::list::notify_inserted(s.scene.as_mut().unwrap(), ul, 2, 2).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.item_count, 7, "item_count grown by 2");
        // 所有 slot 的 parent 仍是 ul。
        for s in &ls.slots {
            assert_eq!(
                scene.get(s.node).unwrap().parent,
                Some(ul),
                "no detach on insert"
            );
        }
        // 原 item_index >= 2 的 slot 已移位 +2。排除 parked（无），验 active index 集。
        let indices: Vec<usize> = ls.slots.iter().map(|s| s.item_index).collect();
        let mut sorted = indices.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![0, 1, 4, 5, 6],
            "indices shifted: 0,1 stay; 2→4, 3→5, 4→6"
        );
    }

    /// notify_moved：parked slot 不入 pending_binds（与 notify_inserted/notify_removed 一致）。
    /// 序列：先删一些 item 产生 parked slot，再插，再 move——验证 move 的 bind 队列不含 parked。
    #[test]
    fn notify_moved_filters_parked_from_binds() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 清空冷启动 binds。
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // Step 1: 删 items [3,5) → slot 3,4 parked（stale idx 3,4）。
        crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 3, 2).unwrap();
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // Step 2: 在 at=3 插入 1 项 → 原 slot 3,4 的 stale idx 4,5 移位后成 5,6（仍 parked）。
        crate::list::notify_inserted(s.scene.as_mut().unwrap(), ul, 3, 1).unwrap();
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // Step 3: move item 0 → 2。parked slot 的 stale idx 碰巧落在 [0,2] 区间，
        // 但 notify_moved 应过滤 parked slot，不让它进 bind 队列。
        crate::list::notify_moved(s.scene.as_mut().unwrap(), ul, 0, 2).unwrap();
        let binds = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        let ls = s.scene.as_ref().unwrap().lists.get(ul).unwrap();
        // 找到所有 parked slot 的 node。
        let parked_nodes: std::collections::HashSet<NodeId> = ls
            .slots
            .iter()
            .filter(|s| s.parked)
            .map(|s| s.node)
            .collect();
        assert!(
            !parked_nodes.is_empty(),
            "there must be parked slots in the pool"
        );
        for (node, _idx) in &binds {
            assert!(
                !parked_nodes.contains(node),
                "parked slot {:?} leaked into bind queue",
                node
            );
        }
        // 同时验证 active slot 在 to_rebind 中。
        let active_nodes: std::collections::HashSet<NodeId> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| s.node)
            .collect();
        let in_bind: std::collections::HashSet<NodeId> = binds.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            in_bind, active_nodes,
            "all active (non-parked) slots must appear in bind queue"
        );
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

    /// refresh_items 只刷 **active** slot。parked slot 的 `item_index` 是 stale 复用参考——
    /// 它可能仍落在刷新区间内，但那个 slot 是 display:none 的隐形节点，入队会让驱动对看不见的
    /// 节点跑 BindItem（无谓回调 + 业务数据写进隐形节点）。同 notify_inserted/notify_removed 的
    /// bind 过滤规则。
    #[test]
    fn refresh_items_skips_parked_slots() {
        let (mut s, ul, _li) = stage_with_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 10);
        // 冷启动（无滚动祖先 → viewport.h=0）visible=[0,5) → 5 个 slot 绑 items 0..5。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 删 [3,5)：绑 items 3、4 的 slot 就地 park（item_index 保留 3/4 作复用参考）。
        crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 3, 2).unwrap();
        {
            let ls = s.scene.as_ref().unwrap().lists.get(ul).unwrap();
            assert_eq!(ls.slots.len(), 5, "high-water pool keeps all 5 slots");
            assert_eq!(
                ls.slots.iter().filter(|s| s.parked).count(),
                2,
                "precondition: removed-range slots parked (still item_index 3/4)"
            );
        }
        // 清掉此前累积的 binds，只看 refresh 入队的。
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // 刷全表 [0,8)：parked slot 的 stale item_index 3/4 也落在区间内，但不该入队。
        crate::list::refresh_items(s.scene.as_mut().unwrap(), ul, 0, 8).unwrap();
        let binds = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        let mut idxs: Vec<usize> = binds.iter().map(|(_, i)| *i).collect();
        idxs.sort_unstable();
        assert_eq!(
            idxs,
            vec![0, 1, 2],
            "only active slots re-queued (parked slots' stale item_index must not bind)"
        );
    }

    /// plan 阶段的池化契约（spec §2.3）：**只标记不搬树**。
    ///
    /// 离开可见区的 slot 就地标 `parked` + 写 display:none 便签，NodeId/parent/reuse_key 全保留
    /// （无 detach、无 remove_child、无 free 池）；留在可见区的 slot 保持 active；可见区内还没
    /// active slot 绑的 item 收进 `to_bind` 供 execute 复用/扩容。plan 自身不 bind、不建树。
    #[test]
    fn plan_visible_marks_park_no_detach() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        // 均匀 20px/项 + 视口 100 → 可见区可精确预期（避免 estimate=0 退化为冷启动定数）。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 100.0);
            st.scroll_pos = (0.0, 0.0);
        }
        // 第一帧（plan+execute）：可见 0..7 → 7 个 active slot 绑 items 0..6。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let (slots_before, children_before) = {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            assert_eq!(ls.visible, 0..7, "frame 1 visible");
            assert!(ls.slots.iter().all(|s| !s.parked), "frame 1: all active");
            (ls.slots.len(), scene.get(ul).unwrap().children.len())
        };
        // 清 bind 队列，验 plan 自身不入队。
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // 第二帧：滚 60px → 可见 1..10。item 0 离开（→park），items 1..6 留在区内（active），
        // items 7,8,9 尚无 active slot（→to_bind）。**只 plan，不 execute**。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 60.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        assert_eq!(ops.len(), 1, "one ListView planned");
        let op = &ops[0];
        assert_eq!(op.new_visible, 1..10, "frame 2 visible");
        let mut to_bind = op.to_bind.clone();
        to_bind.sort_unstable();
        assert_eq!(
            to_bind,
            vec![7, 8, 9],
            "visible items lacking an active slot collected for execute"
        );

        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        // 池不缩、树不动：slot 数与 ul.children 数一字不变，每个 slot 仍挂在 ul 下。
        assert_eq!(
            ls.slots.len(),
            slots_before,
            "high-water pool never shrinks"
        );
        assert_eq!(
            scene.get(ul).unwrap().children.len(),
            children_before,
            "no slot removed from ul.children"
        );
        for slot in &ls.slots {
            let n = scene.get(slot.node).expect("slot node still live");
            assert_eq!(n.parent, Some(ul), "no slot detached");
            assert!(
                scene.get(ul).unwrap().children.contains(&slot.node),
                "slot still a child of ul"
            );
        }
        // 分区正确：离开可见区的 park、留在区内的仍 active。
        let parked: Vec<usize> = ls
            .slots
            .iter()
            .filter(|s| s.parked)
            .map(|s| s.item_index)
            .collect();
        let mut active: Vec<usize> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| s.item_index)
            .collect();
        active.sort_unstable();
        assert_eq!(
            parked,
            vec![0],
            "off-range slot parked (item 0 scrolled out)"
        );
        assert_eq!(active, vec![1, 2, 3, 4, 5, 6], "in-range slots stay active");
        // parked slot 已写 display:none 便签（下帧 rematch 拷进 style → taffy 跳 + render 剪枝）。
        let parked_node = ls.slots.iter().find(|s| s.parked).unwrap().node;
        let pn = scene.get(parked_node).unwrap();
        assert_ne!(
            pn.inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
            0,
            "parked slot carries the display inline override bit"
        );
        assert_eq!(
            pn.inline_override.taffy_style.display,
            taffy::Display::None,
            "parked slot's override value is display:none"
        );
        // plan 不 bind（bind 是 execute 的活）。
        assert!(
            ls.pending_binds.is_empty(),
            "plan must not queue binds (execute does)"
        );

        // 第三帧：滚回顶部 → 可见回 0..7。此时池里那个 parked slot 的 item_index 仍是 0
        // （stale 复用参考）——若把它当「已绑」，item 0 会漏出 to_bind，execute 就永远不会
        // unpark 它，item 0 在界面上永久隐形。故「已绑」只算 active slot。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 0.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        let op = &ops[0];
        assert_eq!(op.new_visible, 0..7, "frame 3 scrolled back to top");
        assert!(
            op.to_bind.contains(&0),
            "item 0 must be re-bound: its slot is parked, and a parked slot's stale \
             item_index never counts as bound (to_bind={:?})",
            op.to_bind
        );
    }

    /// execute 阶段的池化契约（spec §2.4）：**unpark + bind**。
    ///
    /// 滚动后 plan 标 park / 收 to_bind，execute 把池里的 parked slot 翻回 active 绑给新 item：
    /// 每个可见 item 恰有一个 active slot 绑它、离开可见区的 slot 留 display:none 便签、
    /// 本帧新绑的全进 pending_binds。零 detach、零重建。
    #[test]
    fn execute_unparks_and_binds_visible_items() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        // 均匀 20px/项 + 视口 100 → 可见区可精确预期。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            // 首帧大视口（400）：把池撑到 22 个 slot，给下一帧留出富余 parked 库存。
            st.viewport_size = (1000.0, 400.0);
            st.scroll_pos = (0.0, 0.0);
        }
        // 第一帧：可见 0..22 → 池长到 22。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let _ = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        // 第二帧：视口缩到 100 + 滚到 500px → 可见 23..32（九项，与首帧 0..22 无交集）。
        // 高水位池不缩：旧 slot 全部 park，其中九个被 unpark 换绑新 item——零克隆零重建。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.viewport_size = (1000.0, 100.0);
            st.scroll_pos = (0.0, 500.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);

        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        let visible = ls.visible.clone();
        assert_eq!(visible, 23..32, "frame 2 visible");
        // active slot 全绑可见 item，且可见区每项恰有一个 active slot。
        let mut active: Vec<usize> = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .map(|s| s.item_index)
            .collect();
        active.sort_unstable();
        assert_eq!(
            active,
            visible.clone().collect::<Vec<_>>(),
            "active slots bind exactly the visible items (one each)"
        );
        // 离开可见区的 slot 是 parked，且带 display:none 便签（不占布局、不渲染）。
        let parked_count = ls.slots.iter().filter(|s| s.parked).count();
        assert!(
            parked_count > 0,
            "scrolled-out slots stay in pool as parked"
        );
        for slot in ls.slots.iter().filter(|s| s.parked) {
            let n = scene.get(slot.node).expect("parked slot still live");
            assert_ne!(
                n.inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
                0,
                "parked slot carries the display inline override bit"
            );
            assert_eq!(
                n.inline_override.taffy_style.display,
                taffy::Display::None,
                "parked slot's override value is display:none"
            );
            assert_eq!(n.parent, Some(ul), "parked slot never detached");
        }
        // 本帧新绑的 item 全部入队（等 C# DrainPendingBinds → BindItem）。
        let mut bound: Vec<usize> = ls.pending_binds.iter().map(|(_, i)| *i).collect();
        bound.sort_unstable();
        assert_eq!(
            bound,
            visible.clone().collect::<Vec<_>>(),
            "every newly-unparked slot queued a bind for its item"
        );
        // 池只增不减：九项可见区全由首帧的 22 个 slot 复用，未新增克隆。
        assert_eq!(
            ls.slots.len(),
            22,
            "pool reused in place (no clone, no shrink)"
        );
        assert_all_slots_well_parented(scene, ul);
    }

    /// execute 扩容契约（spec §2.2/§2.4）：池里无 parked slot 可复用时克隆模板扩容。
    ///
    /// 高水位只增不减——扩容后即便滚回去也不缩（无驱逐，约束 e）。新 slot 挂 ul
    /// （head/tail spacer 之间），parent 与 NodeId 从此永驻。
    #[test]
    fn execute_grows_by_cloning_when_no_parked_slot() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 1000);
        // 20px/项 + 视口 400 → 可见约 20 项 + BUFFER，远超预分配的 INITIAL_SLOTS。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..1000 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 400.0);
            st.scroll_pos = (0.0, 0.0);
        }
        let before = {
            let ls = s.scene.as_ref().unwrap().lists.get(ul).unwrap();
            assert_eq!(
                ls.slots.len(),
                crate::list::INITIAL_SLOTS,
                "precondition: only the pre-allocated batch exists"
            );
            ls.slots.len()
        };
        // 可见项数 > 池容量 → 池耗尽后克隆扩容。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let after = {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            let visible_len = ls.visible.len();
            assert!(
                visible_len > before,
                "precondition: visible ({visible_len}) exceeds pool ({before})"
            );
            assert_eq!(ls.slots.len(), visible_len, "grew to cover visible range");
            // 新 slot 也挂在 ul 下（永驻子树），reuse_key 非 0（出生即定）。
            for slot in &ls.slots {
                let n = scene.get(slot.node).expect("slot live");
                assert_eq!(n.parent, Some(ul), "cloned slot parented to ul");
                assert_ne!(n.reuse_key, 0, "cloned slot got a reuse_key at birth");
            }
            assert_all_slots_well_parented(scene, ul);
            ls.slots.len()
        };
        assert!(after > before, "grew by cloning");
        // 滚回顶部（可见区回到少量项）→ 池只增不减，绝不驱逐。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 0.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_ref().unwrap();
        assert_eq!(
            scene.lists.get(ul).unwrap().slots.len(),
            after,
            "high-water pool never shrinks (no eviction)"
        );
    }

    /// unpark 必须 **清** display 便签（`unset_inline_override`），不能写 `display:block`
    /// （spec §2.6）——后者会盖掉作者样式（`li { display:flex }` 的 item 会塌成块流）。
    ///
    /// 观测点：unpark 后 slot 的 `inline_set` display bit 必须被清零，cascade 回落到
    /// base_style 的真实 display。写 `display:block` 的实现会留着 bit（值 Block），此测红。
    #[test]
    fn execute_unpark_clears_display_bit_not_sets_block() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 100.0);
            st.scroll_pos = (0.0, 0.0);
        }
        // 预分配的 slot 全 parked（display:none 便签已置）——unpark 前的基线。
        {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            assert!(
                ls.slots.iter().all(|s| s.parked),
                "precondition: all parked"
            );
            for slot in &ls.slots {
                assert_ne!(
                    scene.get(slot.node).unwrap().inline_set.0
                        & crate::style::dynamic::INLINE_DISPLAY,
                    0,
                    "precondition: pre-allocated slot carries display:none note"
                );
            }
        }
        // 第一帧：unpark 预分配的 slot 绑 items 0..N。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let frame1_active: std::collections::HashMap<NodeId, usize> = {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            for slot in ls.slots.iter().filter(|s| !s.parked) {
                let n = scene.get(slot.node).unwrap();
                assert_eq!(
                    n.inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
                    0,
                    "unpark must CLEAR the display bit (unset_inline_override), \
                     not set a display:block override"
                );
                assert_ne!(
                    n.inline_override.taffy_style.display,
                    taffy::Display::Block,
                    "unpark must not stamp display:block over the author's style"
                );
            }
            ls.slots
                .iter()
                .filter(|s| !s.parked)
                .map(|s| (s.node, s.item_index))
                .collect()
        };
        // 再滚一帧走 park→unpark 往返：同一 slot 被复用给新 item 后 bit 仍是清的。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 200.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        // 真·往返：至少一个首帧的 slot 节点被 park 后又 unpark 换绑到了别的 item
        // （同 NodeId、新 item_index）——否则本段只是重复验首帧的清 bit 路径。
        let recycled = ls
            .slots
            .iter()
            .filter(|s| !s.parked)
            .filter(|s| {
                frame1_active
                    .get(&s.node)
                    .is_some_and(|&old| old != s.item_index)
            })
            .count();
        assert!(
            recycled > 0,
            "some slots round-tripped park→unpark and re-bound to a new item"
        );
        for slot in ls.slots.iter().filter(|s| !s.parked) {
            assert_eq!(
                scene.get(slot.node).unwrap().inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
                0,
                "re-unparked slot's display bit cleared again (park→unpark round-trip)"
            );
        }
    }

    /// reuse_key 出生即定、永不旋转（坑182 子因②根治）。
    /// slot[0] 的 key 在 enter_data_driven 预分配时设定，经历 park→unpark 往返后不变。
    #[test]
    fn reuse_key_stable_across_scroll_frames() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 1000);
        // 20px/项 + 视口 200 → 可见 ~10 项 + BUFFER。
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..1000 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 200.0);
            st.scroll_pos = (0.0, 0.0);
        }
        // 第一帧：实例化初始 slot，拿 slot[0] 的 reuse_key 当基线。
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let key_of_slot0 = {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            assert!(!ls.slots.is_empty(), "slot[0] exists");
            scene.get(ls.slots[0].node).unwrap().reuse_key
        };
        assert_ne!(key_of_slot0, 0, "slot[0] has a non-zero reuse_key at birth");

        // 滚到 item 500：slot[0] 离开可见区→park，之后可能 unpark 换绑给新 item。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 500.0 * 20.0); // scroll to ~item 500
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        crate::list::collect_heights(s.scene.as_mut().unwrap());

        // 再滚回顶部：slot[0] 可能被 unpark 并换绑回低序号 item。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 0.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);

        // slot[0] 是同一个 NodeId（slots vec 永缩、无移除），其 reuse_key 跨帧不变。
        let key_after_scroll = {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            scene.get(ls.slots[0].node).unwrap().reuse_key
        };
        assert_eq!(
            key_after_scroll, key_of_slot0,
            "reuse_key permanent — never rotated across park/unpark/rebind"
        );
    }

    // ── 保险测试（spec §6.1）──────────────────────────────────────────────

    /// taffy Display::None 保险：parked slot 挂 display:none 便签 → rematch 后
    /// style.taffy_style.display == None → solve 跳该节点、布局零尺寸。
    /// 同时验 active slot 正常参与布局（display 不是 None）。
    #[test]
    fn taffy_display_none_excludes_parked_slot_from_flow() {
        let (mut s, ul, _li, _pane) = stage_with_pane_ul_li();
        // 给蓝图设显式高度，使 slot 在 taffy 里有非零尺寸（否则空 div 高度 0，
        // 无法区分"taffy 跳了"还是"本来就没高度"）。
        {
            let scene = s.scene.as_mut().unwrap();
            use taffy::style::Dimension;
            let li = scene.get(ul).unwrap().children[0];
            scene.get_mut(li).unwrap().style.taffy_style.size.height = Dimension::length(40.0);
        }
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        // 执行后 5 个 slot 全 active（视口 0 → cold start INITIAL_SLOTS=5）。
        // 删 items [2, 4) → slot 2,3 就地 park。
        crate::list::notify_removed(s.scene.as_mut().unwrap(), ul, 2, 2).unwrap();
        // rematch → 把 display:none 便签拷进 node.style。
        crate::style::dynamic::rematch_pseudo_classes(s.scene.as_mut().unwrap());
        // solve → taffy 跳 parked slot（display:none），active slot 拿 40px 高。
        crate::layout::solve(
            s.scene.as_mut().unwrap(),
            &s.fonts,
            s.root_size,
            &s.image_sizes,
        );
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        // parked slot：style 已设 display:none + layout_rect 归零（taffy 跳过）。
        for slot in ls.slots.iter().filter(|s| s.parked) {
            let n = scene.get(slot.node).unwrap();
            assert_eq!(
                n.style.taffy_style.display,
                taffy::Display::None,
                "parked slot style.display == None after rematch"
            );
            assert_eq!(
                n.layout_rect.h, 0.0,
                "parked slot layout_rect.h == 0 (taffy skipped)"
            );
        }
        // active slot：display 不是 None，有正常布局高度。
        let mut active_bottoms: Vec<f32> = Vec::new();
        for slot in ls.slots.iter().filter(|s| !s.parked) {
            let n = scene.get(slot.node).unwrap();
            assert_ne!(
                n.style.taffy_style.display,
                taffy::Display::None,
                "active slot style.display != None"
            );
            assert!(
                n.layout_rect.h > 0.0,
                "active slot has non-zero layout height"
            );
            active_bottoms.push(n.layout_rect.y + n.layout_rect.h);
        }
        // active slot 之间无间隙：每个 slot 的 bottom 等于下一个 slot 的 top。
        // 仅当 >=2 个 active slot 时才有相邻可验。
        if active_bottoms.len() >= 2 {
            for w in active_bottoms.windows(2) {
                let gap = (w[0] - w[1]).abs();
                assert!(
                    gap < 0.5,
                    "active slots contiguous: bottom={:.1} vs next top (gap={:.1})",
                    w[0],
                    gap
                );
            }
        }
    }

    /// insert_before 排序保险：多次 park/unpark 往返后，head_spacer 始终 children[0]，
    /// tail_spacer 始终 children.last()。parked slot 的物理位置不破坏这一不变量。
    #[test]
    fn insert_before_keeps_spacer_ordering_with_parked_slots() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 200.0);
            st.scroll_pos = (0.0, 0.0);
        }
        // 多帧往返：滚→停→滚→停，触发 park/unpark 多次。
        for scroll_y in [0.0, 400.0, 0.0, 800.0, 0.0, 200.0] {
            {
                let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
                st.scroll_pos = (0.0, scroll_y);
            }
            let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
            crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
            crate::list::collect_heights(s.scene.as_mut().unwrap());
        }
        // 每帧后验 spacer 不变量。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 0.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(
            ul_node.children.first(),
            Some(&ls.head_spacer),
            "head_spacer is always children[0] after park/unpark cycles"
        );
        assert_eq!(
            ul_node.children.last(),
            Some(&ls.tail_spacer),
            "tail_spacer is always children.last() after park/unpark cycles"
        );
        // 再验 assert_all_slots_well_parented（含无重复子 + active 顺序严格递增）。
        assert_all_slots_well_parented(scene, ul);
    }

    /// tick 时序不变量：tick_and_render 内 solve 在 rematch 之后、每次 tick 都执行。
    ///
    /// "solve 一次/帧" 是声明式不变量（spec §1.1 / §1.3），无 instrumentation 无法直接
    /// 计数。这里用间接证据链：
    ///   1. tick_and_render 后 active slot 有非零 layout_rect（solve 跑了且产出布局）。
    ///   2. 滚动触发 park/unpark → 再 tick → layout_rect 反映新可见区（solve 对变更响应）。
    ///   3. parked slot 的 display:none 已由 rematch 生效进 style（时序：rematch 在 solve 前）。
    ///
    /// 若将来需要直接计数 solve，加 instrumentation（如 scene.solve_count: u32），
    /// 本测即可精确断言 "solve_count 增量 == 1"。当前间接证据链已覆盖核心风险。
    #[test]
    fn tick_order_one_solve_per_frame_with_parking() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 200.0);
            st.scroll_pos = (0.0, 0.0);
        }
        // 证据 1：tick_and_render 后 active slot 有非零 layout_rect（solve 产出布局）。
        s.tick_and_render();
        {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            let active_with_layout = ls
                .slots
                .iter()
                .filter(|s| !s.parked)
                .filter(|s| {
                    let n = scene.get(s.node).unwrap();
                    n.layout_rect.h > 0.0
                })
                .count();
            assert!(
                active_with_layout > 0,
                "solve ran: active slots have non-zero layout_rect"
            );
            // 证据 3：parked slot 的 style 已生效 display:none（rematch 在 solve 前）。
            for slot in ls.slots.iter().filter(|s| s.parked) {
                assert_eq!(
                    scene.get(slot.node).unwrap().style.taffy_style.display,
                    taffy::Display::None,
                    "rematch applied display:none to parked slot before solve"
                );
            }
        }
        // 证据 2：滚动触发 park/unpark → 再 tick → layout 反映新状态。
        let pre_scroll_parked: usize = {
            s.scene
                .as_ref()
                .unwrap()
                .lists
                .get(ul)
                .unwrap()
                .slots
                .iter()
                .filter(|s| s.parked)
                .count()
        };
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 400.0);
        }
        s.tick_and_render();
        {
            let scene = s.scene.as_ref().unwrap();
            let ls = scene.lists.get(ul).unwrap();
            let post_scroll_parked = ls.slots.iter().filter(|s| s.parked).count();
            // 滚动后 parked 集变化（部分 slot park、部分 unpark）。
            // 若 parked 集相同（视口全覆盖），至少 active 的 item_index 变了。
            let active_set_changed = ls
                .slots
                .iter()
                .filter(|s| !s.parked)
                .any(|s| s.item_index >= 10); // 滚到 ~item 20，应有些 item_index >= 10
            assert!(
                post_scroll_parked != pre_scroll_parked || active_set_changed,
                "scroll tick caused state change: parked {}→{} (pre→post)",
                pre_scroll_parked,
                post_scroll_parked
            );
            // 新 active slot 仍有 layout（solve 对变更响应）。
            let active_with_layout = ls
                .slots
                .iter()
                .filter(|s| !s.parked)
                .filter(|s| {
                    let n = scene.get(s.node).unwrap();
                    n.layout_rect.h > 0.0
                })
                .count();
            assert!(
                active_with_layout > 0,
                "post-scroll solve ran: active slots still have layout"
            );
        }
    }

    /// 所有 slot 的 reuse_key 必须 >0（0 = MirrorPool"无 key"）且互不重复。
    #[test]
    fn reuse_key_pairwise_distinct_and_positive() {
        let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 100);
        {
            let scene = s.scene.as_mut().unwrap();
            let ls = scene.lists.get_mut(ul).unwrap();
            for i in 0..100 {
                ls.heights.set(i, 20.0);
            }
            let st = scene.scroll.ensure(pane);
            st.viewport_size = (1000.0, 200.0);
            st.scroll_pos = (0.0, 0.0);
        }
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);

        // 滚一次让一些 slot park/unpark，触发 rebind。
        {
            let st = s.scene.as_mut().unwrap().scroll.ensure(pane);
            st.scroll_pos = (0.0, 500.0);
        }
        let ops2 = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(s.scene.as_mut().unwrap(), ops2);

        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert!(!ls.slots.is_empty(), "at least one slot");
        let mut keys = std::collections::HashSet::new();
        for slot in &ls.slots {
            let key = scene.get(slot.node).unwrap().reuse_key;
            assert_ne!(key, 0, "each slot has a positive reuse_key");
            assert!(
                keys.insert(key),
                "each slot has a distinct reuse_key; duplicate: {key}"
            );
        }
    }
}
