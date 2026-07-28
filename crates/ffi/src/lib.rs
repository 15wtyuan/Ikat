//! FFI 导出层（§14.1 csbindgen）：extern "C" 薄包装，opaque Stage 句柄。
//! 命名前缀 `loomgui_`，csbindgen 扫描本文件生成 C# 绑定。

// FFI 边界固有，放行：not_unsafe_ptr_arg_deref（外部 raw ptr 解引用是 C ABI 常态）、
// too_many_arguments（签名由 C# 调用方契约固定）、type_complexity（ptr+len 出参组合）、
// unnecessary_cast（abi_tests 里 raw ptr 同型 cast，clippy --fix 对 raw ptr 保守不动）、
// needless_range_loop（blob/tests 顶点/索引填装按 i 下标写，iterator 改写可读性反降）。
// field_reassign_with_default：测试 setup（同 core）。
#![allow(
    clippy::not_unsafe_ptr_arg_deref,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_cast,
    clippy::needless_range_loop,
    clippy::field_reassign_with_default
)]

pub mod blob;

use loomgui_core::input::{EventRecord, KeyEvent, PointerEvent};
use loomgui_core::scene::node::ControlState;
use loomgui_core::scene::{dynamic, NodeId};
use loomgui_core::stage::Stage;
use loomgui_core::style::computed::ComputedNodeStyle;
use loomgui_core::style::resolved::TextAlign;
use loomgui_core::transform::{self, NodeTransform};
use std::ffi::CString;

/// 版本字符串（C null-terminated `b"v1e\0"`）。
///
/// 返回 `*const u8`（csbindgen 映射为 C# `byte*`）；CString::as_ptr 给的是
/// `*const c_char`（i8），这里 cast 对齐签名。OnceLock 缓存，避免每次分配+泄漏。
#[no_mangle]
pub extern "C" fn loomgui_version() -> *const u8 {
    static VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| CString::new("v1e").unwrap())
        .as_ptr() as *const u8
}

/// opaque 句柄：Stage + 缓存的最近一帧 blob（borrow_frame 返回它的指针，下帧 reset）。
pub struct StageHandle {
    stage: Stage,
    frame_blob: Vec<u8>, // borrow_frame 返回 &this[..]；tick 时被覆盖。
    dump_blob: CString,  // dump_scene 缓存（Rust 拥有）
}

/// 创建 Stage 句柄（不收字体路径）。字体由 loomgui_stage_register_font 单独注册。
/// 失败返回 null（当前 Stage::new 不返回 Err，保留 null 分支以保持对称）。
#[no_mangle]
pub extern "C" fn loomgui_stage_new(w: f32, h: f32) -> *mut StageHandle {
    let stage = match Stage::new((w, h)) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(StageHandle {
        stage,
        frame_blob: Vec::new(),
        dump_blob: CString::new("").unwrap(),
    }))
}

/// 注册字体进 Stage 字体表。family = UTF-8 字符串（指针+len），bytes = ttf/ttc/otf 字节数据。
/// is_default: 0=否，非 0=是（设定为默认 fallback 字体）。返回 0=成功，-1=错误（null 句柄/非 UTF-8 family/字体解析失败）。
#[no_mangle]
pub extern "C" fn loomgui_stage_register_font(
    h: *mut StageHandle,
    family: *const u8,
    family_len: usize,
    bytes: *const u8,
    bytes_len: usize,
    is_default: u8,
) -> i32 {
    if h.is_null() || family.is_null() || bytes.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let family =
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(family, family_len) }) {
            Ok(s) => s,
            Err(_) => return -1,
        };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) }.to_vec();
    match sh.stage.register_font(family, bytes, is_default != 0) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 设全局字体回退链。text = UTF-8 字符串，family 名以 `\n` 分隔（如 "wqy-microhei\nLXGWWenKai"）。
/// 空/全空白 = 清空回退（退回单字体）。未 register 的 family 静默跳过。返回 0=成功，-1=错误。
/// 主字体缺字时按序 probe 回退链，首个含该字的补上（RmlUi fallback 模型）。
/// source-agnostic：后端把系统字体 register 进来后，其 family 名同样填这里即可。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_fallback_families(
    h: *mut StageHandle,
    text: *const u8,
    text_len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let families: Vec<String> = if text.is_null() || text_len == 0 {
        Vec::new()
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, text_len) }) {
            Ok(s) => s
                .split('\n')
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty())
                .collect(),
            Err(_) => return -1,
        }
    };
    sh.stage.set_fallback_families(&families);
    0
}

/// null-safe 释放 Stage 句柄。
#[no_mangle]
pub extern "C" fn loomgui_stage_free(h: *mut StageHandle) {
    if h.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(h));
    }
}

/// 装载二进制包（spec §12/§13）。name = 包名（进 packages 字典 key），bytes = .pkg.bin。
/// 0=ok，-1=err。null 句柄/空指针返回 -1。包是 Rust-internal，C# 只透传 bytes（不解析）。
///
/// FFI 签名带 name 参数（对齐 `Stage::load_package(name, bytes)`）。
/// load_package 只进资源池不建 scene——Unity 侧需先 create_root 建 scene 再 instantiate 建内容。
#[no_mangle]
pub extern "C" fn loomgui_stage_load_package(
    h: *mut StageHandle,
    name: *const u8,
    name_len: usize,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    if h.is_null() || name.is_null() || bytes.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
    match sh.stage.load_package(name, bytes) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 从包克隆一个组件进当前 scene，返组件根 NodeId（u32）。
/// pkg/comp = UTF-8 字节（指针+len）。失败返 0xFFFF_FFFF（INVALID，同 create_root 失败语义）。
/// scene 必须已存在（create_root 先建），否则 Err→sentinel。null 句柄 → sentinel。
///
/// **常驻（不 gate）。**包装 `Stage::instantiate(pkg, comp)`（spec §4.2/§4.4）。
#[no_mangle]
pub extern "C" fn loomgui_stage_instantiate(
    h: *mut StageHandle,
    pkg: *const u8,
    pkg_len: usize,
    comp: *const u8,
    comp_len: usize,
) -> u32 {
    const INVALID: u32 = 0xFFFF_FFFF;
    if h.is_null() || pkg.is_null() || comp.is_null() {
        return INVALID;
    }
    let sh = unsafe { &mut *h };
    let pkg = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(pkg, pkg_len) }) {
        Ok(s) => s,
        Err(_) => return INVALID,
    };
    let comp = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(comp, comp_len) }) {
        Ok(s) => s,
        Err(_) => return INVALID,
    };
    match sh.stage.instantiate(pkg, comp) {
        Ok(id) => id.0,
        Err(_) => INVALID,
    }
}

