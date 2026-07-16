# Spec-2: core 类型化重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand NodeKind from 5 payload variants to 22 unit variants, split the 23-field Node struct, move leaf data to Scene side tables, persist set-ness in ResolvedStyle, and bump pkg format to v17.

**Architecture:** NodeKind becomes a pure discriminator (all unit variants, derives Copy). Leaf data (text content, image src) moves from enum payload to `Scene.text_contents` / `Scene.image_srcs` HashMaps, persisted across pkg.bin via new `TemplateNode.content` / `TemplateNode.src` fields. Pseudo-class bools collapse into `NodeFlags` bitflags; interaction fields group into `NodeInteraction` sub-struct. InheritedSet promoted from dynamic.rs local type to ResolvedStyle field for bake-time set-ness persistence.

**Tech Stack:** Rust 2021, bitflags (new dependency), bincode, slotmap 1.1, serde, taffy 0.5.

**Spec:** `docs/superpowers/specs/2026-07-16-core-type-refactor-design.md`

**Key constraint:** This is a compiler-guided mechanical refactor. Task 3 is the "big bang" — enum expansion breaks all 81+ match sites simultaneously. The engineer fixes compile errors file-by-file; intermediate `cargo build` runs will show decreasing error counts but won't fully succeed until all files are migrated. The commit happens only when `cargo test` is green.

---

## File Structure

| File | Responsibility | Action |
|---|---|---|
| `crates/core/Cargo.toml` | Add bitflags dependency | Modify |
| `crates/core/src/style/resolved.rs` | InheritedSet type + ResolvedStyle.inherited_set | Modify |
| `crates/core/src/scene/node.rs` | NodeKind enum + predicates + NodeFlags + NodeInteraction + Node split + Scene side tables + build() | Modify |
| `crates/core/src/asset/mod.rs` | TemplateNode content/src + PKG_FORMAT_VERSION v17 + MIN/MAX_VERSION | Modify |
| `crates/core/src/scene/dynamic.rs` | create_node, create_node_from_template, kind_from_tag, rematch set-ness, dirty_text | Modify |
| `crates/core/src/stage.rs` | instantiate loop fills side tables | Modify |
| `crates/core/src/layout/mod.rs` | measure dispatch (TextNode side table) + RichText removal | Modify |
| `crates/core/src/render/mod.rs` | container/leaf predicates + Text/Image side table + RichText removal | Modify |
| `crates/core/src/dump.rs` | kind->tag mapping for 22 variants | Modify |
| `crates/core/src/hit.rs` | NodeKind references in tests | Modify |
| `crates/core/src/tween.rs` | NodeKind references in tests | Modify |
| `crates/core/src/style/dynamic.rs` | InheritedSet import change (moved to resolved.rs) | Modify |
| `crates/core/src/scene/node/tests.rs` | Text{content} -> TextNode adaptation | Modify |
| `crates/core/src/asset/tests.rs` | Text{content} -> TextNode, RichText removal, content/src fields | Modify |
| `crates/core/src/input/tests.rs` | Text{content} -> TextNode | Modify |
| `crates/core/src/render/tests.rs` | RichText removal + Text/Image adaptation | Modify |
| `crates/core/src/render/batch.rs` | Text{content} test ref | Modify |
| `crates/core/src/stage/dynamic_tests.rs` | Text{content} -> TextNode | Modify |
| `crates/core/src/scroll/tests.rs` | NodeKind refs | Modify |
| `crates/fence/tests/cascade_spike.rs` | mini-bridge SceneEntry 10-tuple + TextNode + side table | Modify |

---

## Task 1: Foundation types (zero-breakage)

Add new types that don't break existing code. These are used by later tasks.

**Files:**
- Modify: `crates/core/Cargo.toml`
- Modify: `crates/core/src/style/resolved.rs`
- Modify: `crates/core/src/scene/node.rs`

- [ ] **Step 1: Add bitflags dependency**

In `crates/core/Cargo.toml`, add under `[dependencies]`:

```toml
bitflags = "2"
```

Run `cargo build -p loomgui_core` to verify it resolves.

- [ ] **Step 2: Add InheritedSet to resolved.rs**

In `crates/core/src/style/resolved.rs`, add before the `ResolvedStyle` struct:

