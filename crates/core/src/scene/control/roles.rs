use crate::scene::node::{NodeId, Scene};

/// Dropdown（role=combobox）的弹出列表容器 role。open Dropdown 的 listbox 子树走浮层渲染
/// （render 末尾追加、mask=0 跳出祖先 clip），pub 供 render/hit 层定位 popup 根。
pub const ROLE_LISTBOX: &str = "listbox";
/// listbox 内的列表项 role（作者写 `<div role="option">`，core 现按 NodeKind::OptionItem
/// 识别 option；此常量保留给将来按 role 字符串查询的场景）。
pub const ROLE_OPTION: &str = "option";
/// TabList 容器 role（`role="tablist"`，含 `role=tab` 子节点）。持有 ControlState::TabList
/// { selected_index }，子 tab 的 aria-selected 由 synth_aria_value 合成。
pub const ROLE_TABLIST: &str = "tablist";
/// TabList 内的单个 tab role（`role="tab"`）。带 `aria-controls="<panel-id>"` 指向其关联
/// panel（跨树，非 tablist 子）；sync_control_visuals 据此 find_by_id_attr 解析 panel 切显隐。
pub const ROLE_TAB: &str = "tab";
/// ProgressBar 的填充条 / Slider 的可选视觉填充（`data-slot="fill"`，width:% 由 value 驱动）。
pub const SLOT_FILL: &str = "fill";
/// Slider 的滑块头（`data-slot="thumb"`，位移走 transform，拖拽高频）。
pub const SLOT_THUMB: &str = "thumb";
/// Dropdown 选中项显示区（`data-slot="value"`，内嵌 TextNode 承载选中 option 文本）。
pub const SLOT_VALUE: &str = "value";

/// 在 parent 的直接子节点里按 role 找第一个匹配（基于 RoleTable）。无匹配 / parent 不
/// live → None。
///
/// 控件结构是单层或两层固定深度（combobox → listbox、slider → thumb/fill），只查直接子节点
/// 即可；不递归——防误深入用户内容区（同旧 class 查找的约束）。
/// 需要递归定位的场景（popup 的 listbox 可能被作者裹在一层 wrapper 里）用
/// [`find_child_by_role_recursive`]。
pub fn find_child_by_role(scene: &Scene, parent: NodeId, role: &str) -> Option<NodeId> {
    let children = scene.get(parent)?.children.clone();
    children
        .into_iter()
        .find(|&cid| scene.roles.role_of(cid) == Some(role))
}

/// 在 parent 的子树里按 role 深度优先找第一个匹配（pre-order）。无匹配 / parent 不 live → None。
///
/// 专为 popup listbox 定位：作者可能把 listbox 裹在 wrapper 里（`combobox > wrapper >
/// listbox`），直接子查找会漏，需递归兜底。pre-order 保证优先取最近层匹配。
pub fn find_child_by_role_recursive(scene: &Scene, root: NodeId, role: &str) -> Option<NodeId> {
    // 显式栈 DFS（pre-order）：先把根的直接子节点按声明逆序压栈，pop 时取声明首者先出。
    let mut stack: Vec<NodeId> = scene
        .get(root)?
        .children
        .clone()
        .into_iter()
        .rev()
        .collect();
    while let Some(id) = stack.pop() {
        if scene.roles.role_of(id) == Some(role) {
            return Some(id);
        }
        if let Some(n) = scene.get(id) {
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
    }
    None
}

/// 在 parent 的直接子节点里按 data-slot 找第一个匹配（基于 RoleTable，key 存在即命中）。
///
/// data-slot 映射成 RoleInfo.slots 的 key（值空串占位，见 stage instantiate），故只判 key 是否
/// 存在。无匹配 / parent 不 live → None。同 [`find_child_by_role`]，只查直接子节点不递归。
pub fn find_child_by_slot(scene: &Scene, parent: NodeId, slot: &str) -> Option<NodeId> {
    let children = scene.get(parent)?.children.clone();
    children
        .into_iter()
        .find(|&cid| scene.roles.slot_of(cid, slot).is_some())
}
