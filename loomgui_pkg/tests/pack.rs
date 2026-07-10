use loomgui_core::asset::{read_package, PKG_MAGIC};
use loomgui_pkg::pack;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

/// 建临时 workspace_root：写每个 html_name（无扩展名）对应 `<name>.html`，
/// 同时建 res_files 相对路径的空占位文件。
/// res_files 形如 `["images/x.png", "images/icons/skin.png"]` —— 相对 workspace_root。
fn make_ws_dir_with_html(html_names: &[&str], res_files: &[&str]) -> PathBuf {
    let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("loomgui_pkg_t6_{}_{}", std::process::id(), seq));
    fs::create_dir_all(&dir).unwrap();
    for name in html_names {
        let html = format!(r#"<div class="{}"><span>hi</span></div>"#, name);
        fs::write(dir.join(format!("{name}.html")), html).unwrap();
    }
    for rf in res_files {
        let p = dir.join(rf);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // 空占位文件
        fs::write(&p, b"").unwrap();
    }
    dir
}

/// 将 html_names 和 workspace_root 转成 pack() 要求的 [(rel_path, abs_path)]。
fn html_pairs(ws_root: &std::path::Path, names: &[&str]) -> Vec<(String, PathBuf)> {
    names
        .iter()
        .map(|n| (format!("{n}.html"), ws_root.join(format!("{n}.html"))))
        .collect()
}

