# LoomGUI 打包器 & FFI 层代码审查报告

> 审查范围：`loomgui_pkg/src/lib.rs`、`loomgui_pkg/src/main.rs`、`loomgui_ffi_c/src/lib.rs`、`loomgui_ffi_c/src/blob.rs`、`loomgui_ffi_c/src/abi_tests.rs`、`loomgui_ffi_c/src/tests.rs`

---

## 一、严重问题

### 🔴 1. `unwrap_or("")` 静默吞非 UTF-8 输入 → 语义错误，失去错误返回

**位置：** `loomgui_ffi_c/src/lib.rs:198`、`:225`、`:396`、`:505`、`:1028`、`:1055`、`:1146`、`:1167`、`:1206`、`:1225`

**代码片段：**
```rust
let name = std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }).unwrap_or("");
```

**问题：** 当 C# 侧传入非 UTF-8 字节时，`unwrap_or("")` 把非法输入**静默改为空字符串**继续执行，而非返回错误码（-1 或 sentinel）。以 `loomgui_stage_load_package` 为例：传非法 name 字节 → `name=""` → `Stage::load_package("", bytes)` 可能以空 key 写入 packages 字典，产生不可预测副作用。其他函数类似——`set_text` 传非法 UTF-8 会把节点内容设成空串，而非返回 -1。

**修复方向：** 统一改成：
```rust
let name = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }) {
    Ok(s) => s,
    Err(_) => return -1, // 或 return INVALID
};
```
注意 `loomgui_stage_load_package`（`:198`）已存在 `unwrap_or("")` 且函数签名返 `i32`，可直接 return -1。但 `loomgui_stage_instantiate`（`:225`）返 `u32`，需 return `INVALID`（`0xFFFF_FFFF`）。

**严重级别：🔴 严重**

---

### 🔴 2. CLAUDE.md 描述的 blob 格式（22 列 + text_arena）与实际代码（v10、20 列、无 text_arena）不一致

**相关文件：**
- `CLAUDE.md:91` — "当前 22 列...text_arena"
- `loomgui_ffi_c/src/blob.rs:11` — `const VERSION: u32 = 10; // v10：text 塌进 mesh_arena，删 text_off/text_len 列 + text_arena`
- `loomgui_ffi_c/src/blob.rs:18` — "v10：删 text_off/text_len 列（22→20 列）"

**问题：** CLAUDE.md 作为项目"权威契约"，其 FFI 契约段（§13.3）描述 blob 为 22 列 + 包含 text_arena，但代码 v10 已改为 20 列、text_arena 已删除（字形数据直接进 mesh_arena）。这是最危险的文档/代码不一致——AI 和新贡献者会以 CLAUDE.md 为准来理解 blob 格式，导致 C# 解析端或调试时出错。

CLAUD.md 还提到 "NodePayload 只剩 Mesh/Text"，但代码中 `NodePayload` 只剩 `Mesh` 一个变体（`blob.rs:100` 的 `match &rn.payload { NodePayload::Mesh { ... } => { ... } }`），Text 已不存在。

**修复方向：** 更新 CLAUDE.md §13.3 及相关段落：改 "22 列" 为 "20 列"，删除 text_arena 描述，更新 NodePayload 描述。

**严重级别：🔴 严重**

---

## 二、高优先级问题

### 🟠 3. `loomgui_stage_load_html` 在 FFI 层直接调 `node::build_scene` 而非 `Stage` 方法

**位置：** `loomgui_ffi_c/src/lib.rs:173`

**代码片段：**
```rust
sh.stage.scene = Some(loomgui_core::scene::node::build_scene(&tree, &styles));
```

**问题：** 直接调用 crate 内部模块 `node::build_scene`，跳过 `Stage` 的任何封装逻辑。如果 `Stage` 将来在 `build_scene` 前后加入初始化/验证步骤（如默认字体注册校验、场景初始化回调），本路径会遗漏。对比 `loomgui_stage_load_package` 路径走 `Stage::instantiate`（正确封装），load_html 路径是孤立的。