/// 跑一帧 tick_and_render → build_blob 写入缓存。dt 累积进 time_s（双击窗口，C# 传 unscaledDeltaTime）。
#[no_mangle]
pub extern "C" fn loomgui_stage_tick(h: *mut StageHandle, dt: f32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.advance_time(dt);
    let frame = sh.stage.tick_and_render();
    sh.frame_blob = blob::build_blob(&frame);
}

/// 借出最近一帧 blob：写 len 到 out_len，返回 Rust 拥有缓存指针（下 tick 失效）。
/// null 句柄或未 tick 过返回 null + len=0。
#[no_mangle]
pub extern "C" fn loomgui_stage_borrow_frame(
    h: *mut StageHandle,
    out_len: *mut usize,
) -> *const u8 {
    if h.is_null() {
        if !out_len.is_null() {
            unsafe { *out_len = 0 };
        }
        return std::ptr::null();
    }
    let sh = unsafe { &*h };
    // 未 tick 过：frame_blob 是空 Vec，as_ptr() 返回非空悬挂哨兵（违反"未 tick→null"契约）。
    // 显式判空 → null + len=0，与 null-handle 分支一致。
    if sh.frame_blob.is_empty() {
        if !out_len.is_null() {
            unsafe { *out_len = 0 };
        }
        return std::ptr::null();
    }
    if !out_len.is_null() {
        unsafe { *out_len = sh.frame_blob.len() };
    }
    sh.frame_blob.as_ptr()
}

/// dump 整树 JSON（调试）。返 Rust 拥有的 UTF-8 C 串 + len；下 tick 失效。
#[no_mangle]
pub extern "C" fn loomgui_stage_dump_scene(h: *mut StageHandle, out_len: *mut usize) -> *const u8 {
    if h.is_null() || out_len.is_null() {
        return std::ptr::null();
    }
    let handle = unsafe { &mut *h };
    let json = match &handle.stage.scene {
        Some(scene) => loomgui_core::dump::dump_scene_json(scene),
        None => String::from("[]"),
    };
    handle.dump_blob = CString::new(json).unwrap_or_else(|_| CString::new("[]").unwrap());
    let bytes = handle.dump_blob.as_bytes_with_nul();
    unsafe {
        *out_len = bytes.len();
    }
    handle.dump_blob.as_ptr() as *const u8
}

/// 注入本帧指针事件（扁平 PointerEvent 数组）。tick 前调。
/// null/len=0 = 本帧无输入事件（清空 pending_input，hover diff 仍跑——指针位置沿用上帧 last_pos）。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_input(
    h: *mut StageHandle,
    events: *const PointerEvent,
    len: usize,
) {
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
}

/// 拉取本帧事件 SOA（pull，同 borrow_frame 语义）。返 `last_events` 的 `as_ptr` + 写 len。
/// null 句柄或未 tick（last_events 空）→ null + len=0。指针下 tick 失效。
///
/// **常驻（不 gate）：**事件是 runtime 稳定入口。EventRecord 是 `#[repr(C)]` POD，
/// C 侧按 `len * sizeof(EventRecord)` 切片读。
#[no_mangle]
pub extern "C" fn loomgui_stage_borrow_events(
    h: *const StageHandle,
    out_len: *mut usize,
) -> *const u8 {
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
}

/// UI 挡住时游戏不响应点击（§10.6）。= 任一活跃槽 last_hit 非空且非根（多指：鼠标 slot0 + 已分配触摸槽）。
/// null 句柄 → false。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_is_pointer_on_ui(h: *const StageHandle) -> bool {
    if h.is_null() {
        return false;
    }
    let sh = unsafe { &*h };
    sh.stage.is_pointer_on_ui()
}

/// 业务设节点 disabled 状态（伪类源 + active/click 抑制）。NodeId.0 越界静默跳过。
/// null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_node_disabled(
    h: *mut StageHandle,
    node_id: u32,
    disabled: bool,
) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.set_node_disabled(NodeId(node_id), disabled);
}

/// 返 parent node_id（C# 事件路由沿链用，spec §4.2）。根/越界/无 scene → 0xFFFF_FFFF（sentinel）。
///
/// **常驻（不 gate）：**runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn loomgui_node_parent(h: *const StageHandle, node_id: u32) -> u32 {
    const ROOT_SENTINEL: u32 = 0xFFFF_FFFF;
    if h.is_null() {
        return ROOT_SENTINEL;
    }
    let sh = unsafe { &*h };
    match &sh.stage.scene {
        Some(scene) => {
            // NodeId(u32) → slotmap lookup（代际安全）。无效/悬空 NodeId → sentinel。
            match scene.get(NodeId(node_id)) {
                Some(n) => n.parent.map(|p| p.0 as u32).unwrap_or(ROOT_SENTINEL),
                None => ROOT_SENTINEL,
            }
        }
        None => ROOT_SENTINEL,
    }
}

/// 按 CSS id 属性查节点（业务用 id 定位节点替代硬编码 build 序 id）。
/// id = UTF-8 字节（指针+len）。返 node_id；null 句柄/非 UTF-8/无匹配 → 0xFFFF_FFFF（sentinel，同 node_parent）。
///
/// **常驻（不 gate）：**runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn loomgui_stage_find_node_by_id(
    h: *const StageHandle,
    id: *const u8,
    id_len: usize,
) -> u32 {
    const NOT_FOUND: u32 = 0xFFFF_FFFF;
    if h.is_null() || id.is_null() {
        return NOT_FOUND;
    }
    let sh = unsafe { &*h };
    let bytes = unsafe { std::slice::from_raw_parts(id, id_len) };
    let id_str = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return NOT_FOUND,
    };
    match sh.stage.find_node_by_id(id_str) {
        Some(nid) => nid.0 as u32,
        None => NOT_FOUND,
    }
}

