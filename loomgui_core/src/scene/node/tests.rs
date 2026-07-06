use super::*;

#[test]
fn node_id_index_and_gen_decode() {
    // 高 20 bit index + 低 12 bit gen
    let id = NodeId((5 << 12) | 7);
    assert_eq!(id.index(), 5, "index = 高 20 bit");
    assert_eq!(id.gen(), 7, "gen = 低 12 bit");
}

#[test]
fn node_id_invalid_sentinel() {
    assert!(!NodeId::INVALID.is_valid(), "0xFFFF_FFFF = INVALID");
    assert!(NodeId(0).is_valid(), "0 有效");
}

#[test]
fn scene_nodes_is_slotmap_and_get_by_id() {
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
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Button,
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    // slotmap get by NodeId
    let root_id = scene.roots[0];
    assert!(
        scene.nodes.get(root_id.to_key()).is_some(),
        "live NodeId 可 get（经 to_key）"
    );
    assert!(scene.get(root_id).is_some(), "Scene::get 桥接可用");
    // get_mut
    if let Some(n) = scene.get_mut(root_id) {
        n.disabled = true;
    }
    assert!(scene.get(root_id).unwrap().disabled);
}

#[test]
fn scene_from_nodes_helper_builds_tree() {
    // test helper：从 Vec<Node> 建 Scene（替代字面量）
    let root = Node::default();
    let child = Node::default();
    let scene = Scene::from_nodes(vec![root, child], vec![(0, 1)]); // (parent_idx, child_idx)
    assert_eq!(scene.nodes.len(), 2);
    assert_eq!(scene.roots.len(), 1, "root 无 parent → roots 1 个");
    let root_id = scene.roots[0];
    let root_node = scene.get(root_id).unwrap();
    assert_eq!(root_node.children.len(), 1, "root 有 1 child");
    let child_id = root_node.children[0];
    assert_eq!(scene.get(child_id).unwrap().parent, Some(root_id));
}

#[test]
fn node_id_from_key_to_key_roundtrip() {
    // 验证 NodeId ↔ DefaultKey 桥接 roundtrip（version=1，无删除）
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
        ResolvedStyle::default(),
        Vec::new(),
        None,
        false,
        None,
        None,
    )];
    let scene = Scene::build(&entries);
    let id = scene.roots[0];
    // to_key 后 slotmap 能查到
    assert!(
        scene.nodes.get(id.to_key()).is_some(),
        "to_key 重构的 key 能查到节点"
    );
    // index = slotmap idx = 1（slotmap free_head 从 1 起，idx 0 是 sentinel）
    assert_eq!(id.index(), 1, "首节点 slotmap idx=1");
    assert_eq!(id.gen(), 1, "version=1（无删除）");
}

#[test]
fn node_id_index_capacity_20bit() {
    // 20 bit index 上限 = (1<<20)-1 = 1048575
    let max_idx = (1u32 << 20) - 1;
    let id = NodeId(max_idx << 12);
    assert_eq!(id.index(), max_idx as usize);
}

#[test]
fn node_has_runtime_state_fields_default() {
    let n = Node::default();
    assert!(n.touchable, "touchable 默认 true");
    assert!(!n.hovered);
    assert!(!n.active);
    assert!(!n.disabled);
    assert!(n.classes.is_empty());
    assert!(n.id_attr.is_none());
    // base_style 与 style 初始相同（Default）
    assert_eq!(n.base_style, n.style);
}

#[test]
fn node_has_draggable_field_default_false() {
    let n = Node::default();
    assert!(!n.draggable, "draggable 默认 false");
}

#[test]
fn scene_build_6tuple_sets_draggable() {
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
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Button,
            ResolvedStyle::default(),
            Vec::new(),
            None,
            true,
            None,
            None,
        ),
    ];
    let scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let btn_id = scene.get(root_id).unwrap().children[0];
    assert!(
        !scene.get(root_id).unwrap().draggable,
        "root draggable=false"
    );
    assert!(scene.get(btn_id).unwrap().draggable, "btn draggable=true");
}

