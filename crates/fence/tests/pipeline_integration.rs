use loomgui_fence::diagnostic::{DiagnosticCode, Severity};
use loomgui_fence::ir::IrNodeKind;
use loomgui_fence::pipeline::parse_template;
use loomgui_fence::schema::tag::SemanticKind;

#[test]
fn complex_template_parses_clean() {
    let html = r#"<style>input[type="range"]{width:100%}</style><div id="root" class="panel">
        <div><my-title>Title</my-title>
            <button class="close" style="display:block">X</button>
        </div>
        <ul>
            <li><span>Item 1</span></li>
            <li><span>Item 2</span></li>
        </ul>
        <input type="range" min="0" max="100" style="display:block">
    </div>"#;
    let result = parse_template(html, "complex.html");
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    // my-title is a custom element (contains '-') -- should be accepted
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Verify semantic annotation
    let root = result.tree.roots[0];
    assert_eq!(
        result.tree.element(root).unwrap().semantic,
        Some(SemanticKind::Container)
    );

    // Find the input and check it's a Slider
    for node in &result.tree.nodes {
        if let IrNodeKind::Element(el) = &node.kind {
            if el.tag == "input" {
                assert_eq!(el.semantic, Some(SemanticKind::Slider));
            }
            if el.tag == "ul" {
                assert_eq!(el.semantic, Some(SemanticKind::ListView));
            }
        }
    }
}

#[test]
fn fence_out_tags_reported() {
    let result = parse_template(r#"<video src="x.mp4"></video>"#, "bad.html");
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::FenceUnknownTag));
}

#[test]
fn multiple_errors_collected() {
    let html = r#"<video></video><audio></audio><h4>x</h4>"#;
    let result = parse_template(html, "multi.html");
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::FenceUnknownTag)
        .collect();
    assert!(
        errors.len() >= 3,
        "should report all 3 unknown tags, got {}",
        errors.len()
    );
}

#[test]
fn rich_text_mixed_children() {
    let html = r#"<div>Hello <span>bold</span> and <span>italic</span>!</div>"#;
    let result = parse_template(html, "rich.html");
    assert!(
        result.diagnostics.is_empty(),
        "rich text should parse clean: {:?}",
        result.diagnostics
    );
    let root = result.tree.roots[0];
    // div should have 5 children: Text, span, Text, span, Text
    assert_eq!(result.tree.nodes[root.0].children.len(), 5);
}

#[test]
fn display_grid_rejected() {
    let result = parse_template(r#"<div style="display:grid"></div>"#, "grid.html");
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::FenceBadCssValue));
}
