//! Scene 层：持久 Node 树（场景图）。
//!
//! 见 `node` 模块（Node 树数据结构 + Scene::build 建树入口）。

pub mod animation;
pub mod control;
pub mod dynamic;
pub mod node;
pub mod text_cursor;
pub mod transform;

pub use animation::{
    AnimatableProps, KeyframeStop, KeyframeStopSelector, KeyframesRule, TransformAnim,
};
pub use node::{is_whitespace_only_text, Node, NodeId, NodeKind, Rect, Scene};