此外，`loomgui_stage_load_html:168-172` 在赋值 scene 前手动 `tweens.clear()` + `scroll.clear()` + `prev_node_hashes.clear()`，这些清理由 FFI 层负责而非 Stage 内部，违反封装原则——Stage 内部新增状态时需要记得改 FFI 层。

**修复方向：** 把清场逻辑和 `build_scene` 调用封装进 `Stage::load_html(html, css)` 方法中，FFI 层只做薄调用。

**严重级别：🟠 高**

---

### 🟠 4. `stage_new_with_dejavu` 测试辅助函数在两处重复定义

**位置：** `loomgui_ffi_c/src/abi_tests.rs:31-50`、`loomgui_ffi_c/src/tests.rs:6-25`

**代码片段：** 两份完全相同的 20 行函数，读字体文件 + 注册 DejaVu。

**问题：** 任何改动（如字体路径、注册参数）需同步两处，否则测试行为差异不易发现。两个模块都在 `#[cfg(test)]` 下，可以共用一份定义。

**修复方向：** 提取为公共 `test_util` 模块或至少放在一个模块中让另一个 `use` 引用。

**严重级别：🟠 高**

---

### 🟠 5. blob `build_blob` 20 个裸列变量——添加新列需改 5 处

**位置：** `loomgui_ffi_c/src/blob.rs:60-79`（变量声明）、`:81-161`（填充）、`:164-185`（col_bufs 注册）、`:18-42`（列名/字节注释）、`:44`（header_len 计算依赖 `num_col_offsets`）

**代码片段（列变量声明）：**
```rust
let mut col_node_id = Vec::<u8>::new();
let mut col_parent_id = Vec::<u8>::new();
let mut col_visible = Vec::<u8>::new();
let mut col_alpha = Vec::<u8>::new();
let mut col_sort_key = Vec::<u8>::new();
let mut col_mask = Vec::<u8>::new();
let mut col_ma = Vec::<u8>::new();
let mut col_mb = Vec::<u8>::new();
let mut col_mc = Vec::<u8>::new();
let mut col_md = Vec::<u8>::new();
let mut col_mtx = Vec::<u8>::new();
let mut col_mty = Vec::<u8>::new();
let mut col_kind = Vec::<u8>::new();
let mut col_mesh_off = Vec::<u8>::new();
let mut col_mesh_len = Vec::<u8>::new();
let mut col_path_idx = Vec::<u8>::new();
let mut col_program = Vec::<u8>::new();
let mut col_color_matrix = Vec::<u8>::new();
let mut col_change_level = Vec::<u8>::new();
let mut col_reuse_key = Vec::<u8>::new();
```

**问题：** 新增一列需修改：① `columns` 元数据表、② 变量声明、③ 填充循环、④ `col_bufs` 注册表、⑤ C# 解析端。任何一处遗漏 → 静默偏移错误（SOA 模式中列偏移计算依赖各列字节数总和，某一列 buf 长度不对会导致后续列位移）。

当前项目历史已验证此风险——v10 删除 text_off/text_len 两列时正是要精确同步 5 处。`header_len` 依赖 `num_col_offsets`（自动从 columns.len() 推导），但各列 buf 填充是否正确无编译期检查。

**修复方向：** 考虑用声明式 schema（`&[(name, fill_fn)]` 宏或 builder 模式）替代 20 个裸变量。短期可在 `col_bufs` 注册后加 `debug_assert_eq!(col_offsets.len(), columns.len())`（已有，但还可加各列预期字节数检查）。

**严重级别：🟠 高**

---

### 🟠 6. 约 20 个 FFI 函数缺少 ABI 层测试

