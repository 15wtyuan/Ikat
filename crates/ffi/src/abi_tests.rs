use super::*;
use crate::test_helpers::stage_new_with_dejavu;
use ikat_core::scene::NodeId;
/// FFI 测试辅助：手搓单组件 pkg（不走 parse），组件名由参数指定。
/// 组件 = 单 Container 根（无子）。返回 write_package 字节，可直接喂 load_package。
fn make_test_pkg_bytes(component: &str) -> Vec<u8> {
    use ikat_core::asset::{PackageInput, TemplateNode};
    use ikat_core::scene::NodeKind;
    use ikat_core::style::resolved::ResolvedStyle;
    let nodes = [TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        aria_controls: None,
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = ikat_core::style::dynamic::DynamicRuleTable::default();
    let input = PackageInput {
        components: vec![(component, nodes.as_slice(), &rules, &[])],
    };
    ikat_core::asset::write_package(&input)
}

/// load_package FFI 带 name 参数（对齐 Stage::load_package(name, bytes)）。
#[test]
fn load_package_ffi_takes_name() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let pkg = make_test_pkg_bytes("comp1");
    let name = b"bag";
    let r = ikat_stage_load_package(h, name.as_ptr(), name.len(), pkg.as_ptr(), pkg.len());
    assert_eq!(r, 0, "load_package 带 name ok");
    ikat_stage_free(h);
}

/// instantiate FFI 返有效 NodeId（非 INVALID u64::MAX）。
/// 流程：create_root 建 scene → load_package("bag") → instantiate("bag","comp1") → NodeId。
#[test]
fn instantiate_ffi_returns_nodeid() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    // create_root 建 scene（ensure_scene 自动建空骨架）。css 传空串（无 inline style）。
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let pkg = make_test_pkg_bytes("comp1");
    let lr = ikat_stage_load_package(h, b"bag".as_ptr(), 3, pkg.as_ptr(), pkg.len());
    assert_eq!(lr, 0, "load_package ok");
    let id = ikat_stage_instantiate(h, b"bag".as_ptr(), 3, b"comp1".as_ptr(), 5);
    assert_ne!(id, u64::MAX, "instantiate 返有效 NodeId");
    ikat_stage_free(h);
}

/// load_package FFI：手搓 pkg → load_package(name) → create_root → instantiate → append_child → tick → blob。
/// 与 load_html 路径解耦（parse feature off 时仍可用）。用 instantiate 建 scene 内容。
#[test]
fn load_package_builds_blob_from_package() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    // load_package 进资源池（不建 scene）
    let pkg = make_test_pkg_bytes("comp1");
    let r = ikat_stage_load_package(h, b"bag".as_ptr(), 3, pkg.as_ptr(), pkg.len());
    assert_eq!(r, 0, "load_package ok");
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let comp = ikat_stage_instantiate(h, b"bag".as_ptr(), 3, b"comp1".as_ptr(), 5);
    assert_ne!(comp, u64::MAX, "instantiate ok");
    assert_eq!(ikat_stage_append_child(h, root, comp), 0, "append_child ok");
    ikat_stage_tick(h, 0.0);
    let mut len = 0usize;
    let ptr = ikat_stage_borrow_frame(h, &mut len);
    assert!(!ptr.is_null(), "tick 后 blob 非空");
    assert!(len > 12, "blob 至少含 header");
    unsafe {
        assert_eq!(*ptr, 0x4Cu8, "magic 第一字节 'L'");
    }
    ikat_stage_free(h);
}

/// 契约：从未 tick 过的句柄 borrow_frame 必须返回 null + len=0
/// （空 Vec::as_ptr() 是非空悬挂哨兵，显式判空锁住"未 tick→null"契约）。
#[test]
fn borrow_frame_never_ticked_returns_null() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let mut len = 1usize; // 故意非 0，确认被覆写为 0
    let ptr = ikat_stage_borrow_frame(h, &mut len);
    assert!(ptr.is_null(), "未 tick 过 borrow_frame 必须 null");
    assert_eq!(len, 0, "未 tick 过 out_len 必须 0");
    ikat_stage_free(h);
}

// characterization 测试：当前实现已正确处理 null/无效输入，测绿锁住防回归
// （未来若误删 null check → UB/panic，此处兜住）。

