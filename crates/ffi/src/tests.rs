use super::*;
use crate::test_helpers::stage_new_with_dejavu;
use loomgui_core::scene::node::{ControlState, EditState, NodeKind};
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

// ===== text input channel (set_text_input -> focused TextField) =====

/// 测试辅助：建根 div + 一个聚焦的 TextField（初始 value），返回 (handle, textfield_node)。
/// FFI 表面无 control_init setter（打包期产物），故测试侧手工注入 ControlState + 设焦点。
/// 光标初始在 value 末尾（from_init 默认），便于在末尾追加的断言。
fn make_stage_with_focused_textfield(value: &str) -> (*mut StageHandle, u32) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    // create_node 走 kind_from_tag 白名单（不含 input），改用 create_node_from_template 的
    // FFI 等价：先建个 div 再手工把 kind 改成 TextField 不现实——这里直接复用 create_node
    // 建 div 占位，再注入 TextField ControlState（kind 字段保留 div 不影响 insert_text：
    // insert_text 收 NodeKind 入参，测试侧显式传 TextField）。
    let tf = loomgui_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(tf, 0xFFFF_FFFF, "create textfield node ok");
    loomgui_stage_append_child(h, root, tf);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(tf),
        ControlState::TextField(EditState::from_init(value.into(), String::new(), 0, false)),
    );
    scene.focused_node = Some(NodeId(tf));
    (h, tf)
}

/// 读 TextField value（断言为 TextField，否则 panic）。
fn textfield_value(h: *mut StageHandle, node: u32) -> String {
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(node)) {
        Some(ControlState::TextField(e)) => e.value.clone(),
        _ => panic!("node {node} is not a TextField"),
    }
}

/// set_text_input 推 UTF-32 codepoints → tick → 插入聚焦 TextField 末尾。
/// 初始 value "ab" + codepoints ['b','c'] → "ab" + "bc" = "abbc"。
#[test]
fn ffi_text_input_inserts_into_focused_textfield() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    let cps = [b'b' as u32, b'c' as u32];
    assert_eq!(
        loomgui_stage_set_text_input(h, cps.as_ptr(), 2),
        0,
        "set_text_input rc"
    );
    loomgui_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "abbc");
    loomgui_stage_free(h);
}

/// null/len=0 → 清空 pending（no-op），不 UB。返 0。
#[test]
fn ffi_set_text_input_null_is_noop() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    let rc = loomgui_stage_set_text_input(h, std::ptr::null(), 0);
    assert_eq!(rc, 0, "null/len=0 must return 0 (no-op)");
    loomgui_stage_tick(h, 0.0);
    // 无字符插入，value 不变
    assert_eq!(textfield_value(h, tf), "ab");
    loomgui_stage_free(h);
}

/// null 句柄 → -1（不 panic）。
#[test]
fn ffi_set_text_input_null_handle_err() {
    let rc = loomgui_stage_set_text_input(std::ptr::null_mut(), std::ptr::null(), 0);
    assert_eq!(rc, -1, "null handle must return -1");
}

// ===== IME composition (set_composition / commit_composition / get_cursor_rect) =====
//
// IME 渠道：后端读 Input.compositionString 回灌 core，core 把 composition 拼进显示文本
// （measure + render 同源），提交时落定进 value。下划线由 Task 12 的 composition 分支按
// display 字节区间画（此处验 composition 进了显示文本 + 提交落定 + 光标矩形可读）。

/// 读 TextField 的 cached TextLayout 的 text_width（measure_text_controls 在 solve 后写入）。
/// 无缓存 / 非 TextField/TextArea → None。
fn textfield_text_width(h: *const StageHandle, node: u32) -> Option<f32> {
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    if !matches!(
        scene.controls.get(NodeId(node)),
        Some(ControlState::TextField(_) | ControlState::TextArea(_))
    ) {
        return None;
    }
    scene
        .text_layouts
        .get(NodeId(node).index())
        .and_then(|l| l.as_ref().map(|l| l.text_width))
}