**被测试覆盖的 FFI 函数（~36 个）：** version, stage_new, register_font, set_fallback_families, free, load_html, load_package, instantiate, tick, borrow_frame, dump_scene, set_input, borrow_events, borrow_controller_changed_events, get_controller, set_selected_index, get_selected_index, is_pointer_on_ui, set_node_disabled, node_parent, find_node_by_id, add_touch_monitor, cancel_click, set_key_input, set_scroll_pos, request_focus, focused_node, tween, create_root, create_node, append_child, insert_before, remove_child, remove_node, set_text, set_src, set_style

**未经 FFI 边界直接测试的函数（~20 个）：**

| 函数 | 行号 |
|------|------|
| `loomgui_stage_remove_touch_monitor` | :537 |
| `loomgui_stage_set_content_size` | :620 |
| `loomgui_stage_clear_content_size_override` | :636 |
| `loomgui_stage_get_scroll_pos` | :647 |
| `loomgui_stage_get_node_layout_rect` | :675 |
| `loomgui_stage_get_node_world_matrix` | :719 |
| `loomgui_stage_get_node_sort_key` | :770 |
| `loomgui_stage_get_node_visible` | :791 |
| `loomgui_stage_font_atlas_dirty_pages` | :813 |
| `loomgui_stage_font_atlas_page` | :829 |
| `loomgui_stage_font_atlas_clear_dirty` | :872 |
| `loomgui_stage_set_reuse_key` | :882 |
| `loomgui_stage_kill_tween` | :974 |
| `loomgui_stage_clear_anim` | :986 |
| `loomgui_stage_clear_anim_prop` | :996 |
| `loomgui_stage_set_rich_text` | :1156 |
| `loomgui_stage_rich_link_at` | :1178 |
| `loomgui_shutdown` | :931 |

这些函数多数是薄包装，但 **FFI 边界本身（null 检查、指针验证、错误码转换）需要测试**。例如 `get_node_world_matrix` 接受 6 个 `*mut f32` out 参数，个别为 null 时的行为未经测试。

**修复方向：** 优先为 NativeHost 查询通道（`get_node_world_matrix`/`sort_key`/`visible`，坑 127 路径）和虚拟列表 setter（`set_content_size`/`set_reuse_key`）补 FFI 边界测试，null 句柄 + 无效 NodeId + 正常值三轮。

**严重级别：🟠 高**

---

## 三、中优先级问题

### 🟡 7. CLI 参数解析：缺值时静默生成空串而非报错

**位置：** `loomgui_pkg/src/main.rs:29-30`、`:39`、`:43`

**代码片段：**
```rust
let v = args.get(i + 1).cloned().unwrap_or_default();
// --html 缺值 → "" → split(',') → [""] filter 后 → [] → "no .html files found"
res_root = Some(PathBuf::from(args.get(i + 1).cloned().unwrap_or_default()));
// --res-root 缺值 → PathBuf::from("") → 后续路径拼接异常
out_path = args.get(i + 1).cloned();
// -o 缺值 → None → fallback 到默认路径（相对安全）
```

**问题：**
- `--html` 缺值时，`unwrap_or_default()` 给空串，split+filter 得空 Vec → `html_files` 为空 → 报 "no .html files found"，用户看到不准确的错误信息。
- `--res-root` 缺值时，`PathBuf::from("")` 产生空路径 → 后续 `res_root.join(&entry.path)` 会产生畸形路径 → PNG IHDR 读取失败（w/h=0），但无明确报错。
- 当 `--html` 后跟另一个 flag（如 `--html --res-root foo`），会静默地把 `--res-root` 当作 html 列表中的一项。

**修复方向：** 手动解析多加 `if i + 1 >= args.len() || args[i + 1].starts_with('-')` 检查，缺值时报清晰的错误信息后 exit(2)。不改 clap 也完全可以——手动解析体量小（~40 行），只是缺边界值检查。

**严重级别：🟡 中**

---

