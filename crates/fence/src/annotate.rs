use crate::ir::{IrNodeKind, IrTree};
use crate::schema::tag::resolve_semantic;

/// Run Stage 6 (Annotate): fill in `IrElement.semantic` for all elements.
///
/// Semantics are role-driven: a WAI-ARIA `role` attribute takes precedence and
/// maps to the corresponding `SemanticKind`; without a role the tag itself maps
/// to a kind. CSS class or computed style never changes the result. Controls and
/// lists have no dedicated tag -- authors express them with `role` on a `div`.
pub fn annotate(tree: &mut IrTree) {
    for node in &mut tree.nodes {
        if let IrNodeKind::Element(el) = &mut node.kind {
            let explicit_role = el
                .attributes
                .iter()
                .find(|a| a.name == "role")
                .map(|a| a.value.as_str());
            let aria_multiline = el
                .attributes
                .iter()
                .any(|a| a.name == "aria-multiline" && a.value == "true");
            el.semantic = resolve_semantic(&el.tag, explicit_role, aria_multiline);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrElement, IrNodeId, Span};
    use crate::schema::tag::SemanticKind;

    fn build_tree(tag: &str, attrs: &[(&str, &str)]) -> IrTree {
        let mut tree = IrTree::default();
        let attributes = attrs
            .iter()
            .map(|(k, v)| crate::ir::IrAttribute {
                name: (*k).into(),
                value: (*v).into(),
                span: Span::default(),
            })
            .collect();
        tree.push_element(
            IrElement {
                tag: tag.into(),
                attributes,
                semantic: None,
            },
            Span::default(),
            None,
        );
        tree
    }

    fn semantic_at(tree: &IrTree, id: IrNodeId) -> Option<SemanticKind> {
        match &tree.nodes[id.0].kind {
            IrNodeKind::Element(e) => e.semantic,
            _ => unreachable!(),
        }
    }

    #[test]
    fn div_with_role_combobox_is_dropdown() {
        let mut tree = build_tree("div", &[("role", "combobox")]);
        annotate(&mut tree);
        assert_eq!(
            semantic_at(&tree, IrNodeId(0)),
            Some(SemanticKind::Dropdown)
        );
    }

    #[test]
    fn div_role_textbox_aria_multiline_is_textarea() {
        let mut tree = build_tree("div", &[("role", "textbox"), ("aria-multiline", "true")]);
        annotate(&mut tree);
        assert_eq!(
            semantic_at(&tree, IrNodeId(0)),
            Some(SemanticKind::TextArea)
        );
    }

    #[test]
    fn role_takes_precedence_and_unknown_role_falls_back() {
        // role takes precedence over the tag.
        let mut tree = build_tree("button", &[("role", "slider")]);
        annotate(&mut tree);
        assert_eq!(semantic_at(&tree, IrNodeId(0)), Some(SemanticKind::Slider));

        // An unrecognized role falls back to the tag mapping.
        let mut tree = build_tree("div", &[("role", "totally-made-up")]);
        annotate(&mut tree);
        assert_eq!(
            semantic_at(&tree, IrNodeId(0)),
            Some(SemanticKind::Container)
        );
    }
}
