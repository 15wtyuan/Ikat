use super::*;
use crate::scene::node::{NodeKind, Rect};
use crate::style::resolved::ResolvedStyle;

/// 构造滚动测试场景：
///   root0 = scroll 容器（overflow_y=Scroll），layout_rect (0,0,100,100)
///   child1 = root0 子，layout_rect (0,0,40,40)
///   child2 = root0 子，layout_rect (0,50,30,30)
///   root1 = 非 scroll（overflow 双轴 Visible），layout_rect (0,0,50,50)
/// content AABB = (max_right 40, max_bottom 80)。
fn build_scroll_scene() -> Scene {
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Scroll;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            scroll_style.clone(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            None,
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
    ];
    let mut s = Scene::build(&entries);
    // root0 = scroll 容器（roots[0]）；root1 = 非 scroll（roots[1]）。
    let root0 = s.roots[0];
    let root1 = s.roots[1];
    let (c0, c1) = {
        let n = s.get(root0).unwrap();
        (n.children[0], n.children[1])
    };
    s.get_mut(root0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 50.0,
        w: 30.0,
        h: 30.0,
    };
    s.get_mut(root1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    s
}

/// 取 scroll 容器 id（= roots[0]）。
fn scroll_container_id(s: &Scene) -> NodeId {
    s.roots[0]
}
/// 取 root0 的两个子 id。
fn child_ids(s: &Scene) -> (NodeId, NodeId) {
    let n = s.get(s.roots[0]).unwrap();
    (n.children[0], n.children[1])
}
/// 取非 scroll 节点 id（= roots[1]）。
fn non_scroll_id(s: &Scene) -> NodeId {
    s.roots[1]
}

#[test]
fn content_size_is_children_aabb() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    refresh_content_sizes(&mut s);
    let st = s.scroll.get(root0).expect("scroll 容器有 state");
    assert!(
        (st.content_size.0 - 40.0).abs() < 1e-3 && (st.content_size.1 - 80.0).abs() < 1e-3,
        "content_size = (40, 80)，got {:?}",
        st.content_size
    );
}

#[test]
fn viewport_and_overlap_from_geometry() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    refresh_content_sizes(&mut s);
    let st = s.scroll.get(root0).unwrap();
    // viewport = layout_rect border box = (100, 100)
    assert!((st.viewport_size.0 - 100.0).abs() < 1e-3);
    assert!((st.viewport_size.1 - 100.0).abs() < 1e-3);
    // overlap = max(content - viewport, 0) = (0, 0) 因 content < viewport 各轴
    // 注：content=(40,80) < viewport=(100,100) → overlap (0,0)
    assert_eq!(st.overlap, (0.0, 0.0));
}

#[test]
fn overlap_clamps_negative_to_zero() {
    // content < viewport → overlap 0（与上一测同场景，显式命名）
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    refresh_content_sizes(&mut s);
    let st = s.scroll.get(root0).unwrap();
    assert_eq!(st.overlap, (0.0, 0.0));
}

#[test]
fn overlap_positive_when_content_exceeds_viewport() {
    // 改子 layout_rect 让 content > viewport y 轴
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    };
    refresh_content_sizes(&mut s);
    let st = s.scroll.get(root0).unwrap();
    // content = (40, 200)；viewport = (100,100) → overlap = (0, 100)
    assert!(
        (st.overlap.0 - 0.0).abs() < 1e-3 && (st.overlap.1 - 100.0).abs() < 1e-3,
        "overlap y = 100，got {:?}",
        st.overlap
    );
}

#[test]
fn non_scroll_node_has_no_state() {
    let mut s = build_scroll_scene();
    let root1 = non_scroll_id(&s);
    refresh_content_sizes(&mut s);
    // root1 双轴 Visible → 非 scroll 容器 → scroll.get 返 None
    assert!(s.scroll.get(root1).is_none(), "非 scroll 节点无 state");
}

