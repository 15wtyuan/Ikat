# 待办 · 重构（可维护性 / 结构）

> 来源：`docs/report/review-*.md`，剔除已修项。这些**不是 bug**（行为正确），是结构债：上帝方法、重复代码、死代码、封装泄漏、所有权不当。改不改取决于投入产出，ponytail 默认不做未请求的重构——这份清单是"将来要动时从这里挑"。
> 行号会漂移，按符号 grep。

---

## 上帝方法 / 大函数（最高结构债）

| 级 | 问题 | 位置 |
|---|---|---|
| 🔴 | `tick_and_render` ~100 行、10+ 交织流水线步骤、局部借用纠缠，无法隔离测试 | stage.rs:`tick_and_render`（716 附近） |
| 🔴 | `input::process` 421 行，Move arm 单独 161 行混 5 个关注点（longpress/idle-hover/4 PointerKind/scroll 仲裁/drag-detect/click-focus/事件发射） | input.rs:459 附近 |
| 🔴 | `build_render_nodes` ~490 行 5 层嵌套，含 display 剪枝/5-kind match/scrollbar 合成/sub-page sort/dirty-hash 循环 | render/mod.rs:123 附近 |
| 🔴 | `nine_slice_rounded` 重新实现了 `rounded_rect`+`nine_slice` 已有的 arc-fan/quad 生成，~150 行可共享 | render/mesh.rs:267 附近 |

## 重复代码

| 级 | 问题 | 位置 |
|---|---|---|
| 🟠 | scroll 物理用 `(f32,f32)` 元组逼出逐轴 `if ax==0` 分支，6 个方法各重复 8–15 次（~250 行），改 `[f32;2]`+AXIS 常量每方法 ~50→~15 行 | scroll.rs（drag_follow/begin_inertia/begin_bounce/advance/apply_wheel/set_pos） |
| 🔴 | scrollbar thumb hit 产不在 Scene 的 sentinel NodeId（高位 flag bits），靠巧合（低位=真 id）+ 特例 `!grip_dragging` 跳过工作，NodeId 分配逼近 flag bits 时有碰撞风险 | hit.rs:52 附近（input.rs:681,810） |
| 🔴 | input slot-lookup 逻辑 copy-paste 3 处（add_touch_monitor/cancel_click/find_or_alloc_slot），改策略需同步 3 处 | input.rs:399, 424, 438 附近 |
| 🟠 | `#ifdef` 把 input 代码重复 4 次（ENABLE_INPUT_SYSTEM / #else / #endif），pointer/wheel/modifier 逻辑近乎相同，应收到 IPointerSource 后 | LoomInputCollector.cs:54 等 |
| 🟡 | `CollectWheel`/`ScreenToDesign` 重算 sf/offX 公式，与 `LoomStageDriver.ComputeRootTransform` 一份——两份公式须手动同步 | LoomInputCollector.cs:169 附近 |
| 🟢 | `RenderNode` test-factory helper（container_node/placeholder_rn/mesh_rn/mesh_node）跨 6 处重叠，加字段触 6 处 | render/tests.rs 等 |
| 🟠 | root+A+B 场景构造重复 ≥6 处（hover/two_touch/rollover/focusable，差异仅 Button/tabindex） | input/tests.rs 多处 |
| 🟡 | scroll 场景构造 4 处复制粘贴（entries+build+clip_rect+compute 序列） | input/tests.rs:2871 等 |
| 🟠 | `load_html_css`+`font_path` 在 snapshot/node_sort_keys/v1e_dirty/stage_getters 4 处完全重复定义 | tests/ 4 文件 |
| 🟠 | `EventRouter.cs`（测试）是生产 `LoomEventHandler.cs` 的分离副本，生产改则测试副本漂移 | tests/dotnet/EventRouter.cs ↔ Runtime/LoomEventHandler.cs |
| 🟠 | `stage_new_with_dejavu` 测试辅助在 abi_tests.rs 与 tests.rs 两份完全相同 20 行 | loomgui_ffi_c 两测试文件 |
| 🟢 | `ToAbs`/`ProjectRoot` 模式跨 6 处重复/内联，未提取 LoomEditorPaths | LoomAtlasSync/LoomSettingsWindow/LoomConfigExporter/LoomWorkspaceInitializer |
| 🟡 | `SpriteResolverTests` 与 `AtlasMirrorPoolTests` 测试完全重叠（Init_NullSettings 等价） | tests/dotnet 两文件 |

## 封装 / 所有权 / 契约

| 级 | 问题 | 位置 |
|---|---|---|
| 🟠 | `loomgui_stage_load_html` 在 FFI 层直接调 `node::build_scene`，跳过 Stage 封装；且手动 `tweens.clear()`+`scroll.clear()`+`prev_node_hashes.clear()` 由 FFI 层负责——Stage 内加状态须记得改 FFI | loomgui_ffi_c/src/lib.rs:173 附近 |
| 🟡 | `create_root`/`create_node` 用 `self.scene.as_mut().unwrap()`，仅靠隐式 ensure_scene 契约安全，未来 ensure_scene 失败会静默编译成 panic | stage.rs:517, 526 附近 |
| 🟡 | `desugar_block_divs` 取 tree+styles 所有权后只改 rich_runs 就返还，应 `(&mut tree, &styles)` | loomgui_pkg/src/lib.rs:`desugar_block_divs` |
| 🟡 | `scene_to_template` 依赖 slotmap `values()` 迭代序 = DFS 先序的不变量（非编译期保证） | loomgui_pkg/src/lib.rs:49 附近 |
| 🟠 | blob `build_blob` 20 个裸列变量，加列需改 5 处（columns 元数据/声明/填充/col_bufs 注册/C# 解析），任一遗漏静默偏移。考虑声明式 schema | loomgui_ffi_c/src/blob.rs |
| 🟡 | font-atlas 路径 `loomgui://font-atlas/p{n}` 是隐式跨文件契约（LoomStage 构造、SpriteResolver 消费），应抽 `FontAtlasPath.Format(page)` | LoomStage.cs + SpriteResolver.cs |
| 🟢 | `match_element` 用 `std::ptr::eq` 找 element index，caller 传 cloned node 静默匹配不到，签名应取 ElementId | parse/selector.rs:344 附近 |

