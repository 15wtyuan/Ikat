//! Scene.node_sort_keys 端到端验证：assign_sort_keys DFS 序号在 merge_meshes 后仍保留。
//!
//! 场景：root > [nh-stage(空 div，无 bg), nh-effect(有 bg #000)]。两节点同 DrawState
//! （image_path=None, program=0, mask=0, alpha=1.0）→ merge_meshes 合两 Mesh 成一个
//! merged Mesh payload（anchor=min(node_id)），nh-stage 的 RenderNode entry 被吞。
//! 但 Scene.node_sort_keys[nh_stage.index()] 仍保留 DFS 序号（FFI 查询用）。
//!
//! 参考 NativeHost FFI 查询口子设计：Task 2/3 读 node_sort_keys 给空 div slot
//! 兜底 sort 信息（merge 后查不到 RenderNode.sort_key，回 scene.node_sort_keys）。

use loomgui_core::parse::css::parse_css;
use loomgui_core::parse::dom::parse_html;
use loomgui_core::scene::node::build_scene;
use loomgui_core::stage::Stage;
use loomgui_core::style::cascade::resolve_styles;

fn font_path() -> String {
    format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// HTML+CSS → scene（parse_html + build_scene），复用 v1e_dirty 的 load 模式。
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

/// 空 div slot + 有 bg 兄弟：node_sort_keys 在 merge_meshes 后仍保留两节点 DFS 序号。
#[test]
fn node_sort_keys_filled_for_empty_div_slot() {
    let mut stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    // root > [nh-stage(空 div), nh-effect(黑底)]。
    let html = r#"<div><div id="nh-stage"></div><div id="nh-effect" style="background-color:#000"></div></div>"#;
    load_html_css(&mut stage, html, "");
    stage.advance_time(0.016);
    let _frame = stage.tick_and_render();

    let scene = stage.scene.as_ref().expect("scene live post-tick");
    let nh_stage = scene
        .find_by_id_attr("nh-stage")
        .expect("nh-stage 节点存在");
    let nh_effect = scene
        .find_by_id_attr("nh-effect")
        .expect("nh-effect 节点存在");

    // sort_keys buffer 至少覆盖到两节点 index（capacity+1 扩容保证）。
    let stage_key = scene.node_sort_keys[nh_stage.index()];
    let effect_key = scene.node_sort_keys[nh_effect.index()];

    // 两节点 DFS 序号均 > 0（root=0，DFS 先序：root → nh-stage → nh-effect，
    // nh-stage 至少 ≥1，nh-effect ≥2）。具体值依赖 slotmap idx 与 DFS 推进。
    assert!(
        stage_key > 0,
        "nh-stage node_sort_keys 非 0（DFS 序号），got {}",
        stage_key
    );
    assert!(
        effect_key > 0,
        "nh-effect node_sort_keys 非 0（DFS 序号），got {}",
        effect_key
    );
    // DFS 序保 DOM 顺序：nh-stage 在 nh-effect 前。
    assert!(
        stage_key < effect_key,
        "nh-stage DFS 序号 < nh-effect（DOM 顺序），got stage={} effect={}",
        stage_key,
        effect_key
    );
}

/// node_sort_keys 与 RenderNode.sort_key 在 assign_sort_keys（merge 前）阶段一致。
/// 用 root 单节点（不触发 merge）查证 buffer 写入与 RenderNode.sort_key 对齐。
#[test]
fn node_sort_keys_matches_render_node_sort_key_pre_merge() {
    let mut stage = Stage::new(&font_path(), (200.0, 200.0)).expect("stage");
    // 单 div 带 bg（避免空 div 走 merge）。
    let html = r#"<div id="only" style="background-color:#ff0000"></div>"#;
    load_html_css(&mut stage, html, "");
    stage.advance_time(0.016);
    let frame = stage.tick_and_render();

    let scene = stage.scene.as_ref().expect("scene");
    let only = scene.find_by_id_attr("only").expect("only 节点");
    // node_sort_keys[idx] 与 RenderNode.sort_key（merge 后仍保留——单节点不 merge）一致。
    let buf_key = scene.node_sort_keys[only.index()];
    let rn_key = frame
        .nodes
        .iter()
        .find(|n| n.node_id == only.0)
        .expect("only 节点的 RenderNode 存在")
        .sort_key;
    assert_eq!(
        buf_key, rn_key,
        "单节点无 merge：node_sort_keys[idx] == RenderNode.sort_key，got buf={} rn={}",
        buf_key, rn_key
    );
}
