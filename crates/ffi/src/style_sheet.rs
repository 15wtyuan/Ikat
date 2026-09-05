//! 运行时 CSS 注入与 custom props FFI（#11 StyleSheet 逃生舱 + SetVar/RemoveVar）。
//!
//! Add 的解析在 FFI 层做（fence `parse_runtime_css`——core 不能依赖 fence，依赖方向
//! core ← fence）；解析产物是 core 已有的 `DynamicRule` 结构，经 Stage 注入全局规则
//! 表（rematch 每帧全量 → 下一帧生效）。解析失败走句柄上的 last-error blob（消息 +
//! 行列，C# 抛 UIStyleException），复用 dump/warnings 的「Rust 拥有、调用方拉取」
//! 缓存模式。

use crate::{ffi_guard, StageHandle};
use yio_core::scene::node::NodeId;

/// 注入一段运行时 CSS（UIContext.StyleSheet.Add）。css = UTF-8 字节。
///
/// 返回：0 = ok（\*out_set_id 写入撤销句柄）；1 = 解析失败（yio_stage_style_sheet_last_error
/// 读消息/行列，句柄无效）；-1 = 基础设施错（null 句柄/无 scene/非 UTF-8）。
#[no_mangle]
pub extern "C" fn yio_stage_style_sheet_add(
    h: *mut StageHandle,
    css: *const u8,
    css_len: usize,
    out_set_id: *mut u64,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || css.is_null() || out_set_id.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let css = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(css, css_len) }) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        match yio_fence::css_rules::parse_runtime_css(css) {
            Ok(rules) => match sh.stage.style_sheet_add_rules(rules) {
                Ok(id) => {
                    unsafe { *out_set_id = id };
                    0
                }
                Err(_) => -1,
            },
            Err(diag) => {
                sh.style_err_blob = std::ffi::CString::new(format!(
                    "StyleSheet.Add: {} ({})",
                    diag.message, diag.location.file
                ))
                .unwrap_or_else(|_| std::ffi::CString::new("StyleSheet.Add: parse error").unwrap());
                sh.style_err_line = diag.location.line as u32;
                sh.style_err_col = diag.location.column as u32;
                1
            }
        }
    })
}

/// 撤销一批注入规则（Add 返回句柄的 Dispose）。set_id 无效 / 基础设施错 → -1；ok → 0。
#[no_mangle]
pub extern "C" fn yio_stage_style_sheet_remove(h: *mut StageHandle, set_id: u64) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        match sh.stage.style_sheet_remove(set_id) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// 清空全部运行时注入规则（StyleSheet.Clear；pkg 规则不动）。ok → 0；基础设施错 → -1。
#[no_mangle]
pub extern "C" fn yio_stage_style_sheet_clear(h: *mut StageHandle) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        sh.stage.style_sheet_clear().map(|_| 0).unwrap_or(-1)
    })
}

/// 读最近一次 Add 解析失败的信息（NUL 结尾 UTF-8；无失败记录返空串）。
/// out_line/out_col 可为 null（只取消息）。返回指针由 StageHandle 拥有，下次 Add 失败前有效。
#[no_mangle]
pub extern "C" fn yio_stage_style_sheet_last_error(
    h: *mut StageHandle,
    out_line: *mut u32,
    out_col: *mut u32,
) -> *const u8 {
    ffi_guard(std::ptr::null(), || {
        if h.is_null() {
            return std::ptr::null();
        }
        let sh = unsafe { &mut *h };
        if !out_line.is_null() {
            unsafe { *out_line = sh.style_err_line };
        }
        if !out_col.is_null() {
            unsafe { *out_col = sh.style_err_col };
        }
        sh.style_err_blob.as_ptr() as *const u8
    })
}

/// 运行时 SetVar（#11）。name/value = UTF-8 字节。name 须 `--` 前缀（custom prop 命名域）。
/// 0 = ok；-1 = 基础设施错 / 名字非法 / 节点不 live。
#[no_mangle]
pub extern "C" fn yio_stage_node_set_var(
    h: *mut StageHandle,
    node: u64,
    name: *const u8,
    name_len: usize,
    value: *const u8,
    value_len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || name.is_null() || value.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) })
        {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let value =
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(value, value_len) }) {
                Ok(s) => s,
                Err(_) => return -1,
            };
        sh.stage
            .node_set_var(NodeId(node), name, value)
            .map(|_| 0)
            .unwrap_or(-1)
    })
}

/// 运行时 RemoveVar（#11，撤销 SetVar 条目回落 CSS 声明值）。name = UTF-8 字节。
/// 0 = ok（含未设过 no-op）；-1 = 基础设施错 / 节点不 live。
#[no_mangle]
pub extern "C" fn yio_stage_node_remove_var(
    h: *mut StageHandle,
    node: u64,
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
        sh.stage
            .node_remove_var(NodeId(node), name)
            .map(|_| 0)
            .unwrap_or(-1)
    })
}
