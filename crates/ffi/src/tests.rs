use super::*;
use crate::test_helpers::stage_new_with_dejavu;
use loomgui_core::scene::node::NodeKind;
use loomgui_core::style::resolved::DisplayMode;
use std::ffi::CStr;

#[test]
fn version_returns_c_string() {
    unsafe {
        let s = CStr::from_ptr(loomgui_version() as *const i8);
        assert_eq!(s.to_str().unwrap(), "v1e");
    }
}

// ===== get_node_kind + get_node_computed_style FFI（③ cascade 对外查询出口） =====

/// get_node_kind FFI round-trip + 哨兵不撞：div → Container(0)；无效 node → rc 非 0。
/// 关键：return-code 模式（不用 -> u8 + 0 哨兵），否则 Container=0 会与「不存在」撞。
#[test]
fn ffi_get_node_kind_div_and_invalid() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let mut kind: u8 = 255;
    let rc = loomgui_stage_get_node_kind(h, root, &mut kind);
    assert_eq!(rc, 0, "div kind rc");
    assert_eq!(kind, NodeKind::Container as u8, "div == Container(0)");
    // 无效 node → rc 非 0（关键：不撞 Container=0 哨兵）。
    let rc_bad = loomgui_stage_get_node_kind(h, 0xFFFF_FFFF, &mut kind);
    assert_ne!(
        rc_bad, 0,
        "invalid node must not return 0 (collides with Container)"
    );
    loomgui_stage_free(h);
}

/// get_node_computed_style FFI round-trip：div 默认 → opacity=1, display=Flex；无效 node → rc 非 0。
#[test]
fn ffi_get_node_computed_style_div() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let mut repr = ComputedNodeStyleRepr::default();
    let rc = loomgui_stage_get_node_computed_style(h, root, &mut repr);
    assert_eq!(rc, 0, "computed style rc");
    assert_eq!(repr.opacity, 1.0);
    assert_eq!(repr.display_mode, DisplayMode::Flex as u8);
    // 无效 node → rc 非 0。
    let rc_bad = loomgui_stage_get_node_computed_style(h, 0xFFFF_FFFF, &mut repr);
    assert_ne!(rc_bad, 0);
    loomgui_stage_free(h);
}

/// ComputedNodeStyleRepr 是 repr(C) POD，max align = 4（f32/[f32;4]）→ size 必 4 对齐。
/// ④ 实现 C# 镜像时，把 C# Marshal.SizeOf 与 Rust size_of 锁等（手写 struct 逐字段对齐须匹配）。
#[test]
fn computed_style_repr_is_aligned_pod() {
    let sz = std::mem::size_of::<ComputedNodeStyleRepr>();
    assert!(
        sz > 0 && sz.is_multiple_of(4),
        "repr(C) POD must be 4-byte aligned: got {sz}"
    );
}

/// null out + 节点存在 → rc 非 0：return code 0 严格意味「*out 已填」，
/// null out 没填不能返 0（否则 C 侧只看 rc 会用 uninit memory）。
#[test]
fn ffi_null_out_is_error() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let rc = loomgui_stage_get_node_kind(h, root, std::ptr::null_mut());
    assert_ne!(
        rc, 0,
        "get_node_kind: null out + existing node must not return 0"
    );
    let rc_c = loomgui_stage_get_node_computed_style(h, root, std::ptr::null_mut());
    assert_ne!(
        rc_c, 0,
        "get_node_computed_style: null out + existing node must not return 0"
    );
    loomgui_stage_free(h);
}
