pub mod attr;
pub mod css;
pub mod tag;

// Re-export the most commonly used types for convenience.
pub use attr::{find_structural_attr, is_content_attr, is_global_attr, AttrSpec, AttrValueDomain};
pub use css::{
    find_css_prop, find_shorthand, CssPropSpec, CssValueParser, ShorthandKind, ShorthandSpec,
};
pub use tag::{
    find_tag, is_shell_tag, resolve_semantic, Category, ContentModel, DisplayDefault, SemanticKind,
    TagSpec, ROLE_TO_SEMANTIC, SHELL_TAGS, TAGS,
};
