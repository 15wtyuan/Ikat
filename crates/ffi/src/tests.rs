use super::*;
use crate::test_helpers::stage_new_with_dejavu;
use ikat_core::input::EventRecord;
use ikat_core::scene::node::{ControlState, EditState, NodeKind};
use ikat_core::scene::NodeId;
use ikat_core::style::resolved::DisplayMode;
use std::ffi::CStr;

/// 测试辅助：建根 div 后直接往 scene.controls 注入 Progress 状态。
/// FFI 表面无 control_init setter（打包期产物），故测试侧手工填。
fn make_progress_stage(value: f32, max: f32) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(root),
        ControlState::Progress {
            value,
            min: 0.0,
            max,
            indeterminate: false,
        },
    );
    (h, root)
}

/// 测试辅助：建根 div 后直接往 scene.controls 注入 Slider 状态。
fn make_slider_stage(value: f32, min: f32, max: f32, step: f32) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
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
fn make_toggle_stage(checked: bool) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
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
        let s = CStr::from_ptr(ikat_version() as *const i8);
        assert_eq!(s.to_str().unwrap(), "v1e");
    }
}

/// get_node_kind FFI round-trip + 哨兵不撞：div → Container(0)；无效 node → rc 非 0。
/// 关键：return-code 模式（不用 -> u8 + 0 哨兵），否则 Container=0 会与「不存在」撞。
#[test]
fn ffi_get_node_kind_div_and_invalid() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let mut kind: u8 = 255;
    let rc = ikat_stage_get_node_kind(h, root, &mut kind);
    assert_eq!(rc, 0, "div kind rc");
    assert_eq!(kind, NodeKind::Container as u8, "div == Container(0)");
    // 无效 node → rc 非 0（关键：不撞 Container=0 哨兵）。
    let rc_bad = ikat_stage_get_node_kind(h, u64::MAX, &mut kind);
    assert_ne!(
        rc_bad, 0,
        "invalid node must not return 0 (collides with Container)"
    );
    ikat_stage_free(h);
}

/// get_node_computed_style FFI round-trip：div 默认 → opacity=1, display=Flex；无效 node → rc 非 0。
#[test]
fn ffi_get_node_computed_style_div() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let mut repr = ComputedNodeStyleRepr::default();
    let rc = ikat_stage_get_node_computed_style(h, root, &mut repr);
    assert_eq!(rc, 0, "computed style rc");
    assert_eq!(repr.opacity, 1.0);
    assert_eq!(repr.display_mode, DisplayMode::Block as u8);
    // 无效 node → rc 非 0。
    let rc_bad = ikat_stage_get_node_computed_style(h, u64::MAX, &mut repr);
    assert_ne!(rc_bad, 0);
    ikat_stage_free(h);
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
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let rc = ikat_stage_get_node_kind(h, root, std::ptr::null_mut());
    assert_ne!(
        rc, 0,
        "get_node_kind: null out + existing node must not return 0"
    );
    let rc_c = ikat_stage_get_node_computed_style(h, root, std::ptr::null_mut());
    assert_ne!(
        rc_c, 0,
        "get_node_computed_style: null out + existing node must not return 0"
    );
    ikat_stage_free(h);
}

// caller 传 `ptr::null(), 0` 表「空字符串」（C# 默认 string、null 字面）。
// slice::from_raw_parts(null, 0) 是 UB（即使 len=0），故 FFI 必须 null-safe 兜底为 ""。
// 跑过即证明 null 被守卫；UB 在 Miri/ASAN 下会 crash，普通 cargo test 通常静默走过。

/// create_root(null css) 必须成功（= 空 inline css），不 UB。
#[test]
fn create_root_null_css_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, std::ptr::null(), 0);
    assert_ne!(
        root,
        u64::MAX,
        "create_root with null css must succeed (treated as empty css)"
    );
    ikat_stage_free(h);
}

/// create_node(null css) 必须成功，不 UB。
#[test]
fn create_node_null_css_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, std::ptr::null(), 0);
    assert_ne!(
        node,
        u64::MAX,
        "create_node with null css must succeed (treated as empty css)"
    );
    ikat_stage_free(h);
}

/// set_inline_override(null css) 必须返 0（= 空覆盖，no-op 语义），不 UB。
#[test]
fn set_inline_override_null_css_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let rc = ikat_stage_set_inline_override(h, root, std::ptr::null(), 0);
    assert_eq!(
        rc, 0,
        "set_inline_override with null css must succeed (treated as empty css)"
    );
    ikat_stage_free(h);
}

/// set_text(null text) 必须返 0（= 清空 Text 内容），不 UB。
#[test]
fn set_text_null_does_not_ub() {
    let h = stage_new_with_dejavu(100.0, 100.0);
    let text = ikat_stage_create_node(h, b"span".as_ptr(), 4, std::ptr::null(), 0);
    assert_ne!(text, u64::MAX, "create span ok");
    let rc = ikat_stage_set_text(h, text, std::ptr::null(), 0);
    assert_eq!(
        rc, 0,
        "set_text with null text must succeed (treated as empty content)"
    );
    ikat_stage_free(h);
}

/// ProgressBar set_control_value(90) → get_control_value == 90；超 max 的 150 被 clamp 到 100。
/// 验 return-code + out-param 模式（避免 Container=0 哨兵撞）。
#[test]
fn ffi_set_get_control_value_progress() {
    let (h, node) = make_progress_stage(70.0, 100.0);
    // 合法区间：90 → 90
    let rc = ikat_stage_set_control_value(h, node, 90.0);
    assert_eq!(rc, 0, "set_control_value(90) rc");
    let mut out = 0.0f32;
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0, "get_control_value rc");
    assert!((out - 90.0).abs() < 0.001, "value == 90, got {out}");
    // 超 max：150 → clamp 100
    let rc = ikat_stage_set_control_value(h, node, 150.0);
    assert_eq!(rc, 0, "set_control_value(150) rc");
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 100.0).abs() < 0.001,
        "150 clamped to max 100, got {out}"
    );
    // 负值：-10 → clamp 0
    let rc = ikat_stage_set_control_value(h, node, -10.0);
    assert_eq!(rc, 0);
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 0.0).abs() < 0.001, "-10 clamped to 0, got {out}");
    ikat_stage_free(h);
}

/// Slider set_control_value：clamp + step 量化（step=5 → 83 → 85）。
#[test]
fn ffi_set_get_control_value_slider() {
    let (h, node) = make_slider_stage(50.0, 0.0, 100.0, 5.0);
    // 83 被 step=5 量化到 85（最近的 step 边界）
    let rc = ikat_stage_set_control_value(h, node, 83.0);
    assert_eq!(rc, 0, "set_control_value(83) rc");
    let mut out = 0.0f32;
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 85.0).abs() < 0.001,
        "83 quantized to 85 (step=5), got {out}"
    );
    // 超 max clamp
    let rc = ikat_stage_set_control_value(h, node, 200.0);
    assert_eq!(rc, 0);
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 100.0).abs() < 0.001,
        "200 clamped to max 100, got {out}"
    );
    ikat_stage_free(h);
}

/// 非 value 控件（Toggle）set/get_control_value → -1（语义不适用）。
#[test]
fn ffi_control_value_non_value_control_err() {
    let (h, node) = make_toggle_stage(false);
    let rc = ikat_stage_set_control_value(h, node, 50.0);
    assert_eq!(rc, -1, "Toggle set_control_value → -1");
    let mut out = -1.0f32;
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, -1, "Toggle get_control_value → -1");
    ikat_stage_free(h);
}

/// get_control_value null out → 非 0（rc=0 严格意味 *out 已填）。
#[test]
fn ffi_get_control_value_null_out_err() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    let rc = ikat_stage_get_control_value(h, node, std::ptr::null_mut());
    assert_ne!(rc, 0, "null out must not return 0");
    ikat_stage_free(h);
}

/// Toggle set_control_checked(true) → get_control_checked == true。
#[test]
fn ffi_set_get_control_checked_toggle() {
    let (h, node) = make_toggle_stage(false);
    let rc = ikat_stage_set_control_checked(h, node, true);
    assert_eq!(rc, 0, "set_control_checked(true) rc");
    let mut out = false;
    let rc = ikat_stage_get_control_checked(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(out, "checked == true");
    // 翻回 false
    let rc = ikat_stage_set_control_checked(h, node, false);
    assert_eq!(rc, 0);
    let rc = ikat_stage_get_control_checked(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(!out, "checked == false");
    ikat_stage_free(h);
}

/// set_control_checked 对非 Toggle/Radio（Progress）→ -1。
#[test]
fn ffi_set_control_checked_non_check_control_err() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    let rc = ikat_stage_set_control_checked(h, node, true);
    assert_eq!(rc, -1, "Progress set_control_checked → -1");
    let mut out = true;
    let rc = ikat_stage_get_control_checked(h, node, &mut out);
    assert_eq!(rc, -1, "Progress get_control_checked → -1");
    ikat_stage_free(h);
}

/// Progress set/get_control_max：max 100 → 200，读回 200。
#[test]
fn ffi_set_get_control_max_progress() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    let rc = ikat_stage_set_control_max(h, node, 200.0);
    assert_eq!(rc, 0, "set_control_max(200) rc");
    let mut out = 0.0f32;
    let rc = ikat_stage_get_control_max(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 200.0).abs() < 0.001, "max == 200, got {out}");
    ikat_stage_free(h);
}

