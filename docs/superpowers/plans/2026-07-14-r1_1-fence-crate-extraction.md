# R1.1 fence crate extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract fence validation code into an independent `crates/fence/` crate, delete all old build-time HTML parsing code from core, and clean up packer + docs.

**Architecture:** fence crate depends on core (uses `style::resolved::ResolvedStyle`). core becomes pure runtime (no HTML/CSS parsing, no `parse` feature gate). packer loses HTML to pkg.bin compilation path (R3 rebuilds it). Documentation rewritten to reflect new 30-tag schema.

**Tech Stack:** Rust 2021, html5gum 0.8, cssparser 0.34, cargo workspace.

**Spec:** `docs/superpowers/specs/2026-07-14-r1_1-fence-crate-extraction-design.md`

**Parallelism:** Task 1 is prerequisite for all others. After Task 1: Tasks 2 then 3 (sequential), 4, 5, 6, 7 can all run in parallel. Task 8 is last.

---

## Task 1: Create fence crate + migrate source + migrate tests

> **Prerequisite for all other tasks.**

**Files:**
- Create: `crates/fence/Cargo.toml`
- Create: `crates/fence/src/lib.rs`
- Move: `crates/core/src/fence/*.rs` to `crates/fence/src/*.rs` (all 12 files)
- Move+Rename: `crates/core/tests/r1_schema_contract.rs` to `crates/fence/tests/schema_contract.rs`
- Move+Rename: `crates/core/tests/r1_pipeline.rs` to `crates/fence/tests/pipeline_integration.rs`
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/core/src/lib.rs`

- [ ] **Step 1: Create fence crate Cargo.toml**

Create `crates/fence/Cargo.toml`:

```
[package]
name = "loomgui_fence"
version = "0.1.0"
edition = "2021"

[dependencies]
loomgui_core = { path = "../core" }
html5gum = "0.8"
cssparser = "0.34"

[dev-dependencies]
insta = "1"
```

- [ ] **Step 2: Move source files**

```
New-Item -ItemType Directory -Path crates/fence/src/schema -Force
New-Item -ItemType Directory -Path crates/fence/tests -Force
Move-Item crates/core/src/fence/schema/tag.rs crates/fence/src/schema/tag.rs
Move-Item crates/core/src/fence/schema/attr.rs crates/fence/src/schema/attr.rs
Move-Item crates/core/src/fence/schema/css.rs crates/fence/src/schema/css.rs
Move-Item crates/core/src/fence/schema/mod.rs crates/fence/src/schema/mod.rs
Move-Item crates/core/src/fence/ir.rs crates/fence/src/ir.rs
Move-Item crates/core/src/fence/diagnostic.rs crates/fence/src/diagnostic.rs
Move-Item crates/core/src/fence/tree_builder.rs crates/fence/src/tree_builder.rs
Move-Item crates/core/src/fence/fence_gate.rs crates/fence/src/fence_gate.rs
Move-Item crates/core/src/fence/css_resolve.rs crates/fence/src/css_resolve.rs
Move-Item crates/core/src/fence/structural.rs crates/fence/src/structural.rs
Move-Item crates/core/src/fence/annotate.rs crates/fence/src/annotate.rs
Move-Item crates/core/src/fence/pipeline.rs crates/fence/src/pipeline.rs
```

- [ ] **Step 3: Create fence crate lib.rs**

Create `crates/fence/src/lib.rs`:

```
pub mod schema;
pub mod ir;
pub mod diagnostic;
pub mod tree_builder;
pub mod fence_gate;
pub mod css_resolve;
pub mod structural;
pub mod annotate;
pub mod pipeline;

