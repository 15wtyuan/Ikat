# 死代码 / 旧注释 / 版本编号清理 设计

> v1.4a（包加载模型重构）留下大量死代码和迷惑性注释。本 spec 是纯清理——无设计分歧，机械执行。与 `2026-07-03-workflow-atlas-rework-design.md`（工作流 + 图集重做）独立，可单独 plan + 执行。
>
> 用户原话：「这次是一次大重构，导致出现了很多死代码（c#、rust），然后我们代码也充斥着大量奇奇怪怪的注释，说之前是怎么怎么样，现在不用了。。。这种注释就很迷惑人，我们不需要这样的注释，然后一些文档上的编号也不要比如 v1.4a 这种，注释要让人一眼能读懂，不能莫名其妙」
>
> **修订记录**：初稿列 ~60 处。经两轮核实补全——(1) 逐条核实原稿行号/措辞（v7→v8 升级、行号偏移、2 条臆造删除）；(2) workflow 全仓独立审计（5 finder × verify agent）新增 85 条 spec 漏列：9 处死代码（含 `grayed` 死字段、重复测试函数、6 处 C# 调试/占位死代码）、28 处历史注释、48 处版本编号（主因是初稿只写 `v1.4-a Tx` 模式漏抓裸 `T5`/`D17` 编号）。现 ~145 处。
>
> **执行记录**（2026-07-03 完成于 worktree-deadcode-cleanup）：grayed 字段已删（含重生 3 个 insta snapshot）；LoomEventHandlerTests font 占位已修（补 `StreamingAssets/DejaVuSans.ttf` 路径，21 个路由测试恢复可跑）。v1.3 plan「grayed 不删」决策经用户确认推翻。

---

## 1. 目标

代码注释让人一眼读懂当前语义，不被历史叙述迷惑。删死代码。去版本编号。

### 原则

- **注释只说当前是什么、为什么这么设计**，不说"之前是 X 现在改了 / 已删 / 旧版 / 废弃"。
- **去掉版本编号**——含 `v1.4a` / `v1.4-a Tx` / `v1.3` / `v1d.2` / `v1d.5` / `v1c.3` 这类**带前缀**编号，**以及 `T5`/`T6`/`T7`/`T8`/`T10`/`D8`/`D17`/`D2`/`D3` 这类裸编号**（无 `v1.4-a` 前缀的 Task/Decision 标记，新读者同样看不懂"T6 是啥"）。保留语义，去掉编号。
- **唯一保留版本号处**：二进制持久化的真实契约版本号（如 `blob.rs` 的 `VERSION: u32 = 7`、`PKG_FORMAT_VERSION`）——这是协议版本，不是历史标注。
- **pitfalls.md 的坑编号保留**——那是历史档案的检索锚点，合理。

---

## 2. 死代码删除

### 2.1 C# `[Ignore]` 测试文件（7 个，~800 行）

v4 blob 手搓测试，被 v8 FrameBlob 封锁（`ExpectedVersion = 8` 后 `IsValid` 恒 false，`Sync` 早退），从未跑通，标了 "rewrite deferred" 但一直没重写。

| 文件 | 行号 | Ignore 标注 |
|---|---|---|
| `loomgui_unity/Assets/LoomGUI/Tests/FrameBlobTests.cs` | 9 | `[Ignore("v8: blob v4 layout + Unchanged variant retired, rewrite deferred")]` |
| `loomgui_unity/Assets/LoomGUI/Tests/FrameBlobV2Tests.cs` | 12 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/MergeMirrorPoolTests.cs` | 17 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/MirrorPoolTests.cs` | 10 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/MirrorPoolFlattenTests.cs` | 18 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/AtlasMirrorPoolTests.cs` | — | 无 `[Ignore]`，但空壳占位（仅 2 冒烟测无实质断言），自述"旧测试退役" |
| `loomgui_unity/Assets/LoomGUI/Tests/MirrorPoolTexIdTests.cs` | — | 同类退役空壳（类名 `MirrorPoolPathTests`），自述"旧 MirrorPoolTexIdTests 随 T8 砍 tex_id/_texMap 退役" |

