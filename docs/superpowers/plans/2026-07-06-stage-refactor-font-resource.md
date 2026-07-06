# LoomStage Refactor + Multi-Font + Resource Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor LoomStage into a pure C# class + LoomStageDriver MonoBehaviour, add multi-font selection (FontTable keyed by CSS font-family), and restructure all LoomGUI resources into a self-built `Assets/LoomGUI/Bundles/` directory (atlas/ui/fonts) with zero asset references in LoomSettings — so projects can consume LoomGUI resources via their own AssetBundle/Addressables strategy instead of being locked into StreamingAssets.

**Architecture:** Two sequenced halves. **Half A (Rust core + FFI):** replace `Stage.font: Arc<Font>` with `fonts: FontTable` (HashMap<family, Arc<Font>> + default), thread `&FontTable` through `solve`/`build_render_nodes`, select font per-node by `ts.font_family`; change `loomgui_stage_new(font_path,...)` to `loomgui_stage_new(w,h)` + `loomgui_stage_register_font`. **Half B (Unity):** split LoomStage (pure class, no Unity deps) from LoomStageDriver (MonoBehaviour, owns Camera/transform/FPS + three virtual load hooks LoadFont/LoadPackageBytes/LoadSpriteAtlas); strip `atlas`/`font` serialized references from LoomSettings (pure config, editor panel loads refs transiently via AssetDatabase); relocate all products to `Bundles/{atlas,ui,fonts}/`; SpriteResolver switches to name→atlasName mapping + driver.LoadSpriteAtlas.

