use super::*;

use crate::scene::node::NodeFlags;

#[test]
fn role_table_get_insert_remove() {
    // 稀疏 side table：空 info 不入表，role_of/slot_of 查无槽节点返 None。
    let mut t = RoleTable::default();
    let id = NodeId(7);
    assert!(t.get(id).is_none());
    assert_eq!(t.role_of(id), None);
    let info = RoleInfo {
        role: Some("slider".into()),
        slots: [("thumb".into(), "".into())].into_iter().collect(),
        aria_controls: None,
    };
    t.insert(id, info.clone());
    assert_eq!(t.role_of(id), Some("slider"));
    assert_eq!(t.slot_of(id, "thumb"), Some(""));
    // 空 info 不入表（保持稀疏：普通 div 无 role/data-slot 不占槽）。
    t.insert(NodeId(8), RoleInfo::default());
    assert!(t.get(NodeId(8)).is_none());
    t.remove(id);
    assert!(t.get(id).is_none());
}

#[test]
fn lookup_scope_flag_exists_distinct_from_scope_root() {
    assert!(NodeFlags::LOOKUP_SCOPE.contains(NodeFlags::LOOKUP_SCOPE));
    assert!(!NodeFlags::LOOKUP_SCOPE.contains(NodeFlags::SCOPE_ROOT));
    let both = NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE;
    assert!(both.contains(NodeFlags::SCOPE_ROOT));
    assert!(both.contains(NodeFlags::LOOKUP_SCOPE));
}

#[test]
fn find_by_id_attr_global_match_unaffected_by_flag_split() {
    // 不引入 scoped find（slot 边界由 list.rs 处理），只拆 flag。
    // 锁定：增加 LOOKUP_SCOPE 后全局首匹配不变。
    use crate::scene::dynamic;
    let mut scene = Scene::default();
    let root = dynamic::create_root(&mut scene, "div", "").unwrap();
    let child = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, root, child).unwrap();
    scene.get_mut(child).unwrap().id_attr = Some("dup".into());
    assert_eq!(scene.find_by_id_attr("dup"), Some(child));
}

#[test]
fn find_node_by_id_in_subtree_self_exclusive() {
    use crate::scene::dynamic;
    let mut scene = Scene::default();
    let root = dynamic::create_root(&mut scene, "div", "").unwrap();
    scene.get_mut(root).unwrap().id_attr = Some("me".into());
    assert_eq!(
        scene.find_node_by_id_in_subtree(root, "me"),
        None,
        "root self should NOT be hit (self-exclusive; only descendants)"
    );
}

#[test]
fn find_node_by_id_in_subtree_hits_descendant_not_others() {
    use crate::scene::dynamic;
    let mut scene = Scene::default();
    let root = dynamic::create_root(&mut scene, "div", "").unwrap();
    let parent_a = dynamic::create_node(&mut scene, "div", "").unwrap();
    let parent_b = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, root, parent_a).unwrap();
    dynamic::append_child(&mut scene, root, parent_b).unwrap();
    let badge_a = dynamic::create_node(&mut scene, "div", "").unwrap();
    let badge_b = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, parent_a, badge_a).unwrap();
    dynamic::append_child(&mut scene, parent_b, badge_b).unwrap();
    scene.get_mut(badge_a).unwrap().id_attr = Some("badge".into());
    scene.get_mut(badge_b).unwrap().id_attr = Some("badge".into());
    assert_eq!(
        scene.find_node_by_id_in_subtree(parent_a, "badge"),
        Some(badge_a),
        "subtree find from parent_a should hit badge_a (descendant)"
    );
    assert_eq!(
        scene.find_node_by_id_in_subtree(parent_b, "badge"),
        Some(badge_b),
        "subtree find from parent_b should hit badge_b (descendant)"
    );
    assert_eq!(
        scene.find_node_by_id_in_subtree(badge_a, "badge"),
        None,
        "badge_a has no descendants; self-exclusive returns None"
    );
}

