# 死代码 / 旧注释 / 版本编号清理 设计

> v1.4a（包加载模型重构）留下大量死代码和迷惑性注释。本 spec 是纯清理——无设计分歧，机械执行。与 `2026-07-03-workflow-atlas-rework-design.md`（工作流 + 图集重做）独立，可单独 plan + 执行。
>
> 用户原话：「这次是一次大重构，导致出现了很多死代码（c#、rust），然后我们代码也充斥着大量奇奇怪怪的注释，说之前是怎么怎么样，现在不用了。。。这种注释就很迷惑人，我们不需要这样的注释，然后一些文档上的编号也不要比如 v1.4a 这种，注释要让人一眼能读懂，不能莫名其妙」

---

## 1. 目标

代码注释让人一眼读懂当前语义，不被历史叙述迷惑。删死代码。去版本编号。

### 原则

- **注释只说当前是什么、为什么这么设计**，不说"之前是 X 现在改了 / 已删 / 旧版 / 废弃"。
- **去掉 `v1.4a` / `v1.4-a Tx` / `v1.3` / `v1d.2` / `v1d.5` 这类版本编号**——新读者看不懂"T6 是啥"。保留语义，去掉编号。
- **唯一保留版本号处**：二进制持久化的真实契约版本号（如 `blob.rs` 的 `VERSION: u32 = 7`、`PKG_FORMAT_VERSION`）——这是协议版本，不是历史标注。
- **pitfalls.md 的坑编号保留**——那是历史档案的检索锚点，合理。

---

## 2. 死代码删除

### 2.1 C# `[Ignore]` 测试文件（6 个，~800 行）

v4 blob 手搓测试，被 v7 FrameBlob 封锁（`ExpectedVersion = 7` 后 `IsValid` 恒 false，`Sync` 早退），从未跑通，标了 "rewrite to v7 deferred" 但一直没重写。

| 文件 | 行号 | Ignore 标注 |
|---|---|---|
| `loomgui_unity/Assets/LoomGUI/Tests/FrameBlobTests.cs` | 9 | `[Ignore("v1.4-a: blob v4 layout, rewrite to v7 deferred")]` |
| `loomgui_unity/Assets/LoomGUI/Tests/FrameBlobV2Tests.cs` | 12 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/MergeMirrorPoolTests.cs` | 17 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/MirrorPoolTests.cs` | 10 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/MirrorPoolFlattenTests.cs` | 18 | 同 |
| `loomgui_unity/Assets/LoomGUI/Tests/AtlasMirrorPoolTests.cs` | — | 无 `[Ignore]`，但空壳占位（仅 2 冒烟测无实质断言），自述"旧测试退役" |

**动作**：6 个文件全删（含 `.meta`）。若 v7 blob 需要等价测试，另起 spec 补——本 spec 只删不补（YAGNI，v7 blob 已有运行时验收）。

### 2.2 Rust 侧

- **无 `#[allow(dead_code)]`**——Rust 死代码已干净，无需删。
- `#[allow(clippy::type_complexity)]`（`render/mesh.rs:144`，nine_slice 复杂返回类型）——合理，保留。

### 2.3 C# 绑定 `#pragma warning disable`

`LoomGUIBindings.cs:5-6` 的 `CS8500` / `CS8981`——csbindgen 自动生成文件标配，保留。

---

## 3. 迷惑注释改写（去"旧 / 已删 / 已砍"修饰）

### 3.1 Rust 侧（~15 处）

模式：`"旧 X"` / `"已删 X"` / `"已砍 X"` → 去修饰，直接说当前语义。

