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
use loomgui_core::scene::animation::{
    player_key_as_u64, player_key_from_u64, register_on_key, PlayerPlayState,
};
use loomgui_core::scene::node::{ControlState, EditState};
use loomgui_core::scene::{dynamic, NodeId};
use loomgui_core::stage::Stage;
use loomgui_core::style::computed::ComputedNodeStyle;
use loomgui_core::style::resolved::TextAlign;
use loomgui_core::transform::{self, NodeTransform};
use std::ffi::CString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};

/// FFI panic 兜底计数：guard 捕获的 panic 累计（0 = 从未）。
static FFI_PANIC_COUNT: AtomicU32 = AtomicU32::new(0);

/// 全部导出的统一 panic 边界。panic 展开穿越 `extern "C"` 是 UB（实践中直接
/// abort 宿主进程——本库的宿主是 Unity 编辑器/玩家，不可接受），因此在函数体内
/// 捕获：计数、返回调用方约定的错误哨兵。panic 消息由默认 panic hook 先行打到
/// stderr。取舍：panic 点之后 Stage 可能处于半修改状态，继续运行不保证一致——
/// 但比崩宿主可诊断；后端每帧读 `loomgui_ffi_panic_count`，变化即告警。
fn ffi_guard<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => {
            FFI_PANIC_COUNT.fetch_add(1, Ordering::Relaxed);
            fallback
        }
    }
}

/// 读 FFI panic 兜底累计计数（后端每帧轮询，变化即有 Rust panic 被吞）。
#[no_mangle]
pub extern "C" fn loomgui_ffi_panic_count() -> u32 {
    FFI_PANIC_COUNT.load(Ordering::Relaxed)
}

/// 版本字符串（C null-terminated `b"v1e\0"`）。
///
/// 返回 `*const u8`（csbindgen 映射为 C# `byte*`）；CString::as_ptr 给的是
/// `*const c_char`（i8），这里 cast 对齐签名。OnceLock 缓存，避免每次分配+泄漏。
#[no_mangle]
pub extern "C" fn loomgui_version() -> *const u8 {
    ffi_guard(std::ptr::null(), || {
        static VERSION: std::sync::OnceLock<CString> = std::sync::OnceLock::new();
        VERSION
            .get_or_init(|| CString::new("v1e").unwrap())
            .as_ptr() as *const u8
    })
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
    ffi_guard(std::ptr::null_mut(), || {
        let stage = match Stage::new((w, h)) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        Box::into_raw(Box::new(StageHandle {
            stage,
            frame_blob: Vec::new(),
            dump_blob: CString::new("").unwrap(),
        }))
    })
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
    ffi_guard(-1, || {
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
    })
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
    ffi_guard(-1, || {
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
    })
}

/// null-safe 释放 Stage 句柄。
#[no_mangle]
pub extern "C" fn loomgui_stage_free(h: *mut StageHandle) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        unsafe {
            drop(Box::from_raw(h));
        }
    })
}

/// 装载二进制包（spec §12/§13）。name = 包名（进 packages 字典 key），bytes = .pkg.bin。
/// 0=ok；1=pkg 格式版本过旧（TooOld）；2=过新（TooNew）；-1=其他 err（null/UTF-8/损坏）。
/// 版本错配时用 `loomgui_stage_last_pkg_load_version` 取 pkg 声明的版本、
/// `loomgui_pkg_format_version` 取运行时版本，给「Unity 包与 loom.exe 同版本重打」的专属指引。
/// 包是 Rust-internal，C# 只透传 bytes（不解析）。
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
    ffi_guard(-1, || {
        if h.is_null() || name.is_null() || bytes.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) })
        {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
        sh.stage.last_pkg_load_version = 0;
        match sh.stage.load_package(name, bytes) {
            Ok(()) => 0,
            Err(loomgui_core::stage::LoadPkgError::TooOld { pkg, .. }) => {
                sh.stage.last_pkg_load_version = pkg;
                1
            }
            Err(loomgui_core::stage::LoadPkgError::TooNew { pkg, .. }) => {
                sh.stage.last_pkg_load_version = pkg;
                2
            }
            Err(_) => -1,
        }
    })
}

/// 最近一次 load_package 失败的 pkg 声明格式版本（0=无/非版本错）。
/// 配合 `loomgui_stage_load_package` 返回码 1/2 使用。
#[no_mangle]
pub extern "C" fn loomgui_stage_last_pkg_load_version(h: *const StageHandle) -> u32 {
    ffi_guard(0, || {
        if h.is_null() {
            return 0;
        }
        unsafe { (*h).stage.last_pkg_load_version }
    })
}

/// 运行时（本 dll）支持的 pkg 格式版本。pkg 错配诊断用。
#[no_mangle]
pub extern "C" fn loomgui_pkg_format_version() -> u32 {
    loomgui_core::asset::PKG_FORMAT_VERSION
}

/// 卸载包：从 Rust stage 移除模板注册（prefab 删除语义——已实例化活节点不受影响）。
/// atlas 纹理/字体不在此列（workspace 级共享 / driver 级注册，皆不隶属包）。
/// 0=ok；-1=err（null 句柄 / 非 UTF-8 / 包未加载）。
#[no_mangle]
pub extern "C" fn loomgui_stage_unload_package(
    h: *mut StageHandle,
    name: *const u8,
    name_len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || name.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) })
        {
            Ok(s) => s,
            Err(_) => return -1,
        };
        match sh.stage.unload_package(name) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
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
    ffi_guard(u32::MAX, || {
        const INVALID: u32 = 0xFFFF_FFFF;
        if h.is_null() || pkg.is_null() || comp.is_null() {
            return INVALID;
        }
        let sh = unsafe { &mut *h };
        let pkg = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(pkg, pkg_len) }) {
            Ok(s) => s,
            Err(_) => return INVALID,
        };
        let comp = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(comp, comp_len) })
        {
            Ok(s) => s,
            Err(_) => return INVALID,
        };
        match sh.stage.instantiate(pkg, comp) {
            Ok(id) => id.0,
            Err(_) => INVALID,
        }
    })
}

/// 跑一帧 tick_and_render → build_blob 写入缓存。dt 累积进 time_s（双击窗口，C# 传 unscaledDeltaTime）。
#[no_mangle]
pub extern "C" fn loomgui_stage_tick(h: *mut StageHandle, dt: f32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.advance_time(dt);
        let frame = sh.stage.tick_and_render();
        // parked slot keepalive 需读 scene（list 池状态）——tick 产完 FrameData 后 scene 可重新不可变借用。
        // scene=None（load 前）：仍产结构合法的空 blob（node_count=0），与旧行为逐字节一致。
        let empty;
        let scene = match sh.stage.scene.as_ref() {
            Some(s) => s,
            None => {
                empty = loomgui_core::scene::node::Scene::default();
                &empty
            }
        };
        sh.frame_blob = blob::build_blob(&frame, scene);
    })
}

