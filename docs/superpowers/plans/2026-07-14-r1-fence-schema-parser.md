# R1 Fence Schema & Parser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a schema-driven fence validator and new HTML parser that replaces the old `scraper`-based `parse/dom.rs`, producing a validated `IrNode` tree annotated with `SemanticKind`.

**Architecture:** Six-stage pipeline (Tokenize → Tree Build → Fence Gate → CSS Resolve → Structural → Annotate) built on a Rust `const` schema (TAGS, ATTRS, CSS_PROPS, CSS_SHORTHANDS). Uses `html5gum` (WHATWG tokenizer) with a custom `CallbackEmitter` callback for ordered attributes and byte-offset spans. Diagnostics collect ALL errors (not fail-fast). Text is a first-class child node, not an `Option<String>` field.

**Tech Stack:** Rust 2021, `html5gum` 0.8 (tokenizer), `cssparser` 0.34 (CSS lexing — retained), `taffy` 0.5 (layout types — retained). New module: `crates/core/src/fence/`.

**Spec:** `docs/superpowers/specs/2026-07-14-r1-fence-schema-parser-design.md`

---

## File Structure

All new code lives under `crates/core/src/fence/`. The old `parse/` module stays untouched during R1 — it will be retired in R2 when the scene builder switches to the new IR.

| File | Responsibility |
|---|---|
| `fence/mod.rs` | Module exports + `ParsedTemplate` struct |
| `fence/ir.rs` | `Span`, `IrNodeId`, `IrTree`, `IrNode`, `IrNodeKind`, `IrElement`, `IrAttribute` |
| `fence/diagnostic.rs` | `Diagnostic`, `Severity`, `DiagnosticCode`, `SourceLocation`, `DiagnosticNote`, `LineMap` |
| `fence/schema/mod.rs` | Schema re-exports + lookup functions |
| `fence/schema/tag.rs` | `TagSpec`, `Category`, `ContentModel`, `DisplayDefault`, `SemanticKind`, `resolve_semantic`, `TAGS`, `SHELL_TAGS` |
| `fence/schema/attr.rs` | `AttrSpec`, `AttrValueDomain`, structural/content/global attr definitions |
| `fence/schema/css.rs` | `CssPropSpec`, `CssValueParser`, `CSS_PROPS`, `ShorthandSpec`, `ShorthandKind`, `CSS_SHORTHANDS` |
| `fence/tree_builder.rs` | `IrToken`, `IrCallback`, `TreeBuilder` — html5gum → IrTree (Stages 1+2) |
| `fence/fence_gate.rs` | Stage 3: per-element tag/attr/css-name validation |
| `fence/css_resolve.rs` | Stage 4: schema-driven CSS declaration application |
| `fence/structural.rs` | Stage 5: content model, ID uniqueness, ARIA refs, template root |
| `fence/annotate.rs` | Stage 6: SemanticKind annotation |
| `fence/pipeline.rs` | `parse_template()` — orchestrates all stages → `ParsedTemplate` |

Test files (new integration tests):

| File | Coverage |
|---|---|
| `crates/core/tests/r1_schema_contract.rs` | Tag/CSS/attr schema positive + negative cases |
| `crates/core/tests/r1_pipeline.rs` | End-to-end pipeline on representative HTML |

**Build commands** (PowerShell):
- Build: `cargo build -p loomgui_core`
- Test single: `cargo test -p loomgui_core --test r1_schema_contract -- --nocapture`
- Test all R1: `cargo test -p loomgui_core --test r1_schema_contract --test r1_pipeline -- --nocapture`
- Check: `cargo check -p loomgui_core --all-targets`

---

### Task 1: Scaffold — Cargo.toml + Module Skeleton

**Files:**
- Modify: `crates/core/Cargo.toml`
- Modify: `crates/core/src/lib.rs`
- Create: `crates/core/src/fence/mod.rs` (+ all subfiles)

- [ ] **Step 1: Add html5gum dependency**

In `crates/core/Cargo.toml`, add to `[dependencies]`:
```toml
html5gum = { version = "0.8", optional = true }
```
Change the `parse` feature:
```toml
parse = ["dep:scraper", "dep:cssparser", "dep:html5gum"]
```
Add test targets:
```toml
[[test]]
name = "r1_schema_contract"
required-features = ["parse"]

[[test]]
name = "r1_pipeline"
required-features = ["parse"]
```

- [ ] **Step 2: Register fence module in lib.rs**

In `crates/core/src/lib.rs`, add after `pub mod parse;`:
```rust
#[cfg(feature = "parse")]
pub mod fence;
```

- [ ] **Step 3: Create module skeleton files**

Create `crates/core/src/fence/mod.rs`:
```rust
#[cfg(feature = "parse")]
pub mod annotate;
#[cfg(feature = "parse")]
pub mod css_resolve;
#[cfg(feature = "parse")]
pub mod diagnostic;
#[cfg(feature = "parse")]
pub mod fence_gate;
#[cfg(feature = "parse")]
pub mod ir;
#[cfg(feature = "parse")]
pub mod pipeline;
#[cfg(feature = "parse")]
pub mod schema;
#[cfg(feature = "parse")]
pub mod structural;
#[cfg(feature = "parse")]
pub mod tree_builder;
```
Create `crates/core/src/fence/schema/mod.rs`:
```rust
pub mod attr;
pub mod css;
pub mod tag;
```
For every other new file (`ir.rs`, `diagnostic.rs`, `tag.rs`, `attr.rs`, `css.rs`, `tree_builder.rs`, `fence_gate.rs`, `css_resolve.rs`, `structural.rs`, `annotate.rs`, `pipeline.rs`), create with placeholder:
```rust
// R1 implementation — filled in by subsequent tasks.
```

- [ ] **Step 4: Verify it compiles**
Run: `cargo build -p loomgui_core`
Expected: Compiles successfully.

- [ ] **Step 5: Commit**
```bash
git add crates/core/Cargo.toml crates/core/src/lib.rs crates/core/src/fence/
git commit -m "r1: scaffold fence module + html5gum dependency"
```

---

### Task 2: IR Types

**Files:**
- Modify: `crates/core/src/fence/ir.rs`

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/ir.rs` with the test first:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_tree() {
        let mut tree = IrTree::default();
        let div = tree.push_element(
            IrElement {
                tag: "div".into(),
                attributes: vec![IrAttribute {
                    name: "class".into(),
                    value: "panel".into(),
                    span: Span { start: 0, end: 20 },
                }],
                semantic: None,
            },
            Span { start: 0, end: 30 },
        );
        let txt = tree.push_text("hello".into(), Span { start: 5, end: 10 }, Some(div));
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.roots, vec![div]);
        assert_eq!(tree.nodes[div.0].children, vec![txt]);
        assert_eq!(tree.nodes[txt.0].parent, Some(div));
    }

    #[test]
    fn text_is_first_class_child() {
        let mut tree = IrTree::default();
        let p = tree.push_element(
            IrElement { tag: "p".into(), attributes: vec![], semantic: None },
            Span::default(),
        );
        let _t1 = tree.push_text("Hello ".into(), Span::default(), Some(p));
        let strong = tree.push_element(
            IrElement { tag: "strong".into(), attributes: vec![], semantic: None },
            Span::default(),
        );
        let _t2 = tree.push_text("world".into(), Span::default(), Some(strong));
        let _t3 = tree.push_text("!".into(), Span::default(), Some(p));
        assert_eq!(tree.nodes[p.0].children.len(), 3);
        assert_eq!(tree.nodes[strong.0].children.len(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::ir -- --nocapture`
Expected: FAIL — types not defined.

