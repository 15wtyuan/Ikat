//! IrTree → core TemplateNode 桥（生产级，替代 fence/tests/cascade_spike.rs 的 throwaway mini-bridge）。
//! fence parse_template 停在 IrTree；本模块是第一处把 IrTree 翻译成 core 打包结构的代码。

use loomgui_core::asset::{ControlInit, EditInit, TemplateNode};
use loomgui_core::scene::{AnimatableProps, KeyframeStopSelector, KeyframesRule, NodeKind};
use loomgui_core::style::mapping::{parse_color, parse_transform_trs};
use loomgui_fence::css_rules::{
    KeyframeStopSelector as FenceKeyframeStopSelector, KeyframesRule as FenceKeyframesRule,
};
use loomgui_fence::ir::{IrElement, IrNodeKind, IrTree};
use loomgui_fence::schema::tag::SemanticKind;
use loomgui_fence::ParsedTemplate;

/// 把一个组件 HTML 的 ParsedTemplate 翻译成 TemplateNode 树。
///
/// 单根契约：`parsed.tree.roots` 必须恰好 1 个（html/head/body 等 shell 标签已由 fence 剥除）。
/// base_style = fence styles[ir_idx]（inherited_set 在 cascade 时 bake 进 styles）。
pub fn bridge(parsed: &ParsedTemplate) -> Result<Vec<TemplateNode>, String> {
    if parsed.tree.roots.len() != 1 {
        return Err(format!(
            "组件 HTML 必须单一根元素（当前 {} 个顶层；html/head/body 等 shell 标签已由 fence 剥除）",
            parsed.tree.roots.len()
        ));
    }
    validate_template_children(&parsed.tree)?;
    // fence styles 必须与 tree nodes 1:1（css_resolve 对每个 IrNode 产一个 ResolvedStyle）。
    // debug_assert 在测试/dev 暴露契约破裂；release 仍走下方 unwrap_or_default 兜底防 panic。
    debug_assert_eq!(
        parsed.styles.len(),
        parsed.tree.nodes.len(),
        "fence styles must be 1:1 with tree nodes"
    );
    // ir_idx → template_idx 映射（Element/Text 占位；Comment/Doctype 不占）。
    let mut ir_to_tpl: Vec<Option<usize>> = vec![None; parsed.tree.nodes.len()];
    let mut nodes: Vec<TemplateNode> = Vec::new();
    for (ir_idx, node) in parsed.tree.nodes.iter().enumerate() {
        // parent 总在 child 之前 push（tree_builder DFS），故此处 parent 的 tpl_idx 已知。
        let parent_tpl = node.parent.and_then(|pid| ir_to_tpl[pid.0]);
        match &node.kind {
            // Comment/Doctype 不进实例化树——在 clone ResolvedStyle 前跳过，避免对
            // 被丢弃节点做无谓的大结构 clone。
            IrNodeKind::Comment(_) | IrNodeKind::Doctype { .. } => continue,
            IrNodeKind::Element(el) => {
                let kind = map_semantic(el)?;
                let tpl_idx = nodes.len();
                ir_to_tpl[ir_idx] = Some(tpl_idx);
                let src = if kind == NodeKind::Image {
                    attr(el, "src")
                } else {
                    None
                };
                let control_init = extract_control_init(kind, el, ir_idx, &parsed.tree);
                // role/data-slot：从 HTML 属性提取，进 pkg 供 runtime RoleTable 查表。
                // role 驱动语义分派（combobox/slider/...），data-slot 标识控件视觉部件（fill/thumb）。
                let role = attr(el, "role");
                let data_slot = attr(el, "data-slot");
                // aria-controls：tab→panel 跨树关联的 panel id（TabList 专属）。None = 非关联节点。
                let aria_controls = attr(el, "aria-controls");
                nodes.push(TemplateNode {
                    kind,
                    style: parsed.styles.get(ir_idx).cloned().unwrap_or_default(),
                    parent_idx: parent_tpl,
                    classes: extract_classes(el),
                    id_attr: attr(el, "id"),
                    draggable: false,
                    tabindex: attr(el, "tabindex").and_then(|s| s.parse::<i32>().ok()),
                    content: None,
                    src,
                    control_init,
                    role,
                    data_slot,
                    aria_controls,
                    rich_text_block: parsed.rich_text_blocks.contains(&ir_idx),
                    custom_tag: None,
                    component_scope: false,
                });
            }
            IrNodeKind::Text(s) => {
                // Text 节点 → 独立 TextNode 子节点（core 靠 TextNode 渲染文字；保留 HTML 子树结构）。
                let tpl_idx = nodes.len();
                ir_to_tpl[ir_idx] = Some(tpl_idx);
                nodes.push(TemplateNode {
                    kind: NodeKind::TextNode,
                    style: parsed.styles.get(ir_idx).cloned().unwrap_or_default(),
                    parent_idx: parent_tpl,
                    classes: vec![],
                    id_attr: None,
                    draggable: false,
                    tabindex: None,
                    content: Some(s.clone()),
                    src: None,
                    control_init: None,
                    role: None,
                    data_slot: None,
                    aria_controls: None,
                    rich_text_block: false,
                    custom_tag: None,
                    component_scope: false,
                });
            }
        }
    }
    // 全 Comment/Doctype 的输入会让 nodes 空——write_package 产 0 节点 ComponentTemplate
    // 是静默契约违反，显式报错。
    if nodes.is_empty() {
        return Err("组件无可实例化节点，产物为空".into());
    }
    Ok(nodes)
}

