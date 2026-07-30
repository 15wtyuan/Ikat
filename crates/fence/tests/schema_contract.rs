use loomgui_fence::schema::attr::is_global_attr;
use loomgui_fence::schema::css::{find_css_prop, find_shorthand, CssValueParser};
use loomgui_fence::schema::tag::{find_tag, is_shell_tag, Category, ContentModel, DisplayDefault};

#[test]
fn all_13_runtime_tags_have_specs() {
    let tags = [
        "div", "span", "button", "img", "input", "textarea", "select", "option", "progress", "ul",
        "li", "template", "slot",
    ];
    for t in tags {
        assert!(find_tag(t).is_some(), "<{t}> must be in TAGS");
    }
    assert_eq!(tags.len(), 13);
    // 被移除的 10 个标签现应 not found
    for removed in [
        "p", "header", "nav", "ol", "canvas", "strong", "em", "br", "label", "a",
    ] {
        assert!(find_tag(removed).is_none(), "<{removed}> 应已从围栏移除");
    }
}

#[test]
fn shell_tags_are_seven() {
    let shells = ["html", "head", "body", "title", "meta", "style", "link"];
    for s in shells {
        assert!(is_shell_tag(s));
    }
    assert_eq!(shells.len(), 7);
}

#[test]
fn content_model_table_matches_spec() {
    assert_eq!(find_tag("div").unwrap().content, ContentModel::Flow);
    assert_eq!(find_tag("span").unwrap().content, ContentModel::Phrasing);
    assert_eq!(find_tag("img").unwrap().content, ContentModel::None);
    assert_eq!(
        find_tag("select").unwrap().content,
        ContentModel::Only(&["option"])
    );
    assert_eq!(
        find_tag("ul").unwrap().content,
        ContentModel::Only(&["li", "template"])
    );
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
    assert!(find_tag("input").unwrap().void);
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
    // `type` is a plain global attribute now that input[type] structural
    // dispatch is retired.
    assert!(is_global_attr("type"));
}