/// set_composition 把预提交文本拼进显示文本：value "ab" + composition "ni" → 测到 "abni"
/// 的 text_width（严格大于无 composition 时 "ab" 的 text_width）。
#[test]
fn composition_spliced_into_display() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    // 先 tick 拿无 composition 基线 text_width（measure "ab"）。
    loomgui_stage_tick(h, 0.0);
    let baseline = textfield_text_width(h, tf).expect("baseline layout measured");
    assert!(baseline > 0.0, "non-empty value must measure > 0");

    // 设 composition "ni" 在 value 末尾（pos=2）。
    let s = b"ni";
    let rc = loomgui_stage_set_composition(h, tf, s.as_ptr(), s.len(), 2);
    assert_eq!(rc, 0, "set_composition rc");
    loomgui_stage_tick(h, 0.0);
    // composition 拼进显示文本 → text_width 应反映 "abni"（4 字符），严格大于 "ab"。
    let with_comp = textfield_text_width(h, tf).expect("composition layout measured");
    assert!(
        with_comp > baseline,
        "composition spliced in: width {with_comp} must exceed baseline {baseline} (abni > ab)"
    );
    loomgui_stage_free(h);
}

/// commit_composition 落定：composition "ni" 提交后并入 value，value "ab" → "abni"。
#[test]
fn commit_composition_appends_to_value() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    let s = b"ni";
    assert_eq!(
        loomgui_stage_set_composition(h, tf, s.as_ptr(), s.len(), 2),
        0,
        "set_composition rc"
    );
    assert_eq!(
        loomgui_stage_commit_composition(h, tf),
        1,
        "commit returns 1 (changed) when a composition was pending"
    );
    loomgui_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "abni");
    loomgui_stage_free(h);
}

/// 无 composition 时 commit 返 0（未改），value 不变。
#[test]
fn commit_composition_noop_when_none() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    assert_eq!(
        loomgui_stage_commit_composition(h, tf),
        0,
        "commit without composition returns 0 (no change)"
    );
    loomgui_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "ab");
    loomgui_stage_free(h);
}

/// get_cursor_rect 返 0（成功）并写出有限光标矩形（x/y/w/h 皆 finite，h>0）。
/// 光标在 value 末尾，measure 已缓存 TextLayout → 应能定位。null 句柄 → -1。
#[test]
fn get_cursor_rect_returns_finite_rect() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    loomgui_stage_tick(h, 0.0);
    let mut rect = CursorRectRepr::default();
    let rc = loomgui_stage_get_cursor_rect(h, tf, &mut rect);
    assert_eq!(rc, 0, "get_cursor_rect rc");
    assert!(rect.x.is_finite(), "cursor rect x finite");
    assert!(rect.y.is_finite(), "cursor rect y finite");
    assert!(rect.h > 0.0, "cursor rect h = line height > 0");
    loomgui_stage_free(h);
}

/// null 句柄 / 无效 node 的健壮性（不 panic）。
#[test]
fn composition_ffi_null_handle_err() {
    let rc = loomgui_stage_set_composition(std::ptr::null_mut(), 0, b"x".as_ptr(), 1, 0);
    assert_eq!(rc, -1, "null handle set_composition -> -1");
    assert_eq!(
        loomgui_stage_commit_composition(std::ptr::null_mut(), 0),
        -1,
        "null handle commit -> -1"
    );
    assert_eq!(
        loomgui_stage_get_cursor_rect(std::ptr::null_mut(), 0, std::ptr::null_mut()),
        -1,
        "null handle get_cursor_rect -> -1"
    );
}

