# Pooled Slot Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 坑182 (mail scroll GO churn / item disappearance) by replacing the detach-to-free-pool virtual-list model with a parked-but-attached model (slots stay in `<ul>`, display:none when off-screen), add a persistent Unity GO pool driven by a new blob `parked` bit, and fix `Get<T>("id")` global-first-match via subtree-scoped lookup (L1).

**Architecture:** Core decides slot parked/active state and emits it via blob (parked bit in the existing `visible` byte — zero version bump). Unity MirrorPool keeps parked GOs alive (`SetActive(false)`, never destroy — fgui dormant model). Reuse_key becomes a permanent per-slot ordinal. `Get<T>("id")` switches from global find + subtree post-filter to a direct subtree DFS. See `docs/superpowers/specs/2026-08-05-pooled-slot-lifecycle-design.md` for the full design.

**Tech Stack:** Rust (edition 2021, taffy 0.12, slotmap 1.1), csbindgen FFI, C#/Unity (MirrorPool/MaterialManager), headless test harness (Spec-4a, P/Invoke real dll).

## Global Constraints

- **Public API freeze**: `unity/package/Runtime/Public/LoomGUI.*.cs` signatures unchanged; `tests/dotnet/LoomGUI.PublicApi` compile gate must stay green. (Spec constraint b)
- **Tick timing invariant**: `process → rematch → solve → refresh_content → compute_world_transforms → build` order preserved; exactly one solve/frame. (Spec constraint c)
- **Cross-engine portability**: all pooling decisions live in Rust core; Unity MirrorPool only reads blob. No `UnityEngine` in any decision path. (Spec constraint d)
- **No memory eviction**: dormant pool grows to high-water, never shrinks (constraint e accepted). Parked GOs destroyed only on component teardown (`MirrorPool.Clear`).
- **pkg version: zero bump.** blob parked bit packs into existing `visible` byte bit1; list.rs changes are runtime-only (data-driven mode triggers via `ItemCount` setter, not pack-time).
- **AGENTS.md rules**: code comments ship-quality (say WHY, no internal numbers/slang); fix root cause not compensate; `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` clean before every commit; after any Rust change recompile `.dll` + `cargo run -p xtask -- sync-bindings` + copy to `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` (Unity closed).
- **Two-machine rule**: core logic verifiable headless on the encoding machine (Windows); Unity PlayMode + Profiler (mail churn) defer to home machine. Spec §6.6.

---

## File Structure

**Rust core (`crates/core/src/`)** — slot lifecycle + scope:
- `list.rs` — MAJOR: `Slot`/`ListState` structs, `enter_data_driven`, `plan_visible`/`plan_one`, `execute_visible`/`execute_one`, `collect_heights`, `notify_inserted`/`notify_removed`/`notify_moved`, `encode_reuse_key` caller, tests.
- `scene/node.rs` — add `find_node_by_id_in_subtree` (new, alongside existing global `find_by_id_attr`).
- `stage.rs` — add `find_node_by_id_in_subtree` passthrough (alongside `find_node_by_id`).

**FFI (`crates/ffi/src/`, `crates/ffi_c/src/`)**:
- `ffi/src/blob.rs` — `build_blob`: append parked-keepalive entries.
- `ffi_c/src/lib.rs` (or current FFI surface file) — new `loomgui_stage_find_node_by_id_in_subtree` FFI.

**Unity C# (`unity/package/Runtime/`)**:
- `FrameBlob.cs` — `Visible` mask bit0; add `Parked` (bit1).
- `MirrorPool.cs` — `Sync` parked branch + reactivate `SetActive(true)` + `DumpState` active column.
- `Public/LoomGUI.Nodes.cs` — `TryGet`/`Get` call new subtree FFI; drop `IsInSubtree` post-filter.

**Docs** (final task): `docs/design/fence.md`, `docs/design/public-api.md` (scope notes), `docs/pitfalls.md` (坑182 status).

---

## Task 1: Core struct foundation + enter_data_driven pre-alloc parked batch

**Files:**
- Modify: `crates/core/src/list.rs` (Slot ~88-92, ListState ~94-110, enter_data_driven ~151-218, ListState::new/Default)
- Test: `crates/core/src/list.rs` (mod tests)

**Interfaces:**
- Produces: `Slot { node, item_index, parked: bool }`; `ListState` with `free` field DELETED; `enter_data_driven` pre-allocates `INITIAL_SLOTS` slots all parked (display:none override set, attached under ul between spacers).

- [ ] **Step 1: Write failing tests**

