//! Stage 生命周期与全局面：创建/释放、root_size、分辨率适配数学、字体注册与回退链、
//! 包装载/卸载/实例化、全局 shutdown。

use std::ffi::CString;

use loomgui_core::stage::Stage;

use crate::{ffi_guard, StageHandle};

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
            warnings_blob: CString::new("").unwrap(),
        }))
    })
}

/// 改画布尺寸（分辨率适配 / 窗口 resize / 横竖屏切换）。solve 每帧跑，改完下帧
/// 布局即按新 root_size 重排（vw/vh/% 跟随）。返回 0=成功，-1=错误（null 句柄 /
/// 非有限 / ≤0，失败时保持原值不动）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_stage_set_root_size(h: *mut StageHandle, w: f32, hgt: f32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        match sh.stage.set_root_size(w, hgt) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// 分辨率适配数学（纯函数，无句柄——引擎集成层每帧/屏幕变化时调）。
/// mode: 0=letterbox（contain 黑边）/ 1=fit-width（宽锚重排）/ 2=fit-height（高锚重排）。
/// 结果（#[repr(C)] AdaptResult：scale/root_w/root_h/offset_x/offset_y，5×f32）
/// 写入 out 指向的缓冲。返回 0=成功，-1=错误（null out / 未知 mode）。
/// safe 传 (0,0,0,0) 或零宽高矩形 = 全屏（编辑器防御）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn loomgui_compute_adaptation(
    design_w: f32,
    design_h: f32,
    screen_w: f32,
    screen_h: f32,
    safe_x: f32,
    safe_y: f32,
    safe_w: f32,
    safe_h: f32,
    mode: u32,
    out: *mut loomgui_core::adapt::AdaptResult,
) -> i32 {
    ffi_guard(-1, || {
        if out.is_null() {
            return -1;
        }
        let Some(mode) = loomgui_core::adapt::AdaptMode::from_u32(mode) else {
            return -1;
        };
        let r = loomgui_core::adapt::compute(
            (design_w, design_h),
            (screen_w, screen_h),
            (safe_x, safe_y, safe_w, safe_h),
            mode,
        );
        unsafe { *out = r };
        0
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

/// 从包克隆一个组件进当前 scene，返组件根 NodeId（u64）。
/// pkg/comp = UTF-8 字节（指针+len）。失败返 u64::MAX（INVALID，同 create_root 失败语义）。
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
) -> u64 {
    ffi_guard(u64::MAX, || {
        const INVALID: u64 = u64::MAX;
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

/// 全局 shutdown（Domain reload hook）。C# `LoomStage.ResetStatics`（SubsystemRegistration）调用。
///
/// 当前核心无全局 native 态——Stage 是 per-handle（`loomgui_stage_free` drop 全部 Stage 拥有的内存），
/// 故本函数 near-no-op。但 hook 必须存在：将来引入全局 texture/font registry（进程级单例缓存）时，
/// 此处自动成为清理入口，无需再改 C# 接线。
///
/// **注意：Font 的 `Box::leak`（`text/layout.rs`）是真泄漏**——`bytes.clone()` 后 leak 取
/// `'static` 切片喂 ttf-parser Face，原 Vec 虽被 `_bytes` 持有但与 leaked 切片不是同一份，
/// Stage drop 时 `_bytes` 释放的是 clone 来源而非 leaked 副本。每次 Stage 创建都 leak 一份字体字节，
/// 不可由 shutdown 回收（leak 切片无 handle 跟踪）。若未来域重载内存观测触发阈值，
/// 再考虑字体缓存化为进程单例。
#[no_mangle]
pub extern "C" fn loomgui_shutdown() {}
