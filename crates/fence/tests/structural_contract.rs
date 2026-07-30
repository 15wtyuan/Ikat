//! Contract tests for deferred structural validation: ARIA IdRef references.
//!
//! `label[for]` and `<template>`-root-inside-list checks were retired with the
//! `label`/`ul`/`ol`/`li` tags: `label` left the fence earlier, and list→listitem
//! structure is now validated by `control_structure_check` via `role`. Only the
//! ARIA IdRef relation check (aria-controls / aria-labelledby) remains here.

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
