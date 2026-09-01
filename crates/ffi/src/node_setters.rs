//! 节点写面：树构建（create/append/insert/remove）、text/src/inline override 写、
//! class 增删查、交互标志（disabled/touchable/focusable）、user transform、子树克隆。

use ikat_core::scene::{dynamic, NodeId};
use ikat_core::transform::NodeTransform;

use crate::{ffi_guard, StageHandle};

/// 业务设节点 disabled 状态（伪类源 + active/click 抑制）。NodeId.0 越界静默跳过。
/// null 句柄 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_set_node_disabled(h: *mut StageHandle, node_id: u64, disabled: bool) {
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
pub extern "C" fn ikat_stage_set_node_touchable(
    h: *mut StageHandle,
    node_id: u64,
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

/// 设节点 draggable（公共 Node.Draggable 的后端；HTML `draggable` 属性的运行时面）。
/// true = 参与 drag_target 候选，pointer-down 后按位移阈值启动 DragStart/Move/End。
/// 只写 interaction.draggable（无 rematch 通道）。null 句柄 / 节点缺失 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_set_node_draggable(
    h: *mut StageHandle,
    node_id: u64,
    draggable: bool,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.set_node_draggable(NodeId(node_id), draggable);
    })
}

/// 设节点运行时可获焦性（公共 Node.Focusable 后端）。true → tabindex=0（Tab 链 0 组）；
/// false → tabindex=-1（Tab 链/点击聚焦排除，编程 Focus() 仍可用——DOM 语义）。
/// null 句柄 / 节点缺失 → no-op。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_set_node_focusable(
    h: *mut StageHandle,
    node_id: u64,
    focusable: bool,
) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let sh = unsafe { &mut *h };
        sh.stage.set_node_focusable(NodeId(node_id), focusable);
    })
}

/// 设渲染复用键（虚拟列表 slot）。null 句柄/无效 node → no-op。
#[no_mangle]
pub extern "C" fn ikat_stage_set_reuse_key(h: *mut StageHandle, node_id: u64, key: u32) {
    ffi_guard((), || {
        if h.is_null() {
            return;
        }
        let handle = unsafe { &mut *h };
        handle.stage.set_reuse_key(NodeId(node_id), key);
    })
}