#[test]
fn capable_and_effective_semantics() {
    // capable: Scroll/Auto true；Visible/Hidden false
    assert!(capable(OverflowMode::Scroll));
    assert!(capable(OverflowMode::Auto));
    assert!(!capable(OverflowMode::Visible));
    assert!(!capable(OverflowMode::Hidden));
    // effective: Scroll 永真（capable 且 == Scroll）；Auto 仅 content>viewport
    assert!(
        effective(OverflowMode::Scroll, 10.0, 100.0),
        "Scroll 即使 content<viewport 仍可滚"
    );
    assert!(
        effective(OverflowMode::Auto, 200.0, 100.0),
        "Auto content>viewport 可滚"
    );
    assert!(
        !effective(OverflowMode::Auto, 50.0, 100.0),
        "Auto content<viewport 不可滚"
    );
    assert!(
        !effective(OverflowMode::Visible, 200.0, 100.0),
        "Visible 不可滚"
    );
}

#[test]
fn scrolltable_hashmap_get_mut_ensure_clear() {
    // ScrollTable 用 HashMap<NodeId, ScrollPaneState>。NodeId 已 impl Hash+Eq，
    // 不依赖 slotmap 主表存不存在，故此单元测试可不经 Scene 直接造字面量 NodeId。
    let mk = |idx: u32| NodeId((idx << 12) | 1);
    let mut t = ScrollTable::default();
    assert!(t.get(mk(2)).is_none(), "空表 get → None");
    // ensure 插 default
    let st = t.ensure(mk(2));
    st.scroll_pos = (5.0, 7.0);
    assert_eq!(t.0.len(), 1, "ensure(mk(2)) → 1 个条目");
    let got = t.get(mk(2)).unwrap();
    assert_eq!(got.scroll_pos, (5.0, 7.0));
    // get_mut
    {
        let m = t.get_mut(mk(2)).unwrap();
        m.scroll_pos = (1.0, 2.0);
    }
    assert_eq!(t.get(mk(2)).unwrap().scroll_pos, (1.0, 2.0));
    // ensure 同 id 二次返同槽（不重置）
    let st2 = t.ensure(mk(2));
    assert_eq!(st2.scroll_pos, (1.0, 2.0), "二次 ensure 不重置已有值");
    // 不同 id → 不同槽
    t.ensure(mk(5)).scroll_pos = (9.0, 9.0);
    assert_eq!(t.0.len(), 2, "ensure 不同 id → 2 个条目");
    assert!(t.get(mk(5)).is_some());
    // 未 ensure 的 id → None
    assert!(t.get(mk(99)).is_none(), "未 ensure 的 id → None");
    // clear
    t.clear();
    assert!(t.0.is_empty(), "clear 清空");
    assert!(t.get(mk(2)).is_none(), "clear 后 get None");
}

#[test]
fn content_size_dirty_flag_when_changes() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    refresh_content_sizes(&mut s);
    let st = s.scroll.get(root0).unwrap();
    // 首次：default (0,0) → (40,80) → dirty true
    assert!(st.content_size_dirty, "首次填入非零 content → dirty");
    // 再 refresh 一次（content 不变）→ dirty false
    refresh_content_sizes(&mut s);
    let st2 = s.scroll.get(root0).unwrap();
    assert!(!st2.content_size_dirty, "content 未变 → dirty false");
    // 改子尺寸 → dirty true
    let (_, c1) = child_ids(&s);
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    };
    refresh_content_sizes(&mut s);
    let st3 = s.scroll.get(root0).unwrap();
    assert!(st3.content_size_dirty, "content 变 → dirty true");
}