/// 借出最近一帧 blob：写 len 到 out_len，返回 Rust 拥有缓存指针（下 tick 失效）。
/// null 句柄或未 tick 过返回 null + len=0。
#[no_mangle]
pub extern "C" fn loomgui_stage_borrow_frame(
    h: *mut StageHandle,
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
    })
}

/// dump 整树 JSON（调试）。返 Rust 拥有的 UTF-8 C 串 + len；下 tick 失效。
#[no_mangle]
pub extern "C" fn loomgui_stage_dump_scene(h: *mut StageHandle, out_len: *mut usize) -> *const u8 {
    ffi_guard(std::ptr::null(), || {
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
    })
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
pub extern "C" fn loomgui_stage_borrow_events(
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
pub extern "C" fn loomgui_stage_get_event_string(
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
pub extern "C" fn loomgui_stage_is_pointer_on_ui(h: *const StageHandle) -> bool {
    ffi_guard(false, || {
        if h.is_null() {
            return false;
        }
        let sh = unsafe { &*h };
        sh.stage.is_pointer_on_ui()
    })
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
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.set_node_disabled(NodeId(node_id), disabled);
    })
}

/// 设节点 touchable（公共 Node.Touchable 的后端；CSS `pointer-events` 的运行时面）。
/// false = 本节点不参与命中（子节点照常——透传语义）。写 interaction（hit 判据）+
/// base_style（rematch 重起源）。null 句柄 / 节点缺失 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_node_touchable(
    h: *mut StageHandle,
    node_id: u32,
    touchable: bool,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.set_node_touchable(NodeId(node_id), touchable);
    })
}

/// 读节点 touchable（interaction.touchable，hit_test 同源）。null 句柄 / 无 scene /
/// 节点缺失 → -1（不与 false 混淆）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_touchable(
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
        match scene.get(NodeId(node_id)) {
            Some(n) => {
                unsafe { *out = u8::from(n.interaction.touchable) };
                0
            }
            None => -1,
        }
    })
}

/// 读节点 LOOKUP_SCOPE 查找边界标记（组件展开域 host / ListView slot 根 / 实例根打此位）。
/// C# Query&lt;T&gt;/Query(selector) 的 DFS 剪枝用——遇此标记的子节点 visit 后不下钻
///（Get/TryGet 走 core find_node_by_id_in_subtree 已内置剪枝，本 FFI 补 Query 的 C# 侧路径）。
/// null 句柄 / 无 scene / 节点缺失 → -1（不与 false 混淆）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_node_is_lookup_scope(h: *const StageHandle, node_id: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match scene.get(NodeId(node_id)) {
            Some(n) => i32::from(
                n.interaction
                    .flags
                    .contains(loomgui_core::scene::node::NodeFlags::LOOKUP_SCOPE),
            ),
            None => -1,
        }
    })
}

/// 读 CustomElement 原始 hyphen 标签名（`<game-item-card>` → "game-item-card"；打包期展开
/// 保留，tag 选择器 + 诊断用）。双调法：首次 buf_cap 不足返 -2 + out_len 写所需字节数，
/// 调用方二次调用取串。非 CustomElement / null 句柄 / 无 scene / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_custom_tag(
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
        let Some(tag) = scene
            .get(NodeId(node_id))
            .and_then(|n| n.custom_tag.as_deref())
        else {
            return -1;
        };
        let bytes = tag.as_bytes();
        let needed = bytes.len();
        unsafe { *out_len = needed };
        if needed > buf_cap {
            return -2;
        }
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

/// 返 parent node_id（C# 事件路由沿链用，spec §4.2）。根/越界/无 scene → 0xFFFF_FFFF（sentinel）。
///
/// **常驻（不 gate）：**runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn loomgui_node_parent(h: *const StageHandle, node_id: u32) -> u32 {
    ffi_guard(u32::MAX, || {
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
    })
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
    ffi_guard(u32::MAX, || {
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
    })
}

/// 在 root 子树内 DFS 查找 id 属性匹配的首个节点（self-exclusive：从 root 的直接子开始 DFS，root 自身 id_attr 不参与匹配，与 DOM querySelectorAll/Query<T> 一致）。
/// root、id = UTF-8 字节（指针+len）。返 node_id；null 句柄/非 UTF-8/无匹配 → 0xFFFF_FFFF（sentinel）。
/// 替代"全局首匹配 + 父链后过滤"——C# TryGet/Get 用此入口避免 list slot 间 id 碰撞。
///
/// **常驻（不 gate）：**runtime 稳定入口。
#[no_mangle]
pub extern "C" fn loomgui_stage_find_node_by_id_in_subtree(
    h: *const StageHandle,
    root: u32,
    id: *const u8,
    id_len: usize,
) -> u32 {
    ffi_guard(u32::MAX, || {
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
        match sh.stage.find_node_by_id_in_subtree(NodeId(root), id_str) {
            Some(nid) => nid.0 as u32,
            None => NOT_FOUND,
        }
    })
}

/// 构造最小测试包（headless test fixture helper）。
/// 组件名=comp_spec UTF-8 前缀（取 comp_len 长度），含两个 Container 节点：根容器 + 子容器 id="badge"（2-node，配合 self-exclusive 子树查找）。
/// 返 pkg bytes 指针+长度；失败返 null（空字符串/格式错）。
/// 调用方用完后调 loomgui_bytes_free 释放。
///
/// **测试 helper（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_make_test_pkg(
    comp: *const u8,
    comp_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    ffi_guard(std::ptr::null_mut(), || {
        use loomgui_core::asset::{write_package, PackageInput, TemplateNode};
        use loomgui_core::scene::NodeKind;
        use loomgui_core::style::resolved::ResolvedStyle;
        if comp.is_null() || out_len.is_null() {
            return std::ptr::null_mut();
        }
        let comp_name =
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(comp, comp_len) }) {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut(),
            };
        if comp_name.is_empty() {
            return std::ptr::null_mut();
        }
        // Two-node component: root container (no id) with a child container id="badge".
        // The child id allows tests to verify subtree-scoped id lookup (find_node_by_id_in_subtree).
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
                id_attr: Some("badge".to_string()),
                draggable: false,
                tabindex: None,
                content: None,
                src: None,
                control_init: None,
                role: None,
                data_slot: None,
                aria_controls: None,
                rich_text_block: false,
                custom_tag: None,
                component_scope: false,
            },
        ];
        let rules = loomgui_core::style::dynamic::DynamicRuleTable::default();
        let pkg = write_package(&PackageInput {
            components: vec![(comp_name, nodes.as_slice(), &rules, &[])],
        });
        let len = pkg.len();
        let ptr = pkg.as_ptr() as *mut u8;
        std::mem::forget(pkg);
        unsafe {
            *out_len = len;
        }
        ptr
    })
}