/// 克隆场景内子树（游离根，不挂树）。返回新 node_id；u64::MAX = err / null 句柄 / 无效 src。
#[no_mangle]
pub extern "C" fn ikat_stage_clone_subtree(h: *mut StageHandle, src: u64) -> u64 {
    ffi_guard(u64::MAX, || {
        const ERR: u64 = u64::MAX;
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

// 错误语义：create_root/create_node 返 u64 NodeId（u64::MAX = 失败）；
// 其余返 i32（0=ok，-1=err）。null 句柄 → 失败/sentinel（不 panic）。

/// 建根节点并设为 roots[0]。kind/css = UTF-8 字节。返 NodeId；u64::MAX = 失败。
///
/// null 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）：**runtime 稳定入口，`--no-default-features` 构建的 .dll 仍有本函数。
#[no_mangle]
pub extern "C" fn ikat_stage_create_root(
    h: *mut StageHandle,
    kind: *const u8,
    kind_len: usize,
    css: *const u8,
    css_len: usize,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const FAIL: u64 = u64::MAX;
        if h.is_null() {
            return FAIL;
        }
        let sh = unsafe { &mut *h };
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

/// 建节点（不挂父）。kind/css = UTF-8 字节。返 NodeId；u64::MAX = 失败。
/// 需配合 append_child/insert_before 挂到树。
///
/// null 指针（含 len=0）兜底为空串（spec §6.1 deferred ②：from_raw_parts(null,0) 是 UB）。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_create_node(
    h: *mut StageHandle,
    kind: *const u8,
    kind_len: usize,
    css: *const u8,
    css_len: usize,
) -> u64 {
    ffi_guard(u64::MAX, || {
        const FAIL: u64 = u64::MAX;
        if h.is_null() {
            return FAIL;
        }
        let sh = unsafe { &mut *h };
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
pub extern "C" fn ikat_stage_append_child(h: *mut StageHandle, parent: u64, child: u64) -> i32 {
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

/// 在 parent.children 中 ref_id 之前插 child。ref_id=u64::MAX（INVALID）→ 末尾追加。
/// 0=ok，-1=err。null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_insert_before(
    h: *mut StageHandle,
    parent: u64,
    child: u64,
    ref_id: u64,
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
pub extern "C" fn ikat_stage_remove_child(h: *mut StageHandle, parent: u64, child: u64) -> i32 {
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
pub extern "C" fn ikat_stage_remove_node(h: *mut StageHandle, node: u64) -> i32 {
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
pub extern "C" fn ikat_stage_set_text(
    h: *mut StageHandle,
    node: u64,
    text: *const u8,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
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
pub extern "C" fn ikat_stage_restart_animations(h: *mut StageHandle, node_id: u64) -> i32 {
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
pub extern "C" fn ikat_stage_set_src(
    h: *mut StageHandle,
    node: u64,
    src: *const u8,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
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
pub extern "C" fn ikat_stage_set_inline_override(
    h: *mut StageHandle,
    node: u64,
    css: *const u8,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
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
pub extern "C" fn ikat_stage_unset_inline_override(
    h: *mut StageHandle,
    node: u64,
    prop: *const u8,
    len: usize,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
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

/// 加 class（重复名不重复 push）。name = UTF-8 字节。0=ok，-1=err。
/// 标 dirty_mesh 触发下帧 rematch。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_add_class(
    h: *mut StageHandle,
    node: u64,
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
pub extern "C" fn ikat_stage_remove_class(
    h: *mut StageHandle,
    node: u64,
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
pub extern "C" fn ikat_stage_has_class(
    h: *const StageHandle,
    node: u64,
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

/// 设节点 user transform（位移/缩放/旋转/原点）。走 `set_user_transform`（dynamic.rs）：
/// 只写 `node.user_transform`，不触发 layout solve——`compute_world_transforms` 在
/// 世界矩阵累计时并入（渲染/命中层，同 CSS transform）。供高频拖拽等运行时定位用。
/// `ox/oy` = 旋转/缩放原点（local 坐标 px），连接 C# `NodeTransform.Origin`。
/// 不 live 节点 / null 句柄 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_set_transform(
    h: *mut StageHandle,
    node_id: u64,
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

/// 运行时渲染隐藏（世界锚点出屏/相机背后自动隐藏）。与 display:none 正交：不影响布局/
/// 命中，只控本节点全部渲染行 visible 位（后端语义：保留镜像对象、SetActive(false)——
/// C# MirrorPool 对 visible=0 行清 stale + 隐藏，不销毁）。visible：非0=显示 0=隐藏。
/// 返回 0=成功，-1=null 句柄 / 无场景 / 节点不 live。
#[no_mangle]
pub extern "C" fn ikat_stage_set_node_visible(
    h: *mut StageHandle,
    node_id: u64,
    visible: u8,
) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let node = ikat_core::scene::node::NodeId(node_id);
        match sh.stage.set_node_render_hidden(node, visible == 0) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// world-space 挂载登记（#109 C8）：node 子树挂到业务摆放的 3D 容器——渲染行顶点
/// re-base 到挂载根局部系 + blob mount_id 列写 slot（后端按槽位路由 SetParent）。
/// slot：driver 分配保证唯一；0 = 解除挂载回屏幕空间。
/// 返回 0=成功，-1=null 句柄 / 无场景 / 节点不 live。
#[no_mangle]
pub extern "C" fn ikat_stage_set_node_mount(h: *mut StageHandle, node_id: u64, slot: u32) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &mut *h };
        let node = ikat_core::scene::node::NodeId(node_id);
        match sh.stage.set_node_mount(node, slot) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}