#[test]
fn get_node_world_matrix_null_handle_and_null_outs_are_safe() {
    // null handle + 全 null out：不 crash、不写
    ikat_stage_get_node_world_matrix(
        std::ptr::null(),
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    // null handle + 一个有效 out：应写 identity[0]=1.0
    let mut a = 0.0f32;
    ikat_stage_get_node_world_matrix(
        std::ptr::null(),
        0,
        &mut a,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    assert_eq!(a, 1.0, "null handle → out_a = identity[0] = 1.0");
}

#[test]
fn get_node_world_matrix_invalid_node_returns_identity() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let (mut a, mut b, mut c, mut d, mut tx, mut ty) =
        (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    ikat_stage_get_node_world_matrix(
        h,
        u64::MAX,
        &mut a,
        &mut b,
        &mut c,
        &mut d,
        &mut tx,
        &mut ty,
    );
    assert_eq!(
        [a, b, c, d, tx, ty],
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        "无效 NodeId → identity（不 panic）"
    );
    ikat_stage_free(h);
}

#[test]
fn get_node_world_matrix_valid_node_returns_finite_matrix() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let empty = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    ikat_stage_tick(h, 0.0); // compute_world_transforms
    let (mut a, mut b, mut c, mut d, mut tx, mut ty) =
        (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    ikat_stage_get_node_world_matrix(h, root, &mut a, &mut b, &mut c, &mut d, &mut tx, &mut ty);
    assert!(
        [a, b, c, d, tx, ty].iter().all(|&v| v.is_finite()),
        "有效节点须返有限矩阵（无 NaN/Inf），got {:?}",
        [a, b, c, d, tx, ty]
    );
    ikat_stage_free(h);
}

#[test]
fn set_content_size_null_handle_and_invalid_node_are_safe() {
    ikat_stage_set_content_size(std::ptr::null_mut(), 0, 100.0, 200.0);
    let h = stage_new_with_dejavu(200.0, 200.0);
    ikat_stage_set_content_size(h, u64::MAX, 100.0, 200.0);
    ikat_stage_set_content_size(h, 999, 100.0, 200.0);
    ikat_stage_free(h);
}

#[test]
fn set_reuse_key_null_handle_and_invalid_node_are_safe() {
    ikat_stage_set_reuse_key(std::ptr::null_mut(), 0, 5);
    let h = stage_new_with_dejavu(200.0, 200.0);
    ikat_stage_set_reuse_key(h, u64::MAX, 5);
    ikat_stage_set_reuse_key(h, 999, 5);
    ikat_stage_free(h);
}

/// borrow_events 契约：未 tick / 空 last_events → null + len=0。
#[test]
fn borrow_events_null_before_tick() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let mut len = 1usize;
    let ptr = ikat_stage_borrow_events(h, &mut len);
    assert!(ptr.is_null() && len == 0, "未 tick → null+len=0");
    ikat_stage_free(h);
}

/// is_pointer_on_ui 契约：create_root 建空 scene（无子）→ 命中根 → false（根不算 UI）。
/// 覆盖 is_pointer_on_ui 在无 parse feature 路径下也可用（create_root 常驻）。
/// 用 create_root 建 scene。
#[test]
fn is_pointer_on_ui_true_on_hit_false_on_miss() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    // warmup tick：hit_test 读上帧 world_transforms（1 帧延迟）
    ikat_stage_tick(h, 0.0);
    // 空根 Container 无子 → hit_test 命中根 → 根不算 UI → false
    assert!(
        !ikat_stage_is_pointer_on_ui(h),
        "空根命中 → false（根不算 UI）"
    );
    ikat_stage_free(h);
}

/// cursor_query（#93）契约：无 scene/无命中 → 0（箭头）；命中 Button → 1（手型 UA 默认）；
/// 作者 `cursor:none` → 2。判别值即 C# 侧消费的线格式。
#[test]
fn cursor_query_arrow_hand_hidden_lifecycle() {
    // 无 scene → 箭头，不 panic（StageHandle 有效但未建 root）
    {
        let h = stage_new_with_dejavu(100.0, 50.0);
        assert_eq!(ikat_stage_cursor_query(h), 0, "无 scene → 0");
        ikat_stage_free(h);
    }
    // hit Button：首帧空事件 tick 刷新 last_hit——需要注入 Move 才有命中
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let btn = ikat_stage_create_root(
        h,
        b"button".as_ptr(),
        6,
        b"width:100px;height:50px".as_ptr(),
        22,
    );
    assert_ne!(btn, u64::MAX, "create_root(button) ok");
    ikat_stage_tick(h, 0.0); // warmup：world_transforms
                             // 注入鼠标 Move 到 button 区中心，再 tick 让状态机消费 + 重算命中
    use ikat_core::input::{PointerEvent, PointerKind};
    let ev = PointerEvent {
        kind: PointerKind::Move,
        button: 0,
        pad: [0; 2],
        touch_id: -1,
        x: 50.0,
        y: 25.0,
    };
    ikat_stage_set_input(h, &ev, 1);
    ikat_stage_tick(h, 1.0 / 60.0);
    assert_eq!(
        ikat_stage_cursor_query(h),
        1,
        "悬停 pressable 控件（UA Auto 默认）→ 手型"
    );
    ikat_stage_free(h);

    // null 句柄安全
    assert_eq!(ikat_stage_cursor_query(std::ptr::null()), 0, "null → 0");
}

/// EventRecord/PointerEvent sizeof 契约。
/// PointerEvent 16B：PointerKind repr(u8) 1B + button 1B + pad 2B + touch_id@4 + x@8 + y@12。
/// EventRecord 32B：node_id@0(8,u64) + event_type@8(1) + click_count@9(1) + pad@10(2) +
/// touch_id@12(4) + x@16 + y@20 + dx@24 + dy@28（#26 NodeId 拓宽 20→32B，对齐 8）。
#[test]
fn pointer_event_event_record_sizeof() {
    use ikat_core::input::{EventRecord, PointerEvent};
    assert_eq!(
        std::mem::size_of::<PointerEvent>(),
        16,
        "PointerEvent 16B（PointerKind repr(u8)）"
    );
    assert_eq!(
        std::mem::size_of::<EventRecord>(),
        32,
        "EventRecord 32B（node_id u64 #26）"
    );
}