/// 取走缺字诊断报告（tofu 取证）：shaping 全链（主字体+回退）缺字记录，每行一条
/// （family + 字符 + 码位 + 修法）。返回堆分配 UTF-8 buffer（含尾部 NUL），调用方用
/// `loomgui_bytes_free` 释放；无新记录 → null（*out_len=0）。会话级去重（同 family+char
/// 只报一次），pending 累积不丢。宿主每帧 tick 后调。
#[no_mangle]
pub extern "C" fn loomgui_stage_take_missing_glyphs(
    h: *mut StageHandle,
    out_len: *mut usize,
) -> *mut u8 {
    ffi_guard(std::ptr::null_mut(), || {
        if h.is_null() || out_len.is_null() {
            return std::ptr::null_mut();
        }
        let sh = unsafe { &mut *h };
        let reports = sh.stage.take_missing_glyph_reports();
        if reports.is_empty() {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        }
        let mut s = reports.join("\n");
        s.push('\0');
        let bytes = s.into_bytes();
        unsafe { *out_len = bytes.len() };
        let ptr = bytes.as_ptr() as *mut u8;
        std::mem::forget(bytes);
        ptr
    })
}

/// 释放 loomgui_make_test_pkg 返回的 bytes。
#[no_mangle]
pub extern "C" fn loomgui_bytes_free(ptr: *mut u8, len: usize) {
    ffi_guard((), || {
        if !ptr.is_null() {
            unsafe {
                drop(Vec::from_raw_parts(ptr, len, len));
            }
        }
    })
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
pub extern "C" fn loomgui_stage_remove_touch_monitor(h: *mut StageHandle, node_id: u32) {
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
pub extern "C" fn loomgui_stage_cancel_click(h: *mut StageHandle, touch_id: i32) {
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
pub extern "C" fn loomgui_stage_set_key_input(
    h: *mut StageHandle,
    keys: *const KeyEvent,
    len: usize,
) {
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
    ffi_guard((), || {
        loomgui_core::scene::control::register_clipboard(set_fn, get_fn);
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
    ffi_guard((), || {
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
    })
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
    node_id: u32,
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
pub extern "C" fn loomgui_stage_clear_content_size_override(h: *mut StageHandle, node_id: u32) {
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
    node_id: u32,
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
    ffi_guard((), || {
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
    })
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
    ffi_guard((), || {
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
    })
}

/// 读节点 sort_key（merge 前快照，DFS 序号）。null/无效 → 写 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_sort_key(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u32,
) {
    ffi_guard((), || {
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
    })
}

/// 读节点可见性（存在 + 非 display:none）。null/无效 → 写 0（false）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_visible(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
) {
    ffi_guard((), || {
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
    ffi_guard(-1, || {
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
    })
}

/// 读节点 computed style 快照。return code：0 = ok 且 `*out` 填好；非 0 = 失败（节点不存在
/// 或 `out` = null）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_computed_style(
    h: *const StageHandle,
    node_id: u32,
    out: *mut ComputedNodeStyleRepr,
) -> i32 {
    ffi_guard(-1, || {
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
    })
}

// ===== font atlas FFI（v1.6 自绘字体 pull 模型） =====

/// 拉脏页 page_idx 列表（写入 out，返实际数）。null 句柄 / null out → 返 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_dirty_pages(
    h: *const StageHandle,
    out: *mut u32,
    max: usize,
) -> usize {
    ffi_guard(usize::MAX, || {
        if h.is_null() || out.is_null() {
            return 0;
        }
        let sh = unsafe { &*h };
        let buf = unsafe { std::slice::from_raw_parts_mut(out, max) };
        sh.stage.font_atlas_dirty_pages(buf)
    })
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
    ffi_guard(usize::MAX, || {
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
    })
}

/// 清脏页（backend 拉完后调）。null 句柄 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_clear_dirty(h: *mut StageHandle) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.font_atlas_clear_dirty();
    })
}

/// 设渲染复用键（虚拟列表 slot）。null 句柄/无效 node → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_reuse_key(h: *mut StageHandle, node_id: u32, key: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let handle = unsafe { &mut *h };
        handle.stage.set_reuse_key(NodeId(node_id), key);
    })
}

/// 克隆场景内子树（游离根，不挂树）。返回新 node_id；0xFFFF_FFFF = err / null 句柄 / 无效 src。
#[no_mangle]
pub extern "C" fn loomgui_stage_clone_subtree(h: *mut StageHandle, src: u32) -> u32 {
    ffi_guard(u32::MAX, || {
        const ERR: u32 = 0xFFFF_FFFF;
        if h.is_null() {
            return ERR;
        }
        let sh = unsafe { &mut *h };
        match sh.stage.clone_subtree(NodeId(src)) {
            Ok(id) => id.0,
            Err(_) => ERR,
        }
    })
}

/// 编程聚焦节点（照 fgui RequestFocus）。强制聚焦任意非 disabled 节点
/// （含 tabindex=None/-1）；disabled 拒；越界跳过。null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_request_focus(h: *mut StageHandle, node_id: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.request_focus(NodeId(node_id));
    })
}

/// 读当前焦点节点。无焦点/无 scene → 0xFFFF_FFFF（sentinel，同 node_parent）。null 句柄 → sentinel。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_focused_node(h: *const StageHandle) -> u32 {
    ffi_guard(u32::MAX, || {
        const NONE: u32 = 0xFFFF_FFFF;
        if h.is_null() {
            return NONE;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => scene.focused_node.map(|n| n.0 as u32).unwrap_or(NONE),
            None => NONE,
        }
    })
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
    ffi_guard((), || {
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
        let mut s = [0.0f32; 5];
        let mut e = [0.0f32; 5];
        s[..sz].copy_from_slice(st);
        e[..sz].copy_from_slice(en);
        sh.stage
            .tween(NodeId(node_id), prop, s, e, ease, delay, duration, tag);
    })
}

/// 停该节点该 prop 的 tween（override 保留末值）。
#[no_mangle]
pub extern "C" fn loomgui_stage_kill_tween(h: *mut StageHandle, node_id: u32, prop: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if let Some(prop) = loomgui_core::tween::TweenProp::try_from(prop) {
            sh.stage.kill_tween(NodeId(node_id), prop);
        }
    })
}

/// 清该节点所有动画 override（回 CSS）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_anim(h: *mut StageHandle, node_id: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.clear_anim(NodeId(node_id));
    })
}

/// 清该节点某 prop 对应通道（回 CSS）。
#[no_mangle]
pub extern "C" fn loomgui_stage_clear_anim_prop(h: *mut StageHandle, node_id: u32, prop: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        if let Some(prop) = loomgui_core::tween::TweenProp::try_from(prop) {
            sh.stage.clear_anim_prop(NodeId(node_id), prop);
        }
    })
}

// ===== @keyframes player FFI（M2 spec §7.3：play/pause/resume/stop/time/state/on-key） =====
//
// C# `node.Play(name)` → Animation 句柄（T11 投影）。PlayerKey 以 u64 跨 FFI
// （slotmap `KeyData::as_ffi`，player_key_as_u64/from_u64 转换，0 = 恒无效 key）。
// 句柄控制直接操作 scene.players（既有 `loomgui_node_parent` 同款 scene 直取模式，
// 控制语义在 core 的 update_all/PlayerPlayState 层，FFI 保持薄包装）。

