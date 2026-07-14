//! Contract tests for deferred structural validation:
//! ARIA references, template root inside list views, and label[for].

use loomgui_fence::diagnostic::{Diagnostic, DiagnosticCode, LineMap, Severity};
use loomgui_fence::structural::run_structural;
use loomgui_fence::tree_builder::parse_html_to_ir;

fn structural(html: &str) -> Vec<Diagnostic> {
    let (tree, _) = parse_html_to_ir(html);
    let lm = LineMap::new(html);
    run_structural(&tree, "test.html", &lm)
}

fn errors(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ---------------------------------------------------------------------------
// label[for]
// ---------------------------------------------------------------------------

#[test]
fn label_for_valid_target() {
    let diags = structural(r#"<label for="name">Name</label><input id="name" type="text">"#);
    assert!(
        errors(&diags).is_empty(),
        "valid label[for] should pass: {:?}",
        diags
    );
}

#[test]
fn label_for_missing_target() {
    let diags = structural(r#"<label for="ghost">Name</label>"#);
    let idref = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::InvalidIdRef);
    assert!(
        idref.is_some(),
        "should report InvalidIdRef for label[for] with missing target: {:?}",
        diags
    );
    let d = idref.unwrap();
    assert!(
        d.message.contains("ghost"),
        "message should mention the missing id: {}",
        d.message
    );
}

#[test]
fn label_for_empty_value() {
    let diags = structural(r#"<label for="">Name</label>"#);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::InvalidIdRef),
        "empty for value should be reported: {:?}",
        diags
    );
}

#[test]
fn label_without_for_passes() {
    let diags = structural(r#"<label>Name</label>"#);
    assert!(
        errors(&diags).is_empty(),
        "label without for should be fine: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// aria-controls / aria-labelledby
// ---------------------------------------------------------------------------

#[test]
fn aria_controls_valid_target() {
    let diags =
        structural(r#"<button aria-controls="panel">Toggle</button><div id="panel"></div>"#);
    assert!(
        errors(&diags).is_empty(),
        "valid aria-controls should pass: {:?}",
        diags
    );
}

#[test]
fn aria_controls_missing_target() {
    let diags = structural(r#"<button aria-controls="phantom">Toggle</button>"#);
    let aria = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::InvalidAriaRelation);
    assert!(
        aria.is_some(),
        "should report InvalidAriaRelation for missing aria-controls target: {:?}",
        diags
    );
    assert!(
        aria.unwrap().message.contains("phantom"),
        "message should mention the missing id"
    );
}

#[test]
fn aria_labelledby_valid_target() {
    let diags = structural(r#"<div aria-labelledby="title"><span id="title">Hello</span></div>"#);
    assert!(
        errors(&diags).is_empty(),
        "valid aria-labelledby should pass: {:?}",
        diags
    );
}

#[test]
fn aria_labelledby_missing_target() {
    let diags = structural(r#"<div aria-labelledby="ghost"></div>"#);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidAriaRelation),
        "should report InvalidAriaRelation: {:?}",
        diags
    );
}

#[test]
fn aria_controls_multiple_targets_one_missing() {
    let diags = structural(r#"<button aria-controls="a ghost">X</button><div id="a"></div>"#);
    let aria_errors: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::InvalidAriaRelation)
        .collect();
    assert_eq!(
        aria_errors.len(),
        1,
        "should report exactly one error for the missing 'ghost': {:?}",
        diags
    );
    assert!(aria_errors[0].message.contains("ghost"));
}

#[test]
fn aria_non_idref_attr_not_checked() {
    // aria-label is free text, not an IdRef -- should not be validated
    let diags = structural(r#"<div aria-label="Settings"></div>"#);
    assert!(
        errors(&diags).is_empty(),
        "aria-label should not be treated as IdRef: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// template root inside ul/ol
// ---------------------------------------------------------------------------

#[test]
fn template_root_li_in_ul_valid() {
    let diags = structural(r#"<ul><template><li>item</li></template></ul>"#);
    assert!(
        errors(&diags).is_empty(),
        "template with li root in ul is valid: {:?}",
        diags
    );
}

#[test]
fn template_root_li_in_ol_valid() {
    let diags = structural(r#"<ol><template><li>item</li></template></ol>"#);
    assert!(
        errors(&diags).is_empty(),
        "template with li root in ol is valid: {:?}",
        diags
    );
}

#[test]
fn template_root_not_li_in_ul() {
    let diags = structural(r#"<ul><template><div>item</div></template></ul>"#);
    let troot = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::InvalidTemplateRoot);
    assert!(
        troot.is_some(),
        "should report InvalidTemplateRoot when template root is not li: {:?}",
        diags
    );
    let d = troot.unwrap();
    assert!(
        d.message.contains("li"),
        "message should suggest li: {}",
        d.message
    );
    assert!(
        d.message.contains("div"),
        "message should mention the actual root tag: {}",
        d.message
    );
}

#[test]
fn template_no_element_children_in_ul() {
    let diags = structural(r#"<ul><template></template></ul>"#);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidTemplateRoot),
        "empty template in ul should be reported: {:?}",
        diags
    );
}

#[test]
fn template_outside_list_not_checked() {
    // template outside ul/ol doesn't need li root
    let diags = structural(r#"<div><template><div>card</div></template></div>"#);
    assert!(
        diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::InvalidTemplateRoot)
            .count()
            == 0,
        "template outside list should not be checked: {:?}",
        diags
    );
}
