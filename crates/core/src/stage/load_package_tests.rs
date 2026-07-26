use super::*;
use crate::asset::{PackageInput, TemplateNode};
use crate::scene::NodeKind;
use crate::style::resolved::ResolvedStyle;

/// 辅助：内存建单组件 pkg（组件名 comp_name，单 Container 根）。
/// 走 write_package → bytes，供 load_package 消费。
fn make_test_pkg(_comp_name: &str) -> Vec<u8> {
    let nodes = [TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None, // 组件根
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        control_init: None,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![(_comp_name, &nodes, &rules)],
    };
    crate::asset::write_package(&input)
}

#[test]
fn load_package_into_pool_without_scene() {
    let mut s = Stage::new_for_test(); // scene = Some(空骨架)
    let pkg_bytes = make_test_pkg("comp1");
    s.load_package("bag", &pkg_bytes).unwrap();
    assert!(s.packages.contains_key("bag"), "进资源池");
    assert!(s.scene.is_some(), "scene 不变（load 不建/不清 scene）");
    // scene 仍是空骨架（无 roots）——load_package 没碰 scene
    assert!(
        s.scene.as_ref().unwrap().roots.is_empty(),
        "scene roots 仍空（load 不建 scene）"
    );
}

#[test]
fn load_package_multi_pkg_coexist() {
    let mut s = Stage::new_for_test();
    s.load_package("bag", &make_test_pkg("c1")).unwrap();
    s.load_package("mail", &make_test_pkg("c2")).unwrap();
    assert_eq!(s.packages.len(), 2, "多包共存");
    assert!(s.packages.contains_key("bag"));
    assert!(s.packages.contains_key("mail"));
}

#[test]
fn load_package_replace_same_name() {
    let mut s = Stage::new_for_test();
    s.load_package("bag", &make_test_pkg("c1")).unwrap();
    assert_eq!(s.packages.len(), 1);
    s.load_package("bag", &make_test_pkg("c2")).unwrap();
    assert_eq!(s.packages.len(), 1, "同名替换（不堆积）");
    // 替换后包内组件应是 c2（验证是替换不是 no-op）
    assert!(
        s.packages["bag"].components.contains_key("c2"),
        "替换后是新包（含 c2）"
    );
}

/// load_package 不碰 scene 的不变量：load 前 scene 有内容，load 后 scene 不变。
/// 验证 load_package 不清/不重建 scene（load_package 只进资源池，不建 scene）。
#[test]
fn load_package_does_not_touch_scene() {
    let mut s = Stage::new_for_test();
    // 先建 scene 内容（create_root 建根）
    let root = s.create_root("div", "width:100px;height:100px").unwrap();
    let scene_root_count_before = s.scene.as_ref().unwrap().roots.len();
    assert_eq!(scene_root_count_before, 1);
    // load_package 进资源池
    s.load_package("bag", &make_test_pkg("c1")).unwrap();
    // scene 完全不变（roots 不变、节点数不变）
    let scene = s.scene.as_ref().unwrap();
    assert_eq!(scene.roots.len(), 1, "scene roots 不变");
    assert_eq!(scene.roots[0], root, "scene root NodeId 不变");
    assert_eq!(scene.nodes.len(), 1, "scene 节点数不变");
}
