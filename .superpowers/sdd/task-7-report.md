## Task 7 Report: alpha 归 header_hash

**Status**: done
**Commit**: `d76474c`
**Test summary**: 495 passed (baseline 493 + 2 new), 0 failed -- `header_hash_alpha_change` + `payload_hash_ignores_alpha` both pass after adding alpha to header_hash.

### Changes

- `loomgui_core/src/render/dirty.rs`:
  - `header_hash()`: added `rn.alpha.to_le_bytes().hash(&mut h);` after `visible` hash
  - Updated doc comment to list alpha among header fields, noting C# does SetPropertyBlock(_Alpha)
  - `payload_hash()`: no change (confirmed: core-layer `colors` never baked node alpha -- they come from `background_color`/tint, not multiplied by `rn.alpha`)

### Tests added

1. `header_hash_alpha_change` -- alpha 1.0 vs 0.5 produce different header_hash
2. `payload_hash_ignores_alpha` -- alpha 1.0 vs 0.5 produce same payload_hash (geometry unchanged)

### Test commands

```bash
# Baseline (before change)
cargo test -p loomgui_core --features parse
# → 493 passed

# TDD Step 2: confirm header_hash_alpha_change FAILS (alpha not in header_hash)
cargo test -p loomgui_core --features parse -- header_hash_alpha_change
# → FAILED (same hash for both: 3490146233768110007)

# TDD Step 4: confirm both pass after adding alpha to header_hash
cargo test -p loomgui_core --features parse -- header_hash_alpha_change payload_hash_ignores_alpha
# → 2 passed

# Full suite final
cargo test -p loomgui_core --features parse
# → 495 passed (480 lib + 10 fence + 3 snapshot + 2 dirty = 495)
```

### Self-review

- The change is one line in `header_hash` + 2 tests. Minimal and correct.
- `payload_hash` was confirmed to never hash alpha -- it hashes `colors` from the core-layer Mesh payload, which are constructed from `background_color`/tint, not multiplied by `rn.alpha`. The existing test (`payload_hash_ignores_alpha`) locks this.
- Impact: opacity tween now produces `ChangeLevel::Header`, meaning C# (T8) will do `SetPropertyBlock(_Alpha, alpha)` instead of a full `UploadMesh`. That is the payoff of the alpha strip started in T6.

### Concerns

None.
