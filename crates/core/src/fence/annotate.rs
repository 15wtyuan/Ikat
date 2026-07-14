#![cfg(feature = "parse")]

use crate::fence::ir::{IrNodeKind, IrTree};
use crate::fence::schema::tag::resolve_semantic;

/// Run Stage 6 (Annotate): fill in `IrElement.semantic` for all elements.
///
/// The semantic kind is determined by tag name and, for `<input>`,
/// the `type` structural attribute. This is immutable: CSS class or
/// computed style never changes it.
pub fn annotate(tree: &mut IrTree) {
    for node in &mut tree.nodes {
        if let IrNodeKind::Element(el) = &mut node.kind {
            let input_type = el
                .attributes
                .iter()
                .find(|a| a.name == "type")
                .map(|a| a.value.as_str());
            el.semantic = resolve_semantic(&el.tag, input_type);
        }
    }
}
