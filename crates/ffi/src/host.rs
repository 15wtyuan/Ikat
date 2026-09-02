//! 资源宿主生命周期：host 句柄创建/释放 + 宿主级资源操作（字体注册/回退链/包/
//! 图尺寸/glyph atlas 拉取）。Stage 侧同名入口保留且行为等价（自建宿主的 Stage
//! 在自己宿主上操作；`ikat_stage_new_bound` 挂接的 Stage 在共享宿主上操作——
//! 即 stage 级入口在多 Stage 下是「宿主操作的路由糖」）。

use std::cell::RefCell;
use std::rc::Rc;

use ikat_core::host::ResourceHost;
use ikat_core::stage::{LoadPkgError, Stage};

use crate::{ffi_guard, StageHandle};

/// opaque 宿主句柄：`Rc<RefCell<ResourceHost>>`。`ikat_stage_new_bound` 克隆 Rc
/// 挂进 Stage；宿主释放须在所有挂接 Stage 释放之后（顺序由 C# 侧保证，越序 =
/// Stage 悬垂 Rc 仍安全——Rc 引用计数使 host_free 后 Stage 自持的克隆仍有效，
/// 真正释放发生在最后一个引用 drop）。
pub struct HostHandle {
    host: Rc<RefCell<ResourceHost>>,
}

/// 创建宿主句柄。字体/atlas/包/图尺寸先于此注册，再 `ikat_stage_new_bound` 挂 Stage。
#[no_mangle]
pub extern "C" fn ikat_host_new() -> *mut HostHandle {
    ffi_guard(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(HostHandle {
            host: Rc::new(RefCell::new(ResourceHost::new())),
        }))
    })
}

/// null-safe 释放宿主句柄。挂接中的 Stage 仍持 Rc 克隆，资源随最后一个引用释放。
#[no_mangle]
pub extern "C" fn ikat_host_free(h: *mut HostHandle) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        unsafe {
            drop(Box::from_raw(h));
        }
    })
}

/// 挂外部宿主建 Stage（多 Stage 共享一份资源驻留）。失败返回 null。
/// 老入口 `ikat_stage_new` 等价「自建独占宿主」——单 Stage 用法不变。
#[no_mangle]
pub extern "C" fn ikat_stage_new_bound(h: *mut HostHandle, w: f32, hgt: f32) -> *mut StageHandle {
    ffi_guard(std::ptr::null_mut(), || {
        if h.is_null() {
            return std::ptr::null_mut();
        }
        let host = unsafe { (*h).host.clone() };
        let stage = match Stage::new_bound(host, (w, hgt)) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        Box::into_raw(Box::new(StageHandle {
            stage,
            frame_blob: Vec::new(),
            dump_blob: std::ffi::CString::new("").unwrap(),
            tree_blob: std::ffi::CString::new("").unwrap(),
            warnings_blob: std::ffi::CString::new("").unwrap(),
            style_err_blob: std::ffi::CString::new("").unwrap(),
            style_err_line: 0,
            style_err_col: 0,
        }))
    })
}

/// 宿主级字体注册（签名/语义同 `ikat_stage_register_font`，落共享宿主）。
/// 返回 0=成功，-1=错误。
#[no_mangle]
pub extern "C" fn ikat_host_register_font(
    h: *mut HostHandle,
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
        let host = unsafe { (*h).host.clone() };
        let family =
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(family, family_len) }) {
                Ok(s) => s,
                Err(_) => return -1,
            };
        let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) }.to_vec();
        let mut host = host.borrow_mut();
        host.bump_generation();
        match host.fonts.register(family, bytes, is_default != 0) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// 宿主级回退链（签名/语义同 `ikat_stage_set_fallback_families`，落共享宿主）。
#[no_mangle]
pub extern "C" fn ikat_host_set_fallback_families(
    h: *mut HostHandle,
    text: *const u8,
    text_len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let host = unsafe { (*h).host.clone() };
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
        let mut host = host.borrow_mut();
        host.bump_generation();
        host.fonts.set_fallback_families(&families);
        0
    })
}

