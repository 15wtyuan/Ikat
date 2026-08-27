//! 帧输出面：tick、帧 blob 借出、场景 dump、运行时警告/缺字诊断拉取、
//! 测试 pkg 构造与堆分配 bytes 释放。

use std::ffi::CString;

use crate::{blob, ffi_guard, StageHandle};

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

/// 拉取累积的运行时警告（drain：取走后清空，同 take_pending_binds 语义）。多条以 `\n`
/// 连接成单个 UTF-8 C 串 + len；宿主 split('\n') 逐条打到引擎日志（Unity Debug.LogWarning）。
/// 无警告 → null + len=0（宿主不 log）。警告推送方自带 warn-once 去重（core list.rs），
/// 无人调用本函数缓冲也不会无限涨。指针到下次 take 失效。
///
/// **常驻（不 gate）：**运行时诊断出口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn loomgui_stage_take_warnings(
    h: *mut StageHandle,
    out_len: *mut usize,
) -> *const u8 {
    ffi_guard(std::ptr::null(), || {
        if h.is_null() || out_len.is_null() {
            return std::ptr::null();
        }
        let handle = unsafe { &mut *h };
        let warnings = match handle.stage.scene.as_mut() {
            Some(scene) => std::mem::take(&mut scene.warnings),
            None => Vec::new(),
        };
        if warnings.is_empty() {
            handle.warnings_blob = CString::new("").unwrap();
            unsafe { *out_len = 0 };
            return std::ptr::null();
        }
        // 分条截断内嵌 NUL（而非整串丢弃）：任一条含 NUL 时 join+CString::new 整体失败，
        // 旧兜底静默吞掉全部警告——逐条截到 NUL 前再拼，坏一条不殃及其余。
        let sanitized: Vec<String> = warnings
            .iter()
            .map(|w| w.split('\0').next().unwrap_or_default().to_string())
            .collect();
        handle.warnings_blob = CString::new(sanitized.join("\n")).unwrap();
        let bytes = handle.warnings_blob.as_bytes_with_nul();
        unsafe { *out_len = bytes.len() };
        handle.warnings_blob.as_ptr() as *const u8
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
                href: None,
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
                href: None,
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

/// dump 可读树视图（#85）：每节点一行 `tag#id.class (x,y,w,h)` + 文本
/// （font/行高乘数/行数/内容摘要）与滚动（viewport/content/overlap/pos/物理）
/// 关键 resolved 值，ASCII 树缩进表父子——AI 代理与人都比深嵌套 JSON 好读。
/// filter = UTF-8 子串（指针+len），null/len=0 = 全量；只出 id/class 命中的子树。
/// 返 Rust 拥有的 UTF-8 C 串 + len（StageHandle 持有，下次调用覆盖）。
#[no_mangle]
pub extern "C" fn loomgui_stage_dump_tree(
    h: *mut StageHandle,
    filter: *const u8,
    filter_len: usize,
    out_len: *mut usize,
) -> *const u8 {
    ffi_guard(std::ptr::null(), || {
        if h.is_null() || out_len.is_null() {
            return std::ptr::null();
        }
        let handle = unsafe { &mut *h };
        let filter = if filter.is_null() || filter_len == 0 {
            None
        } else {
            match std::str::from_utf8(unsafe { std::slice::from_raw_parts(filter, filter_len) }) {
                Ok(s) => Some(s),
                Err(_) => return std::ptr::null(),
            }
        };
        let tree = match &handle.stage.scene {
            Some(scene) => loomgui_core::dump::dump_scene_tree(scene, filter),
            None => String::from("(no scene)"),
        };
        handle.tree_blob = CString::new(tree).unwrap_or_else(|_| CString::new("").unwrap());
        let bytes = handle.tree_blob.as_bytes_with_nul();
        unsafe { *out_len = bytes.len() };
        handle.tree_blob.as_ptr() as *const u8
    })
}
