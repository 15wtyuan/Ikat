//! Stage 6.7：控件必须被 CSS 命中校验。
//!
//! LoomGUI 控件（ProgressBar / Slider / Toggle / RadioButton / Dropdown /
//! TextField / TextArea / NumberField）**不带 UA 默认样式**——
//! core 刻意保持纯净，不开「框架自带样式源」先例。代价：写了控件标签却没匹配的 CSS
//! 规则 = 运行时渲染空白，作者无法察觉（HTML 在浏览器预览里浏览器会套自己的 UA 表，
//! 看着正常，打包进 LoomGUI 却空）。
//!
//! 本 pass 在打包期（cascade resolve 之后）拦下这种写法：对每个控件节点，检查是否有
//! 任意 `<style>` 规则的选择器命中它本身（tag / class / id / 后代链落地在该节点）。
//! 完全无命中 → `FenceControlWithoutCss` error + 教学。
//!
//! 控件一律由 `role` 驱动（spec §2.2）：`<div role="...">`。教学文案按
//! **role/slot** 表述（`data-slot="fill"`、`role="listbox"`、`[aria-checked]` 属性
//! 选择器），不引用任何框架注入的 `.loom-*` 子节点。
//!
//! 选择器匹配消费 fence 的 IrTree（解析期产物），不依赖运行时 Node——复用 css_rules
//! 解析出的 `DynamicRule` 表，按 tag/class/id/attr 字面对照 IrElement 判定。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrElement, IrNodeKind, IrTree};
use loomgui_core::style::dynamic::{AttrOp, Compound, DynamicRule, ParsedSelector};

/// 触发本校验的控件 role（spec §2.2）。带这些 role 的元素必被检查；`textbox` 同时覆盖
/// TextField 与 TextArea（后者加 `aria-multiline="true"`）。
const CONTROL_ROLES: &[&str] = &[
    "combobox",
    "slider",
    "spinbutton",
    "switch",
    "radio",
    "progressbar",
    "textbox",
];

/// 读元素的 `role` 属性值（若存在）。
fn node_role(el: &IrElement) -> Option<&str> {
    el.attributes
        .iter()
        .find(|a| a.name == "role")
        .map(|a| a.value.as_str())
}

/// 判定元素是否为受校验控件：`role` 在 CONTROL_ROLES 即是（spec §2.2）。
fn is_control(el: &IrElement) -> bool {
    node_role(el).is_some_and(|r| CONTROL_ROLES.contains(&r))
}

/// compound（单段选择器，无空格）是否匹配 IrElement——tag/class/id/attr 字面对照。
///
/// 伪类（hover/active/...）不参与：本检查只问「作者是否在样式这个控件」，带状态的规则
/// （`progress:hover{}`）同样表明作者意图——只校 tag/class/id/attr 的静态部分。
fn compound_matches_element(c: &Compound, el: &IrElement) -> bool {
    if let Some(t) = &c.tag {
        if !t.eq_ignore_ascii_case(&el.tag) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        let node_id = el
            .attributes
            .iter()
            .find(|a| a.name == "id")
            .map(|a| a.value.as_str());
        if node_id != Some(id.as_str()) {
            return false;
        }
    }
    if !c.classes.is_empty() {
        let node_classes: Vec<&str> = el
            .attributes
            .iter()
            .find(|a| a.name == "class")
            .map(|a| a.value.split_whitespace().collect())
            .unwrap_or_default();
        for cls in &c.classes {
            if !node_classes.contains(&cls.as_str()) {
                return false;
            }
        }
    }
    for a in &c.attrs {
        let node_attr = el
            .attributes
            .iter()
            .find(|na| na.name.eq_ignore_ascii_case(&a.name));
        match a.op {
            AttrOp::Exists => {
                if node_attr.is_none() {
                    return false;
                }
            }
            AttrOp::Eq => match node_attr {
                Some(na) => {
                    if na.value != a.value.as_deref().unwrap_or("") {
                        return false;
                    }
                }
                None => return false,
            },
        }
    }
    true
}

/// 完整选择器是否命中 node_id：最后一段须命中 node 本身，前面各段沿祖先链匹配
/// （fence 子集只有后代组合——空格——parse_selector 拒 > + ~）。
fn selector_matches_node(sel: &ParsedSelector, tree: &IrTree, node_idx: usize) -> bool {
    let comps = &sel.compound;
    if comps.is_empty() {
        return false;
    }
    let last = &comps[comps.len() - 1];
    let last_el = match &tree.nodes[node_idx].kind {
        IrNodeKind::Element(e) => e,
        _ => return false,
    };
    if !compound_matches_element(last, last_el) {
        return false;
    }
    if comps.len() == 1 {
        return true;
    }
    match_ancestor_chain(comps, comps.len() - 1, node_idx, tree)
}

