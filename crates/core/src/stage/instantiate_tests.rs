use super::*;
use crate::asset::{PackageInput, TemplateNode};
use crate::scene::NodeKind;
use crate::style::dynamic::{
    Combinator, Compound, Declaration, DynamicRule, ParsedSelector, Specificity,
};
use crate::style::resolved::ResolvedStyle;
use taffy::style::FlexDirection;

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
            content: None,
            src: None,
            control_init: None,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            control_init: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules)],
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
        content: None,
        src: None,
        control_init: None,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c1", &nodes, &rules)],
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
            content: None,
            src: None,
            control_init: None,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(2), // 越界前向引用（只有 2 节点，index 0/1）
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            control_init: None,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c1", &nodes, &rules)],
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

// ── dynamic_rules 作用域隔离测试（Shadow DOM 风格，main-design §5.4）──
//
// 背景（坑）：旧实现把模板规则 push 进全局 scene.dynamic_rules 且不清理、去重只比 selector。
// 导致 home 的 `.root{column}` 残留污染后实例化的 settings 的 `.root{row}`——
// 两个组件 selector 相同（都 `.root`）但 declaration 不同，被错误去重丢弃，
// settings 的 .root 被 rematch 成 column（上下布局）。
//
// 修复后：规则带 scope_root（实例根），rematch 按 scope 过滤 + 后代选择器不穿透边界。
// 切页（remove + instantiate）不再跨组件污染。

/// 测试用 selector 构造器：支持单个 `.class` / `tag` / `tag.class`。
fn single_selector(raw: &str) -> ParsedSelector {
    let mut c = Compound {
        tag: None,
        classes: Vec::new(),
        id: None,
        combinator: Combinator::Descendant,
        pseudo_hover: false,
        pseudo_active: false,
        pseudo_disabled: false,
        pseudo_focus: false,
        attrs: Vec::new(),
    };
    let mut rest = raw;
    // tag（开头非 . # 的字母段）
    if let Some(end) = rest.find(['.', '#']) {
        if end > 0 {
            c.tag = Some(rest[..end].to_string());
            rest = &rest[end..];
        }
    } else if !rest.is_empty() && rest.chars().next().unwrap().is_alphabetic() {
        c.tag = Some(rest.to_string());
        rest = "";
    }
    while !rest.is_empty() {
        if rest.starts_with('.') {
            let r = &rest[1..];
            let end = r
                .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                .unwrap_or(r.len());
            c.classes.push(r[..end].to_string());
            rest = &r[end..];
        } else {
            rest = &rest[1..];
        }
    }
    let class_count = c.classes.len() as u32 + c.id.is_some() as u32;
    let tag_count = c.tag.is_some() as u32;
    ParsedSelector {
        raw: raw.to_string(),
        compound: vec![c],
        specificity: Specificity(class_count, 0, tag_count),
    }
}

fn class_rule(class: &str, prop: &str, val: &str) -> DynamicRule {
    DynamicRule {
        selector: single_selector(&format!(".{class}")),
        declarations: vec![Declaration {
            prop: prop.to_string(),
            value: val.to_string(),
        }],
    }
}

