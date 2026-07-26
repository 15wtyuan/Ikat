use super::*;
use crate::test_helpers::stage_new_with_dejavu;
use loomgui_core::scene::node::{ControlState, NodeKind};
use loomgui_core::style::resolved::DisplayMode;
use std::ffi::CStr;

/// 测试辅助：建根 div 后直接往 scene.controls 注入 Progress 状态。
/// FFI 表面无 control_init setter（打包期产物），故测试侧手工填。
fn make_progress_stage(value: f32, max: f32) -> (*mut StageHandle, u32) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(root),
        ControlState::Progress {
            value,
            max,
            indeterminate: false,
        },
    );
    (h, root)
}

/// 测试辅助：建根 div 后直接往 scene.controls 注入 Slider 状态。
fn make_slider_stage(value: f32, min: f32, max: f32, step: f32) -> (*mut StageHandle, u32) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(root),
        ControlState::Slider {
            value,
            min,
            max,
            step,
            dragging: false,
        },
    );
    (h, root)
}

/// 测试辅助：建根 div 后直接往 scene.controls 注入 Toggle 状态。
fn make_toggle_stage(checked: bool) -> (*mut StageHandle, u32) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene
        .controls
        .ensure(NodeId(root), ControlState::Toggle { checked });
    (h, root)
}

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
    assert_eq!(repr.display_mode, DisplayMode::Block as u8);
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

// ===== control value/checked/transform get/set FFI =====

/// ProgressBar set_control_value(90) → get_control_value == 90；超 max 的 150 被 clamp 到 100。
/// 验 return-code + out-param 模式（避免 Container=0 哨兵撞）。
#[test]
fn ffi_set_get_control_value_progress() {
    let (h, node) = make_progress_stage(70.0, 100.0);
    // 合法区间：90 → 90
    let rc = loomgui_stage_set_control_value(h, node, 90.0);
    assert_eq!(rc, 0, "set_control_value(90) rc");
    let mut out = 0.0f32;
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0, "get_control_value rc");
    assert!((out - 90.0).abs() < 0.001, "value == 90, got {out}");
    // 超 max：150 → clamp 100
    let rc = loomgui_stage_set_control_value(h, node, 150.0);
    assert_eq!(rc, 0, "set_control_value(150) rc");
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 100.0).abs() < 0.001,
        "150 clamped to max 100, got {out}"
    );
    // 负值：-10 → clamp 0
    let rc = loomgui_stage_set_control_value(h, node, -10.0);
    assert_eq!(rc, 0);
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 0.0).abs() < 0.001, "-10 clamped to 0, got {out}");
    loomgui_stage_free(h);
}

/// Slider set_control_value：clamp + step 量化（step=5 → 83 → 85）。
#[test]
fn ffi_set_get_control_value_slider() {
    let (h, node) = make_slider_stage(50.0, 0.0, 100.0, 5.0);
    // 83 被 step=5 量化到 85（最近的 step 边界）
    let rc = loomgui_stage_set_control_value(h, node, 83.0);
    assert_eq!(rc, 0, "set_control_value(83) rc");
    let mut out = 0.0f32;
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 85.0).abs() < 0.001,
        "83 quantized to 85 (step=5), got {out}"
    );
    // 超 max clamp
    let rc = loomgui_stage_set_control_value(h, node, 200.0);
    assert_eq!(rc, 0);
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 100.0).abs() < 0.001,
        "200 clamped to max 100, got {out}"
    );
    loomgui_stage_free(h);
}

/// 非 value 控件（Toggle）set/get_control_value → -1（语义不适用）。
#[test]
fn ffi_control_value_non_value_control_err() {
    let (h, node) = make_toggle_stage(false);
    let rc = loomgui_stage_set_control_value(h, node, 50.0);
    assert_eq!(rc, -1, "Toggle set_control_value → -1");
    let mut out = -1.0f32;
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, -1, "Toggle get_control_value → -1");
    loomgui_stage_free(h);
}

/// get_control_value null out → 非 0（rc=0 严格意味 *out 已填）。
#[test]
fn ffi_get_control_value_null_out_err() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    let rc = loomgui_stage_get_control_value(h, node, std::ptr::null_mut());
    assert_ne!(rc, 0, "null out must not return 0");
    loomgui_stage_free(h);
}

/// Toggle set_control_checked(true) → get_control_checked == true。
#[test]
fn ffi_set_get_control_checked_toggle() {
    let (h, node) = make_toggle_stage(false);
    let rc = loomgui_stage_set_control_checked(h, node, true);
    assert_eq!(rc, 0, "set_control_checked(true) rc");
    let mut out = false;
    let rc = loomgui_stage_get_control_checked(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(out, "checked == true");
    // 翻回 false
    let rc = loomgui_stage_set_control_checked(h, node, false);
    assert_eq!(rc, 0);
    let rc = loomgui_stage_get_control_checked(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(!out, "checked == false");
    loomgui_stage_free(h);
}

/// set_control_checked 对非 Toggle/Radio（Progress）→ -1。
#[test]
fn ffi_set_control_checked_non_check_control_err() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    let rc = loomgui_stage_set_control_checked(h, node, true);
    assert_eq!(rc, -1, "Progress set_control_checked → -1");
    let mut out = true;
    let rc = loomgui_stage_get_control_checked(h, node, &mut out);
    assert_eq!(rc, -1, "Progress get_control_checked → -1");
    loomgui_stage_free(h);
}

/// Progress set/get_control_max：max 100 → 200，读回 200。
#[test]
fn ffi_set_get_control_max_progress() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    let rc = loomgui_stage_set_control_max(h, node, 200.0);
    assert_eq!(rc, 0, "set_control_max(200) rc");
    let mut out = 0.0f32;
    let rc = loomgui_stage_get_control_max(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 200.0).abs() < 0.001, "max == 200, got {out}");
    loomgui_stage_free(h);
}

