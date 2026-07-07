# Task 5 Report: FFI 3 atlas extern ports (pull dirty pages / page R8 / clear dirty)

**Status:** DONE
**Commit:** `ef6fc9e` `feat(ffi): font atlas pull ports (dirty_pages / page / clear_dirty)`

## Stage Methods Added (`loomgui_core/src/stage.rs`, after `get_node_visible`)

```rust
pub fn font_atlas_dirty_pages(&self, out: &mut [u32]) -> usize
pub fn font_atlas_page(&self, page: u32, out_w: &mut u32, out_h: &mut u32, out: &mut [u8]) -> usize
pub fn font_atlas_clear_dirty(&mut self)
```

`font_atlas_page` uses double-call pattern: if `out.len() < needed`, returns `needed` without writing;
caller allocates and retries. Invalid page returns 0 (page_bytes now returns empty for out-of-bounds).

## FFI Externs Added (`loomgui_ffi_c/src/lib.rs`)

```rust
#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_dirty_pages(
    h: *const StageHandle, out: *mut u32, max: usize,
) -> usize

#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_page(
    h: *const StageHandle, page: u32,
    out_w: *mut u32, out_h: *mut u32, out_buf: *mut u8, buf_len: usize,
) -> usize

#[no_mangle]
pub extern "C" fn loomgui_stage_font_atlas_clear_dirty(h: *mut StageHandle)
```

## Reference Convention

Aligned with `loomgui_stage_get_node_layout_rect` (`lib.rs:645`):
- `*const StageHandle` for readers, `*mut StageHandle` for mutators (clear_dirty needs `&mut Stage`)
- Individual `*mut` out params with null checks before write
- No status code; null/invalid writes defaults (0) or no-op
- No `.unwrap()`/`.expect()` on FFI inputs (pit 102 no-panic)

## Additional Fix: `page_bytes` Bounds Safety

`GlyphAtlas::page_bytes` in `atlas.rs` used direct indexing `self.pages[page as usize]` which
panics on invalid page. Changed to `self.pages.get(page as usize)` returning `(&[], 0, 0)`
on out-of-bounds, ensuring FFI never panics on untrusted page input.

## Build & Verification

- `cargo build -p loomgui_ffi_c --release`: **SUCCESS** (15.6s)
- Symbol check (grep -a on binary): **3/3** symbols confirmed
- `.dll` destination: `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`
- `.dll` copy: **SUCCESS** (not locked)
- md5: **MATCH** `3935d0b9ec2c80f3b9c1d58361e2890a`
- `LoomGUIBindings.cs` new P/Invoke: **3** (`loomgui_unity_package/Plugins/LoomGUI/Bindings/`)

## Test Results

- `cargo test` workspace: **653 passed, 0 failed**
- `cargo fmt --all -- --check`: **CLEAN**
- `cargo clippy --all-targets -- -D warnings`: **CLEAN**

## Files Changed

- `loomgui_core/src/stage.rs` (+42 lines: 3 Stage methods)
- `loomgui_core/src/text/atlas.rs` (+1/-1: page_bytes bounds safety)
- `loomgui_ffi_c/src/lib.rs` (+79 lines: 3 extern ports)
- `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` (binary, rebuilt)
- `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` (regenerated, +6 lines)

## Self-Review

- Brief Step 1 (Stage methods): all 3 implemented matching brief specs.
- Brief Step 2 (extern ports): all 3 implemented, aligned with existing get_node_layout_rect convention.
- No panic paths: null handle, null out pointers, invalid page all handled gracefully.
- `page_bytes` fix corrects a T1 implementation gap (the brief's design assumed `(&[],0,0)` on
  invalid page, but T1 used direct indexing which panics).
- `font_atlas_page` FFI extern adds defensive `out_buf.is_null()` check after the double-call probe,
  catching caller errors the brief's draft omitted.
- csbindgen auto-generated C# bindings correctly map `usize` to `nuint`, `*mut StageHandle` for
  clear_dirty vs `*const StageHandle` for readers.

## Concerns

**None.** .dll was not locked (Unity closed). All 3 symbols present in the .dll. All tests and lints green.