#[test]
fn find_node_by_id_in_subtree_returns_none_for_foreign() {
    use crate::scene::dynamic;
    let mut scene = Scene::default();
    let root = dynamic::create_root(&mut scene, "div", "").unwrap();
    let child = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, root, child).unwrap();
    scene.get_mut(child).unwrap().id_attr = Some("badge".into());
    // badge 在 root 子树内但不在 other_root（独立根）子树内。
    let other_root = dynamic::create_root(&mut scene, "div", "").unwrap();
    assert_eq!(
        scene.find_node_by_id_in_subtree(other_root, "badge"),
        None,
        "foreign subtree should return None"
    );
    assert_eq!(
        scene.find_node_by_id_in_subtree(child, "nonexistent"),
        None,
        "missing id should return None"
    );
}

#[test]
fn find_node_by_id_in_subtree_n_slots_same_internal_id() {
    use crate::scene::dynamic;
    let mut scene = Scene::default();
    let root = dynamic::create_root(&mut scene, "div", "").unwrap();
    let mut slots = Vec::new();
    for _ in 0..3 {
        let slot = dynamic::create_node(&mut scene, "div", "").unwrap();
        scene.get_mut(slot).unwrap().id_attr = Some("slot".into());
        let badge = dynamic::create_node(&mut scene, "div", "").unwrap();
        scene.get_mut(badge).unwrap().id_attr = Some("badge".into());
        dynamic::append_child(&mut scene, slot, badge).unwrap();
        dynamic::append_child(&mut scene, root, slot).unwrap();
        slots.push((slot, badge));
    }
    // each slot's subtree should find its own badge
    for &(slot, badge) in &slots {
        assert_eq!(
            scene.find_node_by_id_in_subtree(slot, "badge"),
            Some(badge),
            "each slot should find its own badge"
        );
    }
    assert_ne!(slots[0].1, slots[1].1);
    assert_ne!(slots[1].1, slots[2].1);
}

/// 不变式回归守卫：public 语义树 ≠ internal taffy/render 树。
/// rich-text-block 容器的 inline 子（TextNode / span=TextElement）在 solve 期被折出 taffy
/// （`layout::solve::build` 对 rich_text_block 不递归子进 taffy → 它们 layout_rect 塌成 0），
/// 但仍留在 Scene 树里——故 `find_node_by_id_in_subtree` 仍能按 id 找到 span，`Get<T>("id")`
/// 语义不破。此前仅 `dump_rich_text` example 证据此性质，此处固化为自动测试防回归
/// （若有人在折叠路径上误从 `scene.children` 移除 inline 子，此测试即失败）。
#[test]
fn rich_text_block_inline_children_remain_in_scene_tree_after_solve() {
    use crate::text::layout::FontTable;
    // DejaVu 测试字体（与 render tests 同源，仓库内 fixtures）；缺则跳过——保持跨机可跑。
    let path = format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = match std::fs::read(&path).ok() {
        Some(b) => b,
        None => {
            eprintln!("skip: no test font at {}", path);
            return;
        }
    };
    let mut fonts = FontTable::new();
    fonts
        .register("DejaVu", bytes, true)
        .expect("DejaVu fixture 字体注册");

    // 结构（镜像 rich_compile 的 span_text_run_source_is_span_not_textnode）。
    //   0:root(Container) > 1:div(rich_text_block) > 2:TextNode "hello "（直接 inline 子）
    //                                              > 3:span(TextElement, id="x") > 4:TextNode "world"
    let mk = |kind: NodeKind| Node {
        kind,
        ..Default::default()
    };
    let mut root = mk(NodeKind::Container);
    root.style.taffy_style.size.width = taffy::style::Dimension::length(200.0);
    let mut div = mk(NodeKind::Container);
    div.style.taffy_style.size.width = taffy::style::Dimension::length(100.0);
    let mut scene = Scene::from_nodes(
        vec![
            root,
            div,
            mk(NodeKind::TextNode),
            mk(NodeKind::TextElement),
            mk(NodeKind::TextNode),
        ],
        vec![(0, 1), (1, 2), (1, 3), (3, 4)],
    );
    let root = scene.roots[0];
    let div = scene.get(root).unwrap().children[0]; // node 1
    let outer_tn = scene.get(div).unwrap().children[0]; // node 2
    let span = scene.get(div).unwrap().children[1]; // node 3（TextElement）
    let inner_tn = *scene.get(span).unwrap().children.first().unwrap(); // node 4
    scene.text_contents.insert(outer_tn, "hello ".into());
    scene.text_contents.insert(inner_tn, "world".into());
    scene.get_mut(span).unwrap().id_attr = Some("x".into());
    scene.get_mut(div).unwrap().rich_text_block = true;

    // solve：rich_text_block → build 不递归 inline 子进 taffy → 它们 layout_rect 保持 0。
    let image_sizes: crate::layout::ImageSizeTable = std::collections::HashMap::new();
    crate::layout::solve(&mut scene, &fonts, (200.0, 1000.0), &image_sizes);

    // 折叠证据：span 及其内外 TextNode layout_rect 塌成 0（不在 taffy 树里 = 无独立几何）。
    let span_rect = scene.get(span).unwrap().layout_rect;
    assert!(
        span_rect.w.abs() < 0.1 && span_rect.h.abs() < 0.1,
        "folded inline span 应无独立 layout_rect，got {:?}",
        span_rect
    );
    let outer_tn_rect = scene.get(outer_tn).unwrap().layout_rect;
    assert!(
        outer_tn_rect.w.abs() < 0.1 && outer_tn_rect.h.abs() < 0.1,
        "folded inline TextNode 应无独立 layout_rect，got {:?}",
        outer_tn_rect
    );

    // 不变式：尽管被折出 taffy/render，inline span 仍在 Scene 树里 → find 仍命中。
    assert_eq!(
        scene.find_node_by_id_in_subtree(div, "x"),
        Some(span),
        "rich-text-block 的 inline span 须留在 Scene 树供 Get<T>(\"x\") 查找（public 树 ≠ internal 树）"
    );
}

