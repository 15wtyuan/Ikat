//! 滚动面：编程滚动位置（含缓动）、虚拟列表 content_size override 注入/清除、
//! scroll_pos 读取。

use loomgui_core::scene::NodeId;

use crate::{ffi_guard, StageHandle};

/// 编程滚动到指定位置。非 scroll 容器 / 越界 node → no-op（不 panic）。
/// animated: u8（0=瞬移 1=缓动 cubic-out）。null 句柄 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_scroll_pos(
    h: *mut StageHandle,
    node_id: u64,
    x: f32,
    y: f32,
    animated: u8,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let handle = unsafe { &mut *h };
        handle
            .stage
            .set_scroll_pos(NodeId(node_id), x, y, animated != 0);
    })
}

/// driver 注入滚动容器 content_size（虚拟列表）。node 无效/非滚动容器 → no-op。
/// null 句柄 → no-op（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_content_size(
    h: *mut StageHandle,
    node_id: u64,
    w: f32,
    height: f32,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let handle = unsafe { &mut *h };
        handle.stage.set_content_size(NodeId(node_id), w, height);
    })
}

/// 清除 driver 注入的 content_size override（列表销毁/退回普通滚动时用）。
/// null 句柄/无效 node → no-op（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_content_size_override(h: *mut StageHandle, node_id: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let handle = unsafe { &mut *h };
        handle.stage.clear_content_size_override(NodeId(node_id));
    })
}

/// 读 scroll_pos。null 句柄/无效 node → out 填 0（不 panic）。
/// out_x/out_y 是 out 参数（C# 传 ref float）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_scroll_pos(
    h: *const StageHandle,
    node_id: u64,
    out_x: *mut f32,
    out_y: *mut f32,
) {
    ffi_guard((), || {
        let (x, y) = if h.is_null() {
            (0.0, 0.0)
        } else {
            let sh = unsafe { &*h };
            sh.stage
                .get_scroll_pos(NodeId(node_id))
                .unwrap_or((0.0, 0.0))
        };
        if !out_x.is_null() {
            unsafe {
                *out_x = x;
            }
        }
        if !out_y.is_null() {
            unsafe {
                *out_y = y;
            }
        }
    })
}