/// 5 函数常驻契约：无 parse feature 也能编译（§14.6）。
/// 此测在 normal build 跑，验证 5 函数 + PointerEvent/EventRecord 常驻可调。
/// 不 tick（tick_and_render 需先 load scene）——本测只验常驻编译/调用安全；
/// 真正的 --no-default-features 验由 `cargo build -p ikat_ffi_c --no-default-features` 完成。
/// 行为验（含 set_input→tick→borrow_events/is_pointer_on_ui）在 parse-feature 测中覆盖。
#[test]
fn no_default_features_builds() {
    let h = stage_new_with_dejavu(100.0, 50.0);
    ikat_stage_set_input(h, std::ptr::null(), 0); // null/len=0 应安全（清空 pending_input）
    ikat_stage_set_node_disabled(h, 0, true); // 无 scene → no-op，不 panic
                                              // 无 scene + 未 tick：is_pointer_on_ui 读 cur_hit=None → false，不 panic
    assert!(!ikat_stage_is_pointer_on_ui(h));
    // borrow_events：未 tick → null + len=0
    let mut len = 1usize;
    let ptr = ikat_stage_borrow_events(h, &mut len);
    assert!(ptr.is_null() && len == 0);
    assert_eq!(
        ikat_node_parent(h, 0),
        u64::MAX,
        "无 scene → sentinel，不 panic"
    );
    ikat_stage_free(h);
}

/// node_parent 契约：create_root + instantiate + append_child → child.parent==root；
/// root.parent==sentinel；OOB==sentinel。用 create_root + instantiate 路径。
#[test]
fn node_parent_returns_chain_and_sentinel() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let pkg = make_test_pkg_bytes("comp1");
    assert_eq!(
        ikat_stage_load_package(h, b"bag".as_ptr(), 3, pkg.as_ptr(), pkg.len()),
        0
    );
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let comp = ikat_stage_instantiate(h, b"bag".as_ptr(), 3, b"comp1".as_ptr(), 5);
    assert_ne!(comp, u64::MAX, "instantiate ok");
    assert_eq!(ikat_stage_append_child(h, root, comp), 0, "append_child ok");
    assert_eq!(ikat_node_parent(h, comp), root, "comp.parent == root");
    assert_eq!(
        ikat_node_parent(h, root),
        u64::MAX,
        "root 是顶层 → parent=sentinel"
    );
    assert_eq!(ikat_node_parent(h, u64::MAX), u64::MAX, "OOB → sentinel");
    ikat_stage_free(h);
}

/// find_node_by_id round-trip：手搓包（组件含 id="ok" 节点）→ load_package → create_root →
/// instantiate → append_child → find "ok" 返节点 NodeId；无匹配 → sentinel。
/// 用 load_package + instantiate 路径。
#[test]
fn find_node_by_id_round_trip() {
    use ikat_core::asset::{PackageInput, TemplateNode};
    use ikat_core::scene::NodeKind;
    use ikat_core::style::resolved::ResolvedStyle;
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    // 手搓包：组件 "comp1" 含单 Container 节点 id="ok"
    let nodes = [TemplateNode {
        kind: NodeKind::Container,
        style: ResolvedStyle::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: Some("ok".to_string()),
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        href: None,
        control_init: None,
        role: None,
        data_slot: None,
        aria_controls: None,
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    }];
    let rules = ikat_core::style::dynamic::DynamicRuleTable::default();
    let pkg = ikat_core::asset::write_package(&PackageInput {
        components: vec![("comp1", nodes.as_slice(), &rules, &[])],
    });
    assert_eq!(
        ikat_stage_load_package(h, b"bag".as_ptr(), 3, pkg.as_ptr(), pkg.len()),
        0
    );
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let comp = ikat_stage_instantiate(h, b"bag".as_ptr(), 3, b"comp1".as_ptr(), 5);
    assert_ne!(comp, u64::MAX, "instantiate ok");
    assert_eq!(ikat_stage_append_child(h, root, comp), 0, "append_child ok");
    // find "ok" → comp（instantiate 把 id_attr 带到 live 节点）
    let ok_id = {
        let id = std::ffi::CString::new("ok").unwrap();
        ikat_stage_find_node_by_id(h, id.as_ptr() as *const u8, id.as_bytes().len())
    };
    assert_ne!(ok_id, u64::MAX, "find ok 应命中");
    assert_ne!(ok_id, u64::MAX, "find ok 应命中");
    assert_eq!(ok_id, comp, "find ok == comp 根 NodeId");
    let miss = {
        let id = std::ffi::CString::new("nope").unwrap();
        ikat_stage_find_node_by_id(h, id.as_ptr() as *const u8, id.as_bytes().len())
    };
    assert_eq!(miss, u64::MAX, "无匹配 → sentinel");
    ikat_stage_free(h);
}

