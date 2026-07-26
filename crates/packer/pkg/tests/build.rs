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

/// build() 必须把围栏一致性 warning 收集进 BuildReport.warnings。
/// 修前 pack_components 丢弃 warning（只查 Error 级），build 也未暴露 → CLI/GUI
/// 都看不到 W1/W2，机制对作者名存实亡。本测锁住「warning 经 build 进报告」的链路。
#[test]
fn build_propagates_warnings_into_report() {
    let tmp = std::env::temp_dir().join("loom_build_warnings_test");
    let _ = std::fs::remove_dir_all(&tmp);

    let pkg_src_dir = tmp.join("ui/showcase");
    std::fs::create_dir_all(&pkg_src_dir).unwrap();
    // home.html 触发 W1（border-width 无 border-style）；无 img/background 引用 → 不走交叉验证。
    std::fs::write(
        pkg_src_dir.join("home.html"),
        r#"<div style="border-width:2px;border-color:#ff0000"></div>"#,
    )
    .unwrap();
    // shop.html 触发 W2（background-image 无 background-size）。url(a.png) 会被
    // extract_sprites 收进 referenced_sprites → 须提供同名 atlas 图，否则交叉验证拦。
    std::fs::write(
        pkg_src_dir.join("shop.html"),
        r#"<div style="background-image:url(a.png)"></div>"#,
    )
    .unwrap();
    // 与 shop.html 同目录的 sprite（sprite_key = ui/showcase/a.png）。
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 255, 0, 255]));
    img.save(pkg_src_dir.join("a.png")).unwrap();

    let json = r#"{
    "version": 1,
    "output_dir": "output",
    "packages": [{ "name": "showcase", "dirs": ["ui/showcase"], "html": [] }],
    "atlases": [{ "name": "ui", "default": true, "dirs": ["ui/showcase"] }],
    "fonts": []
}"#;
    std::fs::write(tmp.join("loom.workspace.json"), json).unwrap();

    let report = build(&tmp).expect("warning 不阻断打包：build 应返 Ok");
    assert!(
        report.packages.contains(&"showcase".to_string()),
        "pkg 仍正常产出（warning 未阻断）: {:?}",
        report.packages
    );
    // W1（home 组件）必须在报告里。
    let w1 = report
        .warnings
        .iter()
        .find(|w| w.code == "FenceBorderWithoutStyle" && w.component == "home")
        .expect("BuildReport.warnings 应含 W1（home）");
    assert_eq!(w1.file, "ui/showcase/home.html");
    assert!(w1.line >= 1);
    // W2（shop 组件）也必须在报告里（跨组件收集）。
    let w2 = report
        .warnings
        .iter()
        .find(|w| w.code == "FenceBgImageWithoutSize" && w.component == "shop")
        .expect("BuildReport.warnings 应含 W2（shop）");
    assert_eq!(w2.file, "ui/showcase/shop.html");

    let _ = std::fs::remove_dir_all(&tmp);
}

/// I-3 coverage: build() packages path (resolve_html_list dir scan + stem() +
/// pack_components + ui/<name>.pkg.bin write + runtime.packages fill-back) via its
/// real entry point. Existing tests all use `"packages": []`, leaving T5 orchestration
/// untested. This catches a silent regression in resolve_html_list/runtime ordering.
#[test]
fn build_e2e_packages_path_writes_pkg_bin_and_fills_runtime() {
    let tmp = std::env::temp_dir().join("loom_build_packages_e2e_test");
    let _ = std::fs::remove_dir_all(&tmp);

    // Workspace dir scanned by resolve_html_list (auto mode: pkg.html empty → scan pkg.dirs).
    let pkg_src_dir = tmp.join("ui/showcase");
    std::fs::create_dir_all(&pkg_src_dir).unwrap();
    // A real component HTML on disk (single div root, no image refs → no atlas cross-validate).
    std::fs::write(
        pkg_src_dir.join("home.html"),
        r#"<div class="root"><div>hi</div></div>"#,
    )
    .unwrap();

    // One package in auto mode (dirs scan, not explicit html list).
    let json = r#"{
    "version": 1,
    "output_dir": "output",
    "packages": [{ "name": "showcase", "dirs": ["ui/showcase"], "html": [] }],
    "atlases": [],
    "fonts": []
}"#;
    std::fs::write(tmp.join("loom.workspace.json"), json).unwrap();

    let report = build(&tmp).expect("build should succeed");
    assert!(
        report.packages.contains(&"showcase".to_string()),
        "report.packages missing showcase: {:?}",
        report.packages
    );

    let output = tmp.join("output");
    // ui/<name>.pkg.bin written + non-empty.
    let pkg_path = output.join("ui/showcase.pkg.bin");
    assert!(pkg_path.exists(), "ui/showcase.pkg.bin exists");
    assert!(
        std::fs::metadata(&pkg_path).unwrap().len() > 0,
        "pkg.bin should be non-empty"
    );

    // loom.runtime.json packages matches report.packages (fill-back ordering correct).
    let rt_path = output.join(RUNTIME_FILE);
    assert!(rt_path.exists(), "loom.runtime.json exists");
    let rt_text = std::fs::read_to_string(&rt_path).unwrap();
    let rt: RuntimeManifest = serde_json::from_str(&rt_text).unwrap();
    assert_eq!(
        rt.packages, report.packages,
        "runtime.packages must match report.packages (fill-back ordering)"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
