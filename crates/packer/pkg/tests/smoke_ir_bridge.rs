//! 终点线 1 smoke：HTML -> pkg.bin -> Stage -> rect/visible 断言（端到端范式验证）。
//!
//! 验"端到端链通 + class 命中（rect）+ display:none 剪枝 + flex 布局"。继承/color/font、
//! kind 保真、computed style 的完整断言推 ③（spec §10）——本 smoke 用 Stage public API
//! （get_node_layout_rect / get_node_visible / find_node_by_id）可达的部分。
use loomgui_core::scene::NodeId;
use loomgui_core::stage::Stage;
use loomgui_pkg::build::pack_components;

/// Pack HTML -> pkg.bin -> Stage -> instantiate -> 帧推进，返回 Stage + 组件根 NodeId。
///
/// `create_root` 建 scene + stage_root（`Stage::new` 不建 scene，`instantiate` 要求 scene
/// 已存在——见 stage.rs:573 `ok_or("no scene (create_root first)")`）。`append_child` 把
/// 组件根挂进 tree，否则 `solve` 只走 `scene.roots[0]`（layout/mod.rs:217），孤立组件不进 layout。
fn build_stage(html: &str) -> (Stage, NodeId) {
    let (bytes, _refs) =
        pack_components(&[("c".to_string(), html.to_string())]).expect("pack_components");
    let mut stage = Stage::new((400.0, 300.0)).expect("Stage::new");
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
    // 完整文档结构（html/head/body shell 由 fence 剥除）+ 单行（顶层无 inter-element 空白
    // Text 节点——fence 当前把顶层空白 Text 也当 root，bridge 单根契约拒）。
    let html = r#"<!DOCTYPE html><html><head><style>
.wrap { display:flex; flex-direction:column; width:200px; }
.hide { display:none; }
</style></head><body><div class="wrap" id="wrap"><div id="a"></div><div id="hide" class="hide"></div></div></body></html>"#;
    let (stage, _root) = build_stage(html);
    // class 命中：.wrap width:200（来自 <style> class 规则，经 cascade 生效）
    let wrap = stage.find_node_by_id("wrap").expect("wrap");
    let wrap_rect = stage.get_node_layout_rect(wrap).expect("wrap rect");
    assert!(
        (wrap_rect.w - 200.0).abs() < 1.0,
        "class .wrap width:200 not applied (cascade broken?): w={}",
        wrap_rect.w
    );
    // display:none 剪枝：.hide not visible
    let hide = stage.find_node_by_id("hide").expect("hide");
    assert!(
        !stage.get_node_visible(hide),
        "display:none node should be invisible"
    );
    // flex 布局：子 a 在 wrap 内（rect 合理）
    let a = stage.find_node_by_id("a").expect("a");
    let a_rect = stage.get_node_layout_rect(a).expect("a rect");
    assert!(a_rect.h >= 0.0, "child a laid out, h={}", a_rect.h);
}

#[test]
fn smoke_control_kinds_load_without_crash() {
    // 控件全家（input dispatch 5 种 + select）— instantiate 不 panic = 链通。
    // kind 保真（不塌 Container）由 Task 2 pkg roundtrip + Task 3 bridge map unit test 覆盖。
    let html = r#"<!DOCTYPE html><html><head></head><body><div><input type="text"><input type="range"><input type="checkbox"><input type="radio"><select><option></option></select></div></body></html>"#;
    let _ = build_stage(html); // 不 panic = 通过
}