In `crates/core/src/list.rs` `mod tests`, add:

```rust
#[test]
fn enter_data_driven_pre_allocates_parked_slots() {
    let mut stage = build_test_list_stage(/* ul with 1 design-time <li> template */, 100);
    stage.enter_data_driven(ul);  // item_count=100
    let ls = stage.scene.lists.get(ul).unwrap();
    assert!(ls.free.is_empty(), "free pool deleted — no detach model");
    assert!(ls.slots.len() >= INITIAL_SLOTS, "pre-allocated batch");
    for s in &ls.slots {
        assert_eq!(stage.scene.get(s.node).unwrap().parent, Some(ul), "slot parented to ul");
        assert!(s.parked, "initial batch all parked");
        // display:none override set (sticky-note layer):
        assert!(stage.scene.get(s.node).unwrap().inline_set.contains(InlineBit::Display),
            "display:none inline override set on parked slot");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p loomgui_core enter_data_driven_pre_allocates_parked_slots`
Expected: FAIL (compile error: no `parked` field / `free` field access / INITIAL_SLOTS pre-alloc).

- [ ] **Step 3: Implement struct changes**

In `list.rs`:
```rust
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub node: NodeId,
    pub item_index: usize,
    pub parked: bool,   // true = display:none override set, off-screen
}
```
Delete `pub free: Vec<NodeId>` from `ListState` and its init in `ListState::new`/Default. Delete any `ls.free` reads/writes (grep `ls\.free` / `\.free\.`). Adjust all `Slot { node, item_index }` literals to add `parked: true` (initial) — grep `Slot {` to find them.

- [ ] **Step 4: Rework enter_data_driven to pre-alloc parked batch**

In `enter_data_driven` (~151-218): after the existing "backup template + clear design-time li" logic, replace the "empty slots vec" init with a loop cloning `INITIAL_SLOTS` slots from `template_root`, each: `append_child(scene, ul, cloned)` (between head/tail spacers — use `insert_before(tail_spacer)`), `set_inline_override(scene, cloned, "display:none")`, push `Slot { node: cloned, item_index: 0, parked: true }`. Keep `template_root` field (still needed for grow clones in Task 3).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p loomgui_core enter_data_driven_pre_allocates_parked_slots`
Expected: PASS.

- [ ] **Step 6: Run full core suite + fmt/clippy, commit**

Run: `cargo test -p loomgui_core && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: some pre-existing free-pool tests now fail (expected — rewritten in Task 7); gate this task's commit on ONLY the new test + struct-adjacent tests passing, not the full suite. Stash/`#[ignore]` the now-broken free-pool tests temporarily (Task 7 rewrites them) OR commit with `#[ignore]` + a TODO comment.
Commit: `refactor(core): parked-but-attached slot struct + enter_data_driven pre-alloc`

---

## Task 2: plan_visible — mark active/parked, delete detach logic

**Files:**
- Modify: `crates/core/src/list.rs` (`plan_visible`/`plan_one` ~846-924, esp. Phase B/C `to_free`/`remove_child`/`ls.free.extend`)
- Test: `crates/core/src/list.rs`

**Interfaces:**
- Consumes: Task 1 `Slot.parked`.
- Produces: `plan_visible` marks `slot.parked` per visible range; no `remove_child`, no `to_free`. Slots staying in visible range keep `parked=false`; off-range slots set `parked=true`. Items in range lacking a slot are collected as `to_bind`.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn plan_visible_marks_park_no_detach() {
    let mut stage = build_test_list_stage(ul, 100);
    stage.enter_data_driven(ul);
    // scroll so visible = [10..20]; slots initially parked at item 0
    stage.set_scroll(ul, scroll_for_item_10);
    stage.plan_visible(ul);   // does NOT execute yet
    let ls = stage.scene.lists.get(ul).unwrap();
    for s in &ls.slots {
        // none detached:
        assert_eq!(stage.scene.get(s.node).unwrap().parent, Some(ul), "no slot detached");
    }
    // at least one slot marked parked, at least one not (visible-range items get slots in execute, not plan;
    // plan only sets parked flags + to_bind)
    assert!(ls.slots.iter().any(|s| s.parked), "off-range slots parked");
}
```

- [ ] **Step 2: Run, expect FAIL** (`cargo test -p loomgui_core plan_visible_marks_park_no_detach` — plan still detaches).

- [ ] **Step 3: Rewrite plan_one Phase B/C**

Replace the `drain + keep_slots + to_free` + `remove_child` + `ls.free.extend` blocks with:
```rust
// Phase B: partition current slots — keep-active vs mark-to-park; collect unbound visible items.
let mut to_bind: Vec<usize> = Vec::new();
let bound_items: HashSet<usize> = ls.slots.iter()
    .filter(|s| !s.parked).map(|s| s.item_index).collect();
