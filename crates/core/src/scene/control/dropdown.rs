use crate::input::{EventRecord, EVT_SELECTION_CHANGED, KEY_DOWN, KEY_ESCAPE, KEY_RETURN, KEY_UP};
use crate::scene::dynamic::{append_child, remove_child};
use crate::scene::node::{ControlState, NodeFlags, NodeId, NodeKind, Scene};

use super::roles::{find_child_by_role_recursive, ROLE_LISTBOX};

/// 取 combobox（Dropdown）的第 `n` 个 option 的文本内容。
///
/// option 是作者写的 `role="listbox"` 子节点里的 `role="option"`（运行时结构
/// `combobox > [data-slot=value, role=listbox > [role=option...]]`）。先定位 listbox（递归兜底，
/// 作者可能裹 wrapper），再在其直接子节点里按 `NodeKind::OptionItem` 取第 n 个。文本可能在
/// option 自身的 `text_contents`（打包期把 content 存进 side table），也可能在后代 TextNode
/// （`<div role=option><span>B</span></div>`），故递归收集 option 子树所有文本，与 render 的
/// 文本采集口径一致。
///
/// 越界（n 超过 option 数）/ combobox 无 listbox / 无 option → None。调用方据此显空（value 清空）。
pub fn nth_option_text(scene: &Scene, select: NodeId, n: usize) -> Option<String> {
    let popup = find_child_by_role_recursive(scene, select, ROLE_LISTBOX)?;
    let children = scene.get(popup)?.children.clone();
    let opt = children
        .into_iter()
        .filter(|&cid| {
            scene
                .get(cid)
                .is_some_and(|c| c.kind == NodeKind::OptionItem)
        })
        .nth(n)?;
    let mut buf = String::new();
    collect_subtree_text(scene, opt, &mut buf);
    Some(buf)
}

/// 把 combobox 的 `role="option"`（`NodeKind::OptionItem`）直接子节点 reparent 进它的
/// `role="listbox"` 子节点（运行时结构）。
///
/// 必要性：作者正确写法是 `combobox > listbox > option`（option 已在 listbox 内），此时本函数
/// 为 no-op。但若作者把 option 直接写在 combobox 下（打包期结构契约会报缺 listbox 的 error），
/// reparent 作兜底把它们挪进 listbox，保证浮层渲染（render 末尾追加 DFS 从 listbox 根展开子树）
/// 能拿到 option 列表——否则 option 留在 combobox 直接子，会被祖先 `overflow:hidden` 裁掉。
///
/// listbox 用 [`find_child_by_role_recursive`] 定位（作者可能裹 wrapper）。无 listbox 时为 no-op
/// （结构契约报 error，但运行时不 panic）。幂等：option 已在 listbox 里（非 combobox 直接子）
/// 时无 option 可移，为 no-op。由 `Stage::instantiate` 在建树循环后对每个 Dropdown 调一次。
pub fn reparent_options_into_popup(scene: &mut Scene, select: NodeId) {
    // 先定位 listbox（不可变借），再收集 option（不可变借），最后 detach/attach（可变借）。
    // 三阶段分开避免边迭代 select.children 边 mutate 的借用冲突 + 漏项。
    let Some(popup) = find_child_by_role_recursive(scene, select, ROLE_LISTBOX) else {
        return;
    };
    let options: Vec<NodeId> = scene
        .get(select)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&cid| {
            scene
                .get(cid)
                .is_some_and(|c| c.kind == NodeKind::OptionItem)
        })
        .collect();
    for opt in options {
        // move = remove_child（从 select 摘：清 select.children 条目 + option.parent=None）
        //       + append_child（挂到 popup：push popup.children + option.parent=Some(popup)）。
        // 两个 helper 各自维护 children 列表 + parent 指针，不手编列表。option 已确保是
        // select 的直接子节点（filter 取的就是其 children），remove_child 的直系校验必过。
        let _ = remove_child(scene, select, opt);
        let _ = append_child(scene, popup, opt);
    }
}

/// 递归收集 `id` 子树的全部文本：先取节点自身的 text_contents（option 自带 content 的常见路径），
/// 再 DFS 所有子节点。与 render 只渲染 TextNode 的口径一致——非 TextNode 的 text_contents
/// 不参与渲染，但 option 节点自身的 content 是打包期为非 TextNode 叶子存的源文本，这里一并
/// 收（option 几乎不含非 TextNode 子树，叠加不会重复）。
fn collect_subtree_text(scene: &Scene, id: NodeId, buf: &mut String) {
    if let Some(t) = scene.text_contents.get(&id) {
        buf.push_str(t);
    }
    if let Some(n) = scene.get(id) {
        for &c in n.children.clone().iter() {
            collect_subtree_text(scene, c, buf);
        }
    }
}

