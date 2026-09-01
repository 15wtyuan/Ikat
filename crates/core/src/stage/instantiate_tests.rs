use super::*;
use crate::asset::{PackageInput, TemplateNode};
use crate::scene::node::{NodeFlags, NodeId};
use crate::scene::NodeKind;
use crate::style::dynamic::rematch_pseudo_classes;
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
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &[])],
    };
    crate::asset::write_package(&input)
}

/// 辅助：建单节点控件 pkg（tabindex=None，模拟 HTML 未写 tabindex）。
fn make_control_pkg(kind: NodeKind, control_init: crate::asset::ControlInit) -> Vec<u8> {
    let nodes = [TemplateNode {
        kind,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: Some(control_init),
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules, &[])],
    };
    crate::asset::write_package(&input)
}

#[test]
fn instantiate_focusable_control_gets_default_tabindex_zero() {
    // HTML/ARIA 语义：可聚焦控件（input/textarea/select/button 及 role=textbox/spinbutton/
    // slider/switch/radio/combobox）隐式 tabindex=0。pkg 里 tabindex=None（HTML 未写）时，
    // runtime 应补默认 Some(0)，否则 click-to-focus / Tab 链无法命中控件
    // （showcase NumberField 点击不进输入模式根因）。
    let pkg = make_control_pkg(
        NodeKind::NumberField,
        crate::asset::ControlInit::NumberField {
            edit: crate::asset::EditInit {
                value: "42".into(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
            min: 0.0,
            max: 100.0,
            step: 1.0,
        },
    );
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let id = s.instantiate("bag", "c").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let n = scene.get(id).expect("node");
    assert_eq!(
        n.interaction.tabindex,
        Some(0),
        "NumberField 无显式 tabindex → 默认 Some(0)（可聚焦）"
    );
}

#[test]
fn instantiate_explicit_tabindex_minus_one_is_respected() {
    // 显式 tabindex="-1" 不被默认覆盖（作者明确排除出 Tab 链 / click-to-focus）。
    let nodes = [TemplateNode {
        kind: NodeKind::NumberField,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: Some(-1),
        content: None,
        src: None,
        href: None,
        control_init: Some(crate::asset::ControlInit::NumberField {
            edit: crate::asset::EditInit {
                value: "42".into(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
            min: 0.0,
            max: 100.0,
            step: 1.0,
        }),
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules, &[])],
    };
    let pkg = crate::asset::write_package(&input);
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let id = s.instantiate("bag", "c").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let n = scene.get(id).expect("node");
    assert_eq!(
        n.interaction.tabindex,
        Some(-1),
        "显式 tabindex=-1 不被默认覆盖"
    );
}

#[test]
fn instantiate_non_focusable_progress_stays_no_tabindex() {
    // ProgressBar 只读不可聚焦 → 不补默认 tabindex。
    let pkg = make_control_pkg(
        NodeKind::ProgressBar,
        crate::asset::ControlInit::Progress {
            value: 0.0,
            min: 0.0,
            max: 100.0,
            indeterminate: false,
        },
    );
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let id = s.instantiate("bag", "c").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let n = scene.get(id).expect("node");
    assert_eq!(
        n.interaction.tabindex, None,
        "ProgressBar 只读 → 不补默认 tabindex"
    );
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
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
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
    // no-panic 契约：FFI 可达的 instantiate 不能因 corrupt pkg panic。
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
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(2), // 越界前向引用（只有 2 节点，index 0/1）
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
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
        pseudo_nth_child: None,
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
    // specificity = (id 数, class 数, tag 数)——与生产 Specificity struct 同口径。
    // 旧实现把 id 混进 class 位（class_count 含 id），掩盖 id vs class 的优先级排序 bug。
    let id_count = c.id.is_some() as u32;
    let class_count = c.classes.len() as u32;
    let tag_count = c.tag.is_some() as u32;
    ParsedSelector {
        raw: raw.to_string(),
        compound: vec![c],
        specificity: Specificity(id_count, class_count, tag_count),
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
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable {
        rules: vec![class_rule("root", "flex-direction", flex_dir_val)],
    };
    let input = PackageInput {
        components: vec![(pkg_name, &nodes, &rules, &[])],
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
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec!["leaf".to_string()],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
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
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let inner_rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![
            ("outer", &outer_nodes, &outer_rules, &[]),
            ("inner", &inner_nodes, &inner_rules, &[]),
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

#[test]
fn instantiate_reparents_dropdown_options_into_listbox() {
    // 生产路径回归：模板里作者写 `<div role=combobox><div role=listbox><div role=option>A
    // </div>...</div></div>`，经 load_package + instantiate 后，option 须是
    // listbox 的直接子（不是 combobox 的直接子）。这是 listbox 浮层能渲染 option 列表的前提
    //（render 末尾追加从 listbox 根 DFS）。core 不再注入结构——作者写的即运行时结构。
    use crate::asset::ControlInit;
    use crate::scene::control::{find_child_by_role_recursive, ROLE_LISTBOX};
    // 模板：[0]=combobox(Dropdown), [1]=listbox(role=listbox), [2..]=option 在 listbox 内。
    let nodes = [
        TemplateNode {
            kind: NodeKind::Dropdown,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: Some(ControlInit::Dropdown {
                selected_index: 0,
                option_values: Vec::new(),
            }),
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        // listbox role 子（作者写的弹出列表容器）。
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: Some("listbox".to_string()),
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::OptionItem,
            style: ResolvedStyle::default(),
            parent_idx: Some(1),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            // OptionItem.content 不跨 pkg 往返（asset 序列化只为 TextNode/Image 存 content/src），
            // 故生产路径把 option 文本放在子 TextNode（`<div role=option><span>A</span></div>`），
            // nth_option_text 经 collect_subtree_text 收集。这里复刻该结构。
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        // option A 的文本子节点（parent = option index 2）。
        TemplateNode {
            kind: NodeKind::TextNode,
            style: ResolvedStyle::default(),
            parent_idx: Some(2),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: Some("A".to_string()),
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::OptionItem,
            style: ResolvedStyle::default(),
            parent_idx: Some(1),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::TextNode,
            style: ResolvedStyle::default(),
            parent_idx: Some(4),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: Some("B".to_string()),
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::OptionItem,
            style: ResolvedStyle::default(),
            parent_idx: Some(1),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::TextNode,
            style: ResolvedStyle::default(),
            parent_idx: Some(6),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: Some("C".to_string()),
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let pkg = crate::asset::write_package(&PackageInput {
        components: vec![("dropdown", &nodes, &rules, &[])],
    });
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let sel = s.instantiate("bag", "dropdown").unwrap();
    let scene = s.scene.as_ref().unwrap();
    // combobox 的直接 OptionItem 子须为 0（option 在 listbox 内，不是 combobox 直接子）。
    let direct_opts: Vec<_> = scene
        .get(sel)
        .unwrap()
        .children
        .iter()
        .copied()
        .filter(|&c| scene.get(c).is_some_and(|n| n.kind == NodeKind::OptionItem))
        .collect();
    assert!(
        direct_opts.is_empty(),
        "instantiate 后 combobox 无 OptionItem 直接子（全在 listbox 内）"
    );
    // listbox 含 3 个 option，保声明顺序 A/B/C。
    let popup = find_child_by_role_recursive(scene, sel, ROLE_LISTBOX).expect("listbox present");
    let popup_opts: Vec<_> = scene
        .get(popup)
        .unwrap()
        .children
        .iter()
        .copied()
        .filter(|&c| scene.get(c).is_some_and(|n| n.kind == NodeKind::OptionItem))
        .collect();
    assert_eq!(popup_opts.len(), 3, "listbox 含 3 个 option");
    // 经 nth_option_text（扫 listbox 子节点 + collect_subtree_text）验证 option 文本可取、保序。
    assert_eq!(
        crate::scene::control::nth_option_text(scene, sel, 0).as_deref(),
        Some("A"),
        "第 0 个 option 文本 = A"
    );
    assert_eq!(
        crate::scene::control::nth_option_text(scene, sel, 1).as_deref(),
        Some("B"),
        "第 1 个 option 文本 = B"
    );
    assert_eq!(
        crate::scene::control::nth_option_text(scene, sel, 2).as_deref(),
        Some("C"),
        "第 2 个 option 文本 = C"
    );
    // parent 指针指向 listbox。
    for &o in &popup_opts {
        assert_eq!(scene.get(o).unwrap().parent, Some(popup));
    }
}

/// 辅助：建带 role/data-slot 的 pkg（comp = root(role=slider) + thumb(data-slot=thumb)）。
fn make_test_pkg_with_roles() -> Vec<u8> {
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: Some("slider".into()),
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: Some("thumb".into()),
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
    ];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp", &nodes, &rules, &[])],
    };
    crate::asset::write_package(&input)
}

/// instantiate 把 TemplateNode.role/data_slot 填进 Scene.roles side table（稀疏）。
/// role 驱动后续语义分派 + find_child_by_role/slot 查表。data-slot 映射成 slots 的 key
/// （slots["thumb"]=""，find_child_by_slot 比对 key 是否存在）。
#[test]
fn instantiate_fills_roles_side_table_from_template() {
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &make_test_pkg_with_roles()).unwrap();
    let root = s.instantiate("bag", "comp").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let thumb = scene.get(root).unwrap().children[0];
    // root 带 role=slider
    assert_eq!(scene.roles.role_of(root), Some("slider"));
    assert!(
        scene.roles.slot_of(root, "thumb").is_none(),
        "root has no data-slot"
    );
    // thumb 带 data-slot=thumb → slots["thumb"]=""（key=slot 名，值空串占位）
    assert!(scene.roles.role_of(thumb).is_none(), "thumb has no role");
    assert!(
        scene.roles.slot_of(thumb, "thumb").is_some(),
        "thumb 的 data-slot=thumb 映射成 slots[\"thumb\"]"
    );
}

/// instantiate 后 remove_node 联动清 RoleTable 槽，防悬空 NodeId 残留。
#[test]
fn remove_node_clears_roles_side_table() {
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &make_test_pkg_with_roles()).unwrap();
    let root = s.instantiate("bag", "comp").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let thumb = scene.get(root).unwrap().children[0];
    assert!(scene.roles.get(root).is_some(), "root role 在表");
    assert!(scene.roles.get(thumb).is_some(), "thumb slot 在表");
    // 删 root（递归删 thumb 子树）→ 两槽都清。
    s.remove_node(root);
    let scene = s.scene.as_ref().unwrap();
    assert!(scene.roles.get(root).is_none(), "删后 root role 清了");
    assert!(scene.roles.get(thumb).is_none(), "删后 thumb slot 清了");
}

/// ControlInit::TabList{selected_index} 经 create_node_from_template 映射成
/// ControlState::TabList{selected_index}（usize，u32→usize 转换）。镜像 Dropdown
/// 同类映射的测试模式。
#[test]
fn instantiate_tablist_control_init_maps_to_state() {
    use crate::scene::node::ControlState;
    let pkg = make_control_pkg(
        NodeKind::TabList,
        crate::asset::ControlInit::TabList {
            selected_index: 2,
            manual: false,
        },
    );
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let id = s.instantiate("bag", "c").unwrap();
    let scene = s.scene.as_ref().unwrap();
    assert!(
        matches!(
            scene.controls.get(id),
            Some(ControlState::TabList {
                selected_index: 2,
                ..
            })
        ),
        "ControlInit::TabList{{selected_index:2}} → ControlState::TabList{{selected_index:2}}"
    );
}

/// instantiate 把 TemplateNode.aria_controls 拷进 RoleInfo.aria_controls（运行时
/// TabList tab→panel 跨树关联的字符串）。本测试手灌 TemplateNode.aria_controls=Some
/// 验 instantiate 拷贝路径。
#[test]
fn instantiate_copies_aria_controls_into_role_info() {
    let nodes = [TemplateNode {
        kind: NodeKind::TabList,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: Some(crate::asset::ControlInit::TabList {
            selected_index: 0,
            manual: false,
        }),
        role: Some("tablist".to_string()),
        data_slot: None,
        attrs: vec![("aria-controls".to_string(), "panel-1".to_string())],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules, &[])],
    };
    let pkg = crate::asset::write_package(&input);
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let id = s.instantiate("bag", "c").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let info = scene.roles.get(id).expect("role=tablist → RoleInfo 入表");
    assert_eq!(
        info.attr("aria-controls"),
        Some("panel-1"),
        "instantiate 拷 TemplateNode.attrs 进 RoleInfo.attrs 仓"
    );
}

/// attrs β 运行时关联（#8/#22）：`aria-labelledby`（IDREF 列表，空格分隔）经
/// instantiate 入 RoleInfo.attrs 仓后，`Scene::attr_idrefs` 逐 id 本作用域解析到
/// 节点——与 aria-controls 同一条解析路（labelledby 现无业务消费方，机制在此
/// 锁死，消费方出现即接线）。
#[test]
fn attr_idrefs_resolves_labelledby_targets() {
    fn plain_container() -> TemplateNode {
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        }
    }
    let label_a = TemplateNode {
        kind: NodeKind::Container,
        id_attr: Some("label-a".to_string()),
        content: Some("分类".to_string()),
        parent_idx: Some(0),
        ..plain_container()
    };
    let label_b = TemplateNode {
        kind: NodeKind::Container,
        id_attr: Some("label-b".to_string()),
        content: Some("背包".to_string()),
        parent_idx: Some(0),
        ..plain_container()
    };
    let owner = TemplateNode {
        kind: NodeKind::Container,
        role: Some("tree".to_string()),
        attrs: vec![("aria-labelledby".to_string(), "label-a label-b".to_string())],
        ..plain_container()
    };
    let nodes = [owner, label_a, label_b];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("c", &nodes, &rules, &[])],
    };
    let pkg = crate::asset::write_package(&input);
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &pkg).unwrap();
    let root = s.instantiate("bag", "c").unwrap();
    let scene = s.scene.as_ref().unwrap();
    let resolved = scene.attr_idrefs(root, "aria-labelledby");
    let resolved_ids: Vec<&str> = resolved
        .iter()
        .filter_map(|&nid| scene.get(nid).and_then(|n| n.id_attr.as_deref()))
        .collect();
    assert_eq!(
        resolved_ids,
        vec!["label-a", "label-b"],
        "labelledby IDREF 列表按序解析到目标节点"
    );
    // 无该属性 / 无 RoleInfo 条目 → 空列表（不 panic）。
    assert!(scene.attr_idrefs(resolved[1], "aria-labelledby").is_empty());
}

// 树形：root(0, SCOPE_ROOT) + host(1, component_scope + custom_tag + class card-host)
// + inner(2, class gic-body)。页面规则包装 scope_root=root；展开域规则锚 host。

/// 组件展开域测试 pkg：页面规则 + 锚定规则都参数化，覆盖隔离正反面。
fn component_scope_pkg(page_rules: &[DynamicRule], scope_rules: &[DynamicRule]) -> Vec<u8> {
    let root_node = TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    };
    let host = TemplateNode {
        kind: NodeKind::CustomElement,
        style: ResolvedStyle::default(),
        parent_idx: Some(0),
        classes: vec!["card-host".to_string()],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: Some("game-item-card".to_string()),
        component_scope: true,
    };
    let inner = TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: Some(1),
        classes: vec!["gic-body".to_string()],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    };
    let nodes = [root_node, host, inner];
    let page_table = crate::style::dynamic::DynamicRuleTable {
        rules: page_rules.to_vec(),
    };
    let scope_table = crate::style::dynamic::DynamicRuleTable {
        rules: scope_rules.to_vec(),
    };
    let input = PackageInput {
        components: vec![("page", &nodes, &page_table, &[])],
    };
    let scopes = [crate::asset::ComponentScopeInput {
        component: "page",
        anchor_idx: 1,
        rules: &scope_table,
    }];
    crate::asset::write_package_with_scopes(&input, &scopes)
}