for item in new_visible_range {
    if !bound_items.contains(&item) { to_bind.push(item); }
}
// mark off-range slots parked (don't detach):
for s in ls.slots.iter_mut() {
    if !new_visible_range.contains(&s.item_index) {
        if !s.parked { s.parked = true; }  // execute applies the display override
    }
}
```
Delete all `to_free` / `remove_child` / `ls.free` references. `plan_visible` returns an op struct carrying `to_bind` (+ list_ul, tail_spacer) for execute.

- [ ] **Step 4: Run, expect PASS.** Commit: `refactor(core): plan_visible marks parked, no detach`

---

## Task 3: execute_visible — unpark+bind / park / grow (lazy clone)

**Files:**
- Modify: `crates/core/src/list.rs` (`execute_visible`/`execute_one` ~950-1000)
- Test: `crates/core/src/list.rs`

**Interfaces:**
- Consumes: Task 2 `to_bind` op + `Slot.parked`.
- Produces: execute flips parked→active (`unset_inline_override(display)` + `parked=false` + rebind), active→parked (`set_inline_override(display:none)` + `parked=true`), grows by cloning from `template_root` when no parked slot available. Populates `pending_binds`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn execute_unparks_and_binds_visible_items() {
    let mut stage = build_test_list_stage(ul, 100);
    stage.enter_data_driven(ul);
    stage.set_scroll(ul, scroll_for_item_10);
    let ops = stage.plan_visible(ul);
    stage.execute_visible(ops);
    let ls = stage.scene.lists.get(ul).unwrap();
    // visible items bound to active (un-parked) slots:
    let active: Vec<usize> = ls.slots.iter().filter(|s| !s.parked).map(|s| s.item_index).collect();
    assert!(active.iter().all(|i| (10..20).contains(i)), "active slots bind visible items");
    // parked slots have display:none override set:
    for s in ls.slots.iter().filter(|s| s.parked) {
        assert!(stage.scene.get(s.node).unwrap().inline_set.contains(InlineBit::Display));
    }
    // binds queued:
    assert!(!ls.pending_binds.is_empty());
}

#[test]
fn execute_grows_by_cloning_when_no_parked_slot() {
    let mut stage = build_test_list_stage(ul, 1000);
    stage.enter_data_driven(ul);
    let before = stage.scene.lists.get(ul).unwrap().slots.len();
    // jump scroll far — visible needs more slots than parked pool has
    stage.set_scroll(ul, scroll_for_item_500);
    let ops = stage.plan_visible(ul); stage.execute_visible(ops);
    let after = stage.scene.lists.get(ul).unwrap().slots.len();
    assert!(after > before, "grew by cloning");
    assert!(after >= before, "never shrinks (no eviction)");
}
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Rewrite execute_one**

```rust
// For each item in to_bind:
for item_index in &op.to_bind {
    // 1) prefer a parked slot already bound to this item_index:
    let slot = ls.slots.iter_mut().find(|s| s.parked && s.item_index == *item_index)
        .or_else(|| ls.slots.iter_mut().find(|s| s.parked));  // 2) else any parked slot
    let node = match slot {
        Some(s) => { s.parked = false; s.item_index = *item_index; unset_inline_override(scene, s.node, "display"); s.node }
        None => {
            // 3) grow: clone template, attach, ordinal = slots.len()
            let n = clone_node_recursive(scene, template_root);
            insert_before(scene, ul, n, tail_spacer);
            set_reuse_key(scene, n, encode_reuse_key(list_ordinal, ls.slots.len()));  // Task 4 makes ordinal permanent
            ls.slots.push(Slot { node: n, item_index: *item_index, parked: false });
            n
        }
    };
    ls.pending_binds.push((node, *item_index));
}
// For slots still marked parked (off-range): ensure display:none override applied
for s in ls.slots.iter().filter(|s| s.parked) {
    set_inline_override(scene, s.node, "display:none");
}
```
(Exact iteration order / borrow-splitting per source; the structure above is the contract.)

- [ ] **Step 4: Run, expect PASS.** Commit: `refactor(core): execute_visible unpark/bind/park/grow`

---

## Task 4: reuse_key permanent ordinal

**Files:**
- Modify: `crates/core/src/list.rs` (`encode_reuse_key` ~999 + its caller in execute/grow)
- Test: `crates/core/src/list.rs`

**Interfaces:**
- Produces: `encode_reuse_key(list_ordinal, ordinal)` where `ordinal` is the permanent slot index assigned once at clone-time (Task 3 grow path already passes `ls.slots.len()` at creation — which IS the permanent ordinal since slots never shrink). Verify no other caller rotates the key.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn reuse_key_stable_across_scroll_frames() {
    let mut stage = build_test_list_stage(ul, 1000);
    stage.enter_data_driven(ul);
    stage.set_scroll(ul, scroll_for_item_10);
    stage.tick();  // plan+execute+solve
    let key_of_slot0 = scene.get(ls.slots[0].node).unwrap().reuse_key;
    stage.set_scroll(ul, scroll_for_item_500);  // slot0 likely parks/rebinds
    stage.tick(); stage.tick();  // settle
    // slot[0] is the same NodeId (slots vec never shrinks/reorders by removal):
    assert_eq!(scene.get(ls.slots[0].node).unwrap().reuse_key, key_of_slot0,
        "reuse_key permanent — never rotated");
}
```

