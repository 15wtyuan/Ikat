//! IrTree → core TemplateNode 桥（生产级，替代 fence/tests/cascade_spike.rs 的 throwaway mini-bridge）。
//! fence parse_template 停在 IrTree；本模块是第一处把 IrTree 翻译成 core 打包结构的代码。

use loomgui_core::asset::{ControlInit, EditInit, TemplateNode};
use loomgui_core::scene::NodeKind;
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

/// `<template>` 的直接子元素必须全是 `<li>`——它是 ListView item 蓝图，克隆产的
/// slot 根必须是 ListItem。主循环按 IrTree 顺序建节点、不好回溯 template→child 关系，
/// 故做成独立前置遍历。
fn validate_template_children(tree: &IrTree) -> Result<(), String> {
    for node in &tree.nodes {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        if el.semantic != Some(SemanticKind::Template) {
            continue;
        }
        for child in &node.children {
            if let IrNodeKind::Element(cel) = &tree.nodes[child.0].kind {
                if cel.tag != "li" {
                    return Err(format!(
                        "<template> 子元素必须是 <li>（当前 <{}>）",
                        cel.tag
                    ));
                }
            }
        }
    }
    Ok(())
}

/// SemanticKind → NodeKind（total，非静默）。
/// InputDispatch 不进 IrTree（annotate 已分派）；
/// None = 未识别标签 → Err（围栏门应已挡，防御性兜底）。
fn map_semantic(el: &IrElement) -> Result<NodeKind, String> {
    match el.semantic {
        Some(SemanticKind::Container) => Ok(NodeKind::Container),
        Some(SemanticKind::TextElement) => Ok(NodeKind::TextElement),
        Some(SemanticKind::Button) => Ok(NodeKind::Button),
        Some(SemanticKind::Image) => Ok(NodeKind::Image),
        Some(SemanticKind::TextField) => Ok(NodeKind::TextField),
        Some(SemanticKind::PasswordField) => Ok(NodeKind::PasswordField),
        Some(SemanticKind::SearchField) => Ok(NodeKind::SearchField),
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
        Some(SemanticKind::Slot) => Ok(NodeKind::Slot),
        Some(SemanticKind::CustomElement) => Ok(NodeKind::CustomElement),
        Some(SemanticKind::InputDispatch) => Err(format!(
            "InternalError: InputDispatch reached bridge (annotate should have dispatched) on <{}>",
            el.tag
        )),
        Some(SemanticKind::Template) => Ok(NodeKind::Template),
        None => Err(format!(
            "未识别标签 <{}>（semantic=None；围栏门应已挡）",
            el.tag
        )),
    }
}

fn attr(el: &IrElement, name: &str) -> Option<String> {
    el.attributes
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.value.clone())
}

/// 按 NodeKind 从 HTML 属性提取控件初始值（打包期 bake 进 pkg.bin，instantiate 时读出）。
///
/// 语义：
/// - ProgressBar：始终产 Some。无 value 属性视为 indeterminate（HTML 语义：浏览器
///   同样把无 value 的 progress 渲染为旋转动画）；value 缺省 0.0，max 缺省 100.0。
/// - Slider：无 value 返回 None（运行时用默认值兜底）。
/// - Toggle/RadioButton：始终产 Some，显式记录勾选状态（checked 缺省 false）。
///   radio name 缺省空串。
///
/// 从 IrTree 中收集某个元素的所有直接文本子节点内容，拼接成单个字符串。
/// 用于 `<textarea>`：按 HTML 规范 value 来自元素文本内容，非 value 属性。
fn collect_element_text(ir_idx: usize, tree: &IrTree) -> String {
    let mut out = String::new();
    for child_id in &tree.nodes[ir_idx].children {
        if let IrNodeKind::Text(s) = &tree.nodes[child_id.0].kind {
            out.push_str(s);
        }
    }
    out
}

/// 从 value/placeholder/maxlength/readonly 属性构建 EditInit（TextField/NumberField 共用）。
/// 缺省值与 HTML 一致：value/placeholder 空串、maxlength 0（无限）、readonly false。
/// 注意 TextArea 不用本函数——其 value 按 HTML 规范取元素文本内容而非 value 属性。
fn extract_edit_init(el: &IrElement) -> EditInit {
    EditInit {
        value: attr(el, "value").unwrap_or_default(),
        placeholder: attr(el, "placeholder").unwrap_or_default(),
        max_length: attr(el, "maxlength")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        readonly: attr(el, "readonly").is_some(),
    }
}