- [ ] **Step 3: Write the implementation**
Add above the test module in `ir.rs`:
```rust
/// Byte offset range in source text (start inclusive, end exclusive).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrNodeId(pub usize);

#[derive(Debug, Clone, Default)]
pub struct IrTree {
    pub nodes: Vec<IrNode>,
    pub roots: Vec<IrNodeId>,
}

#[derive(Debug, Clone)]
pub struct IrNode {
    pub kind: IrNodeKind,
    pub span: Span,
    pub parent: Option<IrNodeId>,
    pub children: Vec<IrNodeId>,
}

#[derive(Debug, Clone)]
pub enum IrNodeKind {
    Element(IrElement),
    Text(String),
    Comment(String),
    Doctype { force_quirks: bool },
}

#[derive(Debug, Clone)]
pub struct IrElement {
    pub tag: String,
    pub attributes: Vec<IrAttribute>,
    pub semantic: Option<crate::fence::schema::tag::SemanticKind>,
}

#[derive(Debug, Clone)]
pub struct IrAttribute {
    pub name: String,
    pub value: String,
    pub span: Span,
}

impl IrTree {
    pub fn push_element(&mut self, element: IrElement, span: Span) -> IrNodeId {
        self.push_node(IrNode {
            kind: IrNodeKind::Element(element),
            span,
            parent: None,
            children: Vec::new(),
        })
    }

    pub fn push_text(&mut self, text: String, span: Span, parent: Option<IrNodeId>) -> IrNodeId {
        self.push_node(IrNode {
            kind: IrNodeKind::Text(text),
            span,
            parent,
            children: Vec::new(),
        })
    }

    fn push_node(&mut self, node: IrNode) -> IrNodeId {
        let id = IrNodeId(self.nodes.len());
        let parent = node.parent;
        self.nodes.push(node);
        if let Some(pid) = parent {
            self.nodes[pid.0].children.push(id);
        } else {
            self.roots.push(id);
        }
        id
    }

    pub fn element(&self, id: IrNodeId) -> Option<&IrElement> {
        match &self.nodes[id.0].kind {
            IrNodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, id: IrNodeId) -> Option<&mut IrElement> {
        match &mut self.nodes[id.0].kind {
            IrNodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    /// Iterate all element node IDs (depth-first, roots first).
    /// Used by Stages 3 (Fence Gate), 5 (Structural), and 6 (Annotate).
    pub fn all_element_ids(&self) -> Vec<IrNodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<IrNodeId> = self.roots.iter().copied().collect();
        while let Some(id) = stack.pop() {
            if matches!(self.nodes[id.0].kind, IrNodeKind::Element(_)) {
                out.push(id);
            }
            for child in self.nodes[id.0].children.iter().rev() {
                stack.push(*child);
            }
        }
        out
    }
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::ir -- --nocapture`
Expected: PASS — 2 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/ir.rs
git commit -m "r1: IR types — IrNode, IrTree, IrElement with text as first-class child"
```

---

### Task 3: Diagnostic Types + LineMap

**Files:**
- Modify: `crates/core/src/fence/diagnostic.rs`

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/diagnostic.rs` with the test first:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_map_single_line() {
        let map = LineMap::new("hello world");
        assert_eq!(map.locate(0), (1, 1));
        assert_eq!(map.locate(5), (1, 6));
    }

    #[test]
    fn line_map_multi_line() {
        let map = LineMap::new("ab\ncd\nef");
        assert_eq!(map.locate(0), (1, 1));
        assert_eq!(map.locate(3), (2, 1));
        assert_eq!(map.locate(6), (3, 1));
    }

    #[test]
    fn source_location_has_line_text() {
        let map = LineMap::new("ab\ncd");
        let loc = map.source_location(3, "test.html".into());
        assert_eq!(loc.line, 2);
        assert_eq!(loc.column, 1);
        assert_eq!(loc.source_text, "cd");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::diagnostic -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the implementation**
Add above the test module in `diagnostic.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    FenceUnknownTag, FenceUnknownAttr, FenceUnknownCssProp,
    FenceBadCssValue, FenceBadAttrValue, DuplicateId,
    UnclosedTag, InvalidContentModel, InvalidIdRef,
    InvalidTemplateRoot, UnregisteredCustomElement,
    InvalidAriaRelation, TokenizerError,
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: String,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub source_text: String,
}

#[derive(Debug, Clone)]
pub struct DiagnosticNote {
    pub kind: NoteKind,
    pub text: String,
    pub location: Option<SourceLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind { Help, Note, Related }

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub location: SourceLocation,
    pub notes: Vec<DiagnosticNote>,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, location: SourceLocation) -> Self {
        Self { severity: Severity::Error, code, message: message.into(), location, notes: Vec::new() }
    }
    pub fn with_help(mut self, text: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote { kind: NoteKind::Help, text: text.into(), location: None });
        self
    }
}

#[derive(Debug, Clone)]
pub struct LineMap {
    line_starts: Vec<usize>,
    source: String,
}

impl LineMap {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' { line_starts.push(i + 1); }
        }
        Self { line_starts, source: source.to_string() }
    }

    pub fn locate(&self, offset: usize) -> (u32, u32) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let col = offset.saturating_sub(self.line_starts[line_idx]);
        ((line_idx + 1) as u32, (col + 1) as u32)
    }

    pub fn source_location(&self, offset: usize, file: String) -> SourceLocation {
        let (line, column) = self.locate(offset);
        let source_text = self.source_line(line);
        SourceLocation { file, offset, line, column, source_text }
    }

    fn source_line(&self, line: u32) -> String {
        let idx = (line as usize).saturating_sub(1);
        let start = *self.line_starts.get(idx).unwrap_or(&0);
        let end = self.line_starts.get(idx + 1).copied().unwrap_or(self.source.len());
        self.source[start..end].trim_end_matches('\n').trim_end_matches('\r').to_string()
    }
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::diagnostic -- --nocapture`
Expected: PASS — 3 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/diagnostic.rs
git commit -m "r1: diagnostic types + LineMap for offset-to-line/col conversion"
```

---

### Task 4: Schema Enums + resolve_semantic

**Files:**
- Modify: `crates/core/src/fence/schema/tag.rs`

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/schema/tag.rs` with:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_input_types() {
        assert_eq!(resolve_semantic("input", None), Some(SemanticKind::TextField));
        assert_eq!(resolve_semantic("input", Some("text")), Some(SemanticKind::TextField));
        assert_eq!(resolve_semantic("input", Some("range")), Some(SemanticKind::Slider));
        assert_eq!(resolve_semantic("input", Some("checkbox")), Some(SemanticKind::Toggle));
        assert_eq!(resolve_semantic("input", Some("radio")), Some(SemanticKind::RadioButton));
        assert_eq!(resolve_semantic("input", Some("number")), Some(SemanticKind::NumberField));
    }

    #[test]
    fn resolve_input_bogus_type() {
        assert_eq!(resolve_semantic("input", Some("bogus")), None);
    }

    #[test]
    fn resolve_non_input_tags() {
        assert_eq!(resolve_semantic("div", None), Some(SemanticKind::Container));
        assert_eq!(resolve_semantic("button", None), Some(SemanticKind::Button));
        assert_eq!(resolve_semantic("video", None), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::schema::tag -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the implementation**
Add above the test module in `tag.rs`:
```rust
use super::attr::AttrSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category { Void, Phrasing, Block, Transparent }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentModel {
    None, Text, Phrasing, Flow, Transparent,
    Only(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayDefault { Block, Inline, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticKind {
    Container, TextBlock, TextElement, LineBreak, Label, Button, Link,
    Image, Canvas, InputDispatch, TextField, NumberField, Slider, Toggle,
    RadioButton, TextArea, Dropdown, OptionItem, ProgressBar, ListView,
    ListItem, Template, Slot, CustomElement,
}

pub struct TagSpec {
    pub name: &'static str,
    pub semantic: Option<SemanticKind>,
    pub display: DisplayDefault,
    pub category: Category,
    pub content: ContentModel,
    pub void: bool,
    pub structural_attrs: &'static [AttrSpec],
    pub content_attrs: &'static [&'static str],
}

pub fn resolve_semantic(tag: &str, input_type: Option<&str>) -> Option<SemanticKind> {
    match tag {
        "div" | "header" | "nav" => Some(SemanticKind::Container),
        "p" => Some(SemanticKind::TextBlock),
        "span" | "strong" | "em" => Some(SemanticKind::TextElement),
        "br" => Some(SemanticKind::LineBreak),
        "label" => Some(SemanticKind::Label),
        "button" => Some(SemanticKind::Button),
        "a" => Some(SemanticKind::Link),
        "img" => Some(SemanticKind::Image),
        "canvas" => Some(SemanticKind::Canvas),
        "input" => match input_type.unwrap_or("text") {
            "text" | "password" | "search" => Some(SemanticKind::TextField),
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
        "ul" | "ol" => Some(SemanticKind::ListView),
        "li" => Some(SemanticKind::ListItem),
        "template" => Some(SemanticKind::Template),
        "slot" => Some(SemanticKind::Slot),
        _ => if tag.contains('-') { Some(SemanticKind::CustomElement) } else { None },
    }
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::schema::tag -- --nocapture`
Expected: PASS — 3 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/schema/tag.rs
git commit -m "r1: schema enums (Category, ContentModel, DisplayDefault, SemanticKind) + resolve_semantic"
```

---

### Task 5: TagSpec Registry (TAGS + SHELL_TAGS)

**Files:**
- Modify: `crates/core/src/fence/schema/tag.rs`

- [ ] **Step 1: Write the failing test**
Add to the test module in `tag.rs`:
```rust
    #[test]
    fn all_runtime_tags_present() {
        let expected = [
            "div", "header", "nav", "p", "span", "strong", "em", "br",
            "label", "button", "a", "img", "canvas", "input", "textarea",
            "select", "option", "progress", "ul", "ol", "li", "template", "slot",
        ];
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
        assert_eq!(find_tag("img").unwrap().void, true);
        assert_eq!(find_tag("select").unwrap().content, ContentModel::Only(&["option"]));
    }
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::schema::tag -- --nocapture`
Expected: FAIL — `find_tag`/`TAGS` not defined.

- [ ] **Step 3: Write the implementation**
Add after `resolve_semantic` in `tag.rs`:
```rust
pub const SHELL_TAGS: &[&str] = &["html", "head", "body", "title", "meta", "style", "link"];

pub fn is_shell_tag(name: &str) -> bool {
    SHELL_TAGS.contains(&name)
}

pub static TAGS: &[TagSpec] = &[
    TagSpec { name: "div",      semantic: Some(SemanticKind::Container),    display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Flow,        void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "header",   semantic: Some(SemanticKind::Container),    display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Flow,        void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "nav",      semantic: Some(SemanticKind::Container),    display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Flow,        void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "p",        semantic: Some(SemanticKind::TextBlock),    display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Phrasing,    void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "span",     semantic: Some(SemanticKind::TextElement),  display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Phrasing,    void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "strong",   semantic: Some(SemanticKind::TextElement),  display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Phrasing,    void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "em",       semantic: Some(SemanticKind::TextElement),  display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Phrasing,    void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "br",       semantic: Some(SemanticKind::LineBreak),    display: DisplayDefault::Inline, category: Category::Void,        content: ContentModel::None,        void: true,  structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "label",    semantic: Some(SemanticKind::Label),        display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Phrasing,    void: false, structural_attrs: &super::attr::LABEL_STRUCTURAL, content_attrs: &[] },
    TagSpec { name: "button",   semantic: Some(SemanticKind::Button),       display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Phrasing,    void: false, structural_attrs: &[],          content_attrs: &["disabled"] },
    TagSpec { name: "a",        semantic: Some(SemanticKind::Link),         display: DisplayDefault::Inline, category: Category::Transparent, content: ContentModel::Transparent, void: false, structural_attrs: &super::attr::A_STRUCTURAL,     content_attrs: &[] },
    TagSpec { name: "img",      semantic: Some(SemanticKind::Image),        display: DisplayDefault::Inline, category: Category::Void,        content: ContentModel::None,        void: true,  structural_attrs: &[],          content_attrs: &["src", "alt", "width", "height"] },
    TagSpec { name: "canvas",   semantic: Some(SemanticKind::Canvas),       display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Flow,        void: false, structural_attrs: &[],          content_attrs: &["width", "height"] },
    TagSpec { name: "input",    semantic: Some(SemanticKind::InputDispatch), display: DisplayDefault::Inline, category: Category::Void,       content: ContentModel::None,        void: true,  structural_attrs: &super::attr::INPUT_STRUCTURAL, content_attrs: &["value", "min", "max", "step", "placeholder", "readonly", "disabled", "checked", "name", "pattern", "maxlength"] },
    TagSpec { name: "textarea", semantic: Some(SemanticKind::TextArea),     display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Text,        void: false, structural_attrs: &[],          content_attrs: &["placeholder", "readonly", "disabled", "name", "rows", "cols", "maxlength"] },
    TagSpec { name: "select",   semantic: Some(SemanticKind::Dropdown),     display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Only(&["option"]), void: false, structural_attrs: &[],   content_attrs: &["name", "disabled"] },
    TagSpec { name: "option",   semantic: Some(SemanticKind::OptionItem),   display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Text,        void: false, structural_attrs: &[],          content_attrs: &["value", "selected", "disabled"] },
    TagSpec { name: "progress", semantic: Some(SemanticKind::ProgressBar),  display: DisplayDefault::Inline, category: Category::Phrasing,    content: ContentModel::Phrasing,    void: false, structural_attrs: &[],          content_attrs: &["value", "max"] },
    TagSpec { name: "ul",       semantic: Some(SemanticKind::ListView),     display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Only(&["li", "template"]), void: false, structural_attrs: &[], content_attrs: &[] },
    TagSpec { name: "ol",       semantic: Some(SemanticKind::ListView),     display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Only(&["li", "template"]), void: false, structural_attrs: &[], content_attrs: &[] },
    TagSpec { name: "li",       semantic: Some(SemanticKind::ListItem),     display: DisplayDefault::Block,  category: Category::Block,       content: ContentModel::Flow,        void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "template", semantic: Some(SemanticKind::Template),     display: DisplayDefault::None,   category: Category::Phrasing,    content: ContentModel::Flow,        void: false, structural_attrs: &[],          content_attrs: &[] },
    TagSpec { name: "slot",     semantic: Some(SemanticKind::Slot),         display: DisplayDefault::Inline, category: Category::Transparent, content: ContentModel::Transparent, void: false, structural_attrs: &[],          content_attrs: &["name"] },
];

pub fn find_tag(name: &str) -> Option<&'static TagSpec> {
    TAGS.iter().find(|t| t.name == name)
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::schema::tag -- --nocapture`
Expected: PASS — 7 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/schema/tag.rs
git commit -m "r1: TAGS registry — 23 runtime tags with full Category×ContentModel mapping"
```

---

### Task 6: Attribute Schema

**Files:**
- Modify: `crates/core/src/fence/schema/attr.rs`

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/schema/attr.rs` with:
```rust
#![cfg(feature = "parse")]

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
    }

    #[test]
    fn non_global_attrs() {
        assert!(!is_global_attr("src"));
        assert!(!is_global_attr("type"));
    }

    #[test]
    fn input_type_values() {
        let spec = &INPUT_STRUCTURAL[0];
        assert_eq!(spec.name, "type");
        match &spec.values {
            AttrValueDomain::Enum(vals) => {
                assert!(vals.contains(&"range"));
                assert!(vals.contains(&"text"));
            }
            _ => panic!("expected Enum"),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::schema::attr -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the implementation**
Add above the test module in `attr.rs`:
```rust
#[derive(Debug, Clone)]
pub struct AttrSpec {
    pub name: &'static str,
    pub values: AttrValueDomain,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub enum AttrValueDomain {
    Enum(&'static [&'static str]),
    IdRef,
    FreeText,
    Number,
}

pub static INPUT_STRUCTURAL: &[AttrSpec] = &[
    AttrSpec { name: "type", values: AttrValueDomain::Enum(&["range", "checkbox", "radio", "text", "password", "number", "search"]), required: false },
];

pub static LABEL_STRUCTURAL: &[AttrSpec] = &[
    AttrSpec { name: "for", values: AttrValueDomain::IdRef, required: false },
];

pub static A_STRUCTURAL: &[AttrSpec] = &[
    AttrSpec { name: "href", values: AttrValueDomain::FreeText, required: false },
];

pub fn is_global_attr(name: &str) -> bool {
    matches!(name, "id" | "class" | "style" | "slot" | "hidden" | "tabindex" | "role")
      || name.starts_with("aria-")
      || name.starts_with("data-")
      || name.starts_with("--")
}

pub fn find_structural_attr(tag_spec: &super::tag::TagSpec, attr_name: &str) -> Option<&'static AttrSpec> {
    tag_spec.structural_attrs.iter().find(|a| a.name == attr_name)
}

pub fn is_content_attr(tag_spec: &super::tag::TagSpec, attr_name: &str) -> bool {
    tag_spec.content_attrs.contains(&attr_name)
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::schema::attr -- --nocapture`
Expected: PASS — 3 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/schema/attr.rs
git commit -m "r1: attribute schema — structural attrs + global attr detection"
```

---

### Task 7: CSS Schema (CSS_PROPS + CSS_SHORTHANDS)

**Files:**
- Modify: `crates/core/src/fence/schema/css.rs`

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/schema/css.rs` with:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_css_props() {
        assert!(find_css_prop("width").is_some());
        assert!(find_css_prop("color").is_some());
        assert!(find_css_prop("display").is_some());
    }

    #[test]
    fn unknown_css_props() {
        assert!(find_css_prop("grid-template-columns").is_none());
        assert!(find_css_prop("cursor").is_none());
    }

    #[test]
    fn display_excludes_grid() {
        match &find_css_prop("display").unwrap().parser {
            CssValueParser::Keyword(kws) => assert!(!kws.contains(&"grid")),
            _ => panic!("expected Keyword"),
        }
    }

    #[test]
    fn shorthands_resolve() {
        assert!(find_shorthand("padding").is_some());
        assert!(find_shorthand("overflow").is_some());
        assert!(find_shorthand("background").is_some());
    }

    #[test]
    fn non_shorthand_returns_none() {
        assert!(find_shorthand("width").is_none());
    }

    #[test]
    fn overflow_is_replicate() {
        let sh = find_shorthand("overflow").unwrap();
        assert_eq!(sh.kind, ShorthandKind::Replicate);
        assert_eq!(sh.expands_to, &["overflow-x", "overflow-y"]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::schema::css -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the implementation**
Add above the test module in `css.rs`:
```rust
#[derive(Debug)]
pub struct CssPropSpec {
    pub name: &'static str,
    pub default: &'static str,
    pub inherited: bool,
    pub parser: CssValueParser,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CssValueParser {
    Keyword(&'static [&'static str]),
    Length, LengthPercent, LengthPercentAuto, Color, Number, Integer,
    FourSidedPx, FourSidedMargin, BorderRadius, Transform, Overflow,
    Filter, BoxShadow, TextShadow, Transition, Gradient2,
    TextEffect, TextStroke, BackgroundClipText, Url, Raw,
}

#[derive(Debug)]
pub struct ShorthandSpec {
    pub name: &'static str,
    pub expands_to: &'static [&'static str],
    pub kind: ShorthandKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShorthandKind {
    Box, Replicate, FallThrough, BorderShorthand, BackgroundShorthand,
}

pub static CSS_PROPS: &[CssPropSpec] = &[
    CssPropSpec { name: "width",         default: "auto",     inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "height",        default: "auto",     inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "min-width",     default: "auto",     inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "min-height",    default: "auto",     inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "max-width",     default: "auto",     inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "max-height",    default: "auto",     inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "padding-top",    default: "0",    inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "padding-right",  default: "0",    inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "padding-bottom", default: "0",    inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "padding-left",   default: "0",    inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "margin-top",     default: "0",    inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "margin-right",   default: "0",    inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "margin-bottom",  default: "0",    inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "margin-left",    default: "0",    inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "display",           default: "block",      inherited: false, parser: CssValueParser::Keyword(&["block", "flex", "none", "inline"]) },
    CssPropSpec { name: "flex-direction",    default: "row",        inherited: false, parser: CssValueParser::Keyword(&["row", "row-reverse", "column", "column-reverse"]) },
    CssPropSpec { name: "flex-wrap",         default: "nowrap",     inherited: false, parser: CssValueParser::Keyword(&["nowrap", "wrap", "wrap-reverse"]) },
    CssPropSpec { name: "justify-content",   default: "flex-start", inherited: false, parser: CssValueParser::Keyword(&["flex-start", "center", "flex-end", "space-between", "space-around", "space-evenly"]) },
    CssPropSpec { name: "align-items",       default: "stretch",    inherited: false, parser: CssValueParser::Keyword(&["flex-start", "center", "flex-end", "stretch", "baseline"]) },
    CssPropSpec { name: "align-content",     default: "stretch",    inherited: false, parser: CssValueParser::Keyword(&["flex-start", "center", "flex-end", "stretch", "space-between", "space-around", "space-evenly"]) },
    CssPropSpec { name: "align-self",        default: "auto",       inherited: false, parser: CssValueParser::Keyword(&["auto", "flex-start", "center", "flex-end", "stretch", "baseline"]) },
    CssPropSpec { name: "flex-grow",         default: "0",          inherited: false, parser: CssValueParser::Number },
    CssPropSpec { name: "flex-shrink",       default: "1",          inherited: false, parser: CssValueParser::Number },
    CssPropSpec { name: "flex-basis",        default: "auto",       inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "gap",               default: "0",          inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "row-gap",           default: "0",          inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "column-gap",        default: "0",          inherited: false, parser: CssValueParser::Length },
    CssPropSpec { name: "position",          default: "relative",   inherited: false, parser: CssValueParser::Keyword(&["absolute", "relative"]) },
    CssPropSpec { name: "top",               default: "auto",       inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "right",             default: "auto",       inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "bottom",            default: "auto",       inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "left",              default: "auto",       inherited: false, parser: CssValueParser::LengthPercentAuto },
    CssPropSpec { name: "aspect-ratio",      default: "auto",       inherited: false, parser: CssValueParser::Number },
    CssPropSpec { name: "order",             default: "0",          inherited: false, parser: CssValueParser::Integer },
    CssPropSpec { name: "border-color",      default: "transparent", inherited: false, parser: CssValueParser::Color },
    CssPropSpec { name: "border-radius",     default: "0",           inherited: false, parser: CssValueParser::BorderRadius },
    CssPropSpec { name: "border-image-slice",default: "none",        inherited: false, parser: CssValueParser::FourSidedPx },
    CssPropSpec { name: "background-color",  default: "transparent", inherited: false, parser: CssValueParser::Color },
    CssPropSpec { name: "background-image",  default: "none",        inherited: false, parser: CssValueParser::Url },
    CssPropSpec { name: "background-size",   default: "stretch",     inherited: false, parser: CssValueParser::Keyword(&["cover", "contain", "100%", "stretch"]) },
    CssPropSpec { name: "background-clip",   default: "border-box",  inherited: false, parser: CssValueParser::Keyword(&["border-box", "padding-box", "content-box", "text"]) },
    CssPropSpec { name: "-webkit-background-clip", default: "border-box", inherited: false, parser: CssValueParser::Keyword(&["border-box", "padding-box", "content-box", "text"]) },
    CssPropSpec { name: "opacity",           default: "1",           inherited: false, parser: CssValueParser::Number },
    CssPropSpec { name: "overflow-x",        default: "visible",     inherited: false, parser: CssValueParser::Overflow },
    CssPropSpec { name: "overflow-y",        default: "visible",     inherited: false, parser: CssValueParser::Overflow },
    CssPropSpec { name: "color",             default: "#000000",     inherited: true,  parser: CssValueParser::Color },
    CssPropSpec { name: "box-shadow",        default: "none",        inherited: false, parser: CssValueParser::BoxShadow },
    CssPropSpec { name: "pointer-events",    default: "auto",        inherited: false, parser: CssValueParser::Keyword(&["auto", "none"]) },
    CssPropSpec { name: "transform",         default: "none",        inherited: false, parser: CssValueParser::Transform },
    CssPropSpec { name: "filter",            default: "none",        inherited: false, parser: CssValueParser::Filter },
    CssPropSpec { name: "font-size",         default: "16px",        inherited: true,  parser: CssValueParser::Length },
    CssPropSpec { name: "font-family",       default: "inherit",     inherited: true,  parser: CssValueParser::Raw },
    CssPropSpec { name: "font-weight",       default: "400",         inherited: true,  parser: CssValueParser::Integer },
    CssPropSpec { name: "text-align",        default: "left",        inherited: true,  parser: CssValueParser::Keyword(&["left", "center", "right"]) },
    CssPropSpec { name: "line-height",       default: "0",           inherited: true,  parser: CssValueParser::Number },
    CssPropSpec { name: "letter-spacing",    default: "0",           inherited: true,  parser: CssValueParser::Length },
    CssPropSpec { name: "white-space",       default: "normal",      inherited: true,  parser: CssValueParser::Keyword(&["normal", "nowrap"]) },
    CssPropSpec { name: "text-shadow",       default: "none",        inherited: true,  parser: CssValueParser::TextShadow },
    CssPropSpec { name: "-webkit-text-stroke", default: "0 transparent", inherited: true, parser: CssValueParser::TextStroke },
    CssPropSpec { name: "font-effect",       default: "none",        inherited: true,  parser: CssValueParser::TextEffect },
    CssPropSpec { name: "transition",        default: "none",        inherited: false, parser: CssValueParser::Transition },
];

pub static CSS_SHORTHANDS: &[ShorthandSpec] = &[
    ShorthandSpec { name: "padding",       expands_to: &["padding-top", "padding-right", "padding-bottom", "padding-left"], kind: ShorthandKind::Box },
    ShorthandSpec { name: "margin",        expands_to: &["margin-top", "margin-right", "margin-bottom", "margin-left"],     kind: ShorthandKind::Box },
    ShorthandSpec { name: "overflow",      expands_to: &["overflow-x", "overflow-y"], kind: ShorthandKind::Replicate },
    ShorthandSpec { name: "border",        expands_to: &["border-color"],             kind: ShorthandKind::BorderShorthand },
    ShorthandSpec { name: "border-width",  expands_to: &[],                            kind: ShorthandKind::Box },
    ShorthandSpec { name: "border-top",    expands_to: &[],                            kind: ShorthandKind::FallThrough },
    ShorthandSpec { name: "border-right",  expands_to: &[],                            kind: ShorthandKind::FallThrough },
    ShorthandSpec { name: "border-bottom", expands_to: &[],                           kind: ShorthandKind::FallThrough },
    ShorthandSpec { name: "border-left",   expands_to: &[],                            kind: ShorthandKind::FallThrough },
    ShorthandSpec { name: "background",    expands_to: &[],                            kind: ShorthandKind::BackgroundShorthand },
    ShorthandSpec { name: "flex",          expands_to: &["flex-grow", "flex-shrink", "flex-basis"], kind: ShorthandKind::FallThrough },
];

pub fn find_css_prop(name: &str) -> Option<&'static CssPropSpec> {
    CSS_PROPS.iter().find(|p| p.name == name)
}

pub fn find_shorthand(name: &str) -> Option<&'static ShorthandSpec> {
    CSS_SHORTHANDS.iter().find(|s| s.name == name)
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::schema::css -- --nocapture`
Expected: PASS — 6 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/schema/css.rs
git commit -m "r1: CSS schema — CssPropSpec + ShorthandSpec registries"
```

---

### Task 8: Schema Lookup Convenience Functions

**Files:**
- Modify: `crates/core/src/fence/schema/mod.rs`

This task adds re-exports and convenience wrappers so callers don't need to navigate the full module path.

- [ ] **Step 1: Write the implementation**
Replace `crates/core/src/fence/schema/mod.rs`:
```rust
pub mod attr;
pub mod css;
pub mod tag;

// Re-export the most commonly used types for convenience.
pub use attr::{is_global_attr, find_structural_attr, is_content_attr, AttrSpec, AttrValueDomain};
pub use css::{find_css_prop, find_shorthand, CssPropSpec, CssValueParser, ShorthandSpec, ShorthandKind};
pub use tag::{find_tag, is_shell_tag, resolve_semantic, Category, ContentModel, DisplayDefault, SemanticKind, TagSpec, TAGS, SHELL_TAGS};
```

- [ ] **Step 2: Verify it compiles**
Run: `cargo build -p loomgui_core`
Expected: PASS.

- [ ] **Step 3: Commit**
```bash
git add crates/core/src/fence/schema/mod.rs
git commit -m "r1: schema re-exports and convenience lookups"
```

---

### Task 9: Tree Builder (Stages 1+2: Tokenize → IrTree)

**Files:**
- Modify: `crates/core/src/fence/tree_builder.rs`

This is the core of R1: html5gum's WHATWG tokenizer produces a token stream, our `IrCallback` (implementing `Callback`) translates events into `IrToken`s with ordered attributes and spans, and `TreeBuilder` consumes the token stream to build an `IrTree`.

Key design decisions (per spec):
- No implicit auto-close — explicit closing tags required. Unclosed tags produce `UnclosedTag` diagnostics.
- Void elements (declared in schema) don't need closing tags.
- Text is a first-class child `IrNodeKind::Text(String)`, not a field on the parent.
- Attributes preserve source order (Vec, not HashMap).
- html5gum handles entity decoding, tag/attribute lexing. We handle tree structure.
- `naive_next_state` is enabled for correct `<style>`/`<textarea>`/`<title>` content tokenization.

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/tree_builder.rs` with the test:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_div_with_text() {
        let (tree, diags) = parse_html_to_ir(r#"<div>hello</div>"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert_eq!(tree.roots.len(), 1);
        let div = tree.roots[0];
        assert_eq!(tree.element(div).unwrap().tag, "div");
        assert_eq!(tree.nodes[div.0].children.len(), 1);
        match &tree.nodes[tree.nodes[div.0].children[0].0].kind {
            IrNodeKind::Text(t) => assert_eq!(t, "hello"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn nested_structure() {
        let (tree, diags) = parse_html_to_ir(r#"<div><span>x</span></div>"#);
        assert!(diags.is_empty());
        let div = tree.roots[0];
        let span_id = tree.nodes[div.0].children[0];
        assert_eq!(tree.element(span_id).unwrap().tag, "span");
    }

    #[test]
    fn void_element_no_close_tag() {
        let (tree, diags) = parse_html_to_ir(r#"<div><img src="a.png"></div>"#);
        assert!(diags.is_empty(), "img is void — no closing tag needed");
        let div = tree.roots[0];
        assert_eq!(tree.nodes[div.0].children.len(), 1);
    }

    #[test]
    fn attributes_preserve_order() {
        let (tree, _) = parse_html_to_ir(r#"<div id="x" class="y" data-z="w"></div>"#);
        let el = tree.element(tree.roots[0]).unwrap();
        assert_eq!(el.attributes.len(), 3);
        assert_eq!(el.attributes[0].name, "id");
        assert_eq!(el.attributes[1].name, "class");
        assert_eq!(el.attributes[2].name, "data-z");
    }

    #[test]
    fn unclosed_tag_produces_diagnostic() {
        let (tree, diags) = parse_html_to_ir("<div><span>text</div>");
        assert!(!diags.is_empty(), "unclosed <span> should produce a diagnostic");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnclosedTag));
    }

    #[test]
    void body_wrapper_extracted() {
        let (tree, diags) = parse_html_to_ir(
            r#"<html><head></head><body><div>hi</div></body></html>"#
        );
        assert!(diags.is_empty());
        assert_eq!(tree.roots.len(), 1);
        assert_eq!(tree.element(tree.roots[0]).unwrap().tag, "div");
    }

    #[test]
    fn rich_text_inline_mix() {
        let (tree, diags) = parse_html_to_ir(
            r#"<p>Hello <strong>world</strong>!</p>"#
        );
        assert!(diags.is_empty());
        let p = tree.roots[0];
        // p should have 3 children: Text, strong, Text
        assert_eq!(tree.nodes[p.0].children.len(), 3);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::tree_builder -- --nocapture`
Expected: FAIL — `parse_html_to_ir` not defined.

- [ ] **Step 3: Write the implementation**
Add above the test module in `tree_builder.rs`:
```rust
use crate::fence::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::fence::ir::{IrAttribute, IrElement, IrNode, IrNodeId, IrNodeKind, IrTree, Span};
use crate::fence::schema::tag::{find_tag, is_shell_tag};
use html5gum::emitters::callback::{Callback, CallbackEmitter, CallbackEvent};
use html5gum::{Tokenizer, Span as GumSpan};

/// Token produced by our html5gum callback — consumed by the tree builder.
pub enum IrToken {
    StartTag {
        name: String,
        attributes: Vec<IrAttribute>,
        self_closing: bool,
        span: Span,
    },
    EndTag {
        name: String,
        span: Span,
    },
    String {
        text: String,
        span: Span,
    },
    Comment {
        text: String,
        span: Span,
    },
    Error {
        message: String,
        span: Span,
    },
}

// ── html5gum callback ──

struct PendingTag {
    name: String,
    attributes: Vec<IrAttribute>,
    start_span: Span,
}

#[derive(Default)]
struct IrCallback {
    pending_tag: Option<PendingTag>,
    current_attr: Option<(String, String, usize)>, // (name, value, name_start_offset)
}

impl Callback<IrToken, usize> for IrCallback {
    fn handle_event(&mut self, event: CallbackEvent<'_>, span: GumSpan<usize>) -> Option<IrToken> {
        let s = Span { start: span.start, end: span.end };
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.pending_tag = Some(PendingTag {
                    name: String::from_utf8_lossy(name).into_owned(),
                    attributes: Vec::new(),
                    start_span: s,
                });
                None
            }
            CallbackEvent::AttributeName { name } => {
                self.current_attr = Some((
                    String::from_utf8_lossy(name).into_owned(),
                    String::new(),
                    span.start,
                ));
                None
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some((_, val, _)) = &mut self.current_attr {
                    *val = String::from_utf8_lossy(value).into_owned();
                }
                None
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                // Flush pending attribute
                if let Some((name, value, name_start)) = self.current_attr.take() {
                    if let Some(tag) = &mut self.pending_tag {
                        tag.attributes.push(IrAttribute {
                            name,
                            value,
                            span: Span { start: name_start, end: span.end },
                        });
                    }
                }
                let mut tag = self.pending_tag.take()?;
                tag.name.make_ascii_lowercase();
                Some(IrToken::StartTag {
                    name: tag.name,
                    attributes: tag.attributes,
                    self_closing,
                    span: Span { start: tag.start_span.start, end: span.end },
                })
            }
            CallbackEvent::EndTag { name } => {
                let mut name = String::from_utf8_lossy(name).into_owned();
                name.make_ascii_lowercase();
                Some(IrToken::EndTag { name, span: s })
            }
            CallbackEvent::String { value } => {
                Some(IrToken::String {
                    text: String::from_utf8_lossy(value).into_owned(),
                    span: s,
                })
            }
            CallbackEvent::Comment { value } => {
                Some(IrToken::Comment {
                    text: String::from_utf8_lossy(value).into_owned(),
                    span: s,
                })
            }
            CallbackEvent::Doctype { .. } => None, // ignored in R1
            CallbackEvent::Error(error) => {
                Some(IrToken::Error {
                    message: error.as_str().to_string(),
                    span: s,
                })
            }
        }
    }
}

/// Tokenize HTML using html5gum with our custom callback.
pub fn tokenize(html: &str) -> Vec<IrToken> {
    let mut emitter = CallbackEmitter::new(IrCallback::default());
    emitter.naively_switch_states(true);
    Tokenizer::new_with_emitter(html, emitter).collect()
}

// ── Tree builder ──

struct TreeBuilder {
    tree: IrTree,
    stack: Vec<IrNodeId>,
    diagnostics: Vec<Diagnostic>,
    line_map: LineMap,
    file: String,
    // Track body depth: when > 0, elements are runtime; otherwise they're shell.
    in_body: bool,
}

impl TreeBuilder {
    fn new(html: &str, file: String) -> Self {
        Self {
            tree: IrTree::default(),
            stack: Vec::new(),
            diagnostics: Vec::new(),
            line_map: LineMap::new(html),
            file,
            in_body: false,
        }
    }

    fn loc(&self, offset: usize) -> crate::fence::diagnostic::SourceLocation {
        self.line_map.source_location(offset, self.file.clone())
    }

    fn current_parent(&self) -> Option<IrNodeId> {
        self.stack.last().copied()
    }

    fn is_runtime_context(&self) -> bool {
        // If we're inside body (or there's no html/body wrapper at all),
        // elements go to the runtime tree.
        self.in_body || self.stack.is_empty()
    }

    fn process_token(&mut self, token: IrToken) {
        match token {
            IrToken::StartTag { name, attributes, self_closing, span } => {
                self.handle_start_tag(name, attributes, self_closing, span);
            }
            IrToken::EndTag { name, span } => {
                self.handle_end_tag(name, span);
            }
            IrToken::String { text, span } => {
                if self.is_runtime_context() && !text.is_empty() {
                    let parent = self.current_parent();
                    if self.stack.is_empty() || self.in_body {
                        self.tree.push_text(text, span, parent);
                    }
                }
            }
            IrToken::Comment { text, span: _ } => {
                // Comments preserved in tree but not in runtime roots.
                // For R1, just ignore comments.
                let _ = text;
            }
            IrToken::Error { message, span } => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::TokenizerError,
                    format!("HTML tokenizer error: {}", message),
                    self.loc(span.start),
                ));
            }
        }
    }

    fn handle_start_tag(&mut self, name: String, attributes: Vec<IrAttribute>, self_closing: bool, span: Span) {
        // Track body entry
        if name == "body" {
            self.in_body = true;
            return; // body itself is not a runtime node
        }
        if name == "html" || name == "head" {
            // Shell wrappers — push to stack so children attach correctly,
            // but don't create runtime nodes.
            // We use a synthetic approach: push a marker.
            // For simplicity, we skip html/head entirely and let body's children
            // become roots.
            return;
        }
        if is_shell_tag(&name) && !self.in_body {
            // meta, title, style, link in head — skip for now (CSS extraction in pipeline)
            return;
        }

        // Normal element
        let element = IrElement { tag: name.clone(), attributes, semantic: None };
        let node = IrNode {
            kind: IrNodeKind::Element(element),
            span,
            parent: self.current_parent(),
            children: Vec::new(),
        };
        let id = IrNodeId(self.tree.nodes.len());
        self.tree.nodes.push(node);

        let parent = self.current_parent();
        if let Some(pid) = parent {
            self.tree.nodes[pid.0].children.push(id);
        } else {
            self.tree.roots.push(id);
        }

        // Determine if this tag needs a closing tag
        let is_void = find_tag(&name).map(|s| s.void).unwrap_or(false);
        if !is_void && !self_closing {
            self.stack.push(id);
        }
    }

    fn handle_end_tag(&mut self, name: String, span: Span) {
        if name == "body" {
            self.in_body = false;
            return;
        }
        if name == "html" || name == "head" || (is_shell_tag(&name) && !self.in_body) {
            return;
        }

        // Find matching open tag on the stack (search from top)
        let pos = self.stack.iter().rev().position(|&id| {
            self.tree.element(id).map(|e| e.tag == name).unwrap_or(false)
        });

        match pos {
            Some(depth_from_top) => {
                let stack_len = self.stack.len();
                let match_idx = stack_len - 1 - depth_from_top;

                // Report unclosed tags above the match
                for &id in &self.stack[match_idx + 1..] {
                    let el = self.tree.element(id);
                    let tag_name = el.map(|e| e.tag.as_str()).unwrap_or("unknown");
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnclosedTag,
                        format!("<{}> was not explicitly closed before </{}>", tag_name, name),
                        self.loc(self.tree.nodes[id.0].span.start),
                    ));
                }

                self.stack.truncate(match_idx);
            }
            None => {
                // Stray end tag — report but don't crash
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnclosedTag,
                    format!("stray </{}> with no matching open tag", name),
                    self.loc(span.start),
                ));
            }
        }
    }

    fn finish(mut self) -> (IrTree, Vec<Diagnostic>) {
        // Any remaining open tags are unclosed
        for &id in self.stack.iter().rev() {
            let el = self.tree.element(id);
            let tag_name = el.map(|e| e.tag.as_str()).unwrap_or("unknown");
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnclosedTag,
                format!("<{}> was not closed before end of input", tag_name),
                self.loc(self.tree.nodes[id.0].span.start),
            ));
        }
        (self.tree, self.diagnostics)
    }
}

/// Parse HTML source into an IR tree with diagnostics.
/// This is Stage 1 (Tokenize) + Stage 2 (Tree Build).
pub fn parse_html_to_ir(html: &str) -> (IrTree, Vec<Diagnostic>) {
    parse_html_to_ir_named(html, "<inline>".to_string())
}

/// Same as `parse_html_to_ir` but with a file name for diagnostics.
pub fn parse_html_to_ir_named(html: &str, file: String) -> (IrTree, Vec<Diagnostic>) {
    let tokens = tokenize(html);
    let mut builder = TreeBuilder::new(html, file);
    for token in tokens {
        builder.process_token(token);
    }
    builder.finish()
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::tree_builder -- --nocapture`
Expected: PASS — 7 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/tree_builder.rs
git commit -m "r1: tree builder — html5gum CallbackEmitter → IrTree with ordered attrs + spans"
```

---

### Task 10: Fence Gate (Stage 3)

**Files:**
- Modify: `crates/core/src/fence/fence_gate.rs`

Stage 3 validates each element independently against the schema: tag names, attribute names, and inline CSS property names. No cross-element checks here (those are Stage 5).

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/fence_gate.rs` with:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fence::tree_builder::parse_html_to_ir;

    #[test]
    fn valid_tags_pass() {
        let (tree, _) = parse_html_to_ir(r#"<div><span>ok</span></div>"#);
        let diags = run_fence_gate(&tree, "test.html");
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "valid tags should produce no errors: {:?}", errors);
    }

    #[test]
    fn unknown_tag_reported() {
        let (tree, _) = parse_html_to_ir(r#"<video></video>"#);
        let diags = run_fence_gate(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::FenceUnknownTag
            && d.message.contains("video")));
    }

    #[test]
    fn unknown_attr_reported() {
        let (tree, _) = parse_html_to_ir(r#"<div bogus-attr="x"></div>"#);
        let diags = run_fence_gate(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::FenceUnknownAttr
            && d.message.contains("bogus-attr")));
    }

    #[test]
    fn global_attr_accepted() {
        let (tree, _) = parse_html_to_ir(r#"<div id="x" class="y" data-z="w" style="color:red"></div>"#);
        let diags = run_fence_gate(&tree, "test.html");
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "global attrs should be accepted: {:?}", errors);
    }

    #[test]
    fn bad_input_type_reported() {
        let (tree, _) = parse_html_to_ir(r#"<input type="bogus">"#);
        let diags = run_fence_gate(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::FenceBadAttrValue));
    }

    #[test]
    fn valid_input_type_accepted() {
        let (tree, _) = parse_html_to_ir(r#"<input type="range">"#);
        let diags = run_fence_gate(&tree, "test.html");
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "type=range is valid: {:?}", errors);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::fence_gate -- --nocapture`
Expected: FAIL — `run_fence_gate` not defined.

- [ ] **Step 3: Write the implementation**
Add above the test module in `fence_gate.rs`:
```rust
use crate::fence::diagnostic::{Diagnostic, DiagnosticCode, LineMap, Severity, SourceLocation};
use crate::fence::ir::{IrNode, IrNodeKind, IrTree};
use crate::fence::schema::attr::{find_structural_attr, is_content_attr, is_global_attr, AttrValueDomain};
use crate::fence::schema::css::{find_css_prop, find_shorthand};
use crate::fence::schema::tag::{find_tag, is_shell_tag};

/// Run Stage 3 (Fence Gate): validate every element against the schema.
/// Checks tag names, attribute names/values, and inline CSS property names.
/// Returns diagnostics (may be empty).
pub fn run_fence_gate(tree: &IrTree, file: &str) -> Vec<Diagnostic> {
    let line_map = LineMap::new(""); // spans are byte offsets into original source;
    // For R1 the line_map is built in tree_builder; here we reconstruct locations
    // from node spans. A proper implementation passes the original source.
    // For now, we use offset-only locations.
    let mut diagnostics = Vec::new();
    for id in tree.all_element_ids() {
        let node = &tree.nodes[id.0];
        if let IrNodeKind::Element(el) = &node.kind {
            validate_element(el.tag.as_str(), el, node, file, &mut diagnostics);
        }
    }
    diagnostics
}

fn validate_element(
    tag: &str,
    element: &crate::fence::ir::IrElement,
    node: &IrNode,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let loc = SourceLocation {
        file: file.to_string(),
        offset: node.span.start,
        line: 0,
        column: 0,
        source_text: String::new(),
    };

    // 1. Tag name validation
    if !is_shell_tag(tag) && find_tag(tag).is_none() && !tag.contains('-') {
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceUnknownTag,
            format!("围栏外元素 <{}>，不在支持的标准 HTML 子集内", tag),
            loc,
        ).with_help("使用围栏内标签，或注册自定义元素（名称含 '-'）"));
        return; // Can't validate attrs on unknown tag meaningfully
    }

    // 2. Attribute validation
    let tag_spec = find_tag(tag);
    for attr in &element.attributes {
        // Global attrs are always OK
        if is_global_attr(&attr.name) {
            // Validate inline style CSS property names
            if attr.name == "style" {
                validate_inline_style(&attr.value, &attr.span, file, diagnostics);
            }
            continue;
        }

        // Structural attrs
        if let Some(spec) = tag_spec.and_then(|ts| find_structural_attr(ts, &attr.name)) {
            validate_attr_value(&attr.name, &attr.value, &spec.values, &attr.span, file, diagnostics);
            continue;
        }

        // Content attrs
        if let Some(ts) = tag_spec {
            if is_content_attr(ts, &attr.name) {
                continue;
            }
        }

        // Unknown attr
        diagnostics.push(Diagnostic::error(
            DiagnosticCode::FenceUnknownAttr,
            format!("属性 \"{}\" 不在 <{}> 的围栏内", attr.name, tag),
            SourceLocation { file: file.to_string(), offset: attr.span.start, line: 0, column: 0, source_text: String::new() },
        ));
    }
}

fn validate_attr_value(
    name: &str,
    value: &str,
    domain: &AttrValueDomain,
    span: &crate::fence::ir::Span,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match domain {
        AttrValueDomain::Enum(allowed) => {
            if !allowed.contains(&value.as_str()) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceBadAttrValue,
                    format!("属性 \"{}\" 的值 \"{}\" 不在允许列表内", name, value),
                    SourceLocation { file: file.to_string(), offset: span.start, line: 0, column: 0, source_text: String::new() },
                ));
            }
        }
        AttrValueDomain::IdRef | AttrValueDomain::FreeText | AttrValueDomain::Number => {
 // Stage 5 validates IdRef targets; others pass through.
        }
    }
}

fn validate_inline_style(
    style: &str,
    span: &crate::fence::ir::Span,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Parse inline style declarations and check property names against schema.
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() { continue; }
        let prop = decl.split(':').next().unwrap_or("").trim();
        if prop.is_empty() { continue; }
        if find_css_prop(prop).is_none() && find_shorthand(prop).is_none() {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceUnknownCssProp,
                format!("CSS 属性 \"{}\" 不在围栏内", prop),
                SourceLocation { file: file.to_string(), offset: span.start, line: 0, column: 0, source_text: String::new() },
            ));
        }
    }
}

Note: `all_element_ids()` was already defined in Task 2 as part of `IrTree`. Do NOT re-define it here.

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::fence_gate -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/fence_gate.rs
git commit -m "r1: fence gate (Stage 3)"
```
```

---

### Task 11: CSS Resolve (Stage 4)

**Files:**
- Modify: `crates/core/src/fence/css_resolve.rs`

Stage 4 resolves CSS declarations into `ResolvedStyle` per node, using the schema to validate property names and values. The existing free functions from `style/mapping.rs` (parse_four, parse_color, parse_lp, etc.) are reused as parser backends.

For R1, this stage does not do full cascade (matching CSS rules from `<style>` blocks). It only handles inline `style` attributes and applies the schema-driven value validation. Full cascade integration is deferred to when the pipeline replaces the old `resolve_styles`.

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/css_resolve.rs` with:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fence::tree_builder::parse_html_to_ir;

    #[test]
    fn inline_style_applies_color() {
        let (tree, _) = parse_html_to_ir(r#"<div style="color:#ff0000"></div>"#);
        let styles = resolve_inline_styles(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn display_block_overrides_default() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:block"></div>"#);
        let styles = resolve_inline_styles(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].display_mode, crate::style::resolved::DisplayMode::Block);
    }

    #[test]
    fn display_grid_reports_error() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:grid"></div>"#);
        let (_, diags) = resolve_inline_styles_with_diags(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == crate::fence::diagnostic::DiagnosticCode::FenceBadCssValue));

    #[test]
    fn flex_defaults_to_row_direction() {
        // Per spec: display:flex should default to flex-direction:row
        let (tree, _) = parse_html_to_ir(r#"<div style="display:flex"></div>"#);
        let styles = resolve_inline_styles(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].taffy_style.flex_direction, taffy::FlexDirection::Row);
    }

    #[test]
    fn explicit_flex_direction_preserved() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:flex; flex-direction:column"></div>"#);
        let styles = resolve_inline_styles(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].taffy_style.flex_direction, taffy::FlexDirection::Column);
    }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::css_resolve -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the implementation**
