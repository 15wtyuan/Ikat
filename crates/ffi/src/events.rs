//! 输入与事件面：指针/键盘/滚轮/文本注入、事件 SOA 拉取、touch monitor、点击取消、
//! 剪贴板回调注册、命中测试（含 rich-text 细化）、焦点控制。

use ikat_core::input::{EventRecord, KeyEvent, PointerEvent};
use ikat_core::scene::NodeId;

use crate::{ffi_guard, StageHandle};

/// 注入本帧指针事件（扁平 PointerEvent 数组）。tick 前调。
/// null/len=0 = 本帧无输入事件（清空 pending_input，hover diff 仍跑——指针位置沿用上帧 last_pos）。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn ikat_stage_set_input(
    h: *mut StageHandle,
    events: *const PointerEvent,
    len: usize,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if events.is_null() || len == 0 {
            sh.stage.set_input(&[]);
            return;
        }
        let evs = unsafe { std::slice::from_raw_parts(events, len) };
        sh.stage.set_input(evs);
    })
}

/// 拉取本帧事件 SOA（pull，同 borrow_frame 语义）。返 `last_events` 的 `as_ptr` + 写 len。
/// null 句柄或未 tick（last_events 空）→ null + len=0。指针下 tick 失效。
///
/// **常驻（不 gate）：**事件是 runtime 稳定入口。EventRecord 是 `#[repr(C)]` POD，
/// C 侧按 `len * sizeof(EventRecord)` 切片读。
#[no_mangle]
pub extern "C" fn ikat_stage_borrow_events(
    h: *const StageHandle,
    out_len: *mut usize,
) -> *const u8 {
    ffi_guard(std::ptr::null(), || {
        if h.is_null() {
            if !out_len.is_null() {
                unsafe { *out_len = 0 };
            }
            return std::ptr::null();
        }
        let sh = unsafe { &*h };
        let events: &[EventRecord] = sh.stage.last_events();
        if events.is_empty() {
            if !out_len.is_null() {
                unsafe { *out_len = 0 };
            }
            return std::ptr::null();
        }
        if !out_len.is_null() {
            unsafe { *out_len = events.len() };
        }
        events.as_ptr() as *const u8
    })
}

/// 读事件字符串表条目（spec §7.5：动画事件 name/hook_name 的 24-bit 索引载体，C# demux
/// 按索引读回字符串）。表是 Scene 级持久 intern（只增），索引跨 tick 稳定。
///
/// return-code + out-param（ptr+len）双调法（同 get_control_text）：
/// buf_cap 足够 → rc=0，写入 buf[..*out_len]；buf_cap 不够（含 0 探大小）→ rc=-2，
/// *out_len = 所需字节数（caller 扩容重调）；null 句柄 / 无 scene / 索引越界 → rc=-1，
/// *out_len=0（越界是防御分支——正常路径索引恒由 intern 产生）。
///
/// **常驻（不 gate）：**事件是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn ikat_stage_get_event_string(
    h: *const StageHandle,
    idx: u32,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out_len.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let scene = match sh.stage.scene.as_ref() {
            Some(s) => s,
            None => {
                unsafe { *out_len = 0 };
                return -1;
            }
        };
        let s = match scene.event_strs.get(idx) {
            Some(s) => s,
            None => {
                unsafe { *out_len = 0 };
                return -1;
            }
        };
        let bytes = s.as_bytes();
        let needed = bytes.len();
        unsafe { *out_len = needed };
        // buf_cap 不够（含 0 探大小）→ -2 + 所需 len（双调法，同 get_control_text）。
        if needed > buf_cap {
            return -2;
        }
        // buf_cap >= needed > 0：out 必非 null（caller 保证），拷贝。needed=0 时 null out 也合法。
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, needed);
            }
        }
        0
    })
}

/// UI 挡住时游戏不响应点击（§10.6）。= 任一活跃槽 last_hit 非空且非根（多指：鼠标 slot0 + 已分配触摸槽）。
/// null 句柄 → false。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_is_pointer_on_ui(h: *const StageHandle) -> bool {
    ffi_guard(false, || {
        if h.is_null() {
            return false;
        }
        let sh = unsafe { &*h };
        sh.stage.is_pointer_on_ui()
    })
}

/// 加 touch monitor（C# CaptureTouch 后调）。核心把 node 加进 touch_id 对应槽的 touch_monitors（去重）。
/// touch_id=-1 → 鼠标主指槽；找不到槽 → no-op。null 句柄 → no-op。
///
/// **常驻（不 gate）：**runtime 稳定入口。
#[no_mangle]
pub extern "C" fn ikat_stage_add_touch_monitor(h: *mut StageHandle, touch_id: i32, node_id: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.add_touch_monitor(touch_id, NodeId(node_id));
    })
}

/// 移除 touch monitor（C# 主动释放调）。从所有槽移除该 node。null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_remove_touch_monitor(h: *mut StageHandle, node_id: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.remove_touch_monitor(NodeId(node_id));
    })
}

/// 外部取消待 click（照 fgui Stage.CancelClick(touchId)）。置对应槽 click_cancelled。
/// null 句柄 → no-op。
#[no_mangle]
pub extern "C" fn ikat_stage_cancel_click(h: *mut StageHandle, touch_id: i32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.cancel_click(touch_id);
    })
}

/// 注入本帧键盘事件（扁平 KeyEvent 数组）。tick 前调。null/len=0 = 无键盘输入。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn ikat_stage_set_key_input(h: *mut StageHandle, keys: *const KeyEvent, len: usize) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if keys.is_null() || len == 0 {
            sh.stage.set_key_input(&[]);
            return;
        }
        let ks = unsafe { std::slice::from_raw_parts(keys, len) };
        sh.stage.set_key_input(ks);
    })
}