/// Translate fence-local keyframes into the core/pkg representation. Unsupported declarations
/// remain intentionally absent from `AnimatableProps`; the fence accepts a broader visual CSS
/// subset than the M2 animation channels, while malformed supported values are not invented.
pub fn translate_keyframes(fence_kfs: &[FenceKeyframesRule]) -> Vec<KeyframesRule> {
    fence_kfs
        .iter()
        .map(|fence_kf| KeyframesRule {
            name: fence_kf.name.clone(),
            stops: fence_kf
                .stops
                .iter()
                .map(|fence_stop| {
                    let mut props = AnimatableProps::default();
                    for declaration in &fence_stop.declarations {
                        match declaration.prop.as_str() {
                            "opacity" => {
                                props.opacity = declaration.value.parse::<f32>().ok();
                            }
                            "transform" => {
                                props.transform = parse_transform_trs(&declaration.value);
                            }
                            "background-color" => {
                                props.bg_color = parse_color(&declaration.value);
                            }
                            "color" => {
                                props.text_color = parse_color(&declaration.value);
                            }
                            _ => {}
                        }
                    }
                    loomgui_core::scene::KeyframeStop {
                        selector: match fence_stop.selector {
                            FenceKeyframeStopSelector::From => KeyframeStopSelector::From,
                            FenceKeyframeStopSelector::To => KeyframeStopSelector::To,
                            FenceKeyframeStopSelector::Percent(pct) => {
                                KeyframeStopSelector::Percent(pct)
                            }
                        },
                        props,
                        hook: fence_stop.hook.clone(),
                    }
                })
                .collect(),
        })
        .collect()
}

/// `<template>` 是 ListView item 蓝图：spec §8 要求根为**恰好一个** ListItem
/// 语义节点。作者写 `<div role="listitem">`（WAI-ARIA），经 resolve_semantic 落到
/// `SemanticKind::ListItem`，故本校验按 **semantic** 判定而非字面 tag。主循环按
/// IrTree 顺序建节点、不好回溯 template→child 关系，故做成独立前置遍历。零元素
/// （如 `<template>text`）与多元素（如 `<template><div/><div/></template>`）均拒。
pub(crate) fn validate_template_children(tree: &IrTree) -> Result<(), String> {
    for node in &tree.nodes {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        if el.semantic != Some(SemanticKind::Template) {
            continue;
        }
        let element_children: Vec<&IrElement> = node
            .children
            .iter()
            .filter_map(|c| match &tree.nodes[c.0].kind {
                IrNodeKind::Element(cel) => Some(cel),
                _ => None,
            })
            .collect();
        if element_children.len() != 1
            || element_children[0].semantic != Some(SemanticKind::ListItem)
        {
            return Err(format!(
                "<template> 根必须恰好一个 ListItem（<div role=\"listitem\">）（当前 {} 个元素）",
                element_children.len()
            ));
        }
    }
    Ok(())
}