/// 递归匹配 comps[0..end_idx] 在 start_node 的祖先链上。
/// `start_node` 已命中 comps[end_idx]；为 comps[end_idx-1] 找祖先。
fn match_ancestor_chain(
    comps: &[Compound],
    end_idx: usize,
    start_node: usize,
    tree: &IrTree,
) -> bool {
    if end_idx == 0 {
        return true;
    }
    let target_comp = &comps[end_idx - 1];
    // fence 子集 combinator 恒为 Descendant（parse_selector 拒 Child/Adjacent），
    // 故沿祖先链逐层尝试，不做 child 直父限制。
    let mut cur = tree.nodes[start_node].parent;
    while let Some(ancestor) = cur {
        let matched = matches!(&tree.nodes[ancestor.0].kind, IrNodeKind::Element(anc_el)
            if compound_matches_element(target_comp, anc_el))
            && match_ancestor_chain(comps, end_idx - 1, ancestor.0, tree);
        if matched {
            return true;
        }
        cur = tree.nodes[ancestor.0].parent;
    }
    false
}

/// 任一规则的选择器命中 node_idx。
fn any_rule_matches(rules: &[DynamicRule], tree: &IrTree, node_idx: usize) -> bool {
    rules
        .iter()
        .any(|r| selector_matches_node(&r.selector, tree, node_idx))
}

/// 控件的可读名称（教学文案用）。按 `role` 取名（spec §2.2）。
fn kind_name_for(el: &IrElement) -> &'static str {
    match node_role(el) {
        Some("combobox") => "dropdown (combobox)",
        Some("slider") => "slider",
        Some("spinbutton") => "number field (spinbutton)",
        Some("switch") => "toggle (switch)",
        Some("radio") => "radio button",
        Some("progressbar") => "progress bar",
        Some("textbox") => "text field",
        _ => "control",
    }
}

/// 按控件生成「该怎么配 CSS」教学文案（role/slot 表述，spec §2.2）。
fn fix_hint_for(el: &IrElement) -> String {
    let tag = el.tag.as_str();
    match node_role(el) {
        Some("progressbar") => format!(
            "Provide CSS for <{tag}> (the track — e.g. a background/border) and for its \
             `data-slot=\"fill\"` child (the fill bar). Both elements need CSS; without it \
             the progress bar renders blank."
        ),
        Some("slider") => format!(
            "Provide CSS for <{tag}> (the track — e.g. a background/border) and for its \
             `data-slot=\"thumb\"` child (the draggable handle). A `data-slot=\"fill\"` \
             child is optional for the filled portion. All present elements need CSS."
        ),
        Some("combobox") => format!(
            "Provide CSS for <{tag}> (background/border so the box is visible), for its \
             `role=\"listbox\"` child (the popup list container), and for `role=\"option\"` \
             children (each list row). LoomGUI dropdowns have NO built-in arrow indicator — \
             if you want one, draw it yourself via CSS (e.g. a background-image on the box, \
             or an extra child element)."
        ),
        Some("switch") | Some("radio") => format!(
            "Provide CSS for <{tag}> (background/border so the control is visible). Use the \
             `[aria-checked]` attribute selector to style checked/unchecked states — there is \
             no separate check-mark child element."
        ),
        Some("textbox") => format!(
            "Provide CSS for <{tag}> (background/border and caret-color so the text field is \
             visible). Add `aria-multiline=\"true\"` for a multi-line text area."
        ),
        Some("spinbutton") => format!(
            "Provide CSS for <{tag}> (background/border and caret-color so the number field is \
             visible)."
        ),
        _ => format!("Provide CSS for <{tag}> so the control is visible."),
    }
}