Add above the test module in `css_resolve.rs`:
```rust
use crate::fence::diagnostic::{Diagnostic, DiagnosticCode, SourceLocation};
use crate::fence::ir::{IrNodeKind, IrTree};
use crate::fence::schema::css::{find_css_prop, find_shorthand, CssValueParser};
use crate::fence::schema::tag::{find_tag, DisplayDefault};
use crate::style::mapping::apply_decl;
use crate::style::resolved::{DisplayMode, ResolvedStyle};

/// Resolve inline styles for all nodes in the tree.
/// Returns one `ResolvedStyle` per node, in node-index order.
/// Uses the existing `apply_decl` for value application, but validates
/// property names against the CSS schema first.
pub fn resolve_inline_styles(tree: &IrTree) -> Vec<ResolvedStyle> {
    resolve_inline_styles_with_diags(tree, "<inline>").0
}

/// Resolve inline styles, also returning diagnostics for invalid CSS.
pub fn resolve_inline_styles_with_diags(
    tree: &IrTree,
    file: &str,
) -> (Vec<ResolvedStyle>, Vec<Diagnostic>) {
    let mut styles: Vec<ResolvedStyle> = (0..tree.nodes.len())
        .map(|_| ResolvedStyle::default())
        .collect();
    let mut diagnostics = Vec::new();

    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else { continue };

        let mut flex_direction_set = false;

        // Apply DisplayDefault from schema
        if let Some(spec) = find_tag(&el.tag) {
            styles[idx].display_mode = match spec.display {
                DisplayDefault::Block => DisplayMode::Block,
                DisplayDefault::Inline => DisplayMode::Flex, // inline → flex for taffy compat
                DisplayDefault::None => DisplayMode::None,
            };
        }

        // Apply inline style declarations
        if let Some(style_attr) = el.attributes.iter().find(|a| a.name == "style") {
            for decl in style_attr.value.split(';') {
                let decl = decl.trim();
                if decl.is_empty() { continue; }
                let (prop, value) = match decl.split_once(':') {
                    Some((p, v)) => (p.trim(), v.trim()),
                    None => continue,
                };

                // Validate property name
                let is_known = find_css_prop(prop).is_some() || find_shorthand(prop).is_some();
                if !is_known {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceUnknownCssProp,
                        format!("CSS 属性 \"{}\" 不在围栏内", prop),
                        SourceLocation {
                            file: file.to_string(),
                            offset: node.span.start,
                            line: 0, column: 0, source_text: String::new(),
                        },
                    ));
                    continue;
                }

                // Validate value (keyword check for display)
                if let Some(spec) = find_css_prop(prop) {
                    if let CssValueParser::Keyword(allowed) = &spec.parser {
                        if !allowed.contains(&value) {
                            diagnostics.push(Diagnostic::error(
                                DiagnosticCode::FenceBadCssValue,
                                format!("CSS 值 \"{}\" 对属性 \"{}\" 不合法", value, prop),
                                SourceLocation {
                                    file: file.to_string(),
                                    offset: node.span.start,
                                    line: 0, column: 0, source_text: String::new(),
                                },
                            ));
                            continue;
                        }
                    }
                }

                // Track explicit flex-direction
                if prop == "flex-direction" {
                    flex_direction_set = true;
                }

                // Apply using existing apply_decl
                apply_decl(&mut styles[idx], prop, value);
            }
        }

        // CSS spec: flex-direction initial value is row.
        // ResolvedStyle::default() hardcodes Column (legacy).
        // If display ended up as Flex and no explicit flex-direction was
        // applied, override to Row per CSS standard.
        if styles[idx].display_mode == DisplayMode::Flex && !flex_direction_set {
            styles[idx].taffy_style.flex_direction = taffy::FlexDirection::Row;
        }
    }

    (styles, diagnostics)
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::css_resolve -- --nocapture`
Expected: PASS — 3 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/css_resolve.rs
git commit -m "r1: CSS resolve (Stage 4) — schema-driven inline style validation + application"
```

---

### Task 12: Structural Validator (Stage 5)

**Files:**
- Modify: `crates/core/src/fence/structural.rs`

Stage 5 validates cross-element structural constraints: content model (child Category vs parent ContentModel), ID uniqueness, and ID references (label for, aria-controls, aria-labelledby).

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/structural.rs` with:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fence::tree_builder::parse_html_to_ir;

    #[test]
    fn block_inside_phrasing_rejected() {
        // <span> has ContentModel::Phrasing, <div> is Block → invalid
        let (tree, _) = parse_html_to_ir(r#"<span><div>x</div></span>"#);
        let diags = run_structural(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::InvalidContentModel));
    }

    #[test]
    fn flow_inside_div_accepted() {
        let (tree, _) = parse_html_to_ir(r#"<div><span>ok</span><p>text</p></div>"#);
        let diags = run_structural(&tree, "test.html");
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "div accepts Flow: {:?}", errors);
    }

    #[test]
    fn duplicate_id_reported() {
        let (tree, _) = parse_html_to_ir(r#"<div id="x"></div><div id="x"></div>"#);
        let diags = run_structural(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::DuplicateId));
    }

    #[test]
    fn select_only_accepts_option() {
        let (tree, _) = parse_html_to_ir(r#"<select><option>a</option></select>"#);
        let diags = run_structural(&tree, "test.html");
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "select > option is valid: {:?}", errors);
    }

    #[test]
    fn select_rejects_div() {
        let (tree, _) = parse_html_to_ir(r#"<select><div>x</div></select>"#);
        let diags = run_structural(&tree, "test.html");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::InvalidContentModel));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::structural -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the implementation**