```rust
/// Tracks which inherited CSS properties were explicitly declared (set-ness bitmask).
/// Each bit corresponds to one inheritable property (see INH_* constants in dynamic.rs).
/// Baked at package time into base_style; rematch reads it as the per-frame inheritance baseline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InheritedSet(pub u16);
```

- [ ] **Step 3: Add NodeFlags + NodeInteraction to node.rs**

In `crates/core/src/scene/node.rs`, add after the `Rect` struct (before `Node`):

```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct NodeFlags: u8 {
        const HOVERED  = 1 << 0;
        const ACTIVE   = 1 << 1;
        const FOCUSED  = 1 << 2;
        const DISABLED = 1 << 3;
        const CASCADED = 1 << 4;
    }
}

/// Interaction state — only process + rematch passes touch these.
/// Separated from Node top-level for cache locality (solve/world/build passes skip).
#[derive(Debug, Clone, Default)]
pub struct NodeInteraction {
    pub flags: NodeFlags,
    pub touchable: bool,
    pub draggable: bool,
    pub tabindex: Option<i32>,
}
```

- [ ] **Step 4: Build + test**

Run: `cargo build -p loomgui_core`
Expected: compiles (new types unused but valid).

Run: `cargo test -p loomgui_core`
Expected: all existing tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/core/Cargo.toml crates/core/src/style/resolved.rs crates/core/src/scene/node.rs
git commit -m "feat(core): add foundation types (InheritedSet, NodeFlags, NodeInteraction)"
```

---

## Task 2: ResolvedStyle.inherited_set + rematch set-ness fix

Add the inherited_set field to ResolvedStyle and wire rematch to read from base_style.

**Files:**
- Modify: `crates/core/src/style/resolved.rs`
- Modify: `crates/core/src/style/dynamic.rs`
- Test: `crates/core/src/style/resolved.rs` (bincode roundtrip)

- [ ] **Step 1: Add inherited_set field to ResolvedStyle**

In `crates/core/src/style/resolved.rs`, add field to `ResolvedStyle` struct (after `text_effects`):

```rust
    pub inherited_set: crate::style::resolved::InheritedSet,
```

Update `Default for ResolvedStyle` — add to the initializer:

```rust
            inherited_set: InheritedSet::default(),
```

- [ ] **Step 2: Write bincode roundtrip test**

In `crates/core/src/style/resolved.rs` tests module, add:

```rust
    #[test]
    fn inherited_set_bincode_roundtrip() {
        let mut s = ResolvedStyle::default();
        s.inherited_set = InheritedSet(0b0000_0011); // font-size + color set
        let bytes = bincode::serialize(&s).expect("serialize");
        let back: ResolvedStyle = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.inherited_set, s.inherited_set);
        assert_eq!(back, s, "full round-trip equal");
    }
```

- [ ] **Step 3: Run test**

Run: `cargo test -p loomgui_core inherited_set_bincode_roundtrip`
Expected: PASS

- [ ] **Step 4: Wire rematch to read base_style.inherited_set**

In `crates/core/src/style/dynamic.rs`, find the `rematch_pseudo_classes` function. Currently it creates `let mut inh: InheritedSet = InheritedSet::default();` per node (the local spike type).

Change: import InheritedSet from resolved.rs instead of the local definition. Delete the local `struct InheritedSet(u16)` and the local INH_* constants (lines ~84-93). Import them:

```rust
use crate::style::resolved::InheritedSet;
```

Keep the `inherited_bit` function (it maps CSS prop names to bit positions — still needed).

Change the set-ness initialization to seed from base_style:

```rust
        // Seed from base_style (package-time baked declarations), then OR dynamic cascade bits.
        let base_inh = scene.get(node_id).expect("live node").base_style.inherited_set;
        let mut inh: InheritedSet = base_inh;
```

This replaces `let mut inh: InheritedSet = InheritedSet::default();`.

- [ ] **Step 5: Build + test**

Run: `cargo build -p loomgui_core`
Expected: compiles.

Run: `cargo test -p loomgui_core`
Expected: all tests pass (spike's 4 acceptance tests still green — base_style.inherited_set is all-0 default, same as before).

Run: `cargo test -p loomgui_fence`
Expected: spike acceptance tests still green.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/style/resolved.rs crates/core/src/style/dynamic.rs
git commit -m "feat(core): persist inherited_set in ResolvedStyle, seed rematch from base_style"
```

