//! 文本控件与 IME 面：value 文本/选区/placeholder/readonly/max_length 读写、
//! codepoint 输入注入、composition 回灌/提交、光标世界矩形（IME 候选窗定位）。

use loomgui_core::scene::node::ControlState;
use loomgui_core::scene::NodeId;

use crate::{ffi_guard, StageHandle};

/// 注入本帧字符输入（UTF-32 codepoints 数组，已 shift-mapped 的可打印字符）。tick 前调。
///
/// 与 `set_key_input` 互补：keydown 通道走物理键（KeyEvent），textinput 通道走已映射好的
/// 可打印 codepoint。tick 把 codepoints 插进聚焦的 TextField/TextArea；无焦点 / 非文本控件 /
/// readonly 时静默丢弃（无副作用）。null/len=0 = 清空本帧 pending（no-op）。
///
/// **返回码：** 0=ok，-1=null 句柄。len>0 但 codepoints=null 视作空（防 from_raw_parts(null) UB）。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_text_input(
    h: *mut StageHandle,
    codepoints: *const u32,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        if codepoints.is_null() || len == 0 {
            sh.stage.set_text_input(&[]);
            return 0;
        }
        let cps = unsafe { std::slice::from_raw_parts(codepoints, len) };
        sh.stage.set_text_input(cps);
        0
    })
}

/// 设文本控件的 IME composition（后端读平台 IME compositionString 回灌）。
/// text = UTF-8 字节（指针+len），pos = composition 在 value 中的字节偏移。
/// 非文本控件 / 越界 node → 静默跳过（仍返 0）。null 句柄 → -1。下一帧 measure/render
/// 会把 composition 拼进显示文本（下划线由 composition 分支画）。
///
/// **常驻（不 gate）：**IME 是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_composition(
    h: *mut StageHandle,
    node: u32,
    text: *const u8,
    text_len: usize,
    pos: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        // null/零长兜底为空串（composition 可被清空：传空串 = 取消正在进行的 composition）。
        let s = if text.is_null() || text_len == 0 {
            String::new()
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, text_len) }) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            }
        };
        sh.stage.set_composition(NodeId(node), &s, pos);
        0
    })
}

/// 提交文本控件的 composition（落定进 value）。返 1 = 有 composition 且 value 改变；
/// 0 = 无 composition（或被 readonly/max_length 拒）。非文本控件 / 越界 node → 0。
/// null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_commit_composition(h: *mut StageHandle, node: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage.commit_composition(NodeId(node)) as i32
    })
}

/// 读文本控件光标的世界矩形（IME 候选窗定位用，照 Unity Input.compositionCursorPos）。
/// out 指向 [`CursorRectRepr`]（4 个 f32）。返 0 = 成功且 `*out` 已填；1 = 失败（节点无效 /
/// 非文本控件 / 无缓存 TextLayout / out 为 null）。null 句柄 → -1。
///
/// 几何与 render arm 画光标同源（layout 空间 caret + world transform）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_cursor_rect(
    h: *const StageHandle,
    node: u32,
    out: *mut CursorRectRepr,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        if out.is_null() {
            return 1;
        }
        let sh = unsafe { &*h };
        match sh.stage.cursor_rect(NodeId(node)) {
            Some(r) => {
                unsafe {
                    *out = CursorRectRepr {
                        x: r.x,
                        y: r.y,
                        w: r.w,
                        h: r.h,
                    };
                }
                0
            }
            None => 1,
        }
    })
}

