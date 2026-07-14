// field_reassign_with_default：测试里 `let mut n = Node::default(); n.kind = ..` 比
// struct 字面量更易读（Node 字段多），测试代码统一放行——非生产路径。
// type_complexity：布局/渲染层签名天然复杂（泛型容器组合），pedantic lint 不划算逐个拆 alias。
// too_many_arguments：内部管线 fn（build_render_nodes / batch / tween update）参数由数据流定，重构 param object 收益不抵风险。
// neg_cmp_op_on_partial_ord：浮点 `!(a < b)` 比 partial_cmp 链更直白。
// wrong_self_convention：transform::is_pure_translation(self) 是 trait 签名，改 &self 连锁改 impl/调用，不值。
// doc_lazy_continuation：CJK doc 列表缩进 nit，格式化成本高于收益。
#![allow(
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::wrong_self_convention,
    clippy::doc_lazy_continuation
)]

pub mod asset;
pub mod dump;
pub mod hit;
pub mod input;
pub mod layout;
pub mod render;
pub mod scene;
pub mod scroll;
pub mod stage;
pub mod style;
pub mod text;
pub mod transform;
pub mod tween;

pub use stage::Stage;