### 🟡 8. `check_no_flex_props` 未覆盖全部 flex 属性——`flex-direction`/`flex-wrap` 在 block div 上静默通过

**位置：** `loomgui_pkg/src/lib.rs:287-303`

**代码片段：**
```rust
fn check_no_flex_props(s: &loomgui_core::style::resolved::ResolvedStyle) -> Result<(), String> {
    // 只检查 justify_content / align_items / gap
    if ts.justify_content.is_some() { ... }
    if ts.align_items.is_some() { ... }
    if ts.gap != default.gap { ... }
    Ok(())
}
```

**问题：** 围栏哲学要求 "block div 拒 flex 属性（写了报错，不静默吞——AI 可预测性）"（`:268` 注释）。但当前只拒绝 3 个属性，`flex-direction: row`、`flex-wrap: wrap` 等 flex 属性在 block div 上会被静默接受。虽然 block div 没有 flex 子节点（只有富文本 runs），flex-direction 改变不会导致 taffy 行为差异，但 AI 读了 HTML 后看到 `display:block; flex-direction:row` 会预测错误行为。

**修复方向：** 要么扩展检查覆盖所有围栏内 flex 属性，要么在注释中明确"只拒对布局有语义影响的属性"。围栏测试 `fence_contract.rs` 应锁住此行为。

**严重级别：🟡 中**

---

### 🟡 9. `desugar_block_divs` 获取 tree 和 styles 的所有权后返还——不必要的所有权转移

**位置：** `loomgui_pkg/src/lib.rs:246-248`

**代码片段：**
```rust
pub fn desugar_block_divs(
    mut tree: loomgui_core::parse::dom::ElementTree,
    styles: Vec<loomgui_core::style::resolved::ResolvedStyle>,
) -> Result<(loomgui_core::parse::dom::ElementTree, Vec<...>), String> {
```

**问题：** 函数接收 tree 和 styles 的所有权，只修改 `tree.nodes[i].rich_runs`，然后返回原值。调用处（`:350`）必须重新绑定返回值：
```rust
let (tree, styles) = desugar_block_divs(tree, styles)
    .map_err(|e| format!("desugar_block_divs {hf}: {e}"))?;
```
可改为 `&mut tree` + `&styles`（只读 styles），省去移动-返回。

**修复方向：** 改为 `(&mut ElementTree, &[ResolvedStyle]) -> Result<(), String>`。

**严重级别：🟡 中**

---

### 🟡 10. `scene_to_template` 依赖 slotmap `values()` 迭代序 = DFS 先序的不变量

**位置：** `loomgui_pkg/src/lib.rs:49-54`、注释 `:32-34`

**代码片段：**
```rust
let pos_of: std::collections::HashMap<NodeId, usize> = scene
    .nodes
    .values()
    .enumerate()
    .map(|(i, n)| (n.id, i))
    .collect();
```

**问题：** `parent_idx` 的计算依赖 `scene.nodes.values()` 按插入序返回（DFS 先序），保证父节点总在子节点前出现、`pos_of` 中父节点索引 < 子节点索引。这是 slotmap 的**文档行为**（无删除的 SlotMap 的 values() 按槽位序），但并非 compile-time 保证。如果 build_scene 逻辑改为非 DFS 序插入，或者 slotmap 实现变化，这里会静默产错。

注释中已详细说明了此依赖（`:32-36`），但深度藏在注释里。

**修复方向：** 在 `Scene::build` 的文档或 `scene_to_template` 入口加 `debug_assert` 验证每个节点的 parent 在 Vec 中排在它前面（当前测试 `scene_to_template_parent_idx_maps_to_position` 已间接验证）。

**严重级别：🟡 中**

---

### 🟡 11. 部分测试绕过 FFI 直接操作 Rust 内部状态