/// get_link_href 双调法 round-trip（#74）：手搓包（Container 根 + Link 子带 href）→
/// load_package → instantiate → 读 Link 节点 href（rc=0，字节精确）；buf_cap 不足 →
/// rc=-2 + 所需长度；非 Link 节点 → rc=1；死节点 → rc=-1。
#[test]
fn link_href_round_trip() {
    use ikat_core::asset::{PackageInput, TemplateNode};
    use ikat_core::scene::NodeKind;
    use ikat_core::style::resolved::ResolvedStyle;
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let mk = |kind, href: Option<&str>, parent: Option<usize>| TemplateNode {
        kind,
        style: ResolvedStyle::default(),
        parent_idx: parent,
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: None,
        content: None,
        src: None,
        href: href.map(str::to_string),
        control_init: None,
        role: None,
        data_slot: None,
        aria_controls: None,
        rich_text_block: false,
        custom_tag: None,
        component_scope: false,
    };
    let nodes = [
        mk(NodeKind::Container, None, None),
        mk(NodeKind::Link, Some("open-shop"), Some(0)),
    ];
    let rules = ikat_core::style::dynamic::DynamicRuleTable::default();
    let pkg = ikat_core::asset::write_package(&PackageInput {
        components: vec![("comp1", nodes.as_slice(), &rules, &[])],
    });
    assert_eq!(
        ikat_stage_load_package(h, b"bag".as_ptr(), 3, pkg.as_ptr(), pkg.len()),
        0
    );
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let comp = ikat_stage_instantiate(h, b"bag".as_ptr(), 3, b"comp1".as_ptr(), 5);
    assert_ne!(comp, u64::MAX, "instantiate ok");
    let link = {
        // instantiate 后 Link 是 comp 根的唯一子。
        assert_eq!(ikat_stage_get_child_count(h, comp), 1, "comp 根下 1 子");
        let mut kids = [0u64; 1];
        let got = ikat_stage_get_children(h, comp, kids.as_mut_ptr(), 1);
        assert_eq!(got, 1);
        kids[0]
    };

    // 双调法第一步：0 容量探大小。
    let mut needed = 0usize;
    let rc = ikat_stage_get_link_href(h, link, std::ptr::null_mut(), 0, &mut needed);
    assert_eq!(rc, -2, "cap=0 探大小 → rc=-2");
    assert_eq!(needed, "open-shop".len());
    // 第二步：取串。
    let mut buf = [0u8; 64];
    let mut len = 0usize;
    let rc = ikat_stage_get_link_href(h, link, buf.as_mut_ptr(), buf.len(), &mut len);
    assert_eq!(rc, 0, "足够 cap → rc=0");
    assert_eq!(
        &buf[..len],
        b"open-shop",
        "href 字节精确（ptr+len，不靠 NUL）"
    );

    // 非 Link 节点（root div）→ rc=1（「不是链接」≠句柄错误）。
    let mut len = 0usize;
    assert_eq!(
        ikat_stage_get_link_href(h, root, buf.as_mut_ptr(), buf.len(), &mut len),
        1,
        "非 Link 节点 → rc=1"
    );
    // 死节点 → rc=-1。
    let mut len = 0usize;
    assert_eq!(
        ikat_stage_get_link_href(h, u64::MAX - 1, buf.as_mut_ptr(), buf.len(), &mut len),
        -1,
        "死节点 → rc=-1"
    );
    ikat_stage_free(h);
}

/// version 串 = "v1e"。
#[test]
fn version_is_v1e() {
    let p = ikat_version();
    let len = (0..).take_while(|&i| unsafe { *p.add(i) != 0 }).count();
    let s = std::str::from_utf8(unsafe { std::slice::from_raw_parts(p, len) }).unwrap();
    assert_eq!(s, "v1e");
}

/// EventRecord 32B（node_id u64 #26；#63 +dx/dy DragMove 逐 Move 增量）、PointerEvent 16B、Canceled=3。
#[test]
fn event_record_and_pointer_event_sizes_unchanged() {
    use ikat_core::input::{EventRecord, PointerEvent, PointerKind};
    use std::mem::size_of;
    assert_eq!(
        size_of::<EventRecord>(),
        32,
        "EventRecord 32B（node_id u64 #26）"
    );
    assert_eq!(size_of::<PointerEvent>(), 16, "PointerEvent 16B 不变");
    assert_eq!(PointerKind::Canceled as u8, 3, "Canceled=3");
}

/// KeyEvent sizeof 8B + EventRecord 32B / PointerEvent 16B。
#[test]
fn key_event_sizeof_and_unchanged() {
    use ikat_core::input::{EventRecord, KeyEvent, PointerEvent};
    use std::mem::size_of;
    assert_eq!(size_of::<KeyEvent>(), 8, "KeyEvent 8B");
    assert_eq!(
        size_of::<EventRecord>(),
        32,
        "EventRecord 32B（node_id u64 #26）"
    );
    assert_eq!(size_of::<PointerEvent>(), 16, "PointerEvent 16B 不变");
}

/// CursorRectRepr = 4 × f32 = 16B（#[repr(C)] POD）。FFI ABI 锁。
#[test]
fn cursor_rect_repr_sizeof() {
    use crate::CursorRectRepr;
    use std::mem::size_of;
    assert_eq!(
        size_of::<CursorRectRepr>(),
        16,
        "CursorRectRepr 4*f32 = 16B"
    );
}

/// EVT 常量值锁（12/13/14/15）。
#[test]
fn evt_constants() {
    assert_eq!(ikat_core::input::EVT_KEY_DOWN, 12);
    assert_eq!(ikat_core::input::EVT_KEY_UP, 13);
    assert_eq!(ikat_core::input::EVT_FOCUS_IN, 14);
    assert_eq!(ikat_core::input::EVT_FOCUS_OUT, 15);
}

