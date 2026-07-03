# Task 5 Report: 支柱3 FFI -- blob change_level 列 + SKIP/HEADER 不写 arena

## Status: PASS

All 53 ffi tests pass, all 493 core tests pass. Zero regression.

## Commit

`35242f7` feat(ffi): 支柱3 blob v8 change_level 列 + SKIP/HEADER 不写 arena

## Changes

**File:** `loomgui_ffi_c/src/blob.rs`

1. **VERSION 7 -> 8** -- bump version constant.
2. **`change_level` column (21st column, u8)** -- added to `columns` array, `col_bufs`, `col_change_level` buffer; pushed `rn.change_level as u8` per node before the match.
3. **Arena write gating** -- `let write_arena = matches!(rn.change_level, ChangeLevel::Full)`. Mesh/Text arms wrap arena writes in `if write_arena`; SKIP/HEADER set `col_mesh_off/len` (or `col_text_off/len`) to 0 without extending arena. Payload-kind, path_idx, program, color_matrix, and the public header columns are always written regardless of level.
4. **Deleted `NodePayload::Unchanged` match arm** -- T4 removed the variant from core; now removed from blob builder.
5. **TestView updated** -- `col_off: [usize; 21]`, parse loop `0..21`, new methods `change_level(i)` and `mesh_len_col(i)`.
6. **All RenderNode test helpers** (mesh_node, mesh_node_with_path, mesh_node_with_program, mesh_node_tinted, mesh_node_raw, text_node) -- added `change_level: ChangeLevel::Full`.
7. **Deleted `unchanged_node` helper** -- Unchanged variant no longer exists.
8. **Updated all test assertions** -- version 7->8 assertions, hardcoded byte offsets (12+20*4=92 -> 12+21*4=96), replaced `unchanged_node` usage in `program_column_round_trips`, rewrote `blob_unchanged_kind_is_zero` as `blob_pure_mesh_kind_is_one`.
9. **New test `change_level_column_round_trips`** -- TDD: verifies Skip=0/Header=1/Full=2 round-trip and arena gating (SKIP/HEADER mesh_len=0, FULL mesh_len>0).

## Test Commands + Output

```
$ cargo test -p loomgui_ffi_c
test result: ok. 53 passed; 0 failed; 0 ignored

$ cargo test -p loomgui_core --features parse
test result: ok. 478 unit + 10 doc + 3 integration + 2 snapshot = 493 total
```

## TDD Evidence

1. **Step 1 (write failing test)**: Added `change_level_column_round_trips` test + TestView helpers + col_off[21] before implementing blob builder changes.
2. **Step 2 (confirm fail)**: `cargo build -p loomgui_ffi_c` failed -- `NodePayload::Unchanged` variant not found (E0599), `change_level` field missing in RenderNode constructors, VERSION still 7.
3. **Step 3-4 (implement + fix all helpers)**: VERSION 8, change_level column, arena gating, delete Unchanged arm, fix all 6 test helpers, update all version/hardcoded-offset assertions.
4. **Step 5 (confirm pass)**: All 53 ffi tests pass + 493 core regression pass.
5. **Step 6 (commit)**: Single commit pushed.

## Self-Review

- All 53 ffi tests pass; the TDD test `change_level_column_round_trips` verifies both column values and arena gating.
- All 493 core tests pass; no regression from the T4 changes.
- header_len auto-updates via `columns.len()`; TestView col_off/parse loop updated to 21.
- All hardcoded byte offsets in header tests shifted by +4 (12+20*4=92 -> 12+21*4=96).
- Import includes `ChangeLevel`; `matches!(rn.change_level, ChangeLevel::Full)` is the arena gate.

## Concerns

1. **C# `FrameBlob.cs` expects v7** -- planned as T8. After this commit, the C# side will fail to parse the blob (version mismatch). No impact on Rust tests.
2. **No separate `size_of` assertion needed** -- the change_level column is u8 (1 byte per node), part of the SOA column layout, not a `#[repr(C)]` struct crossing FFI. The blob format is self-describing via column offsets.
3. **`program_column_round_trips` semantic change** -- the middle node changed from Unchanged (kind=0, program=0) to mesh_node (kind=1, program=0). The test still verifies program column round-trip correctly.