/// 注册宿主剪贴板回调。set_fn/get_fn = 后端实现的剪贴板桥（如 Unity
/// GUIUtility.systemCopyBuffer）：set 收 (ptr,len) UTF-8 字节拷走，get 写 (out_ptr,out_len)
/// 返宿主持有的缓冲区（活到下次 get）。传 null 解除注册。
///
/// core 是 cdylib，不能 extern 调宿主符号——故走回调注册。后端应在 Stage 启动后尽早
/// 注册一次。未注册时 Ctrl+C/X 仍走（写丢），Ctrl+V 读空串（no-op）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_register_clipboard(
    set_fn: Option<unsafe extern "C" fn(*const u8, usize) -> i32>,
    get_fn: Option<unsafe extern "C" fn(*mut *mut u8, *mut usize) -> i32>,
) {
    ffi_guard((), || {
        ikat_core::scene::control::register_clipboard(set_fn, get_fn);
    })
}

/// 注入本帧滚轮事件（扁平 WheelEvent 数组）。tick 前调；**累积式**（多次调合并）。
/// null/len=0 = 本帧无滚轮（直接 return，不清空——与 set_key_input 不同；累积语义）。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn ikat_stage_set_wheel_input(
    h: *mut StageHandle,
    events: *const ikat_core::scroll::WheelEvent,
    len: usize,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if events.is_null() || len == 0 {
            return;
        }
        let evs = unsafe { std::slice::from_raw_parts(events, len) };
        sh.stage.set_wheel_input(evs);
    })
}

/// 编程聚焦节点（照 fgui RequestFocus）。强制聚焦任意非 disabled 节点
/// （含 tabindex=None/-1）；disabled 拒；越界跳过。null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_request_focus(h: *mut StageHandle, node_id: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.request_focus(NodeId(node_id));
    })
}

/// 读当前焦点节点。无焦点/无 scene → u64::MAX（sentinel，同 node_parent）。null 句柄 → sentinel。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_focused_node(h: *const StageHandle) -> u64 {
    ffi_guard(u64::MAX, || {
        const NONE: u64 = u64::MAX;
        if h.is_null() {
            return NONE;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => scene.focused_node.map(|n| n.0).unwrap_or(NONE),
            None => NONE,
        }
    })
}

/// 清除当前 focus（`Stage::blur` 的 FFI 包装）：记 pending_focus_request = Some(None)，
/// 下 tick 消费清焦点（与 `request_focus` 对称）。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_blur(h: *mut StageHandle) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage.blur();
        0
    })
}

/// 命中测试（公共 Pick 的后端）：(x,y) 最上层可 touchable 节点。rc=0 命中（out_node 写
/// NodeId u64）；rc=1 未命中；-1 = null 句柄 / 无 scene / null out。坐标 = design 像素
/// （左上原点，同 process 输入）。core hit_test 走上帧 world_transforms（结构变更帧的
/// 新节点本帧未命中，1 帧延迟语义）。scrollbar thumb 合成 id（tag 字节 16/17，
/// NodeId bits[63:56]）decode 回容器 id——公共语义树无 thumb 节点，thumb 命中即容器命中
/// （同 apply_wheel_to_hit 口径）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_hit_test(
    h: *const StageHandle,
    x: f32,
    y: f32,
    out_node: *mut u64,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out_node.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match ikat_core::hit::hit_test(scene, (x, y)) {
            Some(id) => {
                // thumb 合成 tag（bits[63:56] = 16/17）strip，还原容器 primary id
                // （低 56 位）——见 scroll.rs V/H_THUMB_FLAG。hit_test 只产 thumb tag，
                // 低 56 位掩码即容器 id。
                unsafe { *out_node = id.0 & 0x00FF_FFFF_FFFF_FFFF };
                0
            }
            None => 1,
        }
    })
}

/// Rich-text-block 子节点命中细化（spec §10）。
///
/// 在 [`crate::ikat_stage_get_node_layout_rect`] / [`ikat_stage_is_pointer_on_ui`] 已定出命中
/// 目标是 rich-text-block 容器之后，用本函数把容器内的点细化到源 inline 节点
/// （span / TextNode / Image），供后端 firing span 级点击事件。
///
/// - `node_id`：rich-text-block 容器（须 `rich_text_block=true`，且 solve 已为其填
///   `scene.text_layouts[node_id]`）。
/// - `x`/`y`：相对该容器 border-box 左上的 block-local 点（与 hit_test world_to_local 后
///   的本地坐标同空间）。
/// - `out_source`：命中时写 source inline 节点的 NodeId(u64)；未命中不写。null 安全。
///
/// 返 `true` = 命中（`*out_source` 已写）；`false` = 未命中 / null 句柄 / 无 scene /
/// `node_id` 非 rich-text-block / 无 layout（`*out_source` 未动）。
#[no_mangle]
pub extern "C" fn ikat_hit_test_rich(
    h: *const StageHandle,
    node_id: u64,
    x: f32,
    y: f32,
    out_source: *mut u64,
) -> bool {
    ffi_guard(false, || {
        if h.is_null() {
            return false;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return false;
        };
        match ikat_core::text::hit_test::hit_test_rich(scene, NodeId(node_id), (x, y)) {
            Some(src) => {
                if !out_source.is_null() {
                    unsafe {
                        *out_source = src.0;
                    }
                }
                true
            }
            None => false,
        }
    })
}