Add above the test module in `structural.rs`:
```rust
use crate::fence::diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceLocation};
use crate::fence::ir::{IrNodeKind, IrNodeId, IrTree};
use crate::fence::schema::tag::{find_tag, Category, ContentModel};
use std::collections::HashSet;

/// Run Stage 5 (Structural): validate cross-element constraints.
pub fn run_structural(tree: &IrTree, file: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_content_model(tree, file, &mut diagnostics);
    validate_id_uniqueness(tree, file, &mut diagnostics);
    diagnostics
}

fn loc(file: &str, offset: usize) -> SourceLocation {
    SourceLocation { file: file.to_string(), offset, line: 0, column: 0, source_text: String::new() }
}

fn validate_content_model(tree: &IrTree, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    for parent_id in tree.all_element_ids() {
        let parent_tag = match &tree.nodes[parent_id.0].kind {
            IrNodeKind::Element(e) => e.tag.as_str(),
            _ => continue,
        };
        let parent_spec = match find_tag(parent_tag) {
            Some(s) => s,
            None => continue,
        };

        for &child_id in &tree.nodes[parent_id.0].children {
            let child_tag = match &tree.nodes[child_id.0].kind {
                IrNodeKind::Element(e) => e.tag.as_str(),
                IrNodeKind::Text(_) => {
                    // Text nodes: reject if parent does not accept text
                    match parent_spec.content {
                        ContentModel::None | ContentModel::Only(_) => {
                            diagnostics.push(Diagnostic::error(
                                DiagnosticCode::InvalidContentModel,
                                format!("<{}> does not accept text content", parent_tag),
                                loc(file, tree.nodes[child_id.0].span.start),
                            ));
                        }
                        _ => {} // Text/Phrasing/Flow/Transparent accept text
                    }
                    continue;
                }
                _ => continue,
            };

            let child_cat = find_tag(child_tag).map(|s| s.category);
            if !is_child_allowed(&parent_spec.content, child_cat, child_tag) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidContentModel,
                    format!("<{}> 不能出现在 <{}> 内（内容模型冲突）", child_tag, parent_tag),
                    loc(file, tree.nodes[child_id.0].span.start),
                ));
            }
        }
    }
}

fn is_child_allowed(
    parent_content: &ContentModel,
    child_category: Option<Category>,
    child_tag: &str,
) -> bool {
    match parent_content {
        ContentModel::None => false,
        ContentModel::Text => false, // only text, no elements
        ContentModel::Phrasing => matches!(
            child_category,
            Some(Category::Phrasing) | Some(Category::Void) | Some(Category::Transparent)
        ),
        ContentModel::Flow => true, // accepts everything
        ContentModel::Transparent => true, // resolved against ancestor (simplified: accept)
        ContentModel::Only(allowed) => allowed.contains(&child_tag),
    }
}

fn validate_id_uniqueness(tree: &IrTree, file: &str, diagnostics: &mut Vec<Diagnostic>) {
    let mut seen: HashSet<String> = HashSet::new();
    for id in tree.all_element_ids() {
        let el = match &tree.nodes[id.0].kind {
            IrNodeKind::Element(e) => e,
            _ => continue,
        };
        if let Some(id_attr) = el.attributes.iter().find(|a| a.name == "id") {
            if !seen.insert(id_attr.value.clone()) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::DuplicateId,
                    format!("ID \"{}\" 在当前模板作用域内重复定义", id_attr.value),
                    loc(file, id_attr.span.start),
                ));
            }
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::structural -- --nocapture`
Expected: PASS — 5 tests.