/// Slider set/get_control_min/step：min 0 → 10，step 0 → 2。
#[test]
fn ffi_set_get_control_min_step_slider() {
    let (h, node) = make_slider_stage(50.0, 0.0, 100.0, 0.0);
    let rc = ikat_stage_set_control_min(h, node, 10.0);
    assert_eq!(rc, 0, "set_control_min(10) rc");
    let mut out = 0.0f32;
    let rc = ikat_stage_get_control_min(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 10.0).abs() < 0.001, "min == 10, got {out}");

    let rc = ikat_stage_set_control_step(h, node, 2.0);
    assert_eq!(rc, 0, "set_control_step(2) rc");
    let rc = ikat_stage_get_control_step(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!((out - 2.0).abs() < 0.001, "step == 2, got {out}");
    ikat_stage_free(h);
}

/// #97 后 ProgressBar 开放 min（ARIA 填充域参与数学，set/get 生效）；step 仍无语义 → -1。
#[test]
fn ffi_control_min_step_progress_err() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    assert_eq!(ikat_stage_set_control_min(h, node, 10.0), 0);
    assert_eq!(ikat_stage_set_control_step(h, node, 2.0), -1);
    let mut out = 99.0f32;
    assert_eq!(ikat_stage_get_control_min(h, node, &mut out), 0);
    assert_eq!(out, 10.0);
    assert_eq!(ikat_stage_get_control_step(h, node, &mut out), -1);
    ikat_stage_free(h);
}

/// set_transform：写 user_transform，读回 node.user_transform.translate == [50, 0]。
/// 走 set_user_transform（dynamic.rs），不触发 solve（仅渲染/命中层）。
#[test]
fn ffi_set_transform_translates_user_transform() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let rc = ikat_stage_set_transform(h, root, 50.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0);
    assert_eq!(rc, 0, "set_transform rc");
    // 读回 node.user_transform（同 crate 可访私有字段，需 unsafe 解原指针）
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    let node_ref = scene.get(NodeId(root)).expect("node live");
    assert_eq!(node_ref.user_transform.translate, [50.0, 0.0]);
    assert_eq!(node_ref.user_transform.scale, [1.0, 1.0]);
    ikat_stage_free(h);
}

/// set_transform 带 origin（ox,oy）：写入 user_transform.origin 字段。
/// origin = 旋转/缩放原点（local 坐标），连接 C# NodeTransform.Origin。default origin=[0,0]。
#[test]
fn ffi_set_transform_stores_origin() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let rc = ikat_stage_set_transform(h, root, 10.0, 0.0, 1.0, 1.0, 0.0, 5.0, 5.0);
    assert_eq!(rc, 0, "set_transform rc");
    // 读回 node.user_transform.origin（同 crate 可访私有字段，需 unsafe 解原指针）
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    let node_ref = scene.get(NodeId(root)).expect("node live");
    assert_eq!(node_ref.user_transform.origin, [5.0, 5.0]);
    assert_eq!(node_ref.user_transform.translate, [10.0, 0.0]);
    ikat_stage_free(h);
}

/// set_transform 对不 live 节点 → -1（set_user_transform 返 Err）。
#[test]
fn ffi_set_transform_invalid_node_err() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let rc = ikat_stage_set_transform(h, u64::MAX, 10.0, 10.0, 1.0, 1.0, 0.0, 0.0, 0.0);
    assert_eq!(rc, -1, "invalid node set_transform → -1");
    ikat_stage_free(h);
}

/// set_control_max 对 Progress 传负 max 不可 panic（FFI 不可因 caller 输入 abort
/// 进程）；max guard 到 ≥0，rc=0，get_control_max 返 ≥0。
#[test]
fn ffi_set_control_max_negative_does_not_panic() {
    let (h, node) = make_progress_stage(50.0, 100.0);
    // 传负 max：旧实现 value.clamp(0.0, -5.0) 会 panic（min > max）
    let rc = ikat_stage_set_control_max(h, node, -5.0);
    assert_eq!(rc, 0, "set_control_max(-5) on Progress → rc=0 (guard to 0)");
    let mut out = -999.0f32;
    let rc = ikat_stage_get_control_max(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(out >= 0.0, "max guard to ≥0, got {out}");
    // value 也被重新 clamp 进 [0, 0] = 0，不悬空
    let mut v = -999.0f32;
    let rc = ikat_stage_get_control_value(h, node, &mut v);
    assert_eq!(rc, 0);
    assert!(v >= 0.0 && v <= out, "value clamp into [0,max], got {v}");
    ikat_stage_free(h);
}

/// set_control_value 对 Slider 量化后不可超 max：min=0,max=100,step=6,v=100 →
/// 量化得 102 > max，必须重新 clamp 回 100。
#[test]
fn ffi_set_control_value_slider_quantize_respects_max() {
    let (h, node) = make_slider_stage(0.0, 0.0, 100.0, 6.0);
    // 100 / 6 = 16.67 → round 17 → 17*6 = 102 > max 100（旧实现违反区间）
    let rc = ikat_stage_set_control_value(h, node, 100.0);
    assert_eq!(rc, 0);
    let mut out = 0.0f32;
    let rc = ikat_stage_get_control_value(h, node, &mut out);
    assert_eq!(rc, 0);
    assert!(
        out <= 100.0,
        "quantized value must not exceed max 100, got {out}"
    );
    assert!(
        out >= 0.0,
        "quantized value must not go below min 0, got {out}"
    );
    ikat_stage_free(h);
}

/// 测试辅助：建根 div + 一个聚焦的 TextField（初始 value），返回 (handle, textfield_node)。
/// FFI 表面无 control_init setter（打包期产物），故测试侧手工注入 ControlState + 设焦点。
/// 光标初始在 value 末尾（from_init 默认），便于在末尾追加的断言。
fn make_stage_with_focused_textfield(value: &str) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    // create_node 走 kind_from_tag 白名单（不含 input），改用 create_node_from_template 的
    // FFI 等价：先建个 div 再手工把 kind 改成 TextField 不现实——这里直接复用 create_node
    // 建 div 占位，再注入 TextField ControlState（kind 字段保留 div 不影响 insert_text：
    // insert_text 收 NodeKind 入参，测试侧显式传 TextField）。
    let tf = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(tf, u64::MAX, "create textfield node ok");
    ikat_stage_append_child(h, root, tf);
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
fn textfield_value(h: *mut StageHandle, node: u64) -> String {
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
        ikat_stage_set_text_input(h, cps.as_ptr(), 2),
        0,
        "set_text_input rc"
    );
    ikat_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "abbc");
    ikat_stage_free(h);
}

/// 中文（多字节 UTF-8）textinput → insert → tick（measure + render）不 panic。
/// 回归：showcase TextField 拼音选字崩溃调查——验证 core 端到端处理中文安全。
#[test]
fn ffi_text_input_chinese_into_focused_textfield() {
    let (h, tf) = make_stage_with_focused_textfield("");
    let cps = ['你' as u32, '好' as u32];
    assert_eq!(
        ikat_stage_set_text_input(h, cps.as_ptr(), 2),
        0,
        "set_text_input 中文 rc"
    );
    ikat_stage_tick(h, 0.0); // process_text_input + measure + render
    assert_eq!(textfield_value(h, tf), "你好");
    ikat_stage_free(h);
}

/// null/len=0 → 清空 pending（no-op），不 UB。返 0。
#[test]
fn ffi_set_text_input_null_is_noop() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    let rc = ikat_stage_set_text_input(h, std::ptr::null(), 0);
    assert_eq!(rc, 0, "null/len=0 must return 0 (no-op)");
    ikat_stage_tick(h, 0.0);
    // 无字符插入，value 不变
    assert_eq!(textfield_value(h, tf), "ab");
    ikat_stage_free(h);
}

/// null 句柄 → -1（不 panic）。
#[test]
fn ffi_set_text_input_null_handle_err() {
    let rc = ikat_stage_set_text_input(std::ptr::null_mut(), std::ptr::null(), 0);
    assert_eq!(rc, -1, "null handle must return -1");
}

// IME 渠道：后端读 Input.compositionString 回灌 core，core 把 composition 拼进显示文本
// （measure + render 同源），提交时落定进 value。下划线由 composition 分支按
// display 字节区间画（此处验 composition 进了显示文本 + 提交落定 + 光标矩形可读）。

/// 读 TextField 的 cached TextLayout 的 text_width（measure_text_controls 在 solve 后写入）。
/// 无缓存 / 非 TextField/TextArea → None。
fn textfield_text_width(h: *const StageHandle, node: u64) -> Option<f32> {
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
    ikat_stage_tick(h, 0.0);
    let baseline = textfield_text_width(h, tf).expect("baseline layout measured");
    assert!(baseline > 0.0, "non-empty value must measure > 0");

    // 设 composition "ni" 在 value 末尾（pos=2）。
    let s = b"ni";
    let rc = ikat_stage_set_composition(h, tf, s.as_ptr(), s.len(), 2);
    assert_eq!(rc, 0, "set_composition rc");
    ikat_stage_tick(h, 0.0);
    // composition 拼进显示文本 → text_width 应反映 "abni"（4 字符），严格大于 "ab"。
    let with_comp = textfield_text_width(h, tf).expect("composition layout measured");
    assert!(
        with_comp > baseline,
        "composition spliced in: width {with_comp} must exceed baseline {baseline} (abni > ab)"
    );
    ikat_stage_free(h);
}

/// commit_composition 落定：composition "ni" 提交后并入 value，value "ab" → "abni"。
#[test]
fn commit_composition_appends_to_value() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    let s = b"ni";
    assert_eq!(
        ikat_stage_set_composition(h, tf, s.as_ptr(), s.len(), 2),
        0,
        "set_composition rc"
    );
    assert_eq!(
        ikat_stage_commit_composition(h, tf),
        1,
        "commit returns 1 (changed) when a composition was pending"
    );
    ikat_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "abni");
    ikat_stage_free(h);
}

/// 无 composition 时 commit 返 0（未改），value 不变。
#[test]
fn commit_composition_noop_when_none() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    assert_eq!(
        ikat_stage_commit_composition(h, tf),
        0,
        "commit without composition returns 0 (no change)"
    );
    ikat_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "ab");
    ikat_stage_free(h);
}

