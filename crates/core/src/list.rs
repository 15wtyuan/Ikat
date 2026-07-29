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
        let spacer_head_h = (ls.heights.sum(0..visible.start) - gap).max(0.0);
        let spacer_tail_h = (ls.heights.sum(visible.end..ls.item_count) - gap).max(0.0);
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
}