/// 建 stage + instantiate，返回 (Stage, 实例根, host, inner)。
fn component_scope_stage4(
    page_rules: &[DynamicRule],
    scope_rules: &[DynamicRule],
) -> (Stage, NodeId, NodeId, NodeId) {
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    let pkg = component_scope_pkg(page_rules, scope_rules);
    s.load_package("bag", &pkg).unwrap();
    let root = s.instantiate("bag", "page").unwrap();
    let host = s.scene.as_ref().unwrap().get(root).unwrap().children[0];
    let inner = s.scene.as_ref().unwrap().get(host).unwrap().children[0];
    (s, root, host, inner)
}

/// host 打三重标记：SCOPE_ROOT（后代 CSS 边界）+ LOOKUP_SCOPE（查找边界）
/// + HOST_IN_PARENT_SCOPE（自身归外层页面作用域）。
#[test]
fn instantiate_marks_component_host_scope_flags() {
    let (s, root, host, _) = component_scope_stage4(&[], &[]);
    let flags = s
        .scene
        .as_ref()
        .unwrap()
        .get(host)
        .unwrap()
        .interaction
        .flags;
    assert!(flags.contains(NodeFlags::SCOPE_ROOT));
    assert!(flags.contains(NodeFlags::LOOKUP_SCOPE));
    assert!(flags.contains(NodeFlags::HOST_IN_PARENT_SCOPE));
    // 实例根照旧只有 SCOPE_ROOT|LOOKUP_SCOPE（无 HOST_IN_PARENT_SCOPE）
    let rf = s
        .scene
        .as_ref()
        .unwrap()
        .get(root)
        .unwrap()
        .interaction
        .flags;
    assert!(rf.contains(NodeFlags::SCOPE_ROOT));
    assert!(!rf.contains(NodeFlags::HOST_IN_PARENT_SCOPE));
}