#[test]
fn empty_children_content_is_zero() {
    // 滚动容器无子 → content (0,0)
    let mut style = ResolvedStyle::default();
    style.overflow_y = OverflowMode::Scroll;
    let entries = vec![(
        None,
        NodeKind::Container,
        style,
        vec![],
        None,
        false,
        None,
        None,
    )];
    let mut s = Scene::build(&entries);
    let root0 = s.roots[0];
    s.get_mut(root0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    refresh_content_sizes(&mut s);
    let st = s.scroll.get(root0).unwrap();
    assert_eq!(st.content_size, (0.0, 0.0), "无子 content = (0,0)");
    assert_eq!(st.overlap, (0.0, 0.0));
}

// ── 物理方法测 ────────────────────────────────────────
#[test]
fn drag_follow_one_to_one_within_bounds() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0);
    st.viewport_size = (100.0, 50.0);
    st.drag_follow((0.0, 10.0), 0.016); // delta (0,10) 界内 1:1
    assert!(
        (st.scroll_pos.1 - 10.0).abs() < 1e-2,
        "跟手 1:1 界内无打折，got {}",
        st.scroll_pos.1
    );
}

#[test]
fn drag_follow_beyond_bound_damped_by_pull_ratio() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0);
    // viewport 必须够大（vp*PULL_RATIO=50 > delta 30）才不被 cap，打折全额生效
    st.viewport_size = (100.0, 100.0);
    st.scroll_pos = (0.0, 0.0);
    st.drag_follow((0.0, -30.0), 0.016); // 往上越界 30
                                         // 越界打折：over=30，scroll_pos.y = 0 - 30*0.5 = -15（PULL_RATIO）
    assert!(
        (st.scroll_pos.1 - (-15.0)).abs() < 1e-1,
        "越界 PULL_RATIO 打折，got {}",
        st.scroll_pos.1
    );
}

#[test]
fn drag_follow_skips_zero_overlap_axis() {
    // overflow-y 容器 x 轴 overlap=0 → drag 不动 x（防斜拖 x 抖动）。
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0); // x overlap=0（仅垂直可滚）
    st.viewport_size = (100.0, 100.0);
    st.drag_follow((50.0, 10.0), 0.016); // x delta=50 但 overlap.x=0
    assert!(
        st.scroll_pos.0 == 0.0,
        "overlap=0 轴 drag 不动（防抖），got {}",
        st.scroll_pos.0
    );
    assert!(
        (st.scroll_pos.1 - 10.0).abs() < 1e-2,
        "y 轴正常跟手，got {}",
        st.scroll_pos.1
    );
}

/// 大越界（|np|>vp）→ 最大越界 = vp*PULL_RATIO（min(位移*0.5, vp*0.5)）。
#[test]
fn drag_follow_large_over_bound_caps_at_vp_pull_ratio() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0);
    st.viewport_size = (100.0, 100.0); // vp=100
    st.scroll_pos = (0.0, 0.0);
    st.drag_follow((0.0, -500.0), 0.016); // 巨大越界（远超 vp）
                                          // 最大越界 = vp*PULL_RATIO = 100*0.5 = 50
    assert!(
        (st.scroll_pos.1 - (-50.0)).abs() < 1e-1,
        "大越界 cap 在 vp*PULL_RATIO=-50，got {}",
        st.scroll_pos.1
    );
}

#[test]
fn inertia_advances_toward_target_then_settles() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 1000.0);
    st.scroll_pos = (0.0, 0.0);
    st.velocity = (0.0, 2000.0); // |v|=2000 > PC 阈值 500
    st.begin_inertia(false); // is_touch=false (PC 阈值 500)
                             // v2=|v|=2000 → dur=|log(60/2000)/log(0.967)|/60 ≈ 1.74s
                             // change=2000·1.74·0.4≈1387px > overlap 1000 → clamp 到 1000
                             // 1.74s @16ms ≈ 109 步，150 步覆盖 ~2.4s > dur
    for _ in 0..150 {
        st.advance(0.016);
        if st.tweening == 0 {
            break;
        }
    }
    assert!(
        st.scroll_pos.1 > 100.0,
        "惯性产生了位移，got {}",
        st.scroll_pos.1
    );
    assert_eq!(st.tweening, 0, "tween 完成归零");
}

#[test]
fn bounce_returns_to_boundary() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0);
    st.scroll_pos = (0.0, -30.0); // 越界 30 > 20 阈值
    st.begin_bounce();
    for _ in 0..60 {
        st.advance(0.016);
        if st.tweening == 0 {
            break;
        }
    }
    assert!(
        (st.scroll_pos.1 - 0.0).abs() < 1e-2,
        "回弹回边界 0，got {}",
        st.scroll_pos.1
    );
}

