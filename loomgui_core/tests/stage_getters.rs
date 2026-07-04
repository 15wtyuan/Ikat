//! Stage 三个 get_node_* getter 端到端验证（NativeHost FFI 查询口子，Task 2）。
//!
//! - get_node_world_matrix: 读 scene.world_transforms（compute_world_transforms 产物，
//!   全节点含空 div）。
//! - get_node_sort_key: 读 scene.node_sort_keys（assign_sort_keys merge 前快照）。
//! - get_node_visible: 节点存在 + 非 display:none。
//!
//! 无效 NodeId（含 gen 失效 / INVALID sentinel）→ None/false，不 panic（坑 102）。

use loomgui_core::parse::css::parse_css;
use loomgui_core::parse::dom::parse_html;
use loomgui_core::scene::node::{build_scene, NodeId};
use loomgui_core::stage::Stage;
use loomgui_core::style::cascade::resolve_styles;

fn font_path() -> String {
    format!("{}/tests/fixtures/DejaVuSans.ttf", env!("CARGO_MANIFEST_DIR"))
}

/// HTML+CSS → scene（parse_html + build_scene），复用 node_sort_keys.rs 的 load 模式。
fn load_html_css(stage: &mut Stage, html: &str, css: &str) {
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

/// world_matrix 读 slot 位置：transform:translate(100px,200px) → wm[4]=100, wm[5]=200。
#[test]
fn get_node_world_matrix_returns_slot_position() {
    let mut stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    let html = r#"<div id="n" style="width:50px;height:50px;transform:translate(100px,200px)"></div>"#;
    load_html_css(&mut stage, html, "");
    stage.advance_time(0.016);
    stage.tick_and_render();

    let n = stage.find_node_by_id("n").expect("n 节点存在");
    let wm = stage
        .get_node_world_matrix(n)
        .expect("有效节点 → Some(world matrix)");
    // Affine2 = [a,b,c,d,tx,ty]；translate(100,200) → tx=100, ty=200。
    assert!(
        (wm[4] - 100.0).abs() < 1e-3,
        "wm[4] (tx) = 100，got {}",
        wm[4]
    );
    assert!(
        (wm[5] - 200.0).abs() < 1e-3,
        "wm[5] (ty) = 200，got {}",
        wm[5]
    );
}

/// sort_key DFS 序：两兄弟 div（DOM 序 a 先 b 后）→ a.sort_key < b.sort_key。
#[test]
fn get_node_sort_key_returns_dfs_order() {
    let mut stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    let html = r#"<div><div id="a" style="background-color:#ff0000"></div><div id="b" style="background-color:#00ff00"></div></div>"#;
    load_html_css(&mut stage, html, "");
    stage.advance_time(0.016);
    stage.tick_and_render();

    let a = stage.find_node_by_id("a").expect("a 节点存在");
    let b = stage.find_node_by_id("b").expect("b 节点存在");
    let ka = stage
        .get_node_sort_key(a)
        .expect("有效节点 a → Some(sort_key)");
    let kb = stage
        .get_node_sort_key(b)
        .expect("有效节点 b → Some(sort_key)");
    assert!(
        ka < kb,
        "DFS 序：a.sort_key ({}) < b.sort_key ({})",
        ka,
        kb
    );
}

/// display:none → get_node_visible 返 false；同 scene 内正常节点返 true。
#[test]
fn get_node_visible_display_none_false() {
    let mut stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    let html = r#"<div><div id="hidden" style="display:none"></div><div id="visible" style="background-color:#000"></div></div>"#;
    load_html_css(&mut stage, html, "");
    stage.advance_time(0.016);
    stage.tick_and_render();

    let hidden = stage.find_node_by_id("hidden").expect("hidden 节点存在");
    let visible = stage.find_node_by_id("visible").expect("visible 节点存在");
    assert!(
        !stage.get_node_visible(hidden),
        "display:none → get_node_visible=false"
    );
    assert!(
        stage.get_node_visible(visible),
        "正常节点 → get_node_visible=true"
    );
}

/// 无效 NodeId（INVALID sentinel）→ 三个 getter 全 None/false，不 panic。
#[test]
fn get_node_invalid_returns_none() {
    let mut stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    load_html_css(&mut stage, r#"<div id="n"></div>"#, "");
    stage.advance_time(0.016);
    stage.tick_and_render();

    let invalid = NodeId::INVALID;
    assert_eq!(
        stage.get_node_world_matrix(invalid),
        None,
        "无效 NodeId world_matrix → None"
    );
    assert_eq!(
        stage.get_node_sort_key(invalid),
        None,
        "无效 NodeId sort_key → None"
    );
    assert!(
        !stage.get_node_visible(invalid),
        "无效 NodeId → get_node_visible=false"
    );
}

/// 无 scene（Stage 新建未 ensure_scene）→ 三个 getter 全 None/false，不 panic。
/// 覆盖 FFI 在 load_package 前（scene=None）查询的早返路径（坑 102）。
#[test]
fn get_node_no_scene_returns_none() {
    let stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    // 不调 ensure_scene / create_root / load_html_css → scene = None。
    let n = NodeId(0x1_0001); // 合法形态的 NodeId（idx=1, gen=1），但 scene=None
    assert_eq!(stage.get_node_world_matrix(n), None, "scene=None → None");
    assert_eq!(stage.get_node_sort_key(n), None, "scene=None → None");
    assert!(!stage.get_node_visible(n), "scene=None → false");
}