/// 程序化启动 @keyframes 动画（spec §7.3 `play_animation`）。
/// node = 目标节点 NodeId；name = UTF-8 字节（指针+len）。返 PlayerKey u64；失败返 0
/// （null 句柄 / 非 UTF-8 / 无 scene / 节点无效 / keyframes 表无此 name）。
///
/// 建 **programmatic** player（sync_animation_players 完全跳过，不受 class 声明管）：
/// spec 默认 = 1s / 无 delay / 单次迭代 / normal / fill both / cubic-out
/// （C# `Play(name)` 无时长参数，默认由 core `play_programmatic` 定，T13 测试钉死）。
/// 立即写首帧（spec §5.2：不等下帧 step b，防 delay 期闪 base）。
#[no_mangle]
pub extern "C" fn loomgui_stage_play_animation(
    h: *mut StageHandle,
    node: u32,
    name: *const u8,
    name_len: usize,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const INVALID: u64 = 0;
        if h.is_null() {
            return INVALID;
        }
        let sh = unsafe { &mut *h };
        // null/零长兜底为空串（from_raw_parts(null, 0) 是 UB）：空 name 查表失败 → INVALID。
        let name = if name.is_null() || name_len == 0 {
            ""
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }) {
                Ok(s) => s,
                Err(_) => return INVALID,
            }
        };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return INVALID;
        };
        match loomgui_core::scene::animation::play_programmatic(scene, NodeId(node), name) {
            Some(k) => player_key_as_u64(k),
            None => INVALID,
        }
    })
}

/// 同 `loomgui_stage_play_animation`，显式指定时长（秒）。duration_s ≤ 0 / NaN 按 1s
/// 默认。C# `Play(name, durationSeconds)` 重载走此入口——无 `animation:` 声明绑定的
/// keyframes 无声明层时长，程序化播放节奏由调用方给。
#[no_mangle]
pub extern "C" fn loomgui_stage_play_animation_dur(
    h: *mut StageHandle,
    node: u32,
    name: *const u8,
    name_len: usize,
    duration_s: f32,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const INVALID: u64 = 0;
        if h.is_null() {
            return INVALID;
        }
        let sh = unsafe { &mut *h };
        let name = if name.is_null() || name_len == 0 {
            ""
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }) {
                Ok(s) => s,
                Err(_) => return INVALID,
            }
        };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return INVALID;
        };
        match loomgui_core::scene::animation::play_programmatic_with_duration(
            scene,
            NodeId(node),
            name,
            duration_s,
        ) {
            Some(k) => player_key_as_u64(k),
            None => INVALID,
        }
    })
}

/// 暂停 player（Playing → Paused，elapsed 冻结位置保持）。key 无效 / 非 Playing → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_pause_animation(h: *mut StageHandle, key: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            if p.play_state == PlayerPlayState::Playing {
                p.play_state = PlayerPlayState::Paused;
            }
        }
    })
}

/// 恢复播放（Paused → Playing）。key 无效 / 非 Paused → no-op
/// （Completed 是粘性完成态、Stopped 是终态，均不可恢复）。
#[no_mangle]
pub extern "C" fn loomgui_stage_resume_animation(h: *mut StageHandle, key: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            if p.play_state == PlayerPlayState::Paused {
                p.play_state = PlayerPlayState::Playing;
            }
        }
    })
}

/// 停止 player（T6 review Minor 1 钉死：scene 层**终态**，不可恢复，勿当暂停）。
/// 只标记 Stopped：下帧 update_all 清本 player 通道 + 从 players 表移除，PlayerKey 失效。
/// 此后 get_animation_state 恒 255（无效）。key 无效 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_stop_animation(h: *mut StageHandle, key: u64) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            p.play_state = PlayerPlayState::Stopped;
        }
    })
}

/// 读 player 时间轴位置（elapsed——含 delay 计时的唯一时间源头，spec §5.3）。
/// key 无效 / 无 scene → 0.0（不 panic）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_animation_time(h: *const StageHandle, key: u64) -> f32 {
    ffi_guard(f32::NAN, || {
        if h.is_null() {
            return 0.0;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => scene
                .players
                .get(player_key_from_u64(key))
                .map(|p| p.elapsed)
                .unwrap_or(0.0),
            None => 0.0,
        }
    })
}

/// seek：设 player.elapsed，下一帧 step b 按新位置采样（C# `Animation.Time` setter）。
/// 时间源头单一是 elapsed，不校验范围（负值 = 仍在 delay 阶段之前）。key 无效 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_animation_time(h: *mut StageHandle, key: u64, time: f32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        if let Some(p) = scene.players.get_mut(player_key_from_u64(key)) {
            p.elapsed = time;
        }
    })
}

/// 读 player 运行状态。Playing=0 / Paused=1 / Completed=2；Invalid=255（key 不存在 /
/// 无 scene / Stopped——Stopped 是终态，下帧即回收，语义等同无效）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_animation_state(h: *const StageHandle, key: u64) -> u8 {
    ffi_guard(u8::MAX, || {
        const INVALID: u8 = 255;
        if h.is_null() {
            return INVALID;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => match scene.players.get(player_key_from_u64(key)) {
                Some(p) => match p.play_state {
                    PlayerPlayState::Playing => 0,
                    PlayerPlayState::Paused => 1,
                    PlayerPlayState::Completed => 2,
                    PlayerPlayState::Stopped => INVALID,
                },
                None => INVALID,
            },
            None => INVALID,
        }
    })
}

/// 注册 OnKey 百分比阈值（spec §7.3 `animation_on_key`；C# `Animation.OnKey(pct, cb)` 走此 FFI，
/// 回调本身留 C# 按 playerKey 匹配触发）。pct 应 ∈ [0,1]（progress 域外永不触发，注册无害）。
/// 重复注册同 pct 去重（register_on_key）。key 无效 → no-op。
#[no_mangle]
pub extern "C" fn loomgui_stage_animation_on_key(h: *mut StageHandle, key: u64, pct: f32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return;
        };
        register_on_key(scene, player_key_from_u64(key), pct);
    })
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
    ffi_guard(u32::MAX, || {
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
    })
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
    ffi_guard(u32::MAX, || {
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
    })
}

/// 挂子到 parent 末尾。child 必须当前无父。0=ok，-1=err。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_append_child(h: *mut StageHandle, parent: u32, child: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage
            .append_child(NodeId(parent), NodeId(child))
            .map(|_| 0)
            .unwrap_or(-1)
    })
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
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage
            .insert_before(NodeId(parent), NodeId(child), NodeId(ref_id))
            .map(|_| 0)
            .unwrap_or(-1)
    })
}

/// 摘子（不删节点）：从 parent.children 移除 + child.parent=None。节点仍 live 可重挂。
/// 0=ok，-1=err。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_remove_child(h: *mut StageHandle, parent: u32, child: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage
            .remove_child(NodeId(parent), NodeId(child))
            .map(|_| 0)
            .unwrap_or(-1)
    })
}