#[test]
fn pack_multi_html_collects_referenced_sprites() {
    // workspace_root：a.html + b.html + images/x.png
    // a.html 引用 <img src="images/x.png"> → sprite_key = "images/x.png"
    let dir = make_ws_dir_with_html(&["a", "b"], &["images/x.png"]);
    fs::write(
        dir.join("a.html"),
        r#"<div class="a"><img src="images/x.png"></div>"#,
    )
    .unwrap();
    let packed = pack(&dir, "test", &html_pairs(&dir, &["a", "b"])).expect("pack ok");
    assert!(!packed.pkg_bytes.is_empty(), "pkg_bytes 非空");
    assert!(
        packed
            .referenced_sprites
            .contains(&"images/x.png".to_string()),
        "referenced_sprites 含 images/x.png"
    );
    assert_eq!(
        u32::from_le_bytes(packed.pkg_bytes[0..4].try_into().unwrap()),
        PKG_MAGIC
    );
    let pkg = read_package(&packed.pkg_bytes).expect("read ok");
    assert_eq!(pkg.components.len(), 2, "两 HTML → 两组件");
    assert!(
        pkg.components.contains_key("a"),
        "组件名 a（文件名去 .html）"
    );
    assert!(pkg.components.contains_key("b"), "组件名 b");
    // pkg.bin 内 asset_manifest 段已删除（Task 10），图尺寸改走 atlas.json
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_resolves_img_src_relative_to_html_file() {
    // HTML 在 workspace root 下，img src="images/icons/skin.png"
    // → sprite_key = "images/icons/skin.png"
    let dir = make_ws_dir_with_html(&["c"], &["images/icons/skin.png"]);
    fs::write(
        dir.join("c.html"),
        r#"<div class="c"><img src="images/icons/skin.png"></div>"#,
    )
    .unwrap();
    let packed = pack(&dir, "test", &html_pairs(&dir, &["c"])).expect("pack ok");
    assert!(
        packed
            .referenced_sprites
            .contains(&"images/icons/skin.png".to_string()),
        "referenced_sprites 含 images/icons/skin.png"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_single_html_roundtrips() {
    let dir = make_ws_dir_with_html(&["scene"], &[]);
    let packed = pack(&dir, "test", &html_pairs(&dir, &["scene"])).expect("pack ok");
    assert!(!packed.pkg_bytes.is_empty());
    let pkg = read_package(&packed.pkg_bytes).expect("read ok");
    assert_eq!(pkg.components.len(), 1);
    let comp = pkg.components.values().next().unwrap();
    assert!(!comp.nodes.is_empty());
    let has_text = comp.nodes.iter().any(|n| {
        matches!(&n.kind,
        loomgui_core::scene::NodeKind::Text { content } if content == "hi")
    });
    assert!(has_text, "scene.html 的 span 文本 hi 应在节点树");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_missing_html_file_errors() {
    let dir = make_ws_dir_with_html(&["a"], &[]);
    let pairs = vec![("nope.html".to_string(), dir.join("nope.html"))];
    let r = pack(&dir, "test", &pairs);
    assert!(r.is_err(), "缺 HTML 文件应 Err");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pack_img_src_outside_workspace_root_errors() {
    // img src="../outside.png" 越出 workspace_root → Err
    let dir = make_ws_dir_with_html(&["c"], &[]);
    fs::write(
        dir.join("c.html"),
        r#"<div class="c"><img src="../outside.png"></div>"#,
    )
    .unwrap();
    let result = pack(&dir, "test", &html_pairs(&dir, &["c"]));
    assert!(result.is_err(), "越出 workspace_root 的引用应 Err");
    let _ = fs::remove_dir_all(&dir);
}

/// 打包器扫 data-controller + data-page → ControllerEntry 进 pkg。
#[test]
fn pack_scans_data_controller_and_data_page_into_controller_entry() {
    let dir = make_ws_dir_with_html(&["c"], &[]);
    fs::write(
        dir.join("c.html"),
        r#"<div class="c" data-controller="tab" data-page="2"><span>hi</span></div>"#,
    )
    .unwrap();
    let packed = pack(&dir, "test", &html_pairs(&dir, &["c"])).expect("pack ok");
    let pkg = read_package(&packed.pkg_bytes).expect("read ok");
    let comp = &pkg.components["c"];
    assert_eq!(
        comp.controllers.len(),
        1,
        "一个 data-controller → 一个 ControllerEntry"
    );
    let c = &comp.controllers[0];
    assert_eq!(c.name, "tab");
    assert_eq!(c.mount_node_idx, 0, "mount = 组件根（节点 0）");
    assert_eq!(c.initial_selected_index, 2, "data-page=\"2\" → initial=2");
    let _ = fs::remove_dir_all(&dir);
}

/// data-controller 无 data-page → initial_selected_index 默认 0。
#[test]
fn pack_data_controller_without_data_page_defaults_to_zero() {
    let dir = make_ws_dir_with_html(&["c"], &[]);
    fs::write(
        dir.join("c.html"),
        r#"<div class="c" data-controller="tab"><span>hi</span></div>"#,
    )
    .unwrap();
    let packed = pack(&dir, "test", &html_pairs(&dir, &["c"])).expect("pack ok");
    let pkg = read_package(&packed.pkg_bytes).expect("read ok");
    let comp = &pkg.components["c"];
    assert_eq!(comp.controllers.len(), 1);
    assert_eq!(
        comp.controllers[0].initial_selected_index, 0,
        "无 data-page → 默认 0"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ── v1.7 display:block desugar → RichText 叶 ──

/// 构造小 ElementTree + styles 调 desugar_block_divs，验 block div 的 raw_rich → runs。
fn desugar_html(
    html: &str,
) -> Result<
    (
        loomgui_core::parse::dom::ElementTree,
        Vec<loomgui_core::style::resolved::ResolvedStyle>,
    ),
    String,
> {
    let tree = loomgui_core::parse::dom::parse_html(html)?;
    let sheet = loomgui_core::parse::css::parse_css("")?;
    let styles = loomgui_core::style::cascade::resolve_styles(&tree, &sheet);
    loomgui_pkg::desugar_block_divs(tree, styles)
}

#[test]
fn desugar_block_div_produces_rich_runs() {
    let html = r#"<div style="display:block">a <b>b</b> <a href="x">c</a></div>"#;
    let (tree, _styles) = desugar_html(html).expect("desugar ok");
    let div = &tree.nodes[tree.roots[0].0];
    let runs = div.rich_runs.as_ref().expect("desugar 填了 rich_runs");
    assert!(!runs.is_empty(), "rich_runs 非空");
    assert!(
        runs.iter()
            .any(|r| { matches!(r.weight, loomgui_core::text::rich::RichWeight::Bold) }),
        "runs 含 bold"
    );
    assert!(runs.iter().any(|r| r.link_id.is_some()), "runs 含 link");
}

#[test]
fn desugar_block_div_then_build_scene_emits_richtext_kind() {
    use loomgui_core::scene::NodeKind;
    let html = r#"<div style="display:block">hello <b>world</b></div>"#;
    let (tree, styles) = desugar_html(html).expect("desugar ok");
    let scene = loomgui_core::scene::build_scene(&tree, &styles);
    let root_id = scene.roots[0];
    let root = scene.get(root_id).unwrap();
    assert!(
        matches!(root.kind, NodeKind::RichText { .. }),
        "block div 经 desugar + build_scene 产 RichText 叶，非 Container"
    );
}

#[test]
fn desugar_block_div_rejects_justify_content() {
    let html = r#"<div style="display:block; justify-content:center">a <b>b</b></div>"#;
    let result = desugar_html(html);
    assert!(result.is_err(), "block div 拒 justify-content");
    let err = result.unwrap_err();
    assert!(
        err.contains("justify-content"),
        "错误信息提及 justify-content: {err}"
    );
}

#[test]
fn desugar_block_div_rejects_align_items() {
    let html = r#"<div style="display:block; align-items:center">a <b>b</b></div>"#;
    let result = desugar_html(html);
    assert!(result.is_err(), "block div 拒 align-items");
}

#[test]
fn desugar_block_div_rejects_gap() {
    let html = r#"<div style="display:block; gap:10px">a <b>b</b></div>"#;
    let result = desugar_html(html);
    assert!(result.is_err(), "block div 拒 gap");
}

#[test]
fn desugar_block_div_accepts_non_flex_props() {
    let html = r#"<div style="display:block; color:#ff0000; font-size:20px; width:200px">a <b>b</b></div>"#;
    let (tree, _styles) = desugar_html(html).expect("非 flex 属性不应被拒");
    let div = &tree.nodes[tree.roots[0].0];
    assert!(div.rich_runs.is_some(), "rich_runs 仍填");
}

#[test]
fn desugar_flex_div_unaffected() {
    let html = r#"<div><span>hi</span></div>"#;
    let (tree, _styles) = desugar_html(html).expect("desugar ok");
    for el in &tree.nodes {
        assert!(el.raw_rich.is_none(), "flex div 无 raw_rich");
        assert!(el.rich_runs.is_none(), "flex div 无 rich_runs");
    }
}

#[test]
fn desugar_block_div_rejects_flex_direction() {
    // block div + flex-direction: row（非默认 Column）→ 应拒收。
    let html = r#"<div style="display:block; flex-direction:row">a <b>b</b></div>"#;
    let result = desugar_html(html);
    assert!(result.is_err(), "block div 拒 flex-direction");
    let err = result.unwrap_err();
    assert!(
        err.contains("flex-direction"),
        "错误信息提及 flex-direction: {err}"
    );
}

#[test]
fn desugar_block_div_rejects_flex_wrap() {
    // block div + flex-wrap: wrap（非默认 NoWrap）→ 应拒收。
    let html = r#"<div style="display:block; flex-wrap:wrap">a <b>b</b></div>"#;
    let result = desugar_html(html);
    assert!(result.is_err(), "block div 拒 flex-wrap");
    let err = result.unwrap_err();
    assert!(err.contains("flex-wrap"), "错误信息提及 flex-wrap: {err}");
}