// option 的 value/selected、tab 的 selected 都不字面存储（HTML 语义：`value` 是
// 打包期静态配置，selected 是父控件 selected_index + 自身序号的合成值）。这里统一
// 提供「上溯找父控件 + 按声明序对位」的派生读，供 FFI getter 调用。

/// 从 `from` 沿 parent 链上溯找最近的 Dropdown 控件节点（结构契约：
/// combobox > listbox > option；从 option 或其后代起调）。限深防环。
fn dropdown_owner(scene: &Scene, from: NodeId) -> Option<NodeId> {
    let mut cur = scene.get(from)?.parent?;
    for _ in 0..100_000 {
        let n = scene.get(cur)?;
        if matches!(scene.controls.get(cur), Some(ControlState::Dropdown { .. })) {
            return Some(cur);
        }
        cur = n.parent?;
    }
    None
}

/// option 在其所属 Dropdown 里的声明序（与 selected_index / `nth_option_text` 同口径：
/// listbox 的 OptionItem 直接子按文档序计数）。返回 (所属 combobox, 序号)；
/// 上溯无 Dropdown / 不在 option 列表内 → None。
pub fn option_index(scene: &Scene, option: NodeId) -> Option<(NodeId, usize)> {
    let owner = dropdown_owner(scene, option)?;
    let idx = dropdown_option_list(scene, owner)
        .iter()
        .position(|&(cid, _)| cid == option)?;
    Some((owner, idx))
}

/// Dropdown 当前选中项的 value：`value` 属性值（打包期静态配置）优先，缺席回落
/// 该项文本（HTML 语义：无 value 的 option 提交其文本）。无选项 / 非 Dropdown → None。
pub fn dropdown_selected_value(scene: &Scene, select: NodeId) -> Option<String> {
    let (selected_index, option_values) = match scene.controls.get(select) {
        Some(ControlState::Dropdown {
            selected_index,
            option_values,
            ..
        }) => (*selected_index, option_values),
        _ => return None,
    };
    if let Some(Some(v)) = option_values.get(selected_index) {
        return Some(v.clone());
    }
    nth_option_text(scene, select, selected_index)
}

/// 单个 option 的 value：同 `dropdown_selected_value` 的 fallback 语义，按 option
/// 自身序号取。非 option / 上溯无 Dropdown → None。
pub fn option_value(scene: &Scene, option: NodeId) -> Option<String> {
    let (owner, idx) = option_index(scene, option)?;
    if let Some(ControlState::Dropdown { option_values, .. }) = scene.controls.get(owner) {
        if let Some(Some(v)) = option_values.get(idx) {
            return Some(v.clone());
        }
    }
    let mut buf = String::new();
    collect_subtree_text(scene, option, &mut buf);
    Some(buf)
}

/// option 是否为所属 Dropdown 的当前选中项（合成：序号 == 父 selected_index）。
/// 非 option / 上溯无 Dropdown → None。
pub fn option_selected(scene: &Scene, option: NodeId) -> Option<bool> {
    let (owner, idx) = option_index(scene, option)?;
    match scene.controls.get(owner) {
        Some(ControlState::Dropdown { selected_index, .. }) => Some(*selected_index == idx),
        _ => None,
    }
}

// option 的索引语义与 `nth_option_text` 一致：在 popup 的 OptionItem 直接子节点里按声明序
// 从 0 计数（非 OptionItem 的 popup 子节点不计入，与 selected_index 对齐）。disabled option
// 占一个索引档位但 seek / 点击不可落地（照 HTML：disabled option 不可交互）。

/// popup 的 OptionItem 直接子节点列表，按声明序，附是否 disabled 标志。
/// 用于键盘 seek（跳 disabled）和点击命中（disabled 不选中）。select 无 popup / 无 option → 空。
pub(crate) fn dropdown_option_list(scene: &Scene, select: NodeId) -> Vec<(NodeId, bool)> {
    let Some(popup) = find_child_by_role_recursive(scene, select, ROLE_LISTBOX) else {
        return Vec::new();
    };
    scene
        .get(popup)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&cid| {
            scene
                .get(cid)
                .is_some_and(|c| c.kind == NodeKind::OptionItem)
        })
        .map(|cid| {
            let disabled = scene
                .get(cid)
                .is_some_and(|n| n.interaction.flags.contains(NodeFlags::DISABLED));
            (cid, disabled)
        })
        .collect()
}