/// 删节点（递归删子 + 联动清 anim/scroll/tween + slotmap remove）。
/// 该 NodeId 句柄此后失效（gen++）。无 scene / 越界 → no-op。返 0（恒成功，no-op 语义）。
/// null 句柄 → 0（no-op，不 panic）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_remove_node(h: *mut StageHandle, node: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return 0;
        }
        let sh = unsafe { &mut *h };
        sh.stage.remove_node(NodeId(node));
        0
    })
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
    ffi_guard(-1, || {
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
    })
}

/// 重启子树内声明式动画（class 触发 keyframes；programmatic node.Play player 不动）。
/// 下帧 sync 依声明重建 player（delay 重计、backwards/both 立即写首帧）。
/// 0=ok，-1=err（null 句柄 / node 不 live / 无 scene）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_restart_animations(h: *mut StageHandle, node_id: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        match sh.stage.restart_animations(NodeId(node_id)) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
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
    ffi_guard(-1, || {
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
    })
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
    ffi_guard(-1, || {
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
    })
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
    ffi_guard(-1, || {
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
    })
}

/// 读节点子节点数。返回 i32：≥0 = 子节点数；-1 = err（null 句柄 / 节点不 live）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_child_count(h: *const StageHandle, node: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        match sh.stage.get_child_count(NodeId(node)) {
            Some(c) => c as i32,
            None => -1,
        }
    })
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
    ffi_guard(-1, || {
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
    })
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
    ffi_guard(-1, || {
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
    })
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
    ffi_guard(-1, || {
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
    })
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
    ffi_guard(-1, || {
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
    })
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
    })
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
    ffi_guard(-1, || {
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
    })
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
    })
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
    ffi_guard(-1, || {
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
    })
}

/// 设控件 max（ProgressBar / Slider / NumberField）。null 句柄 / 非值控件 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_max(
    h: *mut StageHandle,
    node_id: u32,
    max: f32,
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
            ControlState::NumberField {
                edit, min, step, ..
            } => {
                let max = max.max(min);
                let mut edit = edit;
                renumber_edit_value(&mut edit, min, max, step);
                ControlState::NumberField {
                    edit,
                    min,
                    max,
                    step,
                }
            }
            _ => return -1,
        };
        scene.controls.ensure(id, new_state);
        0
    })
}

/// 读控件 max（ProgressBar / Slider / NumberField）。非值控件 / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_max(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
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
                ControlState::Progress { max, .. }
                | ControlState::Slider { max, .. }
                | ControlState::NumberField { max, .. },
            ) => {
                unsafe { *out = *max };
                0
            }
            _ => -1,
        }
    })
}

/// 设控件 min（Slider / NumberField；ProgressBar 无 min 语义 → -1）。
/// null 句柄 / 节点缺失 → -1。改 min 后 value 重新 clamp。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_min(
    h: *mut StageHandle,
    node_id: u32,
    min: f32,
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
            ControlState::NumberField {
                edit, max, step, ..
            } => {
                let min = min.min(max);
                let mut edit = edit;
                renumber_edit_value(&mut edit, min, max, step);
                ControlState::NumberField {
                    edit,
                    min,
                    max,
                    step,
                }
            }
            _ => return -1,
        };
        scene.controls.ensure(id, new_state);
        0
    })
}

/// 读控件 min（Slider / NumberField）。非数值控件 / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_min(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
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
            Some(ControlState::Slider { min, .. } | ControlState::NumberField { min, .. }) => {
                unsafe { *out = *min };
                0
            }
            _ => -1,
        }
    })
}

/// 设控件 step（Slider / NumberField；ProgressBar 无 step 语义 → -1）。
/// null 句柄 / 节点缺失 → -1。改 step 不重量化 value（对齐 Slider arm：量化只在
/// set value 时发生，改步长只影响后续写入）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_step(
    h: *mut StageHandle,
    node_id: u32,
    step: f32,
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
            ControlState::NumberField { edit, min, max, .. } => {
                // 同 Slider：负 / NaN 拒绝（set_number_value 的量化分支同样假设 step>0）。
                if !step.is_finite() || step < 0.0 {
                    return -1;
                }
                ControlState::NumberField {
                    edit,
                    min,
                    max,
                    step,
                }
            }
            _ => return -1,
        };
        scene.controls.ensure(id, new_state);
        0
    })
}

/// NumberField value 文本按 [min,max] 重约束：parse → clamp → step 量化 → re-format 写回
/// （与 set_number_value 同口径，value 在 NumberField 存文本）。改界后 value 可能越出新区间，
/// 必须重写文本保持一致；文本非数值（用户手输中）时不动 value——界只约束后续写入。
/// value 长度变化后 cursor/anchor 收缩到新 len（字节偏移不可越界）。
fn renumber_edit_value(edit: &mut EditState, min: f32, max: f32, step: f32) {
    let Ok(v) = edit.value.parse::<f32>() else {
        return;
    };
    // clamp：min>max 时 swap，保 clamp 闭区间不 panic（同 set_number_value 纵深守卫）。
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    let quantized = if step > 0.0 {
        ((v.clamp(lo, hi) - lo) / step).round() * step + lo
    } else {
        v.clamp(lo, hi)
    };
    edit.value = format_number(quantized.clamp(lo, hi));
    edit.cursor = edit.cursor.min(edit.value.len());
    edit.anchor = edit.anchor.min(edit.value.len());
}

/// 读控件 step（Slider / NumberField）。非数值控件 / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_step(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
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
            Some(ControlState::Slider { step, .. } | ControlState::NumberField { step, .. }) => {
                unsafe { *out = *step };
                0
            }
            _ => -1,
        }
    })
}

/// 读 ProgressBar indeterminate（不确定进度态）。非 Progress / null out / 节点缺失 → -1。
/// 纯状态位（视觉由作者 CSS 表达，core 不做 marquee 渲染）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_indeterminate(
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
            Some(ControlState::Progress { indeterminate, .. }) => {
                unsafe { *out = u8::from(*indeterminate) };
                0
            }
            _ => -1,
        }
    })
}

/// 设 ProgressBar indeterminate。写状态位（value/max 不动——不确定态下 value 语义由
/// caller 自定，CSS 视觉切换走作者选择器）。非 Progress / null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_control_indeterminate(
    h: *mut StageHandle,
    node_id: u32,
    v: u8,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        // 原地改（get_mut），不重建 ControlState（保 variant 口径，同 set_number_value）。
        match scene.controls.get_mut(NodeId(node_id)) {
            Some(ControlState::Progress { indeterminate, .. }) => {
                *indeterminate = v != 0;
                0
            }
            _ => -1,
        }
    })
}

/// 读 RadioButton 分组名（HTML name 语义：同名组互斥，打包期从 data-name bake）。
/// return-code + out-param（ptr+len）双调法，同 get_control_text：buf_cap 足够 → rc=0；
/// 不够 → rc=-2 + *out_len=所需（caller 扩容重调）；非 Radio / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_radio_name(
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
        let name = match scene.controls.get(NodeId(node_id)) {
            Some(ControlState::Radio { name, .. }) => name.as_bytes(),
            _ => return -1,
        };
        let needed = name.len();
        unsafe { *out_len = needed };
        if needed > buf_cap {
            return -2;
        }
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(name.as_ptr(), out, needed);
            }
        }
        0
    })
}

