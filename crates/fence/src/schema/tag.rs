use super::attr::AttrSpec;

// ── Classification enums ────────────────────────────────────────────

/// Where an element can appear in the tree (HTML "categories" collapsed
/// to the four variants that matter for game UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Void / self-closing (img).
    Void,
    /// Inline text-level (span).
    Phrasing,
    /// Block-level structural (div).
    Block,
    /// Transparent -- adopts parent's content model (slot).
    Transparent,
}

/// What children an element accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentModel {
    /// No children at all (void elements, or non-void elements that
    /// semantically reject children).
    None,
    /// Text only, no child elements.
    Text,
    /// Phrasing content (inline elements + text).
    Phrasing,
    /// Flow content (anything -- blocks, inline, text).
    Flow,
    /// Transparent -- accepts whatever its parent accepts.
    Transparent,
    /// Only the listed child tags are allowed.
    Only(&'static [&'static str]),
}

// ── Display default ─────────────────────────────────────────────────

/// The `display` value applied when the author does not set one in CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayDefault {
    Block,
    Inline,
    /// Element is invisible to layout (template, etc.).
    None,
}

// ── SemanticKind ────────────────────────────────────────────────────

/// Stable semantic type assigned to an element at annotate time.
/// Determined by tag name + immutable structural attributes (e.g.
/// `input[type]`), never by CSS class or computed style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticKind {
    Container,
    TextElement,
    Button,
    Image,
    TextField,
    NumberField,
    Slider,
    Toggle,
    RadioButton,
    TextArea,
    Dropdown,
    OptionItem,
    ProgressBar,
    ListView,
    ListItem,
    Template,
    Slot,
    /// Custom element -- tag name contains a hyphen (e.g. `<my-widget>`).
    CustomElement,
}

// ── TagSpec ─────────────────────────────────────────────────────────

/// Compile-time schema entry for one fence tag.
pub struct TagSpec {
    pub name: &'static str,
    pub semantic: SemanticKind,
    pub display: DisplayDefault,
    pub category: Category,
    pub content: ContentModel,
    pub void: bool,
    /// Structural attributes: validated, immutable, influence type/behaviour.
    pub structural_attrs: &'static [AttrSpec],
    /// Content attributes: passthrough initial values (value, src, alt, -- .
    pub content_attrs: &'static [&'static str],
}

// ── resolve_semantic ────────────────────────────────────────────────

/// WAI-ARIA `role` → `SemanticKind`.
///
/// `textbox` is intentionally absent: `aria-multiline` selects TextArea vs
/// TextField, which a flat value table cannot express, so it is handled inline
/// in [`resolve_semantic`]. `listbox` maps to a plain Container because it is the
/// popup list inside a `combobox` and has no dedicated NodeKind; the runtime
/// addresses it by role.
const ROLE_TO_SEMANTIC: &[(&str, SemanticKind)] = &[
    ("combobox", SemanticKind::Dropdown),
    ("option", SemanticKind::OptionItem),
    ("listbox", SemanticKind::Container),
    ("slider", SemanticKind::Slider),
    ("spinbutton", SemanticKind::NumberField),
    ("switch", SemanticKind::Toggle),
    ("radio", SemanticKind::RadioButton),
    ("progressbar", SemanticKind::ProgressBar),
    ("list", SemanticKind::ListView),
    ("listitem", SemanticKind::ListItem),
];

/// Resolve the [`SemanticKind`] of an element from its tag and, when present,
/// its WAI-ARIA `role`.
///
/// `role` takes precedence over the tag: `<div role="slider">` is a Slider
/// regardless of the `div` tag. Without a role the base tags (`div`/`span`/
/// `button`/`img`/`template`/`slot`) map to their default kind. Controls and
/// lists have no dedicated tag -- authors express them with `role` on a `div`
/// (e.g. `<div role="slider">`, `<div role="list">`).
pub fn resolve_semantic(
    tag: &str,
    role: Option<&str>,
    aria_multiline: bool,
) -> Option<SemanticKind> {
    if let Some(r) = role {
        // `textbox` + aria-multiline selects the multi-line variant.
        if r == "textbox" {
            return Some(if aria_multiline {
                SemanticKind::TextArea
            } else {
                SemanticKind::TextField
            });
        }
        if let Some((_, kind)) = ROLE_TO_SEMANTIC.iter().find(|(k, _)| *k == r) {
            return Some(*kind);
        }
        // Unrecognized role: fall through to the tag-based mapping below.
    }
    match tag {
        "div" => Some(SemanticKind::Container),
        "span" => Some(SemanticKind::TextElement),
        "button" => Some(SemanticKind::Button),
        "img" => Some(SemanticKind::Image),
        "template" => Some(SemanticKind::Template),
        "slot" => Some(SemanticKind::Slot),
        _ if tag.contains('-') => Some(SemanticKind::CustomElement),
        _ => None,
    }
}

// ── Shell tags ──────────────────────────────────────────────────────

/// Document-shell tags recognised by the parser but not part of the
/// runtime object tree.  They provide structure (html/head/body) or
/// metadata (title/meta/style/link/script) and are consumed during tree build.
pub const SHELL_TAGS: &[&str] = &[
    "html", "head", "body", "title", "meta", "style", "link", "script",
];

pub fn is_shell_tag(name: &str) -> bool {
    SHELL_TAGS.contains(&name)
}

// ── TAGS registry ───────────────────────────────────────────────────