- [ ] **Step 5: Commit**
```bash
git add crates/core/src/fence/structural.rs
git commit -m "r1: structural validator (Stage 5) — content model + ID uniqueness"
```

---

### Task 13: Annotator (Stage 6) + Pipeline

**Files:**
- Modify: `crates/core/src/fence/annotate.rs`
- Modify: `crates/core/src/fence/pipeline.rs`
- Modify: `crates/core/src/fence/mod.rs`

Stage 6 annotates each element with its final `SemanticKind`. The pipeline assembles all six stages and produces `ParsedTemplate`.

- [ ] **Step 1: Write the failing test**
Replace `crates/core/src/fence/pipeline.rs` with:
```rust
#![cfg(feature = "parse")]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_simple_template() {
        let result = parse_template(r#"<div id="root"><span>Hello</span></div>"#, "home.html");
        assert!(result.diagnostics.is_empty(), "unexpected diagnostics: {:?}", result.diagnostics);
        assert_eq!(result.tree.roots.len(), 1);

        let root = result.tree.roots[0];
        let el = result.tree.element(root).unwrap();
        assert_eq!(el.tag, "div");
        assert_eq!(el.semantic, Some(SemanticKind::Container));

        let span_id = result.tree.nodes[root.0].children[0];
        let span_el = result.tree.element(span_id).unwrap();
        assert_eq!(span_el.semantic, Some(SemanticKind::TextElement));
    }

    #[test]
    fn pipeline_input_semantic() {
        let result = parse_template(r#"<input type="range">"#, "form.html");
        assert!(result.diagnostics.is_empty());
        let el = result.tree.element(result.tree.roots[0]).unwrap();
        assert_eq!(el.semantic, Some(SemanticKind::Slider));
    }

    #[test]
    fn pipeline_collects_all_errors() {
        let result = parse_template(
            r#"<video></video><div bogus="x" style="grid-template:x"></div>"#,
            "bad.html",
        );
        // Should have multiple errors, not just the first
        assert!(result.diagnostics.len() >= 2,
            "should collect all errors, got: {:?}", result.diagnostics);
    }

    #[test]
    fn pipeline_referenced_sprites() {
        let result = parse_template(
            r#"<img src="icons/home.png">"#, "view.html",
        );
        assert!(result.referenced_sprites.contains(&"icons/home.png".to_string()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**
Run: `cargo test -p loomgui_core fence::pipeline -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Write the annotator**
Replace `crates/core/src/fence/annotate.rs`:
```rust
#![cfg(feature = "parse")]

use crate::fence::ir::{IrNodeKind, IrTree};
use crate::fence::schema::tag::resolve_semantic;

/// Run Stage 6 (Annotate): fill in `IrElement.semantic` for all elements.
pub fn annotate(tree: &mut IrTree) {
    for node in &mut tree.nodes {
        if let IrNodeKind::Element(el) = &mut node.kind {
            let input_type = el.attributes.iter()
                .find(|a| a.name == "type")
                .map(|a| a.value.as_str());
            el.semantic = resolve_semantic(&el.tag, input_type);
        }
    }
}
```

