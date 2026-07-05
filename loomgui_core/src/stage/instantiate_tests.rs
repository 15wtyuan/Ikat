use super::*;
use crate::asset::{PackageInput, TemplateNode};
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
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules)],
        asset_manifest: &[],
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
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c1", &nodes, &rules)],
        asset_manifest: &[],
    };
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    assert!(s.instantiate("nope", "c1").is_err(), "包不存在");
    assert!(s.instantiate("bag", "nope").is_err(), "组件不存在");
}

#[test]
fn instantiate_without_scene_errors() {
    // scene 必须已存在（create_root 建过），否则 Err
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new(font_path, (200.0, 200.0)).unwrap();
    // 不调 create_root，scene = None
    s.load_package("bag", &make_test_pkg_with_subtree())
        .unwrap();
    assert!(s.instantiate("bag", "comp1").is_err(), "无 scene → Err");
}

/// pkg 的 comp1 带 :hover 规则；instantiate 两次 → scene.dynamic_rules 只多一份（去重）。
/// parse-gated：需 parse_selector 构真实选择器（runtime 无字符串解析输入）。
#[cfg(feature = "parse")]
#[test]
fn instantiate_merges_dynamic_rules_dedup() {
    use crate::parse::css::Declaration;
    use crate::parse::selector::parse_selector;
    use crate::style::dynamic::{DynamicRule, DynamicRuleTable};
    let rules = DynamicRuleTable {
        rules: vec![DynamicRule {
            selector: parse_selector(".btn:hover").unwrap(),
            declarations: vec![Declaration {
                prop: "background-color".into(),
                value: "#0000ff".into(),
            }],
        }],
    };
    let nodes = [TemplateNode {
        kind: NodeKind::Button,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec!["btn".into()],
        id_attr: None,
        draggable: false,
        tabindex: None,
    }];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules)],
        asset_manifest: &[],
    };
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let before = s.scene.as_ref().unwrap().dynamic_rules.rules.len();
    s.instantiate("bag", "comp1").unwrap();
    s.instantiate("bag", "comp1").unwrap();
    let after = s.scene.as_ref().unwrap().dynamic_rules.rules.len();
    assert_eq!(after - before, 1, "同选择器规则去重，只加一份");
}