---

## Task 3: The big refactor — NodeKind expansion + Node split + match migration

This is the core mechanical task. It expands the enum, splits the struct, and migrates all references. The compiler guides the engineer through all 81+ match sites.

**Order matters:** Steps must be done top-to-bottom. Each `cargo build` checkpoint shows decreasing errors.

**Files:** All files listed in the File Structure table.

- [ ] **Step 1: Rewrite NodeKind enum**

In `crates/core/src/scene/node.rs`, replace the entire `NodeKind` enum definition with:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum NodeKind {
    #[default]
    Container,
    TextNode,
    TextBlock,
    TextElement,
    LineBreak,
    Label,
    Button,
    Link,
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
    Slot,
    CustomElement,
    Canvas,
}

impl NodeKind {
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Self::Container
                | Self::TextBlock
                | Self::TextElement
                | Self::Label
                | Self::Button
                | Self::Link
                | Self::ListView
                | Self::ListItem
                | Self::Canvas
                | Self::Slot
                | Self::CustomElement
        )
    }

    pub fn is_leaf(self) -> bool {
        !self.is_container()
    }

    pub fn has_children(self) -> bool {
        self.is_container()
    }
}
```

Key changes: `Clone` -> `Copy` (all unit variants now). `Text { content }` -> `TextNode`. `RichText { runs }` deleted. `Image { src }` -> `Image`.

- [ ] **Step 2: Split Node struct**

In `crates/core/src/scene/node.rs`, replace the `Node` struct. Replace the individual fields `touchable/hovered/active/disabled/draggable/tabindex/focused/cascaded_once` with `interaction: NodeInteraction`. The struct becomes:

```rust
#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub style: ResolvedStyle,
    pub taffy_id: Option<taffy::NodeId>,
    pub layout_rect: Rect,
    pub clip_rect: Option<Rect>,
    pub children: Vec<NodeId>,
    pub dirty_mesh: bool,
    pub dirty_text: bool,
    pub base_style: ResolvedStyle,
    pub classes: Vec<String>,
    pub id_attr: Option<String>,
    pub interaction: NodeInteraction,
    pub reuse_key: u32,
    pub data_controller: Option<String>,
}
```

Update `Default for Node` — replace the individual interaction fields with:

```rust
            interaction: NodeInteraction {
                flags: NodeFlags::empty(),
                touchable: true,
                draggable: false,
                tabindex: None,
            },
```

Remove `hovered/active/disabled/draggable/tabindex/focused/cascaded_once` from the default initializer.

- [ ] **Step 3: Add Scene side tables + expand build() entry tuple**

In `crates/core/src/scene/node.rs`, add two fields to `Scene`:

```rust
    pub text_contents: std::collections::HashMap<NodeId, String>,
    pub image_srcs: std::collections::HashMap<NodeId, String>,