- [ ] **Step 2: Run, expect FAIL** (current code re-sets reuse_key on every clone-cycle, rotating).

- [ ] **Step 3: Ensure reuse_key set exactly once per slot**

In `execute_one` grow path: `set_reuse_key(scene, n, encode_reuse_key(list_ordinal, ls.slots.len()))` is called at clone — KEEP. Grep for ALL other `set_reuse_key` callers in list.rs and DELETE any that re-key existing slots on recycle/rebind (the old `execute_one` re-keyed on free-pool pop — that path is gone, but verify). `encode_reuse_key` body unchanged (`((ordinal+1)<<16) | (idx & 0xFFFF)`); the fix is call-site discipline (key once at birth).

- [ ] **Step 4: Run, expect PASS.** Commit: `fix(core): reuse_key permanent per-slot ordinal (坑182 子因②)`

---

## Task 5: collect_heights skip parked

**Files:**
- Modify: `crates/core/src/list.rs` (`collect_heights` ~724-810)
- Test: `crates/core/src/list.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn collect_heights_skips_parked_slots() {
    // list with 2 active slots (known heights) + 3 parked slots (display:none, zero layout_rect.h)
    let mut stage = ...;
    stage.collect_heights(ul);
    let ls = stage.scene.lists.get(ul).unwrap();
    // parked slots must not overwrite known heights with 0:
    for s in ls.slots.iter().filter(|s| s.parked) {
        assert!(ls.heights.known.get(s.item_index).map_or(true, |h| *h > 0.0),
            "parked slot did not corrupt height cache with zero");
    }
}
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Add parked skip in collect_heights loop**

```rust
for s in ls.slots.iter() {
    if s.parked { continue; }  // display:none → zero layout_rect.h, not a real measurement
    let h = scene.get(s.node).unwrap().layout_rect.h;
    ls.heights.set_known(s.item_index, h);
}
```

- [ ] **Step 4: Run, expect PASS.** Commit: `fix(core): collect_heights skips parked slots`

---

## Task 6: notify_inserted/removed/moved — park/shift, no detach

**Files:**
- Modify: `crates/core/src/list.rs` (`notify_inserted`/`notify_removed`/`notify_moved` ~492-712, esp. Phase C recycle arms)
- Test: `crates/core/src/list.rs`

**Interfaces:**
- Produces: item insert/remove/move NEVER detach a slot. Removed-item slots park; indices shift (`>end -= count`, etc.). Newly-visible items reuse parked slots via the next plan/execute cycle.

> **Note:** This is the most algorithmically intricate task. The behavior is fully defined by the tests below; the implementation reuses the existing shift math but replaces every `remove_child`/`ls.free` with `s.parked = true` + `set_inline_override(display:none)`.

- [ ] **Step 1: Write failing tests (behavior battery)**

```rust
#[test]
fn notify_removed_parks_not_detaches() {
    // slots bound to items [5,6,7]; remove items [6,7]
    stage.notify_removed(ul, 6..8);
    let ls = scene.lists.get(ul).unwrap();
    for s in &ls.slots {
        assert_eq!(scene.get(s.node).unwrap().parent, Some(ul), "no detach on remove");
    }
    assert!(ls.slots.iter().any(|s| s.parked), "removed-item slots parked");
    // items >7 shifted down by 2:
    assert!(ls.slots.iter().all(|s| s.item_index < 6 || s.parked));  // shifted or parked
}

