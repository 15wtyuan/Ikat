use crate::input::{EventRecord, EVT_SELECTION_CHANGED, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP};
use crate::scene::node::{ControlState, NodeId, Scene};

use super::roles::ROLE_TAB;

/// 从 `from` 沿 parent 链上溯找最近的 TabList 控件节点。tab 是 tablist 直接子
/// （结构契约），从 tab 或其后代起调一次即命中；限深防环。
fn tablist_owner(scene: &Scene, from: NodeId) -> Option<NodeId> {
    let mut cur = scene.get(from)?.parent?;
    for _ in 0..100_000 {
        let n = scene.get(cur)?;
        if matches!(scene.controls.get(cur), Some(ControlState::TabList { .. })) {
            return Some(cur);
        }
        cur = n.parent?;
    }
    None
}

/// tab 在其所属 TabList 里的 DOM 序（role=tab 直接子按文档序——与键盘路由 /
/// aria-selected 合成同口径）。返回 (所属 tablist, 序号)；上溯无 TabList /
/// 自身不带 tab role → None。
pub fn tab_index(scene: &Scene, tab: NodeId) -> Option<(NodeId, usize)> {
    if scene.roles.role_of(tab) != Some(ROLE_TAB) {
        return None;
    }
    let owner = tablist_owner(scene, tab)?;
    let idx = scene
        .get(owner)?
        .children
        .iter()
        .filter(|&&c| scene.roles.role_of(c) == Some(ROLE_TAB))
        .position(|&c| c == tab)?;
    Some((owner, idx))
}

/// tab 是否为所属 TabList 的当前激活项（合成：序号 == 父 selected_index，
/// 与 aria-selected 派生同源）。非 tab / 上溯无 TabList → None。
pub fn tab_selected(scene: &Scene, tab: NodeId) -> Option<bool> {
    let (owner, idx) = tab_index(scene, tab)?;
    match scene.controls.get(owner) {
        Some(ControlState::TabList { selected_index, .. }) => Some(*selected_index == idx),
        _ => None,
    }
}

/// 设 TabList 的 selected_index，并在净变时发 EVT_SELECTION_CHANGED@tablist（payload
/// touch_id=新 index）。click 命中 tab 与方向键导航共用——仅当 new_index 与当前
/// 值不同时发事件，镜像 [`commit_dropdown_selection`] 的「仅净变才发」语义（HTML change 语义：
/// 点已激活 tab / 方向键移到原位不发 change）。tablist 非 TabList 控件态 → no-op。
pub(super) fn set_tablist_selected_index(
    scene: &mut Scene,
    tablist: NodeId,
    new_index: usize,
    out: &mut Vec<EventRecord>,
) {
    let changed = match scene.controls.get(tablist) {
        Some(ControlState::TabList { selected_index }) => *selected_index != new_index,
        _ => false, // 防御：控件态消失 → 不改、不发
    };
    if let Some(ControlState::TabList { selected_index }) = scene.controls.get_mut(tablist) {
        *selected_index = new_index;
    }
    if changed {
        out.push(EventRecord {
            node_id: tablist.0,
            event_type: EVT_SELECTION_CHANGED,
            click_count: 0,
            pad: [0, 0],
            touch_id: new_index as i32, // payload = 新 selected_index
            x: 0.0,
            y: 0.0,
            dx: 0.0,
            dy: 0.0,
        });
    }
}

/// 从 start 向上找最近的 `ControlState::TabList` 祖先（含 start 自身）。供键盘路由定位
/// TabList：Tab 是 focusable 元素（roving-tabindex-lite，活动 tab 持焦点）、
/// TabList 本身不聚焦，故焦点落在 Tab 上时须向上走到 TabList 才能改 selected_index。
/// 显式限定 TabList 类型（不通用化 find_control_at），避免被其它控件祖先误命中（如包了
/// TabList 的 Dropdown）。panel 跨树（非 TabList 子，靠 aria-controls 关联）→ 从 panel 内容
/// 向上走不会撞到 TabList，故焦点在 panel 内的控件上时不会误触发 TabList 路由。无 → None。
pub(crate) fn find_tablist_ancestor(scene: &Scene, start: Option<NodeId>) -> Option<NodeId> {
    let mut cur = start;
    while let Some(id) = cur {
        if matches!(scene.controls.get(id), Some(ControlState::TabList { .. })) {
            return Some(id);
        }
        cur = scene.get(id).and_then(|n| n.parent);
    }
    None
}

/// TabList 键盘交互路由（automatic-activation）。返回是否消费了该键（消费 → 不发普通 keydown）。
///
/// - 方向键按 TabList 的 `flex-direction` 选轴：row/row-reverse → Left/Right，
///   column/column-reverse → Up/Down；row-reverse/column-reverse 翻转 delta 符号。
/// - clamp 到 `[0, tab_count-1]`，**不 wrap**。
/// - 改变 selected_index 即发 SelectionChanged（automatic-activation：方向键即时提交，
///   与 Dropdown 的 seek 不提交不同——TabList 无展开/提交语义）。
///
/// 非 TabList / 非路由键（含跨轴键，如 row 方向按 Up）/ 0 tab → false（让调用方走普通 keydown）。
/// 由 `process_keys` 在焦点落在 TabList 子树时调用。
pub(crate) fn on_tablist_key(
    scene: &mut Scene,
    tablist: NodeId,
    key_code: u32,
    out: &mut Vec<EventRecord>,
) -> bool {
    // 读当前 selected_index + flex_direction（一次不可变借，释放后再改）。
    let (current, flex_dir) = match scene.get(tablist) {
        Some(n) => {
            let cur = match scene.controls.get(tablist) {
                Some(ControlState::TabList { selected_index }) => *selected_index,
                _ => return false, // 非 TabList 控件态 → 不路由
            };
            (cur, n.style.taffy_style.flex_direction)
        }
        None => return false, // 控件不 live → 不路由
    };
    let delta: i64 = match (flex_dir, key_code) {
        (taffy::FlexDirection::Row, KEY_LEFT) => -1,
        (taffy::FlexDirection::Row, KEY_RIGHT) => 1,
        (taffy::FlexDirection::RowReverse, KEY_LEFT) => 1,
        (taffy::FlexDirection::RowReverse, KEY_RIGHT) => -1,
        (taffy::FlexDirection::Column, KEY_UP) => -1,
        (taffy::FlexDirection::Column, KEY_DOWN) => 1,
        (taffy::FlexDirection::ColumnReverse, KEY_UP) => 1,
        (taffy::FlexDirection::ColumnReverse, KEY_DOWN) => -1,
        _ => return false, // 跨轴键 / 非方向键 → 不路由
    };
    // 按 DOM 序数 role=tab 直接子（与 sync_control_visuals / aria-selected 同口径）。
    let tab_count = scene
        .get(tablist)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| scene.roles.role_of(c) == Some(ROLE_TAB))
        .count();
    if tab_count == 0 {
        return false; // 无 tab → 不消费（让普通 keydown 透传）
    }
    let new = (current as i64 + delta).max(0).min(tab_count as i64 - 1) as usize;
    set_tablist_selected_index(scene, tablist, new, out);
    true
}
