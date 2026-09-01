//! 控件视觉同步：把 ControlState 映射到**作者写的**子节点 inline style。
//!
//! 作者用 WAI-ARIA role + data-slot 自写控件结构：
//! - ProgressBar（role=progressbar）：含 `data-slot="fill"` 子节点（width:% 由 value 驱动）。
//! - Slider（role=slider）：含 `data-slot="fill"`（可选视觉填充）+ `data-slot="thumb"`
//!   （必需，位移走 transform）。fill 与 thumb 是 slider 的兄弟子节点（无 track 中间层）。
//! - Toggle（role=switch）/ RadioButton（role=radio）：无必需子节点——作者用
//!   `[aria-checked="true"]{...}` 属性选择器表达选中态。
//! - Dropdown（role=combobox）：含 `role="listbox"` 子节点（内含 `role="option"` 列表）+
//!   `data-slot="value"` 子节点（显示选中项文本，内嵌 TextNode 承载文本）。
//!
//! core 不注入任何子节点——结构完全由作者掌控，浏览器预览与 Unity 渲染同源。状态→视觉的
//! 桥由 [`sync_control_visuals`] 单向驱动：读 ControlState，按 role/data-slot 定位作者子节点，
//! 写 inline override（HTML 语义最高优先级）。[`find_child_by_role`] / [`find_child_by_slot`]
//! 只查直接子节点（防误深入用户内容区）；popup 的 listbox 可能非直接子，用
//! [`find_child_by_role_recursive`] 兜底。

#[cfg(test)]
mod tests;

mod clipboard;
mod dropdown;
mod edit;
mod pointer;
mod roles;
mod roving;
mod tablist;
mod visuals;

// 路径稳定：crate 内引用点与外部消费者经 `crate::scene::control::X` 取全部导出面
//（pub use glob 只再导出各子模块的 pub 项）。
pub use clipboard::*;
pub use dropdown::*;
pub use edit::*;
pub use pointer::*;
pub use roles::*;
pub(crate) use roving::*;
pub use tablist::*;
pub use visuals::*;

pub(crate) use dropdown::{close_dropdown, on_dropdown_key};
pub(crate) use tablist::{find_tablist_ancestor, on_tablist_key};