/// Slider set/get_control_min/step：min 0 → 10，step 0 → 2。
#[test]
fn ffi_set_get_control_min_step_slider() {
    let (h, node) = make_slider_stage(50.0, 0.0, 100.0, 0.0);
    let rc = loomgui_stage_set_control_min(h, node, 10.0);
    assert_eq!(rc, 0, "set_control_min(10) rc");
    let mut out = 0.0f32;
    let rc = loomgui_stage_get_control_min(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 10.0).abs() < 0.001, "min == 10, got {out}");

    let rc = loomgui_stage_set_control_step(h, node, 2.0);
    assert_eq!(rc, 0, "set_control_step(2) rc");
    let rc = loomgui_stage_get_control_step(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 2.0).abs() < 0.001, "step == 2, got {out}");
    loomgui_stage_free(h);
}

/// Progress 无 min/step 语义 → set/get_control_min/step 返 -1。
#[test]
fn ffi_control_min_step_progress_err() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    assert_eq!(loomgui_stage_set_control_min(h, node, 10.0), -1);
    assert_eq!(loomgui_stage_set_control_step(h, node, 2.0), -1);
    let mut out = 99.0f32;
    assert_eq!(loomgui_stage_get_control_min(h, node, &mut out), -1);
    assert_eq!(loomgui_stage_get_control_step(h, node, &mut out), -1);
    loomgui_stage_free(h);
}

/// set_transform：写 user_transform，读回 node.user_transform.translate == [50, 0]。
/// 走 set_user_transform（dynamic.rs），不触发 solve（仅渲染/命中层）。
#[test]
fn ffi_set_transform_translates_user_transform() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let rc = loomgui_stage_set_transform(h, root, 50.0, 0.0, 1.0, 1.0, 0.0);
    assert_eq!(rc, 0, "set_transform rc");
    // 读回 node.user_transform（同 crate 可访私有字段，需 unsafe 解原指针）
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    let node_ref = scene.get(NodeId(root)).expect("node live");
    assert_eq!(node_ref.user_transform.translate, [50.0, 0.0]);
    assert_eq!(node_ref.user_transform.scale, [1.0, 1.0]);
    loomgui_stage_free(h);
}

/// set_transform 对不 live 节点 → -1（set_user_transform 返 Err）。
#[test]
fn ffi_set_transform_invalid_node_err() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let rc = loomgui_stage_set_transform(h, 0xFFFF_FFFF, 10.0, 10.0, 1.0, 1.0, 0.0);
    assert_eq!(rc, -1, "invalid node set_transform → -1");
    loomgui_stage_free(h);
}

/// set_control_max 对 Progress 传负 max 不可 panic（FFI 不可因 caller 输入 abort
/// 进程）；max guard 到 ≥0，rc=0，get_control_max 返 ≥0。
#[test]
fn ffi_set_control_max_negative_does_not_panic() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    // 传负 max：旧实现 value.clamp(0.0, -5.0) 会 panic（min > max）
    let rc = loomgui_stage_set_control_max(h, node, -5.0);
    assert_eq!(rc, 0, "set_control_max(-5) on Progress → rc=0 (guard to 0)");
    let mut out = -999.0f32;
    let rc = loomgui_stage_get_control_max(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(out >= 0.0, "max guard to ≥0, got {out}");
    // value 也被重新 clamp 进 [0, 0] = 0，不悬空
    let mut v = -999.0f32;
    let rc = loomgui_stage_get_control_value(h, node, &mut v);
    assert_eq!(rc, 0);
    assert!(v >= 0.0 && v <= out, "value clamp into [0,max], got {v}");
    loomgui_stage_free(h);
}

/// set_control_value 对 Slider 量化后不可超 max：min=0,max=100,step=6,v=100 →
/// 量化得 102 > max，必须重新 clamp 回 100。
#[test]
fn ffi_set_control_value_slider_quantize_respects_max() {
    let (h, node) = make_slider_stage(0.0, 0.0, 100.0, 6.0);
    // 100 / 6 = 16.67 → round 17 → 17*6 = 102 > max 100（旧实现违反区间）
    let rc = loomgui_stage_set_control_value(h, node, 100.0);
    assert_eq!(rc, 0);
    let mut out = 0.0f32;
    let rc = loomgui_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        out <= 100.0,
        "quantized value must not exceed max 100, got {out}"
    );
    assert!(
        out >= 0.0,
        "quantized value must not go below min 0, got {out}"
    );
    loomgui_stage_free(h);
}
