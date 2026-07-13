use super::*;
use crate::test_helpers::stage_new_with_dejavu;
use std::ffi::CStr;

#[test]
fn version_returns_c_string() {
    unsafe {
        let s = CStr::from_ptr(loomgui_version() as *const i8);
        assert_eq!(s.to_str().unwrap(), "v1e");
    }
}

/// FFI tween：注册 opacity tween → tick 结束 → borrow_events 验 complete(tag)。
/// borrow_events 返回 *const u8 + len=记录数（非字节数；见 lib.rs:237 注释）。
/// 单切片：按记录数 len 切 typed slice，扫 event_type=EVT_TWEEN_COMPLETE && touch_id==tag。
#[cfg(feature = "parse")]
#[test]
fn stage_tween_complete_event_via_ffi() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    let html = b"<div class=\"b\"></div>";
    let css = b".b{width:100px;height:50px;}";
    loomgui_stage_load_html(h, html.as_ptr(), html.len(), css.as_ptr(), css.len());
    // 经 slotmap 分配的真实根 NodeId（动态树：root_id 非 0，是 idx<<12|gen）。
    // 传 node_id=0 会因 scene.get(NodeId(0)) 悬空 → update 跳过 → 无 complete 事件。
    let root_id = unsafe { (*h).stage.scene.as_ref().unwrap().roots[0].0 };
    let start = [0.0f32; 4];
    let end = [1.0f32, 0.0, 0.0, 0.0];
    // prop=0 (Opacity), ease=0 (Linear), duration=1.0, delay=0, tag=55
    loomgui_stage_tween(h, root_id, 0, start.as_ptr(), end.as_ptr(), 1.0, 0, 0.0, 55);
    loomgui_stage_tick(h, 1.0); // 推进到结束
    let mut len = 0usize;
    let p = loomgui_stage_borrow_events(h, &mut len);
    // len 是记录数（borrow_events out_len = events.len()）；直接切 typed slice。
    let recs =
        unsafe { std::slice::from_raw_parts(p as *const loomgui_core::input::EventRecord, len) };
    assert!(
        recs.iter()
            .any(|e| e.event_type == loomgui_core::input::EVT_TWEEN_COMPLETE && e.touch_id == 55),
        "FFI tween 结束 → complete(tag=55)"
    );
    loomgui_stage_free(h);
}

/// borrow_controller_changed_events 契约：null 句柄 → null + out_len=0（不 panic）。
#[test]
fn borrow_controller_changed_events_null_handle() {
    let mut len = 1usize; // 故意非 0，确认被覆写为 0
    let ptr = loomgui_stage_borrow_controller_changed_events(std::ptr::null(), &mut len);
    assert!(ptr.is_null(), "null 句柄 → null 指针");
    assert_eq!(len, 0, "null 句柄 → out_len=0");
}

/// borrow_controller_changed_events 契约：无 scene / 无事件 → null + out_len=0。
/// create_root 建空 scene（无 set_selected_index 调用）→ pending_controller_events 空。
#[test]
fn borrow_controller_changed_events_empty_when_no_events() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty_css = b"";
    loomgui_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    let mut len = 1usize;
    let ptr = loomgui_stage_borrow_controller_changed_events(h, &mut len);
    assert!(ptr.is_null(), "无事件 → null 指针");
    assert_eq!(len, 0, "无事件 → out_len=0");
    loomgui_stage_free(h);
}

/// borrow_controller_changed_events 端到端：建 mount → set_selected_index 推事件 →
/// borrow 返 ptr + count（COUNT 非字节）。读 ControllerChangedEvent POD slice 验字段。
#[test]
fn borrow_controller_changed_events_round_trip() {
    use loomgui_core::scene::node::ControllerChangedEvent;
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    // 建 root + mount 子节点，mount 挂 data-controller="tab"
    let empty_css = b"";
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root ok");
    let mount = loomgui_stage_create_node(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(mount, 0xFFFF_FFFF, "create_node mount ok");
    assert_eq!(
        loomgui_stage_append_child(h, root, mount),
        0,
        "append_child ok"
    );
    // 直接写 data_controller 字段（FFI 未暴露 set_selected_index，模拟 instantiate 填字段）
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().unwrap();
        scene.get_mut(NodeId(mount)).unwrap().data_controller = Some("tab".to_string());
    }
    // 调 Stage::set_selected_index 推 ControllerChangedEvent（prev=-1 → new=2）
    {
        let sh = unsafe { &mut *h };
        let prev = sh.stage.set_selected_index(NodeId(mount), 2);
        assert_eq!(prev, -1, "首次 set 返 prev=-1");
    }
    // borrow：out_len 是 COUNT（非字节）
    let mut len = 0usize;
    let ptr = loomgui_stage_borrow_controller_changed_events(h, &mut len);
    assert!(!ptr.is_null(), "有事件 → 非空指针");
    assert_eq!(len, 1, "out_len=1（COUNT，非字节）");
    // 读 ControllerChangedEvent POD slice 验字段
    let events = unsafe { std::slice::from_raw_parts(ptr as *const ControllerChangedEvent, len) };
    assert_eq!(events[0].mount_node, mount, "mount_node = mount NodeId.0");
    assert_eq!(events[0].prev, -1, "prev=-1（首次切页前无条目）");
    assert_eq!(events[0].new, 2, "new=2");
    loomgui_stage_free(h);
}