/// 世界坐标 rect-contains：节点盒经 world_transforms 映到世界 AABB 再判。
/// `layout_rect` 是页面内容坐标（未扣祖先滚动），`pos` 是世界坐标（已扣滚动）——
/// 祖先未滚动时两者相等（既有行为），滚动/缩放下必须经世界矩阵换算。
/// 平移/缩放精确；旋转下只映对角两点、非真 AABB（边缘带漏判；hit_test 的逆变换判定
/// 才是精确的）。控件场景无旋转，此近似足够——若未来控件支持旋转，改四角 AABB 或逆变换。
/// 无世界矩阵条目（scene 从未跑 compute_world_transforms，如裸 Scene 单测）→ 回退
/// layout 坐标判定（根级场景 layout 即世界，旧语义）。
pub(crate) fn world_rect_contains(scene: &Scene, node: NodeId, pos: [f32; 2]) -> bool {
    let Some(n) = scene.get(node) else {
        return false;
    };
    let r = n.layout_rect;
    match scene.world_transforms.get(node.index()).copied() {
        Some(wt) => {
            let (x0, y0) = crate::transform::apply_point(&wt, 0.0, 0.0);
            let (x1, y1) = crate::transform::apply_point(&wt, r.w, r.h);
            pos[0] >= x0 && pos[0] <= x1 && pos[1] >= y0 && pos[1] <= y1
        }
        None => pos[0] >= r.x && pos[0] <= r.x + r.w && pos[1] >= r.y && pos[1] <= r.y + r.h,
    }
}