#[test]
fn notify_inserted_shifts_indices_no_detach() {
    // insert 2 items at index 5; slot bound to old-item-5 should shift to 7 (or park if now off-range)
    stage.notify_inserted(ul, 5, 2);
    let ls = scene.lists.get(ul).unwrap();
    for s in &ls.slots { assert_eq!(scene.get(s.node).unwrap().parent, Some(ul)); }
}
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Rework notify_* recycle arms**

For each of `notify_removed`/`notify_inserted`/`notify_moved`: keep the existing item_index/height shift math; replace the Phase C "recycle = remove_child + ls.free.extend" with:
```rust
for s in ls.slots.iter_mut() /* where s in removed/affected range */ {
    s.parked = true;
    set_inline_override(scene, s.node, "display:none");
}
```
Delete every `remove_child(scene, ul, node)` and `ls.free` reference in notify_*. Newly-needed slots (from insert making more items visible) are NOT created here — the next `plan_visible`+`execute_visible` cycle reuses parked slots or grows.

- [ ] **Step 4: Run, expect PASS.** Commit: `refactor(core): notify_* park/shift, no detach`

---

## Task 7: Test consolidation — rewrite free-pool tests, add parked suite, insurance tests

**Files:**
- Modify: `crates/core/src/list.rs` (`mod tests`)
- Test: same file

**Interfaces:**
- Produces: all list.rs tests green; `assert_all_slots_well_parented` invariant relaxed; parked behavior suite; taffy/insert_before/tick-timing insurance tests.

- [ ] **Step 1: Rewrite the ~7 free-pool tests**

For each test lively-moose identified (`update_visible_recycles_slots_across_frames`, `notify_removed_drains_range_and_recycles_slots`, `notify_inserted_shifts_heights_and_slot_indices`, `notify_moved_remaps_height_and_slot_index`, `collect_heights_writes_slot_layout_height`, `collect_heights_uses_margin_box_not_border_box`, `update_visible_instantiates_initial_slots`): replace `ls.free.len()` assertions with parked-flag assertions; replace children-count assertions with `slots.len()` (high-water). Remove any `#[ignore]` added in Task 1.

- [ ] **Step 2: Relax `assert_all_slots_well_parented`**

Change "children strict-ascending" to "head_spacer is first child, tail_spacer is last child, slot nodes anywhere between" — parked slots may sit in any position.

- [ ] **Step 3: Add insurance tests**

```rust
#[test]
fn taffy_display_none_excludes_parked_slot_from_flow() {
    // ul with 3 active slots + 1 parked (display:none); active slots flow contiguously,
    // parked contributes zero size, no gap.
    let (ul_rect, active_rects) = measure(...);
    assert!(active_rects.iter().all(|r| r.h > 0.0));
    assert_eq!(count_gaps(active_rects), 0, "no gap where parked slot sits");
}

#[test]
fn insert_before_keeps_spacer_ordering_with_parked_slots() {
    // after multiple park/unpark cycles, head_spacer remains child[0], tail_spacer last
    ...
    assert_eq!(children[0], head_spacer);
    assert_eq!(children.last(), tail_spacer);
}

#[test]
fn tick_order_one_solve_per_frame_with_parking() {
    // assert solve called exactly once per tick; parked slots don't trigger extra solves
    ...
}
```

- [ ] **Step 4: Run full core suite, expect ALL PASS.** `cargo test -p loomgui_core && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Commit: `test(core): rewrite free-pool tests + parked suite + insurance`

---

## Task 8: build_blob — append parked keepalive entries (Rust)

**Files:**
- Modify: `crates/ffi/src/blob.rs` (`build_blob`, after the existing render_nodes loop ~per spec §3.3)
- Test: `crates/ffi/tests/` (blob round-trip)

**Interfaces:**
- Consumes: Task 1 `Slot.parked` + `ListState.slots`; existing `node.reuse_key`.
- Produces: blob gets one extra entry per parked slot: `{node_id, reuse_key, visible=0b10, payload_kind=0, mesh_off=0, mesh_len=0, change_level=0}`, other columns zero. `node_count` = render_nodes.len() + parked_count.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn blob_emits_parked_keepalive_entries() {
    // scene with 2 active render nodes + 1 list having 3 parked slots
    let blob = build_blob(&scene);
    assert_eq!(blob.node_count, 2 + 3);
    // find the 3 parked entries (visible bit1 set, bit0 clear):
    let parked: Vec<_> = (0..blob.node_count).filter(|&i| blob.parked(i)).collect();
    assert_eq!(parked.len(), 3);
    for i in &parked {
        assert!(!blob.visible(*i), "parked not visible");
        assert!(blob.reuse_key(*i) > 0);
        assert_eq!(blob.mesh_len(*i), 0);  // no mesh for parked
    }
}
```