/// get_cursor_rect 返 0（成功）并写出有限光标矩形（x/y/w/h 皆 finite，h>0）。
/// 光标在 value 末尾，measure 已缓存 TextLayout → 应能定位。null 句柄 → -1。
#[test]
fn get_cursor_rect_returns_finite_rect() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    ikat_stage_tick(h, 0.0);
    let mut rect = CursorRectRepr::default();
    let rc = ikat_stage_get_cursor_rect(h, tf, &mut rect);
    assert_eq!(rc, 0, "get_cursor_rect rc");
    assert!(rect.x.is_finite(), "cursor rect x finite");
    assert!(rect.y.is_finite(), "cursor rect y finite");
    assert!(rect.h > 0.0, "cursor rect h = line height > 0");
    ikat_stage_free(h);
}

/// 回归：cursor_rect 曾对已绝对的 layout_rect 再 apply_point(wm) → x = wm[4] + layout_rect.x
/// （双重计数），IME 候选窗偏到屏外（showcase settings 输入框 world x=3323 实际是 1661 翻倍）。
/// 修复后纯平移用 wm[4] 作原点（与 render arm 光标同源），cursor_rect.x = wm[4] + off_left + cx，
/// 不含 layout_rect.x。设 layout_rect.x=500 + wm=IDENTITY(wm[4]=0)，修复前 rect.x≈500+（含
/// layout_rect.x），修复后 rect.x≈小（off_left+cx，不含 500）。
#[test]
fn get_cursor_rect_pure_translation_no_double_count() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    ikat_stage_tick(h, 0.0);
    let n_id = NodeId(tf);
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        // 设 layout_rect.x=500（非 0，使双重 bug 可观测）+ world_transform=IDENTITY（wm[4]=0）。
        scene.get_mut(n_id).expect("node").layout_rect.x = 500.0;
        scene.world_transforms[n_id.index()] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    let mut rect = CursorRectRepr::default();
    let rc = ikat_stage_get_cursor_rect(h, tf, &mut rect);
    assert_eq!(rc, 0, "get_cursor_rect rc");
    // 修复前：rect.x = wm[4](0) + layout_rect.x(500) + off_left + cx ≈ 500+（双重）。
    // 修复后：rect.x = wm[4](0) + off_left + cx ≈ 小（不含 layout_rect.x=500）。
    assert!(
        rect.x < 100.0,
        "纯平移 wm[4]=0 时 cursor_rect.x 不应含 layout_rect.x（双重 bug），实际 rect.x={}",
        rect.x
    );
    ikat_stage_free(h);
}

/// null 句柄 / 无效 node 的健壮性（不 panic）。
#[test]
fn composition_ffi_null_handle_err() {
    let rc = ikat_stage_set_composition(std::ptr::null_mut(), 0, b"x".as_ptr(), 1, 0);
    assert_eq!(rc, -1, "null handle set_composition -> -1");
    assert_eq!(
        ikat_stage_commit_composition(std::ptr::null_mut(), 0),
        -1,
        "null handle commit -> -1"
    );
    assert_eq!(
        ikat_stage_get_cursor_rect(std::ptr::null_mut(), 0, std::ptr::null_mut()),
        -1,
        "null handle get_cursor_rect -> -1"
    );
}

// 剪贴板走 host callback 注册：core 是 cdylib 不能 extern 调宿主，后端注册 set/get 回调。
// 测试注册一对 Rust fn 做内存 round-trip（不依赖真实系统剪贴板），然后经 set_key_input +
// tick 驱动 Ctrl+C/X/V，验证 process_keys 路由 + 剪贴板读写 + ValueChanged 事件。
// 剪贴板测试共享全局 callback 槽 + 测试 buffer，须串行（CLIP_FFI_TEST_LOCK）。

use ikat_core::input::{KeyEvent, EVT_VALUE_CHANGED, MOD_CTRL};
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
    ikat_register_clipboard(Some(ffi_test_set), Some(ffi_test_get));
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
) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let tf = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(tf, u64::MAX, "create textfield node ok");
    ikat_stage_append_child(h, root, tf);
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
    ikat_stage_set_key_input(h, &ke, 1);
    ikat_stage_tick(h, 0.0);
}

/// 读本帧事件列表（拷贝出来解借 stage 句柄）。borrow_events 的 out_len 是事件元素数
/// （非字节数——FFI 文档「C 侧按 len * sizeof(EventRecord) 切片读」），直接当 count 用。
fn drain_events(h: *const StageHandle) -> Vec<EventRecord> {
    let mut len = 0usize;
    let ptr = ikat_stage_borrow_events(h, &mut len);
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr as *const EventRecord, len) };
    slice.to_vec()
}

/// Ctrl+C 复制选区到剪贴板，不改 value，不发 ValueChanged。
#[test]
fn ffi_ctrl_c_copies_selection() {
    let _g = ffi_clip_setup();
    let (h, tf) = make_stage_with_focused_textfield_selection("hello", Some((0, 3)));
    send_keydown_tick(h, ikat_core::input::KEY_C, MOD_CTRL);
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
    ikat_stage_free(h);
}

/// Ctrl+X 剪切：选区进剪贴板 + value 删除 + 发 ValueChanged。
#[test]
fn ffi_ctrl_x_cuts_and_emits_value_changed() {
    let _g = ffi_clip_setup();
    let (h, tf) = make_stage_with_focused_textfield_selection("hello", Some((1, 4)));
    send_keydown_tick(h, ikat_core::input::KEY_X, MOD_CTRL);
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
    ikat_stage_free(h);
}

/// Ctrl+V 粘贴：把剪贴板插到光标，value 改变 + 发 ValueChanged。
#[test]
fn ffi_ctrl_v_pastes_and_emits_value_changed() {
    let _g = ffi_clip_setup();
    *FFI_TEST_CLIP.lock().unwrap() = "hi".into();
    let (h, tf) = make_stage_with_focused_textfield_selection("XY", None);
    send_keydown_tick(h, ikat_core::input::KEY_V, MOD_CTRL);
    assert_eq!(textfield_value(h, tf), "XYhi", "clipboard pasted at cursor");
    let events = drain_events(h);
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf),
        "Ctrl+V emits ValueChanged when value changes"
    );
    ikat_stage_free(h);
}

/// Ctrl+A 全选 + Ctrl+C 复制：验证两步组合工作。
#[test]
fn ffi_ctrl_a_then_ctrl_c_copies_all() {
    let _g = ffi_clip_setup();
    let (h, tf) = make_stage_with_focused_textfield_selection("hello", None);
    // Ctrl+A 全选（cursor/anchor → 0..len），再 Ctrl+C 复制。
    send_keydown_tick(h, ikat_core::input::KEY_A, MOD_CTRL);
    send_keydown_tick(h, ikat_core::input::KEY_C, MOD_CTRL);
    assert_eq!(textfield_value(h, tf), "hello", "value unchanged");
    assert_eq!(
        *FFI_TEST_CLIP.lock().unwrap(),
        "hello",
        "whole value copied"
    );
    ikat_stage_free(h);
}

/// 未注册回调时 Ctrl+V 读空串 → no-op（value 不变），不 panic。
#[test]
fn ffi_ctrl_v_noop_when_clipboard_unregistered() {
    let _g = CLIP_FFI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ikat_register_clipboard(None, None);
    let (h, tf) = make_stage_with_focused_textfield_selection("XY", None);
    send_keydown_tick(h, ikat_core::input::KEY_V, MOD_CTRL);
    assert_eq!(
        textfield_value(h, tf),
        "XY",
        "paste no-op without clipboard registration"
    );
    ikat_stage_free(h);
    // 复原测试 callback。
    ikat_register_clipboard(Some(ffi_test_set), Some(ffi_test_get));
}

/// ikat_register_clipboard 是 process-scoped 全局——测完须清理，防污染其他测试。
/// 此测试放最后，重注册成 ffi_test_* 以保全局处于已知态（防御性，非断言驱动）。
#[test]
fn ffi_clipboard_global_left_registered() {
    let _g = CLIP_FFI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ikat_register_clipboard(Some(ffi_test_set), Some(ffi_test_get));
}

// 业务侧（C# 投影层）经这些 setter/getter 读写 TextField/TextArea 的 value/selection/
// placeholder/readonly/maxlength。set_control_text 直接替换 value（不走 insert_text 的
// cursor 插入路径）+ 光标移到末尾 + 标 dirty；setter-via-FFI 也产 ValueChanged（与
// C# Value 属性 setter 契约一致），经 Stage.pending_events 缓冲，下 tick 入 last_events。

/// 读 TextField EditState 的 (cursor, anchor) 选区（断言为 TextField）。
fn textfield_selection(h: *mut StageHandle, node: u64) -> (usize, usize) {
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
        ikat_stage_set_control_text(h, tf, new.as_ptr(), new.len()),
        0,
        "set_control_text rc"
    );
    // 光标/anchor 移到 value 末尾（new.len() = 3）。
    assert_eq!(textfield_selection(h, tf), (3, 3), "cursor moved to end");
    // tick 让 measure 重算 + flush pending_events。
    ikat_stage_tick(h, 0.0);
    assert_eq!(textfield_value(h, tf), "new", "value replaced");

    // get_control_text：buf 足够 → rc=0，写入 buf[..len]。
    let mut buf = [0u8; 16];
    let mut len = 0usize;
    assert_eq!(
        ikat_stage_get_control_text(h, tf, buf.as_mut_ptr(), buf.len(), &mut len),
        0,
        "get_control_text rc (buf enough)"
    );
    assert_eq!(len, 3, "written len = value byte len");
    assert_eq!(&buf[..len], b"new", "round-trip value bytes");
    ikat_stage_free(h);
}

