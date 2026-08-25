use crate::input::{EventRecord, EVT_CHANGE_COMMITTED, EVT_CHECKED_CHANGED, EVT_VALUE_CHANGED};
use crate::scene::node::{ControlState, NodeFlags, NodeId, Scene};
use crate::scene::text_cursor::{hit_byte_offset, line_byte_ranges};

use super::dropdown::{
    close_dropdown, commit_dropdown_selection, dropdown_option_at_pos, open_dropdown, pos_in_popup,
    world_rect_contains,
};
use super::edit::{clamp_boundary, display_to_value_byte, display_value_masked, mask_char};
use super::roles::ROLE_TAB;
use super::tablist::set_tablist_selected_index;

// Toggle/Radio 在 pointer-down 翻转/互斥选中；Slider 在 down→move→up 期间拖拽改 value。
// 这些函数是纯逻辑（读 ControlState + track 几何，写 side table），由 PointerState::process
// 在 Down/Move/Up 臂调用（命中控件时）。独立于事件仲裁——只改控件状态，不产事件。

/// 从命中节点向上找最近的控件节点。命中常落在控件的内部部件（thumb/fill 等作者写的
/// data-slot 子节点）上，需向上追溯到控件本身（控件是顶层 control 节点，其部件子节点
/// 不是控件）。无命中 / 链上无控件 → None。
pub fn find_control_at(scene: &Scene, hit: Option<NodeId>) -> Option<NodeId> {
    let mut cur = hit;
    while let Some(id) = cur {
        if scene.controls.get(id).is_some() {
            return Some(id);
        }
        cur = scene.get(id).and_then(|n| n.parent);
    }
    None
}

/// Slider 是否占据指针手势（拖拽期间需抑制祖先 scroll）。未禁用 Slider（任何指针）
/// 与未禁用文本控件（仅鼠标——拖=选区；触摸拖让位视口 pan，浏览器对齐）为真。
/// Toggle/Radio 点击瞬时完成不占手势；disabled 控件不拦截指针（照 HTML：disabled input
/// 不接受交互），否则按下后 scroll 仲裁被清却无人处理，用户滚不动。
/// PointerState 据此决定是否抑制 scroll 候选。
pub fn occupies_gesture(scene: &Scene, id: NodeId, is_mouse: bool) -> bool {
    let disabled = scene
        .get(id)
        .is_some_and(|n| n.interaction.flags.contains(NodeFlags::DISABLED));
    if disabled {
        return false;
    }
    let is_slider = matches!(scene.controls.get(id), Some(ControlState::Slider { .. }));
    let is_text = matches!(
        scene.controls.get(id),
        Some(
            ControlState::TextField(_)
                | ControlState::TextArea(_)
                | ControlState::NumberField { .. }
        )
    );
    is_slider || (is_mouse && is_text)
}

