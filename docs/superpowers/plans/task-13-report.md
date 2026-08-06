# Task 13 Report — Doc Sync (Pooled Slot Lifecycle)

**Status**: DONE  
**Commit**: `1c054c0` on `pool-slot-lifecycle`  
**Summary**: 6 files edited, 26 insertions / 12 deletions. All gates green (cargo build, fmt, clippy).

---

## Edit 1 — fence.md + public-api.md: L1 residual notes

### fence.md — §5 CSS 围栏, `:nth-child` paragraph
Added a blockquote warning that virtualized `<ul>` (`role=list`) MUST NOT use `:nth-child` because parked slots stay attached in the ul (display:none), and CSS counts them as children. Authors should use `data-*` attribute selectors instead. References spec §2.9 and §5.4.

### public-api.md — §3 Node invariants, under `Get<T>` bullet
Added two sub-bullets:
- **L1 已知残留**: Explains `Get<T>("id")` on a list slot searches that slot's subtree correctly (L1 self-exclusive DFS), but `component.Get<T>("id")` descends into list items (L1 residual — full IsScopeRoot isolation is L3). Driver code should use `slot.Get`/`slot.Query`.
- **虚拟化 `<ul>` 禁止 `:nth-child`**: Same warning as fence.md.

---

## Edit 2 — pitfalls.md: 坑182 status → RESOLVED

Replaced the entire entry (previously "未解"). New entry:
- Title: `已解决 ✅`
- Root cause expanded to all 4 causes (reuse_key rotation, detach/free model, slots.len() key instability, MirrorPool stale-destroy)
- Solution: 4 bullet points covering parked-but-attached, permanent ordinal reuse_key, persistent MirrorPool GO pool, L1 subtree find
- Verification: steady-state scroll zero GO churn (Profiler), mail items no longer disappear
- References: spec + plan doc paths

---

## Edit 3 — Spec §4.3: Reactivate contract correction

**Old**: `parked→active 转换帧，change_level 必须 = Full` + `实现 plan 加断言：reactivate 帧 change_level ≠ Full 即 panic`

**New**: Reactivate sets GO `SetActive(true)` + UpdateHeader; Mesh re-upload only when content changes (Full). Parked keepalive entries don't touch `prev_node_hashes` (mesh_off=0/len=0), so unchanged-content reactivate can naturally stay Skip — this is **correct** (GO retained its mesh while parked, which is the whole point). Content changes trigger Full naturally via hash diff. No panic/assertion enforced.

Also updated §6.3 test table: old `reactivate 帧 change_level=Full 断言` → new `reactivate 后 Mesh 正确（内容变→Full，内容不变→Skip 且 GO 保留 mesh）`.

---

## Edit 4 — Spec §5.2: DFS self-exclusive correction

**Old code comment**: `DFS from root through children；命中 root 自身或任一后代的 id_attr 即返`
**New code comment**: `DFS from root's direct children（self-exclusive）；root 自身的 id_attr 不参与匹配` + `行为对齐 DOM querySelectorAll / Query<T> 惯例`

**Old descriptive text**: `DFS 只搜 root 子树`
**New descriptive text**: `self-exclusive DFS：从 root 的直接子开始遍历，root 自身的 id_attr 不参与匹配（与 DOM querySelectorAll / Query<T> 惯例一致）`

Also updated §6.3 test table: `命中 root 自身/后代` → `命中 root 的后代 / 外子树返 None（self-exclusive：root 自身不匹配）`.

---

## Edit 5 — make_test_pkg doc drift (lib.rs + LoomGUIBindings.cs)

### `crates/ffi/src/lib.rs` ~line 475
**Old**: `含单 Container 节点 id="badge"`
**New**: `含两个 Container 节点：根容器 + 子容器 id="badge"（2-node，配合 self-exclusive 子树查找）`

### `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` ~line 186
Same correction in the XML doc comment (C# bindings mirror the Rust doc).

---

## Self-Review Consistency Check

| Document pair | Claim | Consistent? |
|---|---|---|
| Spec §4.3 + §6.3 | "Skip reactivate is benign" vs "内容不变→Skip 且 GO 保留 mesh" | ✅ |
| Spec §5.2 + §6.3 | "self-exclusive" vs "root 自身不匹配" | ✅ |
| Spec §5.2 + public-api.md | "self-exclusive DFS" vs "L1 self-exclusive 子树 DFS" | ✅ |
| fence.md + public-api.md | Both warn `:nth-child` prohibited for virtualized `<ul>` | ✅ |
| 坑182 + spec | Fix items match design (parked-but-attached, permanent ordinal, persistent GO pool, L1 subtree find) | ✅ |
| lib.rs + Bindings.cs | Both say "两个 Container 节点" with matching rationale | ✅ |

No remaining contradictions between docs and shipped code identified.

---

## Fix Round 1

**Commit**: `a8334e5`  
**Trigger**: review found 2 doc-code drifts missed in original task 13.

### Fix 1 — FFI + Bindings: stale "root inclusive" comment

`loomgui_stage_find_node_by_id_in_subtree` was changed to **self-exclusive** in T11-fix (DFS starts from root's children; root's own id_attr never matched). But the FFI doc comment and csbindgen-generated C# binding still said "root inclusive".

- `crates/ffi/src/lib.rs` ~line 448: `root inclusive` → `self-exclusive：从 root 的直接子开始 DFS，root 自身 id_attr 不参与匹配，与 DOM querySelectorAll/Query<T> 一致`
- `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` ~line 174: same correction (hand-edited; matches what `cargo run -p xtask -- sync-bindings` would regenerate)

### Fix 2 — Spec §6.4: reactivate mesh contract clarification

§6.4 test row said "reactivate → SetActive(true) + mesh Full 重传" implying unconditional Full. §4.3 (corrected) says unchanged-content reactivate stays Skip (benign).

Fixed to: `reactivate → SetActive(true)；内容变→Full mesh 重传，内容不变→Skip（GO 保留 mesh，见 §4.3）`

### Gates

`cargo build -p loomgui_ffi_c` ✅ | `cargo fmt --all -- --check` ✅ | `cargo clippy --all-targets -- -D warnings` ✅
