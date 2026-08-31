use super::plan::grid_visible_spacers;
use super::*;
use crate::scene::node::{NodeFlags, NodeId};

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
/// parked slot 是 display:none，不占布局，物理位置任意。
/// 同时检 ul.children 无重复 NodeId。
fn assert_all_slots_well_parented(scene: &crate::scene::node::Scene, ul: NodeId) {
    let ls = scene.lists.get(ul).expect("list state");
    let head = ls.head_spacer;
    let tail = ls.tail_spacer;
    let ul_node = scene.get(ul).unwrap();
    assert_eq!(ul_node.children.first(), Some(&head), "head spacer first");
    assert_eq!(ul_node.children.last(), Some(&tail), "tail spacer last");
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
    let all_slots: std::collections::HashSet<NodeId> = ls.slots.iter().map(|s| s.node).collect();
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
/// 不再有 free 池，slot 从生到死不 detach。
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
/// packer 把 `<template>` 保留为 NodeKind::Template 子，其下 ListItem 才是蓝图。
/// enter_data_driven 须采用 template 内的 ListItem 作模板源。
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
    // ul 下恰好一个 <template> 才自动采用；多个是契约违反。
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
    let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    crate::list::set_item_count(&mut s, ul, 1000);
    // pane 视口未测（首帧 solve 前 viewport.h=0）→ 冷启动只实例化 INITIAL_SLOTS。
    {
        let scene = s.scene.as_mut().unwrap();
        let st = scene.scroll.ensure(pane);
        st.viewport_size = (1000.0, 0.0);
        st.scroll_pos = (0.0, 0.0);
    }
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
    // 有 pane 在场，无「无滚动容器」警告。
    assert!(
        scene.warnings.is_empty(),
        "pane present: no no-pane warning"
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

/// 无滚动容器（自身与祖先链都无 ScrollPane）→ 退化全量渲染 + 一次性警告。
/// 旧行为：假视口 (0,0) 恒走冷启动 → count > INITIAL_SLOTS 的列表静默只剩前几项。
#[test]
fn no_pane_degenerates_to_full_render_with_warning() {
    let (mut s, ul, _li) = stage_with_ul_li();
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    crate::list::set_item_count(&mut s, ul, 100);
    let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
    crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
    {
        let scene = s.scene.as_ref().unwrap();
        let ls = scene.lists.get(ul).unwrap();
        assert_eq!(ls.visible, 0..100, "full render: all items visible");
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(
            ul_node.children.len(),
            102,
            "2 spacers + 100 slots (no silent truncation to INITIAL_SLOTS)"
        );
    }
    // 警告一次性：第一帧已推，第二帧 plan 不再重复。
    assert_eq!(s.scene.as_ref().unwrap().warnings.len(), 1);
    let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
    crate::list::execute_visible(s.scene.as_mut().unwrap(), ops);
    assert_eq!(
        s.scene.as_ref().unwrap().warnings.len(),
        1,
        "warning is once-per-list, not per-frame"
    );
}

/// ul 被直接父容器 flex 纵向拉伸 → enter_data_driven 推一次性警告（拉伸钉死高度 =
/// 视口高 → content_size==viewport 不能滚）。warning 级不 Err——短列表拉伸无害。
#[test]
fn flex_stretched_ul_warns_at_enter() {
    // 1. pane 默认样式（display:flex row、align_items None = CSS 初始 stretch）：
    //    ul 直接子被交叉轴纵向拉伸 → 警告。
    let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
    {
        let scene = s.scene.as_mut().unwrap();
        scene.scroll.ensure(pane);
    }
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    let warns = &s.scene.as_ref().unwrap().warnings;
    assert_eq!(warns.len(), 1, "stretched ul (cross-axis) warns");
    assert!(warns[0].contains("stretched"), "got: {:?}", warns[0]);

    // 2. 父 align-items:flex-start → 不拉伸 → 无警告。
    let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
    {
        let scene = s.scene.as_mut().unwrap();
        scene.get_mut(pane).unwrap().style.taffy_style.align_items =
            Some(taffy::AlignItems::FLEX_START);
        scene.scroll.ensure(pane);
    }
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    assert!(
        s.scene.as_ref().unwrap().warnings.is_empty(),
        "align-items:flex-start breaks the stretch"
    );

    // 3. ul align-self:flex-start 覆盖继承 → 无警告。
    let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
    {
        let scene = s.scene.as_mut().unwrap();
        scene.get_mut(ul).unwrap().style.taffy_style.align_self =
            Some(taffy::AlignSelf::FLEX_START);
        scene.scroll.ensure(pane);
    }
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    assert!(
        s.scene.as_ref().unwrap().warnings.is_empty(),
        "align-self:flex-start on the list opts out of the stretch"
    );

    // 4. 纵向主轴：父 flex column + ul flex-grow>0 → 警告。
    let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
    {
        let scene = s.scene.as_mut().unwrap();
        let pn = scene.get_mut(pane).unwrap();
        pn.style.taffy_style.flex_direction = taffy::FlexDirection::Column;
        scene.get_mut(ul).unwrap().style.taffy_style.flex_grow = 1.0;
        scene.scroll.ensure(pane);
    }
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    assert_eq!(
        s.scene.as_ref().unwrap().warnings.len(),
        1,
        "flex-grow on a column parent stretches the list (main axis)"
    );

    // 5. 自滚模式（ul 自身带 ScrollPane）：拉伸只定 ul 尺寸、滚动发生在内部 → 不警告。
    let (mut s, ul, _li, _pane) = stage_with_pane_ul_li();
    {
        let scene = s.scene.as_mut().unwrap();
        scene.scroll.ensure(ul);
    }
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    assert!(
        s.scene.as_ref().unwrap().warnings.is_empty(),
        "self-scroll list: stretch is harmless"
    );

    // 6. 无 pane：拉伸无所谓（已退化全量渲染）→ 不警告（no-pane 警告归 plan_visible）。
    let (mut s, ul, _li) = stage_with_ul_li();
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    assert!(
        s.scene.as_ref().unwrap().warnings.is_empty(),
        "no pane: no stretch warning at enter"
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
    let scene =
        crate::scene::node::Scene::from_nodes(vec![ancestor, ul_node, li], vec![(0, 1), (1, 2)]);
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
        let slots: Vec<(NodeId, usize)> = ls.slots.iter().map(|s| (s.node, s.item_index)).collect();
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
/// item 的 known 高度，污染下帧可见区计算。
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
        let slots: Vec<(NodeId, usize)> = ls.slots.iter().map(|s| (s.node, s.item_index)).collect();
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

/// #43：Smooth tween 期间高度回填（estimate → 实测更大）→ 锚按最新 heights 重算
/// tween 终点；advance 推到底落在修正后的偏移，不停在过期边界。滚轮接管清锚。
#[test]
fn scroll_to_item_smooth_recomputes_target_on_height_backfill() {
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
        st.content_size = (1000.0, 4000.0);
        st.overlap = (0.0, 3900.0);
        st.scroll_pos = (0.0, 0.0);
    }
    // Smooth：目标 = 50*20 = 1000（estimate 快照），锚已设、tweening[1]=1。
    crate::list::scroll_to_item(&mut s, ul, 50, 1).unwrap();
    {
        let st = s.scene.as_ref().unwrap().scroll.get(pane).unwrap();
        assert_eq!(st.tweening[1], 1, "Smooth 启 tween");
        assert_eq!(st.smooth_scroll_to, Some((ul, 50)), "锚记录 (ul, index)");
    }
    // 模拟滚动中回填：前 60 项实测 30px → sum(0..50) = 1500（目标偏移变了）。
    {
        let scene = s.scene.as_mut().unwrap();
        let ls = scene.lists.get_mut(ul).unwrap();
        for i in 0..60 {
            ls.heights.set(i, 30.0);
        }
    }
    // tick 同款重算（collect_heights 后语义）→ tween 终点更新为 1500。
    crate::list::recompute_smooth_scroll_targets(s.scene.as_mut().unwrap());
    {
        let st = s.scene.as_ref().unwrap().scroll.get(pane).unwrap();
        approx_eq(st.tween_start.1 + st.tween_change.1, 1500.0);
    }
    // advance 推到底（dt > duration）→ 落在修正后偏移；tween 完成清锚。
    let st = s.scene.as_mut().unwrap().scroll.get_mut(pane).unwrap();
    st.advance(1.0);
    let st = s.scene.as_ref().unwrap().scroll.get(pane).unwrap();
    approx_eq(st.scroll_pos.1, 1500.0);
    assert_eq!(st.tweening[1], 0, "tween 完成");
    assert_eq!(st.smooth_scroll_to, None, "完成清锚");

    // 再次 Smooth 后滚轮接管 → 锚作废（重算不复活 tween 终点）。
    crate::list::scroll_to_item(&mut s, ul, 10, 1).unwrap();
    {
        let st = s.scene.as_mut().unwrap().scroll.get_mut(pane).unwrap();
        st.apply_wheel((0.0, -3.0));
        assert_eq!(st.smooth_scroll_to, None, "滚轮接管清锚");
    }
    crate::list::recompute_smooth_scroll_targets(s.scene.as_mut().unwrap());
    let st = s.scene.as_ref().unwrap().scroll.get(pane).unwrap();
    assert_eq!(st.smooth_scroll_to, None, "重算不复活已清锚");
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
    assert_eq!(
        ls.slots.len(),
        slot_count_before,
        "high-water: slots never shrink; parked slots stay in vec"
    );
    assert_eq!(
        ls.slots.iter().filter(|s| s.parked).count(),
        2,
        "two slots parked (items 2,3 removed)"
    );
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
    for s in &ls.slots {
        assert_eq!(
            scene.get(s.node).unwrap().parent,
            Some(ul),
            "no detach on remove: every slot still parented to ul"
        );
    }
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
    for s in &ls.slots {
        assert_eq!(
            scene.get(s.node).unwrap().parent,
            Some(ul),
            "no detach on remove: slot still parented to ul"
        );
    }
    assert_eq!(
        ls.slots.iter().filter(|s| s.parked).count(),
        2,
        "two slots parked (items 3,4 removed)"
    );
    let active_indices: Vec<usize> = ls
        .slots
        .iter()
        .filter(|s| !s.parked)
        .map(|s| s.item_index)
        .collect();
    let mut sorted = active_indices.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2], "active slots cover remaining items");
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
    crate::list::notify_inserted(s.scene.as_mut().unwrap(), ul, 2, 2).unwrap();
    let scene = s.scene.as_ref().unwrap();
    let ls = scene.lists.get(ul).unwrap();
    assert_eq!(ls.item_count, 7, "item_count grown by 2");
    for s in &ls.slots {
        assert_eq!(
            scene.get(s.node).unwrap().parent,
            Some(ul),
            "no detach on insert"
        );
    }
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
    let (mut s, ul, _li, pane) = stage_with_pane_ul_li();
    crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
    crate::list::set_item_count(&mut s, ul, 10);
    // pane 视口未测（viewport.h=0）→ 冷启动 visible=[0,5) → 5 个 slot 绑 items 0..5。
    {
        let scene = s.scene.as_mut().unwrap();
        let st = scene.scroll.ensure(pane);
        st.viewport_size = (1000.0, 0.0);
        st.scroll_pos = (0.0, 0.0);
    }
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

/// plan 阶段的池化契约：**只标记不搬树**。
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

/// execute 阶段的池化契约：**unpark + bind**。
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

/// execute 扩容契约：池里无 parked slot 可复用时克隆模板扩容。
///
/// 高水位只增不减——扩容后即便滚回去也不缩（无驱逐）。新 slot 挂 ul
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
/// ——后者会盖掉作者样式（`li { display:flex }` 的 item 会塌成块流）。
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
                scene.get(slot.node).unwrap().inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
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

/// reuse_key 出生即定、永不旋转。
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

    // slot[0] 是同一个 NodeId，其 reuse_key 跨帧不变。
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
    let host = s.host.borrow();
    crate::layout::solve(
        s.scene.as_mut().unwrap(),
        &host.fonts,
        s.root_size,
        &host.image_sizes,
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
/// "solve 一次/帧" 是声明式不变量，无 instrumentation 无法直接
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