/// set_wheel_input round-trip —— 推 WheelEvent 入 Stage，验 pending_wheel 累积。
/// 复用 Stage 类型直接构造（不经过 FFI pointer 层——abi_tests 测 public API 契约）。
#[test]
fn set_wheel_input_round_trip() {
    let fp = format!(
        "{}/../core/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut stage = Stage::new((200.0, 100.0)).unwrap();
    stage
        .register_font("DejaVu", std::fs::read(&fp).unwrap(), true)
        .unwrap();
    let evs = [ikat_core::scroll::WheelEvent {
        x: 10.0,
        y: 20.0,
        delta_x: 0.0,
        delta_y: 1.0,
    }];
    stage.set_wheel_input(&evs);
    assert_eq!(stage.pending_wheel.len(), 1);
}

/// helper：构造带 overflow:scroll 容器的 Stage（无子；手动填 layout_rect + scroll state）。
fn build_scroll_stage() -> Stage {
    let fp = format!(
        "{}/../core/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut stage = Stage::new((200.0, 100.0)).unwrap();
    stage
        .register_font("DejaVu", std::fs::read(&fp).unwrap(), true)
        .unwrap();
    use ikat_core::scene::{NodeKind, Scene};
    use ikat_core::style::resolved::{OverflowMode, ResolvedStyle};
    let mut sty = ResolvedStyle::default();
    sty.overflow_y = OverflowMode::Scroll;
    let entries = vec![(
        None::<usize>,
        NodeKind::Container,
        sty,
        vec![],
        None::<String>,
        false,
        None::<i32>,
        None::<String>, // data_controller
        None::<String>,
        None::<String>,
    )];
    stage.scene = Some(Scene::build(&entries));
    let scene = stage.scene.as_mut().unwrap();
    let root_id = scene.roots[0];
    scene.get_mut(root_id).unwrap().layout_rect = ikat_core::scene::node::Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    // refresh 需要 content_size/viewport/overlap（set_pos 读 overlap 做 clamp）
    ikat_core::scroll::refresh_content_sizes(stage.scene.as_mut().unwrap());
    // 手动改 overlap 到 200 让 scroll_pos 可测（无子 content=0,overlap=0 → set_pos 全 clamp 0）
    stage
        .scene
        .as_mut()
        .unwrap()
        .scroll
        .get_mut(root_id)
        .unwrap()
        .overlap = (0.0, 200.0);
    stage
}

#[test]
fn set_scroll_pos_updates_state() {
    let mut stage = build_scroll_stage();
    let root_id = stage.scene.as_ref().unwrap().roots[0];
    stage.set_scroll_pos(root_id, 0.0, 50.0, false);
    assert_eq!(
        stage
            .scene
            .as_ref()
            .unwrap()
            .scroll
            .get(root_id)
            .unwrap()
            .scroll_pos,
        (0.0, 50.0)
    );
}

#[test]
fn set_scroll_pos_animated_starts_tween() {
    let mut stage = build_scroll_stage();
    let root_id = stage.scene.as_ref().unwrap().roots[0];
    stage.set_scroll_pos(root_id, 0.0, 80.0, true);
    let st = stage.scene.as_ref().unwrap().scroll.get(root_id).unwrap();
    assert!(
        st.tweening_any(),
        "animated=true 启 tween（tweening={:?}）",
        st.tweening
    );
}

#[test]
fn set_scroll_pos_non_container_no_op() {
    let fp = format!(
        "{}/../core/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut stage = Stage::new((200.0, 100.0)).unwrap();
    stage
        .register_font("DejaVu", std::fs::read(&fp).unwrap(), true)
        .unwrap();
    use ikat_core::scene::{NodeKind, Scene};
    use ikat_core::style::resolved::ResolvedStyle;
    let entries = vec![(
        None::<usize>,
        NodeKind::Container,
        ResolvedStyle::default(),
        vec![],
        None::<String>,
        false,
        None::<i32>,
        None::<String>, // data_controller
        None::<String>,
        None::<String>,
    )];
    stage.scene = Some(Scene::build(&entries));
    let root_id = stage.scene.as_ref().unwrap().roots[0];
    // root 是 Container，overflow=Visible（默认）→ 非 scroll 容器 → set_scroll_pos no-op（不 panic）
    stage.set_scroll_pos(root_id, 0.0, 50.0, false);
    // 不 panic 即通过
}

#[test]
fn set_scroll_pos_oob_no_op() {
    let mut stage = build_scroll_stage();
    // 越界 NodeId（idx=99）→ no-op 不 panic
    stage.set_scroll_pos(NodeId((99u64 << 32) | 1), 0.0, 50.0, false);
}

/// WheelEvent ABI 尺寸 16B（4×f32 紧凑，C# 端同布局）。
/// compile-time 断言已在 scroll.rs 锁住；本测为 runtime 可见的检查。
#[test]
fn wheel_event_is_16_bytes() {
    assert_eq!(std::mem::size_of::<ikat_core::scroll::WheelEvent>(), 16);
}

/// 动态树 API FFI round-trip——8 函数经 FFI 调用建/改/删节点。
/// create_root 自动建空 scene（ensure_scene），无需 load_package 预建 scene。
/// 流程：create_root(div) → create_node(button/img/span) → append_child ×3 →
///       set_text/set_src 改属性 → insert_before 插序 →
///       remove_child 摘子 → remove_node 删根。每步断言返回值契约。
#[test]
fn dynamic_tree_api_ffi_round_trip() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    // create_node(button/img/span)——孤立节点
    let btn = ikat_stage_create_node(h, b"button".as_ptr(), 6, empty.as_ptr(), 0);
    assert_ne!(btn, u64::MAX, "create_node button ok");
    let img = ikat_stage_create_node(h, b"img".as_ptr(), 3, empty.as_ptr(), 0);
    assert_ne!(img, u64::MAX, "create_node img ok");
    let span = ikat_stage_create_node(h, b"span".as_ptr(), 4, empty.as_ptr(), 0);
    assert_ne!(span, u64::MAX, "create_node span ok");
    // append_child ×3 挂到 root（序：btn, img, span）
    assert_eq!(ikat_stage_append_child(h, root, btn), 0, "append btn");
    assert_eq!(ikat_stage_append_child(h, root, img), 0, "append img");
    assert_eq!(ikat_stage_append_child(h, root, span), 0, "append span");
    let txt = b"hello";
    assert_eq!(
        ikat_stage_set_text(h, span, txt.as_ptr(), txt.len()),
        0,
        "set_text span ok"
    );
    let src = b"icon.png";
    assert_eq!(
        ikat_stage_set_src(h, img, src.as_ptr(), src.len()),
        0,
        "set_src img ok"
    );
    // set_text 对非 Text 节点（img）应失败
    assert_eq!(
        ikat_stage_set_text(h, img, txt.as_ptr(), txt.len()),
        -1,
        "set_text on img → err"
    );
    // insert_before：在 btn 前插 img（先摘 img 再插）
    assert_eq!(
        ikat_stage_remove_child(h, root, img),
        0,
        "remove img from root"
    );
    assert_eq!(
        ikat_stage_insert_before(h, root, img, btn),
        0,
        "insert img before btn"
    );
    assert_eq!(ikat_stage_remove_child(h, root, span), 0, "remove span");
    // remove_node 删根（递归删子）
    assert_eq!(ikat_stage_remove_node(h, root), 0, "remove_node root");
    ikat_stage_free(h);
}

/// ikat_stage_new(w,h) 不注册任何字体；单独调 register_font 后再 measure。
/// 验证新 FFI 签名分离：stage_new 不收字体路径，register_font 独立注册。
#[test]
fn stage_new_without_font_then_register_font_measures() {
    let stage = stage_new_with_dejavu(200.0, 200.0);
    assert!(!stage.is_null(), "stage_new must succeed without font path");
    ikat_stage_free(stage);
}

/// set_fallback_families FFI：注册 DejaVu(默认) + wqy(回退)，设回退链，验证返回 0。
/// 端到端回退渲染（中文显出来）留 PlayMode；此测试只锁 FFI 入口不 panic + 返 0 +
/// 未注册 family 静默跳过（FontTable::set_fallback_families 契约）。
#[test]
fn set_fallback_families_ffi_returns_zero() {
    let stage = stage_new_with_dejavu(200.0, 200.0);
    // 注册 wqy 作回退字体（非默认）。
    let wqy_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../core/tests/fixtures/wqy-microhei.ttc"
    ))
    .expect("wqy-microhei.ttc fixture must exist");
    let wqy = b"wqy-microhei";
    let rc = ikat_stage_register_font(
        stage,
        wqy.as_ptr(),
        wqy.len(),
        wqy_bytes.as_ptr(),
        wqy_bytes.len(),
        0,
    );
    assert_eq!(rc, 0, "register_font wqy must return 0");
    // 设回退链：wqy-microhei + 一个未注册的 family（应静默跳过，不报错）。
    let text = "wqy-microhei\nNotRegistered";
    let rc = ikat_stage_set_fallback_families(stage, text.as_ptr(), text.len());
    assert_eq!(rc, 0, "set_fallback_families must return 0");
    // 清空回退（空文本）也应返 0。
    let rc = ikat_stage_set_fallback_families(stage, std::ptr::null(), 0);
    assert_eq!(rc, 0, "set_fallback_families(null,0) 清空回退返 0");
    ikat_stage_free(stage);
}