#[test]
fn wheel_steps_and_clamps() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 1000.0);
    st.apply_wheel((0.0, 1.0)); // delta_y=1 上滚 → scroll 减
                                // 上滚 = scroll_pos.y 减少；clamp 后启 tween
    assert!(
        st.tweening != 0 || st.scroll_pos.1 == 0.0,
        "wheel 启 tween 或 clamp 到 0，tweening={}, pos={}",
        st.tweening,
        st.scroll_pos.1
    );
}

#[test]
fn set_pos_snap_when_not_animated() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0);
    st.tweening = 2; // 已有 tween 进行中
    st.set_pos((0.0, 50.0), false);
    assert_eq!(st.scroll_pos.1, 50.0, "snap 直接到位");
    assert_eq!(st.tweening, 0, "animated=false tweening 归零");
}

#[test]
fn set_pos_animated_starts_tween() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 100.0);
    st.scroll_pos = (0.0, 10.0);
    st.set_pos((0.0, 50.0), true);
    assert_eq!(st.tweening, 1, "animated=true 启 tweening=1");
    assert_eq!(st.tween_start.1, 10.0, "tween_start = 当前 pos");
    assert_eq!(st.tween_change.1, 40.0, "tween_change = target - start");
    assert_eq!(st.tween_duration.1, TWEEN_TIME_DEFAULT);
}

#[test]
fn cubic_out_curve_endpoints() {
    assert!((cubic_out(0.0) - 0.0).abs() < 1e-4, "cubic_out(0)=0");
    assert!((cubic_out(1.0) - 1.0).abs() < 1e-4, "cubic_out(1)=1");
    // 单调增（中点 > 0.5，缓动尾部慢）
    let mid = cubic_out(0.5);
    assert!(
        mid > 0.5 && mid < 1.0,
        "cubic_out(0.5)∈(0.5,1)，got {}",
        mid
    );
}

// ── content_size 变化补偿（最小） ────────────────────────────────────
#[test]
fn content_size_change_clamps_running_tween() {
    // 滚动到 pos=80（overlap=100），tweening≠0；然后 content 缩 → overlap 变 50
    // → scroll_pos 越界（80 > 50）→ refresh 应 clamp + tweening 归零
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    };
    refresh_content_sizes(&mut s);
    let st = s.scroll.get_mut(root0).unwrap();
    st.scroll_pos = (0.0, 80.0);
    st.tweening = 1; // 模拟 tween 进行中
                     // 缩 content：子 2 高度 200→100 → content_y=100，viewport=100 → overlap_y=0
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 100.0,
    };
    refresh_content_sizes(&mut s);
    let st2 = s.scroll.get(root0).unwrap();
    assert_eq!(st2.overlap.1, 0.0, "content 缩后 overlap=0");
    assert_eq!(st2.scroll_pos.1, 0.0, "越界 pos 被 clamp 到新 overlap");
    assert_eq!(st2.tweening, 0, "content 变化时 tween 取消");
}

#[test]
fn content_size_change_in_range_keeps_tween() {
    // pos 在新 [0, overlap] 内 → 不打断 tween（最小补偿仅处理越界）
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    };
    refresh_content_sizes(&mut s);
    let st = s.scroll.get_mut(root0).unwrap();
    st.scroll_pos = (0.0, 10.0);
    st.tweening = 1;
    // content 略缩但 pos=10 仍在 [0, overlap]（新 overlap 仍 ≥ 10）
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 150.0,
    };
    refresh_content_sizes(&mut s);
    let st2 = s.scroll.get(root0).unwrap();
    assert_eq!(st2.tweening, 1, "pos 在范围内不打断 tween");
}