- [ ] **Step 2: Run, expect FAIL** (no `parked` accessor / no keepalive entries).

- [ ] **Step 3: Append keepalive loop in build_blob**

After the existing `for rn in &render_nodes { ... }` loop that fills per-node columns, before computing `node_count`:
```rust
let mut parked_count = 0usize;
for ls in scene.lists.values() {
    for s in ls.slots.iter().filter(|s| s.parked) {
        let n = scene.get(s.node).ok_or("parked slot node missing")?;
        col_node_id.push(s.node.into());
        col_parent_id.push(-1);   // parked not in active tree relationship for render
        col_visible.push(0b10);   // bit1=parked, bit0=not visible
        col_alpha.push(0.0);
        col_sort_key.push(0); col_mask_context.push(0);
        col_ma.push(1.0); col_mb.push(0.0); col_mc.push(0.0); col_md.push(1.0);
        col_mtx.push(0.0); col_mty.push(0.0);
        col_payload_kind.push(0);  // no mesh
        col_mesh_off.push(0); col_mesh_len.push(0);
        col_path_idx.push(0); col_program.push(0);
        col_color_matrix.extend([0f32;20]); col_change_level.push(0);
        col_reuse_key.push(n.reuse_key);
        col_effect_block.extend([0f32;32]);
        parked_count += 1;
    }
}
let node_count = render_nodes.len() + parked_count;
```
(Exact column push names per current `build_blob`; the contract is: every column gets an entry, parked ones are minimal. The `visible` byte = `0b10` is the critical value.)

- [ ] **Step 4: Add Rust-side `parked`/`visible` read helpers if blob tests read them** (mirror FrameBlob accessors for round-trip tests). Run: `cargo test -p loomgui_ffi` — expect PASS.
Commit: `feat(ffi): blob parked keepalive entries (visible byte bit1)`

---

## Task 9: FrameBlob.cs — Visible mask + Parked accessor

**Files:**
- Modify: `unity/package/Runtime/FrameBlob.cs` (~line 65)
- Test: `unity/package/Tests/FrameBlobTests.cs` (extend) — runs in Unity EditMode or headless harness.

**Interfaces:**
- Produces: `Visible(int i) => (_buf[ColOff(2)+i] & 0x01) != 0`; `Parked(int i) => (_buf[ColOff(2)+i] & 0x02) != 0`. All existing `Visible` callers unchanged.

- [ ] **Step 1: Write failing test**

```csharp
[Test] public void ParkedBit_RoundTrips() {
    var blob = BuildTestBlob(active:2, parked:1);  // helper: 2 visible nodes + 1 parked keepalive
    int parked = Enumerable.Range(0, blob.NodeCount).Count(i => blob.Parked(i));
    Assert.That(parked, Is.EqualTo(1));
    Assert.That(blob.Visible(/*parked index*/), Is.False);
    Assert.That(Enumerable.Range(0,2).All(i => blob.Visible(i) && !blob.Parked(i)), Is.True);
}
```

- [ ] **Step 2: Run, expect FAIL** (`Parked` undefined; `Visible` reads full byte).

- [ ] **Step 3: Edit FrameBlob.cs**

```csharp
public bool Visible(int i) => (_buf[ColOff(2) + i] & 0x01) != 0;   // was: != 0
public bool Parked(int i)  => (_buf[ColOff(2) + i] & 0x02) != 0;   // new
```

- [ ] **Step 4: Run test, expect PASS.** Verify `MirrorPool.cs`/`UnityLoomBackend.cs` still compile (they call `Visible` — semantic preserved). Commit: `feat(unity): FrameBlob Visible mask + Parked accessor`

---

## Task 10: MirrorPool — parked branch, reactivate, lazy, DumpState active column

**Files:**
- Modify: `unity/package/Runtime/MirrorPool.cs` (`Sync` ~62-126, `DumpState`/`DumpDict`)
- Test: `unity/package/Tests/MirrorPoolTests.cs`

**Interfaces:**
- Consumes: Task 9 `blob.Parked(i)`.
- Produces: `Sync` handles parked (keep GO SetActive false), active (reactivate SetActive true), gone (destroy stale — unchanged). Lazy: parked with no existing GO → skip creation. `DumpState` prints `active={ro.Go.activeSelf}`.

