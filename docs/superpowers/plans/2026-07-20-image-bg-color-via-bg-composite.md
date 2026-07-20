# Image bg-color via BG_COMPOSITE + back-layer rename Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `<img>` background-color not showing (purple bg invisible) by routing Image kind through the existing `BG_COMPOSITE` shader path when bg-color is present; rename the box-shadow synthetic-id flag/pairs/propagate to describe their back-layer semantics so the concept lives in code.

**Architecture:** The shader (`LoomGUI-Unlit.shader` `BG_COMPOSITE`, program 2/4) already does source-over compositing (`tex over vcol`) — Container uses it for bg-image+bg-color. Image kind (`<img>`) is the only texture-bearing element NOT using it (uses program 0 with a white vertex, dropping bg-color). Fix = make Image use the same path Container uses when bg-color is present. Separately, rename `BOX_SHADOW_FLAG` → `BACK_LAYER_FLAG` (and `shadow_pairs`, `propagate_box_shadow_sort_keys`) to name the bit by its semantics (a back-layer marker), not by its single current user. No synthetic node_id, no sort_key changes, no Unity change, no pkg.bin format change.

**Tech Stack:** Rust (core crate, edition 2021), csbindgen FFI → Unity .dll. Shader: `BG_COMPOSITE` keyword in `unity/package/Shaders/LoomGUI-Unlit.shader`.

## Global Constraints