/// get_control_text buf 不够 → rc=-2（所需大小），不改 buf（双调法探大小）。
#[test]
fn ffi_get_control_text_buf_too_small_returns_needed() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    let mut buf = [0u8; 2];
    let mut len = 0usize;
    let rc = ikat_stage_get_control_text(h, tf, buf.as_mut_ptr(), buf.len(), &mut len);
    assert_eq!(rc, -2, "buf too small returns -2");
    assert_eq!(len, 5, "len reports needed size (b\"hello\" = 5)");
    ikat_stage_free(h);
}

/// set_control_text 产 ValueChanged（经 pending_events → 下 tick 入 last_events）。
#[test]
fn ffi_set_control_text_emits_value_changed() {
    let (h, tf) = make_stage_with_focused_textfield("old");
    let new = b"new";
    assert_eq!(
        ikat_stage_set_control_text(h, tf, new.as_ptr(), new.len()),
        0
    );
    ikat_stage_tick(h, 0.0);
    let events = drain_events(h);
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf),
        "set_control_text emits ValueChanged on next tick"
    );
    ikat_stage_free(h);
}

/// set_selection 设 (anchor, cursor) 选区（可反向 anchor>cursor）。rc=0。
#[test]
fn ffi_set_selection() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    // 选 "el"（字节 [1,3)）。
    assert_eq!(ikat_stage_set_selection(h, tf, 1, 3), 0, "set_selection rc");
    assert_eq!(textfield_selection(h, tf), (3, 1), "cursor=3 anchor=1");
    ikat_stage_free(h);
}

/// get_selection 读回 (start, end) 闭区间（min/max 归一）。
#[test]
fn ffi_get_selection() {
    let (h, tf) = make_stage_with_focused_textfield("hello");
    // 反向选区 anchor=4 cursor=1 → get 归一为 (1, 4)。
    assert_eq!(ikat_stage_set_selection(h, tf, 4, 1), 0);
    let mut start = 0usize;
    let mut end = 0usize;
    assert_eq!(ikat_stage_get_selection(h, tf, &mut start, &mut end), 0);
    assert_eq!(
        (start, end),
        (1, 4),
        "get_selection normalizes to [min,max]"
    );
    ikat_stage_free(h);
}

/// set_control_placeholder 改 placeholder，value 为空时 render 用它。get 回读。
#[test]
fn ffi_set_get_control_placeholder() {
    let (h, tf) = make_stage_with_focused_textfield(""); // 空 value
    let ph = b"type here";
    assert_eq!(
        ikat_stage_set_control_placeholder(h, tf, ph.as_ptr(), ph.len()),
        0,
        "set_control_placeholder rc"
    );
    ikat_stage_tick(h, 0.0);
    // 读回 placeholder。
    let mut buf = [0u8; 16];
    let mut len = 0usize;
    assert_eq!(
        ikat_stage_get_control_placeholder(h, tf, buf.as_mut_ptr(), buf.len(), &mut len),
        0
    );
    assert_eq!(&buf[..len], b"type here", "placeholder round-trip");
    ikat_stage_free(h);
}

/// set_control_readonly 切换 readonly 标志（聚焦时光标不画）。
#[test]
fn ffi_set_control_readonly() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    assert_eq!(
        ikat_stage_set_control_readonly(h, tf, true),
        0,
        "set readonly"
    );
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(tf)) {
        Some(ControlState::TextField(e)) => assert!(e.readonly, "readonly set true"),
        _ => panic!("not a textfield"),
    }
    ikat_stage_free(h);
}

/// set_control_readonly 对 NumberField 生效：setter 返 0，get_control_readonly 读回 1，
/// 且 ControlState variant 仍是 NumberField（保 variant 一致性）。原 setter 漏 NumberField
/// arm，调用静默返 -1；本测试锁读写对称（与 get_control_readonly 三 variant 同口径）。
#[test]
fn ffi_set_control_readonly_numberfield() {
    // make_number_stage 注入的 EditState.readonly 默认 false
    let (h, n) = make_number_stage("5", 0.0, 10.0, 1.0);
    // setter 现在覆盖 NumberField（不再静默返 -1）
    let rc = ikat_stage_set_control_readonly(h, n, true);
    assert_eq!(rc, 0, "set readonly on NumberField returns 0");
    // 读回：get_control_readonly 应返 0 且 out=1
    let mut out: u8 = 9;
    let rc = ikat_stage_get_control_readonly(h, n, &mut out);
    assert_eq!(rc, 0, "get readonly rc");
    assert_eq!(out, 1, "readonly read back as 1");
    // ControlState variant 仍是 NumberField（原地改，不重建为 TextField/TextArea）
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(n)) {
        Some(ControlState::NumberField { edit, .. }) => {
            assert!(edit.readonly, "NumberField.edit.readonly is true");
        }
        _ => panic!("ControlState variant changed (expected NumberField)"),
    }
    ikat_stage_free(h);

    // 非文本控件（Slider）→ -1（不 panic，不变量保留）
    let (h, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    assert_eq!(
        ikat_stage_set_control_readonly(h, s, true),
        -1,
        "non-text control returns -1"
    );
    ikat_stage_free(h);
}

/// set_control_maxlength 改 max_length（UTF-8 字符上限）。0 = 无限。
#[test]
fn ffi_set_control_maxlength() {
    let (h, tf) = make_stage_with_focused_textfield("ab");
    assert_eq!(
        ikat_stage_set_control_maxlength(h, tf, 5),
        0,
        "set maxlength=5"
    );
    let sh = unsafe { &*h };
    let scene = sh.stage.scene.as_ref().expect("scene built");
    match scene.controls.get(NodeId(tf)) {
        Some(ControlState::TextField(e)) => assert_eq!(e.max_length, 5),
        _ => panic!("not a textfield"),
    }
    ikat_stage_free(h);
}

/// set_control_text 非文本控件 → -1（不 panic）。
#[test]
fn ffi_set_control_text_non_text_control_err() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    // div 无 ControlState → -1。
    assert_eq!(
        ikat_stage_set_control_text(h, root, b"x".as_ptr(), 1),
        -1,
        "non-text-control node returns -1"
    );
    ikat_stage_free(h);
}

/// null 句柄 / 无效 node 健壮性（不 panic，返 -1）。
#[test]
fn ffi_control_text_null_handle_err() {
    assert_eq!(
        ikat_stage_set_control_text(std::ptr::null_mut(), 0, b"x".as_ptr(), 1),
        -1
    );
    let mut len = 0usize;
    assert_eq!(
        ikat_stage_get_control_text(std::ptr::null(), 0, std::ptr::null_mut(), 0, &mut len),
        -1
    );
}

// 5 个改写 setter（set_control_text/set_selection/set_control_placeholder/
// set_control_readonly/set_control_maxlength）原以 `match state { TextField(e)|TextArea(e) =>
// ..., _ => ...; ControlState::TextField(e) }` 重建——会把 TextArea 节点的 ControlState
// 改写成 TextField，破坏 ControlState/NodeKind variant 一致性。现改 in-place get_mut 原地改。
// 这两个回归测试锁不变量：在 TextArea 节点上调 setter 后，ControlState 仍为 TextArea。

/// 测试辅助：建根 div + 一个 TextArea（ControlState::TextArea + NodeKind::TextArea），
/// 返回 (handle, textarea_node)。不对焦（setter 不依赖焦点）。
fn make_stage_with_textarea(value: &str) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let ta = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(ta, u64::MAX, "create textarea node ok");
    ikat_stage_append_child(h, root, ta);
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
        ikat_stage_set_control_text(h, ta, b"new".as_ptr(), 3),
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
    ikat_stage_free(h);
}

/// 回归：set_selection 在 TextArea 节点上调用后，ControlState 仍为 TextArea，选区正确。
#[test]
fn ffi_set_selection_preserves_textarea_variant() {
    let (h, ta) = make_stage_with_textarea("hello");
    assert_eq!(ikat_stage_set_selection(h, ta, 1, 3), 0, "set_selection rc");
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
    ikat_stage_free(h);
}

/// 测试辅助：建根 div 子节点并注入 Dropdown 状态。
fn make_dropdown_stage(selected: usize, open: bool) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX, "create dropdown node ok");
    ikat_stage_append_child(h, root, node);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(node),
        ControlState::Dropdown {
            selected_index: selected,
            open,
            open_selected_index: if open { Some(selected) } else { None },
            value_lock: false,
            option_values: Vec::new(),
        },
    );
    if let Some(n) = scene.get_mut(NodeId(node)) {
        n.kind = NodeKind::Dropdown;
    }
    (h, node)
}

/// 测试辅助：建根 div 子节点并注入 TabList 状态。aria-selected 只读合成，无 value_lock。
fn make_tablist_stage(selected: usize) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX, "create tablist node ok");
    ikat_stage_append_child(h, root, node);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(node),
        ControlState::TabList {
            selected_index: selected,
            manual_activation: false,
        },
    );
    if let Some(n) = scene.get_mut(NodeId(node)) {
        n.kind = NodeKind::TabList;
    }
    (h, node)
}

/// 测试辅助：建根 div 子节点并注入 NumberField 状态。value 是数字的文本形式。
fn make_number_stage(value: &str, min: f32, max: f32, step: f32) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX, "create number node ok");
    ikat_stage_append_child(h, root, node);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    let edit = EditState::from_init(value.into(), String::new(), 0, false);
    scene.controls.ensure(
        NodeId(node),
        ControlState::NumberField {
            edit,
            min,
            max,
            step,
        },
    );
    if let Some(n) = scene.get_mut(NodeId(node)) {
        n.kind = NodeKind::NumberField;
    }
    (h, node)
}