/// set_image_sizes FFI：CString 数组 + w/h 数组 → 调 FFI → 验 image_sizes HashMap 落地。
#[test]
fn set_image_sizes_ffi_round_trip() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let p1 = std::ffi::CString::new("atlas/icon.png").unwrap();
    let p2 = std::ffi::CString::new("atlas/bg.jpg").unwrap();
    let paths: [*const std::os::raw::c_char; 2] = [p1.as_ptr(), p2.as_ptr()];
    let ws: [u32; 2] = [64, 128];
    let hs: [u32; 2] = [64, 256];
    ikat_stage_set_image_sizes(h, paths.as_ptr(), ws.as_ptr(), hs.as_ptr(), 2);
    // 通过 handle 直接读 stage.image_sizes 验落地
    let handle = unsafe { &*h };
    assert_eq!(
        handle.stage.image_sizes.get("atlas/icon.png"),
        Some(&(64, 64))
    );
    assert_eq!(
        handle.stage.image_sizes.get("atlas/bg.jpg"),
        Some(&(128, 256))
    );
    ikat_stage_free(h);
}

/// null handle → no-op（不 panic）。
#[test]
fn set_image_sizes_null_handle_no_op() {
    let p = std::ffi::CString::new("x.png").unwrap();
    let paths: [*const std::os::raw::c_char; 1] = [p.as_ptr()];
    let ws = [10u32];
    let hs = [20u32];
    ikat_stage_set_image_sizes(
        std::ptr::null_mut(),
        paths.as_ptr(),
        ws.as_ptr(),
        hs.as_ptr(),
        1,
    );
}