/// 加 touch monitor（C# CaptureTouch 后调）。核心把 node 加进 touch_id 对应槽的 touch_monitors（去重）。
/// touch_id=-1 → 鼠标主指槽；找不到槽 → no-op。null 句柄 → no-op。
///
/// **常驻（不 gate）：**runtime 稳定入口。
#[no_mangle]
pub extern "C" fn loomgui_stage_add_touch_monitor(
    h: *mut StageHandle,
    touch_id: i32,
    node_id: u32,
) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.add_touch_monitor(touch_id, NodeId(node_id));
}

/// 移除 touch monitor（C# 主动释放调）。从所有槽移除该 node。null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_remove_touch_monitor(h: *mut StageHandle, node_id: u32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.remove_touch_monitor(NodeId(node_id));
}

/// 外部取消待 click（照 fgui Stage.CancelClick(touchId)）。置对应槽 click_cancelled。
/// null 句柄 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_cancel_click(h: *mut StageHandle, touch_id: i32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.cancel_click(touch_id);
}

/// 注入本帧键盘事件（扁平 KeyEvent 数组）。tick 前调。null/len=0 = 无键盘输入。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_key_input(
    h: *mut StageHandle,
    keys: *const KeyEvent,
    len: usize,
) {
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
}

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
}

/// 设文本控件的 IME composition（后端读平台 IME compositionString 回灌）。
/// text = UTF-8 字节（指针+len），pos = composition 在 value 中的字节偏移。
/// 非文本控件 / 越界 node → 静默跳过（仍返 0）。null 句柄 → -1。下一帧 measure/render
/// 会把 composition 拼进显示文本（下划线由 Task 12 composition 分支画）。
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
}

/// 提交文本控件的 composition（落定进 value）。返 1 = 有 composition 且 value 改变；
/// 0 = 无 composition（或被 readonly/max_length 拒）。非文本控件 / 越界 node → 0。
/// null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_commit_composition(h: *mut StageHandle, node: u32) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    sh.stage.commit_composition(NodeId(node)) as i32
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
pub extern "C" fn loomgui_register_clipboard(
    set_fn: Option<unsafe extern "C" fn(*const u8, usize) -> i32>,
    get_fn: Option<unsafe extern "C" fn(*mut *mut u8, *mut usize) -> i32>,
) {
    loomgui_core::scene::control::register_clipboard(set_fn, get_fn);
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
}

/// 注入本帧滚轮事件（扁平 WheelEvent 数组）。tick 前调；**累积式**（多次调合并）。
/// null/len=0 = 本帧无滚轮（直接 return，不清空——与 set_key_input 不同；累积语义）。
///
/// **常驻（不 gate）：**输入是 runtime 稳定入口。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_wheel_input(
    h: *mut StageHandle,
    events: *const loomgui_core::scroll::WheelEvent,
    len: usize,
) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    if events.is_null() || len == 0 {
        return;
    }
    let evs = unsafe { std::slice::from_raw_parts(events, len) };
    sh.stage.set_wheel_input(evs);
}

/// driver 启动时把所有 atlas.json 合并出的图尺寸批量灌入（一次调用，非逐条）。
/// paths_ptr: count 个 C 字符串指针；ws/hs: count 个 u32。任一为 null 或 count=0 → no-op。
/// 首帧 solve 前调（启动加载阶段）。FFI 入口不 panic。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_image_sizes(
    h: *mut StageHandle,
    paths_ptr: *const *const std::os::raw::c_char,
    ws: *const u32,
    hs: *const u32,
    count: usize,
) {
    if h.is_null() || paths_ptr.is_null() || ws.is_null() || hs.is_null() || count == 0 {
        return;
    }
    let handle = unsafe { &mut *h };
    let paths = unsafe { std::slice::from_raw_parts(paths_ptr, count) };
    let ws = unsafe { std::slice::from_raw_parts(ws, count) };
    let hs = unsafe { std::slice::from_raw_parts(hs, count) };
    let mut sizes: Vec<(String, u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        if paths[i].is_null() {
            continue;
        }
        let cstr = unsafe { std::ffi::CStr::from_ptr(paths[i]) };
        if let Ok(s) = cstr.to_str() {
            sizes.push((s.to_string(), ws[i], hs[i]));
        }
    }
    handle.stage.set_image_sizes(&sizes);
}

/// 编程滚动到指定位置。非 scroll 容器 / 越界 node → no-op（不 panic）。
/// animated: u8（0=瞬移 1=缓动 cubic-out）。null 句柄 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_scroll_pos(
    h: *mut StageHandle,
    node_id: u32,
    x: f32,
    y: f32,
    animated: u8,
) {
    if h.is_null() {
        return;
    }
    let handle = unsafe { &mut *h };
    handle
        .stage
        .set_scroll_pos(NodeId(node_id), x, y, animated != 0);
}

/// driver 注入滚动容器 content_size（虚拟列表）。node 无效/非滚动容器 → no-op。
/// null 句柄 → no-op（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_content_size(
    h: *mut StageHandle,
    node_id: u32,
    w: f32,
    height: f32,
) {
    if h.is_null() {
        return;
    }
    let handle = unsafe { &mut *h };
    handle.stage.set_content_size(NodeId(node_id), w, height);
}

/// 清除 driver 注入的 content_size override（列表销毁/退回普通滚动时用）。
/// null 句柄/无效 node → no-op（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_content_size_override(h: *mut StageHandle, node_id: u32) {
    if h.is_null() {
        return;
    }
    let handle = unsafe { &mut *h };
    handle.stage.clear_content_size_override(NodeId(node_id));
}

/// 读 scroll_pos。null 句柄/无效 node → out 填 0（不 panic）。
/// out_x/out_y 是 out 参数（C# 传 ref float）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_scroll_pos(
    h: *const StageHandle,
    node_id: u32,
    out_x: *mut f32,
    out_y: *mut f32,
) {
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
}

