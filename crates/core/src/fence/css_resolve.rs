#![cfg(feature = "parse")]

use crate::fence::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::fence::ir::{IrNodeKind, IrTree};
use crate::fence::schema::css::{find_css_prop, find_shorthand, CssValueParser};
use crate::fence::schema::tag::{find_tag, DisplayDefault};
use crate::style::mapping::apply_decl;
use crate::style::resolved::{DisplayMode, ResolvedStyle};

/// Resolve inline styles for all nodes in the tree.
///
/// Returns one `ResolvedStyle` per node, in node-index order.
/// Uses the existing `apply_decl` for value application, but validates
/// property names and keyword values against the CSS schema first.
pub fn resolve_inline_styles(tree: &IrTree) -> Vec<ResolvedStyle> {
    resolve_inline_styles_with_diags(tree, "<inline>", &LineMap::new("")).0
}

/// Resolve inline styles, also returning diagnostics for invalid CSS.
pub fn resolve_inline_styles_with_diags(
    tree: &IrTree,
    file: &str,
    line_map: &LineMap,
) -> (Vec<ResolvedStyle>, Vec<Diagnostic>) {
    let mut styles: Vec<ResolvedStyle> = (0..tree.nodes.len())
        .map(|_| ResolvedStyle::default())
        .collect();
    let mut diagnostics = Vec::new();

    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };

        let mut flex_direction_set = false;

        // Apply DisplayDefault from schema (overrides ResolvedStyle::default
        // which hardcodes Flex + Column for legacy reasons).
        if let Some(spec) = find_tag(&el.tag) {
            match spec.display {
                DisplayDefault::Block => {
                    styles[idx].display_mode = DisplayMode::Block;
                }
                DisplayDefault::Inline => {
                    styles[idx].display_mode = DisplayMode::Flex;
                    // inline -> flex for taffy compatibility; flex-direction
                    // stays Row (taffy default) per CSS standard.
                }
                DisplayDefault::None => {
                    styles[idx].display_mode = DisplayMode::None;
                }
            }
        }

        // Apply inline style declarations
        if let Some(style_attr) = el.attributes.iter().find(|a| a.name == "style") {
            for decl in style_attr.value.split(';') {
                let decl = decl.trim();
                if decl.is_empty() {
                    continue;
                }
                let (prop, value) = match decl.split_once(':') {
                    Some((p, v)) => (p.trim(), v.trim()),
                    None => continue,
                };

                // Validate property name
                let is_known = find_css_prop(prop).is_some() || find_shorthand(prop).is_some();
                if !is_known {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceUnknownCssProp,
                        format!("CSS property \"{}\" is not in the fence", prop),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                    continue;
                }

                // Validate keyword values against schema
                if let Some(spec) = find_css_prop(prop) {
                    if let CssValueParser::Keyword(allowed) = &spec.parser {
                        if !allowed.contains(&value) {
                            diagnostics.push(Diagnostic::error(
                                DiagnosticCode::FenceBadCssValue,
                                format!(
                                    "value \"{}\" is not valid for CSS property \"{}\"",
                                    value, prop
                                ),
                                line_map.source_location(node.span.start, file.to_string()),
                            ));
                            continue;
                        }
                    }
                }

                // Track explicit flex-direction
                if prop == "flex-direction" {
                    flex_direction_set = true;
                }

                // Apply using existing apply_decl.
                // If it returns false, the value failed to parse — report it.
                if !apply_decl(&mut styles[idx], prop, value) {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceBadCssValue,
                        format!(
                            "value \"{}\" is not valid for CSS property \"{}\"",
                            value, prop
                        ),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                }
            }
        }

        // CSS spec: flex-direction initial value is row.
        // ResolvedStyle::default() hardcodes Column (legacy).
        // If display ended up as Flex and no explicit flex-direction was
        // applied, override to Row per CSS standard.
        if styles[idx].display_mode == DisplayMode::Flex && !flex_direction_set {
            styles[idx].taffy_style.flex_direction = taffy::FlexDirection::Row;
        }
    }

    (styles, diagnostics)
}

/// Private helper for tests: resolve without file/line_map (uses empty).
#[cfg(test)]
fn resolve_for_test(tree: &IrTree) -> Vec<ResolvedStyle> {
    resolve_inline_styles_with_diags(tree, "<inline>", &LineMap::new("")).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fence::tree_builder::parse_html_to_ir;

    #[test]
    fn inline_style_applies_color() {
        let (tree, _) = parse_html_to_ir(r#"<div style="color:#ff0000"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn display_block_overrides_default() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:block"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].display_mode, DisplayMode::Block);
    }

    #[test]
    fn display_grid_reports_error() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:grid"></div>"#);
        let (_, diags) = resolve_inline_styles_with_diags(&tree, "test.html", &LineMap::new(""));
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceBadCssValue));
    }

    #[test]
    fn flex_defaults_to_row_direction() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:flex"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].taffy_style.flex_direction,
            taffy::FlexDirection::Row
        );
    }

    #[test]
    fn explicit_flex_direction_preserved() {
        let (tree, _) =
            parse_html_to_ir(r#"<div style="display:flex; flex-direction:column"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].taffy_style.flex_direction,
            taffy::FlexDirection::Column
        );
    }
}