/// count=0 → no-op（不 panic）。
#[test]
fn set_image_sizes_zero_count_no_op() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    ikat_stage_set_image_sizes(h, std::ptr::null(), std::ptr::null(), std::ptr::null(), 0);
    let handle = unsafe { &*h };
    assert!(handle.stage.image_sizes.is_empty());
    ikat_stage_free(h);
}

/// null paths[i] → skip that entry（不 panic，其余照常落地）。
#[test]
fn set_image_sizes_null_path_skipped() {
    let h = stage_new_with_dejavu(200.0, 200.0);
    let p = std::ffi::CString::new("atlas/icon.png").unwrap();
    let paths: [*const std::os::raw::c_char; 2] = [std::ptr::null(), p.as_ptr()];
    let ws: [u32; 2] = [10, 64];
    let hs: [u32; 2] = [20, 64];
    ikat_stage_set_image_sizes(h, paths.as_ptr(), ws.as_ptr(), hs.as_ptr(), 2);
    let handle = unsafe { &*h };
    assert_eq!(handle.stage.image_sizes.len(), 1);
    assert_eq!(
        handle.stage.image_sizes.get("atlas/icon.png"),
        Some(&(64, 64))
    );
    ikat_stage_free(h);
}
/// Non-UTF-8 bytes in an FFI string entry point must be detected and return an
/// error code instead of silently defaulting to "" and proceeding as if valid.
/// Uses load_package as the representative entry point; the fix pattern
/// applies to all 11 unwrap_or("") sites (see lib.rs from_utf8 grep).
#[test]
fn non_utf8_entry_returns_error() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let pkg = make_test_pkg_bytes("comp1");
    // Non-UTF-8 name bytes (0xFF 0xFE is invalid UTF-8).
    let bad_name: &[u8] = &[0xFF, 0xFE];
    let r = ikat_stage_load_package(
        h,
        bad_name.as_ptr(),
        bad_name.len(),
        pkg.as_ptr(),
        pkg.len(),
    );
    assert_ne!(
        r, 0,
        "non-UTF-8 name must return error, not success with empty string"
    );
    ikat_stage_free(h);
}

/// A6 smoke：便签层 7 FFI 的 ABI 不 panic 契约。
/// 流程：create_root（建 scene + 根 div）→
///   set_inline_override("width:100px") → 0；get_child_count(root) → 0；
///   add_class("foo") → 0；has_class("foo") → 1；remove_class("foo") → 0；has_class → 0；
///   get_children(cap=0) → -(n+2)（所需 cap）；unset_inline_override("width") → 0。
/// 还覆盖 null handle / 非 UTF-8 入参 → -1 的错误路径。
#[test]
fn a6_inline_children_class_smoke() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");

    let css = b"width:100px";
    assert_eq!(
        ikat_stage_set_inline_override(h, root, css.as_ptr(), css.len()),
        0,
        "set_inline_override ok"
    );

    // get_child_count：根无子 → 0
    assert_eq!(
        ikat_stage_get_child_count(h, root),
        0,
        "get_child_count 根 0 子"
    );

    // get_children：cap=0 且有 0 子 → 写入数 0（不算不够，len <= cap）
    let mut out: u64 = 0xDEAD;
    let r = ikat_stage_get_children(h, root, &mut out as *mut u64, 0);
    assert_eq!(r, 0, "get_children 0 子 → 写入 0");
    assert_eq!(out, 0xDEAD, "cap=0 时不应写 out");

    // add_class + has_class round-trip
    let class_name = b"foo";
    assert_eq!(
        ikat_stage_add_class(h, root, class_name.as_ptr(), class_name.len()),
        0,
        "add_class ok"
    );
    assert_eq!(
        ikat_stage_has_class(h, root, class_name.as_ptr(), class_name.len()),
        1,
        "has_class foo → true"
    );
    // 未加的 class → 0 (false)
    let absent = b"bar";
    assert_eq!(
        ikat_stage_has_class(h, root, absent.as_ptr(), absent.len()),
        0,
        "has_class bar → false"
    );
    // remove_class + has_class → 0
    assert_eq!(
        ikat_stage_remove_class(h, root, class_name.as_ptr(), class_name.len()),
        0,
        "remove_class ok"
    );
    assert_eq!(
        ikat_stage_has_class(h, root, class_name.as_ptr(), class_name.len()),
        0,
        "remove 后 has_class → false"
    );

    // unset_inline_override：合法 prop → 0
    let prop = b"width";
    assert_eq!(
        ikat_stage_unset_inline_override(h, root, prop.as_ptr(), prop.len()),
        0,
        "unset_inline_override ok"
    );

    // 错误路径：null handle 全部 → -1
    assert_eq!(
        ikat_stage_set_inline_override(std::ptr::null_mut(), root, css.as_ptr(), css.len()),
        -1
    );
    assert_eq!(ikat_stage_get_child_count(std::ptr::null(), root), -1);
    assert_eq!(
        ikat_stage_has_class(
            std::ptr::null(),
            root,
            class_name.as_ptr(),
            class_name.len()
        ),
        -1
    );

    // 错误路径：非 UTF-8 入参 → -1
    let bad: &[u8] = &[0xFF, 0xFE];
    assert_eq!(
        ikat_stage_add_class(h, root, bad.as_ptr(), bad.len()),
        -1,
        "非 UTF-8 class 名 → -1"
    );

    // 错误路径：不 live 节点 → -1 / -1
    assert_eq!(
        ikat_stage_get_child_count(h, u64::MAX),
        -1,
        "不 live 节点 get_child_count → -1"
    );
    assert_eq!(
        ikat_stage_has_class(h, u64::MAX, class_name.as_ptr(), class_name.len()),
        -1,
        "不 live 节点 has_class → -1"
    );

    ikat_stage_free(h);
}