/// 检查所有控件节点是否被至少一条 CSS 规则命中。返回诊断（error 列表）。
///
/// 入参：
/// - `tree`：IrTree（已过 Annotate，`IrElement.semantic` 已填充）
/// - `dynamic_rules`：Stage 4.5 解析出的 `<style>` 规则表
/// - `file` / `line_map`：定位诊断
pub fn check_control_css(
    tree: &IrTree,
    dynamic_rules: &[DynamicRule],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        if !is_control(el) {
            continue;
        }
        if any_rule_matches(dynamic_rules, tree, idx) {
            continue;
        }

        let tag = el.tag.as_str();
        let kind_name = kind_name_for(el);
        let fix_hint = fix_hint_for(el);
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceControlWithoutCss,
            format!(
                "LoomGUI {kind_name} element <{tag}> has no matching CSS rule. \
                 Controls have NO built-in default style — without CSS they render blank. \
                 {fix_hint} See docs/design/fence.md §control-css."
            ),
            line_map.source_location(node.span.start, file.to_string()),
        ));
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_template;

    /// 辅助：解析 HTML 后跑本检查（隔离单元测试，不经 pipeline 全程）。
    fn check(html: &str) -> Vec<Diagnostic> {
        let result = parse_template(html, "t.html");
        // 只取本检查产出的诊断（过滤掉其他 stage 的噪声，如 inline-context）。
        check_control_css(
            &result.tree,
            &result.dynamic_rules,
            "t.html",
            &crate::diagnostic::LineMap::new(html),
        )
    }

    #[test]
    fn compound_matches_by_tag() {
        let mut el = IrElement {
            tag: "div".into(),
            attributes: vec![],
            semantic: None,
        };
        let c = parse_compound("div");
        assert!(compound_matches_element(&c, &el));
        el.tag = "span".into();
        assert!(!compound_matches_element(&c, &el));
    }

    #[test]
    fn compound_matches_by_class() {
        let el = IrElement {
            tag: "div".into(),
            attributes: vec![attr("class", "hp big")],
            semantic: None,
        };
        assert!(compound_matches_element(&parse_compound(".hp"), &el));
        assert!(compound_matches_element(&parse_compound(".big"), &el));
        assert!(!compound_matches_element(&parse_compound(".x"), &el));
    }

    #[test]
    fn compound_matches_by_attr_eq() {
        let el = IrElement {
            tag: "div".into(),
            attributes: vec![attr("role", "slider")],
            semantic: None,
        };
        assert!(compound_matches_element(
            &parse_compound(r#"div[role="slider"]"#),
            &el
        ));
        assert!(!compound_matches_element(
            &parse_compound(r#"div[role="switch"]"#),
            &el
        ));
    }

    #[test]
    fn role_progressbar_without_css_errors() {
        let diags = check(r#"<div role="progressbar"></div>"#);
        assert_eq!(diags.len(), 1);
        // 文案应引导 data-slot="fill"（不再引用已删除的 .loom-fill）
        assert!(
            diags[0].message.contains("data-slot=\"fill\""),
            "{}",
            diags[0].message
        );
        assert!(
            !diags[0].message.contains(".loom-"),
            "不应再引用 .loom-*: {}",
            diags[0].message
        );
    }

    #[test]
    fn role_slider_without_css_errors() {
        let diags = check(r#"<div role="slider"></div>"#);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("data-slot=\"thumb\""),
            "{}",
            diags[0].message
        );
        assert!(!diags[0].message.contains(".loom-"), "{}", diags[0].message);
    }

    #[test]
    fn role_combobox_without_css_errors() {
        let diags = check(
            r#"<div role="combobox"><div role="listbox"><div role="option">A</div></div></div>"#,
        );
        assert_eq!(diags.len(), 1);
        // 文案应引导 role=listbox / role=option + 仍含「NO built-in arrow」教学点
        assert!(
            diags[0].message.contains("role=\"listbox\""),
            "{}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("NO built-in arrow"),
            "{}",
            diags[0].message
        );
        assert!(!diags[0].message.contains(".loom-"), "{}", diags[0].message);
    }

    #[test]
    fn role_switch_without_css_errors() {
        let diags = check(r#"<div role="switch"></div>"#);
        assert_eq!(diags.len(), 1);
        // switch / radio 无必需子节点：文案应引导 [aria-checked] 属性选择器
        assert!(
            diags[0].message.contains("[aria-checked]"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn role_textbox_without_css_errors() {
        let diags = check(r#"<div role="textbox"></div>"#);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("caret-color"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn role_control_with_matching_attr_selector_passes() {
        // [role="slider"] 属性选择器命中 role 驱动控件 → 放行
        let diags =
            check(r#"<style>[role="slider"]{background:#ddd}</style><div role="slider"></div>"#);
        assert!(diags.is_empty(), "{diags:?}");
    }

    fn attr(name: &str, value: &str) -> crate::ir::IrAttribute {
        crate::ir::IrAttribute {
            name: name.into(),
            value: value.into(),
            span: crate::ir::Span::default(),
        }
    }

    fn parse_compound(raw: &str) -> Compound {
        // parse_selector 产 ParsedSelector；单 compound 取 [0]。
        let sel = crate::css_rules::parse_selector(raw).unwrap_or_else(|| panic!("parse {raw:?}"));
        sel.compound.into_iter().next().expect("one compound")
    }
}
