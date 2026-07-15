use crate::annotate::annotate;
use crate::css_resolve::resolve_inline_styles_with_diags;
use crate::diagnostic::{Diagnostic, LineMap};
use crate::fence_gate::run_fence_gate;
use crate::ir::{IrNodeKind, IrTree};
use crate::structural::run_structural;
use crate::tree_builder::parse_html_to_ir_named;
use loomgui_core::style::mapping::parse_url;
use loomgui_core::style::resolved::ResolvedStyle;

/// Final output of the R1 parsing pipeline.
pub struct ParsedTemplate {
    pub tree: IrTree,
    pub styles: Vec<ResolvedStyle>,
    pub diagnostics: Vec<Diagnostic>,
    pub referenced_sprites: Vec<String>,
}

/// Full six-stage pipeline: Tokenize, Tree Build, Fence Gate, CSS Resolve,
/// Structural, Annotate.
///
/// Collects ALL diagnostics (does not fail-fast).
pub fn parse_template(html: &str, file: &str) -> ParsedTemplate {
    let line_map = LineMap::new(html);

    // Stage 1+2: Tokenize + Tree Build
    let (mut tree, mut diagnostics, _style_texts) = parse_html_to_ir_named(html, file.to_string());

    // Stage 3: Fence Gate (per-element validation)
    let gate_diags = run_fence_gate(&tree, file, &line_map);
    diagnostics.extend(gate_diags);

    // Stage 4: CSS Resolve
    let (styles, css_diags) = resolve_inline_styles_with_diags(&tree, file, &line_map);
    diagnostics.extend(css_diags);

    // Stage 5: Structural (content model, IDs)
    let struct_diags = run_structural(&tree, file, &line_map);
    diagnostics.extend(struct_diags);

    // Stage 6: Annotate (fill SemanticKind)
    annotate(&mut tree);

    // Extract referenced sprites (img src, background-image url)
    let referenced_sprites = extract_sprites(&tree);

    ParsedTemplate {
        tree,
        styles,
        diagnostics,
        referenced_sprites,
    }
}

fn extract_sprites(tree: &IrTree) -> Vec<String> {
    let mut sprites = Vec::new();
    for node in &tree.nodes {
        if let IrNodeKind::Element(el) = &node.kind {
            // img src
            if el.tag == "img" {
                if let Some(src) = el.attributes.iter().find(|a| a.name == "src") {
                    sprites.push(src.value.clone());
                }
            }
            // background-image: url(...) in inline style
            if let Some(style) = el.attributes.iter().find(|a| a.name == "style") {
                for decl in style.value.split(';') {
                    let decl = decl.trim();
                    if let Some(prop) = decl.split(':').next() {
                        if prop.trim() == "background-image" {
                            if let Some(value) = decl.split_once(':').map(|(_, v)| v.trim()) {
                                if let Some(url) = parse_url(value) {
                                    sprites.push(url);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    sprites
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tag::SemanticKind;

    #[test]
    fn pipeline_simple_template() {
        let result = parse_template(r#"<div id="root"><span>Hello</span></div>"#, "home.html");
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert_eq!(result.tree.roots.len(), 1);

        let root = result.tree.roots[0];
        let el = result.tree.element(root).unwrap();
        assert_eq!(el.tag, "div");
        assert_eq!(el.semantic, Some(SemanticKind::Container));

        let span_id = result.tree.nodes[root.0].children[0];
        let span_el = result.tree.element(span_id).unwrap();
        assert_eq!(span_el.semantic, Some(SemanticKind::TextElement));
    }

    #[test]
    fn pipeline_input_semantic() {
        let result = parse_template(r#"<input type="range">"#, "form.html");
        assert!(result.diagnostics.is_empty());
        let el = result.tree.element(result.tree.roots[0]).unwrap();
        assert_eq!(el.semantic, Some(SemanticKind::Slider));
    }

    #[test]
    fn pipeline_collects_all_errors() {
        let result = parse_template(
            r#"<video></video><div bogus="x" style="z-index:5"></div>"#,
            "bad.html",
        );
        // Should have multiple errors, not just the first
        assert!(
            result.diagnostics.len() >= 2,
            "should collect all errors, got: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn pipeline_referenced_sprites() {
        let result = parse_template(r#"<img src="icons/home.png">"#, "view.html");
        assert!(result
            .referenced_sprites
            .contains(&"icons/home.png".to_string()));
    }
}
