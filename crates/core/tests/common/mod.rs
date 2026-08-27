//! Shared test helpers for integration tests. Each tests/*.rs uses `mod common; use common::*;`.
#![allow(dead_code)]
pub use ikat_core::parse::css::parse_css;
pub use ikat_core::parse::dom::parse_html;
pub use ikat_core::scene::node::build_scene;
pub use ikat_core::stage::Stage;
pub use ikat_core::style::cascade::resolve_styles;

/// Test font path: repo-internal DejaVuSans.ttf, cross-platform consistent.
pub fn font_path() -> String {
    format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Skip (return true) if font fixture missing.
pub fn skip_if_no_font(font: &str) -> bool {
    if std::fs::read(font).is_err() {
        eprintln!("skip: no font at {}", font);
        return true;
    }
    false
}

/// HTML+CSS -> scene (parse_html + resolve_styles + build_scene), injected into Stage.
pub fn load_html_css(stage: &mut Stage, html: &str, css: &str) {
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    stage.tweens.clear();
    if let Some(scene) = stage.scene.as_mut() {
        scene.scroll.clear();
    }
    stage.prev_node_hashes.clear();
    stage.scene = Some(build_scene(&tree, &styles));
}