/// 测试辅助：建一个 readonly=true 的 TextField 节点。
fn make_readonly_textfield_stage(readonly: bool) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX, "create textfield node ok");
    ikat_stage_append_child(h, root, node);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(node),
        ControlState::TextField(EditState::from_init(
            "ab".into(),
            String::new(),
            0,
            readonly,
        )),
    );
    if let Some(n) = scene.get_mut(NodeId(node)) {
        n.kind = NodeKind::TextField;
    }
    (h, node)
}

/// get_node_disabled：set_node_disabled(true) 后读回 1；默认 0；无效节点写 0。
#[test]
fn ffi_get_node_disabled_reads_flag() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    // 默认非 disabled
    let mut out: u8 = 9;
    ikat_stage_get_node_disabled(h, root, &mut out);
    assert_eq!(out, 0, "default not disabled");
    // set disabled 后读回 1
    ikat_stage_set_node_disabled(h, root, true);
    let mut out: u8 = 9;
    ikat_stage_get_node_disabled(h, root, &mut out);
    assert_eq!(out, 1, "disabled flag read back");
    // 无效节点 → 0（false）
    let mut out: u8 = 9;
    ikat_stage_get_node_disabled(h, u64::MAX, &mut out);
    assert_eq!(out, 0, "invalid node → 0");
    ikat_stage_free(h);
}

/// get_control_readonly：TextField / TextArea / NumberField 三 variant 都能读 readonly；
/// 非文本控件（Slider）返 -1。
#[test]
fn ffi_get_control_readonly_text_and_number() {
    // TextField readonly=true → 1
    let (h, tf) = make_readonly_textfield_stage(true);
    let mut out: u8 = 9;
    let rc = ikat_stage_get_control_readonly(h, tf, &mut out);
    assert_eq!(rc, 0, "textfield rc");
    assert_eq!(out, 1, "textfield readonly=true");
    ikat_stage_free(h);

    // NumberField readonly=true → 1
    let (h, n) = make_number_stage("5", 0.0, 10.0, 1.0);
    // 手工置 readonly（from_init 默认 false）
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        if let Some(ControlState::NumberField { edit, .. }) = scene.controls.get_mut(NodeId(n)) {
            edit.readonly = true;
        }
    }
    let mut out: u8 = 9;
    let rc = ikat_stage_get_control_readonly(h, n, &mut out);
    assert_eq!(rc, 0, "numberfield rc");
    assert_eq!(out, 1, "numberfield readonly=true");
    ikat_stage_free(h);

    // 非文本控件（Slider）→ -1
    let (h, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    let mut out: u8 = 9;
    let rc = ikat_stage_get_control_readonly(h, s, &mut out);
    assert_eq!(rc, -1, "slider get_control_readonly → -1");
    ikat_stage_free(h);
}

/// blur：request_focus 后 focused_node 非 null；blur 后下 tick 清焦点（pending 消费）。
/// 本测试只验 FFI rc=0 且 Stage.pending_focus_request 被置为 Some(None)。
#[test]
fn ffi_blur_sets_pending_none() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    ikat_stage_request_focus(h, root);
    // blur FFI 包装
    let rc = ikat_stage_blur(h);
    assert_eq!(rc, 0, "blur rc");
    // 验 pending_focus_request == Some(None)（下 tick 消费清焦点）
    let sh = unsafe { &*h };
    assert_eq!(
        sh.stage.pending_focus_request,
        Some(None),
        "blur sets pending to Some(None)"
    );
    ikat_stage_free(h);
}