/// 指针按下命中控件 → 更新控件状态。返回产生的事件（空 Vec=未命中/未处理）。
///
/// - Toggle：翻转 checked → 产 EVT_CHECKED_CHANGED（pad[0]=新值）。
/// - Radio：同名组互斥——全树找同 name 的其它 radio 置 checked=false，本 radio 置 true
///   → 产 EVT_CHECKED_CHANGED（仅新选中那个，pad[0]=1；照 HTML 只对选中项发 change）。
/// - Slider：置 dragging=true + 按 pos 重算 value（track 几何取上一帧 solve，1 帧滞后，同 hit_test）。
///   value 实际变化时产 EVT_VALUE_CHANGED（x=新值）。
/// - Progress：无交互（空）。
///
/// disabled 控件不响应（照 HTML：disabled input 不接受点击）。pos 仅 Slider 用。
pub fn on_pointer_down(scene: &mut Scene, id: NodeId, pos: [f32; 2]) -> Vec<EventRecord> {
    let mut out = Vec::new();
    if scene
        .get(id)
        .is_some_and(|n| n.interaction.flags.contains(NodeFlags::DISABLED))
    {
        return out;
    }
    let Some(state) = scene.controls.get(id).cloned() else {
        return out;
    };
    match state {
        ControlState::Toggle { checked } => {
            scene
                .controls
                .ensure(id, ControlState::Toggle { checked: !checked });
            out.push(EventRecord {
                node_id: id.0,
                event_type: EVT_CHECKED_CHANGED,
                click_count: 0,
                pad: [checked_to_u8(!checked), 0],
                touch_id: 0,
                x: 0.0,
                y: 0.0,
                dx: 0.0,
                dy: 0.0,
            });
        }
        ControlState::Radio { name, .. } => {
            select_radio(scene, id, name, &mut out);
        }
        ControlState::Slider { .. } => {
            if let Some(ControlState::Slider { dragging, .. }) = scene.controls.get_mut(id) {
                *dragging = true;
            }
            if let Some(v) = slider_pos_to_value(scene, id, pos) {
                set_slider_value(scene, id, v, &mut out);
            }
        }
        // TextField/TextArea/NumberField: convert world pos to content-area-local coords
        // (subtract layout_rect offset + border+padding inset), then set cursor/anchor
        // via hit_byte_offset. TextLayout glyphs are in content-area-local space.
        // NumberField 是 TextField 的数值变体——光标定位逻辑完全一致（edit 共享 EditState）。
        ControlState::TextField(_)
        | ControlState::TextArea(_)
        | ControlState::NumberField { .. } => {
            if let Some(n) = scene.get(id) {
                let lr = n.layout_rect;
                let border_left = crate::render::resolve_lp(n.style.taffy_style.border.left);
                let padding_left = crate::render::resolve_lp(n.style.taffy_style.padding.left);
                let border_top = crate::render::resolve_lp(n.style.taffy_style.border.top);
                let padding_top = crate::render::resolve_lp(n.style.taffy_style.padding.top);
                let local_x = pos[0] - lr.x - border_left - padding_left;
                let local_y = pos[1] - lr.y - border_top - padding_top;
                on_text_pointer_down(scene, id, local_x, local_y);
            }
        }
        ControlState::Progress { .. } => {}
        ControlState::Dropdown { open, .. } => {
            // 交互（照 RmlUi WidgetDropDown）：
            // - closed → 点 select（header/value 区）→ open=true + 记 open_selected_index。
            // - open → 点 enabled option → 选中 + 收起 + 发 SelectionChanged。
            // - open → 点 header（不在 popup 矩形内）→ toggle 收起。
            // - open → 点 disabled option / popup 背景 → 不动（dropdown_option_at_pos 返 None
            //   且 pos 在 popup 内 → 不收起，照 HTML disabled option 不可交互）。
            if open {
                if let Some(idx) = dropdown_option_at_pos(scene, id, pos) {
                    commit_dropdown_selection(scene, id, idx, &mut out);
                } else if !pos_in_popup(scene, id, pos) {
                    close_dropdown(scene, id);
                }
            } else {
                open_dropdown(scene, id);
            }
        }
        // TabList：点 role=tab 子 → 设 selected_index = 该 tab 的序号 + 发
        // SelectionChanged。find_control_at 从命中节点向上找最近 ControlState：Tab 无
        // ControlState、TabList 有 → id 是 TabList，pos 是点击世界坐标。按声明序遍历 role=tab
        // 子，世界 AABB rect-contains 命中第一个含 pos 的 tab（镜像 dropdown_option_at_pos
        // 命中模式，世界矩阵取上一帧 solve+world）。pos 不落任一 tab（点 tablist padding）→ no-op。
        ControlState::TabList { .. } => {
            let tab_ids: Vec<NodeId> = scene
                .get(id)
                .map(|n| n.children.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|&c| scene.roles.role_of(c) == Some(ROLE_TAB))
                .collect();
            for (i, &tab) in tab_ids.iter().enumerate() {
                if world_rect_contains(scene, tab, pos) {
                    set_tablist_selected_index(scene, id, i, &mut out);
                    break;
                }
            }
        }
    }
    out
}

