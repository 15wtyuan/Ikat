//! Scene 层：持久 Node 树（场景图）。
//!
//! 见 `node` 模块（Node 树数据结构 + Scene::build 建树入口）。

pub mod dynamic;
pub mod node;
pub mod transform;

pub use node::{Node, NodeId, NodeKind, Rect, Scene};