#[test]
fn scene_default_has_empty_dynamic_rules() {
    let s = Scene {
        roots: vec![],
        nodes: SlotMap::with_key(),
        dynamic_rules: Default::default(),
        focused_node: None,
        world_transforms: Vec::new(),
        anim: Default::default(),
        scroll: Default::default(),
        text_layouts: Vec::new(),
        node_sort_keys: Vec::new(),
        controllers: Default::default(),
        pending_controller_events: Vec::new(),
        pending_transitions: Vec::new(),
    };
    assert!(
        s.dynamic_rules.rules.is_empty(),
        "Scene 默认 dynamic_rules 空"
    );
}

#[test]
fn node_has_tabindex_focused_defaults() {
    let n = Node::default();
    assert_eq!(n.tabindex, None, "tabindex 默认 None（不可聚焦）");
    assert!(!n.focused, "focused 默认 false");
}

#[test]
fn scene_default_focused_node_none() {
    let s = Scene {
        roots: vec![],
        nodes: SlotMap::with_key(),
        dynamic_rules: Default::default(),
        focused_node: None,
        world_transforms: Vec::new(),
        anim: Default::default(),
        scroll: Default::default(),
        text_layouts: Vec::new(),
        node_sort_keys: Vec::new(),
        controllers: Default::default(),
        pending_controller_events: Vec::new(),
        pending_transitions: Vec::new(),
    };
    assert_eq!(s.focused_node, None, "Scene 默认 focused_node=None");
}

#[test]
fn scene_build_7tuple_sets_tabindex() {
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
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Button,
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            Some(0),
            None,
        ),
        (
            Some(0),
            NodeKind::Button,
            ResolvedStyle::default(),
            Vec::new(),
            None,
            false,
            Some(3),
            None,
        ),
    ];
    let scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let kids = &scene.get(root_id).unwrap().children;
    let btn1 = kids[0];
    let btn2 = kids[1];
    assert_eq!(
        scene.get(root_id).unwrap().tabindex,
        None,
        "root tabindex=None"
    );
    assert_eq!(
        scene.get(btn1).unwrap().tabindex,
        Some(0),
        "btn1 tabindex=Some(0)"
    );
    assert_eq!(
        scene.get(btn2).unwrap().tabindex,
        Some(3),
        "btn2 tabindex=Some(3)"
    );
    assert!(!scene.get(root_id).unwrap().focused, "focused 默认 false");
    assert_eq!(scene.focused_node, None, "build 后 focused_node=None");
}

#[test]
fn scene_build_constructs_tree_without_parse() {
    // 手搓 entries：root Container + 一个 Text 子（parent=Some(0)）。
    // 不走 parse_html/build_scene——证明 Scene::build 独立于 parse（read_package 依赖此）。
    let root_style = ResolvedStyle::default();
    let text_style = ResolvedStyle::default();
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
            root_style,
            Vec::new(),
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Text {
                content: "hi".into(),
            },
            text_style,
            Vec::new(),
            None,
            false,
            None,
            None,
        ),
    ];
    let scene = Scene::build(&entries);

    assert_eq!(scene.nodes.len(), 2);
    assert_eq!(scene.roots.len(), 1, "根 = parent=None 的节点");
    let root_id = scene.roots[0];
    let root = scene.get(root_id).unwrap();
    assert!(matches!(root.kind, NodeKind::Container));
    assert_eq!(root.children.len(), 1, "Text 子挂 root");
    let text_id = root.children[0];
    assert!(root.clip_rect.is_none(), "overflow Visible → 无 clip slot");
    assert!(!root.dirty_text, "Container dirty_text=false");
    let text = scene.get(text_id).unwrap();
    assert!(matches!(&text.kind, NodeKind::Text { content } if content == "hi"));
    assert_eq!(text.parent, Some(root_id));
    assert!(text.dirty_text, "Text 节点 dirty_text=true");

    // overflow Hidden → clip slot 派生
    let mut of = ResolvedStyle::default();
    of.overflow_x = OverflowMode::Hidden;
    of.overflow_y = OverflowMode::Hidden;
    let scene2 = Scene::build(&[(
        None,
        NodeKind::Container,
        of,
        Vec::new(),
        None,
        false,
        None,
        None,
    )]);
    assert!(
        scene2.get(scene2.roots[0]).unwrap().clip_rect.is_some(),
        "overflow Hidden → clip slot"
    );
}