// ===== clipboard (loomgui_register_clipboard + Ctrl+C/X/V routing) =====
//
// 剪贴板走 host callback 注册：core 是 cdylib 不能 extern 调宿主，后端注册 set/get 回调。
// 测试注册一对 Rust fn 做内存 round-trip（不依赖真实系统剪贴板），然后经 set_key_input +
// tick 驱动 Ctrl+C/X/V，验证 process_keys 路由 + 剪贴板读写 + ValueChanged 事件。
// 剪贴板测试共享全局 callback 槽 + 测试 buffer，须串行（CLIP_FFI_TEST_LOCK）。

use loomgui_core::input::{KeyEvent, EVT_VALUE_CHANGED, MOD_CTRL};
use std::sync::Mutex;

/// 串行所有剪贴板 FFI 测试（共享全局 callback + 测试 buffer）。
static CLIP_FFI_TEST_LOCK: Mutex<()> = Mutex::new(());

/// 测试用剪贴板内容（ffi test_set 写 / ffi test_get 读）。
static FFI_TEST_CLIP: Mutex<String> = Mutex::new(String::new());

/// test_get 把剪贴板内容 leak 成 'static 切片返稳定指针（host 须持有缓冲区至下次 get；
/// 测试小量 leak 可接受，避免 dangling / static_mut_refs lint）。
static FFI_TEST_GET_BYTES: Mutex<&'static [u8]> = Mutex::new(&[]);

/// host 「写剪贴板」回调：拷 (ptr,len) 进 FFI_TEST_CLIP。返 0。
unsafe extern "C" fn ffi_test_set(ptr: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    *FFI_TEST_CLIP.lock().unwrap() = String::from_utf8_lossy(bytes).into_owned();
    0
}

/// host 「读剪贴板」回调：把 FFI_TEST_CLIP 内容 leak 一份返稳定指针 + len。返 0。
unsafe extern "C" fn ffi_test_get(out: *mut *mut u8, out_len: *mut usize) -> i32 {
    let s = FFI_TEST_CLIP.lock().unwrap().clone();
    let leaked: &'static [u8] = s.into_bytes().leak();
    *FFI_TEST_GET_BYTES.lock().unwrap() = leaked;
    unsafe {
        *out = leaked.as_ptr() as *mut u8;
        *out_len = leaked.len();
    }
    0
}

/// 注册测试 callback + 取串行锁。返回 (锁 guard)。调方持有至测试体结束。
fn ffi_clip_setup() -> std::sync::MutexGuard<'static, ()> {
    let g = CLIP_FFI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *FFI_TEST_CLIP.lock().unwrap() = String::new();
    loomgui_register_clipboard(Some(ffi_test_set), Some(ffi_test_get));
    g
}

/// 建根 div + 一个聚焦的 TextField（kind 真为 TextField + 初始 value + 可选选区）。
///
/// process_keys 检查 `n.kind == TextField/TextArea` 才路由控制键——helper 创建的 div 节点
/// kind 是 Container，须手工把 kind 改成 TextField（control state 已注入但 kind 未同步）。
/// selection = Some((anchor, cursor)) 设选区；None 则 cursor/anchor 在末尾（from_init 默认）。
fn make_stage_with_focused_textfield_selection(
    value: &str,
    selection: Option<(usize, usize)>,
) -> (*mut StageHandle, u32) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let tf = loomgui_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(tf, 0xFFFF_FFFF, "create textfield node ok");
    loomgui_stage_append_child(h, root, tf);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    let mut e = EditState::from_init(value.into(), String::new(), 0, false);
    if let Some((anchor, cursor)) = selection {
        e.anchor = anchor;
        e.cursor = cursor;
    }
    scene
        .controls
        .ensure(NodeId(tf), ControlState::TextField(e));
    // process_keys 验 kind——div 节点 kind=Container，手工改成 TextField 让控制键路由生效。
    if let Some(n) = scene.get_mut(NodeId(tf)) {
        n.kind = NodeKind::TextField;
    }
    scene.focused_node = Some(NodeId(tf));
    (h, tf)
}

