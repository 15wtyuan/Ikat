//! 节点只读查询面：布局矩形/世界矩阵/sort_key/可见性/语义类型/id/class/opacity/
//! computed style 快照、父子遍历、id 查找、交互标志读取。

use ikat_core::scene::NodeId;
use ikat_core::style::computed::ComputedNodeStyle;
use ikat_core::style::resolved::TextAlign;
use ikat_core::transform;

use crate::{ffi_guard, StageHandle};

/// 读节点可获焦性（interaction.tabindex >= 0，Tab 链判据同源）。null 句柄 / 无 scene /
/// 节点缺失 → -1（不与 false 混淆）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_focusable(
    h: *const StageHandle,
    node_id: u64,
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
                unsafe { *out = u8::from(matches!(n.interaction.tabindex, Some(t) if t >= 0)) };
                0
            }
            None => -1,
        }
    })
}

/// 读节点 touchable（interaction.touchable，hit_test 同源）。null 句柄 / 无 scene /
/// 节点缺失 → -1（不与 false 混淆）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_touchable(
    h: *const StageHandle,
    node_id: u64,
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

/// 读节点 draggable（interaction.draggable，drag_target 候选判据同源）。
/// null 句柄 / 无 scene / 节点缺失 → -1（不与 false 混淆）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_draggable(
    h: *const StageHandle,
    node_id: u64,
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
                unsafe { *out = u8::from(n.interaction.draggable) };
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
pub extern "C" fn ikat_node_is_lookup_scope(h: *const StageHandle, node_id: u64) -> i32 {
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
                    .contains(ikat_core::scene::node::NodeFlags::LOOKUP_SCOPE),
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
pub extern "C" fn ikat_stage_get_custom_tag(
    h: *const StageHandle,
    node_id: u64,
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

/// 返 parent node_id（C# 事件路由沿链用，spec §4.2）。根/越界/无 scene → u64::MAX（sentinel）。
///
/// **常驻（不 gate）：**runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn ikat_node_parent(h: *const StageHandle, node_id: u64) -> u64 {
    ffi_guard(u64::MAX, || {
        const ROOT_SENTINEL: u64 = u64::MAX;
        if h.is_null() {
            return ROOT_SENTINEL;
        }
        let sh = unsafe { &*h };
        match &sh.stage.scene {
            Some(scene) => {
                // NodeId(u64) → slotmap lookup（代际安全）。无效/悬空 NodeId → sentinel。
                match scene.get(NodeId(node_id)) {
                    Some(n) => n.parent.map(|p| p.0).unwrap_or(ROOT_SENTINEL),
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
pub extern "C" fn ikat_stage_find_node_by_id(
    h: *const StageHandle,
    id: *const u8,
    id_len: usize,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const NOT_FOUND: u64 = u64::MAX;
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
            Some(nid) => nid.0,
            None => NOT_FOUND,
        }
    })
}

/// 在 root 子树内 DFS 查找 id 属性匹配的首个节点（self-exclusive：从 root 的直接子开始 DFS，root 自身 id_attr 不参与匹配，与 DOM querySelectorAll/Query<T> 一致）。
/// root、id = UTF-8 字节（指针+len）。返 node_id；null 句柄/非 UTF-8/无匹配 → u64::MAX（sentinel）。
/// 替代"全局首匹配 + 父链后过滤"——C# TryGet/Get 用此入口避免 list slot 间 id 碰撞。
///
/// **常驻（不 gate）：**runtime 稳定入口。
#[no_mangle]
pub extern "C" fn ikat_stage_find_node_by_id_in_subtree(
    h: *const StageHandle,
    root: u64,
    id: *const u8,
    id_len: usize,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const NOT_FOUND: u64 = u64::MAX;
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
            Some(nid) => nid.0,
            None => NOT_FOUND,
        }
    })
}

/// 读节点 layout_rect。null 句柄/无效 node → out 填 0（不 panic）。
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_layout_rect(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_node_world_matrix(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_node_sort_key(h: *const StageHandle, node_id: u64, out: *mut u32) {
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
pub extern "C" fn ikat_stage_get_node_visible(h: *const StageHandle, node_id: u64, out: *mut u8) {
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

/// FFI 稳定快照（#[repr(C)] POD）。enum→u8（match 稳定化，不靠 enum 隐式 repr），
/// Option<[f32;4]>→present flag + 数组。csbindgen 自动生成 struct C# stub；如需重排字段可扩展或手写覆盖。
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
pub extern "C" fn ikat_stage_get_node_kind(
    h: *const StageHandle,
    node_id: u64,
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

/// 读节点 HTML `id` 属性（authoring id；未声明 → rc=0 + len=0 空串）。
/// return-code + out-param（ptr+len）双调法（同 get_control_text）：buf_cap 足够 →
/// rc=0 写 buf[..*out_len]；不够（含 0 探大小）→ rc=-2 + *out_len=所需；null 句柄 /
/// 无 scene / 死节点 → rc=-1。调试探针（pick 命中链）与 authoring id 读取用。
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_id_attr(
    h: *const StageHandle,
    node_id: u64,
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
            unsafe { *out_len = 0 };
            return -1;
        };
        let Some(node) = scene.get(NodeId(node_id)) else {
            unsafe { *out_len = 0 };
            return -1;
        };
        let value = node.id_attr.as_deref().map(str::as_bytes).unwrap_or(&[]);
        let needed = value.len();
        unsafe { *out_len = needed };
        if needed > buf_cap {
            return -2;
        }
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), out, needed) };
        }
        0
    })
}