- [ ] **Step 1: Write failing tests**

```csharp
[Test] public void ParkedKeepalive_KeepsGo_Inactive() {
    // blob: 1 active slot (creates GO), then next blob same slot parked
    pool.Sync(blobWithActive, root, mm, sprites, fallback);
    var go = pool._poolByReuse[key].Go;
    pool.Sync(blobWithSameSlotParked, ...);
    Assert.IsTrue(go.activeInHierarchy == false || !go.activeSelf);  // kept + inactive
    Assert.AreEqual(1, pool.Count);  // not destroyed
}

[Test] public void Reactivate_SetsActive_ReuploadsFull() {
    // parked -> active transition; blob marks change_level=Full on reactivate
    pool.Sync(blobParked, ...); pool.Sync(blobReactivatedFull, ...);
    Assert.IsTrue(pool._poolByReuse[key].Go.activeSelf);
    // (mesh re-upload verified via Mesh vertex count change or a spy)
}

[Test] public void ParkedNoPriorGo_DoesNotCreate() {
    pool.Sync(blobWithOnlyParkedKeepalive, ...);  // never-active slot
    Assert.AreEqual(0, pool.Count, "lazy — no GO pre-created for parked");
}

[Test] public void SteadyState_ZeroChurn() {
    pool.Sync(blobSteadyActive, ...); pool.Sync(blobSteadyActive, ...);
    // assert no NewRenderObj / no TearDown between frames (spy counters)
}
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Rework Sync loop + DumpState**

Insert parked branch at loop top (before `if (!visible) continue`):
```csharp
bool parked = blob.Parked(i), visible = blob.Visible(i);
if (parked) {
    uint poolKey = blob.ReuseKey(i);
    if (_poolByReuse.TryGetValue(poolKey, out var ro)) {
        ro.Stale = false;
        if (ro.Go.activeSelf) ro.Go.SetActive(false);
    }
    continue;   // lazy: no prior GO -> skip, don't create
}
if (!visible) continue;
// active path (existing) — add after ensuring ro:
if (!ro.Go.activeSelf) ro.Go.SetActive(true);   // reactivate
```
In `DumpDict`, append `active={ro.Go.activeSelf}` to the format string.

- [ ] **Step 4: Run tests, expect PASS.** Commit: `feat(unity): MirrorPool parked branch + reactivate + lazy + dump active`

---

## Task 11: L1 scope — find_node_by_id_in_subtree (core + FFI + Nodes.cs)

**Files:**
- Create: `crates/core/src/scene/node.rs` (`find_node_by_id_in_subtree`), `crates/core/src/stage.rs` (passthrough)
- Create: FFI in `crates/ffi_c/src/` (`loomgui_stage_find_node_by_id_in_subtree`)
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs` (`TryGet`/`Get`)
- Test: core unit test + `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Produces: `Scene::find_node_by_id_in_subtree(root, id) -> Option<NodeId>` (DFS from root through children); FFI `loomgui_stage_find_node_by_id_in_subtree(stage, root, id_ptr, id_len) -> u32`; C# `TryGet` calls it with `_id`, drops `IsInSubtree` post-filter.

- [ ] **Step 1: Write failing core test**

```rust
#[test]
fn find_in_subtree_hits_own_not_others() {
    // scene: slot_a (id="badge") and slot_b (id="badge") both contain a child id="badge"
    let badge_a = scene.find_node_by_id_in_subtree(slot_a, "badge").unwrap();
    let badge_b = scene.find_node_by_id_in_subtree(slot_b, "badge").unwrap();
    assert_ne!(badge_a, badge_b, "each slot finds its own");
    assert_eq!(scene.find_node_by_id_in_subtree(slot_a, "nonexistent"), None);
    // root itself:
    assert_eq!(scene.find_node_by_id_in_subtree(slot_a, "slot_a_id"), Some(slot_a));
}
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Implement core subtree find**

In `scene/node.rs`:
```rust
/// DFS from `root` through its children; returns first node (root-inclusive)
/// whose id_attr == id. Pure structural traversal — does not check display:none.
pub fn find_node_by_id_in_subtree(&self, root: NodeId, id: &str) -> Option<NodeId> {
    let mut stack = vec![root];
    while let Some(nid) = stack.pop() {
        let n = self.get(nid)?;
        if n.id_attr.as_deref() == Some(id) { return Some(nid); }
        stack.extend(n.children.iter().rev());
    }
    None
}
```
Add `stage.rs` passthrough `find_node_by_id_in_subtree(&self, root, id)`.