/// 多实例 :hover 独立性：同组件 instantiate 两次 → 仅 hover 实例1
/// 的按钮变蓝，实例2 按钮保持原色（无 rematch 串状态回归）。
///
/// 设计契约（spec §4.4）：dynamic_rules 按 class 匹配、多实例共享；hit_test 返具体 NodeId →
/// 各实例独立 :hover。此前 `instantiate_multi_instance_independent` 只验结构独立，:hover 行为
/// 独立是设计推断——本测试把它钉成受测事实。
///
/// 布局：scene_root(400x200) → 块流挂两实例（comp root=div 100x50，含 button.btn 100x50）。
/// 实例1 的 btn 几何在 (0,0)-(100,50)，实例2 的 btn 在 (0,50)-(100,100)。Move 到 (50,25)
/// 只命中 btn1。layout solve 仅 roots[0] 下沉——两实例必须挂同一 scene_root 子树下才被布局。
#[cfg(feature = "parse")]
#[test]
fn instantiate_multi_instance_hover_independent() {
    use crate::parse::css::Declaration;
    use crate::parse::selector::parse_selector;
    use crate::style::dynamic::{DynamicRule, DynamicRuleTable};
    use crate::transform::Affine2Ext;

    // .btn:hover { background-color: #0000ff } —— 共享规则，按 class + 伪类匹配各实例
    let rules = DynamicRuleTable {
        rules: vec![DynamicRule {
            selector: parse_selector(".btn:hover").unwrap(),
            declarations: vec![Declaration {
                prop: "background-color".into(),
                value: "#0000ff".into(),
            }],
        }],
    };
    // 组件根 = Container(div) 100x50；子 = Button.btn 100x50（base 灰底，hover 蓝）
    let mut btn_style = ResolvedStyle::default();
    crate::scene::dynamic::apply_css(
        &mut btn_style,
        "width:100px;height:50px;background-color:#cccccc",
    );
    let mut root_style = ResolvedStyle::default();
    crate::scene::dynamic::apply_css(&mut root_style, "width:100px;height:50px");
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: root_style,
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
        },
        TemplateNode {
            kind: NodeKind::Button,
            style: btn_style,
            parent_idx: Some(0),
            classes: vec!["btn".into()],
            id_attr: None,
            draggable: false,
            tabindex: None,
        },
    ];
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules)],
        asset_manifest: &[],
    };

    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new(font_path, (400.0, 200.0)).unwrap();
    // scene_root 作唯一布局根（solve 仅 roots[0] 下沉）；两实例 append_child 挂其下，块流纵向堆叠
    let scene_root = s.create_root("div", "width:400px;height:200px").unwrap();
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let i1 = s.instantiate("bag", "comp1").unwrap();
    let i2 = s.instantiate("bag", "comp1").unwrap();
    s.append_child(scene_root, i1).unwrap();
    s.append_child(scene_root, i2).unwrap();

    // 取两实例各自的 btn NodeId（comp root 的首个 Button 子）
    let btn1 = {
        let sc = s.scene.as_ref().unwrap();
        *sc.get(i1)
            .unwrap()
            .children
            .iter()
            .find(|&&c| matches!(sc.get(c).unwrap().kind, NodeKind::Button))
            .unwrap()
    };
    let btn2 = {
        let sc = s.scene.as_ref().unwrap();
        *sc.get(i2)
            .unwrap()
            .children
            .iter()
            .find(|&&c| matches!(sc.get(c).unwrap().kind, NodeKind::Button))
            .unwrap()
    };
    assert_ne!(btn1, btn2, "两实例 btn NodeId 不同");

    // warmup tick：hit_test 读上帧 world_transforms（1 帧延迟语义，首帧空 → 全 None）
    s.tick_and_render();
    // 校验两 btn 几何已分离（btn1 在 y≈0，btn2 在 y≈50，无重叠 → 命中不串）
    {
        let sc = s.scene.as_ref().unwrap();
        let (_x1, y1) = sc.world_transforms[btn1.index()].apply_point(0.0, 0.0);
        let (_x2, y2) = sc.world_transforms[btn2.index()].apply_point(0.0, 0.0);
        assert!(
            (y2 - y1).abs() >= 40.0,
            "两实例 btn 应纵向分离（y1={y1}, y2={y2}），否则 hover 命中会串"
        );
    }

    // hover 前基线：两 btn 都是灰底 #cccccc = [0.8,0.8,0.8,1.0]
    {
        let sc = s.scene.as_ref().unwrap();
        assert_eq!(
            sc.get(btn1).unwrap().style.background_color,
            Some([0.8, 0.8, 0.8, 1.0]),
            "hover 前 btn1 灰底"
        );
        assert_eq!(
            sc.get(btn2).unwrap().style.background_color,
            Some([0.8, 0.8, 0.8, 1.0]),
            "hover 前 btn2 灰底"
        );
    }

    // Move 到 (50,25) —— 落在 btn1 几何 (0,0)-(100,50) 内，btn2 在 y≈50 外
    s.set_input(&[crate::input::PointerEvent {
        kind: crate::input::PointerKind::Move,
        x: 50.0,
        y: 25.0,
        button: 0,
        pad: [0, 0],
        touch_id: -1,
    }]);
    s.tick_and_render();

    // 核心：btn1 变蓝（hover 命中 + rematch），btn2 保持灰（无串状态）
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(
        sc.get(btn1).unwrap().style.background_color,
        Some([0.0, 0.0, 1.0, 1.0]),
        "btn1 被 hover → 变蓝"
    );
    assert_eq!(
        sc.get(btn2).unwrap().style.background_color,
        Some([0.8, 0.8, 0.8, 1.0]),
        "btn2 未被 hover → 保持灰（无 cross-talk）"
    );
    assert_ne!(
        sc.get(btn1).unwrap().style.background_color,
        sc.get(btn2).unwrap().style.background_color,
        "两实例 :hover 状态独立（btn1 蓝 / btn2 灰）"
    );
}