/// 点中 `pos` 所在的**非 disabled** option 的索引（按 OptionItem 序）。pos 不在任一 enabled
/// option 矩形内 / select 无 popup / 无 option → None。世界 AABB 取上一帧 solve+world
/// （与 hit_test 同口径，1 帧滞后），option 互不重叠故 pos-矩形判定与实际 hit 一致。
pub(crate) fn dropdown_option_at_pos(
    scene: &Scene,
    select: NodeId,
    pos: [f32; 2],
) -> Option<usize> {
    let mut idx = 0usize;
    for (cid, disabled) in dropdown_option_list(scene, select) {
        if disabled {
            idx += 1;
            continue;
        }
        if world_rect_contains(scene, cid, pos) {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

/// `pos` 是否落在 popup 矩形内。用于区分「open 时点 header（select 自身区，不在 popup）→
/// toggle 收起」与「open 时点 disabled option / popup 背景 → 不动」（两者 dropdown_option_at_pos
/// 都返 None，但语义不同）。select 无 popup → false。
pub(crate) fn pos_in_popup(scene: &Scene, select: NodeId, pos: [f32; 2]) -> bool {
    let Some(popup) = find_child_by_role_recursive(scene, select, ROLE_LISTBOX) else {
        return false;
    };
    world_rect_contains(scene, popup, pos)
}

/// 提交选中：设 selected_index=idx + value_lock=true（防反馈环）+ open=false + 清
/// open_selected_index，并发 EVT_SELECTION_CHANGED@select（payload touch_id=新 index）。
/// 仅在 idx 与「展开时刻提交值」（open_selected_index；无快照退回现 selected_index）不同时发
/// 事件——键盘 Up/Down 已移动 selected_index 作高亮，Enter 提交时要跟「打开时的原值」比才
/// 能正确报净变（Down 到 B 后 Enter：B != 打开时的 A → 发；未 Down 直接 Enter：A == A → 不发）。
/// 点击路径同理：点 B → B != 打开时的 A → 发；点已选 A → 不发（与 HTML change 语义一致）。
pub(super) fn commit_dropdown_selection(
    scene: &mut Scene,
    select: NodeId,
    idx: usize,
    out: &mut Vec<EventRecord>,
) {
    let prev_committed = match scene.controls.get(select) {
        Some(ControlState::Dropdown {
            open_selected_index,
            selected_index,
            ..
        }) => open_selected_index.unwrap_or(*selected_index),
        _ => idx, // 防御：控件态消失 → 视为无变化（不发）
    };
    let changed = idx != prev_committed;
    if let Some(ControlState::Dropdown {
        selected_index,
        open,
        value_lock,
        open_selected_index,
        ..
    }) = scene.controls.get_mut(select)
    {
        *selected_index = idx;
        *value_lock = true;
        *open = false;
        *open_selected_index = None;
    }
    if changed {
        out.push(EventRecord {
            node_id: select.0,
            event_type: EVT_SELECTION_CHANGED,
            click_count: 0,
            pad: [0, 0],
            touch_id: idx as i32, // payload = 新 selected_index
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
        });
    }
}

/// 展开 Dropdown：open=true + 记 open_selected_index=当前 selected_index（Esc 回滚快照）。
/// 已 open 时为 no-op（防重复记快照覆盖原始值）。
pub(super) fn open_dropdown(scene: &mut Scene, select: NodeId) {
    if let Some(ControlState::Dropdown {
        selected_index,
        open,
        open_selected_index,
        ..
    }) = scene.controls.get_mut(select)
    {
        if !*open {
            *open = true;
            *open_selected_index = Some(*selected_index);
        }
    }
}

/// 收起 Dropdown（取消语义）：open=false + 把 selected_index 回滚到 open_selected_index
/// （展开时刻快照，丢弃键盘导航的未提交高亮）+ 清 open_selected_index。不发事件——
/// 这是一次取消：Up/Down 只移动高亮不提交，未发 SelectionChanged；收起时应还原到展开
/// 时刻的值。所有非提交收起路径都走这里（Esc / header toggle / outside-click），保证取消
/// 语义一致。提交路径（commit_dropdown_selection：Enter / 点 option）保留新 selected_index
/// 并发 SelectionChanged，不经本函数。open/close 无事件常量，host 轮询 `open` 读状态。
pub(crate) fn close_dropdown(scene: &mut Scene, select: NodeId) {
    if let Some(ControlState::Dropdown {
        selected_index,
        open,
        open_selected_index,
        ..
    }) = scene.controls.get_mut(select)
    {
        if let Some(prev) = *open_selected_index {
            *selected_index = prev;
        }
        *open = false;
        *open_selected_index = None;
    }
}

/// Dropdown 键盘交互路由（仅 open 时生效）。返回是否消费了该键（消费 → 不发普通 keydown）。
///
/// - Up/Down：seek 到前一/后一个非 disabled option（移动 selected_index 作高亮，不发事件、
///   不收起；照 RmlUi SeekSelection——从 cur±1 起步，跳过 disabled，越界则不变）。
/// - Enter：提交当前 selected_index + 收起 + 发 SelectionChanged（净变才报）。
/// - Esc：回滚 selected_index 到 open_selected_index（展开时刻快照）+ 收起（不发事件——
///   回滚后净变=0；照 RmlUi CancelSelectBox）。
///
/// 非 open / 非 Dropdown / 非路由键 → false（让调用方走普通 keydown）。由 `process_keys`
/// 在焦点是 open Dropdown 时调用。
pub(crate) fn on_dropdown_key(
    scene: &mut Scene,
    select: NodeId,
    key_code: u32,
    out: &mut Vec<EventRecord>,
) -> bool {
    let is_open = matches!(
        scene.controls.get(select),
        Some(ControlState::Dropdown { open: true, .. })
    );
    if !is_open {
        return false;
    }
    match key_code {
        KEY_UP | KEY_DOWN => {
            let forward = key_code == KEY_DOWN;
            let opts = dropdown_option_list(scene, select);
            let cur = match scene.controls.get(select) {
                Some(ControlState::Dropdown { selected_index, .. }) => *selected_index,
                _ => return true, // 防御：控件态消失 → 消费但不操作
            };
            let n = opts.len();
            if n == 0 {
                return true; // 无 option → 消费但不操作
            }
            let dir: i64 = if forward { 1 } else { -1 };
            let mut i = cur as i64 + dir;
            while i >= 0 && i < n as i64 {
                if !opts[i as usize].1 {
                    if let Some(ControlState::Dropdown { selected_index, .. }) =
                        scene.controls.get_mut(select)
                    {
                        *selected_index = i as usize;
                    }
                    break;
                }
                i += dir;
            }
            true
        }
        KEY_RETURN => {
            let idx = match scene.controls.get(select) {
                Some(ControlState::Dropdown { selected_index, .. }) => *selected_index,
                _ => return true,
            };
            commit_dropdown_selection(scene, select, idx, out);
            true
        }
        KEY_ESCAPE => {
            close_dropdown(scene, select);
            true
        }
        _ => false,
    }
}
