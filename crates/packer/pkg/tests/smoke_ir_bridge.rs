//! 终点线 1 smoke：HTML -> pkg.bin -> Stage -> rect/visible 断言（端到端范式验证）。
//!
//! 验"端到端链通 + class 命中（rect）+ display:none 剪枝 + flex 布局"。继承/color/font、
//! kind 保真、computed style 的完整断言推 ③（spec §10）——本 smoke 用 Stage public API
//! （get_node_layout_rect / get_node_visible / find_node_by_id）可达的部分。
use loomgui_core::scene::NodeId;
use loomgui_core::stage::Stage;
use loomgui_pkg::build::{pack_components, Component, PackResult};

/// Pack HTML -> pkg.bin -> Stage -> instantiate -> 帧推进，返回 Stage + 组件根 NodeId。
///
/// `create_root` 建 scene + stage_root（`Stage::new` 不建 scene，`instantiate` 要求 scene
/// 已存在——见 stage.rs:573 `ok_or("no scene (create_root first)")`）。`append_child` 把
/// 组件根挂进 tree，否则 `solve` 只走 `scene.roots[0]`（layout/mod.rs:217），孤立组件不进 layout。
fn build_stage(html: &str) -> (Stage, NodeId) {
    let PackResult { bytes, .. } = pack_components(&[Component {
        name: "c".to_string(),
        src: html.to_string(),
        html_rel: "c.html".to_string(),
    }])
    .expect("pack_components");
    let mut stage = Stage::new((400.0, 300.0)).expect("Stage::new");
    // Text layout needs a default font at tick time (measure_text panics without one).
    // Register a default ASCII font; embedded at compile time to stay cwd-independent.
    stage
        .register_font(
            "DejaVu",
            include_bytes!("../../../core/tests/fixtures/DejaVuSans.ttf").to_vec(),
            true,
        )
        .expect("register_font");
    stage.load_package("p", &bytes).expect("load_package");
    let stage_root = stage.create_root("div", "").expect("create_root");
    let comp_root = stage.instantiate("p", "c").expect("instantiate");
    stage
        .append_child(stage_root, comp_root)
        .expect("append_child");
    stage.advance_time(0.0);
    stage.tick_and_render();
    (stage, comp_root)
}

#[test]
fn smoke_main_gate_class_hit_displaynone_flex() {
    // 完整文档结构（html/head/body shell 由 fence 剥除）。多行 HTML 也行——顶层空白
    // 不再变孤立 Text root（fence tree_builder 已修）。
    let html = r#"<!DOCTYPE html><html><head><style>
.wrap { display:flex; flex-direction:column; width:200px; }
.hide { display:none; }
</style></head><body><div class="wrap" id="wrap"><div id="a"></div><div id="hide" class="hide"></div></div></body></html>"#;
    let (stage, _root) = build_stage(html);
    // class 命中：.wrap width:200（来自 <style> class 规则，经 cascade 生效）
    let wrap = stage.find_node_by_id("wrap").expect("wrap");
    let wrap_rect = stage.get_node_layout_rect(wrap).expect("wrap rect");
    assert!(
        (wrap_rect.w - 200.0).abs() < 0.5,
        "class .wrap width:200 not applied (cascade broken?): w={}",
        wrap_rect.w
    );
    // display:none 剪枝：.hide not visible
    let hide = stage.find_node_by_id("hide").expect("hide");
    assert!(
        !stage.get_node_visible(hide),
        "display:none node should be invisible"
    );
    // flex 布局：.wrap 是 flex-direction:column，子 a 无显式 width，align-items 默认
    // stretch → cross-axis（width）应填满 200。仅查 h>=0 不证 flex 跑了（任何默认 rect 都过）。
    let a = stage.find_node_by_id("a").expect("a");
    let a_rect = stage.get_node_layout_rect(a).expect("a rect");
    assert!(
        (a_rect.w - 200.0).abs() < 1.0,
        "child a cross-axis width should stretch to 200 (flex column ran): w={}",
        a_rect.w
    );
}

#[test]
fn smoke_control_kinds_load_without_crash() {
    // 控件全家（input dispatch 5 种 + select）— instantiate 不 panic = 链通。
    // kind 保真（不塌 Container）由 pkg roundtrip + bridge map unit test 覆盖。
    let html = r#"<!DOCTYPE html><html><head><style>input[type="text"],input[type="range"],input[type="checkbox"],input[type="radio"]{width:80px}select{width:80px}option{color:#222222}</style></head><body><div style="display:flex"><input type="text"><input type="range"><input type="checkbox"><input type="radio"><select><option></option></select></div></body></html>"#;
    let _ = build_stage(html); // 不 panic = 通过
}