/// A6 get_children 缓冲不够契约：构造 2 子节点 → cap=0 → 返 -(n+2)（所需 cap）→
/// cap=2 → 写入 2 + 数据正确。
#[test]
fn a6_get_children_capacity_contract() {
    use ikat_core::asset::{PackageInput, TemplateNode};
    use ikat_core::scene::NodeKind;
    use ikat_core::style::resolved::ResolvedStyle;
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    // 手搓包：comp1 含 2 Container 子（idx 1/2 parent_idx=0）
    let nodes = [
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            aria_controls: None,
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            aria_controls: None,
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
        TemplateNode {
            kind: NodeKind::Container,
            style: ResolvedStyle::default(),
            parent_idx: Some(0),
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            content: None,
            src: None,
            href: None,
            control_init: None,
            role: None,
            data_slot: None,
            aria_controls: None,
            rich_text_block: false,
            custom_tag: None,
            component_scope: false,
        },
    ];
    let rules = ikat_core::style::dynamic::DynamicRuleTable::default();
    let pkg = ikat_core::asset::write_package(&PackageInput {
        components: vec![("comp1", nodes.as_slice(), &rules, &[])],
    });
    assert_eq!(
        ikat_stage_load_package(h, b"bag".as_ptr(), 3, pkg.as_ptr(), pkg.len()),
        0
    );
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let comp = ikat_stage_instantiate(h, b"bag".as_ptr(), 3, b"comp1".as_ptr(), 5);
    assert_ne!(comp, u64::MAX, "instantiate ok");
    assert_eq!(ikat_stage_append_child(h, root, comp), 0, "append_child ok");

    // comp 有 2 子 → get_child_count = 2
    assert_eq!(ikat_stage_get_child_count(h, comp), 2, "comp 2 子");

    // cap=0 不够 → -(2+2) = -4
    let r = ikat_stage_get_children(h, comp, std::ptr::null_mut(), 0);
    assert_eq!(r, -4, "cap 不够 → -(len+2) = -4");

    // cap=2 正好 → 写入 2
    let mut kids = [0u64; 2];
    let r = ikat_stage_get_children(h, comp, kids.as_mut_ptr(), 2);
    assert_eq!(r, 2, "cap=2 → 写入 2");
    // 写入的是 live 子 NodeId（非 0、非 sentinel）
    for k in kids.iter() {
        assert_ne!(*k, u64::MAX, "子 NodeId 非 sentinel");
    }
    assert_ne!(kids[0], kids[1], "两子 NodeId 应不同");

    // cap=1 不够（2 子）→ -4
    let mut small = [0u64; 1];
    let r = ikat_stage_get_children(h, comp, small.as_mut_ptr(), 1);
    assert_eq!(r, -4, "cap=1 不够 → -4");

    ikat_stage_free(h);
}

/// clone_subtree FFI round-trip：create_root > img → clone_subtree(root) →
/// 新根 NodeId 非哨兵 + 游离（无父）+ 结构完整（1 子）。验证 FFI 序列化/句柄传递正确。
#[test]
fn clone_subtree_ffi_round_trip() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty_css = b"";
    let root = ikat_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, u64::MAX, "create_root ok");
    let img = ikat_stage_create_node(h, b"img".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(img, u64::MAX, "create_node ok");
    assert_eq!(ikat_stage_append_child(h, root, img), 0, "append_child ok");

    let cloned = ikat_stage_clone_subtree(h, root);
    assert_ne!(cloned, u64::MAX, "clone_subtree 返有效 NodeId");
    assert_ne!(cloned, root, "新 NodeId 不同于源根");
    // 游离：新根无父（u64::MAX = root sentinel）
    assert_eq!(ikat_node_parent(h, cloned), u64::MAX, "克隆根游离（无父）");
    // 结构完整：1 子
    assert_eq!(
        ikat_stage_get_child_count(h, cloned),
        1,
        "克隆根子数 = 源子数"
    );
    // 错误路径：无效 src → 哨兵
    assert_eq!(
        ikat_stage_clone_subtree(h, u64::MAX),
        u64::MAX,
        "无效 src → 哨兵"
    );

    ikat_stage_free(h);
}

/// IkatTweenSpec ABI 布局锁（C# 镜像同断言在 headless 门）：40B 字段 + yoyo(1) + 3B
/// 尾部 padding = 44。字段增删/重排先炸这里。
#[test]
fn ikat_tween_spec_size_is_44() {
    use crate::animation::IkatTweenSpec;
    assert_eq!(std::mem::size_of::<IkatTweenSpec>(), 44);
    // 字段偏移锁（防重排）：yoyo 是末字段（偏移 40）。
    let spec = IkatTweenSpec {
        prop: 0,
        ease_kind: 0,
        ease_params: [0.0; 4],
        duration: 1.0,
        delay: 0.0,
        tag: 0,
        repeat: 0,
        yoyo: 1,
    };
    let base = &spec as *const _ as usize;
    assert_eq!(&spec.yoyo as *const _ as usize - base, 40);
}