/// get/set_dropdown_selected_index round-trip + value_lock 置位 + 非 Dropdown → -1。
#[test]
fn ffi_dropdown_selected_index_roundtrip() {
    let (h, dd) = make_dropdown_stage(1, false);
    let mut idx: u32 = 99;
    let rc = ikat_stage_get_dropdown_selected_index(h, dd, &mut idx);
    assert_eq!(rc, 0, "get rc");
    assert_eq!(idx, 1, "initial selected_index");
    // set 3
    let rc = ikat_stage_set_dropdown_selected_index(h, dd, 3);
    assert_eq!(rc, 0, "set rc");
    // value_lock 应置位（防本轮 cascade 回写）
    {
        let sh = unsafe { &*h };
        let scene = sh.stage.scene.as_ref().expect("scene built");
        match scene.controls.get(NodeId(dd)) {
            Some(ControlState::Dropdown {
                selected_index,
                value_lock,
                ..
            }) => {
                assert_eq!(*selected_index, 3, "selected_index updated");
                assert!(*value_lock, "value_lock set to prevent feedback loop");
            }
            _ => panic!("dropdown state lost"),
        }
    }
    // 读回
    let mut idx: u32 = 99;
    let rc = ikat_stage_get_dropdown_selected_index(h, dd, &mut idx);
    assert_eq!(rc, 0);
    assert_eq!(idx, 3, "read back updated index");
    // 非 Dropdown（Slider）→ -1
    let (h2, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    let mut idx: u32 = 0;
    assert_eq!(ikat_stage_get_dropdown_selected_index(h2, s, &mut idx), -1);
    assert_eq!(ikat_stage_set_dropdown_selected_index(h2, s, 0), -1);
    ikat_stage_free(h);
    ikat_stage_free(h2);
}

/// get/set_dropdown_open round-trip + 非 Dropdown → -1。
#[test]
fn ffi_dropdown_open_roundtrip() {
    let (h, dd) = make_dropdown_stage(0, false);
    let mut open: u8 = 9;
    let rc = ikat_stage_get_dropdown_open(h, dd, &mut open);
    assert_eq!(rc, 0, "get rc");
    assert_eq!(open, 0, "initial closed");
    let rc = ikat_stage_set_dropdown_open(h, dd, 1);
    assert_eq!(rc, 0, "set rc");
    let mut open: u8 = 9;
    let rc = ikat_stage_get_dropdown_open(h, dd, &mut open);
    assert_eq!(rc, 0);
    assert_eq!(open, 1, "open after set");
    // 非 Dropdown（Toggle）→ -1
    let (h2, t) = make_toggle_stage(false);
    let mut open: u8 = 0;
    assert_eq!(ikat_stage_get_dropdown_open(h2, t, &mut open), -1);
    assert_eq!(ikat_stage_set_dropdown_open(h2, t, 1), -1);
    ikat_stage_free(h);
    ikat_stage_free(h2);
}

/// get/set_tablist_selected_index round-trip + 非 TabList → -1。aria-selected 只读合成，
/// 故 setter 不设 value_lock（与 Dropdown 不同）。事件发射在 tick（on_pointer_down/键盘），
/// setter 只改态。
#[test]
fn ffi_tablist_selected_index_roundtrip() {
    let (h, tl) = make_tablist_stage(0);
    let mut idx: u32 = 99;
    let rc = ikat_stage_get_tablist_selected_index(h, tl, &mut idx);
    assert_eq!(rc, 0, "get rc");
    assert_eq!(idx, 0, "initial selected_index");
    // set 2
    let rc = ikat_stage_set_tablist_selected_index(h, tl, 2);
    assert_eq!(rc, 0, "set rc");
    // 读回
    let mut idx: u32 = 99;
    let rc = ikat_stage_get_tablist_selected_index(h, tl, &mut idx);
    assert_eq!(rc, 0);
    assert_eq!(idx, 2, "read back updated index");
    // 非 TabList（Slider）→ -1
    let (h2, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    let mut idx: u32 = 0;
    assert_eq!(ikat_stage_get_tablist_selected_index(h2, s, &mut idx), -1);
    assert_eq!(ikat_stage_set_tablist_selected_index(h2, s, 0), -1);
    ikat_stage_free(h);
    ikat_stage_free(h2);
}

/// get/set_number_value round-trip：clamp[min,max] + step 量化，再写回 EditState.value 文本。
#[test]
fn ffi_number_value_clamp_and_quantize() {
    // min=0 max=10 step=2，set 7 → 量化到 8（最近 step 边界），文本写回 "8"
    let (h, n) = make_number_stage("5", 0.0, 10.0, 2.0);
    let rc = ikat_stage_set_number_value(h, n, 7.0);
    assert_eq!(rc, 0, "set rc");
    let mut out = -1.0f32;
    let rc = ikat_stage_get_number_value(h, n, &mut out);
    assert_eq!(rc, 0, "get rc");
    assert!(
        (out - 8.0).abs() < 0.001,
        "7 quantized to 8 (step=2), got {out}"
    );
    // 文本写回
    {
        let sh = unsafe { &*h };
        let scene = sh.stage.scene.as_ref().expect("scene built");
        match scene.controls.get(NodeId(n)) {
            Some(ControlState::NumberField { edit, .. }) => {
                assert_eq!(
                    edit.value, "8",
                    "EditState.value rewritten to formatted number"
                );
            }
            _ => panic!("numberfield state lost"),
        }
    }
    // 超 max clamp：15 → 10
    let rc = ikat_stage_set_number_value(h, n, 15.0);
    assert_eq!(rc, 0);
    let mut out = -1.0f32;
    let rc = ikat_stage_get_number_value(h, n, &mut out);
    assert_eq!(rc, 0);
    assert!(
        (out - 10.0).abs() < 0.001,
        "15 clamped to max 10, got {out}"
    );
    ikat_stage_free(h);
}

/// NumberField get/set 对非 NumberField 控件（Slider）→ -1。
#[test]
fn ffi_number_value_non_number_control_err() {
    let (h, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    let mut out = -1.0f32;
    assert_eq!(
        ikat_stage_get_number_value(h, s, &mut out),
        -1,
        "get on slider"
    );
    assert_eq!(ikat_stage_set_number_value(h, s, 5.0), -1, "set on slider");
    ikat_stage_free(h);
}

/// NumberField get_control_min/max/step 读回 baked 约束值（min=0/max=10/step=2）。
/// 修复前这三个 getter 只 match Slider，对 NumberField 返 -1；现已扩到 NumberField。
#[test]
fn ffi_get_control_min_max_step_number_field() {
    let (h, n) = make_number_stage("5", 0.0, 10.0, 2.0);

    let mut out = -999.0f32;
    let rc = ikat_stage_get_control_max(h, n, &mut out);
    assert_eq!(rc, 0, "get_control_max rc");
    assert!((out - 10.0).abs() < 0.001, "max == 10, got {out}");

    let mut out = -999.0f32;
    let rc = ikat_stage_get_control_min(h, n, &mut out);
    assert_eq!(rc, 0, "get_control_min rc");
    assert!((out - 0.0).abs() < 0.001, "min == 0, got {out}");

    let mut out = -999.0f32;
    let rc = ikat_stage_get_control_step(h, n, &mut out);
    assert_eq!(rc, 0, "get_control_step rc");
    assert!((out - 2.0).abs() < 0.001, "step == 2, got {out}");
    ikat_stage_free(h);
}

/// 测试辅助：建根 div + 往 scene.keyframes 注入 opacity 0→1 两 stop 的 "fadeIn" 规则。
/// keyframes 表是 runtime 全局表（instantiate 合并产物），测试侧手工注入（同 controls 惯例）。
fn make_anim_stage() -> (*mut StageHandle, u64) {
    use ikat_core::scene::animation::{
        AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule,
    };
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.keyframes.insert(
        "fadeIn".into(),
        KeyframesRule {
            name: "fadeIn".into(),
            stops: vec![
                KeyframeStop {
                    selector: KeyframeStopSelector::From,
                    props: AnimatableProps {
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
                KeyframeStop {
                    selector: KeyframeStopSelector::To,
                    props: AnimatableProps {
                        opacity: Some(1.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        },
    );
    (h, root)
}

/// play_animation：返有效 PlayerKey(>0)，建 programmatic player，立即写首帧（fill both → opacity 0）。
/// 未知 name / 无效节点 / 非 UTF-8 → 0（无效 key 哨兵）。
#[test]
fn ffi_play_animation_creates_programmatic_player() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0, "play returns valid PlayerKey");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().unwrap();
    let p = scene
        .players
        .get(ikat_core::scene::animation::player_key_from_u64(key))
        .expect("player inserted");
    assert!(p.programmatic, "node.Play 建的 player 必须 programmatic");
    assert_eq!(p.spec.name, "fadeIn");
    assert_eq!(p.spec.duration, 1.0, "Play 默认时长 1s");
    assert_eq!(p.node, ikat_core::scene::NodeId(root));
    // 首帧立即写（spec §5.2）：fill both → progress 0 采样 opacity 0。
    let a = scene
        .anim
        .get(ikat_core::scene::NodeId(root))
        .expect("first frame written");
    assert_eq!(a.opacity, Some(0.0), "first frame opacity");
    // 状态 Playing。
    assert_eq!(
        ikat_stage_get_animation_state(h, key),
        0,
        "state == Playing(0)"
    );
    // 未知 name → 0。
    assert_eq!(
        ikat_stage_play_animation(h, root, b"nope".as_ptr(), 4),
        0,
        "unknown name"
    );
    // 无效节点 → 0。
    assert_eq!(
        ikat_stage_play_animation(h, u64::MAX, b"fadeIn".as_ptr(), 6),
        0,
        "invalid node"
    );
    // null name 指针 → 0（from_raw_parts(null,..) UB 防御）。
    assert_eq!(
        ikat_stage_play_animation(h, root, std::ptr::null(), 0),
        0,
        "null name"
    );
    ikat_stage_free(h);
}

/// pause → state Paused(1)；重复 pause 幂等；resume → Playing(0)。
#[test]
fn ffi_pause_resume_animation_state() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0);
    ikat_stage_pause_animation(h, key);
    assert_eq!(ikat_stage_get_animation_state(h, key), 1, "paused");
    ikat_stage_pause_animation(h, key); // 已 Paused 再 pause：幂等
    assert_eq!(ikat_stage_get_animation_state(h, key), 1);
    ikat_stage_resume_animation(h, key);
    assert_eq!(ikat_stage_get_animation_state(h, key), 0, "resumed");
    // 无效 key 全部 no-op（不 panic）。
    ikat_stage_pause_animation(h, 0);
    ikat_stage_resume_animation(h, 0xDEAD_BEEF);
    ikat_stage_free(h);
}

/// set_animation_time → get_animation_time 一致（seek 走 elapsed 单一时间源头）。
/// 无效 key：get 返 0.0，set no-op。
#[test]
fn ffi_set_get_animation_time_roundtrip() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0);
    assert_eq!(
        ikat_stage_get_animation_time(h, key),
        0.0,
        "fresh player time 0"
    );
    ikat_stage_set_animation_time(h, key, 0.25);
    assert_eq!(
        ikat_stage_get_animation_time(h, key),
        0.25,
        "seek roundtrip"
    );
    // 无效 key：get → 0.0；set → no-op（无 panic）。
    assert_eq!(
        ikat_stage_get_animation_time(h, 0),
        0.0,
        "invalid key time 0"
    );
    ikat_stage_set_animation_time(h, 0xDEAD_BEEF, 0.5);
    ikat_stage_free(h);
}

/// stop = scene 层终态：state 立即 255（Stopped 语义等同无效）；
/// resume 不可恢复；下帧 update_all 清通道 + 回收 player（players 表空）。
#[test]
fn ffi_stop_animation_is_terminal() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0);
    ikat_stage_stop_animation(h, key);
    assert_eq!(
        ikat_stage_get_animation_state(h, key),
        255,
        "stopped == invalid(255)"
    );
    ikat_stage_resume_animation(h, key); // Stopped 不可恢复
    assert_eq!(ikat_stage_get_animation_state(h, key), 255);
    // 下帧 update_all：回收 player + 清通道（NodeAnim 回 None）。
    ikat_stage_tick(h, 1.0 / 60.0);
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().unwrap();
    assert_eq!(
        scene.players.len(),
        0,
        "stopped player recycled by update_all"
    );
    assert_eq!(
        scene
            .anim
            .get(ikat_core::scene::NodeId(root))
            .map(|a| a.opacity),
        None,
        "channels cleared"
    );
    assert_eq!(
        ikat_stage_get_animation_time(h, key),
        0.0,
        "dead key time 0"
    );
    ikat_stage_free(h);
}

/// 播完（1s 单次迭代，fill both）→ state Completed(2)，player 保留（续写末值）。
#[test]
fn ffi_animation_state_completed_after_duration() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0);
    ikat_stage_tick(h, 1.1); // 越过 1s 时长
    assert_eq!(ikat_stage_get_animation_state(h, key), 2, "completed");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().unwrap();
    assert_eq!(scene.players.len(), 1, "fill both Completed player 保留");
    // fill both：末值持续写（opacity 1）。
    assert_eq!(
        scene
            .anim
            .get(ikat_core::scene::NodeId(root))
            .map(|a| a.opacity),
        Some(Some(1.0))
    );
    ikat_stage_free(h);
}

/// animation_on_key：注册进 on_key_percents；同 pct 重复注册去重。无效 key no-op。
#[test]
fn ffi_animation_on_key_registers_dedup() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0);
    ikat_stage_animation_on_key(h, key, 0.5);
    ikat_stage_animation_on_key(h, key, 0.5); // 去重
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().unwrap();
    let p = scene
        .players
        .get(ikat_core::scene::animation::player_key_from_u64(key))
        .unwrap();
    assert_eq!(p.on_key_percents, vec![0.5], "registered once (dedup)");
    // 无效 key no-op（不 panic）。
    ikat_stage_animation_on_key(h, 0, 0.5);
    ikat_stage_free(h);
}

/// get_event_string：事件字符串表读取（spec §7.5，C# demux 按 24-bit 索引读回 name）。
/// 动画事件 tick 后 intern "fadeIn" → 双调法读回；索引越界 / 无 scene → -1；小 buffer → -2。
#[test]
fn ffi_get_event_string_reads_animation_name() {
    let (h, root) = make_anim_stage();
    let key = ikat_stage_play_animation(h, root, b"fadeIn".as_ptr(), 6);
    assert_ne!(key, 0);
    // tick 一帧 → START 事件（EVT_ANIMATION_START=18）入 last_events，name intern 进表。
    ikat_stage_tick(h, 0.016);
    let evs = drain_events(h);
    let start = evs
        .iter()
        .find(|e| e.event_type == ikat_core::event::EVT_ANIMATION_START)
        .expect("START emitted on first tick");
    // 24-bit 小端索引：click_count | pad[0]<<8 | pad[1]<<16。
    let idx =
        start.click_count as u32 | ((start.pad[0] as u32) << 8) | ((start.pad[1] as u32) << 16);
    // 探大小（buf_cap=0）→ -2 + 所需 len。
    let mut needed = 0usize;
    let rc = ikat_stage_get_event_string(h, idx, std::ptr::null_mut(), 0, &mut needed);
    assert_eq!(rc, -2, "probe returns -2");
    assert_eq!(needed, b"fadeIn".len(), "probe reports needed len");
    // 真读。
    let mut buf = vec![0u8; needed];
    let rc = ikat_stage_get_event_string(h, idx, buf.as_mut_ptr(), buf.len(), &mut needed);
    assert_eq!(rc, 0, "read ok");
    assert_eq!(&buf[..needed], b"fadeIn", "string round-trip");
    // 索引越界 → -1（防御分支）。
    let mut n = 0usize;
    let rc = ikat_stage_get_event_string(h, u32::MAX, std::ptr::null_mut(), 0, &mut n);
    assert_eq!(rc, -1, "out-of-range index");
    assert_eq!(n, 0);
    // null 句柄 / null out_len → -1。
    let rc = ikat_stage_get_event_string(std::ptr::null(), idx, std::ptr::null_mut(), 0, &mut n);
    assert_eq!(rc, -1, "null handle");
    let rc = ikat_stage_get_event_string(h, idx, std::ptr::null_mut(), 0, std::ptr::null_mut());
    assert_eq!(rc, -1, "null out_len");
    ikat_stage_free(h);
}

