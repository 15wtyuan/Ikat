use crate::css_resolve::unsupported_hint;
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::ir::{IrElement, IrNode, IrNodeKind, IrTree, Span};
use crate::schema::attr::{
    find_structural_attr, is_content_attr, is_global_attr, is_semantic_content_attr,
    AttrValueDomain,
};
use crate::schema::css::{find_css_prop, find_shorthand};
use crate::schema::tag::{
    find_tag, is_known_role, is_shell_tag, known_roles_list, resolve_semantic,
};

/// Run Stage 3 (Fence Gate): validate every element against the schema.
///
/// Checks tag names, attribute names/values, and inline CSS property names.
/// No cross-element checks here (those are Stage 5).
///
/// `line_map` should be the same one built during tree building so that
/// diagnostics carry accurate line/column information.
pub fn run_fence_gate(tree: &IrTree, file: &str, line_map: &LineMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for id in tree.all_element_ids() {
        let node = &tree.nodes[id.0];
        if let IrNodeKind::Element(el) = &node.kind {
            validate_element(el, node, file, line_map, &mut diagnostics);
        }
    }
    diagnostics
}

fn loc(file: &str, offset: usize, line_map: &LineMap) -> SourceLocation {
    line_map.source_location(offset, file.to_string())
}

fn validate_element(
    element: &IrElement,
    node: &IrNode,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tag = element.tag.as_str();

    // 1. Tag name validation -- unknown tag (not in registry, not shell, not custom)
    if !is_shell_tag(tag) && find_tag(tag).is_none() && !tag.contains('-') {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceUnknownTag,
            format!("<{}> is not a recognized fence tag", tag),
            loc(file, node.span.start, line_map),
        ));
        return;
    }

    // 2. Attribute validation
    let tag_spec = find_tag(tag);
    // Resolved control semantic (tag + role), for semantic-scoped content attrs.
    let role = element
        .attributes
        .iter()
        .find(|a| a.name == "role")
        .map(|a| a.value.as_str());
    let aria_multiline = element
        .attributes
        .iter()
        .find(|a| a.name == "aria-multiline")
        .is_some_and(|a| a.value == "true");
    let semantic = resolve_semantic(tag, role, aria_multiline);
    for attr in &element.attributes {
        // Global attrs are always accepted
        if is_global_attr(&attr.name) {
            if attr.name == "style" {
                validate_inline_style(&attr.value, attr.span, file, line_map, diagnostics);
            } else if attr.name == "role" && !is_known_role(&attr.value) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceUnknownRole,
                    format!(
                        "role \"{}\" is not recognized. Known roles: {}. A misspelled role \
                         would silently turn the element into a plain container and skip all \
                         control validations, so the fence rejects it here.",
                        attr.value,
                        known_roles_list()
                    ),
                    loc(file, attr.span.start, line_map),
                ));
            }
            continue;
        }

        // Structural attrs -- validate value against domain
        if let Some(spec) = tag_spec.and_then(|ts| find_structural_attr(ts, &attr.name)) {
            validate_attr_value(
                &attr.name,
                &attr.value,
                &spec.values,
                attr.span,
                file,
                line_map,
                diagnostics,
            );
            continue;
        }

        // Content attrs -- just check name is in the tag's whitelist
        if let Some(ts) = tag_spec {
            if is_content_attr(ts, &attr.name) {
                continue;
            }
        }

        // Semantic-scoped content attrs -- legal only on the role that carries
        // the control semantic (e.g. `value` on role=option), not on the bare tag.
        if let Some(kind) = semantic {
            if is_semantic_content_attr(kind, &attr.name) {
                continue;
            }
        }

        // Unknown attribute
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceUnknownAttr,
            format!("attribute \"{}\" is not recognized on <{}>", attr.name, tag),
            loc(file, attr.span.start, line_map),
        ));
    }
}