```

Expand `Scene::build` entry tuple from 8-tuple to 10-tuple. Change the signature:

```rust
    pub fn build(
        entries: &[(
            Option<usize>,
            NodeKind,
            ResolvedStyle,
            Vec<String>,
            Option<String>,
            bool,
            Option<i32>,
            Option<String>,
            Option<String>, // content (TextNode)
            Option<String>, // src (Image)
        )],
    ) -> Scene {
```

In the build loop, after inserting each node and getting its `id`, fill side tables:

```rust
            // Fill leaf data side tables
            if let Some(c) = content {
                scene.text_contents.insert(id, c.clone());
            }
            if let Some(s) = src {
                scene.image_srcs.insert(id, s.clone());
            }
```

Destructure the new fields from the entry tuple in the loop. The interaction fields (touchable, draggable, tabindex) now go into `interaction`. Map `cascaded_once` -> not needed in build (default false = CASCADED flag not set).

- [ ] **Step 4: Add content/src to TemplateNode**

In `crates/core/src/asset/mod.rs`, add two fields to `TemplateNode`:

```rust
    pub content: Option<String>,
    pub src: Option<String>,
```

- [ ] **Step 5: Update create_node (scene/dynamic.rs)**

In `crates/core/src/scene/dynamic.rs`:

`kind_from_tag`: change to unit variants:

```rust
pub fn kind_from_tag(tag: &str) -> Result<NodeKind, String> {
    match tag {
        "div" => Ok(NodeKind::Container),
        "button" => Ok(NodeKind::Button),
        "img" => Ok(NodeKind::Image),
        "span" => Ok(NodeKind::TextNode),
        other => Err(format!("unknown kind tag: {}", other)),
    }
}
```

`create_node`: the `dirty_text` line changes from `matches!(k, NodeKind::Text { .. })` to `matches!(k, NodeKind::TextNode)`. Fill empty content for TextNode:

```rust
    if matches!(k, NodeKind::TextNode) {
        scene.text_contents.insert(id, String::new());
    }
```

The `create_node` function body constructs a `Node { ... }` literal. Update it to use `interaction: NodeInteraction { ... }` instead of individual fields. Map: `touchable` -> `interaction.touchable`, `hovered/active/disabled/focused/cascaded_once` -> `interaction.flags` (all empty/false by default).

`create_node_from_template`: same struct-literal fix. dirty_text changes to `matches!(kind, NodeKind::TextNode)`. The test `create_node_from_template_text_marks_dirty_text` uses `NodeKind::Text { content: "hi".into() }` -> change to `NodeKind::TextNode` + `scene.text_contents.insert(id, "hi".into())`.

- [ ] **Step 6: Update stage.rs instantiate loop**

In `crates/core/src/stage.rs`, `instantiate` function (line ~597). After `create_node_from_template` returns `node_id`, fill side tables:

```rust
            let node_id = crate::scene::dynamic::create_node_from_template(
                scene,
                tn.kind,
                tn.style.clone(),
            );
            if let Some(c) = &tn.content {
                scene.text_contents.insert(node_id, c.clone());
            }
            if let Some(s) = &tn.src {
                scene.image_srcs.insert(node_id, s.clone());
            }
```

The node field assignments after (classes/id_attr/draggable/tabindex/data_controller) change from `n.draggable = ...` to `n.interaction.draggable = ...`, `n.tabindex = ...` to `n.interaction.tabindex = ...`.

- [ ] **Step 7: Update layout/mod.rs**

In `crates/core/src/layout/mod.rs`:

The measure dispatch `match &node.kind` (line ~152):
- `NodeKind::Text { content }` -> `NodeKind::TextNode` + read `scene.text_contents.get(&node.id)` (need to pass `scene` or `text_contents` into the measure closure). Content becomes `scene.text_contents.get(&node.id).map(|s| s.as_str()).unwrap_or("")`.
- `NodeKind::Image { src }` -> `NodeKind::Image` + read `scene.image_srcs.get(&node.id)`.
- `NodeKind::RichText { runs }` -> DELETE this branch. Rich text layout is retired (复合束 reimplementation).

The RichText-specific auto-width check (line ~207): `matches!(node.kind, NodeKind::RichText { .. })` -> DELETE (no RichText anymore).

Note: the measure closure currently borrows `scene` — verify it can access `text_contents`. If the closure only has `&node`, refactor to pass content/src as parameters. The `solve` function signature may need `scene` passed through, or pre-extract content/src before the closure.

- [ ] **Step 8: Update render/mod.rs**

In `crates/core/src/render/mod.rs`:

- Line 313: `NodeKind::Container | NodeKind::Button => { ... }` -> `if node.kind.is_container() { ... }`. This is the batch DFS container classification.
- Line 761: `matches!(n.kind, NodeKind::Container | NodeKind::Button)` -> `n.kind.is_container()`.
- Line 477: `NodeKind::Image { src }` -> `NodeKind::Image` + read `scene.image_srcs.get(&node.id)`.
- Line 527: `NodeKind::Text { content }` -> `NodeKind::TextNode` + read `scene.text_contents.get(&node.id)`.
- Line 589: `NodeKind::RichText { runs }` -> DELETE this branch.

Note: render build functions need access to `scene.text_contents` / `scene.image_srcs`. Check if they already borrow `scene` — if so, add the side table lookups. If not, pass them as parameters.

- [ ] **Step 9: Update dump.rs**

In `crates/core/src/dump.rs` (line ~31), the `match node.kind` tag mapping. Replace with:

```rust
            match n.kind {
                NodeKind::Container => ("div", "Container".into()),
                NodeKind::TextNode => ("span", "TextNode".into()),
                NodeKind::TextBlock => ("p", "TextBlock".into()),
                NodeKind::TextElement => ("span", "TextElement".into()),
                NodeKind::LineBreak => ("br", "LineBreak".into()),
                NodeKind::Label => ("label", "Label".into()),
                NodeKind::Button => ("button", "Button".into()),
                NodeKind::Link => ("a", "Link".into()),
                NodeKind::Image => ("img", "Image".into()),
                NodeKind::Canvas => ("canvas", "Canvas".into()),
                NodeKind::TextField => ("input", "TextField".into()),
                NodeKind::NumberField => ("input", "NumberField".into()),
                NodeKind::Slider => ("input", "Slider".into()),
                NodeKind::Toggle => ("input", "Toggle".into()),
                NodeKind::RadioButton => ("input", "RadioButton".into()),
                NodeKind::TextArea => ("textarea", "TextArea".into()),
                NodeKind::Dropdown => ("select", "Dropdown".into()),
                NodeKind::OptionItem => ("option", "OptionItem".into()),
                NodeKind::ProgressBar => ("progress", "ProgressBar".into()),
                NodeKind::ListView => ("ul", "ListView".into()),
                NodeKind::ListItem => ("li", "ListItem".into()),
                NodeKind::Slot => ("slot", "Slot".into()),
                NodeKind::CustomElement => ("custom", "CustomElement".into()),
            }
```

Note: dump.rs currently reads content from `NodeKind::Text { content }`. With unit variants, content is in side table. If dump needs content, read from `scene.text_contents.get(&n.id)`. Check how dump accesses the scene.

- [ ] **Step 10: Migrate all remaining match sites**

Run `cargo build -p loomgui_core 2>&1 | findstr "error"` to get the remaining error list. Fix each file:

**Transformation rules for test code:**
- `NodeKind::Text { content: "x".into() }` -> `NodeKind::TextNode` (+ `scene.text_contents.insert(id, "x".into())` if the test reads content)
- `NodeKind::Text { content }` in pattern -> `NodeKind::TextNode` (+ read from side table)
- `NodeKind::Image { src: "x".into() }` -> `NodeKind::Image` (+ `scene.image_srcs.insert(id, "x".into())`)
- `matches!(n.kind, NodeKind::Text { .. })` -> `matches!(n.kind, NodeKind::TextNode)`
- `matches!(n.kind, NodeKind::Image { .. })` -> `matches!(n.kind, NodeKind::Image)`
- `NodeKind::RichText { runs }` -> DELETE test or convert to `NodeKind::TextNode` with plain text
- `n.hovered = true` -> `n.interaction.flags |= NodeFlags::HOVERED` (or `n.interaction.flags.insert(NodeFlags::HOVERED)`)
- `n.active = true` -> `n.interaction.flags.insert(NodeFlags::ACTIVE)`
- `n.focused = true` -> `n.interaction.flags.insert(NodeFlags::FOCUSED)`
- `n.disabled = true` -> `n.interaction.flags.insert(NodeFlags::DISABLED)`
- `n.touchable = false` -> `n.interaction.touchable = false`
- `n.draggable = true` -> `n.interaction.draggable = true`
- `n.tabindex = Some(x)` -> `n.interaction.tabindex = Some(x)`
- `n.cascaded_once = true` -> `n.interaction.flags.insert(NodeFlags::CASCADED)`
- `n.hovered` (read) -> `n.interaction.flags.contains(NodeFlags::HOVERED)` (same for others)
- `n.touchable` (read) -> `n.interaction.touchable`

**Files with NodeKind refs in tests** (fix per rules above):
- `crates/core/src/scene/node/tests.rs`
- `crates/core/src/asset/tests.rs`
- `crates/core/src/input/tests.rs`
- `crates/core/src/render/tests.rs`
- `crates/core/src/render/batch.rs`
- `crates/core/src/stage/dynamic_tests.rs`
- `crates/core/src/scroll/tests.rs`
- `crates/core/src/hit.rs`
- `crates/core/src/tween.rs`

**Files with interaction field refs in production code** (hovered/active/focused/disabled/touchable/draggable/tabindex/cascaded_once):
- `crates/core/src/scene/dynamic.rs` (rematch reads hovered/active/focused/disabled)
- `crates/core/src/input.rs` (process reads/writes hovered/active)
- `crates/core/src/stage.rs` (reads focused, sets disabled)
- `crates/core/src/style/dynamic.rs` (rematch reads hovered/active/focused/disabled, cascaded_once)

For production code, use `.interaction.flags.contains(NodeFlags::HOVERED)` for reads, `.interaction.flags.insert(NodeFlags::HOVERED)` for writes.

- [ ] **Step 11: Handle RichText test removal**

Files with RichText test references (41 in render/tests.rs, several in asset/tests.rs):

For tests that test RichText-specific behavior (link fragments, inline runs, mixed formatting): DELETE the test. Rich text is retired until 复合束. Add a comment:
```rust
// RichText retired in Spec-2; rich text tests deferred to 复合束 (text model).
```

For tests that use RichText as a generic text node: convert `NodeKind::RichText { runs }` -> `NodeKind::TextNode` + `scene.text_contents.insert(id, plain_text_from_runs)`.

The `crates/core/src/text/rich.rs` module is KEPT (algorithm asset, roadmap §5). Only the enum variant + layout/render branches are deleted.

`Scene.rich_fragments` field: keep in struct (dead code until 复合束, add `#[allow(dead_code)]` if clippy complains).

- [ ] **Step 12: Fix TemplateNode construction in asset/tests.rs**

Every `tn(NodeKind::Container)` test helper needs updating. The `tn` helper creates a `TemplateNode` — it must now include `content: None, src: None`. Find the `tn` helper function and add the two fields.

For tests that construct `TemplateNode { kind: NodeKind::Text { content: "x".into() }, ... }`:
```rust
TemplateNode { kind: NodeKind::TextNode, content: Some("x".into()), src: None, ... }
```

For tests that construct `TemplateNode { kind: NodeKind::Image { src: "x".into() }, ... }`:
```rust
TemplateNode { kind: NodeKind::Image, content: None, src: Some("x".into()), ... }
```

- [ ] **Step 13: Build checkpoint**

Run: `cargo build -p loomgui_core`
Expected: compiles with zero errors. If errors remain, fix them per the transformation rules.

- [ ] **Step 14: Test checkpoint**

Run: `cargo test -p loomgui_core`
Expected: all tests pass. Some RichText tests deleted (acceptable). If tests fail, debug.

Run: `cargo test -p loomgui_pkg`
Expected: passes (packer may reference NodeKind — adapt per rules).

- [ ] **Step 15: Commit**

```bash
git add -A
git commit -m "refactor(core): expand NodeKind to 22 unit variants, split Node struct, side tables for leaf data"
```

---

## Task 4: Spike mini-bridge adaptation

Adapt the Spec-1 cascade spike mini-bridge to the new enum + side tables.

**Files:**
- Modify: `crates/fence/tests/cascade_spike.rs`

- [ ] **Step 1: Update SceneEntry type alias**

In `crates/fence/tests/cascade_spike.rs`, the `type SceneEntry` (8-tuple) expands to 10-tuple:

```rust
    type SceneEntry = (
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
        Option<String>, // content
        Option<String>, // src
    );
```

Update every `entries.push(...)` to include `content: Some(content)` (or `None`) and `src: None` as the last two tuple fields.

- [ ] **Step 2: Update kind mapping**

The bridge's kind mapping changes:

```rust
        let kind = match el.tag.as_str() {
            "div" | "main" | "section" | "header" | "footer" | "nav" | "article" | "aside" => {
                NodeKind::Container
            }
            _ => NodeKind::TextNode,
        };
```

Content goes into the tuple's content field (Some(content)) instead of NodeKind payload.

- [ ] **Step 3: Run spike tests**

Run: `cargo test -p loomgui_fence`
Expected: all 4 spike acceptance tests green (cascade class hit, font-size inheritance, layout rect, display:none pruning).

- [ ] **Step 4: Commit**

```bash
git add crates/fence/tests/cascade_spike.rs
git commit -m "test(fence): adapt cascade spike mini-bridge to TextNode + side tables"
```

---

## Task 5: pkg format v17 + bincode stability

Bump version and add stability tests.

**Files:**
- Modify: `crates/core/src/asset/mod.rs`
- Test: `crates/core/src/asset/tests.rs`

- [ ] **Step 1: Bump version constants**

In `crates/core/src/asset/mod.rs`:

```rust
pub const PKG_FORMAT_VERSION: u32 = 17; // v17: NodeKind unit-variant expansion + inherited_set + TemplateNode content/src
pub(crate) const MIN_VERSION: u32 = 17;
pub(crate) const MAX_VERSION: u32 = 17;
```

- [ ] **Step 2: Add NodeKind bincode stability test**

In `crates/core/src/scene/node.rs` tests (or a new test module), add:

```rust
    #[test]
    fn node_kind_all_variants_bincode_roundtrip() {
        let all = [
            NodeKind::Container, NodeKind::TextNode, NodeKind::TextBlock,
            NodeKind::TextElement, NodeKind::LineBreak, NodeKind::Label,
            NodeKind::Button, NodeKind::Link, NodeKind::Image,
            NodeKind::TextField, NodeKind::NumberField, NodeKind::Slider,
            NodeKind::Toggle, NodeKind::RadioButton, NodeKind::TextArea,
            NodeKind::Dropdown, NodeKind::OptionItem, NodeKind::ProgressBar,
            NodeKind::ListView, NodeKind::ListItem, NodeKind::Slot,
            NodeKind::CustomElement, NodeKind::Canvas,
        ];
        for k in all {
            let bytes = bincode::serialize(&k).unwrap();
            let back: NodeKind = bincode::deserialize(&bytes).unwrap();
            assert_eq!(k, back, "roundtrip failed for {:?}", k);
        }
    }

    #[test]
    fn node_kind_unit_variant_is_one_byte() {
        assert_eq!(bincode::serialize(&NodeKind::Container).unwrap().len(), 1);
    }
```

Note: `NodeKind` needs `Serialize`/`Deserialize` — add to derive list if not present. Check: current `NodeKind` does NOT derive Serialize/Deserialize (it's inside Node/TemplateNode which do via serde on the containing struct). Add `Serialize, Deserialize` to NodeKind derive.

- [ ] **Step 3: Add TemplateNode bincode roundtrip test**

In `crates/core/src/asset/tests.rs`, add:

```rust
    #[test]
    fn template_node_content_src_bincode_roundtrip() {
        let tn = TemplateNode {
            kind: NodeKind::TextNode,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
            content: Some("hello".into()),
            src: None,
        };
        let bytes = bincode::serialize(&tn).unwrap();
        let back: TemplateNode = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.content.as_deref(), Some("hello"));
        assert_eq!(back.kind, NodeKind::TextNode);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p loomgui_core`
Expected: all green including new stability tests.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/asset/mod.rs crates/core/src/asset/tests.rs crates/core/src/scene/node.rs
git commit -m "feat(core): bump pkg format to v17, add NodeKind/TemplateNode bincode stability tests"
```

---

## Task 6: fmt + clippy + full test green + dll rebuild

Final cleanup and verification.

**Files:** All touched files.

- [ ] **Step 1: Format check**

Run: `cargo fmt --all -- --check`
Expected: if errors, run `cargo fmt --all` to fix.

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: zero warnings. If clippy reports dead_code for rich_fragments or rich module, add `#[allow(dead_code)]` with a comment explaining retirement. If `field_reassign_with_default` or other lint in test code, add crate-level allow in tests or fix the pattern.

- [ ] **Step 3: Feature-gate check**

Run: `cargo build -p loomgui_core --no-default-features --all-targets`
Expected: compiles.

- [ ] **Step 4: Full workspace test**

Run: `cargo test`
Expected: all tests green across all crates.

- [ ] **Step 5: Build FFI + sync bindings**

Run:
```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
```

Copy the dll (Unity must be closed):
```bash
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: fmt + clippy + dll rebuild for Spec-2 core type refactor"
```

- [ ] **Step 7: Update roadmap status**

In `docs/roadmap/roadmap.md`, mark Spec-2 (①) as complete in §2 and §8 (key decisions). Add commit range.

```bash
git add docs/roadmap/roadmap.md
git commit -m "docs: roadmap — Spec-2 (① core type refactor) complete"
```