/// SemanticKind → NodeKind（total，非静默）。
/// None = 未识别标签 → Err（围栏门应已挡，防御性兜底）。
pub(crate) fn map_semantic(el: &IrElement) -> Result<NodeKind, String> {
    match el.semantic {
        Some(SemanticKind::Container) => Ok(NodeKind::Container),
        Some(SemanticKind::TextElement) => Ok(NodeKind::TextElement),
        Some(SemanticKind::Button) => Ok(NodeKind::Button),
        Some(SemanticKind::Image) => Ok(NodeKind::Image),
        Some(SemanticKind::TextField) => Ok(NodeKind::TextField),
        Some(SemanticKind::NumberField) => Ok(NodeKind::NumberField),
        Some(SemanticKind::Slider) => Ok(NodeKind::Slider),
        Some(SemanticKind::Toggle) => Ok(NodeKind::Toggle),
        Some(SemanticKind::RadioButton) => Ok(NodeKind::RadioButton),
        Some(SemanticKind::TextArea) => Ok(NodeKind::TextArea),
        Some(SemanticKind::Dropdown) => Ok(NodeKind::Dropdown),
        Some(SemanticKind::OptionItem) => Ok(NodeKind::OptionItem),
        Some(SemanticKind::ProgressBar) => Ok(NodeKind::ProgressBar),
        Some(SemanticKind::ListView) => Ok(NodeKind::ListView),
        Some(SemanticKind::ListItem) => Ok(NodeKind::ListItem),
        Some(SemanticKind::TabList) => Ok(NodeKind::TabList),
        Some(SemanticKind::Tab) => Ok(NodeKind::Tab),
        Some(SemanticKind::Slot) => Ok(NodeKind::Slot),
        Some(SemanticKind::CustomElement) => Ok(NodeKind::CustomElement),
        Some(SemanticKind::Template) => Ok(NodeKind::Template),
        None => Err(format!(
            "未识别标签 <{}>（semantic=None；围栏门应已挡）",
            el.tag
        )),
    }
}

pub(crate) fn attr(el: &IrElement, name: &str) -> Option<String> {
    el.attributes
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.value.clone())
}

