//! 诊断（临时）：FFI 层 set_node_visible 是否生效（bug 10 取证）。
//! FFI setter + Stage::tick_and_render 全链，直接断言 RenderNode.visible。

use crate::*;
use yio_core::render::node::ChangeLevel;

#[test]
fn ffi_set_node_visible_emits_invisible_row() {
    let h = test_helpers::stage_new_with_dejavu(800.0, 600.0);

    let kind = b"div";
    let root = yio_stage_create_root(h, kind.as_ptr(), kind.len(), std::ptr::null(), 0);
    assert_ne!(root, 0, "create_root");

    let css = b"position:absolute;left:0;top:0;width:200px;height:100px;background-color:#123456";
    let n1 = yio_stage_create_node(h, kind.as_ptr(), kind.len(), css.as_ptr(), css.len());
    assert_ne!(n1, 0);
    let n2 = yio_stage_create_node(h, kind.as_ptr(), kind.len(), css.as_ptr(), css.len());
    assert_ne!(n2, 0);
    assert_eq!(yio_stage_append_child(h, root, n1), 0);
    assert_eq!(yio_stage_append_child(h, root, n2), 0);

    let sh = unsafe { &mut *h };
    let f1 = sh.stage.tick_and_render();
    println!(
        "frame1: rows={} hidden={}",
        f1.nodes.len(),
        f1.nodes.iter().filter(|r| !r.visible).count()
    );

    let rc = yio_stage_set_node_visible(h, n1, 0);
    println!("set_node_visible rc={rc}");
    assert_eq!(rc, 0, "FFI set_node_visible must return 0");

    let f2 = sh.stage.tick_and_render();
    let hidden = f2.nodes.iter().filter(|r| !r.visible).count();
    let lean = f2
        .nodes
        .iter()
        .filter(|r| r.change_level != ChangeLevel::Skip)
        .count();
    println!(
        "frame2: rows={} hidden={} lean={} levels={:?}",
        f2.nodes.len(),
        hidden,
        lean,
        f2.nodes.iter().map(|r| r.change_level).collect::<Vec<_>>()
    );
    assert!(hidden > 0, "隐藏后必须出现 visible=false 行（bug 10）");
}