**动作**：7 个文件全删（含 `.meta`）。孤立性已核实——`FrameBlobTests`/`MirrorPoolTests` 类型名在 `Assets/` 下零外部引用。若 v8 blob 需要等价测试，另起 spec 补——本 spec 只删不补（YAGNI，v8 blob 已有运行时验收）。

### 2.2 Rust 死代码

> 原草稿断言"Rust 死代码已干净"——**核实有误**，至少 3 处死代码：

| 文件:行 | 死代码 | 动作 |
|---|---|---|
| `render/node.rs:78` | `pub grayed: bool` —— 全库（render/mod.rs、batch.rs、merge.rs、dirty.rs、blob.rs）恒置 false 从不赋真；`mapping.rs` 无分支解析它；`dirty.rs` 的 header_hash/payload_hash 均不读；灰化语义已被 v1.3 ColorFilter（`filter:grayscale`）取代。设计文档明确记载其为占位死字段。 | 删字段 + 删所有 `grayed: false` 初始化处 |
| `loomgui_ffi_c/src/lib.rs:1177` | `fn version_is_v1d_5()` —— 与相邻 `:1186` 的 `version_is_v1e` **逐字重复**（同体、同 `assert_eq!(s, "v1e")`），函数名仍叫 v1d_5（名实不符的过期残留）。 | 删 `version_is_v1d_5`（保留 `version_is_v1e`） |
| `render/mod.rs:671` | 注释记录已删测试 `image_uv_flips_v_for_design_y_down`，该测试全库无定义（仅此注释提及），引用已不存在的 atlas 子区 UV 行为。 | 删该注释段 |

- `#[allow(dead_code)]`：全库 0 处（干净）。
- `#[allow(unused_imports)]`（`blob.rs:4,8`）：import 实际在测试 helper 中被使用，非死代码，保留。
- `#[allow(clippy::type_complexity)]`（`mesh.rs:21,50,144,230`）+ `#[allow(clippy::too_many_arguments)]`（`stage.rs:199`、`text/layout.rs:110`）：合理 lint 抑制，保留。

### 2.3 C# 绑定 `#pragma warning disable`

`LoomGUIBindings.cs:5-6` 的 `CS8500` / `CS8981`——csbindgen 自动生成文件标配，保留。

### 2.4 C# 运行时 / 测试死代码

| 文件:行 | 死代码 | 动作 |
|---|---|---|
| `LoomStage.cs:541` | `[DBG-HOVER]` F1 dump 整树调试块——注释自标"验完删"未删，每帧 LateUpdate 检 F1。`DumpScene()` 仅此处调用。 | 删调试块 + 若 `DumpScene` 无其它调用方一并删 |
| `SpriteResolver.cs:36` | `public Sprite MissingSprite` 属性——setter 全仓零调用（`grep '\.MissingSprite\s*='` 0 命中），`_missingSprite` 恒 null，spec 描述的"紫色占位注入"从未落地。 | 删属性 + `_missingSprite` 字段 + GetSprite 里的 null 透传分支 |
| `SpriteResolver.cs:88` | `[DBG-IMG]` Debug.Log 诊断——自标"验完删"未删，每次 GetSprite 热路径打日志。 | 删 Debug.Log 行 |
| `TextRasterizer.cs:41` | `BuildMesh(..., float alpha)` 的 `alpha` 参数——函数体从不引用（`tinted=color` 直接弃用 alpha），调用方恒传 1f，测试传 0.5f 也无效果。 | 删 `alpha` 参数 + 改所有调用方签名（注意 FFI 边界：BuildMesh 是 C# 内部静态方法，非 FFI 导出，改签名安全） |
| `LoomEventHandlerTests.cs:17-20` | `byte[] fontPathBytes = null` 占位 + 注释块内嵌完整 font_path 补法——占位未补，`stage_new` 遇 null font 返 null，致 `BuildStage()` 产无效 stage，下游 12 个路由测试（`BubbleRoute`/`StopPropagation`/`RollOver` 等，:152-402）恒挂在断言上。**这既是死占位也是潜在 bug**。 | 二选一：(a) 补真 font 路径让测试跑通（修 bug）；(b) 若这些路由测已被 [Ignore] 或属退役范畴则删整个文件。plan 阶段先确认这些测试当前是否在跑、是否被 CI 跳过，再定 |