#[test]
fn node_id_index_and_gen_decode() {
    // 位型：index = bits[31:0]（低 32 位），gen = bits[55:32]（u32 时代 idx 高 20/gen 低 12）。
    let id = NodeId((7u64 << 32) | 5);
    assert_eq!(id.index(), 5, "index = bits[31:0]");
    assert_eq!(id.gen(), 7, "gen = bits[55:32]");
}

#[test]
fn node_id_invalid_sentinel() {
    assert!(
        !NodeId::INVALID.is_valid(),
        "u64::MAX = INVALID（tag=0xFF/idx=全1/gen=全1）"
    );
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
        Option<String>,
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
            None,
            None,
        ),
    ];
    let mut scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    assert!(
        scene.nodes.get(root_id.to_key()).is_some(),
        "live NodeId 可 get（经 to_key）"
    );
    assert!(scene.get(root_id).is_some(), "Scene::get 桥接可用");
    if let Some(n) = scene.get_mut(root_id) {
        n.interaction.flags.insert(NodeFlags::DISABLED);
    }
    assert!(scene
        .get(root_id)
        .unwrap()
        .interaction
        .flags
        .contains(NodeFlags::DISABLED));
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
        Option<String>,
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
        None,
        None,
    )];
    let scene = Scene::build(&entries);
    let id = scene.roots[0];
    assert!(
        scene.nodes.get(id.to_key()).is_some(),
        "to_key 重构的 key 能查到节点"
    );
    // index = slotmap idx = 1（slotmap free_head 从 1 起，idx 0 是 sentinel）
    assert_eq!(id.index(), 1, "首节点 slotmap idx=1");
    assert_eq!(id.gen(), 1, "version=1（无删除）");
}

#[test]
fn node_id_index_capacity_32bit() {
    // index 全宽 32 bit（bits[31:0]）。u32 时代 idx 曾挤高 20 bit（上限 1048575），
    // u64 拓宽后与 slotmap idx 同宽，该上限不复存在。
    let max_idx = u32::MAX as u64;
    let id = NodeId(max_idx);
    assert_eq!(id.index(), max_idx as usize);
}