/// 读节点 layout_rect。null 句柄/无效 node → out 填 0（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_layout_rect(
    h: *const StageHandle,
    node_id: u32,
    out_x: *mut f32,
    out_y: *mut f32,
    out_w: *mut f32,
    out_h: *mut f32,
) {
    let r = if h.is_null() {
        None
    } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_layout_rect(NodeId(node_id))
    };
    let (x, y, w, hh) = r
        .map(|r| (r.x, r.y, r.w, r.h))
        .unwrap_or((0.0, 0.0, 0.0, 0.0));
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
    if !out_w.is_null() {
        unsafe {
            *out_w = w;
        }
    }
    if !out_h.is_null() {
        unsafe {
            *out_h = hh;
        }
    }
}

/// 读节点 world transform（compute_world_transforms 产物）。null/无效 → 写 identity。
/// out: a,b,c,d,tx,ty（6 个 f32，Affine2 列主序）。对齐 get_node_layout_rect 惯例
/// （独立 *mut out + 无状态码 + null/无效写默认）。空 div（merge_meshes 后 RenderNode
/// 消失）仍可查——world_transforms 保留全节点（与 node_sort_keys 同）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_world_matrix(
    h: *const StageHandle,
    node_id: u32,
    out_a: *mut f32,
    out_b: *mut f32,
    out_c: *mut f32,
    out_d: *mut f32,
    out_tx: *mut f32,
    out_ty: *mut f32,
) {
    let m = if h.is_null() {
        None
    } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_world_matrix(NodeId(node_id))
    }
    .unwrap_or(transform::IDENTITY); // [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    if !out_a.is_null() {
        unsafe {
            *out_a = m[0];
        }
    }
    if !out_b.is_null() {
        unsafe {
            *out_b = m[1];
        }
    }
    if !out_c.is_null() {
        unsafe {
            *out_c = m[2];
        }
    }
    if !out_d.is_null() {
        unsafe {
            *out_d = m[3];
        }
    }
    if !out_tx.is_null() {
        unsafe {
            *out_tx = m[4];
        }
    }
    if !out_ty.is_null() {
        unsafe {
            *out_ty = m[5];
        }
    }
}

/// 读节点 sort_key（merge 前快照，DFS 序号）。null/无效 → 写 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_sort_key(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u32,
) {
    let sk = if h.is_null() {
        None
    } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_sort_key(NodeId(node_id))
    }
    .unwrap_or(0);
    if !out.is_null() {
        unsafe {
            *out = sk;
        }
    }
}

/// 读节点可见性（存在 + 非 display:none）。null/无效 → 写 0（false）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_visible(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
) {
    let vis = if h.is_null() {
        false
    } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_visible(NodeId(node_id))
    };
    if !out.is_null() {
        unsafe {
            *out = if vis { 1 } else { 0 };
        }
    }
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

/// FFI 稳定快照（#[repr(C)] POD）。enum→u8（match 稳定化，不靠 enum 隐式 repr），
/// Option<[f32;4]>→present flag + 数组。csbindgen 自动生成 struct C# stub；④ 如需重排字段可扩展或手写覆盖。
#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct ComputedNodeStyleRepr {
    pub display_mode: u8,
    pub flex_direction: u8,
    pub overflow_x: u8,
    pub overflow_y: u8,
    pub color: [f32; 4],
    pub bg_present: u8,
    pub background_color: [f32; 4],
    pub opacity: f32,
    pub border_present: u8,
    pub border_color: [f32; 4],
    pub font_size: f32,
    pub font_weight: u16,
    pub text_align: u8,
    pub line_height: f32,
    pub letter_spacing: f32,
}

impl ComputedNodeStyleRepr {
    /// 从 typed `ComputedNodeStyle` 投影 FFI 稳定布局。
    /// `DisplayMode`/`OverflowMode` 是 `#[repr(u8)]` 可直接 `as u8`；`taffy::FlexDirection`
    /// （外部类型，无 repr 保证）与 `TextAlign`（无 repr）用 `match` 显式映射，不依赖判别值。
    fn from_computed(c: &ComputedNodeStyle) -> Self {
        let (bg_present, background_color) = match c.background_color {
            Some(col) => (1, col),
            None => (0, [0.0; 4]),
        };
        let (border_present, border_color) = match c.border_color {
            Some(col) => (1, col),
            None => (0, [0.0; 4]),
        };
        Self {
            display_mode: c.display_mode as u8,
            flex_direction: match c.flex_direction {
                taffy::FlexDirection::Row => 0,
                taffy::FlexDirection::Column => 1,
                taffy::FlexDirection::RowReverse => 2,
                taffy::FlexDirection::ColumnReverse => 3,
            },
            overflow_x: c.overflow_x as u8,
            overflow_y: c.overflow_y as u8,
            color: c.color,
            bg_present,
            background_color,
            opacity: c.opacity,
            border_present,
            border_color,
            font_size: c.font_size,
            font_weight: c.font_weight,
            text_align: match c.text_align {
                TextAlign::Left => 0,
                TextAlign::Center => 1,
                TextAlign::Right => 2,
            },
            line_height: c.line_height,
            letter_spacing: c.letter_spacing,
        }
    }
}

/// 读节点语义类型。return code：0 = ok 且 `*out` = kind 判别值；非 0 = 失败（节点不存在
/// 或 `out` = null）。不用 `-> u8` + 0 哨兵：`NodeKind` 首变体 `Container` 判别值 = 0，
/// 会与「不存在」撞。`NodeKind` 是 `#[repr(u8)]`，`k as u8` 跨 FFI 稳定。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_kind(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
) -> i32 {
    if h.is_null() {
        return 1;
    }
    let sh = unsafe { &*h };
    match sh.stage.get_node_kind(NodeId(node_id)) {
        Some(k) => {
            if out.is_null() {
                return 1;
            }
            unsafe { *out = k as u8 };
            0
        }
        None => 1,
    }
}

