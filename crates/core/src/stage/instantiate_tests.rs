use super::*;
use crate::asset::{ControllerEntry, PackageInput, TemplateNode};
use crate::scene::NodeKind;
use crate::style::resolved::ResolvedStyle;

/// 辅助：建带子树的 pkg（comp1 = root(Container) + child(Container)）。
fn make_test_pkg_with_subtree() -> Vec<u8> {
    let mut root_style = ResolvedStyle::default();
    // 给 root 显式尺寸，便于后续断言可扩展（此处仅验结构）
    crate::scene::dynamic::apply_css(&mut root_style, "width:100px;height:100px");
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: root_style,
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
            content: None,
            src: None,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
            content: None,
            src: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &[])],
    };
    crate::asset::write_package(&input)
}

#[test]
fn instantiate_clones_subtree_returns_orphan_root() {
    let mut s = Stage::new_for_test();
    s.create_root("div", "width:100px;height:100px").unwrap();
    s.load_package("bag", &make_test_pkg_with_subtree())
        .unwrap();
    let root = s.instantiate("bag", "comp1").unwrap();
    let scene = s.scene.as_ref().unwrap();
    // 组件根 parent = None（孤立）
    assert!(scene.get(root).unwrap().parent.is_none(), "孤立根");
    // comp1 含 root + child → 子树串好（root.children 含 child）
    assert_eq!(scene.get(root).unwrap().children.len(), 1, "root 有 1 子");
    let child = scene.get(root).unwrap().children[0];
    assert_eq!(
        scene.get(child).unwrap().parent,
        Some(root),
        "child.parent=root"
    );
    // scene 节点数 = create_root 的 1 + 组件的 2 = 3
    assert_eq!(scene.nodes.len(), 3, "scene 多了组件的 2 节点");
}

#[test]
fn instantiate_multi_instance_independent() {
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &make_test_pkg_with_subtree())
        .unwrap();
    let i1 = s.instantiate("bag", "comp1").unwrap();
    let i2 = s.instantiate("bag", "comp1").unwrap();
    assert_ne!(i1, i2, "两实例不同 NodeId");
    // 两实例都孤立，各自独立子树
    let scene = s.scene.as_ref().unwrap();
    assert!(scene.get(i1).unwrap().parent.is_none(), "i1 孤立");
    assert!(scene.get(i2).unwrap().parent.is_none(), "i2 孤立");
    // 各自的 child 不同（独立子树，不串）
    let c1 = scene.get(i1).unwrap().children[0];
    let c2 = scene.get(i2).unwrap().children[0];
    assert_ne!(c1, c2, "两实例的 child 不同");
    assert_eq!(scene.get(c1).unwrap().parent, Some(i1), "c1.parent=i1");
    assert_eq!(scene.get(c2).unwrap().parent, Some(i2), "c2.parent=i2");
}

#[test]
fn instantiate_missing_pkg_or_comp_errors() {
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    // 用 load_package_tests 的 make_test_pkg（单组件 c1）——这里内联一个最小 pkg
    let nodes = [TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        data_controller: None,
        content: None,
        src: None,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c1", &nodes, &rules, &[])],
    };
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    assert!(s.instantiate("nope", "c1").is_err(), "包不存在");
    assert!(s.instantiate("bag", "nope").is_err(), "组件不存在");
}

#[test]
fn instantiate_corrupt_parent_idx_returns_err_not_panic() {
    // 坑102 no-panic 契约：FFI 可达的 instantiate 不能因 corrupt pkg panic。
    // parent_idx 越界前向引用（child 引用不存在的 node 2）违反"parent_idx < i 且 < len"不变量——
    // 当前实现 `id_map[pidx]`（pidx 越界）会 index-out-of-bounds panic，必须改成返 Err。
    // node[0]=root（write_package 的 debug_assert 只查 node[0]，node[1] 的 corrupt parent_idx 透传）。
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
            content: None,
            src: None,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(2), // 越界前向引用（只有 2 节点，index 0/1）
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
            content: None,
            src: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c1", &nodes, &rules, &[])],
    };
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let result = s.instantiate("bag", "c1");
    assert!(
        result.is_err(),
        "corrupt parent_idx（前向引用）应返 Err 不能 panic，实际: {result:?}"
    );
}

#[test]
fn instantiate_without_scene_errors() {
    // scene 必须已存在（create_root 建过），否则 Err
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 200.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    // 不调 create_root，scene = None
    s.load_package("bag", &make_test_pkg_with_subtree())
        .unwrap();
    assert!(s.instantiate("bag", "comp1").is_err(), "无 scene → Err");
}