/// 宿主级包装载（签名/语义同 `ikat_stage_load_package`，落共享宿主）。
/// 0=ok；1=TooOld；2=TooNew；-1=其他 err。版本记录由 core 侧写入宿主
/// （`ikat_stage_last_pkg_load_version` 对共享宿主同样可见）。
#[no_mangle]
pub extern "C" fn ikat_host_load_package(
    h: *mut HostHandle,
    name: *const u8,
    name_len: usize,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || name.is_null() || bytes.is_null() {
            return -1;
        }
        let host = unsafe { (*h).host.clone() };
        let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) })
        {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
        let mut host = host.borrow_mut();
        host.bump_generation();
        match ikat_core::host::load_package_into(&mut host, name, bytes) {
            Ok(()) => 0,
            Err(LoadPkgError::TooOld { .. }) => 1,
            Err(LoadPkgError::TooNew { .. }) => 2,
            Err(_) => -1,
        }
    })
}

/// 宿主级图尺寸批量注入（签名/语义同 `ikat_stage_set_image_sizes`，落共享宿主）。
#[no_mangle]
pub extern "C" fn ikat_host_set_image_sizes(
    h: *mut HostHandle,
    paths_ptr: *const *const std::os::raw::c_char,
    ws: *const u32,
    hs: *const u32,
    count: usize,
) {
    ffi_guard((), || {
        if h.is_null() || paths_ptr.is_null() || ws.is_null() || hs.is_null() || count == 0 {
            return;
        }
        let host = unsafe { (*h).host.clone() };
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
        let mut host = host.borrow_mut();
        host.bump_generation();
        for (path, w, hgt) in sizes {
            host.image_sizes.insert(path, (w, hgt));
        }
    })
}

/// 宿主级 glyph atlas 脏页拉取（签名/语义同 `ikat_stage_font_atlas_dirty_pages`）。
#[no_mangle]
pub extern "C" fn ikat_host_font_atlas_dirty_pages(
    h: *const HostHandle,
    out: *mut u32,
    max: usize,
) -> usize {
    ffi_guard(usize::MAX, || {
        if h.is_null() || out.is_null() {
            return 0;
        }
        let host = unsafe { (*h).host.clone() };
        let buf = unsafe { std::slice::from_raw_parts_mut(out, max) };
        let n = {
            let host = host.borrow();
            let dirty = host.glyph_atlas.dirty_pages();
            let n = dirty.len().min(buf.len());
            buf[..n].copy_from_slice(&dirty[..n]);
            n
        };
        n
    })
}

/// 宿主级 glyph atlas 页像素拉取（签名/语义同 `ikat_stage_font_atlas_page`，双调法）。
#[no_mangle]
pub extern "C" fn ikat_host_font_atlas_page(
    h: *const HostHandle,
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
        let host = unsafe { (*h).host.clone() };
        // 先探所需大小（传空 buf，不碰 out_buf 指针）
        let needed = {
            let host = host.borrow();
            let (_, pw, ph) = host.glyph_atlas.page_bytes(page);
            (pw * ph) as usize
        };
        if buf_len < needed {
            return needed;
        }
        if needed == 0 {
            return 0;
        }
        if out_buf.is_null() {
            return 0;
        }
        let buf = unsafe { std::slice::from_raw_parts_mut(out_buf, buf_len) };
        let (w, hgt) = {
            let host = host.borrow();
            let (bytes, pw, ph) = host.glyph_atlas.page_bytes(page);
            buf[..needed].copy_from_slice(bytes);
            (pw, ph)
        };
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
        needed
    })
}

/// 宿主级清脏页（签名/语义同 `ikat_stage_font_atlas_clear_dirty`）。
#[no_mangle]
pub extern "C" fn ikat_host_font_atlas_clear_dirty(h: *mut HostHandle) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let host = unsafe { (*h).host.clone() };
        host.borrow_mut().glyph_atlas.clear_dirty();
    })
}