## 死代码 / 冗余

| 级 | 问题 | 位置 |
|---|---|---|
| 🔴 | `NodePayload` 是单变体死 enum（Text 合并后只剩 Mesh），强制冗余 match arm + lint allow | render/node.rs:47 附近 |
| 🔴 | `program` 字段是无类型 `u32` magic number，值 0–4 硬编码散布 10+ 处，batch.rs 与 merge.rs 对"哪些 program 可合并"判断不一致 | render/node.rs:54 等 |
| 🟢 | 死字段 `Node.taffy_id` 全项目 0 引用（taffy 树每帧从零建于局部），占 8 字节/节点 | scene/node.rs:96 附近 |
| 🟡 | `rich_fragments` 回写 `resize_with(..,None)`+`fill(None)` 等价单次 `resize(cap+1,None)` | stage.rs:797 附近 |

## 结构 grouping

| 级 | 问题 | 位置 |
|---|---|---|
| 🟡 | `TouchSlot` god struct 17 字段跨 4 状态机（click/drag/scroll 仲裁/longpress），每次 process 全检全置 | input.rs:139 附近 |
| 🟡 | `Stage` struct 25 pub 字段无分组，input-buffer 与 frame-state 可拆子结构以便单次 mem::take | stage.rs:21 附近 |
| 🟢 | `Node` 23 字段混树/语义/布局/脏标/选择器/交互/特性多职责，Default boilerplate 高发 | scene/node.rs:90 附近 |
| 🟡 | `scroll_gesture` u8 bitfield 用 magic 1/2（bit0=V/bit1=H），散落裸 `|= 1`/`& 2`，应命名常量 | input.rs:164 等 |
| 🟡 | `TweenProp` 加变体需触 4 处（def/try_from/prop_value_size/apply）+ C# enum 同步，6 变体可接受、过 ~15 难维护 | tween.rs |
| 🟢 | input 测试扁平 85 用例单 mod tests，注释分节而非子模块，IDE 大纲平 | input/tests.rs 整体 |

## Editor / 后端杂项

| 级 | 问题 | 位置 |
|---|---|---|
| 🟡 | SetDirty+Save+Export 三件套不统一，类内有 `SaveSettings()` 封装但 SmartRecognizeDir/RefreshPackage/DrawAtlasEntry/atlas 删除/LoomAtlasSync.SyncAll 仍手写 | LoomSettingsWindow 多处 |
| 🟡 | `DirectoryDropField` 41 行嵌套 4 层，3 处目录拖拽逻辑（DrawPackageDropZone/HandleFolderDrop/DirectoryDropField）未提取公共 helper | LoomSettingsWindow.cs:465 附近 |
| 🟡 | `SyncEntry` 已定义 `DiffPackables` 却未用，每次全量 Remove+Add packables，100+ sprite 图集触发不必要 reimport | LoomAtlasSync.cs:148 附近 |
| 🟡 | `EnsureAtlasAsset` 在 pkgOutputDir 空时静默返 null，SyncAll 循环末统一 LogWarning，分不清哪个图集失败 | LoomAtlasSync.cs:65 附近 |
| 🟡 | AssetPostprocessor 在 settings 未加载时静默跳过，工作区 PNG 不被设 Sprite 且事后建 settings 不自动重导 | LoomWorkspaceAssetPostprocessor.cs:17 附近 |
| 🟠 | `ColorMatrix(i)` 每次调 heap-alloc 新 `float[20]`，多 color-filter 节点每帧加 GC 压力 | FrameBlob.cs:84 附近 |
| 🟠 | `SetClipBox` 每个 mask context 每帧 O(n) 遍历全部 cached materials，mask 增多时需按 ctx 建索引 | MaterialManager.cs:50 附近 |
| 🟡 | `display:none` 子树仍整棵送进 taffy（build 闭包不检查 Display::None），浪费插入/measure/遍历 | layout/mod.rs:182 附近 |
| 🟡 | `text_layouts` "Some 优先"存储策略依赖 taffy 先 None 后 Some(available) 的测量序，taffy 改序则短文本换行丢失 | layout/mod.rs:300 附近 |
| 🟢 | `out: Vec<EventRecord>` 跨 6 步存活才回写 `last_events`，未来加 early-return 会静默丢事件 | stage.rs:727 附近 |
| 🟡 | `dump_interact.rs` c1/c2/c3 通道命名混乱（c1 返 Green 通道）且与前 3 通道 *255 转换不一致 | dump_interact.rs:90 附近 |

---

## 备注

- **上帝方法四件**（tick_and_render / process / build_render_nodes / nine_slice_rounded）是最大结构债，但都是行为正确的高风险函数——重构须配齐测试再动，ponytail 不建议无测试网贸然拆。
- `NodePayload` 死 enum + `program` magic 是一组：把 program 升成 enum 时正好顺手收掉 NodePayload。