// ── apply_wheel_to_hit ─────────────────────────────────────────────
#[test]
fn apply_wheel_to_hit_scrolls_nearest_effective_ancestor() {
    use crate::scene::transform::compute_world_transforms;

    // 构造 scene：overflow:scroll 容器 + content>viewport（effective_y=true）
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (_, c1) = child_ids(&s);
    // 扩子节点使 content_size > viewport_size on y 轴
    // content AABB y = max(40, 250) = 250 > viewport=100 → overlap_y=150
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 250.0,
    };
    // Scene::build 为 overflow node 设 clip_rect=Rect::default()（(0,0,0,0) 挡全部命中）；
    // 手填为 layout_rect 同尺寸让 hit_test 能命中。
    s.get_mut(root0).unwrap().clip_rect = Some(Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });

    // 填 scroll state（content_size/viewport/overlap）+ world transforms（hit_test 用）
    refresh_content_sizes(&mut s);
    compute_world_transforms(&mut s);

    // 核实场景生效
    {
        let st = s.scroll.get(root0).unwrap();
        assert!(
            st.overlap.1 > 0.0,
            "content 超出 viewport，overlap_y={}",
            st.overlap.1
        );
        assert_eq!(st.tweening, 0, "初始 tweening=0");
    }

    // hit 容器内一点 (10,10) → hit_test 命中子节点 1 → parent 遍历到节点 0
    // → 节点 0 overflow_y=Scroll + effective → apply_wheel
    apply_wheel_to_hit(
        &mut s,
        WheelEvent {
            x: 10.0,
            y: 10.0,
            delta_x: 0.0,
            delta_y: 1.0,
        },
    );

    let st = s.scroll.get(root0).unwrap();
    assert!(
        st.tweening != 0,
        "wheel 触发滚动 tween，tweening={}",
        st.tweening
    );
}

/// wheel 落 thumb 区域，hit_test 返 sentinel → apply_wheel_to_hit 解码
/// container_id 继续祖先链，不 crash 且正确滚该容器。
#[test]
fn apply_wheel_to_hit_on_thumb_decodes_sentinel() {
    use crate::scene::transform::compute_world_transforms;

    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (_, c1) = child_ids(&s);
    // content_y=250 > viewport=100 → overlap_y=150
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 250.0,
    };
    s.get_mut(root0).unwrap().clip_rect = Some(Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });

    refresh_content_sizes(&mut s);
    compute_world_transforms(&mut s);

    // 核实 scroll state
    let st = s.scroll.get(root0).unwrap();
    assert!(st.overlap.1 > 0.0, "overlap needed for thumb");
    assert_eq!(st.tweening, 0);

    // v_thumb_rect: x=92, y=0, w=8, h=40（100*(100/250)=40）
    // 点 (96, 20) 在 thumb 内 → hit_test 应返 sentinel
    let hit = crate::hit::hit_test(&s, (96.0, 20.0));
    assert!(
        hit.is_some_and(|id| id.0 & 0x6000_0000 != 0),
        "thumb 命中应返 sentinel，got {:?}",
        hit
    );

    // apply_wheel_to_hit：sentinel 解码 → container 0 → apply_wheel
    apply_wheel_to_hit(
        &mut s,
        WheelEvent {
            x: 96.0,
            y: 20.0,
            delta_x: 0.0,
            delta_y: 1.0,
        },
    );

    let st = s.scroll.get(root0).unwrap();
    assert!(
        st.tweening != 0,
        "thumb wheel 应触发滚动，tweening={}",
        st.tweening
    );
}

// ── thumb rect 测 ─────────────────────────────────────────
#[test]
fn v_thumb_rect_is_right_edge_with_proportional_size() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    };
    refresh_content_sizes(&mut s);
    // viewport=(100,100) content=(40,200) → overlap=(0,100)
    // thumb_h = 100*(100/200)=50, track_h=100, perc=0 → thumb_y = lr.y=0
    let r = v_thumb_rect(&s, root0).expect("overlap>0 → thumb");
    assert_eq!(r.w, 8.0, "track_w=8");
    assert!(
        (r.h - 50.0).abs() < 1e-2,
        "thumb_h = 100*(100/200)=50, got {}",
        r.h
    );
    assert_eq!(r.x, 92.0, "右边缘: x = lr.x(0) + lr.w(100) - track_w(8)");
    assert_eq!(r.y, 0.0, "scroll_pos=0 → thumb 在顶端");
}