/// 按 NodeKind 从 HTML 属性提取控件初始值（打包期 bake 进 pkg.bin，instantiate 时读出）。
///
/// 属性源：**ARIA/data-***。控件一律 role 驱动（spec §2.2），`<div role="progressbar"
/// aria-valuenow="50">` 把初始值放在 ARIA（`aria-valuenow`、`aria-checked`、…）或
/// `data-*`（`data-step`、`data-name`）里——围栏禁止 `<div>` 上出现 plain 属性。
///
/// 语义：
/// - ProgressBar：始终产 Some。value 源缺席 = indeterminate（HTML 语义：浏览器把无 value
///   的 progress 渲染为旋转动画）；value 缺省 0.0，max 缺省 100.0。
/// - Slider：value 源缺席返回 None（运行时用默认值兜底）。
/// - Toggle/RadioButton：始终产 Some，显式记录勾选状态（缺省 false）；radio name 缺省空串。
/// - TextField：value 取元素文本内容（ARIA 无 textbox-value 属性）。
/// - TextArea：value 取元素文本内容。
/// - Dropdown：扫子树找首个被选中 option（`aria-selected="true"`），无则默认第 0 项；
///   详见 [`dropdown_selected_index`]。
pub(crate) fn extract_control_init(
    kind: NodeKind,
    el: &IrElement,
    ir_idx: usize,
    tree: &IrTree,
) -> Option<ControlInit> {
    match kind {
        NodeKind::ProgressBar => {
            // value 源缺席 = indeterminate（先判 is_some 再 parse，否则 indeterminate 误判 false）。
            let value_attr = attr(el, "aria-valuenow");
            let indeterminate = value_attr.is_none();
            let value = value_attr
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0);
            let max = attr(el, "aria-valuemax")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(100.0);
            Some(ControlInit::Progress {
                value,
                max,
                indeterminate,
            })
        }
        NodeKind::Slider => attr(el, "aria-valuenow")
            .and_then(|v| v.parse::<f32>().ok())
            .map(|value| {
                let min = attr(el, "aria-valuemin")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(0.0);
                let max = attr(el, "aria-valuemax")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(100.0);
                let step = attr(el, "data-step")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(1.0);
                ControlInit::Slider {
                    value,
                    min,
                    max,
                    step,
                }
            }),
        NodeKind::Toggle => Some(ControlInit::Toggle {
            checked: bool_attr(el, "aria-checked"),
        }),
        NodeKind::RadioButton => Some(ControlInit::Radio {
            checked: bool_attr(el, "aria-checked"),
            // data-name 承载 radio 分组（ARIA 无「radio 组名」属性）。
            name: attr(el, "data-name").unwrap_or_default(),
        }),
        NodeKind::TextField => Some(ControlInit::TextField(extract_edit_init_with_value(
            el,
            collect_element_text(ir_idx, tree),
        ))),
        NodeKind::TextArea => Some(ControlInit::TextArea(extract_edit_init_with_value(
            el,
            collect_element_text(ir_idx, tree),
        ))),
        NodeKind::Dropdown => Some(ControlInit::Dropdown {
            selected_index: dropdown_selected_index(ir_idx, tree),
            option_values: dropdown_option_values(ir_idx, tree),
        }),
        NodeKind::TabList => Some(ControlInit::TabList {
            // 初始选中项 = 首个 aria-selected="true" 的 role=tab 直接子的序号；
            // 无则默认第 0 项（与 Dropdown 默认选项同语义）。多重 aria-selected=true 取首个
            // （作者失误，运行时不崩；T5 选中态从 selected_index 派生，与 aria-selected 解耦）。
            selected_index: tab_children(ir_idx, tree)
                .iter()
                .position(|t| bool_attr(t, "aria-selected"))
                .unwrap_or(0) as u32,
        }),
        NodeKind::NumberField => {
            let edit =
                extract_edit_init_with_value(el, attr(el, "aria-valuenow").unwrap_or_default());
            let min = attr(el, "aria-valuemin")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(f32::MIN);
            let max = attr(el, "aria-valuemax")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(f32::MAX);
            let step = attr(el, "data-step")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0);
            Some(ControlInit::NumberField {
                edit,
                min,
                max,
                step,
            })
        }
        _ => None,
    }
}

/// Boolean state from an ARIA attribute (value `"true"`/`"false"`).
///
/// ARIA boolean attributes are value-driven: `aria-checked="true"` is on,
/// `aria-checked="false"` is off. Absent or any other value means false.
fn bool_attr(el: &IrElement, aria: &str) -> bool {
    attr(el, aria).is_some_and(|v| v == "true")
}

/// Build EditInit from a caller-supplied value plus the shared
/// placeholder/maxlength/readonly sources (ARIA/data-*). TextField and TextArea
/// differ only in where `value` comes from; the other three fields share one
/// resolution path.
fn extract_edit_init_with_value(el: &IrElement, value: String) -> EditInit {
    EditInit {
        value,
        placeholder: attr(el, "aria-placeholder").unwrap_or_default(),
        max_length: attr(el, "data-maxlength")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        readonly: bool_attr(el, "aria-readonly"),
    }
}

/// Collect an element's direct text children into one string.
///
/// Used for TextArea/TextField initial values: role-driven `<div role="textbox">`
/// carries its content as element text children (ARIA has no textbox-value
/// attribute), matching the HTML `<textarea>` semantics.
fn collect_element_text(ir_idx: usize, tree: &IrTree) -> String {
    let mut out = String::new();
    for child_id in &tree.nodes[ir_idx].children {
        if let IrNodeKind::Text(s) = &tree.nodes[child_id.0].kind {
            out.push_str(s);
        }
    }
    out
}