/// 读 `<a>` 节点的 href（#74 链接目标，opaque 字符串；C# Link 事件处理按它路由）。
/// 双调法同 [`ikat_stage_get_node_id_attr`]：buf_cap 足够 → rc=0 写 buf[..*out_len]；
/// 不够（含 0 探大小）→ rc=-2 + *out_len=所需；null 句柄 / 无 scene / 死节点 → rc=-1
///（*out_len 置 0）。非 Link 节点或 link_hrefs 无条目 → rc=1（区别于 -1 的句柄错误：
/// 调用方拿 1 判「不是链接」，拿 -1 判「参数/场景错误」）。href 由 instantiate 从
/// pkg TemplateNode 灌入 `Scene.link_hrefs`（围栏保证非空）。
#[no_mangle]
pub extern "C" fn ikat_stage_get_link_href(
    h: *const StageHandle,
    node_id: u64,
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
            unsafe { *out_len = 0 };
            return -1;
        };
        // 先判节点存在（-1）再判是否 Link/有条目（1）——错误码分层见函数文档。
        let Some(node) = scene.get(NodeId(node_id)) else {
            unsafe { *out_len = 0 };
            return -1;
        };
        if node.kind != ikat_core::scene::NodeKind::Link {
            unsafe { *out_len = 0 };
            return 1;
        }
        let Some(href) = scene.link_hrefs.get(&NodeId(node_id)) else {
            unsafe { *out_len = 0 };
            return 1;
        };
        let value = href.as_bytes();
        let needed = value.len();
        unsafe { *out_len = needed };
        if needed > buf_cap {
            return -2;
        }
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), out, needed) };
        }
        0
    })
}

/// 读节点 computed opacity（rematch 后 style.opacity，与渲染/命中同源）。
/// 调试探针用：「播完即隐形」的演出层偷命中时 opacity=0 但仍接住指针——链顶即凶手。
/// rc：0 = ok 且 *out 已填；1 = null 句柄 / 无 scene / 节点不存在 / out null。
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_opacity(
    h: *const StageHandle,
    node_id: u64,
    out: *mut f32,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() || out.is_null() {
            return 1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return 1;
        };
        match scene.get(NodeId(node_id)) {
            Some(n) => {
                unsafe { *out = n.style.opacity };
                0
            }
            None => 1,
        }
    })
}

/// 读节点 class 列表（空格 join；无 class → rc=0 + len=0）。双调法同
/// [`ikat_stage_get_node_id_attr`]。调试探针用（ClassList 公共面是
/// Contains/Add 族，无全量枚举——本出口补齐只读枚举）。
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_classes(
    h: *const StageHandle,
    node_id: u64,
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
            unsafe { *out_len = 0 };
            return -1;
        };
        let Some(node) = scene.get(NodeId(node_id)) else {
            unsafe { *out_len = 0 };
            return -1;
        };
        let joined = node.classes.join(" ");
        let value = joined.as_bytes();
        let needed = value.len();
        unsafe { *out_len = needed };
        if needed > buf_cap {
            return -2;
        }
        if needed > 0 {
            if out.is_null() {
                return -2;
            }
            unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), out, needed) };
        }
        0
    })
}

/// 读节点 computed style 快照。return code：0 = ok 且 `*out` 填好；非 0 = 失败（节点不存在
/// 或 `out` = null）。
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_computed_style(
    h: *const StageHandle,
    node_id: u64,
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

/// 读节点子节点数。返回 i32：≥0 = 子节点数；-1 = err（null 句柄 / 节点不 live）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_child_count(h: *const StageHandle, node: u64) -> i32 {
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

/// 读节点子节点 NodeId 列表，写入 `out` buffer（u64 per slot）。
/// 返回 i32：≥0 = 实际写入数；负值 = err（-1 = null 句柄 / 节点不 live；
/// -(n+2) = buffer 不够，n = 所需 cap）。调用方遇负值重分配 n+ 容量再调。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_children(
    h: *const StageHandle,
    node: u64,
    out: *mut u64,
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

/// 读节点 disabled 伪类态（`NodeFlags::DISABLED`）。null 句柄 / 无 scene / 节点缺失 → 写 0（false）。
/// 与 `ikat_stage_set_node_disabled` 对称的读出口（伪类态级联查询用）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_node_disabled(h: *const StageHandle, node_id: u64, out: *mut u8) {
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