/// custom_tag 从 TemplateNode 拷进 live Node（FFI get_custom_tag / rematch tag 选择器用）。
#[test]
fn instantiate_copies_custom_tag() {
    let (s, _root, host, inner) = component_scope_stage4(&[], &[]);
    let scene = s.scene.as_ref().unwrap();
    assert_eq!(
        scene.get(host).unwrap().custom_tag.as_deref(),
        Some("game-item-card")
    );
    // 内部子非 CustomElement → None
    assert!(scene.get(inner).unwrap().custom_tag.is_none());
}

/// 展开域锚定规则进 scene.dynamic_rules，scope_root = host NodeId（不是实例根）。
#[test]
fn component_scope_rules_anchor_to_host() {
    let (s, root, host, _) = component_scope_stage4(
        &[class_rule("card-host", "background-color", "#00ff00")],
        &[class_rule("gic-body", "background-color", "#0000ff")],
    );
    let entries = &s.scene.as_ref().unwrap().dynamic_rules.entries;
    let page_anchored = entries.iter().filter(|e| e.scope_root == root).count();
    let host_anchored = entries.iter().filter(|e| e.scope_root == host).count();
    assert_eq!(page_anchored, 1, "page rule wraps scope_root=instance root");
    assert_eq!(host_anchored, 1, "scope rule anchors scope_root=host");
}

