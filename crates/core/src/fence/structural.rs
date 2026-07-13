#![cfg(feature = "parse")]

use crate::fence::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::fence::ir::{IrNodeKind, IrTree};
use crate::fence::schema::tag::{find_tag, Category, ContentModel};
use std::collections::HashSet;

/// Run Stage 5 (Structural): validate cross-element constraints.
///
/// Checks:
/// - Content model: child Category must be allowed by parent's ContentModel
/// - Text children rejected by parents that don't accept text
/// - ID uniqueness within the template scope
pub fn run_structural(tree: &IrTree, file: &str, line_map: &LineMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_content_model(tree, file, line_map, &mut diagnostics);
    validate_id_uniqueness(tree, file, line_map, &mut diagnostics);
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
                IrNodeKind::Text(_) => {
                    // Text nodes: reject if parent does not accept text
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
                        diagnostics.push(Diagnostic::error(
                            DiagnosticCode::InvalidContentModel,
                            format!(
                                "<{}> cannot appear inside <{}> (content model conflict)",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fence::diagnostic::LineMap;
    use crate::fence::diagnostic::Severity;
    use crate::fence::tree_builder::parse_html_to_ir;

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
        let diags = structural(r#"<div><span>ok</span><p>text</p></div>"#);
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

    #[test]
    fn select_only_accepts_option() {
        let diags = structural(r#"<select><option>a</option></select>"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "select > option is valid: {:?}", errors);
    }

    #[test]
    fn select_rejects_div() {
        let diags = structural(r#"<select><div>x</div></select>"#);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidContentModel));
    }
}