---

## 3. 迷惑注释改写（去"旧 / 已删 / 已砍"修饰）

### 3.1 Rust 侧

模式：`"旧 X"` / `"已删 X"` / `"已砍 X"` / `"不再 X"` / `"原 X"` / `"替代旧 X"` → 去修饰，直接说当前语义。**已砍术语**（`tex_id`、`Unchanged` 变体、`textures`/`atlas`、`load_inline`、`注册概念`）在注释里出现即误导——它们当前已不存在，注释应说现在的等价物。

> 行号为核实快照，plan 执行时以 grep 关键词定位为准（已核实部分有 ±1~+52 偏移）。

| 文件 | 行号 | 现 | 改 |
|---|---|---|---|
| `stage.rs` | 228 | `旧 NodeId 此后失效` | `NodeId 此后失效`（slotmap remove 后 gen++ 是标准语义，不该叫"旧"）|
| `stage.rs` | 477 | `语义同旧 load_inline` | `语义同 load_html_css_for_test`（load_inline 已砍） |
| `stage.rs` | 836 | `旧 bug：AnimTable::get...` | 去"旧 bug"，直接说防的盲区 |
| `stage.rs` | 1199 | `与旧 load_package 建 scene 语义对立` | `验证 load_package 不建 scene` |
| `stage.rs` | 83,88,89 | `旧包的 path 条目` / `旧条目会悬空` | `前次加载的包的 path 条目` |
| `style/color_filter.rs` | 28 | `旧实现照搬 fgui...` | 去旧实现描述，只说当前乘法语义 |
| `style/resolved.rs` | 6-7 | `替旧 overflow_hidden: bool` | 去旧字段描述，只说当前 OverflowAxis |
| `scene/transform.rs` | 86 | `替代旧 nodes: vec![...]` | 去"替代旧" |
| `scene/node.rs` | 67 | `render 层映射到占位 tex_id` | tex_id 已砍，改 `映射到 image_path` |
| `render/mesh.rs` | 282 | `旧全局线性 umin+...` | 去"旧"，说当前分段 UV 防越界 |
| `render/mesh.rs` | 754 | `Important 2 修复后该顶点已删` | 去"已删顶点"历史 |
| `render/mod.rs` | 109 | `不再预分配 Unchanged 占位` | Unchanged 变体已不存在，删"不再预分配 Unchanged"或改说当前 `change_level 先占 Full 末尾定级` |
| `render/mod.rs` | 629,631 | 测试注释用 `tex_id=0`/`tex_id=1` 描述 DrawState | tex_id 已砍，改 `image_path=None`/`Some("a.png")` |
| `render/mod.rs` | 883 | `旧 hash 表残留` | `前一帧 hash 表` |
| `render/mod.rs` | 1000 | `T6：原"未注册"用例——path 现在总是直填` | 去 T6 + "原…现在"，说 `path 直填（无注册概念）` |
| `render/mod.rs` | 1170 | `T6：原"未注册 url → texture=0"用例` | 同上，去历史叙述 |
| `scene/dynamic.rs` | 11 | `旧 NodeId 失效` | `NodeId 失效` |
| `render/node.rs` | 8-10 | `v1.4-a T6：核心不知图集...render 不再查 textures/atlas` | `核心不知图集...`（去版本 + "不再查"暗示） |
| `input.rs` | 1514 | `旧 euclidean 11.3>10 会拒` | 去"旧"对比，只说当前 per-axis 语义 |
| `fence_contract.rs` | 18,27 | `砍 l-container` / `l-container 砍后是围栏外` | 去"砍"历史，说 `l-container 不在围栏内（用 div）` |
| `tests/snapshot.rs` | 12,13 | `同旧 l...` / `textures/atlases 已砍...tex_id=0` | 去"旧/已砍"，说当前 `load_html_css helper` + `Image 走 image_path fallback` |
| `tests/v1e_dirty.rs` | 4 | `同旧 load_inline 逻辑` | `同 load_html_css 逻辑` |
| `loomgui_ffi_c/src/blob.rs` | 318 | `v7：取代 mesh_node_with_tex` | 去"取代 mesh_node_with_tex"历史，说当前 `path→idx 映射` |
| `loomgui_ffi_c/src/blob.rs` | 739 | `v7：第 18 列 path_idx...Text=0（Unchan...` | 去 v7 前缀，保留列布局事实 |
| `loomgui_ffi_c/src/blob.rs` | 924 | `blob 不再烤 alpha（T6...）` | 去"不再烤"+T6，说当前 `alpha 走 _Alpha uniform 不烤顶点` |
| `loomgui_pkg/src/lib.rs` | 16 | `v1.4-a 砍 atlas_png/atlas_filename` | 删整段（已砍的不需记录） |