/// 光标世界矩形（IME 候选窗定位用）。#[repr(C)] POD，4 × f32 = 16B。后端读 [`crate::CursorRectRepr`]
/// 定位 Unity Input.compositionCursorPos / Win32 IME 候选窗。
#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct CursorRectRepr {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 设文本控件 value（TextField / TextArea）。直接替换 EditState.value + 光标/anchor 移到
/// 末尾（不走 insert_text 的光标插入路径——这是编程 setter，照 JS `.value = ...` 语义）。
/// 改变时产 ValueChanged（经 Stage.pending_events 缓冲，下 tick 入 last_events）。
/// readonly 不拦（编程可写，照 HTML JS 语义）；非文本控件 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_text(
    h: *mut StageHandle,
    node_id: u32,
    text: *const u8,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
        let text = if text.is_null() || len == 0 {
            ""
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, len) }) {
                Ok(s) => s,
                Err(_) => return -1,
            }
        };
        let sh = unsafe { &mut *h };
        let scene = match sh.stage.scene.as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let id = NodeId(node_id);
        let new_value = text.to_string();
        // 原地改 EditState（get_mut），不重建 ControlState——重建为 TextField 会把 TextArea
        // 节点改写成 TextField，破坏 ControlState/NodeKind variant 一致性不变量。同 control.rs
        // on_text_pointer_down 的 in-place 改法。
        let mut changed = false;
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) =
            scene.controls.get_mut(id)
        {
            // 同值仍重置光标但不发事件。
            if e.value != new_value {
                e.value = new_value.clone();
                e.cursor_visible = true;
                e.cursor_timer = 0.0;
                changed = true;
            }
            e.cursor = new_value.len();
            e.anchor = e.cursor;
            // value 被整体替换 → 抹掉正在进行的 composition（旧预提交文本失效）。
            e.composition = None;
        } else {
            return -1;
        }
        // ValueChanged 须在 get_mut 借用结束后产：if-let 块出来后 scene 借用（借 sh.stage.scene）
        // 由 NLL 释放，方可另借 sh.stage.pending_events（不同字段）。
        if changed {
            loomgui_core::scene::control::emit_value_changed(&mut sh.stage.pending_events, id);
        }
        0
    })
}

/// 读文本控件 value（TextField / TextArea）。return-code + out-param（ptr+len）双调法：
/// buf_cap 足够 → rc=0，写入 buf[..*out_len]；buf_cap 不够 → rc=-2，*out_len = 所需字节数
/// （caller 扩容重调）；非文本控件 / null 句柄 → -1。buf_cap=0 探大小 → rc=-2 + 所需 len。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_text(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out_len.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        let value = match scene.controls.get(NodeId(node_id)) {
            Some(ControlState::TextField(e) | ControlState::TextArea(e)) => e.value.as_bytes(),
            _ => return -1,
        };
        let needed = value.len();
        unsafe { *out_len = needed };
        // buf_cap 不够（含 0 探大小）→ -2 + 所需 len（双调法，同 font_atlas_page）。
        if needed > buf_cap {
            return -2;
        }
        // buf_cap >= needed > 0：out 必非 null（caller 保证），拷贝。needed=0 时 null out 也合法。
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(value.as_ptr(), out, needed);
            }
        }
        0
    })
}

/// 设文本控件选区 (anchor, cursor)（字节偏移）。反向（anchor>cursor）允许，get_selection
/// 会归一。越界偏移 clamp 到 [0, value.len()]（不 panic）。非文本控件 / null 句柄 → -1。
/// 偏移须落在 char 边界——caller 传字节偏移（同 EditState 约定）；越界字节位置 clamp
/// 到最近的合法边界（value.len() 总是合法边界）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_selection(
    h: *mut StageHandle,
    node_id: u32,
    anchor: usize,
    cursor: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let scene = match sh.stage.scene.as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let id = NodeId(node_id);
        // 原地改（get_mut），保 variant（重建为 TextField 会把 TextArea 改写成 TextField）。
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) =
            scene.controls.get_mut(id)
        {
            let len = e.value.len();
            // clamp 到 [0, len]（len 总是合法 char 边界 = 末尾）。中间字节位置若非法 char
            // 边界，向右退到最近合法边界（避免 UTF-8 切割 panic）。
            e.anchor = clamp_char_boundary(&e.value, anchor.min(len));
            e.cursor = clamp_char_boundary(&e.value, cursor.min(len));
        } else {
            return -1;
        }
        0
    })
}

/// 读文本控件选区。写入 *start/*end（闭区间，min/max 归一）。有选区 start<end，退化
/// 选区 start==end（零宽光标）。非文本控件 / null 句柄 / null out → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_selection(
    h: *const StageHandle,
    node_id: u32,
    start: *mut usize,
    end: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || start.is_null() || end.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match scene.controls.get(NodeId(node_id)) {
            Some(ControlState::TextField(e) | ControlState::TextArea(e)) => {
                let (b, c) = e.selection_range();
                unsafe {
                    *start = b;
                    *end = c;
                }
                0
            }
            _ => -1,
        }
    })
}