#[test]
fn v_thumb_rect_moves_with_scroll_pos() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    };
    refresh_content_sizes(&mut s);
    let st = s.scroll.get_mut(root0).unwrap();
    st.scroll_pos.1 = 50.0; // 50% scrolled
    let r = v_thumb_rect(&s, root0).unwrap();
    // thumb_h=50, track_h=100, travel=50, perc=0.5 → thumb_y = 0 + 50*0.5 = 25
    assert!(
        (r.y - 25.0).abs() < 1e-2,
        "50% scroll → thumb_y=25, got {}",
        r.y
    );
}

#[test]
fn thumb_rect_returns_none_when_no_overlap() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    // content < viewport → overlap=(0,0)
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 50.0,
        w: 30.0,
        h: 30.0,
    };
    refresh_content_sizes(&mut s);
    assert!(v_thumb_rect(&s, root0).is_none(), "overlap=0 → 无 thumb");
    assert!(h_thumb_rect(&s, root0).is_none(), "overlap=0 → 无 thumb");
}

#[test]
fn h_thumb_rect_is_bottom_edge() {
    let mut s = build_scroll_scene();
    let root0 = scroll_container_id(&s);
    let (c0, c1) = child_ids(&s);
    s.get_mut(c0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 40.0,
    };
    s.get_mut(c1).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 50.0,
        w: 30.0,
        h: 30.0,
    };
    refresh_content_sizes(&mut s);
    // viewport=(100,100) content=(200,80) → overlap=(100,0)
    // h_thumb: track_h=8, track_w=100, thumb_w=100*(100/200)=50
    let r = h_thumb_rect(&s, root0).expect("overlap_x>0 → h_thumb");
    assert_eq!(r.h, 8.0, "track_h=8");
    assert!((r.w - 50.0).abs() < 1e-1, "thumb_w = 100*(100/200)=50");
    assert_eq!(r.y, 92.0, "底边: y = lr.y(0) + lr.h(100) - track_h(8)");
}

// ── 滚动松手物理 ─────────────────────────────────────────────────
// 越界松手：直接 bounce 回边界，不 inertia。
// 界内：二次 ratio 削弱低速；inertia target 不 clamp，advance 运行时越界 >20px 截断 + 回弹。
/// 界内速度刚过阈值：二次 ratio 削弱使 change 极小（≈5px），而非全速冲越界。
#[test]
fn inertia_quad_ratio_damps_low_velocity() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 1000.0);
    st.viewport_size = (200.0, 200.0);
    st.scroll_pos = (0.0, 500.0);
    st.velocity = (0.0, 625.0); // 刚过 PC 阈值 500
    st.begin_inertia(false);
    assert_eq!(st.tweening, 2, "ratio>0 启 inertia");
    assert!(
        st.tween_change.1.abs() < 10.0,
        "二次 ratio 削弱：|change|<10（≈5px），got {}",
        st.tween_change.1
    );
}

/// 界内快速 inertia（target 远超 overlap）→ advance 运行时越界截断 + 回弹
/// （弹性过冲），不冲远空白；最终回弹到边界。
#[test]
fn inertia_overshoot_then_bounce_back_to_boundary() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 400.0);
    st.viewport_size = (200.0, 200.0);
    st.scroll_pos = (0.0, 380.0);
    st.velocity = (0.0, 2000.0);
    st.begin_inertia(false);
    let mut max_pos: f32 = 0.0;
    let mut settled = false;
    for _ in 0..300 {
        st.advance(0.016);
        max_pos = max_pos.max(st.scroll_pos.1);
        if st.tweening == 0 {
            settled = true;
            break;
        }
    }
    assert!(settled, "inertia + 回弹应完成");
    assert!(
        (st.scroll_pos.1 - 400.0).abs() < 1e-1,
        "最终回弹到边界 400，got {}",
        st.scroll_pos.1
    );
    // 过冲有上限（运行时截断）
    assert!(
        max_pos < 500.0,
        "过冲 <500（弹性过冲上限），got {}",
        max_pos
    );
}