> 原草稿列的 `render/dirty.rs:83「与旧 unwrap_or(0) 行为一致」`核实不存在（dirty.rs 全文无"旧"字、无 `unwrap_or(0)`），删除该条。

### 3.2 C# 侧

| 文件 | 行号 | 改写方向 |
|---|---|---|
| `LoomStage.cs` | 388 | "砍 LoadHtml / LoadPackage()..." 列已砍 API 名 → 删整段（记录历史无意义）或改说当前加载模型 |
| `LoomStage.cs` | 390 | `不再 LoadAtlas/_texMap/tex_id` → 去"不再"，说当前 path→Sprite |
| `LoomStage.cs` | 50 | `stress500 已砍` → 删 stress500 注释（读者不知 stress500 是啥）|
| `LoomStage.cs` | 103 | `替代硬编码 build 序 id` → 去"替代硬编码"历史，说当前 `按 CSS id 查节点` |
| `LoomStage.cs` | 271 | `旧 NodeId 此后失效` | `NodeId 此后失效` |
| `FrameBlob.cs` | 42 | `v7：原 tex_id → path_idx` → 保留版本迁移事实但去 `v1.4-a T6` 编号（注：草稿原列 :22，实际 :22 现讲 v8 change_level，v7 注释在 :42） |
| `FrameBlob.cs` | 39 | `0 不再产生——变更级别由 change_level 列表达` → 去"不再产生"历史 |
| `MirrorPool.cs` | 187 | `v1.3 ColorFilter` → 去 `v1.3`（草稿原列 :162，实际 :162 现讲 font null 防御，v1.3 ColorFilter 在 :187） |
| `MaterialManager.cs` | 39 | `坑 v1.3` → 去版本号或改说语义 |
| `LoomShowcaseDriver.cs` | 192,286,343,409 | 4 处 `复用旧 driver 的 SubscribeXxx 逻辑，改用 AddPageListener` → 去"复用旧 driver"，说当前 `AddPageListener 订阅` |
| `TextRasterizer.cs` | 31,45 | `alpha 参数现已废弃...node opacity 走 _Alpha` → 删"现已废弃"（配合 §2.4 删 alpha 参数） |
| `TextRasterizerTests.cs` | 9,129 | `BuildMesh 不再烤 node alpha（T6/T8...）` → 去"不再烤"+T6/T8，说当前 `alpha 走 _Alpha uniform` |

### 3.3 C# 绑定"手补镜像"注释

- `LoomGUIWheelEvent.cs:5` / `LoomGUIKeyEvent.cs:5` 的 `v1d.5-T12` / `v1d.2` → 去版本号，保留"csbindgen 不为 use-imported struct 生成 stub，故手补镜像"核心信息。
- `LoomGUIPointerEvent.cs:5,14,22` 的 `v1c.3`（3 处）→ 同上，去 `v1c.3`，保留手补镜像核心信息。草稿漏列此文件。

---

## 4. 版本编号去编号化

模式：`v1.4-a Tx：X` / 裸 `Tx：X` / 裸 `D17：X` → `X`（去所有 Task/Decision 编号前缀，含带 `v1.4-a` 前缀和无前缀的裸 `T5`/`T6`/`D8`/`D17` 等）。逐文件扫描清理：