/// 读 Dropdown 当前选中项的 value（`value` 属性优先，缺席回落该项文本——HTML 语义）。
/// return-code + out-param（ptr+len）双调法，同 get_radio_name：buf_cap 足够 → rc=0；
/// 不够 → rc=-2 + *out_len=所需；非 Dropdown / null 句柄 → -1；无选项（value 为 null
/// 语义）→ rc=1（*out_len=0，不写 buf）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_dropdown_selected_value(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        let value = match read_control_string(h, node_id, out, buf_cap, out_len, |scene, id| {
            loomgui_core::scene::control::dropdown_selected_value(scene, id)
        }) {
            Ok(v) => v,
            Err(rc) => return rc,
        };
        match value {
            Some(v) => write_out_string(&v, out, buf_cap, out_len),
            None => {
                unsafe { *out_len = 0 };
                1
            }
        }
    })
}

/// 读单个 option 的 value（同 dropdown_selected_value 的 fallback 语义，按 option
/// 自身序号取）。双调法同上；非 option / 上溯无 Dropdown / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_option_value(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        let value = match read_control_string(h, node_id, out, buf_cap, out_len, |scene, id| {
            loomgui_core::scene::control::option_value(scene, id)
        }) {
            Ok(v) => v,
            Err(rc) => return rc,
        };
        match value {
            Some(v) => write_out_string(&v, out, buf_cap, out_len),
            None => -1,
        }
    })
}

/// option 是否为所属 Dropdown 的当前选中项（合成：序号 == 父 selected_index）。
/// 1=选中，0=未选中，-1=非 option / 上溯无 Dropdown / null 句柄。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_is_option_selected(h: *const StageHandle, node_id: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match loomgui_core::scene::control::option_selected(scene, NodeId(node_id)) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    })
}

/// tab 是否为所属 TabList 的当前激活项（合成：序号 == 父 selected_index，与
/// aria-selected 派生同源）。1=激活，0=未激活，-1=非 tab / 上溯无 TabList / null 句柄。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_is_tab_selected(h: *const StageHandle, node_id: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match loomgui_core::scene::control::tab_selected(scene, NodeId(node_id)) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    })
}

/// 控件派生字符串读的公共骨架：校验句柄 + 取 scene + 跑派生闭包。
/// Ok(Some(v)) = 有值（由调用方写 buf）；Ok(None) = 语义空值；Err(rc) = 直接返回的错码。
fn read_control_string(
    h: *const StageHandle,
    node_id: u32,
    _out: *mut u8,
    _buf_cap: usize,
    out_len: *mut usize,
    f: impl Fn(&loomgui_core::scene::Scene, NodeId) -> Option<String>,
) -> Result<Option<String>, i32> {
    if h.is_null() || out_len.is_null() {
        return Err(-1);
    }
    let sh = unsafe { &*h };
    let Some(scene) = sh.stage.scene.as_ref() else {
        return Err(-1);
    };
    Ok(f(scene, NodeId(node_id)))
}

/// 把派生字符串写进 out（ptr+len 双调法收尾）：够 → rc=0；不够 → rc=-2 + *out_len=所需。
fn write_out_string(v: &str, out: *mut u8, buf_cap: usize, out_len: *mut usize) -> i32 {
    let bytes = v.as_bytes();
    let needed = bytes.len();
    unsafe { *out_len = needed };
    if needed > buf_cap {
        return -2;
    }
    if needed > 0 {
        if out.is_null() {
            return -2;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, needed);
        }
    }
    0
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
            // 直接替换 value + 光标/anchor 移到末尾（编程 setter，不走 insert_text 路径）。
            // readonly 不拦（编程可写，照 JS .value = ... 语义）。同值仍重置光标但不发事件。
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

/// 设节点 user transform（位移/缩放/旋转/原点）。走 `set_user_transform`（dynamic.rs）：
/// 只写 `node.user_transform`，不触发 layout solve——`compute_world_transforms` 在
/// 世界矩阵累计时并入（渲染/命中层，同 CSS transform）。供高频拖拽等运行时定位用。
/// `ox/oy` = 旋转/缩放原点（local 坐标 px），连接 C# `NodeTransform.Origin`。
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
    ox: f32,
    oy: f32,
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
        let t = NodeTransform {
            translate: [tx, ty],
            scale: [sx, sy],
            rotation: rot,
            origin: [ox, oy],
        };
        match dynamic::set_user_transform(scene, NodeId(node_id), t) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

// ===== get_node_disabled / get_control_readonly / blur / Dropdown / NumberField FFI =====

/// 读节点 disabled 伪类态（`NodeFlags::DISABLED`）。null 句柄 / 无 scene / 节点缺失 → 写 0（false）。
/// 与 `loomgui_stage_set_node_disabled` 对称的读出口（伪类态级联查询用）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_disabled(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
) {
    ffi_guard((), || {
        let disabled = if h.is_null() {
            false
        } else {
            let sh = unsafe { &*h };
            sh.stage.get_node_disabled(NodeId(node_id))
        };
        if !out.is_null() {
            unsafe {
                *out = if disabled { 1 } else { 0 };
            }
        }
    })
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

/// 清除当前 focus（`Stage::blur` 的 FFI 包装）：记 pending_focus_request = Some(None)，
/// 下 tick 消费清焦点（与 `request_focus` 对称）。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_blur(h: *mut StageHandle) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage.blur();
        0
    })
}

/// 读 Dropdown 当前选中项索引（`ControlState::Dropdown.selected_index`）。
/// 非 Dropdown / null 句柄 / 节点缺失 / null out → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_dropdown_selected_index(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u32,
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
            Some(ControlState::Dropdown { selected_index, .. }) => {
                unsafe { *out = *selected_index as u32 };
                0
            }
            _ => -1,
        }
    })
}

/// 设 Dropdown 选中项。置 `value_lock=true` 防本轮 cascade 回写（popup option 子项的
/// selected 类规则在 rematch 阶段读 value_lock 跳过回写）。事件发射（EVT_SELECTION_CHANGED）
/// 在 tick，非此处——照 ValueChanged 模式。非 Dropdown / null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_dropdown_selected_index(
    h: *mut StageHandle,
    node_id: u32,
    index: u32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        if let Some(ControlState::Dropdown {
            selected_index,
            value_lock,
            ..
        }) = scene.controls.get_mut(NodeId(node_id))
        {
            *selected_index = index as usize;
            *value_lock = true;
            0
        } else {
            -1
        }
    })
}

/// 读 TabList 当前选中项索引（`ControlState::TabList.selected_index`）。
/// 非 TabList / null 句柄 / 节点缺失 / null out → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_tablist_selected_index(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u32,
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
            Some(ControlState::TabList { selected_index }) => {
                unsafe { *out = *selected_index as u32 };
                0
            }
            _ => -1,
        }
    })
}