/// 推一个 keydown 事件 + tick。tick 后 last_events 更新。
fn send_keydown_tick(h: *mut StageHandle, key_code: u32, modifiers: u8) {
    let ke = KeyEvent {
        key_code,
        modifiers,
        is_down: true,
        pad: [0, 0],
    };
    loomgui_stage_set_key_input(h, &ke, 1);
    loomgui_stage_tick(h, 0.0);
}

/// 读本帧事件列表（拷贝出来解借 stage 句柄）。borrow_events 的 out_len 是事件元素数
/// （非字节数——FFI 文档「C 侧按 len * sizeof(EventRecord) 切片读」），直接当 count 用。
fn drain_events(h: *const StageHandle) -> Vec<EventRecord> {
    let mut len = 0usize;
    let ptr = loomgui_stage_borrow_events(h, &mut len);
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    // len = 事件数（非字节）。直接构造切片。
    let slice = unsafe { std::slice::from_raw_parts(ptr as *const EventRecord, len) };
    slice.to_vec()
}

/// Ctrl+C 复制选区到剪贴板，不改 value，不发 ValueChanged。
#[test]
fn ffi_ctrl_c_copies_selection() {
    let _g = ffi_clip_setup();
    let (h, tf) = make_stage_with_focused_textfield_selection("hello", Some((0, 3)));
    send_keydown_tick(h, loomgui_core::input::KEY_C, MOD_CTRL);
    assert_eq!(
        textfield_value(h, tf),
        "hello",
        "copy does not change value"
    );
    assert_eq!(*FFI_TEST_CLIP.lock().unwrap(), "hel", "clipboard filled");
    // Ctrl+C 不发 ValueChanged（copy 非破坏）。
    let events = drain_events(h);
    assert!(
        events.iter().all(|e| e.event_type != EVT_VALUE_CHANGED),
        "Ctrl+C emits no ValueChanged"
    );
    loomgui_stage_free(h);
}

/// Ctrl+X 剪切：选区进剪贴板 + value 删除 + 发 ValueChanged。
#[test]
fn ffi_ctrl_x_cuts_and_emits_value_changed() {
    let _g = ffi_clip_setup();
    let (h, tf) = make_stage_with_focused_textfield_selection("hello", Some((1, 4)));
    send_keydown_tick(h, loomgui_core::input::KEY_X, MOD_CTRL);
    assert_eq!(textfield_value(h, tf), "ho", "selection removed by cut");
    assert_eq!(
        *FFI_TEST_CLIP.lock().unwrap(),
        "ell",
        "clipboard has cut text"
    );
    let events = drain_events(h);
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf),
        "Ctrl+X emits ValueChanged when value changes"
    );
    loomgui_stage_free(h);
}

/// Ctrl+V 粘贴：把剪贴板插到光标，value 改变 + 发 ValueChanged。
#[test]
fn ffi_ctrl_v_pastes_and_emits_value_changed() {
    let _g = ffi_clip_setup();
    *FFI_TEST_CLIP.lock().unwrap() = "hi".into();
    let (h, tf) = make_stage_with_focused_textfield_selection("XY", None);
    send_keydown_tick(h, loomgui_core::input::KEY_V, MOD_CTRL);
    assert_eq!(textfield_value(h, tf), "XYhi", "clipboard pasted at cursor");
    let events = drain_events(h);
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf),
        "Ctrl+V emits ValueChanged when value changes"
    );
    loomgui_stage_free(h);
}

/// Ctrl+A 全选（Task 10 已有）+ Ctrl+C 复制：验证两步组合工作。
#[test]
fn ffi_ctrl_a_then_ctrl_c_copies_all() {
    let _g = ffi_clip_setup();
    let (h, tf) = make_stage_with_focused_textfield_selection("hello", None);
    // Ctrl+A 全选（cursor/anchor → 0..len），再 Ctrl+C 复制。
    send_keydown_tick(h, loomgui_core::input::KEY_A, MOD_CTRL);
    send_keydown_tick(h, loomgui_core::input::KEY_C, MOD_CTRL);
    assert_eq!(textfield_value(h, tf), "hello", "value unchanged");
    assert_eq!(
        *FFI_TEST_CLIP.lock().unwrap(),
        "hello",
        "whole value copied"
    );
    loomgui_stage_free(h);
}