/// 指针移动。仅 Slider 拖拽中（dragging=true）跟随指针重算 value → value 变化时产
/// EVT_VALUE_CHANGED；其它情况返空。PointerState Move 臂在 control_target 存在时调用
/// （函数内部自检 dragging，安全）。
/// 指针按住移动（每 Move）：Slider 跟手更新 value；文本控件（TextField/TextArea/
/// NumberField）拖拽扩展选区——anchor 保持 Down 落点，cursor 跟随命中推进。
/// `is_mouse` 门控文本臂：触摸拖选让位视口 pan（占据手势侧 occupies_gesture 同判）。
pub fn on_pointer_move(
    scene: &mut Scene,
    id: NodeId,
    pos: [f32; 2],
    is_mouse: bool,
) -> Vec<EventRecord> {
    let mut out = Vec::new();
    let dragging = matches!(
        scene.controls.get(id),
        Some(ControlState::Slider { dragging: true, .. })
    );
    if dragging {
        if let Some(v) = slider_pos_to_value(scene, id, pos) {
            set_slider_value(scene, id, v, &mut out);
        }
        return out;
    }
    let is_text = matches!(
        scene.controls.get(id),
        Some(
            ControlState::TextField(_)
                | ControlState::TextArea(_)
                | ControlState::NumberField { .. }
        )
    );
    if is_mouse && is_text {
        // 世界坐标 → content-area-local（同 on_pointer_down 文本臂的转换链）。
        if let Some(n) = scene.get(id) {
            let lr = n.layout_rect;
            let border_left = crate::render::resolve_lp(n.style.taffy_style.border.left);
            let padding_left = crate::render::resolve_lp(n.style.taffy_style.padding.left);
            let border_top = crate::render::resolve_lp(n.style.taffy_style.border.top);
            let padding_top = crate::render::resolve_lp(n.style.taffy_style.padding.top);
            let local_x = pos[0] - lr.x - border_left - padding_left;
            let local_y = pos[1] - lr.y - border_top - padding_top;
            on_text_pointer_drag(scene, id, local_x, local_y);
        }
    }
    out
}

/// 指针松手。Slider 清 dragging（结束本次拖拽）+ 产 EVT_CHANGE_COMMITTED（x=最终值，
/// 仅当本次确实在拖拽）；其它控件返空。PointerState Up/Canceled 臂调用。
pub fn on_pointer_up(scene: &mut Scene, id: NodeId) -> Vec<EventRecord> {
    let mut out = Vec::new();
    let prev = scene.controls.get(id).cloned();
    if let Some(ControlState::Slider {
        value, dragging, ..
    }) = prev
    {
        if dragging {
            if let Some(ControlState::Slider { dragging: d, .. }) = scene.controls.get_mut(id) {
                *d = false;
            }
            out.push(EventRecord {
                node_id: id.0,
                event_type: EVT_CHANGE_COMMITTED,
                click_count: 0,
                pad: [0, 0],
                touch_id: 0,
                x: value,
                y: 0.0,
                dx: 0.0,
                dy: 0.0,
            });
        }
    }
    out
}

/// 文本控件 pointer-down：世界坐标已转为 content-area-local（减 layout_rect.xy + border+padding），
/// 用 hit_byte_offset 计算字节偏移，设 cursor=anchor=offset，重置闪烁 timer。
///
/// 无缓存 TextLayout（首帧尚无 measure）→ no-op。非 TextField/TextArea/NumberField → no-op。
pub fn on_text_pointer_down(scene: &mut Scene, id: NodeId, local_x: f32, local_y: f32) {
    let Some(offset) = text_hit_offset(scene, id, local_x, local_y) else {
        return;
    };
    if let Some(
        ControlState::TextField(e)
        | ControlState::TextArea(e)
        | ControlState::NumberField { edit: e, .. },
    ) = scene.controls.get_mut(id)
    {
        e.cursor = offset;
        e.anchor = offset;
        e.ideal_cursor_valid = false;
        e.cursor_visible = true;
        e.cursor_timer = 0.0;
    }
}