/// 读节点 computed style 快照。return code：0 = ok 且 `*out` 填好；非 0 = 失败（节点不存在
/// 或 `out` = null）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_computed_style(
    h: *const StageHandle,
    node_id: u32,
    out: *mut ComputedNodeStyleRepr,
) -> i32 {
    if h.is_null() {
        return 1;
    }
    let sh = unsafe { &*h };
    match sh.stage.get_node_computed_style(NodeId(node_id)) {
        Some(c) => {
            if out.is_null() {
                return 1;
            }
            unsafe { *out = ComputedNodeStyleRepr::from_computed(&c) };
            0
        }
        None => 1,
    }
}

// ===== font atlas FFI（v1.6 自绘字体 pull 模型） =====

/// 拉脏页 page_idx 列表（写入 out，返实际数）。null 句柄 / null out → 返 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_dirty_pages(
    h: *const StageHandle,
    out: *mut u32,
    max: usize,
) -> usize {
    if h.is_null() || out.is_null() {
        return 0;
    }
    let sh = unsafe { &*h };
    let buf = unsafe { std::slice::from_raw_parts_mut(out, max) };
    sh.stage.font_atlas_dirty_pages(buf)
}

/// 读某页 R8 像素 + 尺寸。buf_len 不够返所需大小（双调法：先传小 buf 探大小）。
/// 无此页 / null 句柄 / null out_buf → 返 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_page(
    h: *const StageHandle,
    page: u32,
    out_w: *mut u32,
    out_h: *mut u32,
    out_buf: *mut u8,
    buf_len: usize,
) -> usize {
    if h.is_null() {
        return 0;
    }
    let sh = unsafe { &*h };
    // 先探所需大小（传空 buf，不碰 out_buf 指针）
    let (mut w, mut hgt) = (0u32, 0u32);
    let needed = sh.stage.font_atlas_page(page, &mut w, &mut hgt, &mut []);
    if buf_len < needed {
        return needed; // 双调：caller 扩 buf 重调
    }
    if needed == 0 {
        return 0; // 空页 / 越界 page
    }
    // buf_len >= needed > 0：out_buf 必非 null（caller 保证），否则 slice 构造 UB。
    // 安全侧加防御检查。
    if out_buf.is_null() {
        return 0;
    }
    let buf = unsafe { std::slice::from_raw_parts_mut(out_buf, buf_len) };
    let n = sh.stage.font_atlas_page(page, &mut w, &mut hgt, buf);
    if !out_w.is_null() {
        unsafe {
            *out_w = w;
        }
    }
    if !out_h.is_null() {
        unsafe {
            *out_h = hgt;
        }
    }
    n
}

/// 清脏页（backend 拉完后调）。null 句柄 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_clear_dirty(h: *mut StageHandle) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.font_atlas_clear_dirty();
}

/// 设渲染复用键（虚拟列表 slot）。null 句柄/无效 node → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_reuse_key(h: *mut StageHandle, node_id: u32, key: u32) {
    if h.is_null() {
        return;
    }
    let handle = unsafe { &mut *h };
    handle.stage.set_reuse_key(NodeId(node_id), key);
}

/// 编程聚焦节点（照 fgui RequestFocus）。强制聚焦任意非 disabled 节点
/// （含 tabindex=None/-1）；disabled 拒；越界跳过。null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_request_focus(h: *mut StageHandle, node_id: u32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.request_focus(NodeId(node_id));
}

/// 读当前焦点节点。无焦点/无 scene → 0xFFFF_FFFF（sentinel，同 node_parent）。null 句柄 → sentinel。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_focused_node(h: *const StageHandle) -> u32 {
    const NONE: u32 = 0xFFFF_FFFF;
    if h.is_null() {
        return NONE;
    }
    let sh = unsafe { &*h };
    match &sh.stage.scene {
        Some(scene) => scene.focused_node.map(|n| n.0 as u32).unwrap_or(NONE),
        None => NONE,
    }
}

/// 全局 shutdown（Domain reload hook）。C# `LoomStage.ResetStatics`（SubsystemRegistration）调用。
///
/// 当前核心无全局 native 态——Stage 是 per-handle（`loomgui_stage_free` drop 全部 Stage 拥有的内存），
/// 故本函数 near-no-op。但 hook 必须存在：将来引入全局 texture/font registry（进程级单例缓存）时，
/// 此处自动成为清理入口，无需再改 C# 接线。
///
/// **注意：Font 的 `Box::leak`（`text/layout.rs:76`）是真泄漏**——`bytes.clone()` 后 leak 取
/// `'static` 切片喂 ttf-parser Face，原 Vec 虽被 `_bytes` 持有但与 leaked 切片不是同一份，
/// Stage drop 时 `_bytes` 释放的是 clone 来源而非 leaked 副本。每次 Stage 创建都 leak 一份字体字节，
/// 不可由 shutdown 回收（leak 切片无 handle 跟踪）。若未来域重载内存观测触发阈值，
/// 再考虑字体缓存化为进程单例。
#[no_mangle]
pub extern "C" fn loomgui_shutdown() {}

// ===== tween FFI =====

/// 注册 tween。start/end 指向 ≥value_size 个 f32（value_size 由 prop 隐含）。
/// null 句柄/null 指针 → no-op。越界 node / duration<=0 由 core update 处理（跳过/立即 complete）。
#[no_mangle]
pub extern "C" fn loomgui_stage_tween(
    h: *mut StageHandle,
    node_id: u32,
    prop: u32,
    start: *const f32,
    end: *const f32,
    duration: f32,
    ease: u32,
    delay: f32,
    tag: u32,
) {
    if h.is_null() || start.is_null() || end.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    let prop = match loomgui_core::tween::TweenProp::try_from(prop) {
        Some(p) => p,
        None => return,
    };
    let ease = match loomgui_core::tween::Ease::try_from(ease) {
        Some(e) => e,
        None => return,
    };
    let sz = loomgui_core::tween::prop_value_size(prop) as usize;
    let st = unsafe { std::slice::from_raw_parts(start, sz) };
    let en = unsafe { std::slice::from_raw_parts(end, sz) };
    let mut s = [0.0f32; 4];
    let mut e = [0.0f32; 4];
    s[..sz].copy_from_slice(st);
    e[..sz].copy_from_slice(en);
    sh.stage
        .tween(NodeId(node_id), prop, s, e, ease, delay, duration, tag);
}

