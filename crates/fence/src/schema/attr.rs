/// Attribute value constraint -- how the fence validator checks a given value.
#[derive(Debug, Clone)]
pub enum AttrValueDomain {
    /// Must be one of the listed keywords.
    Enum(&'static [&'static str]),
    /// Must reference an existing element ID in the same template scope.
    IdRef,
    /// Free-form text -- no structural validation.
    FreeText,
    /// Numeric value.
    Number,
}

/// Schema entry for a structural attribute (validated, immutable).
/// Structural attributes influence the element's type or core behaviour
/// and are validated at fence-gate time.
#[derive(Debug, Clone)]
pub struct AttrSpec {
    pub name: &'static str,
    pub values: AttrValueDomain,
    pub required: bool,
}

/// Global attributes accepted on every element.
pub fn is_global_attr(name: &str) -> bool {
    matches!(
        name,
        "id" | "class" | "style" | "slot" | "hidden" | "tabindex" | "role" | "type"
    ) || name.starts_with("aria-")
        || name.starts_with("data-")
        || name.starts_with("--")
}

/// Look up a structural attribute on a tag spec by name.
pub fn find_structural_attr(
    tag_spec: &super::tag::TagSpec,
    attr_name: &str,
) -> Option<&'static AttrSpec> {
    tag_spec
        .structural_attrs
        .iter()
        .find(|a| a.name == attr_name)
}

/// Check whether an attribute name is in the tag's content-attr list.
pub fn is_content_attr(tag_spec: &super::tag::TagSpec, attr_name: &str) -> bool {
    tag_spec.content_attrs.contains(&attr_name)
}

/// Content attributes legal only on elements that resolve to a specific
/// control semantic. Role-based controls (`<div role="option">` etc.) have no
/// literal tag of their own, so their non-global attributes cannot live in a
/// TagSpec (keyed by literal tag). Each entry mirrors the standard-HTML
/// attribute of the same name on the native control; values are free-form
/// text (no value-domain validation).
pub const SEMANTIC_CONTENT_ATTRS: &[(super::tag::SemanticKind, &[&str])] =
    &[(super::tag::SemanticKind::OptionItem, &["value"])];

/// Check an attribute name against the semantic-scoped content-attr table.
/// `semantic` is the element's resolved SemanticKind (tag + role).
pub fn is_semantic_content_attr(semantic: super::tag::SemanticKind, attr_name: &str) -> bool {
    SEMANTIC_CONTENT_ATTRS
        .iter()
        .any(|(kind, attrs)| *kind == semantic && attrs.contains(&attr_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_attrs_recognized() {
        assert!(is_global_attr("id"));
        assert!(is_global_attr("class"));
        assert!(is_global_attr("style"));
        assert!(is_global_attr("data-foo"));
        assert!(is_global_attr("--my-var"));
        assert!(is_global_attr("aria-label"));
        // `type` is a plain global attribute now that input[type] structural
        // dispatch is retired (role drives control semantics instead).
        assert!(is_global_attr("type"));
    }

    #[test]
    fn non_global_attrs() {
        assert!(!is_global_attr("src"));
        assert!(!is_global_attr("value"));
        assert!(!is_global_attr("href"));
    }
}
