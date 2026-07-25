use crate::css_resolve::unsupported_hint;
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::ir::{IrElement, IrNode, IrNodeKind, IrTree, Span};
use crate::schema::attr::{find_structural_attr, is_content_attr, is_global_attr, AttrValueDomain};
use crate::schema::css::{find_css_prop, find_shorthand};
use crate::schema::tag::{find_tag, is_shell_tag};

/// Run Stage 3 (Fence Gate): validate every element against the schema.
///
/// Checks tag names, attribute names/values, and inline CSS property names.
/// No cross-element checks here (those are Stage 5).
///
/// `line_map` should be the same one built during tree building so that
/// diagnostics carry accurate line/column information.
pub fn run_fence_gate(tree: &IrTree, file: &str, line_map: &LineMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for id in tree.all_element_ids() {
        let node = &tree.nodes[id.0];
        if let IrNodeKind::Element(el) = &node.kind {
            validate_element(el, node, file, line_map, &mut diagnostics);
        }
    }
    diagnostics
}

fn loc(file: &str, offset: usize, line_map: &LineMap) -> SourceLocation {
    line_map.source_location(offset, file.to_string())
}

fn validate_element(
    element: &IrElement,
    node: &IrNode,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tag = element.tag.as_str();

    // 1. Tag name validation -- unknown tag (not in registry, not shell, not custom)
    if !is_shell_tag(tag) && find_tag(tag).is_none() && !tag.contains('-') {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceUnknownTag,
            format!("<{}> is not a recognized fence tag", tag),
            loc(file, node.span.start, line_map),
        ));
        return;
    }

    // 2. Attribute validation
    let tag_spec = find_tag(tag);
    for attr in &element.attributes {
        // Global attrs are always accepted
        if is_global_attr(&attr.name) {
            if attr.name == "style" {
                validate_inline_style(&attr.value, attr.span, file, line_map, diagnostics);
            }
            continue;
        }

        // Structural attrs -- validate value against domain
        if let Some(spec) = tag_spec.and_then(|ts| find_structural_attr(ts, &attr.name)) {
            validate_attr_value(
                &attr.name,
                &attr.value,
                &spec.values,
                attr.span,
                file,
                line_map,
                diagnostics,
            );
            continue;
        }

        // Content attrs -- just check name is in the tag's whitelist
        if let Some(ts) = tag_spec {
            if is_content_attr(ts, &attr.name) {
                continue;
            }
        }

        // Unknown attribute
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceUnknownAttr,
            format!("attribute \"{}\" is not recognized on <{}>", attr.name, tag),
            loc(file, attr.span.start, line_map),
        ));
    }
}

fn validate_attr_value(
    name: &str,
    value: &str,
    domain: &AttrValueDomain,
    span: Span,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match domain {
        AttrValueDomain::Enum(allowed) => {
            if !allowed.contains(&value) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceBadAttrValue,
                    format!(
                        "value \"{}\" for attribute \"{}\" is not allowed",
                        value, name
                    ),
                    loc(file, span.start, line_map),
                ));
            }
        }
        AttrValueDomain::IdRef | AttrValueDomain::FreeText | AttrValueDomain::Number => {
            // Stage 5 validates IdRef targets; FreeText and Number pass through here.
        }
    }
}

fn validate_inline_style(
    style: &str,
    span: Span,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let prop = decl.split(':').next().unwrap_or("").trim();
        if prop.is_empty() {
            continue;
        }
        if find_css_prop(prop).is_none() && find_shorthand(prop).is_none() {
            let hint = unsupported_hint(prop)
                .unwrap_or("not supported by fence — remove or replace with a supported property.");
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceUnknownCssProp,
                format!("CSS property \"{}\": {}", prop, hint),
                loc(file, span.start, line_map),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::LineMap;
    use crate::tree_builder::parse_html_to_ir_named;

    fn gate(html: &str) -> Vec<Diagnostic> {
        let (tree, _, _) = parse_html_to_ir_named(html, "test.html".into());
        let lm = LineMap::new(html);
        run_fence_gate(&tree, "test.html", &lm)
    }

    #[test]
    fn valid_tags_pass() {
        let diags = gate(r#"<div><span>ok</span></div>"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::diagnostic::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "valid tags should produce no errors: {:?}",
            errors
        );
    }

    #[test]
    fn unknown_tag_reported() {
        let diags = gate(r#"<video></video>"#);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceUnknownTag && d.message.contains("video")));
    }

    #[test]
    fn unknown_attr_reported() {
        let diags = gate(r#"<div bogus-attr="x"></div>"#);
        assert!(diags.iter().any(
            |d| d.code == DiagnosticCode::FenceUnknownAttr && d.message.contains("bogus-attr")
        ));
    }

    #[test]
    fn global_attr_accepted() {
        let diags = gate(r#"<div id="x" class="y" data-z="w" style="color:red"></div>"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::diagnostic::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "global attrs should be accepted: {:?}",
            errors
        );
    }

    #[test]
    fn bad_input_type_reported() {
        let diags = gate(r#"<input type="bogus">"#);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceBadAttrValue));
    }

    #[test]
    fn valid_input_type_accepted() {
        let diags = gate(r#"<input type="range">"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::diagnostic::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "type=range is valid: {:?}", errors);
    }
}