/// 停该节点该 prop 的 tween（override 保留末值）。
#[no_mangle]
pub extern "C" fn loomgui_stage_kill_tween(h: *mut StageHandle, node_id: u32, prop: u32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    if let Some(prop) = loomgui_core::tween::TweenProp::try_from(prop) {
        sh.stage.kill_tween(NodeId(node_id), prop);
    }
}

/// 清该节点所有动画 override（回 CSS）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_anim(h: *mut StageHandle, node_id: u32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    sh.stage.clear_anim(NodeId(node_id));
}

/// 清该节点某 prop 对应通道（回 CSS）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_anim_prop(h: *mut StageHandle, node_id: u32, prop: u32) {
    if h.is_null() {
        return;
    }
    let sh = unsafe { &mut *h };
    if let Some(prop) = loomgui_core::tween::TweenProp::try_from(prop) {
        sh.stage.clear_anim_prop(NodeId(node_id), prop);
    }
}

// ===== 动态树 API FFI（§7.2）：create_root/create_node/append_child/insert_before/
// remove_child/remove_node/set_text/set_src。转调 Stage 方法。
// 错误语义：create_root/create_node 返 u32 NodeId（0xFFFF_FFFF = 失败）；
// 其余返 i32（0=ok，-1=err）。null 句柄 → 失败/sentinel（不 panic）。

/// 建根节点并设为 roots[0]。kind/css = UTF-8 字节。返 NodeId；0xFFFF_FFFF = 失败。
///
/// null 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）：**runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn loomgui_stage_create_root(
    h: *mut StageHandle,
    kind: *const u8,
    kind_len: usize,
    css: *const u8,
    css_len: usize,
) -> u32 {
    const FAIL: u32 = 0xFFFF_FFFF;
    if h.is_null() {
        return FAIL;
    }
    let sh = unsafe { &mut *h };
    // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
    let kind = if kind.is_null() || kind_len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(kind, kind_len) }) {
            Ok(s) => s,
            Err(_) => return FAIL,
        }
    };
    let css = if css.is_null() || css_len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(css, css_len) }) {
            Ok(s) => s,
            Err(_) => return FAIL,
        }
    };
    match sh.stage.create_root(kind, css) {
        Ok(id) => id.0,
        Err(_) => FAIL,
    }
}

/// 建节点（不挂父）。kind/css = UTF-8 字节。返 NodeId；0xFFFF_FFFF = 失败。
/// 需配合 append_child/insert_before 挂到树。
///
/// null 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_create_node(
    h: *mut StageHandle,
    kind: *const u8,
    kind_len: usize,
    css: *const u8,
    css_len: usize,
) -> u32 {
    const FAIL: u32 = 0xFFFF_FFFF;
    if h.is_null() {
        return FAIL;
    }
    let sh = unsafe { &mut *h };
    // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
    let kind = if kind.is_null() || kind_len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(kind, kind_len) }) {
            Ok(s) => s,
            Err(_) => return FAIL,
        }
    };
    let css = if css.is_null() || css_len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(css, css_len) }) {
            Ok(s) => s,
            Err(_) => return FAIL,
        }
    };
    match sh.stage.create_node(kind, css) {
        Ok(id) => id.0,
        Err(_) => FAIL,
    }
}

/// 挂子到 parent 末尾。child 必须当前无父。0=ok，-1=err。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_append_child(h: *mut StageHandle, parent: u32, child: u32) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    sh.stage
        .append_child(NodeId(parent), NodeId(child))
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 在 parent.children 中 ref_id 之前插 child。ref_id=0xFFFF_FFFF（INVALID）→ 末尾追加。
/// 0=ok，-1=err。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_insert_before(
    h: *mut StageHandle,
    parent: u32,
    child: u32,
    ref_id: u32,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    sh.stage
        .insert_before(NodeId(parent), NodeId(child), NodeId(ref_id))
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 摘子（不删节点）：从 parent.children 移除 + child.parent=None。节点仍 live 可重挂。
/// 0=ok，-1=err。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_remove_child(h: *mut StageHandle, parent: u32, child: u32) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    sh.stage
        .remove_child(NodeId(parent), NodeId(child))
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 删节点（递归删子 + 联动清 anim/scroll/tween + slotmap remove）。
/// 该 NodeId 句柄此后失效（gen++）。无 scene / 越界 → no-op。返 0（恒成功，no-op 语义）。
/// null 句柄 → 0（no-op，不 panic）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_remove_node(h: *mut StageHandle, node: u32) -> i32 {
    if h.is_null() {
        return 0;
    }
    let sh = unsafe { &mut *h };
    sh.stage.remove_node(NodeId(node));
    0
}

/// 改 Text 节点 content + 标 dirty_text。text = UTF-8 字节。0=ok，-1=err。
/// 非 Text 节点 → -1（Stage::set_text Err）。null 句柄 → -1。
///
/// null text 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_text(
    h: *mut StageHandle,
    node: u32,
    text: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
    let text = if text.is_null() || len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, len) }) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    sh.stage
        .set_text(NodeId(node), text)
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 改 Image 节点 src + 标 dirty_mesh。src = UTF-8 字节。0=ok，-1=err。
/// 非 Image 节点 → -1。null 句柄 → -1。
///
/// null src 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_src(
    h: *mut StageHandle,
    node: u32,
    src: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
    let src = if src.is_null() || len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(src, len) }) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    sh.stage.set_src(NodeId(node), src).map(|_| 0).unwrap_or(-1)
}