/// Dropdown initial selected option index.
///
/// A role-driven combobox nests `<div role="option">` inside a `role="listbox"`
/// popup (a structural requirement enforced by control_structure_check), so the
/// options are never direct children of the combobox. Matching by SemanticKind
/// (OptionItem) and a subtree walk covers the layout in document order. Selection
/// is `aria-selected="true"`; when none is selected the default is the first
/// option (index 0).
fn dropdown_selected_index(dropdown_idx: usize, tree: &IrTree) -> u32 {
    let mut selected: Option<u32> = None;
    let mut option_index: u32 = 0;
    visit_options(dropdown_idx, tree, |el| {
        if selected.is_none() && bool_attr(el, "aria-selected") {
            selected = Some(option_index);
        }
        option_index += 1;
    });
    selected.unwrap_or(0)
}

/// Per-option `value` content attribute, in declaration order (same traversal
/// as `dropdown_selected_index`). Absent attribute → `None` slot: the runtime
/// SelectedValue/OptionItem.Value falls back to the option text (HTML
/// semantics: an option without `value` submits its text).
fn dropdown_option_values(dropdown_idx: usize, tree: &IrTree) -> Vec<Option<String>> {
    let mut values = Vec::new();
    visit_options(dropdown_idx, tree, |el| values.push(attr(el, "value")));
    values
}

/// Pre-order DFS over descendant OptionItem-semantic elements in document order.
/// Text children and non-option elements are skipped without advancing the option
/// counter (the counter lives in the caller's closure).
fn visit_options(root: usize, tree: &IrTree, mut visit: impl FnMut(&IrElement)) {
    let mut stack: Vec<usize> = tree.nodes[root]
        .children
        .iter()
        .rev()
        .map(|c| c.0)
        .collect();
    while let Some(idx) = stack.pop() {
        if let IrNodeKind::Element(el) = &tree.nodes[idx].kind {
            if el.semantic == Some(SemanticKind::OptionItem) {
                visit(el);
            }
            stack.extend(tree.nodes[idx].children.iter().rev().map(|c| c.0));
        }
    }
}

/// role=tab 直接子元素（TabList 结构契约要求 tablist 直接子里有 role=tab）。
///
/// 返回 IrElement 引用列表，按文档序——供初始 selected_index 推导按位置取序号。
/// 只看直接子，与 control_structure_check 的 tablist→tab 契约字面对齐。
fn tab_children(parent_idx: usize, tree: &IrTree) -> Vec<&IrElement> {
    tree.nodes[parent_idx]
        .children
        .iter()
        .filter_map(|c| match &tree.nodes[c.0].kind {
            IrNodeKind::Element(el) if attr(el, "role").as_deref() == Some("tab") => Some(el),
            _ => None,
        })
        .collect()
}