- [ ] **Step 4: Write the pipeline**
Add above the test module in `pipeline.rs`:
```rust
use crate::fence::annotate::annotate;
use crate::fence::css_resolve::resolve_inline_styles_with_diags;
use crate::fence::diagnostic::Diagnostic;
use crate::fence::fence_gate::run_fence_gate;
use crate::fence::ir::{IrNodeKind, IrTree};
use crate::fence::schema::tag::SemanticKind;
use crate::fence::structural::run_structural;
use crate::fence::tree_builder::parse_html_to_ir_named;
use crate::style::resolved::ResolvedStyle;

/// Final output of the R1 parsing pipeline.
pub struct ParsedTemplate {
    pub tree: IrTree,
    pub styles: Vec<ResolvedStyle>,
    pub diagnostics: Vec<Diagnostic>,
    pub referenced_sprites: Vec<String>,
}

/// Full six-stage pipeline: Tokenize → Tree Build → Fence Gate → CSS Resolve →
/// Structural → Annotate.
///
/// Collects ALL diagnostics (does not fail-fast).
pub fn parse_template(html: &str, file: &str) -> ParsedTemplate {
    // Stage 1+2: Tokenize + Tree Build
    let (mut tree, mut diagnostics) = parse_html_to_ir_named(html, file.to_string());

    // Stage 3: Fence Gate (per-element validation)
    let gate_diags = run_fence_gate(&tree, file);
    diagnostics.extend(gate_diags);

    // Stage 4: CSS Resolve
    let (styles, css_diags) = resolve_inline_styles_with_diags(&tree, file);
    diagnostics.extend(css_diags);

    // Stage 5: Structural (content model, IDs)
    let struct_diags = run_structural(&tree, file);
    diagnostics.extend(struct_diags);

    // Stage 6: Annotate (fill SemanticKind)
    annotate(&mut tree);

    // Extract referenced sprites (img src, background-image url)
    let referenced_sprites = extract_sprites(&tree);

    ParsedTemplate {
        tree,
        styles,
        diagnostics,
        referenced_sprites,
    }
}

fn extract_sprites(tree: &IrTree) -> Vec<String> {
    let mut sprites = Vec::new();
    for node in &tree.nodes {
        if let IrNodeKind::Element(el) = &node.kind {
            // img src
            if el.tag == "img" {
                if let Some(src) = el.attributes.iter().find(|a| a.name == "src") {
                    sprites.push(src.value.clone());
                }
            }
            // background-image url(...)
                if let Some(url) = crate::style::mapping::parse_url(&style.value) {
                ) {
                    sprites.push(url);
                }
            }
        }
    }
    sprites
}
```