/// instantiate 建 Controller registry：组件带 ControllerEntry（mount_node_idx=0 = 组件根，
/// initial_selected_index=2），instantiate 后 scene.controllers 含 (根 NodeId → selected=2)。
/// mount_node_idx 经 id_map 重映射成活 NodeId（非模板下标 0，而是 slotmap 分配的真实 NodeId）。
#[test]
fn instantiate_builds_controller_registry() {
   let mut root = TemplateNode {
       kind: NodeKind::Container,
       style: ResolvedStyle::default(),
       parent_idx: None,
       classes: vec![],
       id_attr: None,
       draggable: false,
       tabindex: None,
       data_controller: Some("tab".into()),
        content: None,
        src: None,
   };
    let mut child = TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: Some(0),
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        data_controller: None,
        content: None,
        src: None,
    };
    let _ = &mut root; // 借用占位
    let _ = &mut child;
    let nodes = [root, child];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let controllers = vec![ControllerEntry {
        name: "tab".into(),
        mount_node_idx: 0,
        initial_selected_index: 2,
    }];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &controllers)],
    };
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let root_id = s.instantiate("bag", "comp1").unwrap();

    // registry 含 mount_node_idx=0 重映射后的 NodeId（= 组件根 root_id），selected=2。
    let scene = s.scene.as_ref().unwrap();
    assert_eq!(
        scene.controller_selected(root_id),
        Some(2),
        "instantiate 建 registry：根 NodeId → selected=2"
    );
}

/// 多实例 Controller registry 独立：同组件 instantiate 两次 → 两实例各自的根 NodeId
/// 在 registry 中有独立条目（不同 NodeId → 不覆盖），改一个不影响另一个。
#[test]
fn instantiate_multi_instance_controller_registry_independent() {
   let mut root = TemplateNode {
       kind: NodeKind::Container,
       style: ResolvedStyle::default(),
       parent_idx: None,
       classes: vec![],
       id_attr: None,
       draggable: false,
       tabindex: None,
       data_controller: Some("tab".into()),
        content: None,
        src: None,
   };
   let _ = &mut root;
    let nodes = [root];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let controllers = vec![ControllerEntry {
        name: "tab".into(),
        mount_node_idx: 0,
        initial_selected_index: 1,
    }];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &controllers)],
    };
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let i1 = s.instantiate("bag", "comp1").unwrap();
    let i2 = s.instantiate("bag", "comp1").unwrap();
    assert_ne!(i1, i2, "两实例不同 NodeId");

    // 两实例都有独立 registry 条目，初始 selected=1（来自 ControllerEntry）
    {
        let scene = s.scene.as_ref().unwrap();
        assert_eq!(
            scene.controller_selected(i1),
            Some(1),
            "i1 registry selected=1"
        );
        assert_eq!(
            scene.controller_selected(i2),
            Some(1),
            "i2 registry selected=1"
        );
    }

    // 改 i1 不影响 i2（独立条目，不同 NodeId key）
    s.set_selected_index(i1, 3);
    let scene = s.scene.as_ref().unwrap();
    assert_eq!(scene.controller_selected(i1), Some(3), "i1 改后 selected=3");
    assert_eq!(
        scene.controller_selected(i2),
        Some(1),
        "i2 不受影响（独立 registry 条目）"
    );
}

/// instantiate ControllerEntry 的 mount_node_idx 指向非根节点：mount 在子节点上，
/// id_map 重映射后 registry key = 子节点的活 NodeId（非组件根）。
#[test]
fn instantiate_controller_mount_on_child_node() {
    let root = TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        data_controller: None,
        content: None,
        src: None,
    };
   let child = TemplateNode {
       kind: NodeKind::Container,
       style: ResolvedStyle::default(),
       parent_idx: Some(0),
       classes: vec![],
       id_attr: None,
       draggable: false,
       tabindex: None,
       data_controller: Some("panel".into()),
        content: None,
        src: None,
   };
    let nodes = [root, child];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    // mount_node_idx=1 = 子节点（组件内局部下标）
    let controllers = vec![ControllerEntry {
        name: "panel".into(),
        mount_node_idx: 1,
        initial_selected_index: 0,
    }];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &controllers)],
    };
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let root_id = s.instantiate("bag", "comp1").unwrap();

    // 子节点 = root 的首个子
    let scene = s.scene.as_ref().unwrap();
    let child_id = scene.get(root_id).unwrap().children[0];
    // registry key = child_id（非 root_id），selected=0
    assert_eq!(
        scene.controller_selected(child_id),
        Some(0),
        "mount 在子节点 → registry key=child_id"
    );
    assert_eq!(
        scene.controller_selected(root_id),
        None,
        "根节点无 controller → registry 无条目"
    );
}
