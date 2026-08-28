//! 控件状态面：ProgressBar/Slider 的 value/max/step、Toggle/Radio 的 checked、
//! Dropdown/TabList 的选中态、NumberField 数值读写（含派生字符串读的公共骨架）。

use ikat_core::scene::node::{ControlState, EditState};
use ikat_core::scene::NodeId;

use crate::{ffi_guard, StageHandle};

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
pub extern "C" fn ikat_stage_set_control_value(
    h: *mut StageHandle,
    node_id: u64,
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
                min,
                max,
                indeterminate,
                ..
            } => {
                // 存储的 min/max 来自 ControlInit（instantiate sanitize 到 min≤max）或
                // set_control_min/max（各自 guard），但 FFI 边界纵深守卫：min>max 会让
                // clamp panic（镜像 Slider arm 的 lo/hi 交换守卫）。
                let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
                let clamped = value.clamp(lo, hi);
                ControlState::Progress {
                    value: clamped,
                    min: lo,
                    max: hi,
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
pub extern "C" fn ikat_stage_get_control_value(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_set_control_checked(
    h: *mut StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_control_checked(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_set_control_max(h: *mut StageHandle, node_id: u64, max: f32) -> i32 {
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
                min,
                indeterminate,
                ..
            } => {
                // Progress 的区间守卫与 Slider 同口径：f32::clamp 在 min > max 时 panic，
                // FFI 不可因 caller 输入 abort 宿主进程（max.max(min) 守卫）。
                let max = max.max(min);
                // 改 max 后把 value 重新 clamp 进新区间（避免 value > max 的悬空态）
                let value = value.clamp(min, max);
                ControlState::Progress {
                    value,
                    min,
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
pub extern "C" fn ikat_stage_get_control_max(
    h: *const StageHandle,
    node_id: u64,
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

/// 设控件 min（ProgressBar / Slider / NumberField）。null 句柄 / 节点缺失 → -1。
/// 改 min 后 value 重新 clamp。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_set_control_min(h: *mut StageHandle, node_id: u64, min: f32) -> i32 {
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
                max,
                indeterminate,
                ..
            } => {
                let min = min.min(max);
                let value = value.clamp(min, max);
                ControlState::Progress {
                    value,
                    min,
                    max,
                    indeterminate,
                }
            }
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

/// 读控件 min（ProgressBar / Slider / NumberField）。非数值控件 / null out / 节点缺失 → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_control_min(
    h: *const StageHandle,
    node_id: u64,
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
                ControlState::Progress { min, .. }
                | ControlState::Slider { min, .. }
                | ControlState::NumberField { min, .. },
            ) => {
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
pub extern "C" fn ikat_stage_set_control_step(h: *mut StageHandle, node_id: u64, step: f32) -> i32 {
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
pub extern "C" fn ikat_stage_get_control_step(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_control_indeterminate(
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
pub extern "C" fn ikat_stage_set_control_indeterminate(
    h: *mut StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_radio_name(
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
pub extern "C" fn ikat_stage_get_dropdown_selected_value(
    h: *const StageHandle,
    node_id: u64,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        let value = match read_control_string(h, node_id, out, buf_cap, out_len, |scene, id| {
            ikat_core::scene::control::dropdown_selected_value(scene, id)
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
pub extern "C" fn ikat_stage_get_option_value(
    h: *const StageHandle,
    node_id: u64,
    out: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    ffi_guard(-1, || {
        let value = match read_control_string(h, node_id, out, buf_cap, out_len, |scene, id| {
            ikat_core::scene::control::option_value(scene, id)
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
pub extern "C" fn ikat_stage_is_option_selected(h: *const StageHandle, node_id: u64) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match ikat_core::scene::control::option_selected(scene, NodeId(node_id)) {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        }
    })
}

/// option 在其所属 Dropdown 的声明序（0 基，与 selected_index / 键盘 seek 同口径）。
/// -1 = 非 option / 上溯无 Dropdown / null 句柄。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_option_index(h: *const StageHandle, node_id: u64) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match ikat_core::scene::control::option_index(scene, NodeId(node_id)) {
            Some((_, idx)) => idx as i32,
            None => -1,
        }
    })
}

/// tab 是否为所属 TabList 的当前激活项（合成：序号 == 父 selected_index，与
/// aria-selected 派生同源）。1=激活，0=未激活，-1=非 tab / 上溯无 TabList / null 句柄。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_is_tab_selected(h: *const StageHandle, node_id: u64) -> i32 {
    ffi_guard(-1, || {
        if h.is_null() {
            return -1;
        }
        let sh = unsafe { &*h };
        let Some(scene) = sh.stage.scene.as_ref() else {
            return -1;
        };
        match ikat_core::scene::control::tab_selected(scene, NodeId(node_id)) {
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
    node_id: u64,
    _out: *mut u8,
    _buf_cap: usize,
    out_len: *mut usize,
    f: impl Fn(&ikat_core::scene::Scene, NodeId) -> Option<String>,
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

/// 读 Dropdown 当前选中项索引（`ControlState::Dropdown.selected_index`）。
/// 非 Dropdown / null 句柄 / 节点缺失 / null out → -1。
///
/// **常驻（不 gate）。**
#[no_mangle]
pub extern "C" fn ikat_stage_get_dropdown_selected_index(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_set_dropdown_selected_index(
    h: *mut StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_tablist_selected_index(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_set_tablist_selected_index(
    h: *mut StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_get_dropdown_open(
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
pub extern "C" fn ikat_stage_set_dropdown_open(h: *mut StageHandle, node_id: u64, open: u8) -> i32 {
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
pub extern "C" fn ikat_stage_get_number_value(
    h: *const StageHandle,
    node_id: u64,
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
pub extern "C" fn ikat_stage_set_number_value(
    h: *mut StageHandle,
    node_id: u64,
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
        let s = format!("{:.6}", v);
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        if trimmed.is_empty() {
            "0".to_string()
        } else {
            trimmed.to_string()
        }
    }
}
