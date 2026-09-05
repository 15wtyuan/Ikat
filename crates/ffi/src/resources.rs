//! 资源面：图集图片尺寸批量注入（driver 启动）、字体图集脏页/页像素拉取与清脏。

use crate::{ffi_guard, StageHandle};

/// driver 启动时把所有 atlas.json 合并出的图尺寸批量灌入（一次调用，非逐条）。
/// paths_ptr: count 个 C 字符串指针；ws/hs: count 个 u32。任一为 null 或 count=0 → no-op。
/// 首帧 solve 前调（启动加载阶段）。FFI 入口不 panic。
#[no_mangle]
pub extern "C" fn yio_stage_set_image_sizes(
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

/// 拉脏页 page_idx 列表（写入 out，返实际数）。null 句柄 / null out → 返 0。
#[no_mangle]
pub extern "C" fn yio_stage_font_atlas_dirty_pages(
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
pub extern "C" fn yio_stage_font_atlas_page(
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
            return needed;
        }
        if needed == 0 {
            return 0;
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
pub extern "C" fn yio_stage_font_atlas_clear_dirty(h: *mut StageHandle) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.font_atlas_clear_dirty();
    })
}