/// All 6 runtime fence tags with full Category × ContentModel mapping.
pub static TAGS: &[TagSpec] = &[
    TagSpec {
        name: "div",
        semantic: SemanticKind::Container,
        display: DisplayDefault::Block,
        category: Category::Block,
        content: ContentModel::Flow,
        void: false,
        structural_attrs: &[],
        content_attrs: &[],
    },
    TagSpec {
        name: "span",
        semantic: SemanticKind::TextElement,
        display: DisplayDefault::Inline,
        category: Category::Phrasing,
        content: ContentModel::Phrasing,
        void: false,
        structural_attrs: &[],
        content_attrs: &[],
    },
    TagSpec {
        name: "button",
        semantic: SemanticKind::Button,
        display: DisplayDefault::Inline,
        category: Category::Phrasing,
        content: ContentModel::Flow,
        void: false,
        structural_attrs: &[],
        content_attrs: &["disabled"],
    },
    TagSpec {
        name: "img",
        semantic: SemanticKind::Image,
        display: DisplayDefault::Inline,
        category: Category::Void,
        content: ContentModel::None,
        void: true,
        structural_attrs: &[],
        content_attrs: &["src", "alt", "width", "height"],
    },
    TagSpec {
        name: "template",
        semantic: SemanticKind::Template,
        display: DisplayDefault::None,
        category: Category::Phrasing,
        content: ContentModel::Flow,
        void: false,
        structural_attrs: &[],
        content_attrs: &[],
    },
    TagSpec {
        name: "slot",
        semantic: SemanticKind::Slot,
        display: DisplayDefault::Inline,
        category: Category::Transparent,
        content: ContentModel::Transparent,
        void: false,
        structural_attrs: &[],
        content_attrs: &["name"],
    },
];

pub fn find_tag(name: &str) -> Option<&'static TagSpec> {
    TAGS.iter().find(|t| t.name == name)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- resolve_semantic --

    #[test]
    fn resolve_semantic_role_driven() {
        // div + role → control SemanticKind (WAI-ARIA).
        assert_eq!(
            resolve_semantic("div", Some("combobox"), false),
            Some(SemanticKind::Dropdown)
        );
        assert_eq!(
            resolve_semantic("div", Some("slider"), false),
            Some(SemanticKind::Slider)
        );
        assert_eq!(
            resolve_semantic("div", Some("spinbutton"), false),
            Some(SemanticKind::NumberField)
        );
        assert_eq!(
            resolve_semantic("div", Some("switch"), false),
            Some(SemanticKind::Toggle)
        );
        assert_eq!(
            resolve_semantic("div", Some("progressbar"), false),
            Some(SemanticKind::ProgressBar)
        );
        assert_eq!(
            resolve_semantic("div", Some("list"), false),
            Some(SemanticKind::ListView)
        );
        // textbox + aria-multiline selects TextArea vs TextField.
        assert_eq!(
            resolve_semantic("div", Some("textbox"), false),
            Some(SemanticKind::TextField)
        );
        assert_eq!(
            resolve_semantic("div", Some("textbox"), true),
            Some(SemanticKind::TextArea)
        );
        // role takes precedence over the tag.
        assert_eq!(
            resolve_semantic("button", Some("slider"), false),
            Some(SemanticKind::Slider)
        );
        // Base tags without a role.
        assert_eq!(
            resolve_semantic("div", None, false),
            Some(SemanticKind::Container)
        );
        assert_eq!(
            resolve_semantic("span", None, false),
            Some(SemanticKind::TextElement)
        );
        assert_eq!(
            resolve_semantic("button", None, false),
            Some(SemanticKind::Button)
        );
        assert_eq!(
            resolve_semantic("img", None, false),
            Some(SemanticKind::Image)
        );
    }

    #[test]
    fn resolve_semantic_unknown_role_falls_back_and_unknown_tag_is_none() {
        // An unrecognized role falls through to the tag mapping.
        assert_eq!(
            resolve_semantic("div", Some("totally-made-up"), false),
            Some(SemanticKind::Container)
        );
        // Unknown tag with no role is None.
        assert_eq!(resolve_semantic("video", None, false), None);
        // Hyphenated tag → custom element.
        assert_eq!(
            resolve_semantic("my-widget", None, false),
            Some(SemanticKind::CustomElement)
        );
    }

    // -- TAGS registry --

    #[test]
    fn all_runtime_tags_present() {
        let expected = ["div", "span", "button", "img", "template", "slot"];
        // 被移除的标签现在应 not found：旧 block 文本标签 + 旧控件/列表标签。
        for removed in [
            "p", "header", "nav", "ol", "canvas", "strong", "em", "br", "label", "a", "input",
            "textarea", "select", "option", "progress", "ul", "li",
        ] {
            assert!(find_tag(removed).is_none(), "<{removed}> 应已从围栏移除");
        }
        for name in expected {
            assert!(find_tag(name).is_some(), "<{}> missing from TAGS", name);
        }
    }

    #[test]
    fn shell_tags_recognized() {
        for name in [
            "html", "head", "body", "title", "meta", "style", "link", "script",
        ] {
            assert!(is_shell_tag(name), "<{}> should be a shell tag", name);
        }
        assert!(!is_shell_tag("div"));
    }

    #[test]
    fn unknown_tag_not_found() {
        assert!(find_tag("video").is_none());
    }

    #[test]
    fn category_content_model_spot_check() {
        assert_eq!(find_tag("div").unwrap().category, Category::Block);
        assert_eq!(find_tag("div").unwrap().content, ContentModel::Flow);
        assert_eq!(find_tag("span").unwrap().category, Category::Phrasing);
        assert!(find_tag("img").unwrap().void);
        assert_eq!(find_tag("slot").unwrap().content, ContentModel::Transparent);
    }
}