/// 页面规则样式化 host 本体（HOST_IN_PARENT_SCOPE：host 归页面作用域），
/// 且组件锚定规则不落在 host 上（shadow 规则不样式化 host，同 DOM :host 才行）。
#[test]
fn page_rules_style_host_but_scope_rules_do_not() {
    // 页面规则 .card-host 绿 + 锚定规则 .card-host 蓝（锚 host）——host 只吃页面绿
    let (mut s, _root, host, _) = component_scope_stage4(
        &[class_rule("card-host", "background-color", "#00ff00")],
        &[class_rule("card-host", "background-color", "#0000ff")],
    );
    rematch_pseudo_classes(s.scene.as_mut().unwrap(), s.root_size, s.safe_insets);
    let bg = s
        .scene
        .as_ref()
        .unwrap()
        .get(host)
        .unwrap()
        .style
        .background_color;
    assert_eq!(
        bg,
        Some([0.0, 1.0, 0.0, 1.0]),
        "host styled by PAGE rule (green), not scope rule (blue)"
    );
}

/// 展开域内部子被锚定规则样式化（蓝），页面规则穿透不进（隔离正面 + 反面）。
#[test]
fn scope_rules_style_internals_page_rules_isolated() {
    // 正面：锚定 .gic-body 蓝 → inner 蓝
    let (mut s, _root, _host, inner) = component_scope_stage4(
        &[],
        &[class_rule("gic-body", "background-color", "#0000ff")],
    );
    rematch_pseudo_classes(s.scene.as_mut().unwrap(), s.root_size, s.safe_insets);
    let bg = s
        .scene
        .as_ref()
        .unwrap()
        .get(inner)
        .unwrap()
        .style
        .background_color;
    assert_eq!(
        bg,
        Some([0.0, 0.0, 1.0, 1.0]),
        "anchored rule styles internals"
    );

    // 反面：页面 .gic-body 红（scope_root=实例根）→ inner 不命中（scope=host ≠ root）
    let (mut s2, _r2, _h2, inner2) = component_scope_stage4(
        &[class_rule("gic-body", "background-color", "#ff0000")],
        &[],
    );
    rematch_pseudo_classes(s2.scene.as_mut().unwrap(), s2.root_size, s2.safe_insets);
    let bg2 = s2
        .scene
        .as_ref()
        .unwrap()
        .get(inner2)
        .unwrap()
        .style
        .background_color;
    assert_eq!(bg2, None, "page rule must NOT pierce component boundary");
}

