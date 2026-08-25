//! ListView 虚拟化面：item_count/template 设定、pending binds 拉取（SOA）、
//! 同帧推进、区间刷新、增删搬通知（单 FFI 多 op）、滚动到指定 item。

use loomgui_core::scene::NodeId;

use crate::{ffi_guard, StageHandle};

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
