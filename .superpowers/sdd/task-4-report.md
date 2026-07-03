## Task 4 Report: 支柱3 ChangeLevel 机制

### Status: DONE

### Commit
`ace37d7` feat(core): 支柱3 ChangeLevel 机制 -- 删 Unchanged 变体，merge 后按 node_id 定级

### Changes (12 files)
| File | Change |
|------|--------|
| `loomgui_core/src/render/node.rs` | +ChangeLevel enum (#[repr(u8)]) + change_level field on RenderNode; -Unchanged variant; -Unchanged doc |
| `loomgui_core/src/render/mod.rs` | build_render_nodes rewrite: delete pre-allocation of Unchanged, hash after merge by node_id; +change_level_skip_header_full + change_level_reload_all_full tests; -4 old Unchanged tests; thumb_render_node +change_level |
| `loomgui_core/src/render/merge.rs` | merge_batch +change_level: ChangeLevel::Full (merged always Full); comment drop "Unchanged"; mesh_node helper +change_level |
| `loomgui_core/src/render/dirty.rs` | -node_hash function (monolithic hash); -Unchanged arm in payload_hash; -19 node_hash-based tests; mesh_rn/text_rn_content helpers +change_level; keep header_hash + payload_hash |
| `loomgui_core/src/render/batch.rs` | placeholder_rn: Unchanged->Mesh quad; +change_level on both test helpers; +ChangeLevel import |
| `loomgui_core/src/stage.rs` | prev_node_hashes: Vec<u64> -> HashMap<u32,(u64,u64)>; init Vec::new() -> HashMap::new() |
| `loomgui_core/examples/dump_render.rs` | -Unchanged match arm |
| `loomgui_core/examples/dump_interact.rs` | -Unchanged match arm |
| `loomgui_core/tests/v1e_dirty.rs` | Unchanged->ChangeLevel assertions; stage_static_frame_produces_skip / stage_reload_all_full |
| `loomgui_core/tests/snapshots/*.snap` | 3 snapshot updates (JSON now includes change_level: "Full") |

### Test Results
```
cargo test -p loomgui_core --features parse
478 passed (lib) + 10 fence + 3 snapshot + 2 v1e_dirty = 493 passed, 0 failed
```

Key tests: `change_level_skip_header_full` (Full->Skip->Header->Full), `change_level_reload_all_full` (stale prev -> Full).

### Self-Review
- D2 resolution verified: merging happens before hashing; merged nodes that moved have baked-in verts -> payload_hash changes -> auto-Full.
- D3 resolution verified: empty `prev` HashMap -> all Full (via `None` branch in the match).
- Test count drop from 514->478 in lib: expected, due to deletion of 19 monolithic `node_hash` tests + 4 Unchanged-based tests, offset by 2 new change_level tests.
- `loomgui_ffi_c` intentionally left non-compiling (blob.rs still has `NodePayload::Unchanged` match arm -- T5 fixes it).

### Concerns
- **Header-level threshold**: The `header_hash` only covers world_matrix, visible, sort_key, mask_context, color_tint, blend. Layout position changes that go through the pure-translation rect path change verts -> payload_hash -> Full. This means most layout changes will trigger Full, not Header. Header is only triggered by changes to color_tint or visible (in practice). This may be too narrow -- T7 (alpha uniform) will add alpha to header_hash.
- **Test adaptation from brief**: The brief's `change_level_skip_header_full` test expected "pure translation -> HEADER" using `layout_rect.x = 50.0`. This does NOT work because the pure-translation rect path bakes world.tx into verts -> payload_hash changes. The committed test uses `style.color` change instead, which correctly triggers Header-only diff. Noted as a brief-to-reality discrepancy (pure-translation optimization precludes header-only position changes without payload re-hashing).

## Fix: payload_hash position re-base

### Status
Fix applied to address the concern above -- pure translation now correctly produces Header.

### The Bug
`payload_hash` hashed pre-rebase (position-baked) verts. When a pure-translation node moved, `world_matrix` changed (header_hash), but verts also changed (because `Rect{x:wm[4],y:wm[5]}` baked absolute position). This caused `payload_hash` to also change -- the level became **Full**, not **Header**. Sliding/transform animations would rebuild every mesh, defeating 支柱3's purpose.

Root cause: `blob.rs:104-111` re-bases verts to local by subtracting `(tx, ty)` so the bytes C# receives are position-independent. But `payload_hash` (in `dirty.rs`) was hashing the pre-rebase verts, which are position-dependent.

### The Fix
- **`loomgui_core/src/render/dirty.rs`**: In `payload_hash`'s Mesh arm, before hashing verts, re-base them the same way `blob.rs` does: for pure-translation nodes subtract `(world_matrix[4], world_matrix[5])` from each vert; for non-pure-translation nodes verts are already local (no subtract). Uses `crate::transform::is_pure_translation(&rn.world_matrix)` to decide. uvs/colors/indices/image_path/program/color_matrix hashing unchanged.
- **`loomgui_core/src/render/mod.rs`**: Updated `change_level_skip_header_full` test to restore the brief's original intent. Frame 3 now tests position change (`layout_rect.x = 50.0` + `compute_world_transforms`) producing `ChangeLevel::Header`. Color-change Header test kept as frame 4, bg-change Full test as frame 5.

### TDD Evidence
Before fix: position change produced Full (verts baked at absolute world position changed payload_hash). After fix: position change produces Header (verts re-based to local are identical, only world_matrix changes header_hash).

Test `change_level_skip_header_full` assertions:
- Frame 1: Full (no prev baseline)
- Frame 2: Skip (no changes)
- Frame 3: **Header** (position: `layout_rect.x=50`) -- restored per brief
- Frame 4: Header (color: `style.color` changed)
- Frame 5: Full (bg: `style.background_color` changed)

### Merged Nodes
Merged nodes set `world_matrix = IDENTITY`. `is_pure_translation(IDENTITY)` = true, `tx=ty=0`, so re-base subtracts 0 -- merged verts hashed as-is (absolute). Merged nodes are always Full (explicitly set in `merge_batch`), consistent. All merge tests pass.

### Test Results
```
cargo test -p loomgui_core --features parse
478 (lib) + 10 fence + 3 snapshot + 2 v1e_dirty = 493 passed, 0 failed
```