/// 设 TabList 选中项。TabList 无 `value_lock`（aria-selected 是只读合成属性，无 cascade
/// 回写环，与 Dropdown 不同）。事件发射（EVT_SELECTION_CHANGED）在 tick（on_pointer_down/键盘），
/// 非此处——本 setter 仅 host 驱动的程序化改态。非 TabList / null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_tablist_selected_index(
    h: *mut StageHandle,
    node_id: u32,
    index: u32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        if let Some(ControlState::TabList { selected_index }) =
            scene.controls.get_mut(NodeId(node_id))
        {
            *selected_index = index as usize;
            0
        } else {
            -1
        }
    })
}

/// 读 Dropdown popup 是否展开（`ControlState::Dropdown.open`）。
/// 非 Dropdown / null 句柄 / 节点缺失 / null out → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_dropdown_open(
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
            Some(ControlState::Dropdown { open, .. }) => {
                unsafe { *out = if *open { 1 } else { 0 } };
                0
            }
            _ => -1,
        }
    })
}

/// 设 Dropdown popup 展开态。非 Dropdown / null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_dropdown_open(
    h: *mut StageHandle,
    node_id: u32,
    open: u8,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        if let Some(ControlState::Dropdown { open: o, .. }) =
            scene.controls.get_mut(NodeId(node_id))
        {
            *o = open != 0;
            0
        } else {
            -1
        }
    })
}

/// 读 NumberField 数值（解析 `EditState.value` 文本→f32）。解析失败 / 非 NumberField /
/// null 句柄 / 节点缺失 / null out → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_get_number_value(
    h: *const StageHandle,
    node_id: u32,
    out: *mut f32,
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
            Some(ControlState::NumberField { edit, .. }) => match edit.value.parse::<f32>() {
                Ok(v) => {
                    unsafe { *out = v };
                    0
                }
                Err(_) => -1,
            },
            _ => -1,
        }
    })
}

/// 设 NumberField 数值：先 clamp[min,max]（纵深守卫 min>max 不 panic），再 step 量化对齐
/// （step>0 时 round((v-min)/step)*step+min，量化后重 clamp 回区间），最后把量化值格式化为
/// 文本写回 `EditState.value`（保持 value 文本与数值约束一致，与 Slider set_control_value
/// 同口径，只是 Slider 存 f32 而 NumberField 存文本）。step<=0 跳过量化。
/// 非 NumberField / null 句柄 / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_number_value(
    h: *mut StageHandle,
    node_id: u32,
    value: f32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        let id = NodeId(node_id);
        let Some(state) = scene.controls.get(id).cloned() else {
            return -1;
        };
        let ControlState::NumberField { min, max, step, .. } = state else {
            return -1;
        };
        // clamp：min>max 时 swap，保 clamp 闭区间不 panic（同 set_control_value 纵深守卫）。
        let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
        let clamped = value.clamp(lo, hi);
        let quantized = if step > 0.0 {
            ((clamped - lo) / step).round() * step + lo
        } else {
            clamped
        };
        // 量化可能把值推过 hi，重新 clamp 回区间。
        let quantized = quantized.clamp(lo, hi);
        // 写回：原地改 edit.value（get_mut 保 variant，不重建整个 NumberField）。
        if let Some(ControlState::NumberField { edit, .. }) = scene.controls.get_mut(id) {
            // 数字文本用 Rust 默认 f32 格式化（如 "8"、"-3.5"）；trimmed 避免尾随 0。
            edit.value = format_number(quantized);
        } else {
            return -1; // 极端竞态：get_mut 返回 None（理论上 cloned 后同槽仍在）
        }
        0
    })
}

/// NumberField 文本格式化：整数去 `.0` 尾，保留小数。避免 EditState.value 出现 "8.0"。
fn format_number(v: f32) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{:.0}", v)
    } else {
        // 非 整数：用通用格式，strip 尾随 0（如 3.50 → "3.5"）。
        let s = format!("{:.6}", v);
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        if trimmed.is_empty() {
            "0".to_string()
        } else {
            trimmed.to_string()
        }
    }
}

// ── ListView 虚拟化（数据驱动）FFI ────────────────────────────────
// C# ListView 投影经本组 FFI 驱动 core 虚拟化内核：set_item_count 进数据驱动，
// take_pending_binds 是关键——C# 每 tick 前调，取新克隆 slot 列表逐条 BindItem。
// null 句柄 / 无效 node → -1；成功 → 0。

/// 设 ListView 的项数。首次调用若该 node 尚未进入数据驱动模式（无 ListState 条目），
/// 自动 enter_data_driven（取备用模板 = 第一个设计期 li、分配全局 list_ordinal）。
/// 这避免 C# 侧需显式调 enter——ItemCount 是业务进入虚拟化的唯一入口。
#[no_mangle]
pub extern "C" fn loomgui_list_set_item_count(h: *mut StageHandle, node: u32, count: i32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let ul = NodeId(node);
        let needs_enter = sh
            .stage
            .scene
            .as_ref()
            .map(|s| s.lists.get(ul).is_none())
            .unwrap_or(true);
        if needs_enter {
            let ordinal = sh.stage.next_list_ordinal;
            sh.stage.next_list_ordinal = sh.stage.next_list_ordinal.wrapping_add(1);
            if loomgui_core::list::enter_data_driven(&mut sh.stage, ul, ordinal).is_err() {
                return -1;
            }
        }
        loomgui_core::list::set_item_count(&mut sh.stage, ul, count.max(0) as usize);
        0
    })
}

/// 设 ListView 的模板根（覆盖 enter_data_driven 备份的备用 li）。业务通过
/// ListView.ItemTemplate 设——指向场景内克隆出的模板子树根。无 ListState 条目 → -1。
#[no_mangle]
pub extern "C" fn loomgui_list_set_template(
    h: *mut StageHandle,
    node: u32,
    template_node: u32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        match scene.lists.get_mut(NodeId(node)) {
            Some(ls) => {
                ls.template_root = Some(NodeId(template_node));
                0
            }
            None => -1,
        }
    })
}

