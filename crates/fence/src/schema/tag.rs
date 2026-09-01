use super::attr::AttrSpec;

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

/// The `display` value applied when the author does not set one in CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayDefault {
    Block,
    Inline,
    /// Element is invisible to layout (template, etc.).
    None,
}

/// Stable semantic type assigned to an element at annotate time.
/// Determined by tag name + WAI-ARIA `role` (e.g. `<div role="slider">` → Slider),
/// never by CSS class or computed style. See `resolve_semantic`.
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
    /// WAI-ARIA `role="tablist"` — tab 容器（→ NodeKind::TabList）。
    TabList,
    /// WAI-ARIA `role="tab"` — 单个 tab（→ NodeKind::Tab）。无状态，选中态从
    /// 父 TabList.selected_index 派生（aria-selected 是只读 synth，见 ControlState）。
    Tab,
    /// WAI-ARIA `role="tree"`（#8）— 层级列表容器（→ NodeKind::Tree）。子节点是
    /// role=treeitem；嵌套 = treeitem 内直接嵌 treeitem（围栏不认 group 包装层）。
    Tree,
    /// WAI-ARIA `role="treeitem"`（#8）— 树条目（→ NodeKind::TreeItem）。内容模型 =
    /// label 内容 + 可选嵌套 treeitem（有嵌套 = branch 可展开/折叠；无 = leaf）。
    /// 选中态从所属 Tree.selected 派生（跨节点 synth），展开态是自身 ControlState。
    TreeItem,
    /// `<a>` 富文本链接（#74 → NodeKind::Link）。inline 元素，仅 rich-text-block
    /// 上下文合法（rich 外出现围栏报 FenceLinkOutsideRich）；子内容只许文本与
    /// 非 flex span。href 是 opaque 标识符（无 URI 解析语义）。
    Link,
    Template,
    Slot,
    /// Custom element -- tag name contains a hyphen (e.g. `<my-widget>`).
    CustomElement,
}

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

/// WAI-ARIA `role` → `SemanticKind`.
///
/// `textbox` is intentionally absent: `aria-multiline` selects TextArea vs
/// TextField, which a flat value table cannot express, so it is handled inline
/// in [`resolve_semantic`]. `listbox` maps to a plain Container because it is the
/// popup list inside a `combobox` and has no dedicated NodeKind; the runtime
/// addresses it by role. `tabpanel` and `dialog` are likewise intentionally
/// absent: both are plain `<div>` Containers (a panel a tab links to via
/// `aria-controls`; a modal overlay layer), not distinct NodeKinds.
pub const ROLE_TO_SEMANTIC: &[(&str, SemanticKind)] = &[
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
    ("tablist", SemanticKind::TabList),
    ("tab", SemanticKind::Tab),
    ("tree", SemanticKind::Tree),
    ("treeitem", SemanticKind::TreeItem),
];

/// Resolve the [`SemanticKind`] of an element from its tag and, when present,
/// its WAI-ARIA `role`.
///
/// `role` takes precedence over the tag: `<div role="slider">` is a Slider
/// regardless of the `div` tag. Without a role the base tags (`div`/`span`/
/// `button`/`img`/`a`/`template`/`slot`) map to their default kind. Controls and
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
        // Unrecognized role values never reach here in practice: stage 3
        // (fence gate) rejects them with `FenceUnknownRole`. The fallthrough
        // below only covers calls outside the gate pipeline.
    }
    match tag {
        "div" => Some(SemanticKind::Container),
        "span" => Some(SemanticKind::TextElement),
        "button" => Some(SemanticKind::Button),
        "img" => Some(SemanticKind::Image),
        "a" => Some(SemanticKind::Link),
        "template" => Some(SemanticKind::Template),
        "slot" => Some(SemanticKind::Slot),
        _ if tag.contains('-') => Some(SemanticKind::CustomElement),
        _ => None,
    }
}

/// Whether a `role` attribute value is recognized by the fence.
///
/// The role universe is the [`ROLE_TO_SEMANTIC`] registry plus three roles
/// that are intentionally not in it: `textbox` (its TextArea/TextField
/// split needs `aria-multiline`, handled inline in `resolve_semantic`),
/// `tabpanel` (a plain Container a tab links to via `aria-controls`), and
/// `dialog` (a plain Container for a modal overlay layer — a standard
/// WAI-ARIA role AI authors reach for naturally; accepting it as a
/// container beats rejecting standard vocabulary). Stage 3 rejects
/// anything else with `FenceUnknownRole`: a typo'd role would otherwise
/// silently degrade the element to its base-tag type and skip every
/// control validation (required children, CSS hit, structure CSS), which
/// is exactly the silent-degradation the fence exists to prevent.
pub fn is_known_role(role: &str) -> bool {
    role == "textbox"
        || role == "tabpanel"
        || role == "dialog"
        || ROLE_TO_SEMANTIC.iter().any(|(r, _)| *r == role)
}

/// Comma-separated list of every recognized role, for diagnostic messages.
/// Generated from the registry so it cannot drift from the schema.
pub fn known_roles_list() -> String {
    let mut roles: Vec<&str> = ROLE_TO_SEMANTIC.iter().map(|(r, _)| *r).collect();
    roles.push("textbox");
    roles.push("tabpanel");
    roles.push("dialog");
    roles.join(", ")
}

/// Document-shell tags recognised by the parser but not part of the
/// runtime object tree.  They provide structure (html/head/body) or
/// metadata (title/meta/style/link/script) and are consumed during tree build.
pub const SHELL_TAGS: &[&str] = &[
    "html", "head", "body", "title", "meta", "style", "link", "script",
];

pub fn is_shell_tag(name: &str) -> bool {
    SHELL_TAGS.contains(&name)
}

/// All runtime fence tags with full Category × ContentModel mapping.
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
        name: "a",
        semantic: SemanticKind::Link,
        display: DisplayDefault::Inline,
        category: Category::Phrasing,
        // 子内容收窄为 Phrasing，但 Link 专属检查（check_links）进一步只放行
        // 文本与非 flex span——a-in-a / img-in-a 在那拒绝（值域比表更紧）。
        content: ContentModel::Phrasing,
        void: false,
        structural_attrs: &[],
        // href：opaque 链接目标（缺失/trim 空由 check_links 报 FenceLinkHrefRequired）。
        content_attrs: &["href"],
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn resolve_semantic_tablist_tab() {
        // role=tablist/tab → TabList/Tab SemanticKind (WAI-ARIA, TabList).
        assert_eq!(
            resolve_semantic("div", Some("tablist"), false),
            Some(SemanticKind::TabList)
        );
        assert_eq!(
            resolve_semantic("button", Some("tab"), false),
            Some(SemanticKind::Tab)
        );
        // tabpanel 不分派：走 div → Container（panel 靠 aria-controls 关联，不靠 role 分派）。
        assert_eq!(
            resolve_semantic("div", Some("tabpanel"), false),
            Some(SemanticKind::Container)
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

    #[test]
    fn all_runtime_tags_present() {
        let expected = ["div", "span", "button", "img", "a", "template", "slot"];
        // 被移除的标签现在应 not found：旧 block 文本标签 + 旧控件/列表标签。
        for removed in [
            "p", "header", "nav", "ol", "canvas", "strong", "em", "br", "label", "input",
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