> 行号为核实快照，plan 执行时 grep `T[0-9]|D[0-9]|v1\.` 定位。原草稿只写 `v1.4-a Tx` 模式，**漏抓大量裸 T/D 编号**——下方补全。

### 4.1 Rust 源文件

- `asset/mod.rs`（行 3, 15-19, 26, 36, 155, 245, 365）：`v1.4-a 多组件格式` → `多组件格式`；`v1.4-a T4/T6/D17 清理` 段 → 删整段（已删的 struct 不需注释记录）；`PKG_FORMAT_VERSION` 旁注释保留版本号本身（`12`）但去 `v1.4-a D17`。
- `stage.rs`（行 3-4, 21, 76, 317, 477, 602, 636, 689, 938）：`v1.4-a 资源池模型` → `资源池模型`；`v1.4-a T5（spec §4.2/§4.4）` → 去 `v1.4-a T5`，保留 spec 引用；`:938` 的 `T4：compute_world_transforms...` → 去裸 T4。
- `layout/mod.rs`（行 23, 88）：`:23` `v1.4-a D17：核心知图尺寸` → `核心知图尺寸`；`:88` 裸 `T5：remove_node 后 slotmap idx 不变` → 去裸 T5。
- `render/mod.rs`（行 10, 279, 338, 425, 1038, 1102, 1104）：`:10` `v1.4-a D17` → 去；`:279` 裸 `D2/D3` → 去；`:338` 裸 `D17 测试辅助`/`同 T6 行为` → 去；`:425` `T6：bg-image 同走 path` → 去 T6；`:1038` `v1.3 color_filter` → 去 v1.3；`:1102` 裸 `D17` → 去；`:1104` 裸 `T6 硬编码...D17 修` → 去 T6+D17。
- `render/node.rs`（行 8, 55）：`:8` `v1.4-a T6` → 去；`:55` `v1.3 ColorFilter 矩阵` → 去 v1.3。
- `render/merge.rs`（行 10, 43）：`:10` `v1.4-a T6：texture 砍` → 去；`:43` `v1.4-a T6：key 含 Option<String>` → 去。
- `render/batch.rs`（行 41, 163）：`:41` `v1.4-a T6：texture 字段砍` → 去；`:163` 裸 `T5：remove_node 后 slotmap idx 不连续` → 去裸 T5。
- `render/dirty.rs:100`：`v1.4-a T6：texture 砍` → 去。（原草稿列 `:32` 核实不存在——`:32` 无版本号，dirty.rs 唯一版本号在 :100。）
- `render/mod.rs:1071`：原草稿列此处，核实 `:1071` 无版本注释；真正的 `v1.3 color_filter` 在 `:1038`（已在上面覆盖）。删除此条。
- `scene/node.rs`（行 14, 217, 305）：`:14,217` `v1.3+ 动态树 spec §3` → 去 `v1.3+`，保留 spec 引用；`:305` 裸 `T5：容量而非存活数` → 去裸 T5。
- `scene/transform.rs`（行 10, 86）：`:10` 裸 `T5：remove_node 后 slotmap 槽位可复用` → 去裸 T5；`:86` 见 §3.1。
- `scene/dynamic.rs`（行 3, 105, 234）：裸 `T5 实现 remove_node` / `T5 instantiate 用` / `T5 确认 rematch` → 去裸 T5。
- `dump.rs:42`：裸 `T3，经 get(NodeId) 读` → 去裸 T3。
- `loomgui_ffi_c/src/lib.rs`（行 73, 124, 150, 551, 889, 985, 1105, 1128）：`v1.4-a T7：...` / `:551` 裸 `T7 动态树 API FFI` → 去编号保留语义。
- `loomgui_ffi_c/src/blob.rs`（行 22, 336, 990）：`:22` `v6：加 color_matrix 列——v1.3 ColorFilter` → 保留 `v6`（blob 列演进记录）去 `v1.3`；`:336` 裸 `T6/T8` → 去；`:990` `blob_v4_world_matrix_roundtrip` 测试函数名带 v4 → 见 §4.3。**原草稿 `:13`「保留 v7 去 v1.4-a T6」核实已过时**——blob.rs 已升 v8 且本就无 v1.4-a T6 字样，删除该条。
- `loomgui_pkg/src/lib.rs`（行 1-4, 16, 17, 30, 96, 182, 187, 227, 291）：`v1.4-a：每个 HTML 独立 parse` → 去；`砍 image crate / shelf_pack / atlas.png` → 删整段；裸 `D17` 多处 → 去裸 D17；`:187` `spec D10` → 去 D10 保留语义。
- `loomgui_pkg/src/main.rs:4`：`不写 atlas.png——图集归 Unity，D8` → 去 D8 + "不写"历史，说当前 `产物只写 pkg.bin`。
- `tests/snapshot.rs`（行 11, 38）：`v1.4-a T4：load_inline 已砍` → 去；`:38` `v1.4-a T4 helper` → 去。
- `tests/v1e_dirty.rs`（行 3, 19）：`v1.4-a T4：load_inline 已砍` → 去；`:19` `v1.4-a T4 helper` → 去。
- `style/mapping.rs:113`：`v1.3 简化：sepia(1)` → 去 v1.3。