/// 拉取本帧待绑定 slot 列表（SOA）。C# tick 前调：遍历所有 ListView 的 pending_binds，
/// 拍平成 (node_id[], item_index[]) 两列，cap 限 copy 上限。调用方按 out_nodes[i] 的
/// node_id 反查其 ListView 祖先实例调 BindItem。cap 不足时不丢 bind——只取装得下的部分，
/// 余条留在各 ListView 队列里等下一帧再取（走 `drain_pending_binds_bounded` 而非全取）。
/// 任一指针 null → -1；out_len 写实际返回条数。各参数 null 句柄 guard 在最前。
#[no_mangle]
pub extern "C" fn loomgui_list_take_pending_binds(
    h: *mut StageHandle,
    out_nodes: *mut u32,
    out_indices: *mut i32,
    cap: u32,
    out_len: *mut u32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out_nodes.is_null() || out_indices.is_null() || out_len.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        // 快照所有 ListView 的 NodeId（避免在借 scene.lists 时 mutable 借 take_pending_binds）。
        let uls: Vec<NodeId> = sh
            .stage
            .scene
            .as_ref()
            .map(|s| s.lists.0.keys().copied().collect())
            .unwrap_or_default();
        let cap = cap as usize;
        let mut all: Vec<(u32, i32)> = Vec::with_capacity(cap);
        let Some(scene) = sh.stage.scene.as_mut() else {
            // 无 scene：out_len 仍写 0（调用方按 0 处理）。
            unsafe {
                *out_len = 0;
            }
            return 0;
        };
        for ul in uls {
            if all.len() >= cap {
                break;
            }
            // 只取当前剩余容量内的 bind——余条留队列等下帧，避免 cap 溢出时丢 bind。
            let binds = loomgui_core::list::drain_pending_binds_bounded(scene, ul, cap - all.len());
            for (n, idx) in binds {
                all.push((n.0, idx as i32));
            }
        }
        let n = all.len();
        unsafe {
            for (i, (node, idx)) in all.iter().take(n).enumerate() {
                *out_nodes.add(i) = *node;
                *out_indices.add(i) = *idx;
            }
            *out_len = n as u32;
        }
        0
    })
}

/// 同帧推进虚拟化管线（plan+execute，不取 binds 队列——C# `DrainPendingBinds` 取）。
/// ScrollToItem / 首次 ItemCount 调用走此路径——让本帧滚动后新进入可见区的 item 的 slot
/// 同帧克隆、binds 入队等 C# 消费，避免首帧模板原样。null 句柄 → -1；成功 → 0。
#[no_mangle]
pub extern "C" fn loomgui_list_drain_now(h: *mut StageHandle, node: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        loomgui_core::list::drain_now(&mut sh.stage, NodeId(node));
        0
    })
}

/// notify 操作码（与 C# NotifyOp 对齐）。单 FFI 多 op，避免 C# 端三个导入。
const NOTIFY_INSERTED: u8 = 0;
const NOTIFY_REMOVED: u8 = 1;
const NOTIFY_MOVED: u8 = 2;

/// 刷新指定区间当前可见（active）的 slot（重新入 pending_binds，C# 下帧重新 BindItem）。
/// 休眠（parked）slot 不刷——它进可见区时由 execute 的 unpark 路径重新 bind。
/// start/count：负值拒（越界）。0=ok，-1=err（null 句柄 / 非 ListView / 越界）。
#[no_mangle]
pub extern "C" fn loomgui_list_refresh(
    h: *mut StageHandle,
    node: u32,
    start: i32,
    count: i32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || start < 0 || count < 0 {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        match loomgui_core::list::refresh_items(scene, NodeId(node), start as usize, count as usize)
        {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// 增删搬通知（单 FFI 多 op，spec §10）。a/b 语义随 op：
/// - 0=Inserted: a=at, b=count
/// - 1=Removed:  a=at, b=count
/// - 2=Moved:    a=from, b=to
///
/// 返 0=ok，-1=err（null 句柄 / 未知 op / 越界）。
#[no_mangle]
pub extern "C" fn loomgui_list_notify(
    h: *mut StageHandle,
    node: u32,
    op: u8,
    a: i32,
    b: i32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || a < 0 || b < 0 {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let Some(scene) = sh.stage.scene.as_mut() else {
            return -1;
        };
        let ul = NodeId(node);
        let res = match op {
            NOTIFY_INSERTED => {
                loomgui_core::list::notify_inserted(scene, ul, a as usize, b as usize)
            }
            NOTIFY_REMOVED => loomgui_core::list::notify_removed(scene, ul, a as usize, b as usize),
            NOTIFY_MOVED => loomgui_core::list::notify_moved(scene, ul, a as usize, b as usize),
            _ => return -1,
        };
        match res {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// 滚动到指定 item。index 越界 / 负值 → -1。behavior：0=Instant，1=Smooth。
/// 内部 drain_now 让目标 slot 同帧物化；C# 调后需 DrainPendingBinds 把 binds 灌进 BindItem。
#[no_mangle]
pub extern "C" fn loomgui_list_scroll_to(
    h: *mut StageHandle,
    node: u32,
    index: i32,
    behavior: u8,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || index < 0 {
            return -1;
        }
        let sh = unsafe { &mut *h };
        match loomgui_core::list::scroll_to_item(
            &mut sh.stage,
            NodeId(node),
            index as usize,
            behavior,
        ) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// Rich-text-block 子节点命中细化（spec §10）。
///
/// 在 [`loomgui_stage_get_node_layout_rect`] / [`loomgui_stage_is_pointer_on_ui`] 已定出命中
/// 目标是 rich-text-block 容器之后，用本函数把容器内的点细化到源 inline 节点
/// （span / TextNode / Image），供后端 firing span 级点击事件。
///
/// - `node_id`：rich-text-block 容器（须 `rich_text_block=true`，且 solve 已为其填
///   `scene.text_layouts[node_id]`）。
/// - `x`/`y`：相对该容器 border-box 左上的 block-local 点（与 hit_test world_to_local 后
///   的本地坐标同空间）。
/// - `out_source`：命中时写 source inline 节点的 NodeId(u32)；未命中不写。null 安全。
///
/// 返 `true` = 命中（`*out_source` 已写）；`false` = 未命中 / null 句柄 / 无 scene /
/// `node_id` 非 rich-text-block / 无 layout（`*out_source` 未动）。
/// 命中测试（公共 Pick 的后端）：(x,y) 最上层可 touchable 节点。rc=0 命中（out_node 写
/// NodeId u32）；rc=1 未命中；-1 = null 句柄 / 无 scene / null out。坐标 = design 像素
/// （左上原点，同 process 输入）。core hit_test 走上帧 world_transforms（结构变更帧的
/// 新节点本帧未命中，1 帧延迟语义）。scrollbar thumb sentinel id（V/H_THUMB_FLAG 位）
/// decode 回容器 id——公共语义树无 thumb 节点，thumb 命中即容器命中（同
/// apply_wheel_to_hit 口径）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_hit_test(
    h: *const StageHandle,
    x: f32,
    y: f32,
    out_node: *mut u32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out_node.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match loomgui_core::hit::hit_test(scene, (x, y)) {
            Some(id) => {
                // sentinel thumb flag（bit 29/30）strip——见 scroll.rs V/H_THUMB_FLAG。
                unsafe { *out_node = id.0 & !0x6000_0000 };
                0
            }
            None => 1,
        }
    })
}

#[no_mangle]
pub extern "C" fn loomgui_hit_test_rich(
    h: *const StageHandle,
    node_id: u32,
    x: f32,
    y: f32,
    out_source: *mut u32,
) -> bool {
    ffi_guard(false, || {
        if h.is_null() {
            return false;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return false;
        };
        match loomgui_core::text::hit_test::hit_test_rich(scene, NodeId(node_id), (x, y)) {
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod abi_tests;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod test_helpers;