/// 非 control 节点返回 None。
fn extract_control_init(
    kind: NodeKind,
    el: &IrElement,
    ir_idx: usize,
    tree: &IrTree,
) -> Option<ControlInit> {
    match kind {
        NodeKind::ProgressBar => {
            // value 缺席 = indeterminate（必须先判 is_some 再 parse，否则 indeterminate 误判 false）。
            let value_attr = attr(el, "value");
            let indeterminate = value_attr.is_none();
            let value = value_attr
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0);
            let max = attr(el, "max")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(100.0);
            Some(ControlInit::Progress {
                value,
                max,
                indeterminate,
            })
        }
        NodeKind::Slider => attr(el, "value")
            .and_then(|v| v.parse::<f32>().ok())
            .map(|value| {
                let min = attr(el, "min")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(0.0);
                let max = attr(el, "max")
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(100.0);
                let step = attr(el, "step")
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
            checked: attr(el, "checked").is_some(),
        }),
        NodeKind::RadioButton => Some(ControlInit::Radio {
            checked: attr(el, "checked").is_some(),
            name: attr(el, "name").unwrap_or_default(),
        }),
        NodeKind::TextField | NodeKind::PasswordField | NodeKind::SearchField => {
            Some(ControlInit::TextField(extract_edit_init(el)))
        }
        NodeKind::TextArea => Some(ControlInit::TextArea(EditInit {
            // textarea 按 HTML 规范用元素文本内容而非 value 属性（不走 extract_edit_init）。
            value: collect_element_text(ir_idx, tree),
            placeholder: attr(el, "placeholder").unwrap_or_default().to_string(),
            max_length: attr(el, "maxlength")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            readonly: attr(el, "readonly").is_some(),
        })),
        NodeKind::Dropdown => {
            // 扫 select 的 option 子节点，找带 selected 属性的索引；无则默认 0（首项）。
            // selected_index 是「第几个 option」，不是「children 里的第几个」——多行
            // HTML 的 option 之间夹着空白 Text 节点（fence 只剥顶层空白，in-element
            // 保留），用 children 下标会把 option_b 误算成 3 而非 1。
            let mut selected_index: u32 = 0;
            let mut option_index: u32 = 0;
            for child_id in &tree.nodes[ir_idx].children {
                if let IrNodeKind::Element(child) = &tree.nodes[child_id.0].kind {
                    if child.tag == "option" {
                        if child.attributes.iter().any(|a| a.name == "selected") {
                            selected_index = option_index;
                            break;
                        }
                        option_index += 1;
                    }
                }
            }
            Some(ControlInit::Dropdown { selected_index })
        }
        NodeKind::NumberField => {
            let edit = extract_edit_init(el);
            let min = attr(el, "min")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(f32::MIN);
            let max = attr(el, "max")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(f32::MAX);
            let step = attr(el, "step")
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

fn extract_classes(el: &IrElement) -> Vec<String> {
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
            r#"<ul><template><li class="row"><span class="title">x</span></li></template></ul>"#,
        );
        assert!(nodes.iter().any(|n| n.kind == NodeKind::Template));
        assert!(nodes.iter().any(|n| n.kind == NodeKind::ListItem));
    }

    #[test]
    fn template_root_not_li_errors() {
        let parsed = loomgui_fence::parse_template(
            r#"<ul><template><div>x</div></template></ul>"#,
            "test.html",
        );
        assert!(bridge(&parsed).is_err(), "template 直接子元素必须是 <li>");
    }

    #[test]
    fn div_container_text_img_mapping_and_structure() {
        let nodes = bridged(
            r#"<div class="root" id="r"><div class="t">hi</div><img src="a.png" style="display:block"></div>"#,
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
    fn input_dispatch_to_concrete_kinds() {
        let nodes = bridged(
            r#"<style>input[type="range"],input[type="checkbox"]{width:100px}</style><div><input type="range" style="display:block"><input type="checkbox" style="display:block"></div>"#,
        );
        let kinds: Vec<_> = nodes.iter().map(|n| n.kind).collect();
        assert!(
            kinds.contains(&NodeKind::Slider),
            "Slider missing: {kinds:?}"
        );
        assert!(
            kinds.contains(&NodeKind::Toggle),
            "Toggle missing: {kinds:?}"
        );
    }

    #[test]
    fn multi_root_errors() {
        let parsed = loomgui_fence::parse_template(r#"<div>a</div><div>b</div>"#, "t.html");
        assert!(bridge(&parsed).is_err(), "multi-root should error");
    }

    #[test]
    fn template_element_enters_nodes() {
        // v27：template 子树是真实 pkg 节点（运行时克隆源），不再打包期丢弃。
        let nodes = bridged(r#"<div><template><li>x</li></template></div>"#);
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
    }

    #[test]
    fn tabindex_parsed() {
        let nodes = bridged(r#"<div><button tabindex="2" style="display:block">b</button></div>"#);
        let btn = nodes.iter().find(|n| n.kind == NodeKind::Button).unwrap();
        assert_eq!(btn.tabindex, Some(2));
    }

    #[test]
    fn template_as_root_produces_template_node() {
        // 根是 <template> 不再产空——它是合法节点，只是 display:none 不渲染。
        let nodes = bridged(r#"<template><li>x</li></template>"#);
        assert_eq!(nodes[0].kind, NodeKind::Template);
        assert_eq!(nodes[0].parent_idx, None);
    }
}