/// 写 inline override（便签层，优先级 > 动态规则 > base_style）。css = UTF-8 字节。
/// 0=ok，-1=err（null 句柄 / 非 UTF-8 / 节点不 live）。下帧 rematch 应用。
///
/// null css 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_inline_override(
    h: *mut StageHandle,
    node: u32,
    css: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
    let css = if css.is_null() || len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(css, len) }) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    sh.stage
        .set_inline_override(NodeId(node), css)
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 清 inline override 的某 prop bit。prop = UTF-8 字节。0=ok，-1=err。
/// prop 不可 inline 时为 no-op（仍返 0）。
///
/// null prop 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_unset_inline_override(
    h: *mut StageHandle,
    node: u32,
    prop: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    // null/零长兜底为空串：slice::from_raw_parts(null, 0) 是 UB，即使 len=0。
    let prop = if prop.is_null() || len == 0 {
        ""
    } else {
        match std::str::from_utf8(unsafe { std::slice::from_raw_parts(prop, len) }) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    sh.stage
        .unset_inline_override(NodeId(node), prop)
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 读节点子节点数。返回 i32：≥0 = 子节点数；-1 = err（null 句柄 / 节点不 live）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_child_count(h: *const StageHandle, node: u32) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    match sh.stage.get_child_count(NodeId(node)) {
        Some(c) => c as i32,
        None => -1,
    }
}

/// 读节点子节点 NodeId 列表，写入 `out` buffer（u32 per slot）。
/// 返回 i32：≥0 = 实际写入数；负值 = err（-1 = null 句柄 / 节点不 live；
/// -(n+2) = buffer 不够，n = 所需 cap）。调用方遇负值重分配 n+ 容量再调。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_children(
    h: *const StageHandle,
    node: u32,
    out: *mut u32,
    cap: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    match sh.stage.get_children(NodeId(node)) {
        None => -1,
        Some(kids) => {
            if kids.len() > cap {
                return -(kids.len() as i32 + 2);
            }
            if !out.is_null() {
                for (i, k) in kids.iter().enumerate() {
                    unsafe {
                        *out.add(i) = k.0;
                    }
                }
            }
            kids.len() as i32
        }
    }
}

/// 加 class（重复名不重复 push）。name = UTF-8 字节。0=ok，-1=err。
/// 标 dirty_mesh 触发下帧 rematch。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_add_class(
    h: *mut StageHandle,
    node: u32,
    name: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, len) }) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    sh.stage
        .add_class(NodeId(node), name)
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 移除 class（全部匹配）。name = UTF-8 字节。0=ok，-1=err。标 dirty_mesh。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_remove_class(
    h: *mut StageHandle,
    node: u32,
    name: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, len) }) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    sh.stage
        .remove_class(NodeId(node), name)
        .map(|_| 0)
        .unwrap_or(-1)
}

/// 查询 class 是否存在。返回 i32：1 = true；0 = false；-1 = err（null 句柄 / 节点不 live）。
/// name = UTF-8 字节，非 UTF-8 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_has_class(
    h: *const StageHandle,
    node: u32,
    name: *const u8,
    len: usize,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, len) }) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match sh.stage.has_class(NodeId(node), name) {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    }
}

// ===== control state + transform get/set FFI（C# 投影层控件属性回写出口）=====
//
// 业务读写 ProgressBar/Slider 的 value/max、Toggle/Radio 的 checked，
// 以及运行时高频 transform（拖拽 thumb）。所有 getter 走 return-code + out-param
// （rc=0 严格意味 *out 已填；非 0 = err），避免 enum 判别值 0 与哨兵撞。
//
// 实现走「clone ControlState → 改字段 → re-ensure」模式：ControlTable 是 HashMap，
// 原地改需 &mut 借出后写回不便，clone 后整覆盖更直捷且 ControlState 小（克隆成本低）。
// 非语义适用（如 Progress 的 min/step）→ -1，不静默降级。

/// 设控件 value（ProgressBar / Slider）。ProgressBar clamp [0, max]；
/// Slider clamp [min, max] 并按 step 量化。非 value 控件 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_value(
    h: *mut StageHandle,
    node_id: u32,
    value: f32,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let scene = match sh.stage.scene.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    let id = NodeId(node_id);
    let Some(state) = scene.controls.get(id).cloned() else {
        return -1;
    };
    let new_state = match state {
        ControlState::Progress {
            max, indeterminate, ..
        } => {
            // 存储的 max 来自 ControlInit（instantiate sanitize 到 ≥0）或 set_control_max
            // （guard 到 ≥0），但 FFI 边界纵深守卫：负 max 会让 clamp(0.0,max) panic。
            let max = max.max(0.0);
            let clamped = value.clamp(0.0, max);
            ControlState::Progress {
                value: clamped,
                max,
                indeterminate,
            }
        }
        ControlState::Slider {
            min,
            max,
            step,
            dragging,
            ..
        } => {
            // 同上：clamp(min,max) 在 min>max 时 panic，FFI 边界纵深守卫。
            let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
            let clamped = value.clamp(lo, hi);
            let quantized = if step > 0.0 {
                // 对齐到最近 step 边界：round((v - min) / step) * step + min
                ((clamped - lo) / step).round() * step + lo
            } else {
                clamped
            };
            // 量化可能把值推过 hi（如 lo=0,hi=100,step=6,v=100 → 102），
            // 重新 clamp 回 [lo,hi]，保证不违反区间契约。
            let quantized = quantized.clamp(lo, hi);
            ControlState::Slider {
                value: quantized,
                min: lo,
                max: hi,
                step,
                dragging,
            }
        }
        _ => return -1,
    };
    scene.controls.ensure(id, new_state);
    0
}

/// 读控件 value（ProgressBar / Slider）。rc=0 且 *out 已填；非 value 控件 / null out /
/// 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_value(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
) -> i32 {
    if h.is_null() || out.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    let Some(scene) = sh.stage.scene.as_ref() else {
        return -1;
    };
    match scene.controls.get(NodeId(node_id)) {
        Some(ControlState::Progress { value, .. } | ControlState::Slider { value, .. }) => {
            unsafe { *out = *value };
            0
        }
        _ => -1,
    }
}

/// 设控件 checked（Toggle / Radio）。非 check 控件 / null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_checked(
    h: *mut StageHandle,
    node_id: u32,
    checked: bool,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let scene = match sh.stage.scene.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    let id = NodeId(node_id);
    let Some(state) = scene.controls.get(id).cloned() else {
        return -1;
    };
    let new_state = match state {
        ControlState::Toggle { .. } => ControlState::Toggle { checked },
        ControlState::Radio { name, .. } => ControlState::Radio { checked, name },
        _ => return -1,
    };
    scene.controls.ensure(id, new_state);
    0
}