| 文件 | 行号 | 现 | 改 |
|---|---|---|---|
| `stage.rs` | 228 | `旧 NodeId 此后失效` | `NodeId 此后失效`（slotmap remove 后 gen++ 是标准语义，不该叫"旧"）|
| `stage.rs` | 476 | `语义同旧 load_inline` | `语义同 load_inline_for_test` |
| `stage.rs` | 835 | `旧 bug：AnimTable::get...` | 去"旧 bug"，直接说防的盲区 |
| `stage.rs` | 1147 | `与旧 load_package 建 scene 语义对立` | `验证 load_package 不建 scene` |
| `style/color_filter.rs` | 28 | `旧实现照搬 fgui...` | 去旧实现描述，只说当前乘法语义 |
| `style/resolved.rs` | 6-7 | `替旧 overflow_hidden: bool` | 去旧字段描述，只说当前 OverflowAxis |
| `scene/transform.rs` | 86 | `替代旧 nodes: vec![...]` | 去"替代旧" |
| `render/mesh.rs` | 282 | `旧全局线性 umin+...` | 去"旧"，说当前分段 UV 防越界 |
| `render/mesh.rs` | 754 | `Important 2 修复后该顶点已删` | 去"已删顶点"历史 |
| `scene/dynamic.rs` | 11 | `旧 NodeId 失效` | `NodeId 失效` |
| `render/dirty.rs` | 83 | `与旧 unwrap_or(0) 行为一致` | 去"旧 unwrap_or(0)" |
| `render/mod.rs` | 852 | `旧 hash 表残留` | `前一帧 hash 表` |
| `stage.rs` | 83,88,89 | `旧包的 path 条目` / `旧条目会悬空` | `前次加载的包的 path 条目` |
| `render/node.rs` | 8-10 | `v1.4-a T6：核心不知图集...render 不再查 textures/atlas` | `核心不知图集...`（去版本 + "不再查"暗示） |

### 3.2 C# 侧（~7 处）

| 文件 | 行号 | 改写方向 |
|---|---|---|
| `LoomStage.cs` | 388 | "砍 LoadHtml / LoadPackage()..." 列已砍 API 名 → 删整段（记录历史无意义）或改说当前加载模型 |
| `LoomStage.cs` | 390 | `不再 LoadAtlas/_texMap/tex_id` → 去"不再"，说当前 path→Sprite |
| `LoomStage.cs` | 50 | `stress500 已砍` → 删 stress500 注释（读者不知 stress500 是啥）|
| `FrameBlob.cs` | 22, 42 | `v7：tex_id 列 → path_idx 列` → 保留版本迁移事实但去 `v1.4-a T6` 编号 |
| `MirrorPool.cs` | 162 | `v1.3 ColorFilter` → 去 `v1.3` |
| `MaterialManager.cs` | 39 | `坑 v1.3` → 去版本号或改说语义 |

### 3.3 C# 绑定"手补镜像"注释

`LoomGUIWheelEvent.cs:5` / `LoomGUIKeyEvent.cs:5` 的 `v1d.5-T12` / `v1d.2` → 去版本号，保留"csbindgen 不为 use-imported struct 生成 stub，故手补镜像"核心信息。

---

## 4. 版本编号去编号化

### 4.1 Rust 源文件（~30 处）

模式：`v1.4-a Tx：X` → `X`。逐文件扫描清理：

- `asset/mod.rs`（行 3, 15-18, 26, 36, 155, 245, 365）：`v1.4-a 多组件格式` → `多组件格式`；`v1.4-a T4 清理：删...` → 删整段（已删的 struct 不需注释记录）；`PKG_FORMAT_VERSION` 旁注释保留版本号本身（`12`）但去 `v1.4-a D17`。
- `stage.rs`（行 3-4, 21, 76, 317, 477, 602, 635, 688）：`v1.4-a 资源池模型` → `资源池模型`；`v1.4-a T5（spec §4.2/§4.4）` → 去 `v1.4-a T5`，保留 spec 引用。
- `layout/mod.rs:23` / `render/mod.rs:10`：`v1.4-a D17：核心知图尺寸` → `核心知图尺寸`。
- `render/mod.rs:425` / `render/node.rs:8` / `render/merge.rs:10` / `render/batch.rs:41` / `render/dirty.rs:100`：`v1.4-a T6：texture 砍，改 image_path` → `Image RenderNode payload 带 image_path`（去版本 + "砍/改"历史）。
- `loomgui_ffi_c/src/lib.rs`（行 73, 124, 150, 889, 985, 1105, 1128）：`v1.4-a T7：...` → 去编号保留语义。
- `loomgui_ffi_c/src/blob.rs:13`：`v7：tex_id 列 → path_idx 列 + path string table arena（v1.4-a T6「核心不知图集」）` → 保留 `v7`（blob 版本号是契约）+ 去 `v1.4-a T6`。
- `loomgui_pkg/src/lib.rs`（行 1-4, 182）：`v1.4-a：每个 HTML 独立 parse` → 去 `v1.4-a`；`砍 image crate / shelf_pack / atlas.png（图集归 Unity，D8）` → 删整段（已砍的不需记录）。
- `scene/node.rs:14, 217`：`v1.3+ 动态树 spec §3` → 去 `v1.3+`，保留 spec 引用。
- `style/mapping.rs:113` / `render/dirty.rs:32` / `render/mod.rs:1071`：`v1.3 简化` / `v1.3：捕 program` → 去 `v1.3`。

