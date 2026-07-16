//! IrTree → core TemplateNode 桥（生产级，替代 fence/tests/cascade_spike.rs 的 throwaway mini-bridge）。
//! fence parse_template 停在 IrTree；本模块是第一处把 IrTree 翻译成 core 打包结构的代码。

use loomgui_core::asset::{ControllerEntry, TemplateNode};
use loomgui_core::scene::NodeKind;
use loomgui_fence::ir::{IrElement, IrNodeKind};
use loomgui_fence::schema::tag::SemanticKind;
use loomgui_fence::ParsedTemplate;

/// 把一个组件 HTML 的 ParsedTemplate 翻译成 (TemplateNode 树, controllers)。
///
/// 单根契约：`parsed.tree.roots` 必须恰好 1 个（html/head/body 等 shell 标签已由 fence 剥除）。
/// controllers 恒空（② 不做 controller 逻辑，旧范式退役中；data_controller 数据仍抽取保留）。
/// base_style = fence styles[ir_idx]（Task 4 会把 inherited_set bake 进 styles）。
pub fn bridge(
    parsed: &ParsedTemplate,
) -> Result<(Vec<TemplateNode>, Vec<ControllerEntry>), String> {
    if parsed.tree.roots.len() != 1 {
        return Err(format!(
            "组件 HTML 必须单一根元素（当前 {} 个顶层；html/head/body 等 shell 标签已由 fence 剥除）",
            parsed.tree.roots.len()
        ));
    }
    // fence styles 必须与 tree nodes 1:1（css_resolve 对每个 IrNode 产一个 ResolvedStyle）。
    // debug_assert 在测试/dev 暴露契约破裂；release 仍走下方 unwrap_or_default 兜底防 panic。
    debug_assert_eq!(
        parsed.styles.len(),
        parsed.tree.nodes.len(),
        "fence styles must be 1:1 with tree nodes"
    );
    // ir_idx → template_idx 映射（Element/Text 占位；Comment/Doctype/Template 不占）。
    let mut ir_to_tpl: Vec<Option<usize>> = vec![None; parsed.tree.nodes.len()];
    let mut nodes: Vec<TemplateNode> = Vec::new();
    for (ir_idx, node) in parsed.tree.nodes.iter().enumerate() {
        // <template> display:none：整个子树不进实例化（content 是 ListView 复合束蓝图，非活节点）。
        // 检测 = 本节点或任一祖先是 template。packer 构建期跑，O(N*depth) 无妨。
        if is_in_template_subtree(ir_idx, parsed) {
            continue;
        }
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
                nodes.push(TemplateNode {
                    kind,
                    style: parsed.styles.get(ir_idx).cloned().unwrap_or_default(),
                    parent_idx: parent_tpl,
                    classes: extract_classes(el),
                    id_attr: attr(el, "id"),
                    draggable: false,
                    tabindex: attr(el, "tabindex").and_then(|s| s.parse::<i32>().ok()),
                    data_controller: attr(el, "data-controller"),
                    content: None,
                    src,
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
                    data_controller: None,
                    content: Some(s.clone()),
                    src: None,
                });
            }
        }
    }
    // 根被 <template> 包裹（或全 Comment/Doctype）会让 nodes 空——write_package 产 0 节点
    // ComponentTemplate 是静默契约违反，显式报错。
    if nodes.is_empty() {
        return Err(
            "组件根被 <template> 包裹或无实例化节点，产物为空（template 子树整体跳过）".into(),
        );
    }
    Ok((nodes, Vec::new()))
}

/// SemanticKind → NodeKind（total，非静默）。
/// InputDispatch 不进 IrTree（annotate 已分派）；Template 在 bridge 主循环跳过；
/// None = 未识别标签 → Err（围栏门应已挡，防御性兜底）。
fn map_semantic(el: &IrElement) -> Result<NodeKind, String> {
    match el.semantic {
        Some(SemanticKind::Container) => Ok(NodeKind::Container),
        Some(SemanticKind::TextBlock) => Ok(NodeKind::TextBlock),
        Some(SemanticKind::TextElement) => Ok(NodeKind::TextElement),
        Some(SemanticKind::LineBreak) => Ok(NodeKind::LineBreak),
        Some(SemanticKind::Label) => Ok(NodeKind::Label),
        Some(SemanticKind::Button) => Ok(NodeKind::Button),
        Some(SemanticKind::Link) => Ok(NodeKind::Link),
        Some(SemanticKind::Image) => Ok(NodeKind::Image),
        Some(SemanticKind::Canvas) => Ok(NodeKind::Canvas),
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
        Some(SemanticKind::Slot) => Ok(NodeKind::Slot),
        Some(SemanticKind::CustomElement) => Ok(NodeKind::CustomElement),
        Some(SemanticKind::InputDispatch) => Err(format!(
            "InternalError: InputDispatch reached bridge (annotate should have dispatched) on <{}>",
            el.tag
        )),
        Some(SemanticKind::Template) => Err(
            "InternalError: Template reached map_semantic (bridge main loop should skip it)".into(),
        ),
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

/// 本 ir 节点或任一祖先是 `<template>` → true（template 子树整体跳过，content 留 ListView 复合束）。
fn is_in_template_subtree(ir_idx: usize, parsed: &ParsedTemplate) -> bool {
    let mut cur = Some(loomgui_fence::ir::IrNodeId(ir_idx));
    while let Some(cid) = cur {
        if let IrNodeKind::Element(el) = &parsed.tree.nodes[cid.0].kind {
            if el.semantic == Some(SemanticKind::Template) {
                return true;
            }
        }
        cur = parsed.tree.nodes[cid.0].parent;
    }
    false
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
        bridge(&parsed).unwrap().0
    }

    #[test]
    fn div_p_text_img_mapping_and_structure() {
        let nodes =
            bridged(r#"<div class="root" id="r"><p class="t">hi</p><img src="a.png"></div>"#);
        // [0] div Container root (parent=None, class=root, id=r)
        assert_eq!(nodes[0].kind, NodeKind::Container);
        assert_eq!(nodes[0].parent_idx, None);
        assert!(nodes[0].classes.contains(&"root".to_string()));
        assert_eq!(nodes[0].id_attr.as_deref(), Some("r"));
        // [1] p TextBlock (parent=0, class=t)
        assert_eq!(nodes[1].kind, NodeKind::TextBlock);
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
        let nodes = bridged(r#"<div><input type="range"><input type="checkbox"></div>"#);
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
    fn template_element_skipped() {
        let nodes = bridged(r#"<div><template><p>x</p></template></div>"#);
        // [0] = div Container；template 节点本身不进 nodes
        assert_eq!(nodes[0].kind, NodeKind::Container);
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn tabindex_parsed() {
        let nodes = bridged(r#"<div><button tabindex="2">b</button></div>"#);
        let btn = nodes.iter().find(|n| n.kind == NodeKind::Button).unwrap();
        assert_eq!(btn.tabindex, Some(2));
    }

    #[test]
    fn template_root_yields_empty_error() {
        // 根是 <template> → 整棵子树跳过 → nodes 空 → Err（不静默产 0 节点 ComponentTemplate）。
        let parsed = loomgui_fence::parse_template(r#"<template><p>x</p></template>"#, "t.html");
        assert!(
            parsed.diagnostics.is_empty(),
            "diags: {:?}",
            parsed.diagnostics
        );
        let result = bridge(&parsed);
        assert!(result.is_err(), "template 根应报错（产物为空）");
        let err = result.unwrap_err();
        assert!(
            err.contains("产物为空") || err.contains("template"),
            "错误信息应点明 template/空: {err}"
        );
    }
}