/// 设文本控件 placeholder（value 为空时渲染它）。非文本控件 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_placeholder(
    h: *mut StageHandle,
    node_id: u32,
    text: *const u8,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        // null/零长兜底为空串（清 placeholder）：from_raw_parts(null, 0) 是 UB。
        let text = if text.is_null() || len == 0 {
            ""
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, len) }) {
                Ok(s) => s,
                Err(_) => return -1,
            }
        };
        let sh = unsafe { &mut *h };
        let scene = match sh.stage.scene.as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let id = NodeId(node_id);
        // 原地改（get_mut），保 variant。
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) =
            scene.controls.get_mut(id)
        {
            e.placeholder = text.to_string();
        } else {
            return -1;
        }
        0
    })
}

/// 读文本控件 placeholder。return-code + out-param（ptr+len）双调法（同 get_control_text）：
/// buf_cap 足够 → rc=0；buf_cap 不够 → rc=-2 + *out_len=所需；非文本控件 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_placeholder(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out_len.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        let ph = match scene.controls.get(NodeId(node_id)) {
            Some(ControlState::TextField(e) | ControlState::TextArea(e)) => {
                e.placeholder.as_bytes()
            }
            _ => return -1,
        };
        let needed = ph.len();
        unsafe { *out_len = needed };
        if needed > buf_cap {
            return -2;
        }
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(ph.as_ptr(), out, needed);
            }
        }
        0
    })
}

/// 设文本控件 readonly 标志（true = 用户不可编辑，编程 setter 仍可改 value）。
/// 非文本控件 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_readonly(
    h: *mut StageHandle,
    node_id: u32,
    readonly: bool,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let scene = match sh.stage.scene.as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let id = NodeId(node_id);
        // 原地改（get_mut），保 variant。NumberField 共享 EditState.readonly（与
        // get_control_readonly 三 variant 同口径，保读写对称）。
        if let Some(
            ControlState::TextField(e)
            | ControlState::TextArea(e)
            | ControlState::NumberField { edit: e, .. },
        ) = scene.controls.get_mut(id)
        {
            e.readonly = readonly;
        } else {
            return -1;
        }
        0
    })
}

/// 设文本控件 max_length（UTF-8 字符上限；0 = 无限）。非文本控件 / null 句柄 → -1。
/// 注意：改 max_length 不追溯裁剪现有 value（照 HTML maxlength 语义，只限后续输入）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_maxlength(
    h: *mut StageHandle,
    node_id: u32,
    max_length: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let scene = match sh.stage.scene.as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let id = NodeId(node_id);
        // 原地改（get_mut），保 variant。
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) =
            scene.controls.get_mut(id)
        {
            e.max_length = max_length;
        } else {
            return -1;
        }
        0
    })
}

/// 读文本控件 max_length（UTF-8 字符上限；0 = 无限）。非文本控件 / null 句柄 → -1
/// （与 set_control_maxlength 对称——同为 TextField/TextArea 双变体口径）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_maxlength(
    h: *const StageHandle,
    node_id: u32,
    out: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match scene.controls.get(NodeId(node_id)) {
            Some(ControlState::TextField(e) | ControlState::TextArea(e)) => {
                unsafe { *out = e.max_length };
                0
            }
            _ => -1,
        }
    })
}

/// 把字节偏移 clamp 到 [0, len] 且落到合法 UTF-8 char 边界。idx 可能是非法边界（指向
/// 多字节字符中间），向右退到最近合法边界（不 panic）。idx > len → len（len 总是合法）。
/// str::ceil_char_boundary 在 nightly；这里手写等价（线性向前 probe，UTF-8 短串代价可接受）。
fn clamp_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    // 向右 probe 直到 idx 是 char 边界（s.is_char_boundary）。UTF-8 多字节序列最长 4 字节，
    // 最多 probe 3 次。
    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// 读文本控件 readonly（`EditState.readonly`）：TextField / TextArea / NumberField 共享 EditState，
/// 故三者皆读。非文本控件 / null 句柄 / 节点缺失 / null out → -1；命中且 `*out` 已填则返 0。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_readonly(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match scene.controls.get(NodeId(node_id)) {
            Some(
                ControlState::TextField(e)
                | ControlState::TextArea(e)
                | ControlState::NumberField { edit: e, .. },
            ) => {
                unsafe { *out = if e.readonly { 1 } else { 0 } };
                0
            }
            _ => -1,
        }
    })
}
