pub mod annotate;
pub mod consistency_check;
pub mod control_css_check;
pub mod css_resolve;
pub mod css_rules;
pub mod diagnostic;
pub mod fence_gate;
pub mod inline_context_check;
pub mod ir;
pub mod pipeline;
pub mod schema;
pub mod structural;
pub mod tree_builder;

pub use diagnostic::Diagnostic;
pub use ir::{IrElement, IrNode, IrNodeKind, IrTree};
pub use pipeline::{parse_template, ParsedTemplate};
pub use schema::{Category, ContentModel, SemanticKind, TagSpec};