// ===== get_controller / set_selected_index / get_selected_index FFI tests =====

/// loomgui_stage_get_controller null 句柄 → sentinel（0xFFFF_FFFF，不 panic）。
#[test]
fn get_controller_null_handle() {
    let name = b"tab";
    let id = loomgui_stage_get_controller(std::ptr::null(), 0, name.as_ptr(), name.len());
    assert_eq!(id, 0xFFFF_FFFF);
}

/// loomgui_stage_get_controller null name 指针 → sentinel。
#[test]
fn get_controller_null_name_ptr() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let id = loomgui_stage_get_controller(h, 0, std::ptr::null(), 0);
    assert_eq!(id, 0xFFFF_FFFF);
    loomgui_stage_free(h);
}

/// loomgui_stage_set_selected_index null 句柄 → -1（不 panic）。
#[test]
fn set_selected_index_null_handle() {
    let r = loomgui_stage_set_selected_index(std::ptr::null_mut(), 0, 0);
    assert_eq!(r, -1);
}

/// loomgui_stage_get_selected_index null 句柄 → -1（不 panic）。
#[test]
fn get_selected_index_null_handle() {
    let r = loomgui_stage_get_selected_index(std::ptr::null(), 0);
    assert_eq!(r, -1);
}

/// Controller FFI round-trip：get_controller 定位挂载点 → set_selected_index 切页 →
/// get_selected_index 读回。全程经 FFI（不直接调 Rust Stage）。
#[test]
fn controller_ffi_round_trip() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty_css = b"";
    // 建 root + mount 子节点
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(root, 0xFFFF_FFFF);
    let mount = loomgui_stage_create_node(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    assert_ne!(mount, 0xFFFF_FFFF);
    assert_eq!(loomgui_stage_append_child(h, root, mount), 0);
    // 直接写 data_controller 字段（FFI 未暴露 data-controller 属性设置；instantiate 从模板填）
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().unwrap();
        scene.get_mut(NodeId(mount)).unwrap().data_controller = Some("tab".to_string());
    }
    // get_controller 在子树内找
    let name = b"tab";
    let found = loomgui_stage_get_controller(h, root, name.as_ptr(), name.len());
    assert_eq!(found, mount, "get_controller 应找到 mount 节点");
    // 查不存在 controller name → sentinel
    let no_match = loomgui_stage_get_controller(h, root, b"other".as_ptr(), 5);
    assert_eq!(no_match, 0xFFFF_FFFF, "无匹配 → sentinel");
    // 初始无条目 → get_selected_index 返 -1
    let initial = loomgui_stage_get_selected_index(h, mount);
    assert_eq!(initial, -1);
    // 切到第 2 页
    let prev = loomgui_stage_set_selected_index(h, mount, 2);
    assert_eq!(prev, -1, "首次 set 返 prev=-1");
    assert_eq!(loomgui_stage_get_selected_index(h, mount), 2);
    // 再切到第 0 页
    let prev2 = loomgui_stage_set_selected_index(h, mount, 0);
    assert_eq!(prev2, 2);
    assert_eq!(loomgui_stage_get_selected_index(h, mount), 0);
    loomgui_stage_free(h);
}

/// borrow_controller_changed_events 在 tick 后清空（pull 模式：事件仅当帧可见）。
/// set_selected_index 推事件 → tick → borrow 返空（tick start 清空）。
#[test]
fn borrow_controller_changed_events_cleared_after_tick() {
    let h = stage_new_with_dejavu(200.0, 100.0);
    assert!(!h.is_null());
    let empty_css = b"";
    let root = loomgui_stage_create_root(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    let mount = loomgui_stage_create_node(h, b"div".as_ptr(), 3, empty_css.as_ptr(), 0);
    loomgui_stage_append_child(h, root, mount);
    {
        let sh = unsafe { &mut *h };
        let scene = sh.stage.scene.as_mut().unwrap();
        scene.get_mut(NodeId(mount)).unwrap().data_controller = Some("tab".to_string());
        sh.stage.set_selected_index(NodeId(mount), 1);
    }
    // tick → tick start 清空 pending_controller_events
    loomgui_stage_tick(h, 0.0);
    let mut len = 1usize;
    let ptr = loomgui_stage_borrow_controller_changed_events(h, &mut len);
    assert!(ptr.is_null(), "tick 后事件清空 → null");
    assert_eq!(len, 0, "tick 后 out_len=0");
    loomgui_stage_free(h);
}