/// 文本控件拖拽选区（按住移动）：同 on_text_pointer_down 的命中管线，但 anchor 保持
/// Down 时的落点不动、只推进 cursor——选区随拖拽扩展（浏览器鼠标拖选语义）。
/// 调用方负责指针仲裁（on_pointer_move 文本臂，仅鼠标指针）。
pub fn on_text_pointer_drag(scene: &mut Scene, id: NodeId, local_x: f32, local_y: f32) {
    let Some(offset) = text_hit_offset(scene, id, local_x, local_y) else {
        return;
    };
    if let Some(
        ControlState::TextField(e)
        | ControlState::TextArea(e)
        | ControlState::NumberField { edit: e, .. },
    ) = scene.controls.get_mut(id)
    {
        e.cursor = offset;
        e.ideal_cursor_valid = false;
        e.cursor_visible = true;
        e.cursor_timer = 0.0;
    }
}

/// 文本控件 content-area-local 坐标 → value 字节偏移的共享命中管线（down 定位 / 拖选共用）。
/// 掩码（layout glyphs 是显示串）与 view_x（光标跟随水平滚动）都在此换算；结果钳到
/// value.len() + char 边界（placeholder measure 的 layout 可能比 value 长，越界会炸
/// 后续 insert_str）。无缓存 layout / 非文本控件 → None。
fn text_hit_offset(scene: &mut Scene, id: NodeId, local_x: f32, local_y: f32) -> Option<usize> {
    let (view_x, value) = match scene.controls.get(id) {
        Some(
            ControlState::TextField(e)
            | ControlState::TextArea(e)
            | ControlState::NumberField { edit: e, .. },
        ) => (e.view_x, e.value.clone()),
        _ => return None,
    };
    // 入参是可视坐标；layout 空间 = 可视 + 视口偏移（光标跟随滚动的命中换算）。
    let local_x = local_x + view_x;
    // 克隆 TextLayout 解借用冲突：text_layouts 不可变借 + controls 可变写。
    let layout = scene.text_layouts[id.index()].as_ref().cloned()?;
    // 缓存 layout 的 glyphs 是显示串（掩码下 ≠ value 字节）——ranges/hit 都在显示串
    // 字节空间，命中后再换算回 value 字节（e.cursor 是 value 偏移）。
    let display = {
        let mask = scene
            .get(id)
            .and_then(|n| n.style.text_security)
            .map(mask_char);
        match scene.controls.get(id) {
            Some(
                ControlState::TextField(e)
                | ControlState::TextArea(e)
                | ControlState::NumberField { edit: e, .. },
            ) => display_value_masked(e, mask).0,
            _ => return None,
        }
    };
    let ranges = line_byte_ranges(&layout, &display);
    let display_off = hit_byte_offset(&layout, &ranges, local_x, local_y);
    // 无掩码/无 composition 时 display == value，换算即恒等。
    let offset = display_to_value_byte(&display, &value, display_off);
    Some(clamp_boundary(&value, offset))
}

/// bool → EventRecord.pad[0] 载荷编码（0=false / 1=true）。语义由 EVT_CHECKED_CHANGED 消费方约定。
fn checked_to_u8(b: bool) -> u8 {
    if b {
        1
    } else {
        0
    }
}

/// 选 Radio：同名组互斥。全树找同 name 的其它 radio 置 checked=false，本 radio 置 true。
/// HTML 语义：radio 按 name 分组（跨 DOM 层级，不限兄弟），同组至多一个选中。
/// 事件只对新选中项产 EVT_CHECKED_CHANGED（pad[0]=1），照 HTML 只对 change 的那一项发。
fn select_radio(scene: &mut Scene, id: NodeId, name: String, out: &mut Vec<EventRecord>) {
    // 先收集同组其它 radio 的 NodeId（避免边遍历边改 HashMap）。
    let others: Vec<NodeId> = scene
        .controls
        .iter()
        .filter_map(|(nid, s)| match s {
            ControlState::Radio { name: n, .. } if nid != id && n == &name => Some(nid),
            _ => None,
        })
        .collect();
    for oid in others {
        if let Some(ControlState::Radio { checked, .. }) = scene.controls.get_mut(oid) {
            *checked = false;
        }
    }
    if let Some(ControlState::Radio { checked, .. }) = scene.controls.get_mut(id) {
        *checked = true;
    }
    out.push(EventRecord {
        node_id: id.0,
        event_type: EVT_CHECKED_CHANGED,
        click_count: 0,
        pad: [checked_to_u8(true), 0],
        touch_id: 0,
        x: 0.0,
        y: 0.0,
        dx: 0.0,
        dy: 0.0,
    });
}

