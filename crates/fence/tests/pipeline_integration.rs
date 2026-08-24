use loomgui_fence::diagnostic::{DiagnosticCode, Severity};
use loomgui_fence::ir::IrNodeKind;
use loomgui_fence::pipeline::parse_template;
use loomgui_fence::schema::tag::SemanticKind;

#[test]
fn complex_template_parses_clean() {
    // role-driven controls + data-driven list (template blueprint). Controls need
    // a matching CSS rule (no UA defaults), so a `[role]` selector covers them.
    let html = r#"<style>[role="slider"],[role="list"]{background:#ddd} [data-slot="thumb"]{background:#444}</style><div id="root" class="panel">
        <div><my-title>Title</my-title>
            <button class="close" style="display:block">X</button>
        </div>
        <div role="list" data-fill="3">
            <template><div role="listitem" class="item"><span>Item</span></div></template>
        </div>
        <div role="slider" aria-valuenow="50" data-step="1"><div data-slot="thumb"></div></div>
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

    // Find the role-driven controls and check their semantics
    for node in &result.tree.nodes {
        if let IrNodeKind::Element(el) = &node.kind {
            if el.tag == "div"
                && el
                    .attributes
                    .iter()
                    .any(|a| a.name == "role" && a.value == "slider")
            {
                assert_eq!(el.semantic, Some(SemanticKind::Slider));
            }
            if el
                .attributes
                .iter()
                .any(|a| a.name == "role" && a.value == "list")
            {
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

/// `<style>` 规则里的好渐变（多 stop / 任意角度 / radial 全形）零 diagnostic。
#[test]
fn style_rule_gradient_subset_parses_clean() {
    let html = r#"<style>
        .g1 { background-image: linear-gradient(to right, #ff0000, #00ff00, #0000ff) }
        .g2 { background: linear-gradient(137deg, #ff0000, transparent 60%) }
        .g3 { background: radial-gradient(1100px 560px at 82% -12%, rgba(95,180,212,0.10), transparent 60%) }
        .g4 { background-image: radial-gradient(circle closest-side, #fff, #000) }
        .g5 { background-image: none }
        .g6 { background-image: url(res/icons/logo.png) }
    </style><div class="g1"></div>"#;
    let result = parse_template(html, "grad.html");
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    // g1 规则应保留（合法值不因探针被丢）。
    assert!(
        result
            .dynamic_rules
            .iter()
            .any(|r| r.declarations.iter().any(|d| d.value.contains("#00ff00"))),
        "合法多 stop 渐变规则保留"
    );
}

/// `<style>` 规则里的坏渐变（探针捕获）：conic / repeating / 超 8 stops / 坏 radial 配置
/// → 打包期 FenceBadCssValue（原先运行时静默丢背景）。
#[test]
fn style_rule_bad_gradient_reported_at_pack_time() {
    let html = r#"<style>
        .bad1 { background: conic-gradient(#ff0000, #0000ff) }
        .bad2 { background-image: repeating-linear-gradient(to right, #fff, #000) }
        .bad3 { background-image: linear-gradient(to right, #111111, #222222, #333333, #444444, #555555, #666666, #777777, #888888, #999999) }
        .bad4 { background: radial-gradient(circle at, #fff, #000) }
    </style><div class="bad1"></div>"#;
    let result = parse_template(html, "badgrad.html");
    let bad: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::FenceBadCssValue)
        .collect();
    assert_eq!(bad.len(), 4, "4 条坏渐变各自报错，实 {bad:?}");
    // 坏值不进规则表（防运行时静默）。
    assert!(!result
        .dynamic_rules
        .iter()
        .any(|r| r.declarations.iter().any(|d| d.value.contains("conic"))));
}

/// inline style 坏渐变照旧报错（apply_decl false 路径，回归保障）。
#[test]
fn inline_bad_gradient_reported() {
    let result = parse_template(
        r#"<div style="background-image: conic-gradient(#fff, #000)"></div>"#,
        "inline.html",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceBadCssValue),
        "inline conic-gradient 应报 FenceBadCssValue"
    );
}