fn validate_attr_value(
    name: &str,
    value: &str,
    domain: &AttrValueDomain,
    span: Span,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match domain {
        AttrValueDomain::Enum(allowed) => {
            if !allowed.contains(&value) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceBadAttrValue,
                    format!(
                        "value \"{}\" for attribute \"{}\" is not allowed",
                        value, name
                    ),
                    loc(file, span.start, line_map),
                ));
            }
        }
        AttrValueDomain::IdRef | AttrValueDomain::FreeText | AttrValueDomain::Number => {
            // Stage 5 validates IdRef targets; FreeText and Number pass through here.
        }
    }
}

fn validate_inline_style(
    style: &str,
    span: Span,
    file: &str,
    line_map: &LineMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let prop = decl.split(':').next().unwrap_or("").trim();
        if prop.is_empty() {
            continue;
        }
        if find_css_prop(prop).is_none() && find_shorthand(prop).is_none() {
            let hint = unsupported_hint(prop)
                .unwrap_or("not supported by fence — remove or replace with a supported property.");
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceUnknownCssProp,
                format!("CSS property \"{}\": {}", prop, hint),
                loc(file, span.start, line_map),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::LineMap;
    use crate::tree_builder::{parse_html_to_ir_named, RawParse};

    fn gate(html: &str) -> Vec<Diagnostic> {
        let RawParse { tree, .. } = parse_html_to_ir_named(html, "test.html".into());
        let lm = LineMap::new(html);
        run_fence_gate(&tree, "test.html", &lm)
    }

    #[test]
    fn valid_tags_pass() {
        let diags = gate(r#"<div><span>ok</span></div>"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::diagnostic::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "valid tags should produce no errors: {:?}",
            errors
        );
    }

    #[test]
    fn unknown_tag_reported() {
        let diags = gate(r#"<video></video>"#);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceUnknownTag && d.message.contains("video")));
    }

    #[test]
    fn unknown_attr_reported() {
        let diags = gate(r#"<div bogus-attr="x"></div>"#);
        assert!(diags.iter().any(
            |d| d.code == DiagnosticCode::FenceUnknownAttr && d.message.contains("bogus-attr")
        ));
    }

    #[test]
    fn unknown_role_value_reported() {
        // 拼错的 role 若静默回退成基础标签类型，会跳过全部控件校验——必须报错。
        let diags = gate(r#"<div role="silder"><div data-slot="thumb"></div></div>"#);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceUnknownRole && d.message.contains("silder")));
    }

    #[test]
    fn known_roles_pass_gate() {
        // 注册表全集 + textbox/tabpanel/dialog 表外例外都必须放行。
        let html = "<div role=\"tabpanel\"></div>\
                    <div role=\"dialog\"></div>\
                    <div role=\"textbox\" aria-multiline=\"true\"></div>\
                    <div role=\"combobox\"><div role=\"listbox\">\
                    <div role=\"option\" value=\"en\">English</div></div></div>\
                    <div role=\"slider\"><div data-slot=\"thumb\"></div></div>";
        let diags = gate(html);
        let role_errors: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::FenceUnknownRole)
            .collect();
        assert!(
            role_errors.is_empty(),
            "known roles must pass the gate: {:?}",
            role_errors
        );
    }

    #[test]
    fn option_value_attr_accepted_on_role_option_only() {
        // `value` 是 semantic-scoped 内容属性：只在 role=option 上合法（镜像原生
        // <option value> 语义），普通 div 上仍是未知属性。
        let ok = gate(
            r#"<div role="combobox"><div role="listbox"><div role="option" value="en">English</div></div></div>"#,
        );
        let value_errors: Vec<_> = ok
            .iter()
            .filter(|d| {
                d.severity == crate::diagnostic::Severity::Error
                    && d.code == DiagnosticCode::FenceUnknownAttr
                    && d.message.contains("value")
            })
            .collect();
        assert!(
            value_errors.is_empty(),
            "value on role=option must pass: {value_errors:?}"
        );

        let bad = gate(r#"<div value="x"></div>"#);
        assert!(bad
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceUnknownAttr && d.message.contains("value")));
    }

    #[test]
    fn global_attr_accepted() {
        let diags = gate(r#"<div id="x" class="y" data-z="w" style="color:red"></div>"#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::diagnostic::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "global attrs should be accepted: {:?}",
            errors
        );
    }
}