/// Slider pos→value：指针 x 投到 slider 的 layout_rect，映射到 [min,max]，step 量化 + clamp。
/// 新结构无 track 中间层，几何取 slider 自身的 layout_rect（上一帧 solve 写入，1 帧滞后，同
/// hit_test 标准）。宽度退化（≤0）/ 节点非 Slider / min>max（畸形配置，正常路径 instantiate
/// 已 sanitize）→ None（调用方 no-op）。
fn slider_pos_to_value(scene: &Scene, slider: NodeId, pos: [f32; 2]) -> Option<f32> {
    let (min, max, step) = match scene.controls.get(slider)? {
        ControlState::Slider { min, max, step, .. } => (*min, *max, *step),
        _ => return None,
    };
    // 防御：instantiate 已 sanitize min≤max，但 FFI 或外部注入可能破坏不变量。
    // clamp(min,max) 在 min>max 时 panic（FFI 路径不可 panic），此处守卫。
    if min > max {
        return None;
    }
    // 世界 AABB 轨道几何（pos 是世界坐标；页面滚动/缩放下 layout_rect 与世界坐标劈叉，
    // 须经 world_transforms 换算——同 world_rect_contains 的坐标口径。无世界矩阵条目
    // → 根级场景 layout 即世界，旧语义）。
    let n = scene.get(slider)?;
    let r = n.layout_rect;
    if r.w <= 0.0 {
        return None;
    }
    let (x0, x1) = match scene.world_transforms.get(slider.index()).copied() {
        Some(wt) => {
            let (a, _) = crate::transform::apply_point(&wt, 0.0, 0.0);
            let (b, _) = crate::transform::apply_point(&wt, r.w, 0.0);
            (a, b)
        }
        None => (r.x, r.x + r.w),
    };
    if x1 - x0 <= 0.0 {
        return None;
    }
    let ratio = ((pos[0] - x0) / (x1 - x0)).clamp(0.0, 1.0);
    let raw = min + ratio * (max - min);
    let v = if step > 0.0 {
        min + ((raw - min) / step).round() * step
    } else {
        raw
    };
    Some(v.clamp(min, max))
}

/// 写 Slider 的 value（clamp 到 [min,max]，保留 dragging/step）。value 实际变化时产
/// EVT_VALUE_CHANGED（x=新值）—— 用精确 != 防 no-change（同 pos → 同量化值 → 不发误报事件）。
/// 非 Slider / 无槽 → no-op。
fn set_slider_value(scene: &mut Scene, id: NodeId, value: f32, out: &mut Vec<EventRecord>) {
    if let Some(ControlState::Slider {
        value: v, min, max, ..
    }) = scene.controls.get_mut(id)
    {
        // 防御：clamp(min,max) 在 min>max 时 panic。instantiate + FFI setter 已维持
        // min≤max，但此处是 FFI 指针路径下游，纵深守卫保 FFI no-panic 不变量。
        let (lo, hi) = if *min <= *max {
            (*min, *max)
        } else {
            (*max, *min)
        };
        let clamped = value.clamp(lo, hi);
        if *v != clamped {
            *v = clamped;
            out.push(EventRecord {
                node_id: id.0,
                event_type: EVT_VALUE_CHANGED,
                click_count: 0,
                pad: [0, 0],
                touch_id: 0,
                x: clamped,
                y: 0.0,
                dx: 0.0,
                dy: 0.0,
            });
        }
    }
}