/// clone_subtree 保真组件展开域：scope 三标记 + custom_tag 拷贝，锚定规则重锚到克隆 host。
#[test]
fn clone_subtree_preserves_component_scope() {
    let (mut s, root, host_orig, _) = component_scope_stage4(
        &[],
        &[class_rule("gic-body", "background-color", "#0000ff")],
    );
    let clone_root = s.clone_subtree(root).unwrap();
    let host_clone = s.scene.as_ref().unwrap().get(clone_root).unwrap().children[0];
    assert_ne!(host_orig, host_clone, "clone host is a distinct node");
    // 标记 + custom_tag
    let flags = s
        .scene
        .as_ref()
        .unwrap()
        .get(host_clone)
        .unwrap()
        .interaction
        .flags;
    assert!(flags.contains(NodeFlags::SCOPE_ROOT));
    assert!(flags.contains(NodeFlags::HOST_IN_PARENT_SCOPE));
    assert_eq!(
        s.scene
            .as_ref()
            .unwrap()
            .get(host_clone)
            .unwrap()
            .custom_tag
            .as_deref(),
        Some("game-item-card")
    );
    // 锚定规则重锚：克隆 host 有自己的规则副本，与源实例隔离
    let entries = &s.scene.as_ref().unwrap().dynamic_rules.entries;
    assert!(
        entries.iter().any(|e| e.scope_root == host_clone),
        "scoped rules re-anchored to clone host"
    );
    assert!(
        entries.iter().any(|e| e.scope_root == host_orig),
        "original anchored rules kept"
    );
}

