//! Stage 6.4 rich-text-block classification + mixed inline/block error.
//!
//! These tests drive the full pipeline (`parse_template`) and assert on
//! `ParsedTemplate.rich_text_blocks` + `FenceMixedInlineBlock` diagnostics,
//! mirroring how downstream stages (6.5 img exemption, packer bridge) consume
//! the classification.

use loomgui_fence::diagnostic::DiagnosticCode;
use loomgui_fence::pipeline::parse_template;

fn mixed_diags(html: &str) -> Vec<String> {
    let out = parse_template(html, "t.html");
    out.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::FenceMixedInlineBlock)
        .map(|d| d.message.clone())
        .collect()
}

/// span(inline) + div(block) 直接子混在 block div 里 → FenceMixedInlineBlock error。
#[test]
fn mixed_inline_block_in_block_container_errors() {
    let d = mixed_diags("<div><span>x</span><div>y</div></div>");
    assert_eq!(d.len(), 1, "mixed direct children must error: {d:?}");
}

/// 全 inline 直接子（text + span + img）→ 标 rich-text-block，不报 mixed。
#[test]
fn all_inline_classified_no_error() {
    let out = parse_template(
        r#"<div>text <span>x</span> <img src="a.png"></div>"#,
        "t.html",
    );
    assert!(out
        .diagnostics
        .iter()
        .all(|d| d.code != DiagnosticCode::FenceMixedInlineBlock));
    // 根 div 是 rich-text-block。
    assert!(out.rich_text_blocks.contains(&out.tree.roots[0].0));
}

/// 全 block 直接子 → 不标 rich-text-block。
#[test]
fn all_block_children_not_classified() {
    let out = parse_template("<div><div>a</div><div>b</div></div>", "t.html");
    assert!(!out.rich_text_blocks.contains(&out.tree.roots[0].0));
}

/// display:flex 容器即便全 inline 子也不当 rich-text-block（子是 flex item）。
#[test]
fn flex_container_not_classified() {
    let out = parse_template(
        r#"<div style="display:flex"><span>a</span><span>b</span></div>"#,
        "t.html",
    );
    assert!(!out.rich_text_blocks.contains(&out.tree.roots[0].0));
}