/// 未注册回调时 Ctrl+V 读空串 → no-op（value 不变），不 panic。
#[test]
fn ffi_ctrl_v_noop_when_clipboard_unregistered() {
    let _g = CLIP_FFI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    loomgui_register_clipboard(None, None);
    let (h, tf) = make_stage_with_focused_textfield_selection("XY", None);
    send_keydown_tick(h, loomgui_core::input::KEY_V, MOD_CTRL);
    assert_eq!(
        textfield_value(h, tf),
        "XY",
        "paste no-op without clipboard registration"
    );
    loomgui_stage_free(h);
    // 复原测试 callback。
    loomgui_register_clipboard(Some(ffi_test_set), Some(ffi_test_get));
}

/// loomgui_register_clipboard 是 process-scoped 全局——测完须清理，防污染其他测试。
/// 此测试放最后，重注册成 ffi_test_* 以保全局处于已知态（防御性，非断言驱动）。
#[test]
fn ffi_clipboard_global_left_registered() {
    let _g = CLIP_FFI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    loomgui_register_clipboard(Some(ffi_test_set), Some(ffi_test_get));
}

// ===== control text / selection / placeholder / readonly / maxlength FFI (Task 15) =====
//
// 业务侧（C# 投影层）经这些 setter/getter 读写 TextField/TextArea 的 value/selection/
// placeholder/readonly/maxlength。set_control_text 直接替换 value（不走 insert_text 的
// cursor 插入路径）+ 光标移到末尾 + 标 dirty；setter-via-FFI 也产 ValueChanged（与
// C# Value 属性 setter 契约一致），经 Stage.pending_events 缓冲，下 tick 入 last_events。

/// 读 TextField EditState 的 (cursor, anchor) 选区（断言为 TextField）。
fn textfield_selection(h: *mut StageHandle, node: u32) -> (usize, usize) {
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(node)) {
        Some(ControlState::TextField(e)) => (e.cursor, e.anchor),
        _ => panic!("node {node} is not a TextField"),
    }
}

/// set_control_text 替换 value + 光标移末尾。get_control_text 读回经 ptr+len out-param。
#[test]
fn ffi_set_get_control_text_round_trip() {
    let (h, tf) = make_stage_with_focused_textfield("old");
    let new = b"new";
    assert_eq!(
        loomgui_stage_set_control_text(h, tf, new.as_ptr(), new.len()),
        0,
        "set_control_text rc"
    );
    // 光标/anchor 移到 value 末尾（new.len() = 3）。
    assert_eq!(textfield_selection(h, tf), (3, 3), "cursor moved to end");
    // tick 让 measure 重算 + flush pending_events。
    loomgui_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "new", "value replaced");

    // get_control_text：buf 足够 → rc=0，写入 buf[..len]。
    let mut buf = [0u8; 16];
    let mut len = 0usize;
    assert_eq!(
        loomgui_stage_get_control_text(h, tf, buf.as_mut_ptr(), buf.len(), &mut len),
        0,
        "get_control_text rc (buf enough)"
    );
    assert_eq!(len, 3, "written len = value byte len");
    assert_eq!(&buf[..len], b"new", "round-trip value bytes");
    loomgui_stage_free(h);
}