#[test]
fn over_bounds_small_release_bounces_smoothly_not_snap() {
    // drag 越界 5px + 小 velocity（<PC 阈值 500）：松手应平滑 bounce 回边界，
    // 而非 advance done 瞬间 clamp snap。
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 400.0);
    st.viewport_size = (200.0, 200.0);
    st.scroll_pos = (0.0, -5.0);
    st.velocity = (0.0, -100.0);
    st.begin_inertia(false);
    assert_eq!(st.tweening, 2, "越界松手启 bounce tween");
    assert!(
        (st.tween_change.1 - 5.0).abs() < 1e-2,
        "bounce change = 0-(-5) = +5，got {}",
        st.tween_change.1
    );
    // 推进 1 帧：cubic_out(norm<<1) 平滑，pos 不应瞬间到 0（snap）
    st.advance(0.016);
    assert!(
        st.scroll_pos.1 > -5.0 && st.scroll_pos.1 < 0.0,
        "第 1 帧平滑回弹（非瞬间 snap），got {}",
        st.scroll_pos.1
    );
    for _ in 0..60 {
        st.advance(0.016);
        if st.tweening == 0 {
            break;
        }
    }
    assert!(
        (st.scroll_pos.1 - 0.0).abs() < 1e-2,
        "bounce 回边界 0，got {}",
        st.scroll_pos.1
    );
}

#[test]
fn over_bounds_fast_velocity_bounces_not_overshoot() {
    // drag 越界 25px + 越界方向快速 velocity：松手应 bounce 回边界，
    // 不应 inertia 冲到巨量空白再 snap。
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 400.0);
    st.viewport_size = (200.0, 200.0);
    st.scroll_pos = (0.0, -25.0);
    st.velocity = (0.0, -2000.0);
    st.begin_inertia(false);
    for _ in 0..200 {
        st.advance(0.016);
        assert!(
            st.scroll_pos.1 >= -30.0,
            "越界松手不冲空白（>=-30），got {}",
            st.scroll_pos.1
        );
        if st.tweening == 0 {
            break;
        }
    }
    assert!(
        (st.scroll_pos.1 - 0.0).abs() < 1e-1,
        "bounce 回边界 0，got {}",
        st.scroll_pos.1
    );
}

/// 界内 velocity 不足松手 → 停在当前位置（不启 inertia 也不 bounce）。
/// 界内慢拖松手不应回原位、不应弹。
#[test]
fn in_bounds_low_velocity_stays_put() {
    let mut st = ScrollPaneState::default();
    st.overlap = (0.0, 400.0);
    st.viewport_size = (200.0, 200.0);
    st.scroll_pos = (0.0, 100.0); // 界内中间
    st.velocity = (0.0, 100.0); // <500 阈值
    st.begin_inertia(false);
    assert_eq!(st.tweening, 0, "界内 velocity 不足 → 不启 tween（停）");
    st.advance(0.016);
    assert!(
        (st.scroll_pos.1 - 100.0).abs() < 1e-4,
        "pos 保持 100（不回原位/不弹），got {}",
        st.scroll_pos.1
    );
}

// ── content_size 注入测 ─────────────────────────────

/// 建含单个 scroll 容器的 Stage（无子节点），供 驱动注入测试用。
/// root 是 overflow_y=Scroll 的 Container，layout_rect (0,0,200,100)。
fn build_scroll_stage() -> crate::stage::Stage {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut stage = crate::stage::Stage::new((200.0, 200.0)).unwrap();
    stage
        .register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Scroll;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![(
        None,
        NodeKind::Container,
        scroll_style,
        vec![],
        None,
        false,
        None,
        None,
    )];
    let mut s = Scene::build(&entries);
    let root0 = s.roots[0];
    s.get_mut(root0).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    stage.scene = Some(s);
    stage
}

