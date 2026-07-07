## Task 8 Report: Stage B -- multi-page atlas + cross-page mesh split

### Status: DONE

### Multi-page allocate (Step 1)

Already implemented by T1's `allocate` method: iterates existing etagere
allocators, pushes a new `AtlasPage(4096^2)` on overflow. No code change
needed -- built into T1 in anticipation of Stage B.

### build_text_mesh return-type change (Step 2)

Now returns `Vec<(u32 page, Vec<verts>, Vec<uvs>, Vec<colors>, Vec<indices>)>`.
Two-pass: (1) collect glyphs grouped by atlas page into BTreeMap, (2) build
independent mesh per page group. Single-font single-size text stays on one
page -> one Vec entry -> one Mesh (unchanged from stage A). CJK spanning
pages -> multiple Meshes (extra draw calls, acceptable escape valve).

### Multi-RenderNode identity decision

Page 0 uses real `node_id`. Pages 1..N use synthetic id:
`primary_id & 0x00FF_FFFF | ((page & 0xFF) << 24)`. Bits [31:24] reserve
page sub-index. Real scene node indices from slotmap (< ~4096 in practice)
never set these bits -> no collision. Each synthetic id gets independent
change detection in dirty hash table. Backend MirrorPool sees distinct keys.

### Sort key propagation

`propagate_text_sub_page_sort_keys` added between `assign_sort_keys` and
`reorder_for_batching`: shifts subsequent real-node sort_keys by sub-page
count, then sets sub-page sort_key = primary.sort_key + page_idx, copies
mask_context. Draw order stays monotonic.

### Test results

- overflow_allocates_second_page: PASS (alloc->fill page0, ensure->page1+dirty)
- Full cargo test -p loomgui_core: **539/539 passed** (unchanged)
- snapshot_simple_panel and cascade_inheritance: PASS (single-page ASCII unchanged)
- cargo fmt: PASS
- cargo clippy --all-targets -- -D warnings: PASS
- cargo build -p loomgui_ffi_c --release: PASS (no FFI changes, no .dll commit)
- Cargo.lock: unchanged

### Files changed

- `loomgui_core/src/text/atlas.rs` -- overflow_allocates_second_page test
- `loomgui_core/src/render/mod.rs` -- build_text_mesh return type, Text branch
  multi-page emission, synthetic node_id helpers, sort_key propagation

### Commit

`b204d9c` feat(core,v1.6b): multi-page atlas + cross-page text mesh split

### Self-review concerns

- Cross-page draw-call cost: each additional page = 1 extra draw call. CJK at
  48px (~3000 glyphs per 4096^2 page) -> rarely triggers multi-page.
- Synthetic id upper bound: 255 sub-pages = 4GB GPU memory, far beyond practical.
  No collision risk.
- merge_meshes text: different page path -> won't merge, correct behavior.

## Fixes (review round 1)

Commit: `f177320` fix(core,v1.6b): T8 review findings -- reuse_key/sort_key/synth-id guard

### C1 -- `reuse_key` collision on text sub-pages (render/mod.rs:239)

sub-pages had `reuse_key: n.reuse_key` inheriting from the scene text node.
If the text node is in a virtual list slot (reuse_key = slotIdx > 0), all sub-pages
share the same reuse_key. MirrorPool creates/updates ONE GameObject per unique
reuse_key, so sub-page 1 overwrites primary page 0's mesh, sub-page 2 overwrites
sub-page 1, etc. -- only the last sub-page's glyphs survive.

**Fix:** sub-pages use `reuse_key: 0`. Each sub-page has a distinct synthetic
node_id; reuse_key=0 means MirrorPool keys by node_id -- independent GameObjects
per page, all render. When the slot rebinds, old synthetic node_ids disappear,
GOs cleaned up normally.

### I1 -- Stale `primary_sk` in propagate_text_sub_page_sort_keys (render/mod.rs:542-551)

`shifts` captures primary sort_keys BEFORE any shifts. When the shift loop runs,
`*primary_sk` in `rn.sort_key > *primary_sk` is stale -- doesn't reflect the
primary's current sort_key after earlier iterations shifted it. Bug fires when
2+ text nodes each have sub-pages: causes sort_key ties.

**Fix:** track cumulative shift. `adjusted_sk = primary_sk + cum_shift`,
`cum_shift += n` after each iteration. Verified with manual trace: 2 text nodes
(A sk=2 sub=2, B sk=3 sub=1, C sk=4) -- old code produced tie at sk=8,
fixed code produces monotonic 2,3,4,5,6,7.

### I2 -- Synthetic node_id collision risk unguarded (render/mod.rs:490-491)

Real scene node index < 4096 was a soft assumption. If a real node's index >=
4096, `is_text_sub_page` (bits[31:24] > 0) returns TRUE for REAL nodes,
corrupting sort_keys in propagate_text_sub_page_sort_keys.

**Fix:** added `debug_assert!` at the top of `build_render_nodes` that fires
if any real scene node index >= 4096, with a clear message pointing at the
synthetic-id limit. Also added `synth_text_node_id_roundtrip` and
`node_index_4096_triggers_sub_page_collision` tests documenting the boundary.

### I3 -- `id_to_pos` not updated for sub-page nodes (render/mod.rs:222-247)

Sub-page RenderNodes pushed to `nodes` but NOT inserted into `id_to_pos`.
Intentional (synthetic ids shouldn't map back to scene nodes), but a latent
invariant break -- fragile without documentation.

**Fix:** added comment at the sub-page push site explaining WHY sub-pages are
intentionally excluded from `id_to_pos` (synthetic ids; render-only, not scene
nodes; scrollbar/hash code iterates `nodes` not `id_to_pos`).

### Tests added (for C1/I1/I2)

- `text_sub_pages_reuse_key_is_zero_not_inherited` -- C1 regression: verifies
  primary inherits reuse_key=7, any sub-pages get reuse_key=0.
- `propagate_text_sub_page_sort_keys_cumulative_shift_no_ties` -- I1 regression:
  constructs 2 text nodes with sub-pages, verifies monotonic sort_keys with
  no ties (2,3,4,5,6,7).
- `synth_text_node_id_roundtrip` -- I2 encoding roundtrip, verifies index=4095
  is NOT misidentified as sub-page.
- `node_index_4096_triggers_sub_page_collision` -- I2 boundary: verifies
  index=4096 IS misidentified (proof of hard limit motivation).

### Test summary

- `cargo test -p loomgui_core`: **580/580 passed** (543+25+2+3+5+2 unit tests)
- `cargo fmt --all -- --check`: PASS
- `cargo clippy -p loomgui_core --all-targets -- -D warnings`: PASS
- Snapshots unchanged (single-page ASCII text unaffected)
- No FFI change, no .dll rebuild, no CI yml change