Also update `crates/core/src/fence/mod.rs` to export `ParsedTemplate` and `parse_template`:
```rust
#[cfg(feature = "parse")]
pub mod annotate;
#[cfg(feature = "parse")]
pub mod css_resolve;
#[cfg(feature = "parse")]
pub mod diagnostic;
#[cfg(feature = "parse")]
pub mod fence_gate;
#[cfg(feature = "parse")]
pub mod ir;
#[cfg(feature = "parse")]
pub mod pipeline;
#[cfg(feature = "parse")]
pub mod schema;
#[cfg(feature = "parse")]
pub mod structural;
#[cfg(feature = "parse")]
pub mod tree_builder;

#[cfg(feature = "parse")]
pub use pipeline::{parse_template, ParsedTemplate};
```

- [ ] **Step 5: Run test to verify it passes**
Run: `cargo test -p loomgui_core fence::pipeline -- --nocapture`
Expected: PASS — 4 tests.

- [ ] **Step 6: Commit**
```bash
git add crates/core/src/fence/annotate.rs crates/core/src/fence/pipeline.rs crates/core/src/fence/mod.rs
git commit -m "r1: annotator (Stage 6) + pipeline orchestration → ParsedTemplate"
```

---

### Task 14: Integration Tests

**Files:**
- Create: `crates/core/tests/r1_schema_contract.rs`
- Create: `crates/core/tests/r1_pipeline.rs`