- **Rust edition 2021**, no new dependencies (this change adds none).
- **Code comments**: production quality, say WHY not WHAT, Chinese (matches surrounding `render/mod.rs`), no internal pitfall/编号.
- **Fix root cause, not compensation**: Image bug is fixed at its source (program/vertex selection), no downstream tweak.
- **Pre-commit gates** (CI is strict, `push` 前必跑否则 CI 红):
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets --workspace --exclude loomgui_gui -- -D warnings`
  - `cargo test -p loomgui_core`
- **Two-machine workflow**: 本机是唯一编码机。core 改了 → 必须 `cargo build -p loomgui_ffi_c --release` 重编 + commit `.dll`，家里机才能测。**拷 `.dll` 时 Unity 必须关着**（它锁 `.dll`）。本改动不动 FFI 签名 → **不需要** `xtask sync-bindings`。
- **User reads Chinese**; code/commit messages in English.
- This plan **supersedes** `docs/superpowers/specs/2026-07-20-image-bg-compositional-layer-design.md` (the back-layer mechanism for image-bg is abandoned as YAGNI; only the rename portion survives, reframed).

---

## File Structure

- **Modify** `crates/core/src/render/mod.rs` — rename `BOX_SHADOW_FLAG`→`BACK_LAYER_FLAG` + doc (const at ~line 35), `shadow_pairs`→`back_layer_pairs` (decl ~276, push ~641, propagate call ~682), `propagate_box_shadow_sort_keys`→`propagate_back_layer_sort_keys` (def ~760, doc ~755); Image arm bg-color fix (~481-507).
- **Modify** `crates/core/src/render/batch.rs:38` — `BOX_SHADOW_FLAG`→`BACK_LAYER_FLAG`.
- **Modify** `crates/core/src/render/merge.rs:26` — `BOX_SHADOW_FLAG`→`BACK_LAYER_FLAG`.
- **Modify** `crates/core/src/render/tests.rs` — rename refs in box-shadow test (~2756,2761,2767) + bit-boundary test (~2669-2682); add 2 new Image+bg-color tests.
- **Modify** `docs/superpowers/specs/2026-07-20-image-bg-compositional-layer-design.md` — prepend SUPERSEDED note.
- **Rebuild + commit** `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`.

---

### Task 1: Rename box-shadow synthetic-id to back-layer semantics (pure refactor)

Rename the const / pair vec / propagate fn so the name describes the bit's **role** (a back-layer marker) rather than its single current user (box-shadow). **Zero behavior change** — the bit value (`0x1000_0000`), the emission, the sort_key propagation all stay identical. Only identifiers + docs change.

**Files:**
- Modify: `crates/core/src/render/mod.rs` (const ~35, decl ~276, push ~641, call ~682, fn def + doc ~755-808, comments ~734-736, ~1356-1357)
- Modify: `crates/core/src/render/batch.rs:38`
- Modify: `crates/core/src/render/merge.rs:26`
- Modify: `crates/core/src/render/tests.rs` (box-shadow test + bit-boundary test)

**Interfaces:**
- Produces: `pub(crate) const BACK_LAYER_FLAG: u32 = 0x1000_0000;`, `back_layer_pairs: Vec<(u32, u32)>`, `fn propagate_back_layer_sort_keys(nodes: &mut [RenderNode], back_layers: &[(u32, u32)])`. Signatures unchanged from the old names.

- [ ] **Step 1: Find every reference (verify scope before editing)**

Run:
```bash
grep -rn "BOX_SHADOW_FLAG\|shadow_pairs\|propagate_box_shadow_sort_keys" crates/core/src
```
Expected: hits in `render/mod.rs`, `render/batch.rs`, `render/merge.rs`, `render/tests.rs` only (the const is `pub(crate)`, core-internal). No hits outside `crates/core`.

- [ ] **Step 2: Rename the const + rewrite its doc (`render/mod.rs` ~line 29-35)**

Replace the existing const block (comment + `pub(crate) const BOX_SHADOW_FLAG`):

```rust
/// 合成 node_id 标志位：主节点的"下层"合成 RenderNode（独立 draw call 画在主节点之下，
/// sort_key < primary，经 propagate_back_layer_sort_keys 调整）。按语义命名（back-layer
/// marker），不按当前唯一使用者 box-shadow 命名——未来新增"独立 quad 下层"需求复用此位。
///
/// 0x1000_0000（bit 28），不与 V_THUMB_FLAG（0x4000_0000）、H_THUMB_FLAG（0x2000_0000）、
/// 跨页 text 子页（bits [31:24] 值 1..15）、富文本行内图（high byte 232..=255）冲突。
///
/// 边界：bg-color-under-texture（Image/Container 底色透图）**不走**此机制——由 shader
/// BG_COMPOSITE（program 2/4）的 source-over 合成处理（单 quad，GPU 合成），无需独立 RenderNode。
/// 仅"独立 quad 下层"（如 box-shadow 的偏移阴影 quad）走此位。
pub(crate) const BACK_LAYER_FLAG: u32 = 0x1000_0000;
```

- [ ] **Step 3: Rename `shadow_pairs` → `back_layer_pairs`**

In `render/mod.rs`:
- ~line 276: `let mut shadow_pairs: Vec<(u32, u32)> = Vec::new();` → comment + name:
  ```rust
  // back-layer 合成 RenderNode 追踪：(主节点 node_id, 下层合成 node_id)。
  // 当前唯一使用者 box-shadow；未来"独立 quad 下层"复用。
  let mut back_layer_pairs: Vec<(u32, u32)> = Vec::new();
  ```
- ~line 641 (inside box-shadow emission): `shadow_pairs.push((node_id, sid));` → `back_layer_pairs.push((node_id, sid));`
- ~line 682: `propagate_box_shadow_sort_keys(&mut nodes, &shadow_pairs);` → `propagate_back_layer_sort_keys(&mut nodes, &back_layer_pairs);`

- [ ] **Step 4: Rename the propagate fn + update its doc (`render/mod.rs` ~755-760)**

Replace the fn signature + the doc lines above it that say "box-shadow 合成节点":

```rust
/// assign_sort_keys 只给 id_to_pos 中的真 scene 节点赋 sort_key；
/// back-layer 合成节点不在场景树中，初始 sort_key=0。此函数将其调整到
/// 主节点 sort_key 位置，保证下层（如 box-shadow）在主节点之前渲染。
///
/// 处理多个下层节点时从最大 sort_key 开始（降序），避免累积偏移后的 stale 值。
fn propagate_back_layer_sort_keys(
    nodes: &mut [RenderNode],
    back_layers: &[(u32, u32)], // (main_node_id, back_layer_node_id)
) {
```
Body is unchanged (it already references `shadows` param internally — rename that param to `back_layers` and the `shadow_id`/`shadow_pos` locals to `back_id`/`back_pos` for consistency; purely cosmetic, the logic stays).

Also ~line 619-621 (the box-shadow block's leading comment mentions "box-shadow：独立 RenderNode" + "propagate_box_shadow_sort_keys"): update the fn name in that comment to `propagate_back_layer_sort_keys`. The comment can stay box-shadow-specific (it IS the box-shadow block).

- [ ] **Step 5: Rename refs in `batch.rs` and `merge.rs`**

`crates/core/src/render/batch.rs:38`:
```rust
        && (rn.node_id & crate::render::BACK_LAYER_FLAG == 0)
```
Update the surrounding comment (~line 30-38) if it says "box-shadow" to say "back-layer (box-shadow)". Minimal: just the identifier must change.

`crates/core/src/render/merge.rs:26`:
```rust
    if rn.node_id & crate::render::BACK_LAYER_FLAG != 0 {
        return None; // back-layer 合成节点（如 box-shadow）不合批
    }
```

- [ ] **Step 6: Rename refs in `tests.rs`**

In `crates/core/src/render/tests.rs`:
- box-shadow test (~2756, 2761, 2767): `BOX_SHADOW_FLAG` → `BACK_LAYER_FLAG` (3 occurrences in `box_shadow_emits_node_with_offset_and_sort_key`). Test name + assertions stay (it still tests box-shadow behavior, which is unchanged).
- bit-boundary test (~2669-2682, `synth_text_node_id_roundtrip` and neighbors): every `BOX_SHADOW_FLAG` → `BACK_LAYER_FLAG`, and update the inline comments "BOX_SHADOW_FLAG bit 28" → "BACK_LAYER_FLAG bit 28".

- [ ] **Step 7: Verify the refactor compiles + tests pass + lint clean**

Run:
```bash
cargo test -p loomgui_core 2>&1 | tail -5
cargo fmt --all -- --check
cargo clippy --all-targets -p loomgui_core -- -D warnings 2>&1 | tail -5
```
Expected: all core tests pass (box-shadow test green under new name); fmt clean; clippy clean.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/render/mod.rs crates/core/src/render/batch.rs crates/core/src/render/merge.rs crates/core/src/render/tests.rs
git commit -m "refactor(render): rename BOX_SHADOW_FLAG -> BACK_LAYER_FLAG (name by semantics)

The bit marks an auxiliary RenderNode drawn under its primary (synthetic
node_id + sort_key propagation + dedup-exclusion). box-shadow is its only
current producer; the name now describes the role, not the user. Doc spells
out the boundary: bg-color-under-texture goes through shader BG_COMPOSITE,
not this flag. Zero behavior change."
```

---

### Task 2: Fix `<img>` bg-color via BG_COMPOSITE

Make `NodeKind::Image` honor `background-color` the same way Container does: when bg-color is present (alpha > 0), use program 2 (BG_COMPOSITE; program 4 if also filtered) with the bg-color as vertex color, so the shader's source-over composites the texture over the bg-color and transparent texels show the bg. When no bg-color, behavior is unchanged (program 0/3, white vertex).

**Files:**
- Modify: `crates/core/src/render/mod.rs` Image arm (~481-507)
- Test: `crates/core/src/render/tests.rs` (add 2 tests)

**Interfaces:**
- Consumes: `anim: Option<&NodeAnim>` (has `.bg_color`), `has_filter: bool` (both already in scope before the `match n.kind` at ~316), `scene.image_srcs`, `n.style.background_color`, `n.style.border_image_slice`, mesh fns `crate::render::mesh::{quad, nine_slice}`.
- Produces: Image-arm RenderNode with `program` 0/2/3/4 and vertex color white-or-bg (no new public surface).

- [ ] **Step 1: Write the failing test — Image + bg-color → program 2, vertex = bg-color**

Append to `crates/core/src/render/tests.rs` (near the other Image tests, e.g. after `build_image_uv_is_full_region`):

```rust
/// Image (`<img>`) + bg-color → program 2 (BG_COMPOSITE)，顶点色 = bg-color。
/// shader source-over：图(tex) over 底色(vcol)，透明像素透出底色（修紫底不显示 bug）。
/// 无需 back-layer / 合成 node_id——单 quad，GPU 合成（与 Container 同路径）。
#[test]
fn build_image_with_bg_color_uses_bg_composite() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        Some([0.5, 0.0, 0.5, 1.0]), // 紫底
    );
    n.kind = NodeKind::Image;
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.image_srcs.insert(scene.roots[0], "icon.png".into());

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    assert_eq!(frame.nodes.len(), 1, "Image 单 quad（shader 合成，不产 back-layer）");
    match &frame.nodes[0].payload {
        NodePayload::Mesh {
            program,
            colors,
            image_path,
            ..
        } => {
            assert_eq!(*program, 2, "Image+bg-color → program 2 (BG_COMPOSITE)");
            assert_eq!(
                *colors.first().unwrap(),
                [0.5, 0.0, 0.5, 1.0],
                "顶点色 = bg-color（紫）"
            );
            assert_eq!(
                *image_path,
                Some("icon.png".to_string()),
                "image_path = src"
            );
        }
        _ => panic!("expected Mesh"),
    }
}

/// Image + bg-color + filter → program 4（BG_COMPOSITE + COLOR_FILTER 双 keyword）。
#[test]
fn build_image_with_bg_color_and_filter_uses_program_4() {
    let mut n = container_node(
        0,
        None,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        },
        Some([0.5, 0.0, 0.5, 1.0]),
    );
    n.kind = NodeKind::Image;
    n.style.color_filter = Some([0.0; 20]); // grayscale-ish filter 触发 has_filter
    let mut scene = Scene::from_nodes(vec![n], vec![]);
    scene.image_srcs.insert(scene.roots[0], "icon.png".into());

    let fonts = test_font_table().expect("need font");
    crate::scene::transform::compute_world_transforms(&mut scene);
    let (frame, _, _) = build_render_nodes(
        &scene,
        &fonts,
        &std::collections::HashMap::new(),
        &empty_sizes(),
        &mut test_glyph_atlas(),
    );
    match &frame.nodes[0].payload {
        NodePayload::Mesh { program, .. } => {
            assert_eq!(*program, 4, "Image+bg-color+filter → program 4");
        }
        _ => panic!("expected Mesh"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```bash
cargo test -p loomgui_core build_image_with_bg_color 2>&1 | tail -15
```
Expected: both FAIL — `program` is 0 (not 2) and vertex color is `[1,1,1,1]` (white, not purple), because the Image arm currently ignores bg-color.

- [ ] **Step 3: Implement — route Image through BG_COMPOSITE when bg-color present**

In `crates/core/src/render/mod.rs`, replace the `NodeKind::Image` arm (the block starting `NodeKind::Image => {` around line 481 through its `let program = if has_filter { 3u32 } else { 0u32 };` at ~507). New arm:

```rust
            NodeKind::Image => {
                let src = scene.image_srcs.get(&n.id).cloned().unwrap_or_default();
                let image_path = Some(src.clone());
                let uv_min = [0.0, 0.0];
                let uv_max = [1.0, 1.0];
                let (src_w, src_h) = src_size(image_sizes, &src);
                // bg-color 走 BG_COMPOSITE（shader source-over：图 over 底色，透明像素透出
                // 底色）——与 Container 同路径。无 bg-color 时 program 0（tex×白 = 原图）。
                let bg_opt = anim.and_then(|a| a.bg_color).unwrap_or(n.style.background_color);
                let has_bg = bg_opt.map(|c| c[3] > 0.0).unwrap_or(false);
                let vertex_color = if has_bg { bg_opt.unwrap() } else { [1.0, 1.0, 1.0, 1.0] };
                let program = if has_filter {
                    if has_bg { 4u32 } else { 3u32 }
                } else if has_bg {
                    2u32
                } else {
                    0u32
                };
                let (v, uvc, col, idx) = match &n.style.border_image_slice {
                    Some(slice) => {
                        let resolved = resolve_slice_percent(slice, src_w, src_h);
                        crate::render::mesh::nine_slice(
                            rect,
                            vertex_color,
                            &resolved,
                            src_w,
                            src_h,
                            [uv_min[0], uv_max[1]],
                            [uv_max[0], uv_min[1]],
                        )
                    }
                    None => crate::render::mesh::quad(
                        rect,
                        vertex_color,
                        [uv_min[0], uv_max[1]],
                        [uv_max[0], uv_min[1]],
                    ),
                };
                RenderNode {
                    node_id,
```
(The `RenderNode { ... }` tail of the arm is unchanged — `colors: col`, `image_path`, `program` now use the new values.)

- [ ] **Step 4: Run the new tests to verify they pass**

Run:
```bash
cargo test -p loomgui_core build_image_with_bg_color 2>&1 | tail -8
```
Expected: both PASS.

- [ ] **Step 5: Verify no regression in existing Image / Container tests**

Run:
```bash
cargo test -p loomgui_core 2>&1 | tail -5
```
Expected: all green. Specifically unchanged:
- `build_image_uv_is_full_region` (Image, no bg-color → program 0, white vertex) — still green.
- `build_container_bg_image_coexists_with_bg_color` (Container bg-image+bg-color → program 2, vertex = bg-color) — Container arm untouched, still green.
- All box-shadow tests (renamed in Task 1) — still green.

- [ ] **Step 6: Lint clean**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -p loomgui_core -- -D warnings 2>&1 | tail -5
```
Expected: fmt clean; clippy clean.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/render/mod.rs crates/core/src/render/tests.rs
git commit -m "fix(render): Image kind honors background-color via BG_COMPOSITE

NodeKind::Image used program 0 (tex*white) and never read background-color,
so an <img> with a bg-color showed no bg (spec4b card-img purple bg bug).
Route Image through the same BG_COMPOSITE shader path Container already uses
when bg-color is present: program 2 (4 if also filtered), vertex color =
bg-color. The shader's source-over composites tex over bg, transparent
texels show the bg. No bg-color → unchanged (program 0, white vertex)."
```

---

### Task 3: Mark superseded spec + rebuild .dll + commit

The core render change requires a rebuilt `.dll` (家里机 needs it). Also prepend a SUPERSEDED note to the abandoned spec so it doesn't mislead.

**Files:**
- Modify: `docs/superpowers/specs/2026-07-20-image-bg-compositional-layer-design.md` (prepend note)
- Rebuild + commit: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`

- [ ] **Step 1: Prepend SUPERSEDED note to the spec**

At the very top of `docs/superpowers/specs/2026-07-20-image-bg-compositional-layer-design.md`, before the first `#` heading, add:

```markdown
> **⚠️ SUPERSEDED (2026-07-20):** The back-layer mechanism for image-bg described below was abandoned as YAGNI after a shader read revealed `BG_COMPOSITE` (program 2/4) already does source-over compositing for Container — Image just wasn't using it. The actual fix is ~5 lines (Image → BG_COMPOSITE when bg-color present), **not** a synthetic-id back-layer. Only the rename portion (`BOX_SHADOW_FLAG` → `BACK_LAYER_FLAG`, by semantics) survived. See `docs/superpowers/plans/2026-07-20-image-bg-color-via-bg-composite.md`.
```

- [ ] **Step 2: Full workspace verification before .dll rebuild**

Run:
```bash
cargo test 2>&1 | tail -8
cargo fmt --all -- --check
cargo clippy --all-targets --workspace --exclude loomgui_gui -- -D warnings 2>&1 | tail -5
```
Expected: all workspace tests green; fmt clean; clippy clean (CI-exact commands).

- [ ] **Step 3: Rebuild the .dll**

**Ensure Unity is closed** (it locks the `.dll`). Then:

```bash
cargo build -p loomgui_ffi_c --release 2>&1 | tail -3
```
Expected: `Finished release profile`. (No `xtask sync-bindings` — FFI signatures are unchanged.)

- [ ] **Step 4: Copy the rebuilt .dll into the Unity plugin dir**

```bash
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
ls -la unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
Expected: the file's mtime is now (just rebuilt). Confirm the size differs from the previously committed dll.

- [ ] **Step 5: Commit the spec note + rebuilt .dll**

```bash
git add docs/superpowers/specs/2026-07-20-image-bg-compositional-layer-design.md unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "chore(ffi): rebuild dll for Image bg-color BG_COMPOSITE fix + supersede spec

core render changed (Image arm program/vertex selection) -> rebuild dll so
the home machine can test. Also mark the back-layer spec SUPERSEDED: the
shader BG_COMPOSITE path made the synthetic-id mechanism unnecessary."
```

- [ ] **Step 6: Push + confirm CI green**

```bash
git push origin main
```
Then watch the triggered Rust CI run to green (fix-3's clippy is already green; this verifies the new commits don't regress):
```bash
RUN=$(gh run list --branch main --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$RUN" --exit-status && echo "CI green"
```
Expected: `CI green` (all jobs success).

- [ ] **Step 7: Hand off to home machine for Unity visual acceptance**

The core fix is verified headless (Tasks 1-2 tests). The visual confirmation (purple bg actually shows through card-img's transparent texels in Unity PlayMode) is the home machine's job. Note for handoff: spec4b-acceptance card-img 视觉第 5 门 should now turn green; also eyeball any `<img>` + bg-color across showcase pages.

---

## Self-Review (done by plan author)

**Spec coverage:** The agreed scope (rename + Image fix + dll) is fully covered: Task 1 = rename, Task 2 = Image bg-color fix, Task 3 = supersede note + dll + push. The abandoned back-layer mechanism is explicitly marked superseded (Task 3 Step 1) so the earlier spec doesn't mislead. Container is intentionally untouched (shader already handles it) — noted in Task 2 Step 5 regression check.

**Placeholder scan:** No TBD/TODO. Every code step shows complete code. Commands have expected output.

**Type consistency:** `BACK_LAYER_FLAG`, `back_layer_pairs`, `propagate_back_layer_sort_keys` used consistently across Tasks 1's source edits and tests. Image arm uses `bg_opt: Option<[f32;4]>`, `vertex_color: [f32;4]`, `program: u32` — matches `NodePayload::Mesh { program: u32, colors: Vec<[f32;4]>, .. }` and the existing Container arm's types. `anim.and_then(|a| a.bg_color)` mirrors Container arm (`mod.rs:318`), so `NodeAnim.bg_color: Option<[f32;4]>` is correct.

**Risk note:** Task 2 Step 3's `n.style.color_filter = Some([0.0; 20])` in the program-4 test — verify `color_filter` is the right field name by grepping `color_filter` in `style/resolved.rs` before writing the test; if the field is named differently (e.g. `filter`), adjust. `has_filter` at `mod.rs:310` reads `n.style.color_filter.is_some()`, confirming the field name.
