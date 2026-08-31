//! FFI 导出层（§14.1 csbindgen）：extern "C" 薄包装，opaque Stage 句柄。
//! 命名前缀 `ikat_`。按职责拆成子模块（stage/frame/events/node_getters/
//! node_setters/controls/text/scroll/animation/list/resources），csbindgen 扫描
//! lib.rs + 各含 extern fn 的模块文件生成 C# 绑定（文件清单见 build.rs，与
//! crates/xtask/src/bindings.rs 的清单互为镜像，改清单两处同步）。

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

mod animation;
mod controls;
mod events;
mod frame;
mod host;
mod list;
mod node_getters;
mod node_setters;
mod resources;
mod scroll;
mod stage;
mod text;

// 路径稳定：crate 内测试（use crate::*）与外部消费者经根路径取全部导出面。
pub use animation::*;
pub use controls::*;
pub use events::*;
pub use frame::*;
pub use host::*;
pub use list::*;
pub use node_getters::*;
pub use node_setters::*;
pub use resources::*;
pub use scroll::*;
pub use stage::*;
pub use text::*;

use ikat_core::stage::Stage;
use std::ffi::CString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};

/// FFI panic 兜底计数：guard 捕获的 panic 累计（0 = 从未）。
static FFI_PANIC_COUNT: AtomicU32 = AtomicU32::new(0);

/// 全部导出的统一 panic 边界。panic 展开穿越 `extern "C"` 是 UB（实践中直接
/// abort 宿主进程——本库的宿主是 Unity 编辑器/玩家，不可接受），因此在函数体内
/// 捕获：计数、返回调用方约定的错误哨兵。panic 消息由默认 panic hook 先行打到
/// stderr。取舍：panic 点之后 Stage 可能处于半修改状态，继续运行不保证一致——
/// 但比崩宿主可诊断；后端每帧读 `ikat_ffi_panic_count`，变化即告警。
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
pub extern "C" fn ikat_ffi_panic_count() -> u32 {
    FFI_PANIC_COUNT.load(Ordering::Relaxed)
}

/// 版本字符串（C null-terminated `b"v1e\0"`）。
///
/// 返回 `*const u8`（csbindgen 映射为 C# `byte*`）；CString::as_ptr 给的是
/// `*const c_char`（i8），这里 cast 对齐签名。OnceLock 缓存，避免每次分配+泄漏。
#[no_mangle]
pub extern "C" fn ikat_version() -> *const u8 {
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
    frame_blob: Vec<u8>,    // borrow_frame 返回 &this[..]；tick 时被覆盖。
    dump_blob: CString,     // dump_scene 缓存（Rust 拥有）
    tree_blob: CString,     // dump_tree 缓存（Rust 拥有）
    warnings_blob: CString, // take_warnings 缓存（Rust 拥有）
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod abi_tests;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod test_helpers;
