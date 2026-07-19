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

// ===== null css / null text 防御契约（spec §6.1 deferred ②） =====
//
// caller 传 `ptr::null(), 0` 表「空字符串」（C# 默认 string、null 字面）。
// slice::from_raw_parts(null, 0) 是 UB（即使 len=0），故 FFI 必须 null-safe 兜底为 ""。
// 跑过即证明 null 被守卫；UB 在 Miri/ASAN 下会 crash，普通 cargo test 通常静默走过。

/// create_root(null css) 必须成功（= 空 inline css），不 UB。
#[test]
fn create_root_null_css_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, std::ptr::null(), 0);
    assert_ne!(
        root, 0xFFFF_FFFF,
        "create_root with null css must succeed (treated as empty css)"
    );
    loomgui_stage_free(h);
}

/// create_node(null css) 必须成功，不 UB。
#[test]
fn create_node_null_css_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let node = loomgui_stage_create_node(h, b"div".as_ptr(), 3, std::ptr::null(), 0);
    assert_ne!(
        node, 0xFFFF_FFFF,
        "create_node with null css must succeed (treated as empty css)"
    );
    loomgui_stage_free(h);
}

/// set_inline_override(null css) 必须返 0（= 空覆盖，no-op 语义），不 UB。
#[test]
fn set_inline_override_null_css_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let rc = loomgui_stage_set_inline_override(h, root, std::ptr::null(), 0);
    assert_eq!(
        rc, 0,
        "set_inline_override with null css must succeed (treated as empty css)"
    );
    loomgui_stage_free(h);
}

/// set_text(null text) 必须返 0（= 清空 Text 内容），不 UB。
#[test]
fn set_text_null_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let text = loomgui_stage_create_node(h, b"span".as_ptr(), 4, std::ptr::null(), 0);
    assert_ne!(text, 0xFFFF_FFFF, "create span ok");
    let rc = loomgui_stage_set_text(h, text, std::ptr::null(), 0);
    assert_eq!(
        rc, 0,
        "set_text with null text must succeed (treated as empty content)"
    );
    loomgui_stage_free(h);
}
