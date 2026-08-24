use loomgui_fence::schema::attr::is_global_attr;
use loomgui_fence::schema::css::{find_css_prop, find_shorthand, CssValueParser};
use loomgui_fence::schema::tag::{find_tag, is_shell_tag, Category, ContentModel, DisplayDefault};

/// The 6 runtime fence tags (div/span/button/img/template/slot). Controls and
/// lists have no dedicated tag -- authors express them with `role` on a `div`.
#[test]
fn all_6_runtime_tags_have_specs() {
    let tags = ["div", "span", "button", "img", "template", "slot"];
    for t in tags {
        assert!(find_tag(t).is_some(), "<{t}> must be in TAGS");
    }
    assert_eq!(tags.len(), 6);
}

/// The 8 document-shell tags (html/head/body/title/meta/style/link/script).
/// `script` was previously omitted from the assertion even though it is in
/// SHELL_TAGS; this locks all eight.
#[test]
fn shell_tags_are_eight() {
    let shells = [
        "html", "head", "body", "title", "meta", "style", "link", "script",
    ];
    for s in shells {
        assert!(is_shell_tag(s), "<{s}> should be a shell tag");
    }
    assert_eq!(shells.len(), 8);
    assert_eq!(
        loomgui_fence::schema::tag::SHELL_TAGS.len(),
        8,
        "SHELL_TAGS registry must hold exactly 8 entries"
    );
}

/// Retired control/list tags must be rejected by the fence (find_tag is None).
#[test]
fn removed_tags_rejected() {
    for removed in [
        // old block/text tags (retired earlier)
        "p", "header", "nav", "ol", "canvas", "strong", "em", "br", "label", "a",
        // control/list tags retired in favour of `role`
        "input", "textarea", "select", "option", "progress", "ul", "li",
    ] {
        assert!(
            find_tag(removed).is_none(),
            "<{removed}> must not be in TAGS (retired)"
        );
    }
}

#[test]
fn content_model_table_matches_spec() {
    assert_eq!(find_tag("div").unwrap().content, ContentModel::Flow);
    assert_eq!(find_tag("span").unwrap().content, ContentModel::Phrasing);
    assert_eq!(find_tag("img").unwrap().content, ContentModel::None);
    assert_eq!(find_tag("slot").unwrap().content, ContentModel::Transparent);
}

#[test]
fn display_defaults_match_spec() {
    assert_eq!(find_tag("div").unwrap().display, DisplayDefault::Block);
    assert_eq!(find_tag("span").unwrap().display, DisplayDefault::Inline);
    assert_eq!(find_tag("template").unwrap().display, DisplayDefault::None);
}

#[test]
fn void_elements() {
    assert!(find_tag("img").unwrap().void);
    assert!(!find_tag("div").unwrap().void);
}

#[test]
fn category_table() {
    assert_eq!(find_tag("div").unwrap().category, Category::Block);
    assert_eq!(find_tag("span").unwrap().category, Category::Phrasing);
    assert_eq!(find_tag("img").unwrap().category, Category::Void);
    assert_eq!(find_tag("slot").unwrap().category, Category::Transparent);
}

#[test]
fn css_props_count_and_key_ones() {
    for prop in [
        "width",
        "height",
        "color",
        "background-color",
        "display",
        "flex-direction",
        "padding-top",
        "margin-top",
        "border-color",
        "opacity",
        "overflow-x",
        "transform",
        "font-size",
        "transition",
    ] {
        assert!(
            find_css_prop(prop).is_some(),
            "CSS prop '{}' must be in CSS_PROPS",
            prop
        );
    }
}

#[test]
fn css_grid_not_in_display_keywords() {
    match &find_css_prop("display").unwrap().parser {
        CssValueParser::Keyword(kws) => {
            assert!(!kws.contains(&"grid"));
            assert!(kws.contains(&"block"));
            assert!(kws.contains(&"flex"));
        }
        _ => panic!(),
    }
}

#[test]
fn shorthands_table() {
    assert_eq!(
        find_shorthand("overflow").unwrap().expands_to,
        &["overflow-x", "overflow-y"]
    );
    assert!(find_shorthand("padding").is_some());
    assert!(find_shorthand("background").is_some());
}

#[test]
fn global_attr_detection() {
    assert!(is_global_attr("id"));
    assert!(is_global_attr("data-anything"));
    assert!(is_global_attr("aria-label"));
    // `type` is a plain global attribute (control semantics come from `role`).
    assert!(is_global_attr("type"));
}