pub use pipeline::{parse_template, ParsedTemplate};
pub use diagnostic::Diagnostic;
pub use ir::{IrTree, IrElement, IrNode, IrNodeKind};
pub use schema::{TagSpec, SemanticKind, Category, ContentModel};
```

- [ ] **Step 4: Fix imports crate:: to loomgui_core::**

The fence source files use `crate::style::...` to reference core types. After moving these become `loomgui_core::style::...`.

In `crates/fence/src/css_resolve.rs` change:
- `use crate::style::mapping::apply_decl;` to `use loomgui_core::style::mapping::apply_decl;`
- `use crate::style::resolved::{DisplayMode, ResolvedStyle};` to `use loomgui_core::style::resolved::{DisplayMode, ResolvedStyle};`

In `crates/fence/src/pipeline.rs` change:
- `use crate::style::mapping::parse_url;` to `use loomgui_core::style::mapping::parse_url;`
- `use crate::style::resolved::ResolvedStyle;` to `use loomgui_core::style::resolved::ResolvedStyle;`

Search for any other `crate::` references: `rg -n "use crate::" crates/fence/src/`. Internal fence references (`crate::schema`, `crate::ir`, `crate::diagnostic`) stay as `crate::`.

- [ ] **Step 5: Remove all cfg(feature = "parse") gates**

Remove every `#[cfg(feature = "parse")]` and `#![cfg(feature = "parse")]` from all fence source files. Verify: `rg -n "cfg.*feature.*parse" crates/fence/src/` returns zero matches.

- [ ] **Step 6: Move + rename tests**

```
Move-Item crates/core/tests/r1_schema_contract.rs crates/fence/tests/schema_contract.rs
Move-Item crates/core/tests/r1_pipeline.rs crates/fence/tests/pipeline_integration.rs
```

- [ ] **Step 7: Fix test imports**

Replace all `loomgui_core::fence::` with `loomgui_fence::` in both test files. Verify: `rg -n "loomgui_core::fence" crates/fence/tests/` returns zero.

- [ ] **Step 8: Add fence to workspace Cargo.toml**

Add `"crates/fence"` to the members list in root `Cargo.toml`.

- [ ] **Step 9: Remove fence module from core lib.rs**

Delete the `#[cfg(feature = "parse")] pub mod fence;` lines.

- [ ] **Step 10: Remove old fence test entries from core Cargo.toml**

Delete the `[[test]]` blocks for `r1_schema_contract` and `r1_pipeline`.

- [ ] **Step 11: Verify fence crate compiles**

Run: `cargo build -p loomgui_fence`

- [ ] **Step 12: Verify fence tests pass**

Run: `cargo test -p loomgui_fence`

- [ ] **Step 13: Verify core still compiles**

Run: `cargo build -p loomgui_core`

- [ ] **Step 14: Delete empty fence directory from core**

`Remove-Item crates/core/src/fence -Recurse -Force`

- [ ] **Step 15: Commit**

`git add -A; git commit -m "r1.1: extract fence crate from core"`

---

## Task 2: Core source surgery ? remove all build-time parse code

> **After Task 1.** Core will NOT compile after this task ? tests still reference deleted code. Task 3 fixes tests.

**Files:**
- Delete: `crates/core/src/parse/` (entire directory)
- Delete: `crates/core/src/style/cascade.rs`
- Modify: `crates/core/src/lib.rs`, `style/mod.rs`, `scene/mod.rs`, `scene/node.rs`, `text/rich.rs`, `asset/mod.rs`, `stage.rs`, `style/resolved.rs`, `style/mapping.rs`

- [ ] **Step 1: Delete parse/ module**

`Remove-Item crates/core/src/parse -Recurse -Force`

- [ ] **Step 2: Delete style/cascade.rs**

`Remove-Item crates/core/src/style/cascade.rs`

- [ ] **Step 3: Update lib.rs** ? delete `#[cfg(feature = "parse")] pub mod parse;`

- [ ] **Step 4: Update style/mod.rs** ? delete `pub mod cascade;`

- [ ] **Step 5: Update scene/mod.rs** ? delete `pub use node::build_scene;`

- [ ] **Step 6: Surgery on scene/node.rs**