- [ ] **Step 4: Run core test, expect PASS.**

- [ ] **Step 5: Add FFI**

In the FFI surface (csbindgen-decorated), mirror existing `loomgui_stage_find_node_by_id` signature + `root: u32` param:
```rust
pub extern "C" fn loomgui_stage_find_node_by_id_in_subtree(
    stage: StageHandle, root: u32, id: *const u8, id_len: usize,
) -> u32 { /* call stage.find_node_by_id_in_subtree(NodeId(root), str) -> NodeId or RootSentinel */ }
```
Rebuild: `cargo build -p loomgui_ffi_c --release && cargo run -p xtask -- sync-bindings`.

- [ ] **Step 6: Rewire C# TryGet/Get**

In `LoomGUI.Nodes.cs` `TryGet`:
```csharp
fixed (byte* p = idb)
    candidate = Native.loomgui_stage_find_node_by_id_in_subtree(h, _id, p, (nuint)idb.Length);
if (candidate == RootSentinel) return false;
// IsInSubtree post-filter now redundant; keep as Debug.Assert only:
System.Diagnostics.Debug.Assert(IsInSubtree(h, candidate));
```
(`Get<T>` delegates to `TryGet` — unchanged.)

- [ ] **Step 7: Write failing headless test, then verify pass**

```csharp
[Test] public void Get_OnSlot_HitsOwnId_NotOtherSlot() {
    // list with N slots each having internal id="badge"
    var slot3 = list.GetSlot(3);   // or via Query/BindItem captured ref
    var badge = slot3.Get<Label>("badge");
    Assert.AreNotEqual(list.GetSlot(0).Get<Label>("badge").Id, badge.Id);
}
```
Run headless: `dotnet test tests/dotnet/LoomGUI.HeadlessTests`. Commit: `feat(scope): find_node_by_id_in_subtree (L1) fixes Get<T> on list slots`

---

## Task 12: .dll closure + PublicApi gate

**Files:** none (build/copy/verify)

- [ ] **Step 1: Recompile + sync + copy**

Run (Unity closed):
```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
- [ ] **Step 2: Verify md5 synced + PublicApi gate + full suites**

```bash
cargo test                                          # whole workspace
dotnet test tests/dotnet/LoomGUI.PublicApi          # compile gate green (constraint b)
dotnet test tests/dotnet/LoomGUI.HeadlessTests      # includes L1 scope + reactivate-Full assertion
```
Expected: all green. Commit (dll + bindings): `build: recompile dll + sync bindings for pooled slot lifecycle`

---

## Task 13: Doc sync — scope notes + 坑182 status

**Files:**
- Modify: `docs/design/fence.md`, `docs/design/public-api.md` (scope chapter), `docs/pitfalls.md` (坑182)

- [ ] **Step 1: Add scope notes to fence.md / public-api.md**

In the scope/id chapter, document:
1. Virtualized `<ul>` MUST NOT use `:nth-child` (parked slots inflate child count); use item-index/data-attr styling.
2. `Get<T>("id")` on a list slot searches that slot's subtree (L1); `component.Get<T>("id")` descends into list items (L1 residual — full scope-boundary isolation is L3, roadmap §4).

- [ ] **Step 2: Update pitfalls.md 坑182**

Change status to RESOLVED (implementation): root cause (reuse_key rotation + GO stale-destroy) → fix (parked-but-attached + permanent ordinal + persistent MirrorPool GO pool + L1 subtree find). Reference the design spec + this plan.

- [ ] **Step 3: Commit** `docs: pooled slot lifecycle scope notes + 坑182 resolved`

---

## Self-Review (run after writing)

**1. Spec coverage:** §2 core (T1-T7) ✓, §3 blob (T8-T9) ✓, §4 MirrorPool (T10) ✓, §5 L1 (T11) ✓, §6 tests (folded into each T + T7/T9/T10/T11) ✓, §7 migration/docs (T12-T13) ✓. 坑182 root causes all addressed (reuse_key T4, detach T2/T3/T6, stale-destroy T10). Deferred L2/L3 already in roadmap (cross-ref in spec §8). 
**2. Placeholder scan:** Implementation steps give signatures + key logic; notify_* (T6) notes behavior is test-defined (legitimate TDD). No TBD/TODO. 
**3. Type consistency:** `Slot { node, item_index, parked }` consistent across T1-T7. `encode_reuse_key(list_ordinal, ordinal)` signature consistent (T4). `find_node_by_id_in_subtree(root, id)` consistent (T11). `Parked`/`Visible` accessors consistent (T9-T10).