### 4.2 C# 源文件（~15 处）

- `LoomStage.cs`（行 22, 45, 61, 68, 185, 336, 349, 388, 553）：`v1.4-a T8：X` → `X`。
- `MirrorPool.cs`（行 126, 256）：`v1.4-a T8：按 path_idx 取 path` → 去 `v1.4-a T8`。
- `SpriteResolver.cs:8`：`v1.4-a T8「核心不知图集」Unity 侧落地` → 去 `v1.4-a T8`。
- `PkgManifestReader.cs:8` / `LoomPackageSettings.cs:9` / `LoomPackageManagerWindow.cs:12`：`v1.4-a T9` → 去编号。
- `LoomShowcaseDriver.cs:6`：`v1.4-a T11 重写` → 去 `v1.4-a T11`。
- `FrameBlob.cs:22`：`v7：tex_id → path_idx（v1.4-a T6/T8）` → 保留 `v7`，去 `v1.4-a T6/T8`。

### 4.3 函数名中的版本号

`loomgui_ffi_c/src/lib.rs`：
- `version_returns_c_string_v1d5`（行 735）
- `version_is_v1d_5`（行 1177）
- `evt_constants_v1d2`（行 1301）

**动作**：去版本后缀重命名——`version_returns_c_string_v1d5` → `version_returns_c_string`，`version_is_v1d_5` → `version_is_v1d5`（保留 v1d5 语义若它是断言版本号=v1d5 的测试，否则 `version_format_ok`），`evt_constants_v1d2` → `evt_constants`。这些是 `#[test]` 函数，改名不影响 FFI 契约。改名前 grep 确认无外部引用（测试函数应只自身引用）。具体最终名在 plan 阶段看函数体定，本 spec 只要求"去版本后缀、保留语义"。

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

- **不补 v7 blob 等价测试**（YAGNI，v7 blob 有运行时验收）。
- **不重构代码结构**（只清注释 + 删死代码，不动逻辑）。
- **不动 pitfalls.md 坑编号**。
- **不动历史 spec 文件名**。

---

## 7. 测试

- `cargo test -p loomgui_core fence_contract`——清注释后围栏契约不破。
- `cargo test -p loomgui_core` + `cargo test -p loomgui_pkg` + `cargo test -p loomgui_ffi_c`——全 Rust 测试过。
- Unity 侧删 6 个 `[Ignore]` 测试文件后，其余 C# 测试编译过（删文件不破其它测试引用——这些 `[Ignore]` 文件是孤立的）。
- 函数名重命名后 grep 确认无遗漏引用。

---

## 8. 实现顺序（建议）

1. **删 C# `[Ignore]` 测试文件**（§2.1）——6 个文件 + .meta。
2. **Rust 迷惑注释改写**（§3.1）——逐文件 Edit。
3. **C# 迷惑注释改写**（§3.2 + §3.3）。
4. **Rust 版本编号去编号化**（§4.1）——逐文件 Edit。
5. **C# 版本编号去编号化**（§4.2）。
6. **函数名重命名**（§4.3）+ grep 确认无遗漏。
7. **README 过时描述**（§5.1）。
8. **跑测试**（§7）确认绿。
9. **重编 .dll + commit + push**（函数名改了影响 FFI 符号，重编 .dll；注释改动不影响 .dll 但一起 commit）。