**Tech Stack:** Rust edition 2021 (loomgui_core / loomgui_ffi_c), taffy 0.5, ttf-parser 0.20, csbindgen 1; Unity 6.5 URP (C#), SpriteAtlas V2 API.

## Global Constraints

(From spec §1-§9 + CLAUDE.md. Every task implicitly includes these.)

- **Language/comments**: user reads Chinese — user-facing Q&A/summaries in Chinese; **code/commits in English**. Code comments are production-quality, self-contained, explain WHY not WHAT, **never reference internal pitfall numbers or codenames** (no "坑 102", no "v1.5"). Pitfall numbers stay in `docs/pitfalls.md`.
- **Two-machine workflow**: this machine is the only coding machine (builds `.dll` + commits + pushes); home machine does Unity PlayMode acceptance. **Any Rust change → rebuild release `.dll` + copy to `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` + commit** or home machine can't test. Close Unity before copying `.dll` (it locks the file).
- **FFI entry never panics**: cdylib `.unwrap`/`.expect` on None aborts the host (Unity crash). Null handle / invalid NodeId / missing scene → early-return sentinel/zero. Never `.expect` across FFI.
- **FFI strings**: ptr+len, never NUL-terminated. C# reads via `Span<byte>` + `BinaryPrimitives` (no `Marshal.PtrToStructure`).
- **`borrow_*` out_len is COUNT not bytes**: write record count; C# reads `count * size_of::<T>()`.
- **LoomGUIBindings.cs is csbindgen-generated + gitignored**: never hand-edit. Add `#[no_mangle] pub extern "C" fn` in `loomgui_ffi_c/src/lib.rs`; `cargo build` regenerates bindings.
- **Pre-push gates**: `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (workspace). CI is strict. Run before any push.
- **Fence gate**: after touching `apply_decl`/selectors/`FENCE_TAGS`, run `cargo test -p loomgui_core fence_contract`. (This plan does not touch fence, but listed for safety.)
- **Resource model invariants (spec §1-§2)**: LoomSettings.asset holds ZERO `UnityEngine.Object` references (no `SpriteAtlas atlas`, no `Font font` fields). All asset refs are editor-transient (`[NonSerialized]` or locals). All products land in `Assets/LoomGUI/Bundles/{atlas,ui,fonts}/`. Nothing in StreamingAssets. `pkgOutputDir` default = `Assets/LoomGUI/Bundles/`.
- **Multi-font invariants (spec §5)**: `measure_text` signature unchanged (still `font: &Font`); callers `select` first. Rust `Font` still `Face<'static>` via `Box::leak` (acceptable, YAGNI to reclaim). Two font copies (`.bytes` for Rust measure, `Font` asset for Unity raster) must be the same ttf.
- **v1.5 Controller API preserved**: v1.5 Controller + CSS transition already shipped (Rust registry + FFI `get_controller`/`set_selected_index`/`get_selected_index`/`borrow_controller_changed_events` + C# wrappers + event dispatch). The LoomStage split MUST carry these APIs into the pure class verbatim — do not drop, do not pre-reserve. v1.5-b Transition timeline is out of scope.
- **Edition 2021**, pinned deps: taffy 0.5, ttf-parser 0.20, cssparser 0.34, scraper 0.19, slotmap 1.1, csbindgen 1.

## Spec Calibration Notes

(Where this plan refines the spec after code investigation.)

1. **`Stage::new(font_path, root_size)` → `Stage::new(root_size)`** + `pub fn register_font(&mut self, family: &str, bytes: Vec<u8>, is_default: bool)`. Internal field `font: Arc<Font>` → `fonts: FontTable`. All `&self.font` call sites (`solve`, `build_render_nodes` in stage.rs:557/567) pass `&self.fonts`.
2. **`FontTable::select` returns `&Font`** (not `&Arc<Font>`), so `measure_text(..., font)` needs no signature change — caller dereferences. Two call sites: `layout/mod.rs` measure closure (line ~248) and `render/mod.rs` `NodeKind::Text` fallback (line ~312).
3. **`MeasureContext::Text` gains `family: Option<String>`** — filled from `node.style.font_family.clone()` in `layout/mod.rs:119-126`. The measure closure destructures it and calls `fonts.select(ctx.family.as_deref())`.
4. **`Font::from_path` stays** (tests use it via `Stage::new` with path). Add `FontTable::new()` + `register` + `select`. `Stage::new` no longer takes a path; tests calling `Stage::new(font_path, ...)` migrate to `Stage::new(...)` + `register_font("default", fs::read(font_path), true)`.
5. **C# `LoomStage` pure class** holds `StageHandle* _stage` + `MirrorPool _pool` + `MaterialManager _mm` + `NativeHostManager _nhm` + `SpriteResolver _sprites` + `Dictionary<string,Font> _unityFonts` + `Font _defaultUnityFont` + `int _fontVersion`. `LoomStageDriver` (new MonoBehaviour) holds `LoomStage _stage` + `_designSize`/`_uiCamera`/`_showFps`/`_safeArea`/`_inputCollector` + the three virtual load hooks.
6. **`SpriteResolver.Init` switches to name mapping**: builds `folder → atlasName` (string) from `LoomSettings.atlasEntries`. `GetSprite(path)` resolves atlasName → calls an injected `Func<string, SpriteAtlas>` (driver's `LoadSpriteAtlas`) + caches. No `SpriteAtlas` references held in settings or resolver.
7. **`LoomAtlasSync.EnsureAtlasAsset`** currently writes to `{workspaceDir}/atlas/`. Change to `{pkgOutputDir}/atlas/` (= `Bundles/atlas/`). `DeleteAutoAtlas` + `ResolveAtlasPath` follow. PNG Sprite sources (packables) stay in `res/`.
8. **`PackPackage` output** `{pkgOutputDir}/{name}.pkg.bin` → `{pkgOutputDir}/ui/{name}.pkg.bin`. `LoomConfigExporter.BuildJson` `output_dir` becomes `{pkgOutputDir}/ui/` relative to workspace (config.json is consumed by the exe, which writes there).
9. **ShowcaseDriver `LoadPkgBytes`** reads `StreamingAssets/showcase.pkg.bin` → switch to `LoadPackageBytes("showcase")` (driver default reads `Bundles/ui/showcase.pkg.bin`). ShowcaseDriver moves to `loomgui_unity/Assets/LoomUI/Demo/` + `LoomGUI.Demo.asmdef`.
10. **SampleScene** breaks (LoomStage MonoBehaviour → class). Migration is a manual Unity editor step in a dedicated task — not automatable via code.

## File Structure

**Modify (Rust core):**
- `loomgui_core/src/text/layout.rs` — (no signature change; `Font` stays. Possibly add `FontTable` here or in a new module.) Add `FontTable` struct + `new`/`register`/`select`.
- `loomgui_core/src/layout/mod.rs` — `MeasureContext::Text` gains `family`; `solve(scene, &FontTable, ...)`; measure closure selects font.
- `loomgui_core/src/render/mod.rs` — `build_render_nodes(scene, &FontTable, ...)`; `NodeKind::Text` fallback selects font.
- `loomgui_core/src/stage.rs` — `Stage::new(root_size)` + `register_font`; `font: Arc<Font>` → `fonts: FontTable`; `tick_and_render` passes `&self.fonts`. All tests migrate.
- `loomgui_ffi_c/src/lib.rs` — `loomgui_stage_new(w,h)` (drop font_path); add `loomgui_stage_register_font`. Update `abi_tests.rs`.

**Modify (Unity):**
- `loomgui_unity_package/Runtime/LoomStage.cs` — rewrite as pure class. Carries all existing APIs (CreateNode/LoadPackage/Tween/Controller/...) + `RegisterFont` + `Tick(dt, driverCtx)` + `FontVersion` + `_unityFonts`.
- `loomgui_unity_package/Runtime/LoomStageDriver.cs` — NEW MonoBehaviour. Lifecycle + three virtual load hooks.
- `loomgui_unity_package/Runtime/LoomSettings.cs` — strip `AtlasEntry.atlas` + `FontEntry.font`; add `FontEntry.sourceFileName`; add `fonts` list; change `pkgOutputDir` default.
- `loomgui_unity_package/Runtime/SpriteResolver.cs` — name mapping + injected `Func<string,SpriteAtlas>`.
- `loomgui_unity_package/Runtime/TextRasterizer.cs` — drop static `s_fontVersion`/`FontVersion`/`OnRebuilt`/`ResetStatic` (moves to LoomStage instance).
- `loomgui_unity_package/Runtime/MirrorPool.cs` — `Sync` reads `stage.FontVersion` (instance) instead of `TextRasterizer.FontVersion` (static); text raster selects Font by family.
- `loomgui_unity_package/Editor/LoomSettingsWindow.cs` — add Fonts tab; add Publish button; strip atlas/font ref UI to transient; route PublishFonts.
- `loomgui_unity_package/Editor/LoomAtlasSync.cs` — output to `Bundles/atlas/`; drop `entry.atlas =` writes (no field).
- `loomgui_unity_package/Editor/LoomConfigExporter.cs` — `output_dir` → `{pkgOutputDir}/ui/`.
- `loomgui_unity_package/Editor/LoomWorkspaceAssetPostprocessor.cs` — already modified on branch; verify no atlas/font ref assumptions.

**Create (Unity):**
- `loomgui_unity/Assets/LoomUI/Demo/LoomShowcaseDriver.cs` — relocated.
- `loomgui_unity/Assets/LoomUI/Demo/VirtualListDriver.cs` — relocated.
- `loomgui_unity/Assets/LoomUI/Demo/LoomGUI.Demo.asmdef` — references LoomGUI Runtime assembly.

**Delete (Unity):**
- `loomgui_unity_package/Runtime/LoomShowcaseDriver.cs` (moved).

**Modify (CI/docs):**
- `.github/workflows/rust-ci.yml` — add `loomgui_pkg.exe` artifact (spec §8 #1).
- `docs/design/fence.md` + skill docs — out of scope for THIS plan (tracked as follow-up per spec §8 #2/#3). Listed here for visibility only.

---

## Half A — Rust Core + FFI

### Task A1: FontTable struct + select

**Files:**
- Modify: `loomgui_core/src/text/layout.rs` (add `FontTable` after `Font` impl, ~line 100)
- Test: `loomgui_core/src/text/layout.rs` (inline `#[cfg(test)]` module)

**Interfaces:**
- Produces: `pub struct FontTable { fonts: HashMap<String, Arc<Font>>, default_family: Option<String> }`; `FontTable::new() -> Self`; `FontTable::register(&mut self, family: &str, bytes: Vec<u8>, is_default: bool) -> Result<(), String>`; `FontTable::select(&self, family: Option<&str>) -> &Font`. First `register(is_default=true)` sets `default_family`.

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)]` module in `layout.rs`:

```rust
#[test]
fn font_table_select_returns_default_when_no_family() {
    let mut t = FontTable::new();
    t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
    let f = t.select(None);
    assert!((f.ascent(16.0) - ascent_dejavu_16()).abs() < 0.01,
        "select(None) must return default font");
}

#[test]
fn font_table_select_falls_back_when_family_missing() {
    let mut t = FontTable::new();
    t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
    let f = t.select(Some("Nonexistent"));
    // Falls back to default.
    assert!((f.ascent(16.0) - ascent_dejavu_16()).abs() < 0.01);
}

#[test]
fn font_table_select_returns_named_when_present() {
    let mut t = FontTable::new();
    t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
    t.register("Other", font_bytes_dejavu(), false).unwrap(); // same file, diff family
    let f = t.select(Some("Other"));
    // "Other" registered → returned (same metrics here, but distinct entry).
    assert!(t.fonts.contains_key("Other"));
    let _ = f; // selected font is valid
}

#[test]
fn font_table_register_is_default_sets_default() {
    let mut t = FontTable::new();
    t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
    assert_eq!(t.default_family.as_deref(), Some("DejaVu"));
}
```

Use the existing test helper pattern in `layout.rs` (tests already read `tests/fixtures/DejaVuSans.ttf` — see line ~370 `let layout = measure_text("Hello", 16.0, ..., &font)`). Add helpers:

```rust
fn font_bytes_dejavu() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf")).unwrap()
}
fn ascent_dejavu_16() -> f32 {
    // Compute once from a direct Font::from_bytes to avoid circularity.
    let f = Font::from_bytes(font_bytes_dejavu()).unwrap();
    f.ascent(16.0)
}
```

For the empty-table case, `select` on a table with no default: returning a `&Font` is impossible (no font to borrow). `select` returns `&Font` but **panics** if no default registered (FFI layer guarantees a default is registered before any tick that measures text). Document this precondition. Test the panic case with `#[should_panic]`:

```rust
#[test]
#[should_panic(expected = "no default font")]
fn font_table_select_panics_without_default() {
    let t = FontTable::new();
    t.select(None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p loomgui_core font_table`
Expected: FAIL — `FontTable` not defined.

- [ ] **Step 3: Implement FontTable**

Add after the `Font` impl in `layout.rs` (after `from_bytes`/`ascent`/etc., before `measure_text`):

```rust
use std::collections::HashMap;
use std::sync::Arc;

/// 字体表：CSS font-family → Font。无匹配 / None → default 字体。
///
/// 注册第一个 is_default=true 的字体为 default。select 在无 default 时 panic——
/// FFI 层保证任何 tick（会触发 measure）前已注册 default，契约由调用方维护。
/// Font 仍是 Face<'static>（Box::leak 字节，进程级单字体可接受；多字体数量有限，
/// leak 不释放可接受，真要回收改 Arc<Vec<u8>> 持字节，YAGNI）。
pub struct FontTable {
    fonts: HashMap<String, Arc<Font>>,
    default_family: Option<String>,
}

impl FontTable {
    pub fn new() -> Self {
        FontTable { fonts: HashMap::new(), default_family: None }
    }

    /// 注册字体。is_default=true 设为默认（首次或显式覆盖）。
    /// bytes 是 ttf/ttc/otf 字节；Face::parse 失败返 Err。
    pub fn register(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        is_default: bool,
    ) -> Result<(), String> {
        let font = Arc::new(Font::from_bytes(bytes)?);
        self.fonts.insert(family.to_string(), font);
        if is_default {
            self.default_family = Some(family.to_string());
        }
        Ok(())
    }

    /// 按节点 font_family 选字体。None / 无匹配 → default。
    /// 无 default 注册时 panic（契约：FFI 层 tick 前须注册 default）。
    pub fn select(&self, family: Option<&str>) -> &Font {
        if let Some(fam) = family {
            if let Some(f) = self.fonts.get(fam) {
                return f;
            }
        }
        let default = self.default_family.as_ref()
            .expect("no default font registered (register one with is_default=true before tick)");
        &self.fonts[default]
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p loomgui_core font_table`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add loomgui_core/src/text/layout.rs
git commit -m "feat(core): FontTable — multi-font registry keyed by CSS font-family"
```
### Task A2: Thread FontTable through solve + render + MeasureContext

**Files:**
- Modify: `loomgui_core/src/layout/mod.rs:51` (MeasureContext enum), `:80` (solve sig), `:119-126` (Text ctx build), `:248-257` (measure closure)
- Modify: `loomgui_core/src/render/mod.rs:98` (build_render_nodes sig), `:312-321` (Text fallback measure)
- Modify: `loomgui_core/src/render/tests.rs` (call sites)
- Test: `loomgui_core/src/layout/mod.rs` + `loomgui_core/src/render/tests.rs`

**Interfaces:**
- Consumes: `FontTable` (Task A1).
- Produces: `solve(scene, &FontTable, root_size, image_sizes)`; `build_render_nodes(scene, &FontTable, &prev_hashes, image_sizes)`; `MeasureContext::Text { ..., family: Option<String> }`.

- [ ] **Step 1: Add family to MeasureContext::Text**

In `layout/mod.rs:51` (enum `MeasureContext`), add `family` to the `Text` variant:

```rust
enum MeasureContext {
    Text {
        content: String,
        font_size: f32,
        line_height: f32,
        letter_spacing: f32,
        align: TextAlign,
        nowrap: bool,
        family: Option<String>,   // node.style.font_family
    },
    Image { iw: f32, ih: f32, w_dim: LengthPercentageAuto, h_dim: LengthPercentageAuto },
}
```

At the build site (`layout/mod.rs:117-127`), fill `family`:

```rust
NodeKind::Text { content } => {
    let s = &node.style;
    Some(MeasureContext::Text {
        content: content.clone(),
        font_size: s.font_size,
        line_height: s.line_height,
        letter_spacing: s.letter_spacing,
        align: s.text_align,
        nowrap: s.white_space_nowrap,
        family: s.font_family.clone(),
    })
}
```

- [ ] **Step 2: Change solve signature to &FontTable + select in closure**

In `layout/mod.rs:29` update the `use`:
```rust
use crate::text::layout::{measure_text, FontTable, TextLayout};
```

In `layout/mod.rs:80`:
```rust
pub fn solve(scene: &mut Scene, fonts: &FontTable, root_size: (f32, f32), image_sizes: &ImageSizeTable) {
```

In the measure closure (`layout/mod.rs:240-257`), select font by family then call measure_text:

```rust
Some(MeasureContext::Text {
    content, font_size, line_height, letter_spacing, align, nowrap, family,
}) => {
    let font = fonts.select(family.as_deref());
    let layout = measure_text(
        content, *font_size, *line_height, *letter_spacing,
        *align, *nowrap, known.width, font,
    );
    // ... (rest unchanged: store TextLayout into text_layouts slot)
```

- [ ] **Step 3: Change build_render_nodes signature + Text fallback select**

In `render/mod.rs:22` update the `use`:
```rust
use crate::text::layout::{measure_text, FontTable};
```

In `render/mod.rs:98`:
```rust
pub fn build_render_nodes(
    scene: &Scene,
    fonts: &FontTable,
    prev_node_hashes: &HashMap<u32, (u64, u64)>,
    image_sizes: &ImageSizeTable,
) -> (FrameData, HashMap<u32, (u64, u64)>, HashMap<u32, f32>) {
```

In the `NodeKind::Text` fallback (`render/mod.rs:304-322`):
```rust
NodeKind::Text { content } => {
    let s = &n.style;
    let font = fonts.select(s.font_family.as_deref());
    let mut layout = scene
        .text_layouts
        .get(n.id.index())
        .cloned()
        .flatten()
        .unwrap_or_else(|| {
            measure_text(
                content, s.font_size, s.line_height, s.letter_spacing,
                s.text_align, s.white_space_nowrap, Some(rect.w), font,
            )
        });
    // ... (rest unchanged)
```

- [ ] **Step 4: Update internal callers / tests**

Run: `grep -rn "solve(.*&font\|build_render_nodes(.*&font\|: &Font," loomgui_core/src` to find every call site passing a `&Font`. For each, build a `FontTable` and pass `&fonts`:

```rust
// Before: let font = Font::from_path(font_path)?; solve(scene, &font, ...);
// After:
let mut fonts = FontTable::new();
fonts.register("DejaVu", std::fs::read(font_path).unwrap(), true).unwrap();
solve(scene, &fonts, (200.0, 200.0), &image_sizes);
```

Update `render/tests.rs:765` and any `solve` test calls the same way.

- [ ] **Step 5: Run core tests**

Run: `cargo test -p loomgui_core`
Expected: PASS. If a test panics on "no default font", it missed `register_font` — add it.

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add loomgui_core/src/layout/mod.rs loomgui_core/src/render/mod.rs loomgui_core/src/render/tests.rs
git commit -m "feat(core): thread FontTable through solve/build_render_nodes, select per-node font"
```

### Task A3: Stage::new without font_path + register_font

**Files:**
- Modify: `loomgui_core/src/stage.rs` (~line 30 field, `:54-73` new, `:557`/`:567` tick_and_render)
- Modify: `loomgui_core/src/stage/tests.rs`, `loomgui_core/src/stage/instantiate_tests.rs`, `loomgui_core/src/scroll/tests.rs` (~20 `Stage::new` call sites)
- Test: `loomgui_core/src/stage/tests.rs`

**Interfaces:**
- Consumes: `FontTable` (A1), `solve`/`build_render_nodes` &FontTable sig (A2).
- Produces: `Stage::new(root_size: (f32,f32)) -> Result<Self,String>`; `Stage::register_font(&mut self, family: &str, bytes: Vec<u8>, is_default: bool) -> Result<(),String>`; field `fonts: FontTable`.

- [ ] **Step 1: Write a failing test for register_font**

In `stage/tests.rs`, add:

```rust
#[test]
fn stage_register_font_sets_default_for_measure() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((200.0, 200.0)).expect("stage");
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true).unwrap();
    let tree = loomgui_core::parse::dom::parse_html("<div>Hello</div>").unwrap();
    let sheet = loomgui_core::parse::css::parse_css("").unwrap();
    let styles = loomgui_core::style::cascade::resolve_styles(&tree, &sheet);
    s.scene = Some(loomgui_core::scene::node::build_scene(&tree, &styles));
    s.advance_time(0.016);
    let _frame = s.tick_and_render();  // must not panic on "no default font"
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p loomgui_core stage_register_font`
Expected: FAIL — `Stage::new` still takes `font_path` (compile error).

- [ ] **Step 3: Change Stage::new + add register_font + change field**

In `stage.rs` struct field (~line 30): `font: Arc<Font>` → `fonts: FontTable`.

In `stage.rs:54-73`:
```rust
pub fn new(root_size: (f32, f32)) -> Result<Self, String> {
    Ok(Stage {
        scene: None,
        fonts: FontTable::new(),
        root_size,
        packages: std::collections::HashMap::new(),
        image_sizes: std::collections::HashMap::new(),
        pointer_state: PointerState::new(),
        pending_input: Vec::new(),
        last_events: Vec::new(),
        pending_keys: Vec::new(),
        pending_wheel: Vec::new(),
        pending_focus_request: None,
        tweens: crate::tween::TweenManager::new(),
        pending_dt: 0.0,
        prev_node_hashes: std::collections::HashMap::new(),
    })
}

/// 注册字体进字体表。is_default=true 设为默认（measure 的 fallback）。
/// FFI 层在首次 tick 前必须注册至少一个 default 字体，否则 measure 时 select panic。
pub fn register_font(
    &mut self,
    family: &str,
    bytes: Vec<u8>,
    is_default: bool,
) -> Result<(), String> {
    self.fonts.register(family, bytes, is_default)
}
```

Update `use` imports: add `FontTable`, remove `Arc`/`Font` if now unused.

In `tick_and_render` (`stage.rs:557`/`:567`):
```rust
solve(scene, &self.fonts, self.root_size, &self.image_sizes);
// ...
let (frame, new_hashes, sort_keys) =
    build_render_nodes(scene, &self.fonts, &self.prev_node_hashes, &self.image_sizes);
```

- [ ] **Step 4: Migrate all test call sites**

Run: `grep -rn "Stage::new(" loomgui_core/src`

For each `Stage::new(font_path, (w, h))`, change to:
```rust
let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
let mut s = Stage::new((200.0, 200.0)).unwrap();
s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true).unwrap();
```

(Keep the existing `font_path` line; add `register_font` right after `Stage::new`.)

- [ ] **Step 5: Run all core tests**

Run: `cargo test -p loomgui_core`
Expected: PASS. Any "no default font" panic → that test missed `register_font`, add it.

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add loomgui_core/src/stage.rs loomgui_core/src/stage/tests.rs loomgui_core/src/stage/instantiate_tests.rs loomgui_core/src/scroll/tests.rs
git commit -m "feat(core): Stage::new drops font_path, add register_font + FontTable field"
```
### Task A4: FFI — loomgui_stage_new(w,h) + loomgui_stage_register_font

**Files:**
- Modify: `loomgui_ffi_c/src/lib.rs:45-70` (`loomgui_stage_new`), add `loomgui_stage_register_font` after it
- Modify: `loomgui_ffi_c/src/abi_tests.rs` (update `stage_new` calls)
- Modify: `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` (rebuild + copy)
- Test: `loomgui_ffi_c/src/abi_tests.rs`

**Interfaces:**
- Consumes: `Stage::new(root_size)` + `Stage::register_font` (A3).
- Produces (C ABI, csbindgen-regenerated bindings): `loomgui_stage_new(w: f32, h: f32) -> *mut StageHandle`; `loomgui_stage_register_font(h, family_ptr, family_len, bytes_ptr, bytes_len, is_default: u8) -> i32`.

- [ ] **Step 1: Write a failing abi test**

In `abi_tests.rs`, find an existing `loomgui_stage_new` test call (it currently passes a font path). Add a new test:

```rust
#[test]
fn stage_new_without_font_then_register_font_measures() {
    let stage = unsafe { loomgui_ffi_c::loomgui_stage_new(200.0, 200.0) };
    assert!(!stage.is_null(), "stage_new must succeed without font path");
    let font_bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"),
        "/../loomgui_core/tests/fixtures/DejaVuSans.ttf")).unwrap();
    let family = b"DejaVu";
    let rc = unsafe {
        loomgui_ffi_c::loomgui_stage_register_font(
            stage, family.as_ptr(), family.len(), font_bytes.as_ptr(), font_bytes.len(), 1)
    };
    assert_eq!(rc, 0, "register_font must return 0 on valid ttf");
    unsafe { loomgui_ffi_c::loomgui_stage_free(stage) };
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p loomgui_ffi_c stage_new_without_font`
Expected: FAIL — `loomgui_stage_new` still takes `font_path` (compile error).

- [ ] **Step 3: Change loomgui_stage_new + add register_font**

In `lib.rs:45-70`, replace the existing `loomgui_stage_new`:

```rust
/// 创建 Stage 句柄（不收字体路径）。字体由 loomgui_stage_register_font 单独注册。
/// 失败返 null（当前 new 不返 Err，留 null 分支对称）。
#[no_mangle]
pub extern "C" fn loomgui_stage_new(w: f32, h: f32) -> *mut StageHandle {
    let stage = match Stage::new((w, h)) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    Box::into_raw(Box::new(StageHandle {
        stage,
        frame_blob: Vec::new(),
        dump_blob: CString::new("").unwrap(),
    }))
}

/// 注册字体进 Stage 字体表。family = UTF-8（指针+len），bytes = ttf/ttc/otf 字节。
/// is_default: 0=否，非 0=是（设为默认 fallback）。返 0=ok，-1=err（null 句柄/非 UTF-8/parse 失败）。
#[no_mangle]
pub extern "C" fn loomgui_stage_register_font(
    h: *mut StageHandle,
    family: *const u8,
    family_len: usize,
    bytes: *const u8,
    bytes_len: usize,
    is_default: u8,
) -> i32 {
    if h.is_null() || family.is_null() || bytes.is_null() {
        return -1;
    }
    let sh = unsafe { &mut *h };
    let family = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(family, family_len) }) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) }.to_vec();
    match sh.stage.register_font(family, bytes, is_default != 0) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
```

- [ ] **Step 4: Migrate abi_tests.rs existing stage_new calls**

Run: `grep -rn "loomgui_stage_new(" loomgui_ffi_c/src`

Every existing call passes a font path + 2 floats. Change to `loomgui_stage_new(w, h)` + a `loomgui_stage_register_font` follow-up (read the same fixture ttf). Keep tests semantically equivalent.

- [ ] **Step 5: Run ffi tests + fmt/clippy**

Run: `cargo test -p loomgui_ffi_c && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS + clean.

- [ ] **Step 6: Rebuild release dll + copy + verify symbol**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
# Verify new symbol exported (Windows):
nm target/release/loomgui_ffi_c.dll 2>/dev/null | grep -i register_font || \
  dumpbin //exports target/release/loomgui_ffi_c.dll | findstr register_font
```
Expected: `loomgui_stage_register_font` present. (Unity must be CLOSED when copying — it locks the dll.)

- [ ] **Step 7: Commit (dll + Rust + bindings regen)**

```bash
git add loomgui_ffi_c/src/lib.rs loomgui_ffi_c/src/abi_tests.rs loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "feat(ffi): loomgui_stage_new(w,h) + loomgui_stage_register_font (multi-font)"
```

Bindings (`LoomGUIBindings.cs`) regenerate on next `cargo build` and are gitignored — no commit needed for them, but the Unity compile in Task B5 will pick them up.

---

## Half B — Unity

### Task B1: LoomSettings pure config (strip asset references)

**Files:**
- Modify: `loomgui_unity_package/Runtime/LoomSettings.cs` (`AtlasEntry.atlas` field, `FontEntry` struct, `pkgOutputDir` default)
- Test: `loomgui_unity_package/Tests/LoomConfigExporterTests.cs` (adapt to no atlas/font refs)

**Interfaces:**
- Consumes: none.
- Produces: `AtlasEntry` without `atlas` field; `FontEntry { familyName, sourceFileName, isDefault }` (no `font` field); `LoomSettings.fonts: List<FontEntry>`; `LoomSettings.pkgOutputDir = "Assets/LoomGUI/Bundles/"`.

- [ ] **Step 1: Update LoomConfigExporterTests to new shape**

In `LoomConfigExporterTests.cs`, any test constructing `AtlasEntry`/`FontEntry` must drop the `atlas`/`font` fields. Add a test asserting LoomSettings has no serialized Object refs is not feasible via reflection cheaply; instead assert the fields don't exist by compiling against the new shape. Add a smoke test:

```csharp
[Test]
public void FontEntry_HasNoFontAssetField()
{
    var fields = typeof(FontEntry).GetFields();
    Assert.IsFalse(System.Array.Exists(fields, f => f.FieldType == typeof(UnityEngine.Font)),
        "FontEntry must NOT hold a Font asset reference (would drag asset into Resources build)");
    Assert.IsTrue(System.Array.Exists(fields, f => f.Name == "sourceFileName"),
        "FontEntry must have sourceFileName for driver .bytes path");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run Unity test runner (or `cargo`-side N/A — this is Unity): from Unity Editor, run EditMode tests for `LoomConfigExporterTests`. Expected: FAIL — `FontEntry` still has `font` field / lacks `sourceFileName`.

- [ ] **Step 3: Strip asset refs + add fonts + change default**

In `LoomSettings.cs`:

Change `pkgOutputDir` default:
```csharp
[Tooltip("pkg.bin 输出目录（Unity 工程相对路径）")]
public string pkgOutputDir = "Assets/LoomGUI/Bundles/";
```

`AtlasEntry` — delete the `atlas` field:
```csharp
[Serializable]
public sealed class AtlasEntry {
    public string atlasName = "";
    [Tooltip("res 根图（path 无子目录）兜底走此图集")]
    public bool isDefault;
    [Tooltip("packables 文件夹（Unity 相对路径）")]
    public List<string> folders = new();
    // DELETED: public SpriteAtlas atlas;  — would drag asset into Resources build
}
```

Add `fonts` list + new `FontEntry`:
```csharp
[Tooltip("字体列表（familyName=CSS font-family；sourceFileName=driver 拼 .bytes 路径）")]
public List<FontEntry> fonts = new();
```
```csharp
[Serializable]
public class FontEntry {
    [Tooltip("CSS font-family 值。拖入时默认=源文件名去扩展，可手改")]
    public string familyName;
    [Tooltip("源文件名（如 NotoSansSC.ttc）。拖 asset 时自动填，driver 拼 .bytes 路径用")]
    public string sourceFileName;
    [Tooltip("默认回退字体")]
    public bool isDefault;
    // DELETED: public Font font;  — would drag asset into Resources build
}
```

- [ ] **Step 4: Fix compilation fallout**

Run: `grep -rn "\.atlas\b\|entry\.font\b\|\.fonts\b" loomgui_unity_package/` — find every reference to the deleted fields. These break compilation:
- `SpriteResolver.Init` (reads `entry.atlas`) → defer to Task B4 (full rewrite), but for now make it compile by commenting the body with a `// TODO B4: name mapping` and returning early. Actually B4 is the rewrite — leave a stub that compiles:
```csharp
public void Init(LoomSettings settings) {
    _folderToAtlas.Clear(); _cache.Clear(); _warned.Clear(); _defaultAtlas = null;
    // Rewritten in Task B4 to folder→atlasName mapping + injected loader.
}
```
- `LoomAtlasSync` writes `entry.atlas =` → Task B3 fixes. For now comment those lines with `// TODO B3`.
- `LoomSettingsWindow` atlas/font UI → Task B6 fixes. For now comment atlas-binding UI lines.

Goal: project compiles after B1, even if SpriteResolver/AtlasSync/Window are temporarily stubbed.

- [ ] **Step 5: Run EditMode tests**

Run Unity EditMode tests. Expected: `FontEntry_HasNoFontAssetField` PASS; `LoomAtlasSyncTests` may break (atlas field gone) — those are fixed in B3. Mark them `[Ignore("until B3")]` temporarily if blocking, or fix inline in B3.

- [ ] **Step 6: Commit**

```bash
git add loomgui_unity_package/Runtime/LoomSettings.cs loomgui_unity_package/Tests/LoomConfigExporterTests.cs loomgui_unity_package/Runtime/SpriteResolver.cs loomgui_unity_package/Editor/LoomAtlasSync.cs loomgui_unity_package/Editor/LoomSettingsWindow.cs
git commit -m "feat(unity): LoomSettings pure config — strip atlas/Font refs, add fonts+sourceFileName"
```
### Task B2: LoomStage → pure C# class (carry all APIs incl. Controller)

**Files:**
- Modify (rewrite): `loomgui_unity_package/Runtime/LoomStage.cs`
- Modify: `loomgui_unity_package/Runtime/TextRasterizer.cs` (drop static version)
- Modify: `loomgui_unity_package/Runtime/MirrorPool.cs` (`Sync` sig — see Step 3)
- Test: `loomgui_unity_package/Tests/TextRasterizerTests.cs`, `MirrorPoolTests.cs` (adapt to instance version)

**Interfaces:**
- Consumes: new FFI `loomgui_stage_new(w,h)` + `loomgui_stage_register_font` (A4).
- Produces: `public sealed unsafe class LoomStage : IDisposable` (NOT MonoBehaviour). Constructor `LoomStage(Vector2 designSize)`. `RegisterFont(family, bytes, unityFont, isDefault)`. `Tick(float dt, LoomStageDriver driver)`. `int FontVersion` (instance). All existing public APIs carried verbatim: `EventHandler`, `IsPointerOnUI`, `FindNodeById`, `SetNodeDisabled`, `SetScrollPos`, `SetContentSize`, `ClearContentSizeOverride`, `GetScrollPos`, `GetNodeLayoutRect`, `SetReuseKey`, `BindNativeHost` (×2), `UnbindNativeHost`, `DumpScene`, `Tween`, `KillTween`, `ClearAnim`, `ClearAnimProp`, `LoadPackage`, `Instantiate`, `CreateRoot`, `CreateNode`, `AppendChild`, `InsertBefore`, `RemoveChild`, `RemoveNode`, `SetText`, `SetSrc`, `SetStyle`, **`GetController`, `SetSelectedIndex`, `GetSelectedIndex`** (v1.5 — preserved). `StageHandle*` exposed internally.

- [ ] **Step 1: Write a failing test for pure-class construction**

In a new test or `MirrorPoolTests.cs`:

```csharp
[Test]
public void LoomStage_ConstructsAsPureClass_WithoutMonoBehaviour()
{
    using var stage = new LoomGUI.LoomStage(new Vector2(1080, 1920));
    Assert.IsFalse(stage is UnityEngine.Component,
        "LoomStage must be a pure class, not a MonoBehaviour/Component");
    // No font registered yet → tick returns empty frame, no panic.
    stage.Tick(0.016f, driver: null);  // driver null OK when no rendering needed
}
```

- [ ] **Step 2: Run test to verify it fails**

Unity EditMode run. Expected: FAIL — `LoomStage` is still a MonoBehaviour; `new LoomStage()` not allowed on MonoBehaviour.

- [ ] **Step 3: Rewrite LoomStage as pure class**

Open `LoomStage.cs`. Replace the class declaration and all Unity-lifecycle methods. The body keeps every public API method (CreateRoot, Tween, Controller, etc. — they already just call `Native.loomgui_stage_*` and are engine-agnostic). Changes:

(a) Class header + fields:
```csharp
public sealed unsafe class LoomStage : IDisposable
{
    StageHandle* _stage;
    readonly Vector2 _designSize;
    MaterialManager _mm;
    MirrorPool _pool;
    NativeHostManager _nhm;
    SpriteResolver _sprites;
    // family → Font asset (Unity raster). Symmetric to Rust FontTable.
    readonly Dictionary<string, Font> _unityFonts = new();
    Font _defaultUnityFont;
    int _fontVersion;   // per-stage textureRebuilt version
    byte[] _frameBuf;
    readonly LoomEventHandler _eventHandler = new();

    public LoomStage(Vector2 designSize = default) {
        _designSize = designSize == default ? new Vector2(1080, 1920) : designSize;
        _stage = Native.loomgui_stage_new(_designSize.x, _designSize.y);
        if (_stage == null) { Debug.LogError("[LoomStage] loomgui_stage_new failed"); return; }
        _eventHandler.SetHandle((System.IntPtr)_stage);
        var shader = Shader.Find("LoomGUI/Unlit");
        if (shader == null) { Debug.LogError("[LoomStage] Shader LoomGUI/Unlit not found"); FreeStage(); return; }
        _mm = new MaterialManager(shader);
        _pool = new MirrorPool();
        _nhm = new NativeHostManager();
        _nhm.Init(default);  // transform injected at Tick via driver
        _sprites = new SpriteResolver();
    }

    public LoomEventHandler EventHandler => _eventHandler;
    public int FontVersion => _fontVersion;
    internal System.IntPtr StagePtr => (System.IntPtr)_stage;
    public Vector2 DesignSize => _designSize;
```

(b) `RegisterFont`:
```csharp
/// bytes 喂 Rust（测量）；unityFont 存 _unityFonts（光栅）。is_default 设默认。
public void RegisterFont(string family, byte[] bytes, Font unityFont, bool isDefault) {
    if (_stage == null) return;
    byte[] fb = Encoding.UTF8.GetBytes(family ?? "");
    fixed (byte* fp = fb, bp = bytes) {
        Native.loomgui_stage_register_font(_stage, fp, (nuint)fb.Length, bp, (nuint)(bytes?.Length ?? 0), isDefault ? (byte)1 : (byte)0);
    }
    _unityFonts[family] = unityFont;
    if (isDefault) _defaultUnityFont = unityFont;
}

/// textureRebuilt 回调（Driver.Awake 绑 Font.textureRebuilt += stage.OnFontRebuilt）。
public void OnFontRebuilt(Font font) { _fontVersion++; }
```

(c) `InitSprites` (called by Driver after construction, before Tick):
```csharp
/// SpriteResolver 建名字映射 + 注入 atlas 加载委托（Driver.LoadSpriteAtlas）。
public void InitSprites(LoomSettings settings, System.Func<string, SpriteAtlas> loadAtlas) {
    _sprites.Init(settings, loadAtlas);
}
```

(d) `Tick` (replaces `LateUpdate` — driver calls it):
```csharp
public void Tick(float dt, LoomStageDriver driver) {
    if (_stage == null) return;
    Native.loomgui_stage_tick(_stage, dt);
    nuint lenRaw = 0;
    byte* ptr = Native.loomgui_stage_borrow_frame(_stage, &lenRaw);
    int len = (int)lenRaw;
    if (ptr != null && len > 0 && driver != null) {
        if (_frameBuf == null || _frameBuf.Length < len) {
            if (_frameBuf != null) ArrayPool<byte>.Shared.Return(_frameBuf);
            _frameBuf = ArrayPool<byte>.Shared.Rent(len);
        }
        Marshal.Copy((IntPtr)ptr, _frameBuf, 0, len);
        var blob = new FrameBlob(_frameBuf);
        // MirrorPool.Sync reads this.FontVersion (instance) for text re-raster.
        _pool.Sync(blob, driver.transform, _mm, _sprites, Texture2D.whiteTexture, _unityFonts, _defaultUnityFont, _fontVersion);
        _nhm.Sync(_stage);
    }
    nuint evLen = 0;
    byte* evPtr = Native.loomgui_stage_borrow_events(_stage, &evLen);
    _eventHandler.DispatchPending((System.IntPtr)evPtr, (int)evLen);
    nuint ccLen = 0;
    byte* ccPtr = Native.loomgui_stage_borrow_controller_changed_events(_stage, &ccLen);
    _eventHandler.DispatchControllerChanged((System.IntPtr)ccPtr, (int)ccLen);
}
```

(e) `Dispose` (replaces `OnDestroy`):
```csharp
public void Dispose() {
    _pool?.Clear();
    _nhm?.Clear();
    _mm?.Clear();
    _sprites?.Clear();
    if (_frameBuf != null) { ArrayPool<byte>.Shared.Return(_frameBuf); _frameBuf = null; }
    FreeStage();
}
void FreeStage() {
    if (_stage != null) { Native.loomgui_stage_free(_stage); _stage = null; }
}
```

(f) Delete: `[ExecuteAlways]`, `MonoBehaviour`, `_designSize`/`_uiCamera`/`_showFps`/`_safeArea`/`_inputCollector`/`_font`/`_fontFile` SerializeFields, `Awake`, `OnValidate`, `EnsureFont`, `OnGUI`, `EnsureCamera`, `ConfigureTransforms`, `ComputeRootTransform`, `LateUpdate`, `OnDestroy`, `ResetStatics` (SubsystemRegistration hook moves to Driver). Keep ALL public API methods (CreateRoot/Tween/Controller/...) unchanged — they already call `Native.loomgui_stage_*` and are engine-agnostic.

(g) `ResetStatics` (SubsystemRegistration) moves to Driver (Task B3) — it called `TextRasterizer.ResetStatic()` which no longer exists; replace with per-stage `_fontVersion` reset is per-instance so no static to reset. The `Native.loomgui_shutdown()` call stays in Driver's ResetStatics.

- [ ] **Step 4: Update TextRasterizer — drop static version**

In `TextRasterizer.cs`: delete `static int s_fontVersion`, `FontVersion`, `OnRebuilt`, `ResetStatic`. Keep `BuildMesh` (it's called per-text-node by MirrorPool; signature unchanged — caller passes the selected `Font`).

- [ ] **Step 5: Update MirrorPool.Sync signature**

In `MirrorPool.cs`, change `Sync` to accept the font map + version instead of a single `_font`:

```csharp
public void Sync(FrameBlob blob, Transform root, MaterialManager mm, SpriteResolver sprites,
    Texture whiteTex, Dictionary<string, Font> unityFonts, Font defaultFont, int fontVersion) {
    // ... wherever it previously read TextRasterizer.FontVersion (static), read fontVersion (param).
    // ... wherever it called TextRasterizer.BuildMesh(_font, ...), select font per node:
    //     Font f = unityFonts.TryGetValue(nodeFamily, out var fa) ? fa : defaultFont;
    //     TextRasterizer.BuildMesh(f, ...);
}
```

Run: `grep -n "TextRasterizer\.\|_font\b" loomgui_unity_package/Runtime/MirrorPool.cs` to find every static-ref call site. Replace with the parameter.

- [ ] **Step 6: Update MirrorPool/TextRasterizer tests**

`MirrorPoolTests.cs` / `TextRasterizerTests.cs` construct `Sync` calls — update to new signature (pass a `Dictionary<string,Font>` + a default Font + version 0).

- [ ] **Step 7: Compile + run EditMode tests**

Unity EditMode: `LoomStage_ConstructsAsPureClass_WithoutMonoBehaviour` PASS; MirrorPool/TextRasterizer tests PASS.

- [ ] **Step 8: Commit**

```bash
git add loomgui_unity_package/Runtime/LoomStage.cs loomgui_unity_package/Runtime/TextRasterizer.cs loomgui_unity_package/Runtime/MirrorPool.cs loomgui_unity_package/Tests/MirrorPoolTests.cs loomgui_unity_package/Tests/TextRasterizerTests.cs
git commit -m "feat(unity): LoomStage pure class — carries all APIs incl. v1.5 Controller, per-stage font version"
```

### Task B3: LoomStageDriver MonoBehaviour + three virtual load hooks

**Files:**
- Create: `loomgui_unity_package/Runtime/LoomStageDriver.cs`
- Modify: `loomgui_unity_package/Runtime/NativeHostManager.cs` (Init signature — `Init(Transform)` instead of `Init(default)`)

**Interfaces:**
- Consumes: `LoomStage` pure class (B2), `LoomSettings.fonts` (B1).
- Produces: `public class LoomStageDriver : MonoBehaviour` with `[ExecuteAlways]`; `[SerializeField]` `_designSize`/`_uiCamera`/`_showFps`/`_safeArea`/`_inputCollector`; `public LoomStage Stage`; `public virtual (byte[], Font) LoadFont(FontEntry)`; `public virtual byte[] LoadPackageBytes(string name)`; `public virtual SpriteAtlas LoadSpriteAtlas(string atlasName)`; `protected virtual void RegisterFontsFromSettings()`. (Load hooks are `public` so cross-assembly consumers like LoomGUI.Demo can call them; `RegisterFontsFromSettings` is `protected` — internal orchestration, overridden only by subclasses.)

- [ ] **Step 1: Write a failing test**

```csharp
[Test]
public void LoomStageDriver_AwakeBuildsStageAndRegistersFonts() {
    // Setup a temp LoomSettings with one font entry pointing at a test ttf in Bundles/fonts.
    // (Editor test — uses AssetDatabase.)
    var go = new GameObject("driver_test");
    var driver = go.AddComponent<LoomStageDriver>();
    Assert.IsNotNull(driver.Stage, "Driver.Awake must construct LoomStage");
    go.DestroyImmediate();
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — `LoomStageDriver` type not found.

- [ ] **Step 3: Implement LoomStageDriver**

Create `LoomStageDriver.cs`:

```csharp
using System.Collections.Generic;
using System.IO;
using LoomGUI.Bindings;
using UnityEngine;

namespace LoomGUI
{
    /// LoomStage 的 Unity 生命周期宿主。建 stage + 注册字体 + 配相机/transform；
    /// LateUpdate 驱动 tick。三个 virtual 加载函数默认直读 Bundles/，项目继承覆写换 AB/Addressables。
    [ExecuteAlways]
    public class LoomStageDriver : MonoBehaviour
    {
        [SerializeField] Vector2 _designSize = new(1080, 1920);
        [SerializeField] Camera _uiCamera;
        [SerializeField] bool _showFps;
        [SerializeField] bool _safeArea = true;
        [SerializeField] LoomInputCollector _inputCollector;

        LoomStage _stage;
        int _lastScreenW = -1, _lastScreenH = -1;
        const int LoomUILayer = 6;

        public LoomStage Stage => _stage;
        internal Vector2 DesignSize => _designSize;
        internal bool UseSafeArea => _safeArea;

        void Awake() {
            // ExecuteAlways: clear orphan loom_node children from prior domain reload.
            for (int c = transform.childCount - 1; c >= 0; c--) {
                var child = transform.GetChild(c);
                if (child.name == "loom_node") DestroyImmediate(child.gameObject);
            }
            _stage = new LoomStage(_designSize);
            var settings = LoomSettings.GetOrCreateDefault();
            _stage.InitSprites(settings, atlasName => LoadSpriteAtlas(atlasName));
            RegisterFontsFromSettings();
            EnsureCamera();
            ConfigureTransforms();
            Font.textureRebuilt += _stage.OnFontRebuilt;
            gameObject.layer = LoomUILayer;
            if (_inputCollector == null) _inputCollector = GetComponent<LoomInputCollector>();
        }

        /// 默认：遍历 LoomSettings.fonts → LoadFont → stage.RegisterFont。项目可覆写加载策略。
        protected virtual void RegisterFontsFromSettings() {
            var settings = LoomSettings.GetOrCreateDefault();
            foreach (var entry in settings.fonts) {
                var (bytes, unityFont) = LoadFont(entry);
                if (bytes != null && unityFont != null)
                    _stage.RegisterFont(entry.familyName, bytes, unityFont, entry.isDefault);
            }
        }

        /// 默认直读 Bundles/fonts/{sourceFileName}.bytes + editor LoadAssetAtPath<Font>。
        /// 项目覆写换 AB/Addressables。public 以便跨程序集（LoomGUI.Demo）调用。
        public virtual (byte[] bytes, Font unityFont) LoadFont(FontEntry entry) {
            string dir = Path.Combine(Application.streamingAssetsPath, "..", "Assets/LoomGUI/Bundles/fonts");
            // editor: Bundles/ is under Assets/ — resolve via Application.dataPath
            string fontsDir = Path.Combine(Application.dataPath, "LoomGUI/Bundles/fonts");
            string bytesPath = Path.Combine(fontsDir, entry.sourceFileName + ".bytes");
            byte[] bytes = File.Exists(bytesPath) ? File.ReadAllBytes(bytesPath) : null;
#if UNITY_EDITOR
            Font unityFont = UnityEditor.AssetDatabase.FindAssets(entry.sourceFileName + " t:Font").Length > 0
                ? UnityEditor.AssetDatabase.LoadAssetAtPath<Font>(
                    UnityEditor.AssetDatabase.GUIDToAssetPath(
                        UnityEditor.AssetDatabase.FindAssets(entry.sourceFileName + " t:Font")[0]))
                : null;
#else
            Font unityFont = null;  // build: project must override LoadFont to load Font asset via AB
#endif
            return (bytes, unityFont);
        }

        /// 默认直读 Bundles/ui/{name}.pkg.bin。项目覆写换 AB/Addressables。
        public virtual byte[] LoadPackageBytes(string name) {
            string path = Path.Combine(Application.dataPath, "LoomGUI/Bundles/ui", name + ".pkg.bin");
            return File.Exists(path) ? File.ReadAllBytes(path) : null;
        }

        /// 默认 editor LoadAssetAtPath；build 后返 null + 报错（项目须覆写）。
        public virtual SpriteAtlas LoadSpriteAtlas(string atlasName) {
#if UNITY_EDITOR
            string path = "Assets/LoomGUI/Bundles/atlas/" + atlasName + ".spriteatlasv2";
            return UnityEditor.AssetDatabase.LoadAssetAtPath<UnityEngine.U2D.SpriteAtlas>(path);
#else
            Debug.LogError("[LoomStageDriver] LoadSpriteAtlas must be overridden for builds (AB/Addressables).");
            return null;
#endif
        }

        void LateUpdate() {
            if (_stage == null) return;
            if (Screen.width != _lastScreenW || Screen.height != _lastScreenH) {
                _lastScreenW = Screen.width; _lastScreenH = Screen.height;
                ConfigureTransforms();
            }
            if (_inputCollector != null) {
                _inputCollector.Collect(_stage.StagePtr, _designSize, _safeArea);
                _inputCollector.CollectKeys(_stage.StagePtr);
                LoomInputCollector.CollectWheel(this);
            }
            _stage.Tick(Time.unscaledDeltaTime, this);
        }

        void OnGUI() {
            if (!_showFps) return;
            float fps = Time.smoothDeltaTime > 0f ? 1f / Time.smoothDeltaTime : 0f;
            GUI.Label(new Rect(8f, 8f, 240f, 24f), $"FPS {fps:F1}");
        }

        void OnDestroy() {
            if (_stage != null) { Font.textureRebuilt -= _stage.OnFontRebuilt; _stage.Dispose(); _stage = null; }
        }

        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.SubsystemRegistration)]
        static void ResetStatics() { Native.loomgui_shutdown(); }

        // EnsureCamera / ConfigureTransforms / ComputeRootTransform copied verbatim
        // from old LoomStage.cs (they read _uiCamera/_designSize/_safeArea — all here).
    }
}
```

Copy `EnsureCamera`, `ConfigureTransforms`, `ComputeRootTransform` bodies verbatim from the pre-B2 `LoomStage.cs` (they're Unity-transform logic, unchanged).

- [ ] **Step 4: Fix NativeHostManager.Init**

`LoomStage` constructor calls `_nhm.Init(default)` (no transform yet). Change `NativeHostManager.Init` to accept the transform at `Tick` time, OR have Driver pass `transform` to `_nhm` after Awake. Simplest: `NativeHostManager.Init(Transform root)` and LoomStage stores it; Driver sets `_stage.SetNativeHostRoot(transform)` in Awake before first Tick. Add that method to LoomStage:

```csharp
public void SetNativeHostRoot(Transform root) { _nhm.Init(root); }
```
Call in Driver.Awake after `_stage = new LoomStage(...)`:
```csharp
_stage.SetNativeHostRoot(transform);
```

- [ ] **Step 5: Run EditMode test**

Expected: `LoomStageDriver_AwakeBuildsStageAndRegistersFonts` PASS (Stage non-null after Awake).

- [ ] **Step 6: Commit**

```bash
git add loomgui_unity_package/Runtime/LoomStageDriver.cs loomgui_unity_package/Runtime/NativeHostManager.cs loomgui_unity_package/Runtime/LoomStage.cs
git commit -m "feat(unity): LoomStageDriver MonoBehaviour + 3 virtual load hooks (LoadFont/LoadPackageBytes/LoadSpriteAtlas)"
```
### Task B4: SpriteResolver — name mapping + injected loader

**Files:**
- Modify (rewrite): `loomgui_unity_package/Runtime/SpriteResolver.cs`
- Test: `loomgui_unity_package/Tests/SpriteResolverTests.cs`

**Interfaces:**
- Consumes: `LoomSettings.atlasEntries` (folders + atlasName, no atlas ref) (B1), `Func<string, SpriteAtlas>` loader (B3).
- Produces: `SpriteResolver.Init(LoomSettings, Func<string,SpriteAtlas>)`; `GetSprite(path)` resolves folder→atlasName→loader→SpriteAtlas (cached).

- [ ] **Step 1: Update SpriteResolverTests**

```csharp
[Test]
public void GetSprite_ResolvesViaInjectedLoaderAndCaches() {
    var resolver = new SpriteResolver();
    var settings = ScriptableObject.CreateInstance<LoomSettings>();
    settings.atlasEntries.Add(new AtlasEntry { atlasName = "icons", folders = new List<string>{ "Assets/LoomUI/res/icons" } });
    int loadCount = 0;
    SpriteAtlas atlas = MakeFakeAtlasWithSprite("home");  // helper: SpriteAtlas can't be trivially faked; see note
    resolver.Init(settings, name => { loadCount++; return atlas; });
    // ... if SpriteAtlas faking is hard in EditMode, test InitWithMap path instead:
    resolver.InitWithMap(new Dictionary<string,string>{{"icons","icons"}}, atlasName => atlas, "icons");
    // GetSprite("icons/home.png") → atlas.GetSprite("home")
    // (SpriteAtlas.GetSprite needs a real packed atlas — for pure-logic test, mock via InitWithMap delegate)
}
```

**Note:** `SpriteAtlas.GetSprite` requires a real imported atlas. For pure-logic EditMode tests, test the **resolution path** (folder→atlasName→delegate call→cache) with a delegate that returns a sentinel, and keep the actual `atlas.GetSprite` call unmocked (covered by PlayMode acceptance). Adjust `InitWithMap` to take a name-based delegate.

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — `Init` signature changed (B1 stub); `InitWithMap` doesn't take a delegate.

- [ ] **Step 3: Rewrite SpriteResolver**

```csharp
public sealed class SpriteResolver
{
    readonly Dictionary<string, string> _folderToAtlasName = new();  // folder key → atlas name
    readonly Dictionary<string, Sprite> _cache = new();
    readonly HashSet<string> _warned = new();
    readonly Dictionary<string, SpriteAtlas> _atlasCache = new();    // atlasName → atlas (loaded once)
    System.Func<string, SpriteAtlas> _loadAtlas;
    string _defaultAtlasName;

    public void Init(LoomSettings settings, System.Func<string, SpriteAtlas> loadAtlas) {
        _folderToAtlasName.Clear(); _cache.Clear(); _warned.Clear(); _atlasCache.Clear();
        _loadAtlas = loadAtlas; _defaultAtlasName = null;
        if (settings == null) return;
        foreach (var entry in settings.atlasEntries) {
            if (entry == null || string.IsNullOrEmpty(entry.atlasName)) continue;
            if (entry.isDefault) _defaultAtlasName = entry.atlasName;
            foreach (var folder in entry.folders) {
                if (string.IsNullOrEmpty(folder)) continue;
                string key = LastSegment(folder);
                if (!string.IsNullOrEmpty(key)) _folderToAtlasName[key] = entry.atlasName;
            }
        }
    }

    /// Test injection: name mapping + loader + default.
    public void InitWithMap(Dictionary<string, string> folderToAtlasName,
        System.Func<string, SpriteAtlas> loadAtlas, string defaultAtlasName) {
        _folderToAtlasName.Clear(); _cache.Clear(); _warned.Clear(); _atlasCache.Clear();
        foreach (var kv in folderToAtlasName) _folderToAtlasName[kv.Key] = kv.Value;
        _loadAtlas = loadAtlas; _defaultAtlasName = defaultAtlasName;
    }

    public Sprite GetSprite(string path) {
        if (string.IsNullOrEmpty(path)) return null;
        if (_cache.TryGetValue(path, out var cached)) return cached;
        string spriteName = System.IO.Path.GetFileNameWithoutExtension(path);
        SpriteAtlas atlas = ResolveAtlas(path);
        Sprite found = atlas != null ? atlas.GetSprite(spriteName) : null;
        if (found != null) { _cache[path] = found; _warned.Remove(path); return found; }
        if (_warned.Add(path))
            Debug.LogWarning($"[SpriteResolver] 图不存在：path={path}");
        return null;
    }

    SpriteAtlas ResolveAtlas(string path) {
        string atlasName = ResolveAtlasName(path);
        if (atlasName == null) return null;
        if (_atlasCache.TryGetValue(atlasName, out var cached)) return cached;
        var atlas = _loadAtlas?.Invoke(atlasName);
        if (atlas != null) _atlasCache[atlasName] = atlas;
        return atlas;
    }

    string ResolveAtlasName(string path) {
        string topDir = TopDir(path);
        if (topDir == null) return _defaultAtlasName;          // res root → default
        if (_folderToAtlasName.TryGetValue(topDir, out var name)) return name;
        return _defaultAtlasName;
    }

    static string LastSegment(string folder) {
        string key = folder.TrimEnd('/', '\\');
        int sep = key.LastIndexOfAny(new[] { '/', '\\' });
        return sep >= 0 ? key.Substring(sep + 1) : key;
    }
    static string TopDir(string path) {
        string p = path.Replace('\\', '/');
        int slash = p.IndexOf('/');
        return slash <= 0 ? null : p.Substring(0, slash);
    }

    public void Clear() {
        _folderToAtlasName.Clear(); _cache.Clear(); _warned.Clear(); _atlasCache.Clear();
    }
}
```

- [ ] **Step 4: Run tests + commit**

Expected: `SpriteResolverTests` PASS.

```bash
git add loomgui_unity_package/Runtime/SpriteResolver.cs loomgui_unity_package/Tests/SpriteResolverTests.cs
git commit -m "feat(unity): SpriteResolver name→atlasName mapping + injected LoadSpriteAtlas loader"
```

### Task B5: LoomAtlasSync → Bundles/atlas/ + drop atlas ref writes

**Files:**
- Modify: `loomgui_unity_package/Editor/LoomAtlasSync.cs` (`EnsureAtlasAsset`, `DeleteAutoAtlas`, `ResolveAtlasPath`, `SyncEntry`)
- Test: `loomgui_unity_package/Tests/LoomAtlasSyncTests.cs`

**Interfaces:**
- Consumes: `LoomSettings.pkgOutputDir` (= `Bundles/`) (B1), `AtlasEntry` without `atlas` (B1).
- Produces: `.spriteatlasv2` written to `{pkgOutputDir}/atlas/{atlasName}.spriteatlasv2`.

- [ ] **Step 1: Update LoomAtlasSyncTests to expect Bundles/atlas/ path**

```csharp
[Test]
public void EnsureAtlasAsset_WritesToBundlesAtlas() {
    var settings = ScriptableObject.CreateInstance<LoomSettings>();
    settings.workspaceDir = "Assets/LoomUI/";
    settings.pkgOutputDir = "Assets/LoomGUI/Bundles/";   // NEW default
    var entry = new AtlasEntry { atlasName = "testatlas", folders = new List<string>() };
    string rel = LoomAtlasSync.EnsureAtlasAsset(entry, settings.pkgOutputDir, settings.workspaceDir);
    Assert.IsTrue(rel.StartsWith("Assets/LoomGUI/Bundles/atlas/"), "atlas must land in Bundles/atlas/");
    Assert.IsNull(typeof(AtlasEntry).GetField("atlas"), "AtlasEntry must have no atlas field");
}
```

(Adjust `EnsureAtlasAsset` signature — it currently takes `workspaceDir` for the output dir; change to take `pkgOutputDir` for output, keep `workspaceDir` only if needed for PNG source resolution. See Step 3.)

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — atlas still written to `{workspaceDir}/atlas/`.

- [ ] **Step 3: Change atlas output dir to Bundles/atlas/**

In `LoomAtlasSync.cs`:

`EnsureAtlasAsset(entry, pkgOutputDir)` — change signature: second param is now `pkgOutputDir` (output root), not `workspaceDir`. Replace the `string dir = (Path.Combine(workspaceDir, "atlas"))` line (`:67`):

```csharp
public static string EnsureAtlasAsset(AtlasEntry entry, string pkgOutputDir) {
    if (entry == null || string.IsNullOrEmpty(entry.atlasName)) return null;
    string rel = ResolveAtlasPath(entry);
    if (rel != null && File.Exists(ToAbs(rel))) {
        EnsureAxisAlignedPacking(rel);
        return rel;
    }
    if (string.IsNullOrEmpty(pkgOutputDir)) return null;
    string dir = (Path.Combine(pkgOutputDir, "atlas")).Replace('\\', '/');
    Directory.CreateDirectory(ToAbs(dir));
    rel = dir + "/" + entry.atlasName + ".spriteatlasv2";
    var saa = new SpriteAtlasAsset();
    SpriteAtlasAsset.Save(saa, rel);
    AssetDatabase.Refresh();
    EnsureAxisAlignedPacking(rel);
    Debug.Log($"[LoomAtlasSync] 自动创建 V2 图集：{rel}");
    return rel;
}
```

**Delete all `entry.atlas = ...` lines** (the field is gone). `ResolveAtlasPath` previously read `entry.atlas` (`:173-176`) — change to resolve by name under `Bundles/atlas/`:

```csharp
static string ResolveAtlasPath(AtlasEntry entry) {
    // By name under {pkgOutputDir}/atlas/. Caller passes pkgOutputDir via settings.
    // (Kept simple: resolved in EnsureAtlasAsset/SyncEntry with settings.pkgOutputDir in scope.)
    ...
}
```

If `ResolveAtlasPath` needs `pkgOutputDir`, thread it through (pass `settings` or `pkgOutputDir` to `SyncEntry`/`SyncAll`). `SyncAll` already has `settings`:

```csharp
public static void SyncAll(LoomSettings settings) {
    foreach (var entry in settings.atlasEntries) {
        string atlasRel = EnsureAtlasAsset(entry, settings.pkgOutputDir);
        if (atlasRel == null) continue;
        SyncEntry(entry, settings);   // SyncEntry takes settings for pkgOutputDir + res-root PNG scan
    }
}
```

`SyncEntry` scans PNG sources from `{workspaceDir}/{resDirName}/` (unchanged) — only the **output** atlas path moves to Bundles/. `DeleteAutoAtlas` similarly changes its delete path to `{pkgOutputDir}/atlas/`.

- [ ] **Step 4: Update LoomSettingsWindow atlas-binding calls**

In `LoomSettingsWindow.DrawAtlasEntry` (B1 stubbed these), remove the `entry.atlas =` assignment (`:209-210` "已同步/未同步" status now checks file existence under Bundles/atlas/, not a ref):

```csharp
string atlasPath = "Assets/LoomGUI/Bundles/atlas/" + e.atlasName + ".spriteatlasv2";
EditorGUILayout.LabelField("状态", File.Exists(ToAbs(atlasPath)) ? "已同步" : "未同步");
```

(Full Window rewrite in B6.)

- [ ] **Step 5: Run tests + commit**

```bash
git add loomgui_unity_package/Editor/LoomAtlasSync.cs loomgui_unity_package/Tests/LoomAtlasSyncTests.cs loomgui_unity_package/Editor/LoomSettingsWindow.cs
git commit -m "feat(unity): LoomAtlasSync outputs to Bundles/atlas/, drop atlas ref writes"
```

### Task B6: LoomSettingsWindow — Fonts tab + Publish button

**Files:**
- Modify: `loomgui_unity_package/Editor/LoomSettingsWindow.cs` (add Fonts tab, Publish button, PublishFonts, font-asset transient load)
- Modify: `loomgui_unity_package/Editor/LoomConfigExporter.cs` (`output_dir` → `{pkgOutputDir}/ui/`)
- Test: `loomgui_unity_package/Tests/LoomConfigExporterTests.cs`

**Interfaces:**
- Consumes: `LoomSettings.fonts` + `sourceFileName` (B1), `LoomAtlasSync.SyncAll` (B5), `PackPackage` output to `ui/`.
- Produces: Fonts tab (drag Font asset → fill familyName+sourceFileName, drop ref); Publish button (sync atlas + pack pkg + publish fonts + export config).

- [ ] **Step 1: Update config exporter test for ui/ subdir**

```csharp
[Test]
public void BuildJson_OutputDirIsUiSubdir() {
    var s = ScriptableObject.CreateInstance<LoomSettings>();
    s.workspaceDir = "Assets/LoomUI/";
    s.pkgOutputDir = "Assets/LoomGUI/Bundles/";
    string json = LoomConfigExporter.BuildJson(s);
    // output_dir relative to workspace → ../../LoomGUI/Bundles/ui/
    Assert.That(json, Does.Contain("Bundles/ui"), "output_dir must point to ui/ subdir");
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — `output_dir` still points at `pkgOutputDir` root.

- [ ] **Step 3: Change LoomConfigExporter output_dir**

In `LoomConfigExporter.cs:19`:
```csharp
string outRel = RelativeFromWorkspace(s.workspaceDir, s.pkgOutputDir + "ui/");
```
(Trailing slash so `MakeRelativeUri` treats it as a directory.)

- [ ] **Step 4: Add Fonts tab + Publish button to LoomSettingsWindow**

In `LoomSettingsWindow.cs`:

(a) Enum: `enum Tab { Workspace, Packages, Atlas, Fonts }`. SelectionGrid count 4, labels `{ "工作区", "包管理", "图集", "字体" }`.

(b) `DrawFonts()`:
```csharp
void DrawFonts() {
    EditorGUILayout.LabelField("字体列表（" + _settings.fonts.Count + "）——拖 Font asset 自动填", EditorStyles.boldLabel);
    DrawFontDropZone();
    for (int i = 0; i < _settings.fonts.Count; i++) DrawFontEntry(i);
    if (GUILayout.Button("+ 手动添加", GUILayout.Width(120)))
        _settings.fonts.Add(new FontEntry());
    EditorGUILayout.Space(8);
    // first entry auto isDefault if none set
    if (_settings.fonts.Count > 0 && !_settings.fonts.Exists(f => f.isDefault))
        _settings.fonts[0].isDefault = true;
}

void DrawFontDropZone() {
    Rect drop = GUILayoutUtility.GetRect(0, 48, GUILayout.ExpandWidth(true));
    GUI.Box(drop, "拖 Font asset 到此\n自动填 sourceFileName + familyName（不持引用）", EditorStyles.helpBox);
    if (!drop.Contains(Event.current.mousePosition)) return;
    if (Event.current.type == UnityEngine.EventType.DragUpdated) {
        bool hasFont = false;
        foreach (var o in DragAndDrop.objectReferences) if (o is Font) { hasFont = true; break; }
        DragAndDrop.visualMode = hasFont ? DragAndDropVisualMode.Copy : DragAndDropVisualMode.Rejected;
    }
    if (Event.current.type == UnityEngine.EventType.DragPerform) {
        DragAndDrop.AcceptDrag();
        foreach (var o in DragAndDrop.objectReferences) {
            if (o is Font f) {
                string assetPath = AssetDatabase.GetAssetPath(f);
                string fileName = Path.GetFileName(assetPath);           // NotoSansSC.ttc
                string family = Path.GetFileNameWithoutExtension(assetPath); // NotoSansSC
                _settings.fonts.Add(new FontEntry {
                    familyName = family, sourceFileName = fileName,
                    isDefault = _settings.fonts.Count == 0
                });
            }
        }
        Event.current.Use();
        SaveSettings();
    }
}

void DrawFontEntry(int idx) {
    var e = _settings.fonts[idx];
    EditorGUILayout.BeginVertical(EditorStyles.helpBox);
    e.familyName = EditorGUILayout.TextField("familyName (CSS)", e.familyName);
    EditorGUILayout.LabelField("sourceFileName", e.sourceFileName);
    e.isDefault = EditorGUILayout.Toggle("isDefault", e.isDefault);
    if (GUILayout.Button("删除", GUILayout.Width(60))) { _settings.fonts.RemoveAt(idx); SaveSettings(); GUIUtility.ExitGUI(); return; }
    EditorGUILayout.EndVertical();
}
```

(c) Publish button (drawn for all tabs, after the scroll):
```csharp
void DrawPublishButton() {
    EditorGUILayout.Space(12);
    if (GUILayout.Button("发布", GUILayout.Height(36))) Publish();
}

void Publish() {
    AppendLog("[发布] 开始...");
    try {
        LoomAtlasSync.SyncAll(_settings);  AppendLog("[发布] Atlas: OK");
        for (int i = 0; i < _settings.packages.Count; i++) PackPackage(i);
        AppendLog("[发布] Pkg: OK");
        PublishFonts();
        LoomConfigExporter.Export(_settings);  AppendLog("[发布] Config: OK");
    } catch (System.Exception ex) { AppendLog($"[发布] FAIL: {ex.Message}"); }
    AssetDatabase.Refresh();
}

void PublishFonts() {
    string fontsDir = ToAbs(Path.Combine(_settings.pkgOutputDir, "fonts"));
    Directory.CreateDirectory(fontsDir);
    int count = 0;
    foreach (var entry in _settings.fonts) {
        if (string.IsNullOrEmpty(entry.sourceFileName)) continue;
        string assetPath = FindFontAssetPath(entry.sourceFileName);
        if (string.IsNullOrEmpty(assetPath)) { AppendLog($"[发布] 字体 {entry.sourceFileName} 找不到源 asset，跳过"); continue; }
        string absSrc = Path.GetFullPath(assetPath);
        File.Copy(absSrc, Path.Combine(fontsDir, entry.sourceFileName), overwrite: true);
        File.Copy(absSrc, Path.Combine(fontsDir, entry.sourceFileName + ".bytes"), overwrite: true);
        count++;
    }
    AppendLog($"[发布] Fonts: {count} → {fontsDir}");
}

string FindFontAssetPath(string sourceFileName) {
    foreach (var g in AssetDatabase.FindAssets(sourceFileName + " t:Font")) {
        var p = AssetDatabase.GUIDToAssetPath(g);
        if (Path.GetFileName(p) == sourceFileName) return p;
    }
    return null;
}
```

(d) `PackPackage` output path (`:279`): `Path.Combine(_settings.pkgOutputDir, "ui", pkg.pkgName + ".pkg.bin")`.

(e) Call `DrawPublishButton()` in `OnGUI` after `DrawLog()`.

- [ ] **Step 5: Run tests + commit**

```bash
git add loomgui_unity_package/Editor/LoomSettingsWindow.cs loomgui_unity_package/Editor/LoomConfigExporter.cs loomgui_unity_package/Tests/LoomConfigExporterTests.cs
git commit -m "feat(unity): Fonts tab + Publish button (atlas+pkg+fonts+config to Bundles/)"
```
### Task B7: Relocate ShowcaseDriver to Demo/ + read path via LoadPackageBytes

**Files:**
- Create: `loomgui_unity/Assets/LoomUI/Demo/LoomShowcaseDriver.cs`
- Create: `loomgui_unity/Assets/LoomUI/Demo/VirtualListDriver.cs`
- Create: `loomgui_unity/Assets/LoomUI/Demo/LoomGUI.Demo.asmdef`
- Delete: `loomgui_unity_package/Runtime/LoomShowcaseDriver.cs` (contains both classes today)
- Modify: `loomgui_unity/Assets/LoomUI/Demo/LoomShowcaseDriver.cs` (read path via Driver)

**Interfaces:**
- Consumes: `LoomStageDriver.Stage` + `LoadPackageBytes` (B3).
- Produces: `LoomGUI.Demo` assembly referencing `LoomGUI.Runtime`.

- [ ] **Step 1: Create asmdef**

`loomgui_unity/Assets/LoomUI/Demo/LoomGUI.Demo.asmdef`:
```json
{
    "name": "LoomGUI.Demo",
    "references": [ "LoomGUI.Runtime" ],
    "includePlatforms": [],
    "excludePlatforms": []
}
```

(Verify the Runtime assembly name — check `loomgui_unity_package/Runtime/*.asmdef` for the exact `name` field. If the package has no asmdef (namespace-based), the Demo asmdef references the package's runtime assembly name; grep for `*.asmdef` under `loomgui_unity_package/` first.)

- [ ] **Step 2: Move LoomShowcaseDriver + VirtualListDriver**

Copy `loomgui_unity_package/Runtime/LoomShowcaseDriver.cs` → `loomgui_unity/Assets/LoomUI/Demo/LoomShowcaseDriver.cs` (it contains both `LoomShowcaseDriver` and `VirtualListDriver` classes). Delete the original.

Update the driver's `_stage` field type and read path:

```csharp
// Before: [SerializeField] LoomStage _stage;
// After:
[SerializeField] LoomStageDriver _driver;
LoomStage _stage;  // cached from _driver.Stage

void Awake() {
    if (_driver == null) _driver = GetComponent<LoomStageDriver>();
    if (_driver == null) { Debug.LogError("[Showcase] 无 LoomStageDriver"); return; }
    _stage = _driver.Stage;
    if (_stage == null) { Debug.LogError("[Showcase] Driver.Stage 未建"); return; }
}
```

Replace `LoadPkgBytes` (currently reads `StreamingAssets/showcase.pkg.bin`). `LoadPackageBytes` is `public virtual` on Driver (declared in B3) — ShowcaseDriver calls it directly:
```csharp
byte[] LoadPkgBytes(string pkgName) => _driver.LoadPackageBytes(pkgName);
```
(No passthrough needed — B3 already declares LoadFont/LoadPackageBytes/LoadSpriteAtlas as `public virtual` so cross-assembly LoomGUI.Demo can call them.)

- [ ] **Step 3: Verify Demo assembly compiles + ShowcaseDriver references resolve**

Unity Editor reimport. `LoomShowcaseDriver` + `VirtualListDriver` now in Demo asmdef; `LoomStage`/`LoomStageDriver`/`LoomInputCollector`/`EventType`/`EventCallback` resolve from `LoomGUI.Runtime`.

- [ ] **Step 4: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/Demo/ loomgui_unity_package/Runtime/LoomShowcaseDriver.cs loomgui_unity_package/Runtime/LoomStageDriver.cs
git rm loomgui_unity_package/Runtime/LoomShowcaseDriver.cs
git commit -m "refactor(unity): move ShowcaseDriver+VirtualListDriver to Demo/ + LoomGUI.Demo.asmdef"
```

### Task B8: SampleScene migration + PlayMode acceptance

**Files:**
- Modify: `loomgui_unity/Assets/Scenes/SampleScene.unity` (manual editor step)

**Interfaces:** none (runtime acceptance).

- [ ] **Step 1: Open SampleScene, rewire components**

In Unity Editor (home machine or local with Unity open — NOTE: close Unity before any dll copy, but this task is scene editing, Unity must be open):

1. The GO that had `LoomStage` (old MonoBehaviour) now has a broken script ref (GUID `a79441f3...`). Remove the broken component.
2. Add `LoomStageDriver` component to that GO. Configure `_designSize`=(1080,1920), `_safeArea`=true.
3. The GO with `LoomShowcaseDriver` (GUID `99fd3b39...`) — its `_stage` field (old `LoomStage` type) is now broken. Re-add `LoomShowcaseDriver` (now from Demo asmdef), set `_driver` = the LoomStageDriver GO.
4. Configure `LoomSettings.asset` (Resources/LoomGUI/): add fonts entries (drag the project's ttf into Fonts tab → fills familyName+sourceFileName; mark one isDefault). Run **发布** button to populate `Bundles/{atlas,ui,fonts}/`.

- [ ] **Step 2: Run 发布 button**

In `LoomGUI > Settings` → click 发布. Verify Console log: Atlas OK / Pkg OK / Fonts N → Bundles/fonts / Config OK. Verify `Assets/LoomGUI/Bundles/` now has `atlas/*.spriteatlasv2`, `ui/*.pkg.bin`, `fonts/*.{ttf,ttf.bytes}`.

- [ ] **Step 3: PlayMode acceptance (spec §10 + v1.5 §10 regression)**

Enter PlayMode. Verify against the spec's acceptance (the showcase exercises all paths):

1. Showcase home renders (text + images + layout). **No text panic** (default font registered).
2. Multi-font: if a page uses a non-default `font-family`, that font is selected (verify visually — glyph metrics match).
3. Navigation (OpenPage) works — pkg loaded via `Bundles/ui/showcase.pkg.bin`.
4. v1.5 Controller regression: tab/dialog/nested/src-text switch pages work (`GetController`/`SetSelectedIndex` carried into pure class).
5. v1.5 transition regression: panel opacity/display transitions animate on page switch.
6. Virtual list page works (reuse_key, scroll).
7. NativeHost page works (3D character + effect sync).
8. Atlas sprites render (loaded via Driver.LoadSpriteAtlas editor default).

If text panics on "no default font": a font entry's `.bytes` missing or `RegisterFontsFromSettings` not called — check `Bundles/fonts/` populated + Driver.Awake ran.

If sprites missing: atlas not in `Bundles/atlas/` or atlasName mismatch — check 发布 step.

- [ ] **Step 4: Commit scene**

```bash
git add loomgui_unity/Assets/Scenes/SampleScene.unity loomgui_unity/Assets/Resources/LoomGUI/LoomSettings.asset
git commit -m "chore(unity): SampleScene rewired to LoomStageDriver + LoomSettings fonts + Bundles/ publish"
```

### Task B9: CI — add loomgui_pkg.exe artifact

**Files:**
- Modify: `.github/workflows/rust-ci.yml`

**Interfaces:** none.

- [ ] **Step 1: Add exe to Windows release artifact upload**

In `.github/workflows/rust-ci.yml`, find the existing Windows `.dll` artifact upload block. Add (next to it):

```yaml
      - name: Upload loomgui_pkg.exe artifact
        uses: actions/upload-artifact@v4
        with:
          name: loomgui_pkg-exe-${{ github.sha }}
          path: target/release/loomgui_pkg.exe
          retention-days: 7
```

Ensure the release build step builds `loomgui_pkg` (it likely already does via `cargo build --release` workspace-wide; if not, add `cargo build -p loomgui_pkg --release` before the upload).

- [ ] **Step 2: Verify CI yaml syntax**

Run: `python -c "import yaml; yaml.safe_load(open('.github/workflows/rust-ci.yml'))" 2>&1 || echo "yaml parse failed"`
(If python/yaml unavailable, visually verify indentation against the existing dll artifact block.)

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/rust-ci.yml
git commit -m "ci: upload loomgui_pkg.exe as Windows artifact (7-day retention)"
```

---

## Self-Review (run after writing all tasks)

**1. Spec coverage:**
- §1.2 (Bundles/ + zero asset refs + editor transient load) → B1 (strip refs) + B6 (transient drag load) ✓
- §1.3 (Bundles/{atlas,ui,fonts}) → B5 (atlas) + B6 (ui+fonts publish) ✓
- §1.4 (two font copies) → B6 PublishFonts (copies both .ttf + .ttf.bytes) ✓
- §1.5 (atlas in Bundles, PNG src in res/) → B5 ✓
- §2 (LoomSettings pure config) → B1 ✓
- §3 (LoomStage/Driver split, _sceneBuilt removed, SampleScene migrate) → B2 + B3 + B8 ✓
- §4 (Driver 3 virtual load hooks + atlas runtime load) → B3 ✓
- §5.2/§5.3 (FontTable + measure切口) → A1 + A2 ✓
- §5.4 (_unityFonts + SpriteResolver name mapping) → B2 (RegisterFont/_unityFonts) + B4 ✓
- §5.5 (per-stage textureRebuilt) → B2 (instance FontVersion + OnFontRebuilt) + B3 (Driver binds) ✓
- §6 (Publish button, PublishFonts, PackPackage ui/, LoomAtlasSync Bundles) → B5 + B6 ✓
- §7 (Fonts tab) → B6 ✓
- §8 #1 (CI exe) → B9 ✓
- §8 #5 (ShowcaseDriver Demo/) → B7 ✓
- §8 #2/#3 (fence.md/skill docs) → **out of scope** (spec marks as roadmap follow-up, not this plan). Documented in File Structure.
- v1.5 Controller preserved → B2 Step 3 explicitly carries GetController/SetSelectedIndex/GetSelectedIndex; B8 Step 3 #4/#5 regression ✓

**2. Placeholder scan:** No "TBD/TODO" except intentional `// TODO B3`/`// TODO B4` cross-task markers in B1 stubs (resolved by the referenced task) — acceptable, they point at a concrete later task. Verified.

**3. Type consistency:**
- `FontTable::select(&self, family: Option<&str>) -> &Font` — used consistently in A2/A3. ✓
- `Stage::new(root_size)` + `register_font(family, bytes, is_default)` — A3 + A4 consistent. ✓
- `LoomStage.RegisterFont(family, bytes, unityFont, isDefault)` — B2 + B3 consistent. ✓
- `MirrorPool.Sync(..., unityFonts, defaultFont, fontVersion)` — B2 Step 3 + Step 5 consistent. ✓
- `LoadFont/LoadPackageBytes/LoadSpriteAtlas` — B3 `public virtual`, consumed by ShowcaseDriver B7 directly. ✓
- `SpriteResolver.Init(settings, Func<string,SpriteAtlas>)` — B2 (InitSprites) + B4 consistent. ✓

**Gaps found & fixed inline:**
- B3 load hooks declared `public virtual` directly (cross-assembly LoomGUI.Demo needs public; `RegisterFontsFromSettings` stays `protected` — internal orchestration only). ✓
- B2 `NativeHostManager.Init(default)` placeholder — fixed by adding `SetNativeHostRoot(transform)` in B3 Step 4. ✓

No remaining gaps.