**位置（举例）：**
- `abi_tests.rs:872-889` — `set_wheel_input_round_trip` 直接构造 `Stage`，不经过 FFI
- `abi_tests.rs:1004-1051` — `ffi_set_scroll_pos_round_trip` 调用 FFI `set_scroll_pos` 后，通过 `handle.stage.scene` 直接读内部状态验证，而非读 FFI `get_scroll_pos` 的 out 参数
- `tests.rs:40-64` — `stage_tween_complete_event_via_ffi` 用 `(*h).stage.scene` 取 NodeId
- `tests.rs:170-205` — `controller_ffi_round_trip` 用 `scene.get_mut(...).data_controller = Some(...)` 直接写内部字段

**问题：** 测试的双重目的：验证 FFI 契约 + 验证核心逻辑。绕过 FFI 直接操作 Rust 内部状态，测的是核心逻辑而非 FFI 契约。特别是 controller 测试中 `data_controller` 字段的写入绕过了 FFI——如果将来 FFI 添加 `set_data_controller` 函数，这些测试不会发现新函数的 bug。

**修复方向：** 区分测试意图——核心逻辑测试（直接调 Stage API）和 FFI 契约测试（只通过 extern "C" 函数）。当前的"端到端"测试实际上混用了两个层次。

**严重级别：🟡 中**

---

## 四、低优先级问题

### 🟢 12. `_pkg_name` 参数未使用——包名不写入 pkg.bin header

**位置：** `loomgui_pkg/src/lib.rs:318`

**代码片段：**
```rust
pub fn pack(
    source_dir: &Path,
    _pkg_name: &str, // ← 前缀 _ 标记未使用
    ...
```

**问题：** 注释说 `pkg_name` "供 CLI 日志用；未来版本号/元数据可扩展"。当前 pkg.bin 头部没有版本/包名字段。如果将来需要根据包名做逻辑（如同名覆盖检查），需要改 pack 签名 + write_package 格式，涉及向后兼容。

**修复方向：** 若短期内不需要，保持 `_pkg_name` 即可。长期考虑在 pkg.bin header 中加包名+版本字段（类似 blob 有 MAGIC + VERSION）。

**严重级别：🟢 低**

---

### 🟢 13. `pointer_event_event_record_sizeof` 测试名拼写错误

**位置：** `loomgui_ffi_c/src/abi_tests.rs:243`

**代码片段：**
```rust
fn pointer_event_event_record_sizeof() {  // "point" 应为 "pointer"
```

**问题：** 测试名 `point` 缺了 `er`。不影响功能，但 grep 搜索 `pointer_event` 会漏掉此项。

**严重级别：🟢 低**

---

### 🟢 14. `dump_scene` 每次调用重新分配 CString

**位置：** `loomgui_ffi_c/src/lib.rs:285`

**代码片段：**
```rust
handle.dump_blob = CString::new(json).unwrap_or_else(|_| CString::new("[]").unwrap());
```

**问题：** 每次 dump 都重新创建 CString（分配+复制），旧值被 drop。dump 是调试用，频率低，性能无影响。但如果有人在循环中频繁调用（如每帧 dump 到文件），会产生不必要的分配抖动。

**修复方向：** 可保留现状（调试路径，非热路径），或用 `String` + `into_bytes` + 手动加 `\0` 避免每次分配。

**严重级别：🟢 低**

---

### 🟢 15. `loomgui_shutdown` 体为空——Font 泄漏伴随存在

**位置：** `loomgui_ffi_c/src/lib.rs:925-931`

**代码片段：**
```rust
#[no_mangle]
pub extern "C" fn loomgui_shutdown() {}
```

**问题：** 注释详细描述了字体 bytes 的 `Box::leak` 问题——每次 Stage 创建都会 leak 一份字体字节取 `'static` 切片（`:926`），shutdown 无法回收。这是有意为之的 trade-off（等域重载内存观测触发阈值再做字体缓存单例化），代码注释已充分说明。