#[test]
fn node_has_runtime_state_fields_default() {
    let n = Node::default();
    assert!(n.interaction.touchable, "touchable 默认 true");
    assert!(!n.interaction.flags.contains(NodeFlags::HOVERED));
    assert!(!n.interaction.flags.contains(NodeFlags::ACTIVE));
    assert!(!n.interaction.flags.contains(NodeFlags::DISABLED));
    assert!(n.classes.is_empty());
    assert!(n.id_attr.is_none());
    // base_style 与 style 初始相同（Default）
    assert_eq!(n.base_style, n.style);
}

#[test]
fn node_has_draggable_field_default_false() {
    let n = Node::default();
    assert!(!n.interaction.draggable, "draggable 默认 false");
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
        Option<String>,
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
            None,
            None,
        ),
    ];
    let scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let btn_id = scene.get(root_id).unwrap().children[0];
    assert!(
        !scene.get(root_id).unwrap().interaction.draggable,
        "root draggable=false"
    );
    assert!(
        scene.get(btn_id).unwrap().interaction.draggable,
        "btn draggable=true"
    );
}

#[test]
fn scene_default_has_empty_dynamic_rules() {
    let s = Scene::default();
    assert!(
        s.dynamic_rules.entries.is_empty(),
        "Scene 默认 dynamic_rules 空"
    );
}

#[test]
fn node_has_tabindex_focused_defaults() {
    let n = Node::default();
    assert_eq!(
        n.interaction.tabindex, None,
        "tabindex 默认 None（不可聚焦）"
    );
    assert!(
        !n.interaction.flags.contains(NodeFlags::FOCUSED),
        "focused 默认 false"
    );
}

#[test]
fn scene_default_focused_node_none() {
    let s = Scene::default();
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
        Option<String>,
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
            Some(3),
            None,
            None,
            None,
        ),
    ];
    let scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let kids = &scene.get(root_id).unwrap().children;
    let btn1 = kids[0];
    let btn2 = kids[1];
    assert_eq!(
        scene.get(root_id).unwrap().interaction.tabindex,
        None,
        "root tabindex=None"
    );
    assert_eq!(
        scene.get(btn1).unwrap().interaction.tabindex,
        Some(0),
        "btn1 tabindex=Some(0)"
    );
    assert_eq!(
        scene.get(btn2).unwrap().interaction.tabindex,
        Some(3),
        "btn2 tabindex=Some(3)"
    );
    assert!(
        !scene
            .get(root_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::FOCUSED),
        "focused 默认 false"
    );
    assert_eq!(scene.focused_node, None, "build 后 focused_node=None");
}