/// 测试辅助：建根 div 后注入 Radio 状态（name = 分组名，打包期 data-name bake）。
fn make_radio_stage(name: &str) -> (*mut StageHandle, u64) {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let sh = unsafe { &mut *h };
    let scene = sh.stage.scene.as_mut().expect("scene built");
    scene.controls.ensure(
        NodeId(root),
        ControlState::Radio {
            checked: false,
            name: name.into(),
        },
    );
    (h, root)
}

/// set_control_max/min 对 NumberField：改界后 value 文本重约束（parse→clamp→量化→re-format），
/// step setter 换步长；非法 step（负/NaN）拒绝。三者原只有 Slider/Progress arm（NumberField → -1）。
#[test]
fn ffi_set_control_bounds_numberfield() {
    // value=5, min=0, max=10, step=2。
    let (h, n) = make_number_stage("5", 0.0, 10.0, 2.0);
    // max 10→4：value 5 clamp 到 4，量化 round(4/2)*2=4 → "4"。
    assert_eq!(ikat_stage_set_control_max(h, n, 4.0), 0);
    let mut out = -999.0f32;
    assert_eq!(ikat_stage_get_number_value(h, n, &mut out), 0);
    assert!(
        (out - 4.0).abs() < 0.001,
        "value re-clamped into [0,4], got {out}"
    );
    // max 低于 min → guard 到 min（不 panic，同 Slider arm）。
    assert_eq!(ikat_stage_set_control_max(h, n, -1.0), 0);
    assert_eq!(ikat_stage_get_control_max(h, n, &mut out), 0);
    assert!((out - 0.0).abs() < 0.001, "max guard to min 0, got {out}");
    // min 0→3：先恢复 max（guard 测试已把 max 压到 0，min 会被 min.min(max) clamp）。
    assert_eq!(ikat_stage_set_control_max(h, n, 10.0), 0);
    assert_eq!(ikat_stage_set_control_min(h, n, 3.0), 0);
    assert_eq!(ikat_stage_get_control_min(h, n, &mut out), 0);
    assert!((out - 3.0).abs() < 0.001, "min stored");
    // step 换 1：合法。
    assert_eq!(ikat_stage_set_control_step(h, n, 1.0), 0);
    assert_eq!(ikat_stage_get_control_step(h, n, &mut out), 0);
    assert!((out - 1.0).abs() < 0.001, "step stored");
    // 非法 step（负 / NaN）→ -1，不落脏值。
    assert_eq!(ikat_stage_set_control_step(h, n, -2.0), -1);
    assert_eq!(ikat_stage_set_control_step(h, n, f32::NAN), -1);
    // 非数值文本（用户正手输 "abc"）时改界：不 panic，value 文本原样保留。
    let sh = unsafe { &mut *h };
    if let Some(ControlState::NumberField { edit, .. }) =
        sh.stage.scene.as_mut().unwrap().controls.get_mut(NodeId(n))
    {
        edit.value = "abc".into();
    }
    assert_eq!(ikat_stage_set_control_max(h, n, 8.0), 0);
    let mut buf = [0u8; 16];
    let mut len = 0usize;
    assert_eq!(
        ikat_stage_get_control_text(h, n, buf.as_mut_ptr(), 16, &mut len),
        -1,
        "get_control_text 不认 NumberField（口径不变）"
    );
    let _ = len;
    let sh = unsafe { &mut *h };
    match sh.stage.scene.as_ref().unwrap().controls.get(NodeId(n)) {
        Some(ControlState::NumberField { edit, max, .. }) => {
            assert_eq!(edit.value, "abc", "非数值文本不被动");
            assert!((*max - 8.0).abs() < 0.001, "max 仍生效");
        }
        _ => panic!("numberfield state lost"),
    }
    ikat_stage_free(h);
}

/// ProgressBar indeterminate get/set round-trip（Progress arm 独占；非 Progress → -1）。
#[test]
fn ffi_progress_indeterminate_roundtrip() {
    let (h, p) = make_progress_stage(40.0, 100.0);
    let mut b = 2u8;
    assert_eq!(ikat_stage_get_control_indeterminate(h, p, &mut b), 0);
    assert_eq!(b, 0, "初始 determinate");
    assert_eq!(ikat_stage_set_control_indeterminate(h, p, 1), 0);
    assert_eq!(ikat_stage_get_control_indeterminate(h, p, &mut b), 0);
    assert_eq!(b, 1, "set true → get true");
    assert_eq!(ikat_stage_set_control_indeterminate(h, p, 0), 0);
    assert_eq!(ikat_stage_get_control_indeterminate(h, p, &mut b), 0);
    assert_eq!(b, 0, "set false → get false");
    // value/max 不被扰动。
    let mut v = -1.0f32;
    assert_eq!(ikat_stage_get_control_value(h, p, &mut v), 0);
    assert!((v - 40.0).abs() < 0.001);
    // 非 Progress（Slider）→ -1。
    let (h2, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    assert_eq!(ikat_stage_set_control_indeterminate(h2, s, 1), -1);
    assert_eq!(ikat_stage_get_control_indeterminate(h2, s, &mut b), -1);
    ikat_stage_free(h);
    ikat_stage_free(h2);
}

/// get_radio_name：双调法读分组名（buf_cap 不够 → -2 + 所需 len）。非 Radio → -1。
#[test]
fn ffi_get_radio_name() {
    let (h, r) = make_radio_stage("difficulty");
    // 探大小。
    let mut needed = 0usize;
    assert_eq!(
        ikat_stage_get_radio_name(h, r, std::ptr::null_mut(), 0, &mut needed),
        -2
    );
    assert_eq!(needed, 10, "name len bytes");
    // 真读。
    let mut buf = vec![0u8; needed];
    let mut written = 0usize;
    assert_eq!(
        ikat_stage_get_radio_name(h, r, buf.as_mut_ptr(), buf.len(), &mut written),
        0
    );
    assert_eq!(&buf[..written], b"difficulty");
    // 恰好等容也通过（needed == cap）。
    let mut cap_buf = vec![0u8; 10];
    let mut n2 = 0usize;
    assert_eq!(
        ikat_stage_get_radio_name(h, r, cap_buf.as_mut_ptr(), 10, &mut n2),
        0
    );
    assert_eq!(n2, 10);
    // 空 name 合法（无 data-name 的裸 radio）。
    let (h2, r2) = make_radio_stage("");
    let mut n3 = 99usize;
    assert_eq!(
        ikat_stage_get_radio_name(h2, r2, std::ptr::null_mut(), 0, &mut n3),
        0,
        "空串 rc=0（cap 0 >= needed 0）"
    );
    assert_eq!(n3, 0);
    // 非 Radio（Slider）→ -1。
    let (h3, s) = make_slider_stage(50.0, 0.0, 100.0, 1.0);
    let mut n4 = 0usize;
    assert_eq!(
        ikat_stage_get_radio_name(h3, s, std::ptr::null_mut(), 0, &mut n4),
        -1
    );
    ikat_stage_free(h);
    ikat_stage_free(h2);
    ikat_stage_free(h3);
}

/// ikat_stage_hit_test：坐标命中最上层 touchable 节点（rc=0 + out id）；
/// 未命中 rc=1；null 句柄 -1。scrollbar thumb sentinel id 须 decode 回容器 id
/// （公共树无 thumb 节点，thumb 命中 = 容器命中，同 apply_wheel_to_hit 口径）。
#[test]
fn ffi_stage_hit_test_basic() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX);
    ikat_stage_append_child(h, root, node);
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        let r = scene.get_mut(NodeId(root)).unwrap();
        r.layout_rect = ikat_core::scene::node::Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        };
        let c = scene.get_mut(NodeId(node)).unwrap();
        c.layout_rect = ikat_core::scene::node::Rect {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
        };
        ikat_core::scene::transform::compute_world_transforms(scene);
    }
    // 命中子节点。
    let mut out = u64::MAX;
    assert_eq!(ikat_stage_hit_test(h, 20.0, 20.0, &mut out), 0);
    assert_eq!(out, node, "子节点区域命中子");
    // 子外 root 区域 → 无命中（文档根是宿主容器，不可命中——多 Stage 输入路由依赖
    // 「空白处 = 无命中」，可命中的全画布根会把底层 Stage 输入饿死）→ rc=1。
    assert_eq!(ikat_stage_hit_test(h, 150.0, 90.0, &mut out), 1);
    // 画布外 → rc=1。
    assert_eq!(ikat_stage_hit_test(h, 500.0, 500.0, &mut out), 1);
    // null out 指针（防御）→ -1。
    assert_eq!(ikat_stage_hit_test(h, 20.0, 20.0, std::ptr::null_mut()), -1);
    // null 句柄 → -1。
    assert_eq!(
        ikat_stage_hit_test(std::ptr::null(), 20.0, 20.0, std::ptr::null_mut()),
        -1
    );
    ikat_stage_free(h);
}