Delete `build_scene()` function and `gather_rec()` function entirely. Delete the `rich_runs` override branch (was inside gather_rec). Delete any `use` imports only for build_scene (`ElementTree`, `ElementId`). Change module declarations at end: remove `#[cfg(all(test, feature = "parse"))] mod parse_tests;` line.

- [ ] **Step 7: Surgery on text/rich.rs**

Delete `parse_rich_markup()` and all helper functions. Delete all tests calling `parse_rich_markup` (roughly L584-811). Keep type definitions: `RichRun`, `RichKind`, `RichVAlign`, `RichWeight`, `RichStyle`, `RichDeco`, `RichBaseStyle`. Verify: `rg -n "parse_rich_markup" crates/core/src/` returns zero.

- [ ] **Step 8: Surgery on asset/mod.rs**

Delete `extract_component_css()` and `extract_dynamic_rules()`. Delete `use` imports only for these. Keep: `TemplateNode`, `PackageInput`, `ControllerEntry`, `write_package()`, `read_package()`. Verify: `rg -n "extract_component_css|extract_dynamic_rules|use crate::parse" crates/core/src/asset/mod.rs` returns zero.

- [ ] **Step 9: Surgery on stage.rs**

Delete `load_inline_for_test()` method. Delete `use` imports only for it.

- [ ] **Step 10: Clean desugar comments**

In `style/resolved.rs`: delete comments at ~L67 and ~L153 mentioning "desugar". In `style/mapping.rs`: clean comment at ~L660 mentioning "desugar".

- [ ] **Step 11: Commit**

`git add -A; git commit -m "r1.1: remove build-time parse code from core source"`

---

## Task 3: Core test + Cargo.toml cleanup

> **After Task 2.** Makes core compile and test again.

**Files:**
- Delete: `parse_tests.rs`, `fence_contract.rs`, `snapshot.rs`, `stage_getters.rs`, `v1e_dirty.rs`, `node_sort_keys.rs`, dump examples
- Modify: inline tests in mapping/tests.rs, dynamic.rs, asset/tests.rs, layout/mod.rs, stage/tests.rs, stage/instantiate_tests.rs, stage/dynamic_tests.rs, `Cargo.toml`

- [ ] **Step 1: Delete parse_tests.rs**

`Remove-Item crates/core/src/scene/node/parse_tests.rs`

- [ ] **Step 2: Delete old integration tests**

```
Remove-Item crates/core/tests/fence_contract.rs
Remove-Item crates/core/tests/snapshot.rs
Remove-Item crates/core/tests/stage_getters.rs
Remove-Item crates/core/tests/v1e_dirty.rs
Remove-Item crates/core/tests/node_sort_keys.rs
```

Keep `sdf_shader_contract.rs` (no parse dependency).

- [ ] **Step 3: Delete dump examples**

```
Remove-Item crates/core/examples/dump_showcase_text.rs
Remove-Item crates/core/examples/dump_render.rs
Remove-Item crates/core/examples/dump_interact.rs
```

- [ ] **Step 4: Clean inline tests**

For each file, search for parse references and remove entire test functions using them. Verify after: `rg -n "parse_html|parse_css|resolve_styles|build_scene|parse_selector|load_inline|parse::css|parse::dom|parse::selector" crates/core/src/` returns zero.

- [ ] **Step 5: Clean core Cargo.toml**

Remove: `scraper`, `cssparser`, `html5gum` deps. Remove entire `[features]` section. Remove all `[[bench]]`, `[[test]]`, `[[example]]` blocks with `required-features = ["parse"]`. Remove the comment block about parse-feature-gated targets.

- [ ] **Step 6: Verify core compiles**

`cargo build -p loomgui_core`

- [ ] **Step 7: Verify tests pass**

`cargo test -p loomgui_core`

- [ ] **Step 8: Verify clippy + fmt**

`cargo clippy -p loomgui_core --all-targets -- -D warnings` and `cargo fmt --all -- --check`