/// get_control_text buf 不够 → rc=-2（所需大小），不改 buf（双调法探大小）。
#[test]
fn ffi_get_control_text_buf_too_small_returns_needed() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    let mut buf = [0u8; 2];
    let mut len = 0usize;
    let rc = loomgui_stage_get_control_text(h, tf, buf.as_mut_ptr(), buf.len(), &mut len);
    assert_eq!(rc, -2, "buf too small returns -2");
    assert_eq!(len, 5, "len reports needed size (b\"hello\" = 5)");
    loomgui_stage_free(h);
}

/// set_control_text 产 ValueChanged（经 pending_events → 下 tick 入 last_events）。
#[test]
fn ffi_set_control_text_emits_value_changed() {
    let (h, tf) = make_stage_with_focused_textfield("old");
    let new = b"new";
    assert_eq!(
        loomgui_stage_set_control_text(h, tf, new.as_ptr(), new.len()),
        0
    );
    loomgui_stage_tick(h, 0.0);
    let events = drain_events(h);
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf),
        "set_control_text emits ValueChanged on next tick"
    );
    loomgui_stage_free(h);
}

/// set_selection 设 (anchor, cursor) 选区（可反向 anchor>cursor）。rc=0。
#[test]
fn ffi_set_selection() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    // 选 "el"（字节 [1,3)）。
    assert_eq!(
        loomgui_stage_set_selection(h, tf, 1, 3),
        0,
        "set_selection rc"
    );
    assert_eq!(textfield_selection(h, tf), (3, 1), "cursor=3 anchor=1");
    loomgui_stage_free(h);
}

/// get_selection 读回 (start, end) 闭区间（min/max 归一）。
#[test]
fn ffi_get_selection() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    // 反向选区 anchor=4 cursor=1 → get 归一为 (1, 4)。
    assert_eq!(loomgui_stage_set_selection(h, tf, 4, 1), 0);
    let mut start = 0usize;
    let mut end = 0usize;
    assert_eq!(loomgui_stage_get_selection(h, tf, &mut start, &mut end), 0);
    assert_eq!(
        (start, end),
        (1, 4),
        "get_selection normalizes to [min,max]"
    );
    loomgui_stage_free(h);
}

/// set_control_placeholder 改 placeholder，value 为空时 render 用它。get 回读。
#[test]
fn ffi_set_get_control_placeholder() {
    let (h, tf) = make_stage_with_focused_textfield(""); // 空 value
    let ph = b"type here";
    assert_eq!(
        loomgui_stage_set_control_placeholder(h, tf, ph.as_ptr(), ph.len()),
        0,
        "set_control_placeholder rc"
    );
    loomgui_stage_tick(h, 0.0);
    // 读回 placeholder。
    let mut buf = [0u8; 16];
    let mut len = 0usize;
    assert_eq!(
        loomgui_stage_get_control_placeholder(h, tf, buf.as_mut_ptr(), buf.len(), &mut len),
        0
    );
    assert_eq!(&buf[..len], b"type here", "placeholder round-trip");
    loomgui_stage_free(h);
}

/// set_control_readonly 切换 readonly 标志（聚焦时光标不画）。
#[test]
fn ffi_set_control_readonly() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    assert_eq!(
        loomgui_stage_set_control_readonly(h, tf, true),
        0,
        "set readonly"
    );
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(tf)) {
        Some(ControlState::TextField(e)) => assert!(e.readonly, "readonly set true"),
        _ => panic!("not a textfield"),
    }
    loomgui_stage_free(h);
}

/// set_control_maxlength 改 max_length（UTF-8 字符上限）。0 = 无限。
#[test]
fn ffi_set_control_maxlength() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    assert_eq!(
        loomgui_stage_set_control_maxlength(h, tf, 5),
        0,
        "set maxlength=5"
    );
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(tf)) {
        Some(ControlState::TextField(e)) => assert_eq!(e.max_length, 5),
        _ => panic!("not a textfield"),
    }
    loomgui_stage_free(h);
}