#[test]
fn build_clip_rect_slot_for_scroll_auto_and_single_axis() {
    // overflow != Visible（任一轴）→ clip slot。覆盖 scroll/auto/单轴。
    for (x, y, desc) in [
        (OverflowMode::Scroll, OverflowMode::Scroll, "scroll 双轴"),
        (OverflowMode::Auto, OverflowMode::Auto, "auto 双轴"),
        (
            OverflowMode::Scroll,
            OverflowMode::Visible,
            "仅 x 轴 scroll",
        ),
        (OverflowMode::Visible, OverflowMode::Auto, "仅 y 轴 auto"),
    ] {
        let mut s = ResolvedStyle::default();
        s.overflow_x = x;
        s.overflow_y = y;
        let sc = Scene::build(&[(
            None,
            NodeKind::Container,
            s,
            Vec::new(),
            None,
            false,
            None,
            None,
        )]);
        assert!(
            sc.get(sc.roots[0]).unwrap().clip_rect.is_some(),
            "{} → clip slot",
            desc
        );
    }
    // 双轴 Visible → 无 clip slot（对照）
    let mut vis = ResolvedStyle::default();
    vis.overflow_x = OverflowMode::Visible;
    vis.overflow_y = OverflowMode::Visible;
    let sc = Scene::build(&[(
        None,
        NodeKind::Container,
        vis,
        Vec::new(),
        None,
        false,
        None,
        None,
    )]);
    assert!(
        sc.get(sc.roots[0]).unwrap().clip_rect.is_none(),
        "双轴 Visible → 无 clip slot"
    );
}

/// AnimTable 用 HashMap<NodeId, NodeAnim>。测试一律用 slotmap 分配的真实 NodeId
/// + 生产路径写法（ensure(id)），不用字面量 NodeId(N) 撑表。
fn anim_scene_one_node() -> (Scene, NodeId) {
    let sc = Scene::build(&[(
        None,
        NodeKind::Container,
        ResolvedStyle::default(),
        Vec::new(),
        None,
        false,
        None,
        None,
    )]);
    let id = sc.roots[0];
    (sc, id)
}

#[test]
fn animtable_hashmap_get_ensure_clear() {
    let (_sc, id) = anim_scene_one_node();
    let mut t = AnimTable::default();
    // 未 ensure 的 id → get None
    assert!(t.get(id).is_none(), "未 ensure → get None");
    // ensure + 写
    t.ensure(id).opacity = Some(0.5);
    assert_eq!(t.get(id).unwrap().opacity, Some(0.5));
    // 全默认的 NodeAnim（ensure 后未写任何通道）→ get 返 None（is_empty 过滤）
    let other = {
        let sc = Scene::build(&[
            (
                None,
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
            ),
            (
                Some(0),
                NodeKind::Container,
                ResolvedStyle::default(),
                Vec::new(),
                None,
                false,
                None,
                None,
            ),
        ]);
        sc.roots[0]
    };
    // 注：other 是另一 scene 的 NodeId，此处仅验证 ensure 后未写 → get None
    let mut t2 = AnimTable::default();
    t2.ensure(other);
    assert!(
        t2.get(other).is_none(),
        "ensure 后全 None → is_empty 过滤 → get None"
    );
    // clear_node
    t.clear_node(id);
    assert!(t.get(id).is_none(), "clear_node 后 get 返 None");
}

#[test]
fn animtable_clear_prop_keeps_other_channels() {
    let (_sc, id) = anim_scene_one_node();
    let mut t = AnimTable::default();
    let a = t.ensure(id);
    a.opacity = Some(0.5);
    a.transform = Some(crate::transform::from_scale(2.0, 2.0));
    t.clear_prop(id, crate::tween::TweenProp::Scale);
    assert!(t.get(id).unwrap().transform.is_none(), "清 transform 通道");
    assert_eq!(t.get(id).unwrap().opacity, Some(0.5), "opacity 通道保留");
}