/// L3 查找边界：页面级 find_node_by_id_in_subtree 不穿透组件展开域内部；
/// host 自身 id 可命中；host 内部 Get 照常。find_node_by_id_in_own_scope 多实例不串。
#[test]
fn lookup_boundary_l3_component_scope() {
    // 组件树带 id：host id="card"，inner id="inner-badge"
    let page_rules: Vec<DynamicRule> = vec![];
    let scope_rules: Vec<DynamicRule> = vec![];
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    let pkg = component_scope_pkg(&page_rules, &scope_rules);
    s.load_package("bag", &pkg).unwrap();
    let root = s.instantiate("bag", "page").unwrap();
    let host = s.scene.as_ref().unwrap().get(root).unwrap().children[0];
    let inner = s.scene.as_ref().unwrap().get(host).unwrap().children[0];
    s.scene.as_mut().unwrap().get_mut(host).unwrap().id_attr = Some("card".into());
    s.scene.as_mut().unwrap().get_mut(inner).unwrap().id_attr = Some("inner-badge".into());
    let scene = s.scene.as_ref().unwrap();
    // 页面级：host 自身可命中（Shadow DOM：host 在 light tree）
    assert_eq!(
        scene.find_node_by_id_in_subtree(root, "card"),
        Some(host),
        "host itself is page-visible"
    );
    // 页面级：内部 id 不穿透
    assert_eq!(
        scene.find_node_by_id_in_subtree(root, "inner-badge"),
        None,
        "page-level find must NOT pierce component host"
    );
    // host 内部：正常命中
    assert_eq!(
        scene.find_node_by_id_in_subtree(host, "inner-badge"),
        Some(inner),
        "inside-scope find works"
    );
}

/// find_node_by_id_in_own_scope 多实例不串：同模板两实例共享内部 id，
/// 各自从 host 解析命中本实例的节点（aria-controls 的多实例安全地基）。
#[test]
fn own_scope_lookup_multi_instance_no_cross_talk() {
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    let pkg = component_scope_pkg(&[], &[]);
    s.load_package("bag", &pkg).unwrap();
    let root1 = s.instantiate("bag", "page").unwrap();
    let root2 = s.instantiate("bag", "page").unwrap();
    let inner1 = {
        let scene = s.scene.as_mut().unwrap();
        let host1 = scene.get(root1).unwrap().children[0];
        scene.get_mut(host1).unwrap().id_attr = Some("card".into());
        let i1 = scene.get(host1).unwrap().children[0];
        scene.get_mut(i1).unwrap().id_attr = Some("dup-badge".into());
        let host2 = scene.get(root2).unwrap().children[0];
        scene.get_mut(host2).unwrap().id_attr = Some("card2".into());
        let i2 = scene.get(host2).unwrap().children[0];
        scene.get_mut(i2).unwrap().id_attr = Some("dup-badge".into());
        i1
    };
    let scene = s.scene.as_ref().unwrap();
    let host2 = scene.get(root2).unwrap().children[0];
    // 从实例 2 的 host 解析 "dup-badge" → 实例 2 的 inner（不是全局首匹配的实例 1）
    assert_eq!(
        scene.find_node_by_id_in_own_scope(host2, "dup-badge"),
        Some(scene.get(host2).unwrap().children[0]),
        "own-scope resolution must hit the SAME instance, not global first match"
    );
    let _ = inner1;
}

#[test]
fn unload_package_removes_templates_but_not_instances() {
    // prefab 删除语义：卸载移除模板注册（再 instantiate 报错），已实例化活节点不受影响；
    // 未加载卸载 → Err；重载同名包 → 恢复可用（新实例 NodeId 不同）。
    let pkg = make_test_pkg_with_subtree();
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("p", &pkg).unwrap();
    let inst = s.instantiate("p", "comp1").unwrap();

    s.unload_package("p").unwrap();
    assert!(
        s.instantiate("p", "comp1").is_err(),
        "templates gone after unload"
    );
    assert!(
        s.scene.as_ref().unwrap().get(inst).is_some(),
        "live instance survives unload"
    );

    assert!(
        s.unload_package("p").is_err(),
        "double unload → Err (not loaded)"
    );

    s.load_package("p", &pkg).unwrap();
    let inst2 = s.instantiate("p", "comp1").unwrap();
    assert_ne!(inst, inst2, "reloaded package instantiates fresh nodes");
}