/// set_control_text 非文本控件 → -1（不 panic）。
#[test]
fn ffi_set_control_text_non_text_control_err() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    // div 无 ControlState → -1。
    assert_eq!(
        loomgui_stage_set_control_text(h, root, b"x".as_ptr(), 1),
        -1,
        "non-text-control node returns -1"
    );
    loomgui_stage_free(h);
}

/// null 句柄 / 无效 node 健壮性（不 panic，返 -1）。
#[test]
fn ffi_control_text_null_handle_err() {
    assert_eq!(
        loomgui_stage_set_control_text(std::ptr::null_mut(), 0, b"x".as_ptr(), 1),
        -1
    );
    let mut len = 0usize;
    assert_eq!(
        loomgui_stage_get_control_text(std::ptr::null(), 0, std::ptr::null_mut(), 0, &mut len),
        -1
    );
}

// ===== TextArea variant 保持回归（Fix Round 1）=====
//
// 5 个改写 setter（set_control_text/set_selection/set_control_placeholder/
// set_control_readonly/set_control_maxlength）原以 `match state { TextField(e)|TextArea(e) =>
// ..., _ => ...; ControlState::TextField(e) }` 重建——会把 TextArea 节点的 ControlState
// 改写成 TextField，破坏 ControlState/NodeKind variant 一致性。现改 in-place get_mut 原地改。
// 这两个回归测试锁不变量：在 TextArea 节点上调 setter 后，ControlState 仍为 TextArea。

/// 测试辅助：建根 div + 一个 TextArea（ControlState::TextArea + NodeKind::TextArea），
/// 返回 (handle, textarea_node)。不对焦（setter 不依赖焦点）。
fn make_stage_with_textarea(value: &str) -> (*mut StageHandle, u32) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let ta = loomgui_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(ta, 0xFFFF_FFFF, "create textarea node ok");
    loomgui_stage_append_child(h, root, ta);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(ta),
        ControlState::TextArea(EditState::from_init(value.into(), String::new(), 0, false)),
    );
    // kind 同步为 TextArea（保 ControlState/NodeKind 一致，复现真实不变量场景）。
    if let Some(n) = scene.get_mut(NodeId(ta)) {
        n.kind = NodeKind::TextArea;
    }
    (h, ta)
}

/// 回归：set_control_text 在 TextArea 节点上调用后，ControlState 仍为 TextArea（不被
/// 改写成 TextField）。同时验 value/光标 已正确改。锁 variant 一致性不变量。
#[test]
fn ffi_set_control_text_preserves_textarea_variant() {
    let (h, ta) = make_stage_with_textarea("old");
    assert_eq!(
        loomgui_stage_set_control_text(h, ta, b"new".as_ptr(), 3),
        0,
        "set_control_text rc"
    );
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(ta)) {
        Some(ControlState::TextArea(e)) => {
            assert_eq!(e.value, "new", "value replaced");
            assert_eq!(e.cursor, 3, "cursor at end");
            assert_eq!(e.anchor, 3, "anchor at end");
        }
        Some(ControlState::TextField(_)) => {
            panic!("TextArea node rewritten to TextField ControlState by set_control_text");
        }
        _ => panic!("node {ta} lost its control state"),
    }
    loomgui_stage_free(h);
}

/// 回归：set_selection 在 TextArea 节点上调用后，ControlState 仍为 TextArea，选区正确。
#[test]
fn ffi_set_selection_preserves_textarea_variant() {
    let (h, ta) = make_stage_with_textarea("hello");
    assert_eq!(
        loomgui_stage_set_selection(h, ta, 1, 3),
        0,
        "set_selection rc"
    );
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(ta)) {
        Some(ControlState::TextArea(e)) => {
            assert_eq!((e.cursor, e.anchor), (3, 1), "selection set");
        }
        Some(ControlState::TextField(_)) => {
            panic!("TextArea node rewritten to TextField ControlState by set_selection");
        }
        _ => panic!("node {ta} lost its control state"),
    }
    loomgui_stage_free(h);
}
