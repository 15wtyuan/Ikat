# Task 5 Report: v1.4-b FFI 5 入口 + blob v9 reuse_key

## Status: PASS

All 54 ffi tests pass, workspace 488+69+19 = 576 tests pass. Zero regression.

## Commit

`57ce6ed` feat(ffi): 5 个列表 FFI 入口 + blob v9 reuse_key 列（v1.4-b T5）

## Changes

### 1. `loomgui_ffi_c/src/lib.rs` -- 5 个新 FFI 入口

在 `loomgui_stage_set_scroll_pos` 之后插入：

| # | 函数 | 签名 | 用途 |
|---|------|------|------|
| 1 | `loomgui_stage_set_content_size` | `(h: *mut StageHandle, node_id: u32, w: f32, height: f32)` | driver 注入滚动容器 content_size（虚拟列表） |
| 2 | `loomgui_stage_clear_content_size_override` | `(h: *mut StageHandle, node_id: u32)` | 清除 content_size override（列表销毁/退回普通滚动） |
| 3 | `loomgui_stage_get_scroll_pos` | `(h: *const StageHandle, node_id: u32, out_x: *mut f32, out_y: *mut f32)` | 读 scroll_pos（out 参数） |
| 4 | `loomgui_stage_get_node_layout_rect` | `(h: *const StageHandle, node_id: u32, out_x: *mut f32, out_y: *mut f32, out_w: *mut f32, out_h: *mut f32)` | 读节点 layout_rect（4 out 参数） |
| 5 | `loomgui_stage_set_reuse_key` | `(h: *mut StageHandle, node_id: u32, key: u32)` | 设渲染复用键（虚拟列表 slot） |

全部遵循坑 102 约束：null 句柄早返/no-op，out 指针 `if !out_x.is_null()` 守卫。get 类方法 null 句柄/无效 node → out 填 0。

### 2. `loomgui_ffi_c/src/blob.rs` -- VERSION 8→9 + reuse_key 列（第 22 列）

9 处改动：

1. **VERSION**: `8` → `9`，注释 "v9：加 reuse_key 列（第 22 列）"
2. **columns 数组**: 末尾加 `("reuse_key", 4)`（第 22 列）
3. **col_reuse_key Vec**: `let mut col_reuse_key = Vec::<u8>::new();`
4. **主循环 fill**: `col_reuse_key.extend_from_slice(&rn.reuse_key.to_le_bytes());`
5. **col_bufs**: 末尾加 `("reuse_key", &col_reuse_key)`
6. **num_col_offsets 注释**: `// 21` → `// 22`
7. **TestView 适配**: `col_off: [usize; 22]`，parse loop `0..22`，新 `reuse_key()` 方法
8. **全部 VERSION=8→9 断言** (7 tests: build_blob_has_magic_and_count, program_column_round_trips, blob_v4_header, world_matrix, pure_mesh_kind, color_matrix, change_level)
9. **blob_v4_header 硬编码偏移**: `12+21*4=96` → `12+22*4=100`，arena header 偏移全部 +4B (mesh_arena@100, text@108, clip@116, path@124)
10. **所有 RenderNode 字面量补 `reuse_key` 字段**: mesh_node, mesh_node_tinted, mesh_node_raw, text_node, merged, mk closure, color_matrix test -- 共 7 处加 `reuse_key: 0,`（T3 新增必需字段）
11. **test_view_parses byte 偏移**: `blob[100..104]` → `blob[104..108]`（mesh_arena_len 位置后移 4B）
12. **新增 test `blob_v9_round_trips_reuse_key`**: 构造 reuse_key=42 的 RenderNode → build_blob → 验 VERSION=9 + reuse_key round-trip

### 3. `loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll`

release build + cp.

## Test Results

```
$ cargo test -p loomgui_ffi_c
test result: ok. 54 passed; 0 failed (含新 v9 round-trip)

$ cargo test (workspace)
test result: ok. 488 (core) + 54 (ffi lib) + 15 (ffi unit) + 19 (pkg) = 576 passed; 0 failed
```

## .dll 导出确认（坑 100）

5 个新符号全部在 `target/release/loomgui_ffi_c.dll` 中（findstr 确认）：

```
loomgui_stage_set_content_size           ✓
loomgui_stage_clear_content_size_override ✓
loomgui_stage_get_scroll_pos             ✓
loomgui_stage_get_node_layout_rect       ✓
loomgui_stage_set_reuse_key              ✓
```

## LoomGUIBindings.cs

csbindgen 自动生成的 `loomgui_unity/Assets/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` 含 5 个 P/Invoke（csbindgen 用 `float*` 指针而非 `ref float`）：

```csharp
loomgui_stage_set_content_size(StageHandle* h, uint node_id, float w, float height)
loomgui_stage_clear_content_size_override(StageHandle* h, uint node_id)
loomgui_stage_get_scroll_pos(StageHandle* h, uint node_id, float* out_x, float* out_y)
loomgui_stage_get_node_layout_rect(StageHandle* h, uint node_id, float* out_x, float* out_y, float* out_w, float* out_h)
loomgui_stage_set_reuse_key(StageHandle* h, uint node_id, uint key)
```

## Concerns

1. **csbindgen 产 `float*` 而非 `ref float`** -- 版本差异。preview 假定 `ref float`，实际生成的是 `float*` 指针。C# wrapper 层用指针传参功能等价，但需确认调用侧用 `fixed` 或 `&` 取址。
2. **LoomGUIBindings.cs 在 Bindings/ 子目录** -- `.gitignore` 有 `**/LoomGUI*Bindings*.cs` 规则，导致 gitignored。未 commit 此文件。需确认 .gitignore 是否需加 `!**/Plugins/LoomGUI/Bindings/*.cs` 白名单例外。
3. **`set_content_size` 参数命名** -- 高度参数从 `h` 改为 `height` 以避免与 handle `h` 冲突（Rust E0415）。C ABI 不受影响（参数名不进符号表），但 C# wrapper 以 `height` 命名。
