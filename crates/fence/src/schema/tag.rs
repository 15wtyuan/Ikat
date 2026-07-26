use super::attr::AttrSpec;

// ── Classification enums ────────────────────────────────────────────

/// Where an element can appear in the tree (HTML "categories" collapsed
/// to the four variants that matter for game UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Void / self-closing (img, br, input).
    Void,
    /// Inline text-level (span, strong, em, label, a, -- .
    Phrasing,
    /// Block-level structural (div, header, nav, p, ul, -- .
    Block,
    /// Transparent -- adopts parent's content model (a, slot).
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
    /// `<input>` before type dispatch -- replaced by the specific kind
    /// during annotation.
    InputDispatch,
    TextField,
    /// `<input type="password">` — split from TextField so attribute selectors can match it.
    PasswordField,
    /// `<input type="search">` — split from TextField so attribute selectors can match it.
    SearchField,
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

/// Resolve the final `SemanticKind` from tag name and, for `<input>`,
/// the `type` attribute.  Returns `None` for tags outside the fence.
pub fn resolve_semantic(tag: &str, input_type: Option<&str>) -> Option<SemanticKind> {
    match tag {
        "div" => Some(SemanticKind::Container),
        "span" => Some(SemanticKind::TextElement),
        "button" => Some(SemanticKind::Button),
        "img" => Some(SemanticKind::Image),
        "input" => match input_type.unwrap_or("text") {
            "text" => Some(SemanticKind::TextField),
            "password" => Some(SemanticKind::PasswordField),
            "search" => Some(SemanticKind::SearchField),
            "number" => Some(SemanticKind::NumberField),
            "range" => Some(SemanticKind::Slider),
            "checkbox" => Some(SemanticKind::Toggle),
            "radio" => Some(SemanticKind::RadioButton),
            _ => None,
        },
        "textarea" => Some(SemanticKind::TextArea),
        "select" => Some(SemanticKind::Dropdown),
        "option" => Some(SemanticKind::OptionItem),
        "progress" => Some(SemanticKind::ProgressBar),
        "ul" => Some(SemanticKind::ListView),
        "li" => Some(SemanticKind::ListItem),
        "template" => Some(SemanticKind::Template),
        "slot" => Some(SemanticKind::Slot),
        _ => {
            if tag.contains('-') {
                Some(SemanticKind::CustomElement)
            } else {
                None
            }
        }
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

/// All 13 runtime fence tags with full Category × ContentModel mapping.
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
        name: "input",
        semantic: SemanticKind::InputDispatch,
        display: DisplayDefault::Inline,
        category: Category::Void,
        content: ContentModel::None,
        void: true,
        structural_attrs: super::attr::INPUT_STRUCTURAL,
        content_attrs: &[
            "value",
            "min",
            "max",
            "step",
            "placeholder",
            "readonly",
            "disabled",
            "checked",
            "name",
            "pattern",
            "maxlength",
        ],
    },
    TagSpec {
        name: "textarea",
        semantic: SemanticKind::TextArea,
        display: DisplayDefault::Inline,
        category: Category::Phrasing,
        content: ContentModel::Text,
        void: false,
        structural_attrs: &[],
        content_attrs: &[
            "placeholder",
            "readonly",
            "disabled",
            "name",
            "rows",
            "cols",
            "maxlength",
        ],
    },
    TagSpec {
        name: "select",
        semantic: SemanticKind::Dropdown,
        display: DisplayDefault::Inline,
        category: Category::Phrasing,
        content: ContentModel::Only(&["option"]),
        void: false,
        structural_attrs: &[],
        content_attrs: &["name", "disabled"],
    },
    TagSpec {
        name: "option",
        semantic: SemanticKind::OptionItem,
        display: DisplayDefault::Block,
        category: Category::Block,
        content: ContentModel::Text,
        void: false,
        structural_attrs: &[],
        content_attrs: &["value", "selected", "disabled"],
    },
    TagSpec {
        name: "progress",
        semantic: SemanticKind::ProgressBar,
        display: DisplayDefault::Inline,
        category: Category::Phrasing,
        content: ContentModel::Phrasing,
        void: false,
        structural_attrs: &[],
        content_attrs: &["value", "max"],
    },
    TagSpec {
        name: "ul",
        semantic: SemanticKind::ListView,
        display: DisplayDefault::Block,
        category: Category::Block,
        content: ContentModel::Only(&["li", "template"]),
        void: false,
        structural_attrs: &[],
        content_attrs: &[],
    },
    TagSpec {
        name: "li",
        semantic: SemanticKind::ListItem,
        display: DisplayDefault::Block,
        category: Category::Block,
        content: ContentModel::Flow,
        void: false,
        structural_attrs: &[],
        content_attrs: &[],
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

    // -- Task 4: resolve_semantic --

    #[test]
    fn resolve_input_types() {
        assert_eq!(
            resolve_semantic("input", None),
            Some(SemanticKind::TextField)
        );
        assert_eq!(
            resolve_semantic("input", Some("text")),
            Some(SemanticKind::TextField)
        );
        assert_eq!(
            resolve_semantic("input", Some("range")),
            Some(SemanticKind::Slider)
        );
        assert_eq!(
            resolve_semantic("input", Some("checkbox")),
            Some(SemanticKind::Toggle)
        );
        assert_eq!(
            resolve_semantic("input", Some("radio")),
            Some(SemanticKind::RadioButton)
        );
        assert_eq!(
            resolve_semantic("input", Some("number")),
            Some(SemanticKind::NumberField)
        );
    }

    #[test]
    fn resolve_input_bogus_type() {
        assert_eq!(resolve_semantic("input", Some("bogus")), None);
    }

    #[test]
    fn resolve_input_password_search_split() {
        assert_eq!(
            resolve_semantic("input", Some("text")),
            Some(SemanticKind::TextField)
        );
        assert_eq!(
            resolve_semantic("input", Some("password")),
            Some(SemanticKind::PasswordField)
        );
        assert_eq!(
            resolve_semantic("input", Some("search")),
            Some(SemanticKind::SearchField)
        );
        assert_eq!(
            resolve_semantic("input", None),
            Some(SemanticKind::TextField)
        ); // 默认 text
    }

    #[test]
    fn resolve_non_input_tags() {
        assert_eq!(resolve_semantic("div", None), Some(SemanticKind::Container));
        assert_eq!(resolve_semantic("button", None), Some(SemanticKind::Button));
        assert_eq!(resolve_semantic("video", None), None);
    }

    // -- Task 5: TAGS registry --

    #[test]
    fn all_runtime_tags_present() {
        let expected = [
            "div", "span", "button", "img", "input", "textarea", "select", "option", "progress",
            "ul", "li", "template", "slot",
        ];
        // 被移除的 10 个标签（p/header/nav/ol/canvas/strong/em/br/label/a）现在应 not found。
        for removed in [
            "p", "header", "nav", "ol", "canvas", "strong", "em", "br", "label", "a",
        ] {
            assert!(find_tag(removed).is_none(), "<{removed}> 应已从围栏移除");
        }
        for name in expected {
            assert!(find_tag(name).is_some(), "<{}> missing from TAGS", name);
        }
    }

    #[test]
    fn shell_tags_recognized() {
        for name in ["html", "head", "body", "title", "meta", "style", "link"] {
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
        assert_eq!(
            find_tag("select").unwrap().content,
            ContentModel::Only(&["option"])
        );
    }
}