**修复方向：** 无需立即修复，但应追踪——如果 Stage 被反复创建/销毁（如 Unity Domain Reload），泄漏会累积。

**严重级别：🟢 低**

---

### 🟢 16. `strip_style_and_link` 手写 HTML 序列化——`scraper` 本身能序列化

**位置：** `loomgui_pkg/src/lib.rs:202-233`

**代码片段：**
```rust
fn serialize_children(el: &scraper::ElementRef, out: &mut String) {
    for child in el.children() {
        match child.value() {
            scraper::node::Node::Text(t) => { out.push_str(&t.text); }
            scraper::node::Node::Element(e) => {
                if e.name() == "style" || e.name() == "link" { continue; }
                // ... 手写 open tag + attrs + children + close tag
            }
        }
    }
}
```

**问题：** scraper crate 的 `ElementRef` 提供 `inner_html()` / `html()` 方法可以直接序列化。手写的序列化（40 行）需要维护属性转义（当前 `v` 直接拼，不含 `"` 转义）、自闭和标签处理等边缘情况。好处是跳过 style/link 元素无需先序列化再正则 strip。

**修复方向：** 维持现状——手写序列化已通过测试（`strip_style_and_link_removes_style_and_link_elements`、`strip_style_and_link_preserves_img_src`），且避免了构造完整 HTML 字符串再正则删除的性能开销。

**严重级别：🟢 低（可接受的设计选择）**

---

## 五、正向发现

### ✅ FFI 错误处理整体良好

每个 `extern "C"` 函数开头都检查了 `h.is_null()`（句柄空 → 早返）。`loomgui_stage_tick`（`:237`）正确地 match None 场景后返回空 FrameData 而非 panic（坑 102 修复）。`loomgui_stage_tween`（`:953-960`）对非法 prop/ease 值使用 `try_from` 优雅降级。整体 FFI panic 风险低。

### ✅ blob 布局设计良好

SOA 列布局 + offset 表 + arena 分区的设计允许 C# 侧高效零拷贝读取（`Span<byte>` + `BinaryPrimitives`）。`ChangeLevel` 机制（Skip/Header/Full）有效减少 Full-change 节点的数据传输量。path table 的 interning（`:249-263`）去重合理。

### ✅ 打包器流程完整

`pack` 函数覆盖了完整的 HTML→场景→序列化流水线：CSS 提取 → style/link 剥离 → parse → resolve_styles → desugar_block_divs → build_scene → scene_to_template（manifest + controllers）→ PNG 尺寸读取 → write_package。各步骤衔接正确，错误传播处理到位。

### ✅ 测试体量充足

`abi_tests.rs`（1166 行）+ `tests.rs`（230 行）覆盖了主要 FFI 路径：tick→blob、输入→事件、Controller、动态树 API、拖拽/长按、键盘导航、tween 完成事件等。虽然部分函数未测，但核心路径覆盖率不错。

---

## 六、总结

| 级别 | 数量 | 关键条目 |
|------|------|----------|
| 🔴 严重 | 2 | `unwrap_or("")` 静默吞错、CLAUDE.md blob 描述过时 |
| 🟠 高 | 4 | load_html 绕过 Stage 封装、测试辅助重复、blob 列修改点多、20 函数缺 FFI 测 |
| 🟡 中 | 5 | CLI 缺参静默、flex 检查不全、不必要的所有权转移、slotmap 序依赖、测试绕过 FFI |
| 🟢 低 | 5 | `_pkg_name` 未用、测试名拼写、dump 重复分配、shutdown 空体、手写序列化 |

**建议修复次序：**
1. 修复 `unwrap_or("")` → 返回错误码（所有 `name: *const u8` 参数函数）
2. 更新 CLAUDE.md blob 描述为 v10 格式
3. 提取共享 `stage_new_with_dejavu` 到 `test_util` 模块
4. 为 NativeHost 查询通道 FFI 补测试
5. CLI 参数缺值检查