- [ ] **Step 9: Commit**

`git add -A; git commit -m "r1.1: clean up core tests + Cargo.toml"`

---

## Task 4: Packer cleanup

> **After Task 1. Parallel with Tasks 2-3.**

**Files:**
- Delete: `crates/packer/pkg/src/resolve.rs`
- Modify: `crates/packer/pkg/Cargo.toml`, `lib.rs`, `build.rs`, `main.rs`

- [ ] **Step 1: Update packer Cargo.toml**

Change `loomgui_core = { path = "../../core", features = ["parse"] }` to `loomgui_core = { path = "../../core" }`. Remove `scraper = "0.19"`.

- [ ] **Step 2: Delete resolve.rs**

`Remove-Item crates/packer/pkg/src/resolve.rs`

- [ ] **Step 3: Rewrite packer lib.rs**

Remove `pack()`, `scene_to_template()`, `desugar_block_divs()`, `collect_controller_pages()`, `strip_style_and_link()`, `serialize_children()`, `escape_text_into()`, `escape_attr_into()`, and all tests. Keep only: `pub mod atlas; pub mod build; pub mod runtime; pub mod workspace;`

- [ ] **Step 4: Rewrite packer build.rs**

Remove packages loop (the `for pkg in &ws.packages` block). Keep: workspace loading, atlas packing, fonts copying, runtime manifest. Remove `all_referenced` and cross-validation.

- [ ] **Step 5: Update main.rs**

Adjust success message to match simplified BuildReport.

- [ ] **Step 6: Verify**

`cargo build -p loomgui_pkg` and `cargo test -p loomgui_pkg`

- [ ] **Step 7: Commit**

`git add -A; git commit -m "r1.1: packer cleanup ? remove HTML compilation path"`

---

## Task 5: Rewrite docs/design/fence.md

> **Parallel with all other tasks. Can start immediately.**

**Files:** Rewrite: `docs/design/fence.md`

- [ ] **Step 1: Read schema source**

Read `crates/fence/src/schema/tag.rs`, `attr.rs`, `css.rs`, `pipeline.rs`, and `docs/design/main-design.md` section 3.

- [ ] **Step 2: Write new fence.md**

Complete rewrite. Delete old 4-tag fence content. New structure:

1. Design philosophy ? standard HTML semantics + AI priors, tags determine type, CSS grants behavior
2. Authoritative source ? `crates/fence/src/schema/` Rust const tables
3. Element table ? all 30 tags (7 shell + 23 runtime) with SemanticKind, Category, ContentModel, DisplayDefault
4. Stable semantic signatures ? tag + immutable structural attributes determine type
5. CSS three orthogonal dimensions ? CssPropSpec, CssValueParser, ShorthandSpec
6. 6-stage pipeline ? tree_builder through pipeline, collect-all-diagnostics
7. Failure strategy ? compile-time errors, not silent degradation
8. Attribute fence ? per-tag attribute specs with value domains

- [ ] **Step 3: Verify no old references**

`rg -n "FENCE_TAGS|display:block.*desugar|raw_rich|rich_runs|data-widget|???|flex-only|inline mix" docs/design/fence.md` returns zero.

- [ ] **Step 4: Commit**

`git add docs/design/fence.md; git commit -m "docs: rewrite fence.md for new 30-tag schema"`

---

## Task 6: Rewrite packer templates

> **Parallel with all other tasks. Can start immediately.**

**Files:** Rewrite: `workspace-CLAUDE.md`, `skill/SKILL.md`

- [ ] **Step 1: Read current templates**

Read both files in `crates/packer/gui/src-tauri/templates/`.

- [ ] **Step 2: Read new fence rules**

Read `docs/design/fence.md` (if Task 5 done) or `main-design.md` section 3 + fence crate schema source.

- [ ] **Step 3: Rewrite workspace-CLAUDE.md**