#[test]
fn set_content_size_overrides_refresh() {
    // driver 注入 content_size 后，refresh_content_sizes 不覆盖。
    let mut stage = build_scroll_stage();
    let root_id = stage
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .values()
        .next()
        .unwrap()
        .id;
    // driver 注入 content_size
    stage.set_content_size(root_id, 0.0, 8000.0);
    let st = stage.scene.as_ref().unwrap().scroll.get(root_id).unwrap();
    assert_eq!(st.content_size, (0.0, 8000.0));
    assert!(st.content_size_overridden, "注入后标 overridden");
    // refresh 不覆盖
    crate::scroll::refresh_content_sizes(stage.scene.as_mut().unwrap());
    let st = stage.scene.as_ref().unwrap().scroll.get(root_id).unwrap();
    assert_eq!(
        st.content_size,
        (0.0, 8000.0),
        "refresh 不覆盖已注入的 content_size"
    );
    // viewport/overlap 重算（viewport 更新，overlap 用注入的 content_size）
    assert!(
        (st.viewport_size.0 - 200.0).abs() < 1e-3 && (st.viewport_size.1 - 100.0).abs() < 1e-3,
        "viewport 更新为 layout_rect 尺寸"
    );
    assert!(
        (st.overlap.1 - 7900.0).abs() < 1e-3,
        "overlap = content(8000) - viewport(100) = 7900"
    );
}

#[test]
fn get_scroll_pos_reads_state() {
    let mut stage = build_scroll_stage();
    let root_id = stage
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .values()
        .next()
        .unwrap()
        .id;
    // 注入 content_size 造 overlap（override 容器 refresh 后才算 overlap）
    stage.set_content_size(root_id, 0.0, 200.0);
    crate::scroll::refresh_content_sizes(stage.scene.as_mut().unwrap());
    // overlap.y = max(200-100, 0) = 100，scroll_pos 在界内
    stage.set_scroll_pos(root_id, 0.0, 50.0, false);
    assert_eq!(stage.get_scroll_pos(root_id), Some((0.0, 50.0)));
    // 无效 node → None
    assert_eq!(stage.get_scroll_pos(NodeId(0xFFFF_FFFF)), None);
}

#[test]
fn get_node_layout_rect_reads_solved() {
    let stage = build_scroll_stage();
    let root_id = stage
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .values()
        .next()
        .unwrap()
        .id;
    // build_scroll_stage 已手动设 layout_rect (0,0,200,100)
    assert_eq!(
        stage.get_node_layout_rect(root_id),
        Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0
        })
    );
    // 无效 node → None
    assert_eq!(stage.get_node_layout_rect(NodeId(0xFFFF_FFFF)), None);
}

#[test]
fn clear_content_size_override_restores_auto() {
    // clear 后 refresh 恢复子节点 AABB 自动算。
    let mut stage = build_scroll_stage();
    let root_id = stage
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .values()
        .next()
        .unwrap()
        .id;
    stage.set_content_size(root_id, 0.0, 8000.0);
    assert!(
        stage
            .scene
            .as_ref()
            .unwrap()
            .scroll
            .get(root_id)
            .unwrap()
            .content_size_overridden
    );
    stage.clear_content_size_override(root_id);
    assert!(
        !stage
            .scene
            .as_ref()
            .unwrap()
            .scroll
            .get(root_id)
            .unwrap()
            .content_size_overridden
    );
    // refresh 后 content_size 回到子节点 AABB（build_scroll_stage 无子节点 → (0,0)）
    crate::scroll::refresh_content_sizes(stage.scene.as_mut().unwrap());
    let st = stage.scene.as_ref().unwrap().scroll.get(root_id).unwrap();
    assert!(!st.content_size_overridden, "clear 后不再 overridden");
    // content_size 不再是注入的 8000（回到自动算的无子节点 AABB=0）
    assert_ne!(
        st.content_size.1, 8000.0,
        "clear 后 content_size 回到自动算"
    );
}
