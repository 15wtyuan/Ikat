use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::ir::{IrNodeKind, IrTree};
use crate::schema::tag::{find_tag, Category, ContentModel};
use std::collections::HashSet;

/// Run Stage 5 (Structural): validate cross-element constraints.
///
/// Checks:
/// - Content model: child Category must be allowed by parent's ContentModel
/// - Text children rejected by parents that don't accept text
/// - ID uniqueness within the template scope
/// - ARIA IdRef attributes (aria-controls, aria-labelledby) reference existing IDs
pub fn run_structural(tree: &IrTree, file: &str, line_map: &LineMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_content_model(tree, file, line_map, &mut diagnostics);
    validate_id_uniqueness(tree, file, line_map, &mut diagnostics);

    // Deferred validation: reference checks need the full ID set.
    let all_ids = collect_all_ids(tree);
    validate_aria_relations(tree, file, line_map, &all_ids, &mut diagnostics);
    diagnostics
}

fn loc(file: &str, offset: usize, line_map: &LineMap) -> SourceLocation {
    line_map.source_location(offset, file.to_string())
}

fn validate_content_model(
    tree: &IrTree,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for parent_id in tree.all_element_ids() {
        let parent_tag = match &tree.nodes[parent_id.0].kind {
            IrNodeKind::Element(e) => e.tag.as_str(),
            _ => continue,
        };
        let parent_spec = match find_tag(parent_tag) {
            Some(s) => s,
            None => continue,
        };

        for &child_id in &tree.nodes[parent_id.0].children {
            match &tree.nodes[child_id.0].kind {
                IrNodeKind::Text(text) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    if matches!(
                        parent_spec.content,
                        ContentModel::None | ContentModel::Only(_)
                    ) {
                        diagnostics.push(Diagnostic::error(
                            DiagnosticCode::InvalidContentModel,
                            format!("<{}> does not accept text content", parent_tag),
                            loc(file, tree.nodes[child_id.0].span.start, line_map),
                        ));
                    }
                }
                IrNodeKind::Element(child_el) => {
                    let child_tag = child_el.tag.as_str();
                    let child_cat = find_tag(child_tag).map(|s| s.category);
                    if !is_child_allowed(&parent_spec.content, child_cat, child_tag) {
                        // 自定义元素（tag 带 '-'）是块级组件根，嵌进 phrasing 容器
                        // （span 等）必然违规——顺带教学 Web Components 的正确槽位写法。
                        let hint = if child_tag.contains('-') {
                            concat!(
                                " (custom elements are block-level component roots and cannot ",
                                "nest inside phrasing containers like <span>; to slot content ",
                                "into a component, put the slot attribute on a direct child: ",
                                "<my-widget><span slot=\"x\">...</span></my-widget>)"
                            )
                        } else {
                            ""
                        };
                        diagnostics.push(Diagnostic::error(
                            DiagnosticCode::InvalidContentModel,
                            format!(
                                "<{}> cannot appear inside <{}> (content model conflict){hint}",
                                child_tag, parent_tag
                            ),
                            loc(file, tree.nodes[child_id.0].span.start, line_map),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

fn is_child_allowed(
    parent_content: &ContentModel,
    child_category: Option<Category>,
    child_tag: &str,
) -> bool {
    match parent_content {
        ContentModel::None => false,
        ContentModel::Text => false, // text only, no child elements
        ContentModel::Phrasing => matches!(
            child_category,
            Some(Category::Phrasing) | Some(Category::Void) | Some(Category::Transparent)
        ),
        ContentModel::Flow => true,
        ContentModel::Transparent => true, // simplified: accept (proper impl walks ancestors)
        ContentModel::Only(allowed) => allowed.contains(&child_tag),
    }
}

fn validate_id_uniqueness(
    tree: &IrTree,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    for id in tree.all_element_ids() {
        let el = match &tree.nodes[id.0].kind {
            IrNodeKind::Element(e) => e,
            _ => continue,
        };
        if let Some(id_attr) = el.attributes.iter().find(|a| a.name == "id") {
            if !seen.insert(id_attr.value.clone()) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::DuplicateId,
                    format!("ID \"{}\" is defined more than once", id_attr.value),
                    loc(file, id_attr.span.start, line_map),
                ));
            }
        }
    }
}

/// Collect every element ID declared in the tree (for reference checks).
fn collect_all_ids(tree: &IrTree) -> HashSet<String> {
    let mut ids = HashSet::new();
    for id in tree.all_element_ids() {
        if let Some(el) = tree.element(id) {
            if let Some(id_attr) = el.attributes.iter().find(|a| a.name == "id") {
                ids.insert(id_attr.value.clone());
            }
        }
    }
    ids
}

/// ARIA attributes whose values are space-separated token lists of element IDs.
/// Each token must resolve to an existing element within the template scope.
const ARIA_IDREF_ATTRS: &[&str] = &["aria-controls", "aria-labelledby"];

/// Validate ARIA IdRef attributes -- every referenced ID must exist.
fn validate_aria_relations(
    tree: &IrTree,
    file: &str,
    line_map: &LineMap,
    all_ids: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for id in tree.all_element_ids() {
        let el = match &tree.nodes[id.0].kind {
            IrNodeKind::Element(e) => e,
            _ => continue,
        };
        for attr in &el.attributes {
            if !ARIA_IDREF_ATTRS.contains(&attr.name.as_str()) {
                continue;
            }
            for token in attr.value.split_whitespace() {
                if !all_ids.contains(token) {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidAriaRelation,
                        format!(
                            "{}=\"{}\" references ID \"{}\" which does not exist",
                            attr.name, attr.value, token
                        ),
                        loc(file, attr.span.start, line_map),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::LineMap;
    use crate::diagnostic::Severity;
    use crate::tree_builder::parse_html_to_ir;

    fn structural(html: &str) -> Vec<Diagnostic> {
        let (tree, _) = parse_html_to_ir(html);
        let lm = LineMap::new(html);
        run_structural(&tree, "test.html", &lm)
    }

    #[test]
    fn block_inside_phrasing_rejected() {
        // <span> has ContentModel::Phrasing, <div> is Block -> invalid
        let diags = structural(r#"<span><div>x</div></span>"#);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidContentModel));
    }

    #[test]
    fn flow_inside_div_accepted() {
        let diags = structural(r#"<div><span>ok</span><div>text</div></div>"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "div accepts Flow: {:?}", errors);
    }

    #[test]
    fn duplicate_id_reported() {
        let diags = structural(r#"<div id="x"></div><div id="x"></div>"#);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::DuplicateId));
    }
}
