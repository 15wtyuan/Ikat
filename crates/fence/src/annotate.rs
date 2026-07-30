use crate::ir::{IrAttribute, IrNodeKind, IrTree};
use crate::schema::tag::{resolve_semantic, SemanticKind};

/// Run Stage 6 (Annotate): fill in `IrElement.semantic` for all elements.
///
/// Semantics are role-driven: a WAI-ARIA `role` attribute takes precedence and
/// maps to the corresponding `SemanticKind`; without a role, the tag itself maps
/// to a kind. CSS class or computed style never changes the result.
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
            // Transitional: the `input` tag is being retired in favour of
            // `<div role="...">`. Until it leaves the fence, `<input type="...">`
            // (with no explicit role) maps straight to its legacy SemanticKind so
            // existing templates keep resolving to the correct control — including
            // PasswordField/SearchField, which have no WAI-ARIA role and are
            // removed in a dedicated later task. This whole branch is deleted once
            // `input` leaves the fence and authors write `role`.
            el.semantic = if el.tag == "input" && explicit_role.is_none() {
                legacy_input_semantic(&el.attributes)
            } else {
                resolve_semantic(&el.tag, explicit_role, aria_multiline)
            };
        }
    }
}

/// Resolve the SemanticKind of a legacy `<input>` from its `type` attribute.
///
/// The HTML default for a missing `type` is `text`. Unrecognised values also
/// fall back to `text`, matching the prior structural-dispatch behaviour.
fn legacy_input_semantic(attrs: &[IrAttribute]) -> Option<SemanticKind> {
    let input_type = attrs
        .iter()
        .find(|a| a.name == "type")
        .map(|a| a.value.as_str())
        .unwrap_or("text");
    Some(match input_type {
        "range" => SemanticKind::Slider,
        "checkbox" => SemanticKind::Toggle,
        "radio" => SemanticKind::RadioButton,
        "number" => SemanticKind::NumberField,
        "password" => SemanticKind::PasswordField,
        "search" => SemanticKind::SearchField,
        _ => SemanticKind::TextField,
    })
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
            .map(|(k, v)| IrAttribute {
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
    fn legacy_input_type_range_still_resolves_to_slider() {
        // Transitional: `<input type="range">` keeps resolving to Slider while
        // the input tag is being retired.
        let mut tree = build_tree("input", &[("type", "range")]);
        annotate(&mut tree);
        assert_eq!(semantic_at(&tree, IrNodeId(0)), Some(SemanticKind::Slider));
    }

    #[test]
    fn legacy_input_password_and_search_preserved() {
        // password/search have no WAI-ARIA role; they are kept verbatim until a
        // dedicated task removes PasswordField/SearchField.
        let mut tree = build_tree("input", &[("type", "password")]);
        annotate(&mut tree);
        assert_eq!(
            semantic_at(&tree, IrNodeId(0)),
            Some(SemanticKind::PasswordField)
        );
        let mut tree = build_tree("input", &[("type", "search")]);
        annotate(&mut tree);
        assert_eq!(
            semantic_at(&tree, IrNodeId(0)),
            Some(SemanticKind::SearchField)
        );
    }

    #[test]
    fn explicit_role_wins_over_input_type() {
        let mut tree = build_tree("input", &[("type", "text"), ("role", "switch")]);
        annotate(&mut tree);
        assert_eq!(semantic_at(&tree, IrNodeId(0)), Some(SemanticKind::Toggle));
    }
}