#[test]
fn animtable_clear_prop_all_variants() {
    let (_sc, id) = anim_scene_one_node();
    let mut t = AnimTable::default();
    let a = t.ensure(id);
    a.opacity = Some(0.5);
    a.transform = Some(crate::transform::from_scale(2.0, 2.0));
    a.bg_color = Some([1.0; 4]);
    a.text_color = Some([2.0; 4]);
    // 注：clear_prop 后断言用 t.0.get(&id)（绕过 get 的 is_empty 过滤），
    // 因逐通道清到全 None 时 get 会返 None，但条目本身仍在（clear_node 才 remove）。
    // macro：每次展开独立借用，避免闭包持借冲突 clear_prop 的 &mut。
    macro_rules! raw {
        () => {
            t.0.get(&id).expect("条目存在（clear_prop 不 remove）")
        };
    }
    t.clear_prop(id, crate::tween::TweenProp::Opacity);
    assert!(raw!().opacity.is_none(), "清 opacity");
    assert!(raw!().transform.is_some(), "opacity 清了，transform 保留");
    t.clear_prop(id, crate::tween::TweenProp::Translate);
    assert!(raw!().transform.is_none(), "Translate 清 transform");
    // 重新写 transform 再清 Scale/Rotation
    t.ensure(id).transform = Some(crate::transform::from_scale(2.0, 2.0));
    t.clear_prop(id, crate::tween::TweenProp::Scale);
    assert!(raw!().transform.is_none(), "Scale 清 transform");
    t.ensure(id).transform = Some(crate::transform::from_rotate(0.5));
    t.clear_prop(id, crate::tween::TweenProp::Rotation);
    assert!(raw!().transform.is_none(), "Rotation 清 transform");
    t.clear_prop(id, crate::tween::TweenProp::BgColor);
    assert!(raw!().bg_color.is_none(), "清 bg_color");
    t.clear_prop(id, crate::tween::TweenProp::TextColor);
    assert!(raw!().text_color.is_none(), "清 text_color");
    // 全清后 → is_empty → get None（条目仍在，但 get 过滤掉）
    assert!(
        t.get(id).is_none(),
        "全通道清后 get 返 None（is_empty 过滤）"
    );
    // clear_node 才真正 remove
    t.clear_node(id);
    assert!(!t.0.contains_key(&id), "clear_node 后 HashMap 无条目");
}

#[test]
fn nodeanim_is_empty_default_true() {
    assert!(NodeAnim::default().is_empty());
    assert!(!NodeAnim {
        opacity: Some(0.5),
        ..Default::default()
    }
    .is_empty());
}

#[test]
fn controller_changed_event_abi_size() {
    // #[repr(C)] 跨 FFI：u32 mount_node + i32 prev + i32 new = 12 字节。
    // 断言 ABI 尺寸防 padding 漂移（C# 镜像须对齐）。
    assert_eq!(
        std::mem::size_of::<ControllerChangedEvent>(),
        12,
        "ControllerChangedEvent 须 12 字节（u32 + i32 + i32，repr(C) 无 padding）"
    );
}

#[test]
fn controller_default_selected_index_is_zero() {
    // #[derive(Default)] → selected_index = 0（i32::default）。
    // -1 语义仅由 set_controller_selected 的懒注册 or_insert 提供（无条目表）。
    let c = Controller::default();
    assert_eq!(c.selected_index, 0);
}

#[test]
fn scene_controller_selected_set_get_roundtrip() {
    // set_controller_selected 懒注册 + 返 prev；controller_selected 读回。
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
        ResolvedStyle::default(),
        Vec::new(),
        None,
        false,
        None,
        None,
    )];
    let mut scene = Scene::build(&entries);
    let mount = scene.roots[0];

    // 无条目 → None
    assert_eq!(scene.controller_selected(mount), None);
    // 首次 set → 懒建条目，返 prev=-1
    let prev = scene.set_controller_selected(mount, 3);
    assert_eq!(prev, -1);
    assert_eq!(scene.controller_selected(mount), Some(3));
    // 再 set → 返上次的 3
    let prev = scene.set_controller_selected(mount, 0);
    assert_eq!(prev, 3);
    assert_eq!(scene.controller_selected(mount), Some(0));
}