/// 建一个单根组件 pkg：根 Container 带 class `root`，附一条 `.root` 规则。
fn pkg_with_root_rule(pkg_name: &str, flex_dir_val: &str) -> (String, Vec<u8>) {
    let mut root_style = ResolvedStyle::default();
    crate::scene::dynamic::apply_css(&mut root_style, "display:flex");
    let nodes = [TemplateNode {
        kind: NodeKind::Container,
        style: root_style,
        parent_idx: None,
        classes: vec!["root".to_string()],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        control_init: None,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable {
        rules: vec![class_rule("root", "flex-direction", flex_dir_val)],
    };
    let input = PackageInput {
        components: vec![(pkg_name, &nodes, &rules)],
    };
    (pkg_name.to_string(), crate::asset::write_package(&input))
}

#[test]
fn dynamic_rules_scoped_switch_page_no_leak() {
    // 复现总坑：home 的 `.root{column}` 不应泄漏到后实例化的 settings 的 `.root{row}`。
    // 两个组件 .root selector 相同但 declaration 不同——必须各自隔离。
    let (_, home_pkg) = pkg_with_root_rule("home", "column");
    let (_, settings_pkg) = pkg_with_root_rule("settings", "row");
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((400.0, 400.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_package("home", &home_pkg).unwrap();
    s.load_package("settings", &settings_pkg).unwrap();
    let doc = s.create_root("div", "").unwrap();

    // 1. 实例化 home，跑几帧
    let home = s.instantiate("home", "home").unwrap();
    crate::scene::dynamic::append_child(s.scene.as_mut().unwrap(), doc, home).unwrap();
    s.tick_and_render();
    assert_eq!(
        s.scene
            .as_ref()
            .unwrap()
            .get(home)
            .unwrap()
            .style
            .taffy_style
            .flex_direction,
        FlexDirection::Column,
        "home .root 应是 column"
    );

    // 2. 切页：删 home，实例化 settings
    s.remove_node(home);
    let settings = s.instantiate("settings", "settings").unwrap();
    crate::scene::dynamic::append_child(s.scene.as_mut().unwrap(), doc, settings).unwrap();
    s.tick_and_render();

    // 3. settings 的 .root 必须是 row（不被 home 的 column 污染）
    assert_eq!(
        s.scene
            .as_ref()
            .unwrap()
            .get(settings)
            .unwrap()
            .style
            .taffy_style
            .flex_direction,
        FlexDirection::Row,
        "settings .root 应是 row（home 的 column 规则不应跨页泄漏）"
    );
}

#[test]
fn dynamic_rules_descendant_selector_not_cross_scope() {
    // 父组件的 `.a .b`（后代选择器）不应穿透子组件边界命中子组件内的 .b。
    // 父 `.outer{...}` 包一个子组件实例，子组件内有 `.inner`。
    // 规则 `.outer .inner`（scope=父）不应命中子组件内的 .inner（scope=子根）。
    // 这里用两套规则验证：父的 `.outer .leaf` 后代规则只命中父自己的 .leaf，不命中子的 .leaf。
    let mut outer_style = ResolvedStyle::default();
    crate::scene::dynamic::apply_css(&mut outer_style, "display:flex");
    let outer_nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: outer_style.clone(),
            parent_idx: None,
            classes: vec!["outer".to_string()],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            control_init: None,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec!["leaf".to_string()],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            control_init: None,
        },
    ];
    let outer_rules = crate::style::dynamic::DynamicRuleTable {
        rules: vec![DynamicRule {
            // 后代选择器 `.outer .leaf`：两 compound
            selector: ParsedSelector {
                raw: ".outer .leaf".to_string(),
                compound: vec![
                    {
                        let mut c = single_selector(".outer").compound.remove(0);
                        c.combinator = Combinator::Descendant;
                        c
                    },
                    single_selector(".leaf").compound.remove(0),
                ],
                specificity: Specificity(2, 0, 0),
            },
            declarations: vec![Declaration {
                prop: "background-color".to_string(),
                value: "#ff0000".to_string(),
            }],
        }],
    };
    // 子组件：只有 `.leaf`（不含 .outer）
    let inner_nodes = [TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec!["leaf".to_string()],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        control_init: None,
    }];
    let inner_rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![
            ("outer", &outer_nodes, &outer_rules),
            ("inner", &inner_nodes, &inner_rules),
        ],
    };
    let pkg = crate::asset::write_package(&input);

    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((400.0, 400.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_package("p", &pkg).unwrap();
    let doc = s.create_root("div", "").unwrap();

    // 父实例化 → 子嵌套进父
    let outer = s.instantiate("p", "outer").unwrap();
    crate::scene::dynamic::append_child(s.scene.as_mut().unwrap(), doc, outer).unwrap();
    let inner = s.instantiate("p", "inner").unwrap();
    // 把子挂到父的 .leaf 节点下（模拟嵌套组件）
    let outer_leaf = s.scene.as_ref().unwrap().get(outer).unwrap().children[0];
    crate::scene::dynamic::append_child(s.scene.as_mut().unwrap(), outer_leaf, inner).unwrap();
    s.tick_and_render();

    // 父的 .leaf（outer_leaf）应命中 `.outer .leaf`（红）；子的 .leaf（inner 根）不应命中（透明）
    assert_eq!(
        s.scene
            .as_ref()
            .unwrap()
            .get(outer_leaf)
            .unwrap()
            .style
            .background_color,
        Some([1.0, 0.0, 0.0, 1.0]),
        "父的 .leaf 在父作用域内 → 命中后代规则（红）"
    );
    assert_eq!(
        s.scene
            .as_ref()
            .unwrap()
            .get(inner)
            .unwrap()
            .style
            .background_color,
        None,
        "子组件的 .leaf 被作用域边界挡住 → 不命中父的后代规则（无背景）"
    );
}