- [ ] **Step 1: Write the schema contract test**
Create `crates/core/tests/r1_schema_contract.rs`:
```rust
use loomgui_core::fence::schema::tag::{find_tag, is_shell_tag, resolve_semantic, SemanticKind, Category, ContentModel};
use loomgui_core::fence::schema::css::{find_css_prop, find_shorthand, CssValueParser};
use loomgui_core::fence::schema::attr::{is_global_attr, INPUT_STRUCTURAL};

#[test]
fn all_23_runtime_tags_have_specs() {
    let tags = [
        "div", "header", "nav", "p", "span", "strong", "em", "br",
        "label", "button", "a", "img", "canvas", "input", "textarea",
        "select", "option", "progress", "ul", "ol", "li", "template", "slot",
    ];
    for t in tags {
        assert!(find_tag(t).is_some(), "<{}> must be in TAGS", t);
    }
    assert_eq!(tags.len(), 23);
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
    assert_eq!(find_tag("p").unwrap().content, ContentModel::Phrasing);
    assert_eq!(find_tag("span").unwrap().content, ContentModel::Phrasing);
    assert_eq!(find_tag("img").unwrap().content, ContentModel::None);
    assert_eq!(find_tag("select").unwrap().content, ContentModel::Only(&["option"]));
    assert_eq!(find_tag("ul").unwrap().content, ContentModel::Only(&["li", "template"]));
    assert_eq!(find_tag("a").unwrap().content, ContentModel::Transparent);
    assert_eq!(find_tag("slot").unwrap().content, ContentModel::Transparent);
}

#[test]
fn display_defaults_match_spec() {
    use loomgui_core::fence::schema::tag::DisplayDefault;
    assert_eq!(find_tag("div").unwrap().display, DisplayDefault::Block);
    assert_eq!(find_tag("span").unwrap().display, DisplayDefault::Inline);
    assert_eq!(find_tag("template").unwrap().display, DisplayDefault::None);
}

#[test]
fn void_elements() {
    assert!(find_tag("img").unwrap().void);
    assert!(find_tag("br").unwrap().void);
    assert!(find_tag("input").unwrap().void);
    assert!(!find_tag("div").unwrap().void);
}

#[test]
fn css_props_count_and_key_ones() {
    // All the critical CSS props from apply_decl should be present
    for prop in [
        "width", "height", "color", "background-color", "display",
        "flex-direction", "padding-top", "margin-top", "border-color",
        "opacity", "overflow-x", "transform", "font-size", "transition",
    ] {
        assert!(find_css_prop(prop).is_some(), "CSS prop '{}' must be in CSS_PROPS", prop);
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
    assert_eq!(find_shorthand("overflow").unwrap().expands_to, &["overflow-x", "overflow-y"]);
    assert!(find_shorthand("padding").is_some());
    assert!(find_shorthand("background").is_some());
}

#[test]
fn global_attr_detection() {
    assert!(is_global_attr("id"));
    assert!(is_global_attr("data-anything"));
    assert!(is_global_attr("aria-label"));
    assert!(!is_global_attr("type"));
}
```

- [ ] **Step 2: Write the pipeline integration test**
Create `crates/core/tests/r1_pipeline.rs`:
```rust
use loomgui_core::fence::pipeline::parse_template;
use loomgui_core::fence::diagnostic::DiagnosticCode;
use loomgui_core::fence::schema::tag::SemanticKind;

#[test]
fn complex_template_parses_clean() {
    let html = r#"<div id="root" class="panel">
        <header><h1_lol>-- not real --</h1_lol>
            <button class="close">X</button>
        </header>
        <ul>
            <li><span>Item 1</span></li>
            <li><span>Item 2</span></li>
        </ul>
        <input type="range" min="0" max="100">
    </div>"#;
    let result = parse_template(html, "complex.html");
    let errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.severity == loomgui_core::fence::diagnostic::Severity::Error)
        .collect();
    // The <h1_lol> is a custom element (contains '-') — should be accepted
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Verify semantic annotation
    let root = result.tree.roots[0];
    assert_eq!(result.tree.element(root).unwrap().semantic, Some(SemanticKind::Container));

    // Find the input and check it's a Slider
    for node in &result.tree.nodes {
        if let loomgui_core::fence::ir::IrNodeKind::Element(el) = &node.kind {
            if el.tag == "input" {
                assert_eq!(el.semantic, Some(SemanticKind::Slider));
            }
            if el.tag == "ul" {
                assert_eq!(el.semantic, Some(SemanticKind::ListView));
            }
        }
    }
}

#[test]
fn fence_out_tags_reported() {
    let result = parse_template(r#"<video src="x.mp4"></video>"#, "bad.html");
    assert!(result.diagnostics.iter().any(|d| d.code == DiagnosticCode::FenceUnknownTag));
}

#[test]
fn multiple_errors_collected() {
    let html = r#"<video></video><audio></audio><h4>x</h4>"#;
    let result = parse_template(html, "multi.html");
    let errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.code == DiagnosticCode::FenceUnknownTag)
        .collect();
    assert!(errors.len() >= 3, "should report all 3 unknown tags, got {}", errors.len());
}

#[test]
fn rich_text_mixed_children() {
    let html = r#"<p>Hello <strong>bold</strong> and <em>italic</em>!</p>"#;
    let result = parse_template(html, "rich.html");
    assert!(result.diagnostics.is_empty(), "rich text should parse clean: {:?}", result.diagnostics);
    let p = result.tree.roots[0];
    // p should have 5 children: Text, strong, Text, em, Text
    assert_eq!(result.tree.nodes[p.0].children.len(), 5);
}
```

- [ ] **Step 3: Run all R1 tests**
Run: `cargo test -p loomgui_core --test r1_schema_contract --test r1_pipeline -- --nocapture`
Expected: PASS — all tests green.

- [ ] **Step 4: Run full check**
Run: `cargo check -p loomgui_core --all-targets`
Expected: No errors.

- [ ] **Step 5: Commit**
```bash
git add crates/core/tests/r1_schema_contract.rs crates/core/tests/r1_pipeline.rs
git commit -m "r1: integration tests — schema contract + end-to-end pipeline"
```

---

## Self-Review Notes

**Spec coverage check:**
- §2.1 Schema as Rust const tables → Task 5, 6, 7 (TAGS, ATTRS, CSS_PROPS, CSS_SHORTHANDS)
- §2.2 html5gum with custom callback → Task 9 (IrCallback implementing Callback)
- §2.3 CSS three orthogonal dimensions → Task 7 (schema) + Task 11 (resolve)
- §3 Schema structure → Tasks 4-7
- §4 Attribute tiers → Task 6
- §5 CSS schema → Task 7
- §6 IrNode (text as first-class child) → Task 2 + Task 9
- §7 Diagnostics (collect all) → Task 3 + Task 13 (pipeline)
- §8 Six-stage pipeline → Tasks 9-13
- §9 Existing asset reuse → Task 11 (reuses apply_decl, parse functions)
- §10 Test strategy → Task 14
- §11 R1 scope (no R2/R3/R5) → respected throughout
- §12 Locked decisions → all respected

**Type consistency:** `IrNodeId(usize)`, `Span { start, end }`, `SemanticKind` variants match across all tasks. `ParsedTemplate` fields match spec §8.2.


**Note on `all_element_ids`:** Defined in Task 2 (ir.rs) as part of `IrTree`. Used by Stages 3, 5, and 6.

**Note on `display` and `flex-direction`:** The existing `apply_decl` already has both `"display"` and `"flex-direction"` arms. `apply_decl` for `display:flex` sets `display_mode = Flex` but does NOT set `flex_direction` (it stays at the `ResolvedStyle::default()` value of `Column`). To comply with the spec decision that `display:flex` defaults to `flex-direction:row`, the css_resolve stage (Task 11) tracks whether `flex-direction` was explicitly applied and overrides to `Row` if not. See Task 11 tests `flex_defaults_to_row_direction` and `explicit_flex_direction_preserved`.

**Note on diagnostic line/column:** For R1, fence_gate, css_resolve, and structural stages create `SourceLocation` with `line: 0, column: 0` because they do not receive the `LineMap`. The tree_builder creates proper locations for tokenizer and unclosed-tag diagnostics. Full line/column threading to all stages is a known gap that will be addressed when the pipeline is connected to the packer (R2/R3). The `LineMap` infrastructure is already in place (Task 3).
**Note on inline style `display` handling:** The existing `apply_decl` doesn't have a `"display"` arm (display was hardcoded). The CSS resolve stage handles `display` separately — it checks against the schema keyword list and sets `display_mode` directly, then passes other properties through to `apply_decl`. This may need a small addition to `apply_decl` or a direct field set in `css_resolve.rs`. The worker should verify this compiles and add a `"display"` arm to `apply_decl` if needed.