#[test]
fn scene_build_constructs_tree_without_parse() {
    // 手搓 entries：root Container + 一个 Text 子（parent=Some(0)）。
    // 手搓 scene，证明 Scene::build 独立于打包期解析（read_package 依赖此）。
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
        Option<String>,
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
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::TextNode,
            text_style,
            Vec::new(),
            None,
            false,
            None,
            None,
            Some("hi".into()),
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
    assert!(matches!(text.kind, NodeKind::TextNode));
    assert_eq!(text.parent, Some(root_id));
    assert!(text.dirty_text, "Text 节点 dirty_text=true");
    assert_eq!(
        scene.text_contents.get(&text_id).map(|s| s.as_str()),
        Some("hi"),
        "text content via side table"
    );

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
    assert!(t.get(id).is_none(), "未 ensure → get None");
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

/// NodeKind 所有变体的 bincode 序列化往返稳定（pkg.bin 跨版本兼容性门）。
#[test]
fn node_kind_all_variants_bincode_roundtrip() {
    let all = [
        NodeKind::Container,
        NodeKind::TextNode,
        NodeKind::TextElement,
        NodeKind::Button,
        NodeKind::Image,
        NodeKind::TextField,
        NodeKind::NumberField,
        NodeKind::Slider,
        NodeKind::Toggle,
        NodeKind::RadioButton,
        NodeKind::TextArea,
        NodeKind::Dropdown,
        NodeKind::OptionItem,
        NodeKind::ProgressBar,
        NodeKind::ListView,
        NodeKind::ListItem,
        NodeKind::Slot,
        NodeKind::CustomElement,
    ];
    for k in all {
        let bytes = bincode::serialize(&k).unwrap();
        let back: NodeKind = bincode::deserialize(&bytes).unwrap();
        assert_eq!(k, back, "roundtrip failed for {:?}", k);
    }
}

/// Unit-variant enum bincode 序列化为 4 字节（bincode 默认 FixintEncoding：u32 判别值）。
/// pkg.bin 实际不走 bincode（手动编码），这里只验证 serde derive 的稳定性。
#[test]
fn node_kind_unit_variant_is_one_byte() {
    assert_eq!(bincode::serialize(&NodeKind::Container).unwrap().len(), 4);
}

/// Node.inline_override / inline_set 便签层基础设施。
/// 默认 Node 无 inline override——inline_set 全 0、inline_override 全默认。
#[test]
fn node_inline_override_defaults_empty() {
    let n = Node::default();
    assert_eq!(
        n.inline_set.0, 0,
        "inline_set 默认空（无任何 inline override）"
    );
    // inline_override 全默认 = 与 ResolvedStyle::default() 等价（无字段被显式设值）。
    assert_eq!(n.inline_override, ResolvedStyle::default());
}

/// instantiate 把打包期 `ControlInit` 映射填进运行时 `Scene.controls` side table。
/// ProgressBar：value/max/indeterminate 原样透传。
#[test]
fn instantiate_fills_control_state_progress_from_init() {
    let mut scene = Scene::default();
    let id = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::ProgressBar,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::Progress {
            value: 70.0,
            max: 100.0,
            indeterminate: false,
        }),
    );
    let state = scene.controls.get(id).expect("control state filled");
    assert!(
        matches!(
            state,
            ControlState::Progress {
                value: 70.0,
                max: 100.0,
                indeterminate: false
            }
        ),
        "Progress 字段原样透传"
    );
}

/// Toggle：checked 原样透传。
#[test]
fn instantiate_fills_control_state_toggle_from_init() {
    let mut scene = Scene::default();
    let id = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::Toggle,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::Toggle { checked: true }),
    );
    let state = scene.controls.get(id).expect("control state filled");
    assert!(
        matches!(state, ControlState::Toggle { checked: true }),
        "Toggle.checked 透传"
    );
}

/// Radio：checked + name 原样透传。
#[test]
fn instantiate_fills_control_state_radio_from_init() {
    let mut scene = Scene::default();
    let id = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::RadioButton,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::Radio {
            checked: true,
            name: "group-a".into(),
        }),
    );
    let state = scene.controls.get(id).expect("control state filled");
    assert!(
        matches!(state, ControlState::Radio { checked: true, .. }),
        "Radio.checked 透传"
    );
    if let ControlState::Radio { name, .. } = state {
        assert_eq!(name, "group-a", "Radio.name 透传");
    }
}

/// Slider：value/min/max/step 透传，且 `dragging` 初始为 false（运行时独有，
/// 不进 pkg，故 `ControlInit` 无此字段——instantiate 必须补默认 false）。
#[test]
fn instantiate_fills_control_state_slider_dragging_false() {
    let mut scene = Scene::default();
    let id = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::Slider,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::Slider {
            value: 50.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
        }),
    );
    let state = scene.controls.get(id).expect("control state filled");
    assert!(
        matches!(
            state,
            ControlState::Slider {
                value: 50.0,
                min: 0.0,
                max: 100.0,
                step: 1.0,
                dragging: false
            }
        ),
        "Slider 字段透传 + dragging 初始 false（运行时独有，ControlInit 无此字段）"
    );
}

/// 非控件节点（control_init=None）不建 controls 槽——get 返 None，渲染/交互按无控件处理。
#[test]
fn instantiate_no_control_state_for_non_control_node() {
    let mut scene = Scene::default();
    let id = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::Container,
        ResolvedStyle::default(),
        None,
    );
    assert!(
        scene.controls.get(id).is_none(),
        "非控件节点不应有 controls 槽"
    );
}

#[test]
fn control_state_dropdown_variant() {
    let s = ControlState::Dropdown {
        selected_index: 2,
        open: false,
        value_lock: false,
        open_selected_index: None,
        option_values: Vec::new(),
    };
    assert!(matches!(
        s,
        ControlState::Dropdown {
            selected_index: 2,
            open: false,
            ..
        }
    ));
}