Replace old fence section (references "div/span/img/button", "flex-only", "inline mix") with: standard HTML block/inline semantics, `display:flex` defaults row, all 30 fence tags, `overflow:auto/scroll` controls scroll, fence violations are compile-time errors, point to `fence.md`. Keep non-fence sections.

- [ ] **Step 4: Rewrite skill/SKILL.md**

Replace old content with: standard HTML/CSS game UI, 30 fence tags, `display:flex` defaults row, CSS fence whitelist, fence violations cause build failure, "AI can predict rendering from HTML".

- [ ] **Step 5: Verify no old references**

`rg -n "div.*span.*img.*button|flex-only|inline mix|display:block.*desugar|data-widget" crates/packer/gui/src-tauri/templates/` returns zero.

- [ ] **Step 6: Commit**

`git add crates/packer/gui/src-tauri/templates/; git commit -m "docs: rewrite packer templates for new fence schema"`

---

## Task 7: Deferred validation ? ARIA + template root + label[for]

> **After Task 1. Parallel with Tasks 2-6.**

**Files:** Modify: `structural.rs`, `diagnostic.rs`. Create: `tests/structural_contract.rs`

- [ ] **Step 1: Read current structural.rs and diagnostic.rs**

Understand DiagnosticCode enum, how validation collects diagnostics, how IrTree is traversed, how IDs are collected.

- [ ] **Step 2: Add new DiagnosticCode variants**

Add `AriaRefNotFound`, `TemplateRootMustBeLi`, `LabelForNotFound` to the enum.

- [ ] **Step 3: Write failing tests**

Create `crates/fence/tests/structural_contract.rs` with tests: aria-controls missing/valid target, aria-labelledby missing target, template root not-li/li inside ul, label[for] missing/valid target.

- [ ] **Step 4: Run tests to verify they fail**

`cargo test -p loomgui_fence --test structural_contract`

- [ ] **Step 5: Implement ARIA reference validation**

Collect all IDs, check aria-controls/aria-labelledby targets exist, emit AriaRefNotFound if missing.

- [ ] **Step 6: Implement template root validation**

Find ul/ol, check template children root is li, emit TemplateRootMustBeLi if not.

- [ ] **Step 7: Implement label[for] validation**

Check label[for] targets exist, emit LabelForNotFound if missing.

- [ ] **Step 8: Run tests to verify they pass**

`cargo test -p loomgui_fence --test structural_contract`

- [ ] **Step 9: Run full fence test suite**

`cargo test -p loomgui_fence`

- [ ] **Step 10: Commit**

`git add -A; git commit -m "r1.1: deferred validation ? ARIA, template root, label[for]"`

---

## Task 8: Final consistency ? docs + roadmap + AGENTS.md

> **After all other tasks.**

**Files:** Modify: `main-design.md`, `roadmap.md`, `AGENTS.md`

- [ ] **Step 1: Check main-design.md consistency**

Verify section 3.6 points to fence.md accurately, no old references, section 14.1 consistent with code.

- [ ] **Step 2: Update roadmap R1.1 status**

Mark completed items, mark deferred items with target phase (Custom Element registration to R3).

- [ ] **Step 3: Update AGENTS.md**

Remove parse feature section, remove old fence description, update build commands, update fence test paths.

- [ ] **Step 4: Verify full workspace**

`cargo build`, `cargo test`, `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`

- [ ] **Step 5: Final commit**

`git add -A; git commit -m "r1.1: final consistency ? update docs, roadmap, AGENTS.md"`

---

## Self-Review

**Spec coverage:** All spec sections covered ? crate structure (Task 1), core cleanup (Tasks 2-3), packer (Task 4), docs (Tasks 5-6, 8), deferred validation (Task 7). Custom Element registration deferred to R3.

**Placeholder scan:** No TBD/TODO. Each step has exact paths, commands, or code.

**Type consistency:** `loomgui_fence` used consistently. DiagnosticCode variants match between tests and implementation.