### 4.2 C# 源文件

- `LoomStage.cs`（行 8, 22, 45, 61, 68, 185, 217, 336, 349, 388, 553）：`:8` `v1.4-a T8 path→Sprite` → 去；其余 `v1.4-a T8：X` → `X`；`:217` 裸 `T7 csbindgen 生成` → 去裸 T7。
- `MirrorPool.cs`（行 63, 156, 170, 215, 229, 286）：`:63` `v8 三分支` → 保留 v8（blob 版本号）；`:156` `v5 第 18 列` → 去 v5；`:170` 裸 `T6` → 去；`:215,286` `v1.4-a T8` → 去（原草稿列 126/256 核实已无编号，实际在 215/286）；`:229` 裸 `T6 剥离` → 去。
- `SpriteResolver.cs:8`：`v1.4-a T8「核心不知图集」` → 去 `v1.4-a T8`。
- `PkgManifestReader.cs:8` / `LoomPackageSettings.cs:9` / `LoomPackageManagerWindow.cs:12`：`v1.4-a T9` → 去编号。
- `LoomShowcaseDriver.cs`（行 6, 233, 487, 488）：`:6` `v1.4-a T11 重写` → 去；`:233,487,488` 裸 `T10 复用语义 id` / `T10 交接` / `T10 合进` → 去裸 T10。
- `FrameBlob.cs`（行 22, 43, 80）：`:22` `v8：新增 change_level 列` → 保留 v8；`:43` 列布局注释去裸编号；`:80` `v7：第 18 列 path_idx` → 保留 v7 去 `v1.4-a T6/T8`（原草稿只列 :22 一处，实际 :22/:43/:80 三处）。

### 4.3 函数名中的版本号

`loomgui_ffi_c/src/lib.rs`：
- `version_returns_c_string_v1d5`（行 735）
- `version_is_v1d_5`（行 1177）—— **重复死代码，见 §2.2 删除而非重命名**
- `evt_constants_v1d2`（行 1301）
- `blob_v4_world_matrix_roundtrip`（行 990）—— 测试函数名带 v4，与 §2.1 退役的 v4 blob 测试同类

**动作**：`version_is_v1d_5` 直接删（§2.2）。其余去版本后缀重命名——`version_returns_c_string_v1d5` → `version_returns_c_string`，`evt_constants_v1d2` → `evt_constants`，`blob_v4_world_matrix_roundtrip` → `blob_world_matrix_roundtrip`。这些是 `#[test]` 函数，改名不影响 FFI 契约。改名前 grep 确认无外部引用（测试函数应只自身引用，已核实三个 v1d 函数无外部代码调用）。具体最终名在 plan 阶段看函数体定，本 spec 只要求"去版本后缀、保留语义"。

---

## 5. 文档过时描述

### 5.1 README.md

