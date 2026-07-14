//! End-to-end tests: build() produces atlas + font + runtime artifacts.

use loomgui_pkg::build::build;
use loomgui_pkg::runtime::{RuntimeManifest, RUNTIME_FILE};
use std::path::Path;

fn write_workspace_json(root: &Path, atlases: &str, fonts: &str) {
    let json = format!(
        r#"{{
    "version": 1,
    "output_dir": "output",
    "packages": [],
    "atlases": {atlases},
    "fonts": {fonts}
}}"#
    );
    std::fs::write(root.join("loom.workspace.json"), json).unwrap();
}

#[test]
fn build_e2e_produces_all_artifacts() {
    let tmp = std::env::temp_dir().join("loom_build_e2e_test");
    let _ = std::fs::remove_dir_all(&tmp);

    // Create workspace structure.
    std::fs::create_dir_all(tmp.join("assets")).unwrap();
    std::fs::create_dir_all(tmp.join("fonts")).unwrap();

    write_workspace_json(
        &tmp,
        r#"[{ "name": "ui", "default": true, "dirs": ["assets"] }]"#,
        r#"[{ "family": "TestFont", "file": "fonts/f.ttf", "default": true, "fallback": false }]"#,
    );

    // assets/home.png (4x4 RGBA).
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
    img.save(tmp.join("assets/home.png")).unwrap();

    // fonts/f.ttf (stub bytes).
    std::fs::write(tmp.join("fonts/f.ttf"), b"fake font data").unwrap();

    // Build.
    let report = build(&tmp).expect("build should succeed");
    assert!(report.atlases.contains(&"ui".to_string()));
    assert!(!report.fonts.is_empty());

    let output = tmp.join("output");

    // Atlas artifacts.
    assert!(output.join("atlas/ui.png").exists(), "atlas/ui.png exists");
    assert!(
        output.join("atlas/ui.atlas.json").exists(),
        "atlas/ui.atlas.json exists"
    );
    let atlas_text = std::fs::read_to_string(output.join("atlas/ui.atlas.json")).unwrap();
    let atlas_m: loomgui_pkg::atlas::AtlasManifest = serde_json::from_str(&atlas_text).unwrap();
    assert!(
        atlas_m.sprites.contains_key("assets/home.png"),
        "sprite_key 'assets/home.png' in atlas manifest"
    );
    let entry = &atlas_m.sprites["assets/home.png"];
    assert_eq!(entry.orig, [4, 4], "orig matches source image size");

    // Font artifact.
    assert!(
        output.join("fonts/f.ttf.bytes").exists(),
        "fonts/f.ttf.bytes exists"
    );

    // Runtime manifest.
    let rt_path = output.join(RUNTIME_FILE);
    assert!(rt_path.exists(), "loom.runtime.json exists");
    let rt_text = std::fs::read_to_string(&rt_path).unwrap();
    let rt: RuntimeManifest = serde_json::from_str(&rt_text).unwrap();
    assert!(
        rt.atlases.contains(&"ui".to_string()),
        "runtime has ui atlas"
    );
    assert!(!rt.fonts.is_empty(), "runtime fonts non-empty");

    // Cleanup.
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_fails_when_font_file_missing() {
    let tmp = std::env::temp_dir().join("loom_build_missing_font_test");
    let _ = std::fs::remove_dir_all(&tmp);

    std::fs::create_dir_all(tmp.join("assets")).unwrap();

    write_workspace_json(
        &tmp,
        r#"[{ "name": "ui", "default": true, "dirs": ["assets"] }]"#,
        r#"[{ "family": "Missing", "file": "fonts/ghost.ttf", "default": true, "fallback": false }]"#,
    );

    let result = build(&tmp);
    assert!(
        result.is_err(),
        "build should fail when font file does not exist"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_fails_when_output_dir_empty() {
    let tmp = std::env::temp_dir().join("loom_build_no_output_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let json = r#"{"version":1,"output_dir":"","packages":[],"atlases":[],"fonts":[]}"#;
    std::fs::write(tmp.join("loom.workspace.json"), json).unwrap();

    let result = build(&tmp);
    assert!(result.is_err(), "build should fail with empty output_dir");

    let _ = std::fs::remove_dir_all(&tmp);
}