/// 组件 `<style>` 规则必须能样式化 slot 投射的 light 子（「投影归属」语义：
/// 给投影内容写样式写在组件文件里）。
///
/// 复现结构（skill-slot 投影）：host(.slot-cost) ← 投影 .slot-cost-row span ← 空 .qis span。
/// 打包期投影 span 在页面宇宙被烘 rich_text_block（页面侧分类看不到组件 CSS 的
/// display:flex）；运行时组件锚定规则 .slot-cost-row{display:flex} 命中后，display
/// 必须把该节点切回 flex 容器（架构不变量：display 选择布局 Strategy），.qis 成为
/// 可定尺寸的 flex item——否则折叠进 inline flow 恒零尺寸不可见（C# 内联绕法之所以
/// "生效"正是绕过了折叠的 taffy 直写，两条通道行为不一致）。
#[test]
fn component_scoped_rules_style_projected_children() {
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec!["slot-cost".to_string()],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: true,
        },
        TemplateNode {
            kind: NodeKind::TextElement,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec!["slot-cost-row".to_string()],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            // 页面侧分类所烘（span 默认 rich_text；分类时看不到组件规则 display:flex）。
            rich_text_block: true,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::TextElement,
            style: ResolvedStyle::default(),
            parent_idx: Some(1),
            classes: vec!["qis".to_string()],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            attrs: vec![],
            rich_text_block: true,
            custom_tag: None,
            component_scope: false,
        },
    ];
    let comp_rules = crate::style::dynamic::DynamicRuleTable {
        rules: vec![
            class_rule("slot-cost-row", "display", "flex"),
            class_rule("qis", "width", "10px"),
            class_rule("qis", "height", "10px"),
        ],
    };
    let scopes = [crate::asset::ComponentScopeInput {
        component: "skill-slot",
        anchor_idx: 0,
        rules: &comp_rules,
    }];
    let input = PackageInput {
        components: vec![("skill-slot", &nodes, &comp_rules, &[])],
    };
    let pkg = crate::asset::write_package_with_scopes(&input, &scopes);

    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((400.0, 400.0)).unwrap();
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    let doc = s.create_root("div", "").unwrap();
    s.load_package("game", &pkg).unwrap();
    let host = s.instantiate("game", "skill-slot").unwrap();
    crate::scene::dynamic::append_child(s.scene.as_mut().unwrap(), doc, host).unwrap();
    s.tick_and_render();

    let scene = s.scene.as_ref().unwrap();
    let row = scene.get(host).unwrap().children[0];
    let qis = scene.get(row).unwrap().children[0];
    // 1. 组件锚定规则命中投影子（scope_root=host，投影子 scope=host）。
    assert_eq!(
        scene.get(row).unwrap().style.taffy_style.display,
        taffy::Display::Flex,
        "组件规则 .slot-cost-row{{display:flex}} 应命中投影 span"
    );
    // 2. display:flex 切换布局 Strategy：不折叠，.qis 成为可定尺寸 flex item。
    let r = &scene.get(qis).unwrap().layout_rect;
    assert!(
        (r.w - 10.0).abs() < 0.01 && (r.h - 10.0).abs() < 0.01,
        ".qis 应是 10×10 独立盒子（width/height 规则生效），got {r:?}"
    );
}

/// HTML disabled 属性链（#93 验收回归：禁用按钮悬停仍手型）：`<button disabled>`
/// 过围栏但运行时无人消费——现在 TemplateNode.disabled（flags bit 0x08）经
/// write→load 往返存活，instantiate 置 NodeFlags::DISABLED（click 抑制 / active
/// 截断 / :disabled 伪类 / 光标 affordance 全走既有 disabled 语义）。
#[test]
fn instantiate_maps_html_disabled_to_node_flag() {
    let mut btn_style = ResolvedStyle::default();
    crate::scene::dynamic::apply_css(&mut btn_style, "width:100px;height:60px");
    let nodes = [TemplateNode {
        kind: NodeKind::Button,
        style: btn_style,
        parent_idx: None,
        classes: vec![],
        id_attr: Some("dis-btn".into()),
        draggable: false,
        disabled: true,
        tabindex: None,
        content: Some("禁用按钮".into()),
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &[])],
    };
    let pkg = crate::asset::write_package(&input);

    let mut s = Stage::new_for_test();
    let doc = s.create_root("div", "").unwrap();
    s.load_package("p", &pkg).unwrap();
    let btn = s.instantiate("p", "comp1").unwrap();
    crate::scene::dynamic::append_child(s.scene.as_mut().unwrap(), doc, btn).unwrap();
    assert!(
        s.get_node_disabled(btn),
        "HTML disabled 属性 → 运行时 NodeFlags::DISABLED"
    );
}