pub(crate) fn extract_classes(el: &IrElement) -> Vec<String> {
    attr(el, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridged(html: &str) -> Vec<TemplateNode> {
        let parsed = loomgui_fence::parse_template(html, "test.html");
        assert!(
            parsed.diagnostics.is_empty(),
            "diags: {:?}",
            parsed.diagnostics
        );
        bridge(&parsed).unwrap()
    }

    #[test]
    fn template_subtree_enters_pkg() {
        let nodes = bridged(
            r#"<div role="list" data-fill="3"><template><div role="listitem" class="row"><span class="title">x</span></div></template></div>"#,
        );
        assert!(nodes.iter().any(|n| n.kind == NodeKind::Template));
        assert!(nodes.iter().any(|n| n.kind == NodeKind::ListItem));
    }

    #[test]
    fn template_root_not_listitem_errors() {
        let parsed = loomgui_fence::parse_template(
            r#"<div role="list"><template><div>x</div></template></div>"#,
            "test.html",
        );
        assert!(bridge(&parsed).is_err(), "template 根必须是单个 ListItem");
    }

    #[test]
    fn template_root_role_listitem_ok() {
        // role 驱动 ListView：作者写 <div role=list> > template > <div role=listitem>。
        // validate_template_children 按 semantic（ListItem）判定，不挑字面 tag。
        let parsed = loomgui_fence::parse_template(
            r#"<div role="list" data-fill="3"><template><div role="listitem" class="item"><span>x</span></div></template></div>"#,
            "test.html",
        );
        assert!(
            parsed.diagnostics.is_empty(),
            "no fence diags: {:?}",
            parsed.diagnostics
        );
        let nodes = bridge(&parsed).expect("role=listitem template root is valid ListItem");
        assert!(
            nodes.iter().any(|n| n.kind == NodeKind::ListItem),
            "ListItem node present: {:?}",
            nodes.iter().map(|n| n.kind).collect::<Vec<_>>()
        );
        assert!(
            nodes.iter().any(|n| n.kind == NodeKind::ListView),
            "ListView node present"
        );
    }

    #[test]
    fn template_with_two_listitem_errors() {
        // spec §8：template 根必须恰好一个 ListItem，两个是契约违反。
        let parsed = loomgui_fence::parse_template(
            r#"<div role="list"><template><div role="listitem">a</div><div role="listitem">b</div></template></div>"#,
            "test.html",
        );
        assert!(bridge(&parsed).is_err(), "template 根不能是两个 ListItem");
    }

    #[test]
    fn template_with_only_text_errors() {
        // 零元素（纯文本）也拒：根必须是 ListItem。
        let parsed = loomgui_fence::parse_template(
            r#"<div role="list"><template>just text</template></div>"#,
            "test.html",
        );
        assert!(bridge(&parsed).is_err(), "template 根不能是纯文本");
    }

    #[test]
    fn div_container_text_img_mapping_and_structure() {
        // 根 div 设 display:flex：子是 flex item（不走 rich-text inline/block 分类），
        // 避免 T1 FenceMixedInlineBlock 拒载（div 块子 + img 语义 inline 子 = mixed）。
        let nodes = bridged(
            r#"<div class="root" id="r" style="display:flex"><div class="t">hi</div><img src="a.png" style="display:block"></div>"#,
        );
        // [0] div Container root (parent=None, class=root, id=r)
        assert_eq!(nodes[0].kind, NodeKind::Container);
        assert_eq!(nodes[0].parent_idx, None);
        assert!(nodes[0].classes.contains(&"root".to_string()));
        assert_eq!(nodes[0].id_attr.as_deref(), Some("r"));
        // [1] div Container (parent=0, class=t)
        assert_eq!(nodes[1].kind, NodeKind::Container);
        assert_eq!(nodes[1].parent_idx, Some(0));
        // [2] "hi" TextNode (parent=1, content=hi) — Text 保留为独立子节点
        assert_eq!(nodes[2].kind, NodeKind::TextNode);
        assert_eq!(nodes[2].parent_idx, Some(1));
        assert_eq!(nodes[2].content.as_deref(), Some("hi"));
        // [3] img Image (parent=0, src=a.png)
        assert_eq!(nodes[3].kind, NodeKind::Image);
        assert_eq!(nodes[3].parent_idx, Some(0));
        assert_eq!(nodes[3].src.as_deref(), Some("a.png"));
    }

    #[test]
    fn multi_root_errors() {
        let parsed = loomgui_fence::parse_template(r#"<div>a</div><div>b</div>"#, "t.html");
        assert!(bridge(&parsed).is_err(), "multi-root should error");
    }

    #[test]
    fn template_element_enters_nodes() {
        // template 子树是真实 pkg 节点（运行时克隆源），不再打包期丢弃。
        let nodes = bridged(r#"<div><template><div role="listitem">x</div></template></div>"#);
        assert_eq!(nodes[0].kind, NodeKind::Container);
        assert_eq!(nodes[1].kind, NodeKind::Template);
        assert_eq!(nodes[1].parent_idx, Some(0));
        assert_eq!(nodes[2].kind, NodeKind::ListItem);
        assert_eq!(nodes[2].parent_idx, Some(1));
        // display:none 由 fence tag schema 铺底 → render/layout 自动剪整子树。
        assert_eq!(
            nodes[1].style.display_mode,
            loomgui_core::style::resolved::DisplayMode::None
        );
        // 这才是真正驱动剪枝的字段：collect_display_none_subtree / taffy layout cut
        // / hit-test 全都看 taffy_style.display。display_mode 是旁路标记，无消费者。
        // 只断言 display_mode 会放过 css_resolve 漏写 taffy_style.display 的 bug。
        assert_eq!(nodes[1].style.taffy_style.display, taffy::Display::None);
    }

    #[test]
    fn tabindex_parsed() {
        let nodes = bridged(r#"<div><button tabindex="2" style="display:block">b</button></div>"#);
        let btn = nodes.iter().find(|n| n.kind == NodeKind::Button).unwrap();
        assert_eq!(btn.tabindex, Some(2));
    }

    #[test]
    fn role_and_data_slot_extracted_into_template_node() {
        // role 驱动语义分派 + data-slot 标识控件视觉部件：两个属性都须从 HTML 提取进 TemplateNode，
        // 供 runtime RoleTable 查表。验证 bridge 是 HTML→pkg 的唯一入口不丢这两列。
        let nodes = bridged(
            r#"<style>[role="slider"]{background:#ddd}</style><div role="slider"><div data-slot="thumb"></div></div>"#,
        );
        let root = &nodes[0];
        assert_eq!(root.role.as_deref(), Some("slider"));
        assert!(root.data_slot.is_none(), "root has no data-slot");
        let thumb = nodes
            .iter()
            .find(|n| n.data_slot.as_deref() == Some("thumb"))
            .expect("data-slot=thumb node bridged");
        assert!(thumb.role.is_none(), "thumb has no role");
    }

    #[test]
    fn template_as_root_produces_template_node() {
        // 根是 <template> 不再产空——它是合法节点，只是 display:none 不渲染。
        let nodes = bridged(r#"<template><div role="listitem">x</div></template>"#);
        assert_eq!(nodes[0].kind, NodeKind::Template);
        assert_eq!(nodes[0].parent_idx, None);
    }

    #[test]
    fn tablist_aria_controls_and_initial_selected_extracted() {
        // M3 TabList：role=tablist 初始 selected_index 从 aria-selected="true" 的 tab 派生；
        // 每个 role=tab 的 aria-controls 提取进 TemplateNode（runtime 据此关联 panel）。
        let nodes = bridged(
            r#"<style>[role="tab"][aria-selected="true"]{color:#ff0000}</style>
            <div>
              <div role="tablist" style="display:flex">
                <button role="tab" aria-controls="pa" aria-selected="false">A</button>
                <button role="tab" aria-controls="pb" aria-selected="true">B</button>
              </div>
              <div id="pa"></div><div id="pb"></div>
            </div>"#,
        );
        let tablist = nodes
            .iter()
            .find(|n| n.kind == NodeKind::TabList)
            .expect("TabList node bridged");
        assert_eq!(
            tablist.control_init,
            Some(ControlInit::TabList { selected_index: 1 })
        );
        // 每个 tab 的 aria_controls 落到各自 TemplateNode（按 aria-controls 值定位，避免 DFS 序硬编码）。
        let tab_a = nodes
            .iter()
            .find(|n| n.kind == NodeKind::Tab && n.aria_controls.as_deref() == Some("pa"))
            .expect("tab A bridged with aria-controls=pa");
        assert_eq!(tab_a.role.as_deref(), Some("tab"));
        let tab_b = nodes
            .iter()
            .find(|n| n.kind == NodeKind::Tab && n.aria_controls.as_deref() == Some("pb"))
            .expect("tab B bridged with aria-controls=pb");
        assert_eq!(tab_b.role.as_deref(), Some("tab"));
    }

    #[test]
    fn tablist_initial_selected_defaults_to_zero_when_none_marked_true() {
        // 无 aria-selected="true" → 默认第 0 项选中（与 Dropdown 默认选项同语义）。
        let nodes = bridged(
            r#"<style>[role="tab"]{color:#ff0000}</style>
            <div>
              <div role="tablist" style="display:flex">
                <button role="tab" aria-controls="pa">A</button>
                <button role="tab" aria-controls="pb">B</button>
              </div>
              <div id="pa"></div><div id="pb"></div>
            </div>"#,
        );
        let tablist = nodes
            .iter()
            .find(|n| n.kind == NodeKind::TabList)
            .expect("TabList node bridged");
        assert_eq!(
            tablist.control_init,
            Some(ControlInit::TabList { selected_index: 0 })
        );
    }
}
