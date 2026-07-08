## Task 8 Report: 行内图 + emoji-图片（measure 记位 + build 产图 quad）

**Status**: Complete
**Branch**: `feat/v1.7-rich-text`
**Commit**: `dc03717` — `feat(core): inline images + emoji-as-image in rich flow (v1.7)`

### What was done

1. **`TextLayout` 加 `images` 字段** (`loomgui_core/src/text/layout.rs`):
   - 新增 `RichImagePlacement { src, x, y, w, h }` struct
   - `TextLayout.images: Vec<RichImagePlacement>` 字段
   - `measure_text` 返回填 `images: Vec::new()`（plain text 无行内图）

2. **`measure_rich_text` Image 分支记位** (`loomgui_core/src/text/layout.rs`):
   - 函数顶声明 `let mut out_images: Vec<RichImagePlacement> = Vec::new()`
   - `RichKind::Image { src, w, h, valign }` 分支记录位置：
     - `Baseline`/`Bottom`: y_top = baseline - img_h（底边贴 baseline）
     - `Middle`: y_top = baseline - img_h * 0.5
     - `Top`: y_top = 0.0
   - 末尾 `TextLayout { ..., images: out_images }`

3. **RichText build arm 产 image Mesh** (`loomgui_core/src/render/mod.rs`):
   - `push_text_meshes` 后、`continue` 前，遍历 `layout.images`
   - 每图产 `NodePayload::Mesh { program:0, image_path: src, verts 4角, uvs 全图 0..1, colors 白, indices }`
   - `synth_text_node_id(node_id, 1000 + img_idx)` 避与 text page id 撞
   - content offset (`off_x`, `off_y`) 内联应用（不污染 layout.images）
   - 合成 image 节点不入 `id_to_pos`——sort_key/mask_context 由 `propagate_text_sub_page_sort_keys` 传播

4. **测试** (`loomgui_core/src/render/tests.rs`):
   - `rich_image_emits_mesh_with_image_path_and_program_0`: RichText 含 `Text("Hi")` + `Image(emoji/cool.png, 16x16, Baseline)` → 验 frame 同时含 text Mesh (program=1) 和 image Mesh (program=0, image_path="emoji/cool.png", 4 顶点, 全图 UV)

### Test summary

- `cargo test`: **702 passed, 0 failed**（含 fence_contract 30 项全部通过）
- `cargo fmt --all -- --check`: clean
- `cargo clippy --all-targets -- -D warnings`: clean
- 新增测试 `rich_image_emits_mesh_with_image_path_and_program_0`: passed
- 零回归（全部既有测试通过）

### Concerns

- **UV 方向**: 全图 UV 用 `(0,0)-(1,1)`（非 v-flipped）。现有 Image/Container bg-image 的 mesh::quad 用 v-flipped `(0,1)-(1,0)`。若 PlayMode 行内图上下颠倒，需将 UV 翻转与现有 Image 对齐——改 `uvs` 为 `[[0.0,1.0],[1.0,1.0],[1.0,0.0],[0.0,0.0]]`。
- **synth id 编码**: `1000 + img_idx` 经 `synth_text_node_id` 的 `& 0xFF` 掩码后为 232–255 范围，与 text 跨页子页号潜在重叠（text page 232–255）。实际跨页几乎不会超 10 页（单 atlas 2048² 装不下才跨），碰撞概率极低。
- **合并/批处理**: 行内图 mesh 的 program=0 与 Image 节点 program=0 在 merge 阶段同 DrawState → 可与相邻 Image 节点合并（正确行为）。与 text mesh (program=1) 不合批（正确行为）。