/// set/get_node_touchable round-trip + hit_test 联动：untouchable 节点自身不命中
/// （子节点照常——CSS pointer-events 透传语义），恢复后命中回归。
#[test]
fn ffi_node_touchable_roundtrip_and_hit() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX);
    ikat_stage_append_child(h, root, node);
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        let r = scene.get_mut(NodeId(root)).unwrap();
        r.layout_rect = ikat_core::scene::node::Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        };
        let c = scene.get_mut(NodeId(node)).unwrap();
        c.layout_rect = ikat_core::scene::node::Rect {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
        };
        ikat_core::scene::transform::compute_world_transforms(scene);
    }
    // 初始默认 touchable。
    let mut b = 9u8;
    assert_eq!(ikat_stage_get_node_touchable(h, node, &mut b), 0);
    assert_eq!(b, 1, "default touchable");
    // 命中子节点。
    let mut out = u64::MAX;
    assert_eq!(ikat_stage_hit_test(h, 20.0, 20.0, &mut out), 0);
    assert_eq!(out, node);
    // set false → 自身不命中；文档根不可命中（宿主容器），点落到无命中（rc=1）。
    ikat_stage_set_node_touchable(h, node, false);
    assert_eq!(ikat_stage_get_node_touchable(h, node, &mut b), 0);
    assert_eq!(b, 0, "untouchable now");
    assert_eq!(ikat_stage_hit_test(h, 20.0, 20.0, &mut out), 1);
    assert_eq!(out, node, "no hit（rc=1）不写 out——保留上次命中值");
    // 恢复 → 命中回归。
    ikat_stage_set_node_touchable(h, node, true);
    assert_eq!(ikat_stage_hit_test(h, 20.0, 20.0, &mut out), 0);
    assert_eq!(out, node, "touchable restored");
    // 越界节点 / null out → -1。
    let mut n2 = 0u8;
    assert_eq!(ikat_stage_get_node_touchable(h, u64::MAX, &mut n2), -1);
    assert_eq!(
        ikat_stage_get_node_touchable(h, node, std::ptr::null_mut()),
        -1
    );
    ikat_stage_free(h);
}

/// set/get_node_draggable round-trip：默认关；开/关往返；drag_target 候选联动
/// （input.rs drag 检测读 interaction.draggable——开了才有 DragStart 链）。
/// 越界节点 / null out → -1。
#[test]
fn ffi_node_draggable_roundtrip() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(node, u64::MAX);
    ikat_stage_append_child(h, root, node);
    // 初始默认关（drag 事件链需要显式使能）。
    let mut b = 9u8;
    assert_eq!(ikat_stage_get_node_draggable(h, node, &mut b), 0);
    assert_eq!(b, 0, "default not draggable");
    // 开 → round-trip 回 1；再关回 0。
    ikat_stage_set_node_draggable(h, node, true);
    assert_eq!(ikat_stage_get_node_draggable(h, node, &mut b), 0);
    assert_eq!(b, 1, "draggable now");
    ikat_stage_set_node_draggable(h, node, false);
    assert_eq!(ikat_stage_get_node_draggable(h, node, &mut b), 0);
    assert_eq!(b, 0, "drag disabled again");
    // 越界节点 / null out → -1。
    let mut n2 = 0u8;
    assert_eq!(ikat_stage_get_node_draggable(h, u64::MAX, &mut n2), -1);
    assert_eq!(
        ikat_stage_get_node_draggable(h, node, std::ptr::null_mut()),
        -1
    );
    // null 句柄 set = no-op（不 panic）。
    ikat_stage_set_node_draggable(std::ptr::null_mut(), node, true);
    ikat_stage_free(h);
}

/// set/get_tab_activation round-trip：TabList 节点读默认 automatic(0)、set manual(1)
/// 往返；非 TabList 节点（普通 div）→ -1。
#[test]
fn ffi_tab_activation_roundtrip() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    // tablist 节点：直塞 ControlState（绕过 pkg instantiate，聚焦 FFI 读写面）。
    let tl = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(tl, u64::MAX);
    ikat_stage_append_child(h, root, tl);
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        scene.controls.ensure(
            NodeId(tl),
            ikat_core::scene::node::ControlState::TabList {
                selected_index: 0,
                manual_activation: false,
            },
        );
    }
    let mut b = 9u8;
    assert_eq!(ikat_stage_get_tab_activation(h, tl, &mut b), 0);
    assert_eq!(b, 0, "default automatic");
    ikat_stage_set_tab_activation(h, tl, true);
    assert_eq!(ikat_stage_get_tab_activation(h, tl, &mut b), 0);
    assert_eq!(b, 1, "manual now");
    // 非 TabList 节点 → -1。
    let mut n2 = 0u8;
    assert_eq!(ikat_stage_get_tab_activation(h, root, &mut n2), -1);
    assert_eq!(ikat_stage_set_tab_activation(h, root, true), -1);
    ikat_stage_free(h);
}

/// get_custom_tag 双调法 round-trip：CustomElement 节点读出 hyphen 标签；buf 不足返 -2 +
/// 所需长度；非 CustomElement / 越界节点 → -1。custom_tag 由 pkg instantiate 拷入
/// （编码机侧由打包器组件展开烘入）。
#[test]
fn ffi_get_custom_tag_two_call() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    ikat_stage_append_child(h, root, node);
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().expect("scene built");
        scene.get_mut(NodeId(node)).unwrap().custom_tag = Some("game-item-card".into());
    }
    // 第一调：buf 0 → -2 + 所需长度
    let mut needed = 0usize;
    assert_eq!(
        ikat_stage_get_custom_tag(h, node, std::ptr::null_mut(), 0, &mut needed),
        -2
    );
    assert_eq!(needed, "game-item-card".len());
    // 第二调：足量 buf → 0 + 字节
    let mut buf = vec![0u8; needed];
    assert_eq!(
        ikat_stage_get_custom_tag(h, node, buf.as_mut_ptr(), buf.len(), &mut needed),
        0
    );
    assert_eq!(buf, b"game-item-card");
    // 非 CustomElement（custom_tag=None）→ -1
    let mut n2 = 0usize;
    assert_eq!(
        ikat_stage_get_custom_tag(h, root, std::ptr::null_mut(), 0, &mut n2),
        -1
    );
    // 越界节点 → -1
    assert_eq!(
        ikat_stage_get_custom_tag(h, u64::MAX, std::ptr::null_mut(), 0, &mut n2),
        -1
    );
    ikat_stage_free(h);
}

/// ikat_node_is_lookup_scope：实例根 = 查找边界（1）；普通节点 = 0；越界 = -1。
/// C# Query<T> DFS 剪枝的数据源。
#[test]
fn ffi_node_is_lookup_scope() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    let node = ikat_stage_create_node(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    ikat_stage_append_child(h, root, node);
    // create_root 打 LOOKUP_SCOPE（scene 根）；动态 create_node 不打。
    assert_eq!(
        ikat_node_is_lookup_scope(h, root),
        1,
        "scene root is lookup scope"
    );
    assert_eq!(ikat_node_is_lookup_scope(h, node), 0, "plain node is not");
    assert_eq!(ikat_node_is_lookup_scope(h, u64::MAX), -1, "oob node -> -1");
    assert_eq!(
        ikat_node_is_lookup_scope(std::ptr::null(), root),
        -1,
        "null handle -> -1"
    );
    ikat_stage_free(h);
}

/// FFI panic 边界：guard 吞 panic 返回 fallback 哨兵、计数 +1；happy path 直通不计数。
#[test]
fn ffi_guard_swallows_panic_returns_fallback_and_counts() {
    use std::sync::atomic::Ordering;
    crate::FFI_PANIC_COUNT.store(0, Ordering::Relaxed);
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // 抑制 probe 的 panic 打印噪声
    let out = ffi_guard(7i32, || panic!("guard probe"));
    std::panic::set_hook(prev);
    assert_eq!(out, 7, "panic -> fallback sentinel");
    assert_eq!(ikat_ffi_panic_count(), 1, "panic counted once");
    assert_eq!(ffi_guard(9i32, || 9), 9, "happy path passes through");
    assert_eq!(ikat_ffi_panic_count(), 1, "happy path not counted");
    crate::FFI_PANIC_COUNT.store(0, Ordering::Relaxed);
}

/// #9 builder 契约 FFI 面：spec-struct 注册 tween → 推进 → complete 事件带 tag；
/// repeat/yoyo 多轮；bezier ease kind + 参数跨 FFI。
#[test]
fn stage_tween_spec_end_to_end_with_repeat_and_bezier() {
    use crate::animation::IkatTweenSpec;
    let h = stage_new_with_dejavu(200.0, 100.0);
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0);
    assert_ne!(root, u64::MAX);

    let start = [0.0f32];
    let end = [1.0f32];
    // opacity，bezier ease（kind=12），repeat=2 + yoyo，tag=7
    let spec = IkatTweenSpec {
        prop: 0,       // TweenProp::Opacity
        ease_kind: 12, // CUBIC_BEZIER
        ease_params: [0.25, 0.1, 0.25, 1.0],
        duration: 1.0,
        delay: 0.0,
        tag: 7,
        repeat: 2,
        yoyo: 1,
    };
    crate::ikat_stage_tween_spec(h, root, &spec, start.as_ptr(), end.as_ptr());
    // 推进 3 秒 = 3 轮跑满 → complete
    {
        let sh = unsafe { &mut *h };
        assert!(sh.stage.scene.is_some());
    }
    // 一次大 dt tick = 3 秒 → 3 轮跑满产 complete。
    ikat_stage_tick(h, 3.0);
    let evs = drain_events(h);
    let found = evs.iter().any(|e| e.event_type == 16 && e.touch_id == 7);
    assert!(found, "3 轮跑满产 complete（tag=7）");

    // 非法 kind → no-op（不 panic）
    let bad = IkatTweenSpec {
        prop: 0,
        ease_kind: 999,
        ease_params: [0.0; 4],
        duration: 1.0,
        delay: 0.0,
        tag: 0,
        repeat: 0,
        yoyo: 0,
    };
    crate::ikat_stage_tween_spec(h, root, &bad, start.as_ptr(), end.as_ptr());
    // null spec → no-op
    crate::ikat_stage_tween_spec(h, root, std::ptr::null(), start.as_ptr(), end.as_ptr());

    ikat_stage_free(h);
}