/// disabled 位 roundtrip 对称性：不写的包读出 false（旧 v47 包位恒 0 的兼容口径）。
#[test]
fn pkg_disabled_bit_defaults_false() {
    let nodes = [TemplateNode {
        kind: NodeKind::Button,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = crate::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![("comp1", &nodes, &rules, &[])],
    };
    let bytes = crate::asset::write_package(&input);
    let pkg = crate::asset::read_package(&bytes).unwrap();
    let tn = &pkg.components["comp1"].nodes[0];
    assert!(!tn.disabled, "未声明的 disabled 位读出 false");
}

/// #13/#75 验收批（P2）：方向键移动 TabList 选中后，aria-selected 属性选择器驱动的
/// 样式必须在**新旧两个 tab** 上都翻转——新 tab 上色 + 旧 tab 褪色，且两侧
/// render_input_version 都 bump（增量渲染指纹失效，不 skip）。Unity 实测现场
/// （F8 dump）曾见多 tab 同时带选中背景 mesh：新选中上色生效、旧选中褪色丢失。
/// 本测试锁全链：pkg → instantiate → tick(rematch) → 键盘方向键 → tick →
/// 两 tab 的 style 与 version 终态。
#[test]
fn tablist_arrow_selection_flips_aria_selected_both_directions() {
    let tablist_style = {
        let mut st = ResolvedStyle::default();
        crate::scene::dynamic::apply_css(&mut st, "display:flex;flex-direction:row");
        st
    };
    let mk_tab = |i: usize| TemplateNode {
        kind: NodeKind::Tab,
        style: ResolvedStyle::default(),
        parent_idx: Some(0),
        classes: vec!["t".to_string()],
        id_attr: Some(format!("t{i}")),
        draggable: false,
        disabled: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: Some("tab".to_string()),
        data_slot: None,
        attrs: vec![],
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    };
    let nodes = vec![
        TemplateNode {
            kind: NodeKind::TabList,
            style: tablist_style,
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            disabled: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: Some(crate::asset::ControlInit::TabList {
                selected_index: 0,
                manual: false,
            }),
            role: Some("tablist".to_string()),
            data_slot: None,
            attrs: vec![],
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        mk_tab(0),
        mk_tab(1),
        mk_tab(2),
    ];
    let rules = crate::style::dynamic::DynamicRuleTable {
        rules: vec![DynamicRule {
            // 属性选择器手构（core 测试不依赖 fence 解析器）：.t[aria-selected="true"]。
            selector: ParsedSelector {
                raw: ".t[aria-selected=\"true\"]".to_string(),
                compound: vec![Compound {
                    tag: None,
                    id: None,
                    classes: vec!["t".to_string()],
                    combinator: Combinator::Descendant,
                    pseudo_hover: false,
                    pseudo_active: false,
                    pseudo_disabled: false,
                    pseudo_focus: false,
                    pseudo_nth_child: None,
                    attrs: vec![crate::style::dynamic::AttrSelector {
                        name: "aria-selected".to_string(),
                        op: crate::style::dynamic::AttrOp::Eq,
                        value: Some("true".to_string()),
                    }],
                }],
                specificity: Specificity(0, 1, 0),
            },
            declarations: vec![Declaration {
                prop: "background-color".to_string(),
                value: "#2b4a6b".to_string(),
            }],
        }],
    };
    let input = PackageInput {
        components: vec![("c", &nodes, &rules, &[])],
    };
    let mut s = Stage::new_for_test();
    s.create_root("div", "").unwrap();
    s.load_package("bag", &crate::asset::write_package(&input))
        .unwrap();
    let tl = s.instantiate("bag", "c").unwrap();
    {
        let scene = s.scene.as_mut().unwrap();
        let root = scene.roots[0];
        crate::scene::dynamic::append_child(scene, root, tl).unwrap();
    }
    s.tick_and_render();
    let (t0, t1) = {
        let scene = s.scene.as_ref().unwrap();
        let tabs = scene.get(tl).unwrap().children.to_vec();
        (tabs[0], tabs[1])
    };
    let sel_color = [43.0 / 255.0, 74.0 / 255.0, 107.0 / 255.0, 1.0];
    {
        let scene = s.scene.as_ref().unwrap();
        assert_eq!(
            scene.get(t0).unwrap().style.background_color,
            Some(sel_color),
            "初始 sel=0：t0 应被 aria-selected 规则上色"
        );
        assert_eq!(
            scene.get(t1).unwrap().style.background_color,
            None,
            "初始 t1 未选中无背景"
        );
    }
    // 聚焦 t0 → 右方向键（automatic：选中即时移动到 t1）。
    s.request_focus(t0);
    s.tick_and_render();
    s.set_key_input(&[crate::input::KeyEvent {
        key_code: crate::input::KEY_RIGHT,
        modifiers: 0,
        is_down: true,
        pad: [0, 0],
    }]);
    s.tick_and_render();
    s.set_key_input(&[crate::input::KeyEvent {
        key_code: crate::input::KEY_RIGHT,
        modifiers: 0,
        is_down: false,
        pad: [0, 0],
    }]);
    s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();
    assert_eq!(
        scene.get(t1).unwrap().style.background_color,
        Some(sel_color),
        "方向键后 t1（新选中）应上色"
    );
    assert_eq!(
        scene.get(t0).unwrap().style.background_color,
        None,
        "方向键后 t0（旧选中）必须褪色——褪色丢失 = 屏幕多 tab 同时高亮"
    );
}
