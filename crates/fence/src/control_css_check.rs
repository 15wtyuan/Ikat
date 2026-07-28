//! Stage 6.7：控件必须被 CSS 命中校验。
//!
//! LoomGUI 控件（ProgressBar / Slider / Toggle / RadioButton + 文本控件
//! TextField / PasswordField / SearchField / TextArea）**不带 UA 默认样式**——
//! core 刻意保持纯净，不开「框架自带样式源」先例。代价：写了控件标签却没匹配的 CSS
//! 规则 = 运行时渲染空白，作者无法察觉（HTML 在浏览器预览里浏览器会套自己的 UA 表，
//! 看着正常，打包进 LoomGUI 却空）。
//!
//! 本 pass 在打包期（cascade resolve 之后）拦下这种写法：对每个控件节点，检查是否有
//! 任意 `<style>` 规则的选择器命中它本身（tag / class / id / 后代链落地在该节点）。
//! 完全无命中 → `FenceControlWithoutCss` error + 教学。
//!
//! 教学分支：进度/滑块/勾选控件靠框架运行时注入的 `.loom-*` 子节点呈现
//! （fill/track/thumb/check）→ 教学引导为子节点配样式；文本控件无注入子节点
//! （文本和光标由控件自身渲染）→ 教学引导为控件本身配 background/border + caret-color。
//!
//! 选择器匹配消费 fence 的 IrTree（解析期产物），不依赖运行时 Node——复用 css_rules
//! 解析出的 `DynamicRule` 表，按 tag/class/id/attr 字面对照 IrElement 判定。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrElement, IrNodeKind, IrTree};
use crate::schema::tag::SemanticKind;
use loomgui_core::style::dynamic::{AttrOp, Compound, DynamicRule, ParsedSelector};

/// 触发本校验的控件 SemanticKind。
///
/// 共同点：core 不带 UA 默认样式——写了标签却无匹配 CSS = 运行时空白。分两类：
/// - **注入子节点型**（ProgressBar/Slider/Toggle/RadioButton）：core 实例化时注入
///   `.loom-*` 视觉子节点（fill/track/thumb/check），作者须为控件本身 + 子节点配 CSS。
/// - **文本控件型**（TextField/PasswordField/SearchField/TextArea）：控件自身渲染
///   文本和光标，无注入子节点——作者须为控件本身配 background/border + caret-color。
const CONTROL_KINDS: &[SemanticKind] = &[
    SemanticKind::ProgressBar,
    SemanticKind::Slider,
    SemanticKind::Toggle,
    SemanticKind::RadioButton,
    SemanticKind::TextField,
    SemanticKind::PasswordField,
    SemanticKind::SearchField,
    SemanticKind::TextArea,
];

/// 控件是否为「注入子节点型」——core 运行时为其插入 `.loom-*` 视觉子节点。
/// 文本控件返回 false：它们自身渲染文本，不注入子节点，故教学文案不同。
fn has_injected_children(semantic: SemanticKind) -> bool {
    matches!(
        semantic,
        SemanticKind::ProgressBar
            | SemanticKind::Slider
            | SemanticKind::Toggle
            | SemanticKind::RadioButton
    )
}

/// 判定 semantic 是否为受校验控件。
fn is_control(semantic: Option<SemanticKind>) -> bool {
    semantic.is_some_and(|s| CONTROL_KINDS.contains(&s))
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

/// 控件标签名（用于教学文案）：progress / input。
fn control_tag_name(el: &IrElement) -> &str {
    // input 按 type 细分语义，但标签名统一是 "input"；教学文案用 input 即可
    // （value 部分 by semantic 单独展开 loom-* 子节点引导）。
    el.tag.as_str()
}

/// 按 SemanticKind 给出控件内部 `.loom-*` 子节点提示（教学文案用）。
fn loom_children_hint(semantic: SemanticKind) -> &'static str {
    match semantic {
        // progress 节点本身 = track；fill 子 = 填充条
        SemanticKind::ProgressBar => "`.loom-fill` (the fill bar)",
        // track 容器内 fill + thumb
        SemanticKind::Slider => "`.loom-track`, `.loom-fill`, `.loom-thumb`",
        // 勾选图标容器
        SemanticKind::Toggle | SemanticKind::RadioButton => "`.loom-check` (the check mark)",
        _ => "",
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
        if !is_control(el.semantic) {
            continue;
        }
        if any_rule_matches(dynamic_rules, tree, idx) {
            continue;
        }

        let semantic = el.semantic.unwrap();
        let tag = control_tag_name(el);
        let kind_name = match semantic {
            SemanticKind::ProgressBar => "progress bar",
            SemanticKind::Slider => "slider",
            SemanticKind::Toggle => "toggle (checkbox)",
            SemanticKind::RadioButton => "radio button",
            SemanticKind::TextField => "text field",
            SemanticKind::PasswordField => "password field",
            SemanticKind::SearchField => "search field",
            SemanticKind::TextArea => "text area",
            _ => "control",
        };
        // 教学分支：注入子节点型控件需为 .loom-* 子节点配样式；文本控件无子节点，
        // 靠控件本身的 background/border + caret-color 可见（文本和光标自绘）。
        let fix_hint = if has_injected_children(semantic) {
            let children = loom_children_hint(semantic);
            format!(
                "Provide CSS for <{tag}> (e.g. a background/border for the track or box) \
                 and for its internal {children} child element(s), which the framework \
                 injects at runtime."
            )
        } else {
            format!(
                "Provide CSS for <{tag}> (e.g. a background/border and caret-color so the \
                 text field is visible)."
            )
        };
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
            tag: "progress".into(),
            attributes: vec![],
            semantic: None,
        };
        let c = parse_compound("progress");
        assert!(compound_matches_element(&c, &el));
        el.tag = "div".into();
        assert!(!compound_matches_element(&c, &el));
    }

    #[test]
    fn compound_matches_by_class() {
        let el = IrElement {
            tag: "progress".into(),
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
            tag: "input".into(),
            attributes: vec![attr("type", "range")],
            semantic: None,
        };
        assert!(compound_matches_element(
            &parse_compound(r#"input[type="range"]"#),
            &el
        ));
        assert!(!compound_matches_element(
            &parse_compound(r#"input[type="text"]"#),
            &el
        ));
    }

    #[test]
    fn bare_progress_no_rules_errors() {
        let diags = check(r#"<progress value="1"></progress>"#);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::FenceControlWithoutCss);
    }

    #[test]
    fn progress_with_tag_rule_passes() {
        let diags =
            check(r#"<style>progress{background:#ddd}</style><progress value="1"></progress>"#);
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
