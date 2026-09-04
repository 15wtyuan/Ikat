use super::state::{HeightCache, ListState, BUFFER, INITIAL_SLOTS};
use super::viewport::{ancestor_scroll_viewport, warn_no_pane_once};
use crate::scene::node::{NodeId, Scene};

/// 计算可见项区间 [start, end)（含 BUFFER）。viewport.h==0 → 冷启动返 INITIAL_SLOTS。
/// top = scroll_pos.y - listview_offset（ul 相对 pane 的偏移）。
///
/// `fallback_of(i)`：item i 未测时的估高（多模板下按蓝图均值——见 `ListState::bp_estimate_of`）。
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
    fallback_of: &dyn Fn(usize) -> f32,
    gap: f32,
) -> std::ops::Range<usize> {
    if item_count == 0 {
        return 0..0;
    }
    if viewport_h <= 0.0 {
        return 0..INITIAL_SLOTS.min(item_count);
    }
    // 全无已测高度：无法估算可见项数（每项高度 0 → 累积和永不达阈值 → 误判整列可见）。
    // 退化为冷启动定数，等首帧 solve + collect_heights 回填真实高度后下帧才走精准路径。
    if !heights.any_known() {
        return 0..INITIAL_SLOTS.min(item_count);
    }
    let top = scroll_pos_y - listview_offset;
    // first = 首个底边超过 top 的项（累积后判，含 gap）。若全项底边 ≤ top（内容短于视口），
    // 循环不 break，first 保持 0 → start 经 BUFFER 回退到 0，整列可见。
    // bottom(i) = sum(h[0..i+1]) + i*gap（i 个 gap 在 item i 之前）。
    let mut acc = 0.0;
    let mut first = 0usize;
    for i in 0..item_count {
        acc += heights.height_of(i, fallback_of(i));
        if acc + (i as f32) * gap > top {
            first = i;
            break;
        }
    }
    let target = top + viewport_h;
    let mut acc2 = 0.0;
    let mut last = item_count;
    for j in 0..item_count {
        acc2 += heights.height_of(j, fallback_of(j));
        if acc2 + (j as f32) * gap >= target {
            last = j + 1;
            break;
        }
    }
    let start = first.saturating_sub(BUFFER);
    let end = (last + BUFFER).min(item_count);
    start..end
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

/// 网格按行可见区 + spacer 高度（按行虚拟化、行内全量）。
/// 行 r 占 [r*row_pitch, r*row_pitch+row_h]；BUFFER 行；spacer 高含 gap_y 补偿
/// （首 slot 行位置 = spacer_h + gap_y，对齐非虚拟基准 r*row_pitch）。
pub(super) fn grid_visible_spacers(
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
    let visible = compute_visible_range(
        ls.item_count,
        scroll_y,
        ul_y,
        viewport_h,
        &ls.heights,
        &|i| ls.bp_estimate_of(i),
        gap,
    );
    // Gap accounting for flex+gap uls: [head_spacer, slot.., tail_spacer]，可见 slot 在 head spacer
    // 后一个 gap。为对齐非虚拟基准（item[k].top = sum(0..k) + k*gap），head spacer 须保留
    // sum(0..start) + (start-1)*gap：slot.top = spacer.h + gap = sum + count*gap。tail 对称。
    // count=0 → saturating_sub(1)=0（空 spacer 无 gap）。block ul 的 gap=0，本项 no-op。
    let head_count = visible.start;
    let tail_count = ls.item_count.saturating_sub(visible.end);
    let spacer_head_h =
        (ls.sum_heights(0..visible.start) + (head_count.saturating_sub(1) as f32) * gap).max(0.0);
    let spacer_tail_h = (ls.sum_heights(visible.end..ls.item_count)
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
    ensure_grid_detected(scene, ul);
    if let Some(ls) = scene.lists.get_mut(ul) {
        ls.plans_seen = ls.plans_seen.saturating_add(1);
    }
    // Phase A：单次不可变借完成所有只读计算——可见区（Copy 的 Range）+ spacer 高度 + gap。
    // spacer 高需 heights.sum，故一并在此算出，避免后续跨可变借再 clone heights。
    let (visible, spacer_head_h, spacer_tail_h, measured) =
        match ancestor_scroll_viewport(scene, ul) {
            None => {
                // 无滚动容器：退化全量渲染（原 m1-listview spec 语义——宁可全渲染，不可静默
                // 截断）。旧行为返 (0,0) 假视口 → viewport≤0 恒走冷启动 → 超初始 slot 数的
                // 列表静默只剩前 INITIAL_SLOTS 项。
                warn_no_pane_once(scene, ul);
                let count = scene.lists.get(ul)?.item_count;
                (0..count, 0.0, 0.0, None)
            }
            Some((scroll_y, viewport_h)) => {
                // 自滚模式（ul 自身是 ScrollPane）：top = scroll_pos.y − 0——内容原点即 ul 内容盒，
                // 不扣 ul 在页面里的偏移。祖先滚动模式：扣 ul 相对 pane 的 layout 偏移。
                let ul_y = if scene.scroll.get(ul).is_some() {
                    0.0
                } else {
                    scene.get(ul).map(|n| n.layout_rect.y).unwrap_or(0.0)
                };
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
            }
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