- `:48`：`loomgui_pkg/ | 打包器 CLI（HTML+CSS+资源 → .pkg.bin + 图集，复用 core 的 parse 层）` → 去掉"+ 图集"（打包器已不打图集，图集归 Unity）。
- `:16`：`动态树（v1.3+ 代际 NodeId + 命令式 API）` → 评估去 `v1.3+`（README 是门面，版本编号对读者无意义）。

### 5.2 不动

- `docs/pitfalls.md`：坑编号（坑 1-104）保留——历史档案检索锚点。
- `docs/superpowers/specs/` + `plans/`：历史 spec 文件名带版本号保留——时间戳档案。

---

## 6. 不做

- **不补 v8 blob 等价测试**（YAGNI，v8 blob 有运行时验收）。
- **不动逻辑语义**——删死代码/改注释/去编号，但不改运行时行为。删 `grayed` 字段、改 `BuildMesh` 签名删 `alpha` 参数属"删死代码"范畴（删的都是无效果路径），不算重构逻辑。若删某死代码会触发连锁逻辑改动，停下评估是否超范围。
- **不动 pitfalls.md 坑编号**。
- **不动历史 spec 文件名**。

---

## 7. 测试

- `cargo test -p loomgui_core fence_contract`——清注释后围栏契约不破。
- `cargo test -p loomgui_core` + `cargo test -p loomgui_pkg` + `cargo test -p loomgui_ffi_c`——全 Rust 测试过。删 `grayed` 字段后若编译失败，说明有遗漏的初始化/读取点需一并清。
- 删 `version_is_v1d_5`（§2.2）后确认 `version_is_v1e` 仍在覆盖该断言。
- Unity 侧删 7 个 `[Ignore]`/退役测试文件后，其余 C# 测试编译过（孤立性已核实）。
- `BuildMesh` 删 `alpha` 参数后，grep 所有调用方改签名，编译过。
- 函数名重命名后 grep 确认无遗漏引用（含 docs/ 下 spec/plan 文档引用）。
- **`LoomEventHandlerTests.cs` 的 font 占位**（§2.4）：plan 阶段先跑一次该测试确认当前是否真的挂在断言上、是否被 CI/Ignore 跳过。若属未跑通占位 → 按 §2.4 (a) 补 font 路径或 (b) 删文件，二选一需用户确认（涉及修 bug vs 删测试的取舍，不在纯清理范围）。

---

## 8. 实现顺序（建议）

1. **删 C# `[Ignore]`/退役测试文件**（§2.1）——7 个文件 + .meta。
2. **Rust 死代码**（§2.2）——删 `grayed` 字段 + 删 `version_is_v1d_5` + 删 `render/mod.rs:671` 注释段。改完跑 `cargo test -p loomgui_core` + `cargo test -p loomgui_ffi_c` 确认绿。
3. **C# 死代码**（§2.4）——删 `[DBG-HOVER]`/`[DBG-IMG]` 调试块、删 `MissingSprite`、删 `BuildMesh` alpha 参数（改调用方）。`LoomEventHandlerTests.cs` 占位单独决策。
4. **Rust 迷惑注释改写**（§3.1）——逐文件 Edit。
5. **C# 迷惑注释改写**（§3.2 + §3.3）。
6. **Rust 版本编号去编号化**（§4.1）——grep `T[0-9]|D[0-9]|v1\.` 逐文件定位 Edit（含裸 T/D 编号）。
7. **C# 版本编号去编号化**（§4.2）。
8. **函数名重命名**（§4.3）+ grep 确认无遗漏（含 docs 引用）。
9. **README 过时描述**（§5.1）。
10. **跑测试**（§7）确认全绿。
11. **重编 .dll + commit + push**——删 `grayed` 字段改了 RenderNode struct 布局（FFI blob 第 X 列？plan 阶段核实 grayed 是否进 blob SOA 头）、改 FFI 测试函数名影响符号。注释改动不影响 .dll 但一起 commit。**改 struct 字段前必须核实 blob.rs 是否序列化 grayed——若进 blob 则是 ABI 变更，须重编 .dll + 家里机重打 pkg。**