/// 读控件 checked（Toggle / Radio）。rc=0 且 *out 已填；非 check 控件 / null out /
/// 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_checked(
    h: *const StageHandle,
    node_id: u32,
    out: *mut bool,
) -> i32 {
    if h.is_null() || out.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    let Some(scene) = sh.stage.scene.as_ref() else {
        return -1;
    };
    match scene.controls.get(NodeId(node_id)) {
        Some(ControlState::Toggle { checked } | ControlState::Radio { checked, .. }) => {
            unsafe { *out = *checked };
            0
        }
        _ => -1,
    }
}

/// 设控件 max（ProgressBar / Slider）。null 句柄 / 非值控件 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_max(
    h: *mut StageHandle,
    node_id: u32,
    max: f32,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let scene = match sh.stage.scene.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    let id = NodeId(node_id);
    let Some(state) = scene.controls.get(id).cloned() else {
        return -1;
    };
    let new_state = match state {
        ControlState::Progress {
            value,
            indeterminate,
            ..
        } => {
            // Progress 的 max 天然非负；caller 可能传负值，先 guard 到 ≥0 再 clamp。
            // f32::clamp 在 min > max（即 0.0 > max）时 panic，FFI 不可因 caller
            // 输入 abort 宿主进程（镜像 Slider arm 的 max.max(min) 守卫）。
            let max = max.max(0.0);
            // 改 max 后把 value 重新 clamp 进新区间（避免 value > max 的悬空态）
            let value = value.clamp(0.0, max);
            ControlState::Progress {
                value,
                max,
                indeterminate,
            }
        }
        ControlState::Slider {
            value,
            min,
            step,
            dragging,
            ..
        } => {
            let max = max.max(min);
            let clamped = value.clamp(min, max);
            // 改 max 后重新量化（与 set_control_value 同口径，维持 step 对齐不变量）。
            let value = if step > 0.0 {
                ((clamped - min) / step).round() * step + min
            } else {
                clamped
            }
            .clamp(min, max);
            ControlState::Slider {
                value,
                min,
                max,
                step,
                dragging,
            }
        }
        _ => return -1,
    };
    scene.controls.ensure(id, new_state);
    0
}

/// 读控件 max（ProgressBar / Slider）。非值控件 / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_max(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
) -> i32 {
    if h.is_null() || out.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    let Some(scene) = sh.stage.scene.as_ref() else {
        return -1;
    };
    match scene.controls.get(NodeId(node_id)) {
        Some(ControlState::Progress { max, .. } | ControlState::Slider { max, .. }) => {
            unsafe { *out = *max };
            0
        }
        _ => -1,
    }
}

/// 设控件 min（Slider 独有；ProgressBar 无 min 语义 → -1）。
/// null 句柄 / 节点缺失 → -1。改 min 后 value 重新 clamp。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_min(
    h: *mut StageHandle,
    node_id: u32,
    min: f32,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let scene = match sh.stage.scene.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    let id = NodeId(node_id);
    let Some(state) = scene.controls.get(id).cloned() else {
        return -1;
    };
    let new_state = match state {
        ControlState::Slider {
            value,
            max,
            step,
            dragging,
            ..
        } => {
            let min = min.min(max);
            let value = value.clamp(min, max);
            ControlState::Slider {
                value,
                min,
                max,
                step,
                dragging,
            }
        }
        _ => return -1,
    };
    scene.controls.ensure(id, new_state);
    0
}

/// 读控件 min（Slider 独有）。非 Slider / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_min(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
) -> i32 {
    if h.is_null() || out.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    let Some(scene) = sh.stage.scene.as_ref() else {
        return -1;
    };
    match scene.controls.get(NodeId(node_id)) {
        Some(ControlState::Slider { min, .. }) => {
            unsafe { *out = *min };
            0
        }
        _ => -1,
    }
}

/// 设控件 step（Slider 独有；ProgressBar 无 step 语义 → -1）。
/// null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_step(
    h: *mut StageHandle,
    node_id: u32,
    step: f32,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let scene = match sh.stage.scene.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    let id = NodeId(node_id);
    let Some(state) = scene.controls.get(id).cloned() else {
        return -1;
    };
    let new_state = match state {
        ControlState::Slider {
            value,
            min,
            max,
            dragging,
            ..
        } => {
            // step 语义为正（量化步长）：负值/NaN 无意义，拒绝而非存脏（下游 step>0.0
            // 守卫虽不 panic，但存负 step 会让 set_control_value 的量化分支走错路径）。
            if !step.is_finite() || step < 0.0 {
                return -1;
            }
            ControlState::Slider {
                value,
                min,
                max,
                step,
                dragging,
            }
        }
        _ => return -1,
    };
    scene.controls.ensure(id, new_state);
    0
}

/// 读控件 step（Slider 独有）。非 Slider / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_step(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
) -> i32 {
    if h.is_null() || out.is_null() {
        return -1;
    }
    let sh = unsafe { &*h };
    let Some(scene) = sh.stage.scene.as_ref() else {
        return -1;
    };
    match scene.controls.get(NodeId(node_id)) {
        Some(ControlState::Slider { step, .. }) => {
            unsafe { *out = *step };
            0
        }
        _ => -1,
    }
}

/// 设节点 user transform（位移/缩放/旋转）。走 `set_user_transform`（dynamic.rs）：
/// 只写 `node.user_transform`，不触发 layout solve——`compute_world_transforms` 在
/// 世界矩阵累计时并入（渲染/命中层，同 CSS transform）。供高频拖拽等运行时定位用。
/// 不 live 节点 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_transform(
    h: *mut StageHandle,
    node_id: u32,
    tx: f32,
    ty: f32,
    sx: f32,
    sy: f32,
    rot: f32,
) -> i32 {
    if h.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let scene = match sh.stage.scene.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    let t = NodeTransform {
        translate: [tx, ty],
        scale: [sx, sy],
        rotation: rot,
    };
    match dynamic::set_user_transform(scene, NodeId(node_id), t) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod abi_tests;
#[cfg(test)]
mod test_helpers;