#[test]
fn control_state_number_field_variant() {
    let edit = EditState::from_init("3.14".into(), String::new(), 0, false);
    let s = ControlState::NumberField {
        edit,
        min: 0.0,
        max: 100.0,
        step: 1.0,
    };
    assert!(matches!(
        s,
        ControlState::NumberField {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            ..
        }
    ));
}

/// TextField：value/placeholder/max_length/readonly 透传进 EditState，cursor/anchor
/// 初始化为 value.len()（光标在文本末尾），composition 初始 None，视觉标记为可见。
#[test]
fn instantiate_fills_textfield_edit_state_from_init() {
    let mut scene = Scene::default();
    let id = crate::scene::dynamic::create_node_from_template(
        &mut scene,
        NodeKind::TextField,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::TextField(
            crate::asset::EditInit {
                value: "hi".into(),
                placeholder: "p".into(),
                max_length: 10,
                readonly: false,
            },
        )),
    );
    let state = scene.controls.get(id).expect("control state filled");
    match state {
        ControlState::TextField(e) => {
            assert_eq!(e.value, "hi", "value 原样透传");
            assert_eq!(e.cursor, 2, "cursor 初始为 value.len()（光标在末尾）");
            assert_eq!(e.anchor, 2, "anchor 初始同 cursor（无选区）");
            assert_eq!(e.max_length, 10);
            assert!(!e.readonly);
            assert!(e.cursor_visible, "cursor 初始可见");
            assert_eq!(e.cursor_timer, 0.0);
            assert_eq!(e.ideal_cursor_x, 0.0);
            assert!(e.composition.is_none(), "composition 初始 None");
        }
        other => panic!("expected TextField, got {:?}", other),
    }
}

#[test]
fn paint_order_children_sorts_by_z_stable() {
    // root 三子 a(z=0) b(z=2) c(z=1) → 绘制序 [a, c, b]（z 升序，b 最后画 = 顶层）。
    // z 全 0 时稳定排序退化为 children 原序（DOM 序），与历史行为逐位一致。
    let root = Node::default();
    let mut a = Node::default();
    a.style.z_index = 0;
    let mut b = Node::default();
    b.style.z_index = 2;
    let mut c = Node::default();
    c.style.z_index = 1;
    let scene = Scene::from_nodes(vec![root, a, b, c], vec![(0, 1), (0, 2), (0, 3)]);
    let root_id = scene.roots[0];
    let ids: Vec<NodeId> = scene.get(root_id).unwrap().children.clone();
    assert_eq!(
        paint_order_children(&scene, root_id),
        vec![ids[0], ids[2], ids[1]],
        "z 升序稳定排：a(0) → c(1) → b(2)"
    );
    // 负 z 排最前（最先画 = 最底）
    let scene2 = Scene::from_nodes(
        {
            let mut a = Node::default();
            a.style.z_index = -5;
            vec![Node::default(), a, Node::default()]
        },
        vec![(0, 1), (0, 2)],
    );
    let root2 = scene2.roots[0];
    let kids2: Vec<NodeId> = scene2.get(root2).unwrap().children.clone();
    assert_eq!(
        paint_order_children(&scene2, root2),
        vec![kids2[0], kids2[1]],
        "负 z 先画"
    );
    // 不存在的节点 → 空（防御）
    assert!(paint_order_children(&scene, NodeId(0xFFFF_FFFF)).is_empty());
}

#[test]
#[should_panic(expected = "NodeId generation overflow")]
fn node_id_gen_overflow_panics_instead_of_aliasing() {
    // 烧穿 12-bit generation：同槽 insert/remove 循环到版本回卷点，from_key 必须
    // 显式 panic（静默回卷 = 幽灵死节点：id 字段与槽位真实版本不符，get 永久 miss，
    // rematch 等全量遍历每帧炸「live node」）。
    let mut scene = Scene::default();
    loop {
        let key = scene.nodes.insert(Node::default());
        let _ = NodeId::from_key(key); // 超限时 panic
        scene.nodes.remove(key);
    }
}
