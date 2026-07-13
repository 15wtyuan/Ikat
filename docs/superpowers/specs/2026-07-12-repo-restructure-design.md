# 仓库目录重构 + 绑定架构重组设计

- **日期**：2026-07-12（2026-07-13 修订）
- **状态**：待批准
- **关联**：`docs/report/triage/refactor.md`（结构债清单）、`CLAUDE.md`（构建/闭环命令、路径引用）

## 1. 背景与动机

当前仓库根目录平铺 4 个 Rust crate（`loomgui_core` / `loomgui_ffi_c` / `loomgui_pkg` / `loomgui_gui`）+ 2 个 Unity 目录（`loomgui_unity` / `loomgui_unity_package`）+ `docs` / `tests` / `showcase_project` / `temp` / `target`。根目录拥挤，"Rust 核心层" 与 "Unity 后端层" 两个关注域未分层，读者无法从目录一眼看出架构边界。

本次重构：

- Rust 代码收归 `crates/`，对齐 Rust 社区惯例（bevy 等）。
- Unity 代码收归 `unity/`，并把"打包工具家族"（CLI + GUI）归到 `crates/packer/`，体现"运行时核心 vs 构建期工具"的分层。
- `loomgui_gui` 改成 Tauri 2 标准的 `src-tauri/` + 前端 `dist/` 平级布局。
- **绑定架构重组**：新增 `crates/xtask` 编排 crate，将 csbindgen 绑定生成从 ffi `build.rs` 解耦——为多引擎后端（Godot / Unreal 即将同步开搞）预留扩展点。
- 顺手做几条代码重构（`refactor.md` 里筛选过的）。

**不是本次做的**：`refactor.md` 里的上帝方法（`tick_and_render` / `input::process` / `build_render_nodes`）、重复逻辑、死字段等纯代码内部债——它们另开 spec，配测试再动。

## 2. 范围

### In

- 目录搬迁：`crates/{core, ffi, packer/{pkg, gui}}` + `unity/{showcase-unity, package}`
- `loomgui_gui` 改 Tauri 2 标准 `src-tauri/` 布局（workspace member 指向 `src-tauri`）
- **新增 `crates/xtask`**：csbindgen 绑定生成 + 分发编排（路线 A：csbindgen 管 C#，将来 cbindgen 管 C/C++ 头给 Godot/Unreal）
- ffi `build.rs` 解耦：只写 `OUT_DIR`（Rust 测试用），删掉直写 Unity 目录的分支
- 全局路径引用同步：`Cargo.toml`、`manifest.json`、`CLAUDE.md`、`docs/`、`.github/`、`.gitignore`、`tauri.conf.json`、CI workflows
- 顺手代码重构：
  - Rust 测试 helper 统一（core 用 `tests/common/`；ffi 用 `src/test_helpers.rs`，机制不同，见 §6）
  - `FontAtlasPath.Format()` / `font_atlas_path()` 抽取（Rust + C# 各一个构造器，消除裸字符串拼接）

### Out

- **package name 改名**：保持 `loomgui_core` / `loomgui_ffi_c` / `loomgui_pkg` / `loomgui_gui`，所有 `cargo -p loomgui_*` 命令不变。只改目录名。
- `refactor.md` 上帝方法 / 重复逻辑 / 死字段（含 `NodePayload` 死 enum + `program` magic number 一组）。
- `fence_contract.rs` 拆分（nice-to-have，本次不做）。
- `EventRouter.cs` 测试副本归位（`<Link>` 方案行不通——LoomEventHandler.cs 依赖 UnityEngine/FFI，纯 net10.0 项目 `<Link>` 引用会编译失败。副本漂移问题留 refactor.md，将来做路由算法提取时一并解决）。
- 改用 `Samples~` 内嵌示例（保持独立 Unity 工程——需要完整工程跑 PlayMode 验收 + Kenney/Bundles 资源）。
- cbindgen / Godot / Unreal 绑定（xtask 架子搭好，cbindgen 占位 TODO，等后端启动时再加）。

## 3. 目标目录结构

```
LoomGUI/
├── Cargo.toml          # workspace members 改为 crates/*
├── Cargo.lock
├── README.md  CHANGELOG.md  CLAUDE.md  CONTRIBUTING.md  LICENSE
├── .github/  .gitignore
│
├── crates/
│   ├── core/           # ← loomgui_core   (package name 不变)
│   │   ├── Cargo.toml
│   │   ├── src/        # 内联 #[cfg(test)] mod tests 不动
│   │   ├── tests/      # 含 fixtures/ snapshots/ + 新增 common/mod.rs (共享 helper)
│   │   └── examples/   # dump_* 诊断 example
│   ├── ffi/            # ← loomgui_ffi_c
│   │   ├── Cargo.toml
│   │   ├── build.rs    # csbindgen 只写 OUT_DIR（解耦，不再直写 Unity 目录）
│   │   └── src/        # lib.rs / tests.rs / abi_tests.rs / blob/ + 新增 test_helpers.rs
│   ├── packer/
│   │   ├── pkg/        # ← loomgui_pkg
│   │   │   ├── Cargo.toml
│   │   │   ├── src/  tests/  examples/
│   │   └── gui/        # ← loomgui_gui
│   │       ├── dist/               # ← 前端静态文件（与 src-tauri 平级）
│   │       └── src-tauri/          # ← workspace member 指这里
│   │           ├── Cargo.toml      # loomgui_pkg = { path = "../../pkg" }
│   │           ├── build.rs        # tauri_build::build()
│   │           ├── tauri.conf.json # frontendDist: "../dist"
│   │           ├── capabilities/
│   │           ├── icons/          # bundle 资产（留 src-tauri 内）
│   │           ├── templates/      # include_str! 编译期资源（留 src-tauri 内）
│   │           ├── gen/            # gitignored（Tauri 生成物）
│   │           └── src/
│   │               ├── main.rs  commands.rs  recent.rs
│   └── xtask/          # ← 新增：构建编排 crate（绑定生成 + 分发）
│       ├── Cargo.toml  # csbindgen = "1"
│       └── src/
│           ├── main.rs # cargo run -p xtask -- sync-bindings
│           └── bindings.rs
│
├── unity/
│   ├── showcase-unity/ # ← loomgui_unity      (Unity 工程)
│   │   ├── Assets/  ProjectSettings/  Packages/
│   │   └── Packages/manifest.json          # com.loomgui.unity: file:../../package
│   └── package/        # ← loomgui_unity_package  (UPM 包 com.loomgui.unity)
│       ├── package.json
│       └── Editor/  Runtime/  Plugins/  Shaders/  Tests/
│
├── showcase_project/   # 不动 (LoomGUI 工作区 / 打包器素材)
├── tests/dotnet/       # 不动 (FrameBlob.cs <Link> 路径同步)
├── docs/  temp/  target/   # 不动
```

## 4. 搬迁映射

| 旧 | 新 | 改动 |
|---|---|---|
| `loomgui_core/` | `crates/core/` | git mv |
| `loomgui_ffi_c/` | `crates/ffi/` | git mv + 改 path dep + build.rs 解耦 |
| `loomgui_pkg/` | `crates/packer/pkg/` | git mv + 改 path dep |
| `loomgui_gui/` | `crates/packer/gui/` | git mv + **拆 src-tauri 布局**（见 §5.3）+ 改 path dep |
| `loomgui_unity/` | `unity/showcase-unity/` | git mv + **改 manifest.json file: 路径** |
| `loomgui_unity_package/` | `unity/package/` | git mv |
| （无） | `crates/xtask/` | **新建**：绑定编排 crate |

**crate 间 path 依赖**（全改）：

- `crates/ffi/Cargo.toml`：`loomgui_core = { path = "../core" }`
- `crates/packer/pkg/Cargo.toml`：`loomgui_core = { path = "../../core" }`
- `crates/packer/gui/src-tauri/Cargo.toml`：`loomgui_pkg = { path = "../../pkg" }`（从 src-tauri 出发：`../`=gui，`../../`=packer，到 `packer/pkg`）

**根 `Cargo.toml` members**：

```toml
[workspace]
members = [
    "crates/core",
    "crates/ffi",
    "crates/packer/pkg",
    "crates/packer/gui/src-tauri",
    "crates/xtask",
]
resolver = "2"
```

**gui workspace member 指向 `src-tauri`**（不是 `gui/`），因为 Cargo.toml 在 `src-tauri/` 内。

## 5. 路径引用同步（机械风险清单）

搬迁本身是 `git mv`，**真正的工作量和风险在引用同步**。逐项：

1. **根 `Cargo.toml` members**：见 §4。

2. **crate 间 `path =` 依赖**：见 §4，三个 crate 各自的 `Cargo.toml`。

3. **gui `src-tauri/` 布局重构**（非纯移动，要实测）：

   Tauri 2 + cargo 有两条硬约束：
   - cargo 在 `Cargo.toml` 所在目录（包根）找 `src/main.rs` 和 `build.rs`
   - `tauri_build::build()` 的 CWD = 包根，在该目录找 `tauri.conf.json`

   因此 Cargo.toml + build.rs + tauri.conf.json + src/ **必须在同一目录**（src-tauri/）。dist/ 作为前端资产留在 gui/ 层与 src-tauri 平级，对齐 Tauri 官方 `create-tauri-app` 脚手架的标准布局。

   具体搬迁（从旧 `loomgui_gui/` 到新 `crates/packer/gui/`）：

   | 旧位置 | 新位置 | 说明 |
   |---|---|---|
   | `Cargo.toml` | `src-tauri/Cargo.toml` | workspace member 指这里 |
   | `build.rs` | `src-tauri/build.rs` | tauri_build::build() |
   | `tauri.conf.json` | `src-tauri/tauri.conf.json` | frontendDist 改 |
   | `capabilities/` | `src-tauri/capabilities/` | |
   | `icons/` | `src-tauri/icons/` | bundle 资产，留 src-tauri 内 |
   | `templates/` | `src-tauri/templates/` | include_str! 编译期资源，留 src-tauri 内 |
   | `gen/` | `src-tauri/gen/` | gitignored，直接删旧的重生 |
   | `src/` | `src-tauri/src/` | main.rs / commands.rs / recent.rs |
   | `dist/` | `dist/`（gui 层，与 src-tauri 平级）| 前端静态文件 |

   **`tauri.conf.json` 改动**：`frontendDist: "dist"` → `"../dist"`（dist 从 src-tauri 同级变成了上一级）。`bundle.icon: ["icons/icon.ico"]` 不用改（icons 随 src-tauri 移动）。

   **`include_str!` 路径不用改**：commands.rs 里 `include_str!("../templates/...")` 相对 `src/`，移动后 src/ 和 templates/ 仍是 src-tauri 内的兄弟关系。

   验证：`cd crates/packer/gui/src-tauri && tauri build --no-bundle` 出 exe，双击能开 GUI。

4. **`unity/showcase-unity/Packages/manifest.json`**：`com.loomgui.unity` 的 `file:` 路径从 `file:../../loomgui_unity_package` 改为 `file:../../package`。Unity 的 `file:` 路径相对 `Packages/` 目录——工程（`unity/showcase-unity/`）与包（`unity/package/`）搬迁后仍是仓库内兄弟，从 `Packages/` 出发的相对层级 `../../` 不变，只换最后一段目录名。Unity 打开时 Library 缓存可能残留，必要时删 `unity/showcase-unity/Library` 重导。

5. **`CLAUDE.md`**（最大工作量）：构建命令、`.dll`/exe 拷贝目标路径、dump_ example 路径、所有旧目录路径——全量替换为新路径。新增 `cargo run -p xtask -- sync-bindings` 命令说明（替代旧 build.rs 直写 Unity 的行为）。gui 闭环段 `(cd loomgui_gui && tauri build --no-bundle)` 改为 `(cd crates/packer/gui/src-tauri && tauri build --no-bundle)`。这是漂移高发区，搬迁后必须逐条核对。

6. **`docs/`**：`report/triage/refactor.md`（行号路径）、`roadmap/`、`superpowers/specs/` 旧 spec 里提到的目录名——grep 扫描，按语义替换。

7. **`.github/workflows/`**：
   - **`rust-ci.yml`**：基本不用改（全用 package name `cargo -p loomgui_*` / `--workspace`）。新增 xtask 后核对 clippy/test 的 `--exclude loomgui_gui` 是否要扩展（xtask 是纯 Rust 无系统依赖，不用排除）。
   - **`unity-smoke.yml`**：6 处硬编码旧路径需改，且 dll staging 后新增 xtask sync-bindings 步骤（build.rs 不再直写 Unity bindings）。详见下表：

   `unity-smoke.yml` 具体：

   | 旧 | 新 |
   |---|---|
   | `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`（2 处 staging） | `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` |
   | `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`（2 处 git add） | `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` |
   | `loomgui_unity/Library`（2 处 cache path） | `unity/showcase-unity/Library` |
   | `loomgui_unity/Packages/manifest.json`（2 处 cache key） | `unity/showcase-unity/Packages/manifest.json` |
   | `projectPath: loomgui_unity`（2 处） | `projectPath: unity/showcase-unity` |

   且 `cargo build -p loomgui_ffi_c --release` 之后、`git add` bindings 之前，新增：
   ```yaml
   - name: sync bindings via xtask
     run: cargo run -p xtask -- sync-bindings
   ```

8. **csbindgen 输出路径 → xtask 解耦**（原 §5.8，升级为架构变更）：

   **现状**：`crates/ffi/build.rs` 做 csbindgen 生成时直写 Unity 包目录（`../loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`），ffi crate 知道了一个具体引擎后端在哪——耦合源。

   **改后**：
   - ffi `build.rs` 只写 `OUT_DIR/LoomGUIBindings.cs`（Rust 测试/编译用），删掉 Unity 直写分支。
   - 新建 `crates/xtask`：`sync-bindings` 子命令跑 csbindgen，从 ffi 源生成 `.cs`，分发到各后端。当前只管 Unity（`unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`），cbindgen / Godot / Unreal 作为 TODO 占位。

   **多引擎扩展点**：加引擎 = xtask 里加一个分发目标（cbindgen 出 `loomgui.h` → Godot/Unreal）。核心 crate 零改动。

9. **`.gitignore` 白名单路径**（全量更新）：
   - `!loomgui_unity/Assets/LoomUI/res/**/*.unitypackage` → `!unity/showcase-unity/Assets/LoomUI/res/**/*.unitypackage`
   - `!loomgui_unity/Assets/LoomUI/res/**/*.unitypackage.meta` → `!unity/showcase-unity/Assets/LoomUI/res/**/*.unitypackage.meta`
   - `loomgui_gui/gen/` → `crates/packer/gui/src-tauri/gen/`
   - `**/Plugins/**/*.dll` 等通配规则不用改（已核对）。

## 6. 顺手重构项

### 6.1 测试 helper 统一（core + ffi，两种机制）

core 和 ffi 的测试 helper 去重机制**不同**，spec 必须区分：

**core 侧**（`load_html_css` / `font_path`）：

用于 `tests/*.rs`（集成测试文件，`tests/` 目录下）。Rust 集成测试共享 helper 的标准做法：

```rust
// crates/core/tests/common/mod.rs
pub fn load_html_css(name: &str) -> (String, String) { ... }
pub fn font_path() -> &'static Path { ... }
```

各集成测试文件 `mod common; use common::*;`。收 `snapshot` / `node_sort_keys` / `v1e_dirty` / `stage_getters` 4 处重复定义。

**ffi 侧**（`stage_new_with_dejavu`）：

用于 `src/tests.rs` 和 `src/abi_tests.rs`（内联测试，由 lib.rs 底部 `#[cfg(test)] mod tests; mod abi_tests;` 引入）。这些是 crate 内部模块，`use super::*` 能访问 `extern "C"` 函数和私有类型 `StageHandle`。如果放进 `tests/`（集成测试目录）会降级成只能测公开 API 的外部测试，丢失私有项覆盖——**位置不动**。

共享 helper 用 `#[cfg(test)]` 内部模块：

```rust
// crates/ffi/src/test_helpers.rs
use crate::*;  // 能访问 lib.rs 的 extern "C" + 私有 StageHandle

#[cfg(test)]
pub(crate) fn stage_new_with_dejavu(w: f32, h: f32) -> *mut StageHandle { ... }
```

lib.rs 加 `#[cfg(test)] mod test_helpers;`，tests.rs 和 abi_tests.rs 各自 `use crate::test_helpers::stage_new_with_dejavu;`。

**fixture 路径修正**（ffi helper 内）：

`stage_new_with_dejavu` 里用 `env!("CARGO_MANIFEST_DIR")` + `/../loomgui_core/tests/fixtures/DejaVuSans.ttf` 拼路径。搬到 `crates/ffi/` 后，`CARGO_MANIFEST_DIR` = `crates/ffi`，`/../` = `crates/`，core 在 `crates/core/`——所以改成 `/../core/tests/fixtures/DejaVuSans.ttf`（一层 `..`，core 与 ffi 是 `crates/` 下兄弟）。

### 6.2 FontAtlasPath.Format() / font_atlas_path() 抽取

**现状**：`loomgui://n/p{page}` 路径散在多处裸字符串拼接，是隐式跨语言契约——Rust 侧构造写进 blob `image_path`，C# 侧用同样格式构造 key 注册进 SpriteResolver，MirrorPool 用 blob 的 `image_path` 查 SpriteResolver。格式碰巧一致才 work。

| 位置 | 代码 |
|---|---|
| Rust `render/mod.rs`（2 处） | `format!("loomgui://n/p{}", page)` |
| C# `LoomStage.cs:268` | `$"loomgui://n/p{page}"` |
| C# `SpriteResolver.cs`（注释） | `loomgui://n/p{n}` |
| C# `MirrorPool.cs`（注释） | `loomgui://n/...` |

**改法**：每侧各一个命名构造器，不搞跨语言代码生成。

Rust 侧（`render/mod.rs` 或新模块）：
```rust
/// Font-atlas image_path for a given page. Consumed verbatim by Unity
/// SpriteResolver (LoomStage.RegisterFontAtlasPage) — ABI-level contract.
pub(crate) fn font_atlas_path(page: usize) -> String {
    format!("loomgui://n/p{page}")
}
```

C# 侧（SpriteResolver.cs 或新静态类）：
```csharp
/// Must match Rust render::font_atlas_path (image_path in blob).
public static string Format(int page) => $"loomgui://n/p{page}";
```

三处裸拼接改调构造器。行为完全不变（生成的字符串一模一样）。改格式时 grep `font_atlas_path` / `FontAtlasPath` 两处即可。

### 6.3 不做的项

| 项 | 原因 |
|---|---|
| `EventRouter.cs` 测试副本归位 | `<Link>` 引用 `LoomEventHandler.cs` 会编译失败（依赖 UnityEngine + FFI，纯 net10.0 项目无引用）。副本问题是路由算法未从生产源物理拆出。留 refactor.md，将来做 LoomEventHandler 路由提取时一并解决。搬迁时只改 FrameBlob.cs 的 `<Link>` 路径（`..\..\loomgui_unity_package\` → `..\..\unity\package\`）。 |
| `fence_contract.rs` 680 行拆分 | nice-to-have，本次不做 |
| examples 命名（`dump_rich` vs `dump_rich_showcase`） | 低优先，本次不做 |

## 7. 执行计划（分阶段 + worktree 隔离）

在 git worktree 里做（不阻塞 main，可随时停）。每阶段独立 commit，可单独验证 / 回滚。

- **阶段 0 — 开 worktree**：用 `superpowers:using-git-worktrees` 起隔离分支 `refactor/repo-restructure`。

- **阶段 1 — Rust 侧 `crates/` 搬迁（含 gui src-tauri 布局）**
  - `git mv` core / ffi / pkg 到新位
  - `git mv` gui 到 `crates/packer/gui/`，按 §5.3 拆 src-tauri 布局
  - 改根 `Cargo.toml` members（gui 指向 `src-tauri`）
  - 改三个 crate 间 `path =` 依赖
  - 改 `tauri.conf.json` 的 `frontendDist` → `"../dist"`
  - 验证：`cargo build --all-targets` + `cargo test`（workspace）全绿
  - 验证（Windows）：`cd crates/packer/gui/src-tauri && tauri build --no-bundle` 出 exe
  - commit

- **阶段 2 — Unity 侧 `unity/` 搬迁**
  - `git mv loomgui_unity unity/showcase-unity`、`git mv loomgui_unity_package unity/package`
  - 改 `manifest.json` 的 `file:` 路径（见 §5.4）
  - 验证：Unity 打开 `unity/showcase-unity`，Package Manager 能识别 `com.loomgui.unity`，PlayMode 不报 `.dll` 找不到
  - commit

- **阶段 3 — xtask 绑定架构**
  - 新建 `crates/xtask`（Cargo.toml + src/main.rs + src/bindings.rs）
  - `sync-bindings` 子命令：csbindgen 从 ffi 源生成 `.cs` → 写到 `unity/package/Plugins/LoomGUI/Bindings/`
  - 改 `crates/ffi/build.rs`：删 Unity 直写分支，只保留 `OUT_DIR`
  - 加 xtask 到 workspace members
  - 验证：`cargo run -p xtask -- sync-bindings` 生成 `.cs` 到新 Unity 路径；`cargo build -p loomgui_ffi_c` 不再写 Unity 目录
  - commit

- **阶段 4 — 全局路径引用同步**
  - `CLAUDE.md`：构建命令、`.dll`/exe 拷贝路径、`cargo run -p xtask -- sync-bindings` 说明、gui 闭环路径
  - `docs/`：refactor.md / roadmap / specs 旧路径
  - `.github/workflows/`：`unity-smoke.yml` 6 处路径 + xtask sync-bindings 步骤（见 §5.7）；`rust-ci.yml` 核对 exclude
  - `.gitignore`：白名单路径（见 §5.9）
  - `tests/dotnet/LoomGUI.Tests.Core.csproj`：FrameBlob.cs `<Link>` 路径（`..\..\unity\package\`）
  - 收尾扫描：grep 旧**目录路径**残留（如 `loomgui_core/`、`loomgui_unity_package/`、`loomgui_gui/`，即带路径分隔符的用法）在 `*.md` / `*.yml` / `*.toml` / `*.json` / `*.csproj` 无残留。**注意区分**：`cargo -p loomgui_core` 这种 **package name** 引用是合法的（包名不变），不是残留——只查"作为文件系统路径"的用法。
  - commit

- **阶段 5 — 顺手重构（helper 统一 + FontAtlasPath）**
  - core helper 统一到 `tests/common/mod.rs`
  - ffi helper 统一到 `src/test_helpers.rs` + fixture 路径修正（`../core`）
  - `FontAtlasPath.Format()` / `font_atlas_path()` 抽取（Rust + C#）
  - 验证：`cargo test` 全绿 + `tests/dotnet` 编译过
  - commit

- **阶段 6 — 合回 main**：fast-forward 合并 `refactor/repo-restructure`。

## 8. 验证标准（每阶段 Done 判据）

| 阶段 | Done 判据 |
|---|---|
| 1 | `cargo test`（workspace 全 crate）全绿 = Rust 侧搬迁 + 依赖正确；Windows 上 `tauri build --no-bundle` 出 exe |
| 2 | Unity 打开 `unity/showcase-unity`，Package Manager 见 `com.loomgui.unity`，PlayMode 起得来不报 dll 缺失 |
| 3 | `cargo run -p xtask -- sync-bindings` 出 `.cs` 到 `unity/package/Plugins/LoomGUI/Bindings/`；`cargo build -p loomgui_ffi_c` 不写 Unity 目录 |
| 4 | grep 扫描无旧**目录路径**残留（`loomgui_core/` 等，区分于 `-p loomgui_core` 这种合法 package name 引用）；CI 配置路径正确 |
| 5 | `cargo test` 全绿；`tests/dotnet` 编译过 |

## 9. 风险与回滚

- **最大风险：CLAUDE.md / docs / CI 路径漂移**。缓解：阶段 4 的 grep 扫描兜底 + 用户 review。
- **gui src-tauri 布局改错** → `tauri build` 失败或 cargo 找不到 `src/main.rs`。缓解：阶段 1 验证 `tauri build --no-bundle` + exe 双击测。`git revert` 该 commit。
- **xtask path 推算错** → bindings 写到错位置。缓解：阶段 3 验证 `.cs` 落点。
- **Unity `manifest.json` 改错** → Library 缓存残留，删 `unity/showcase-unity/Library` 重导。
- **csbindgen 时序窗口**：阶段 1-3 之间 ffi build.rs 的 OUT_DIR 版本正常（Rust 测试不依赖 Unity 绑定），但 Unity 绑定需要阶段 3 的 xtask 才能生成。阶段 1-2 之间如果需要 Unity 绑定，临时保留 build.rs 直写（阶段 3 再删）。
- **package name 被误改** → `cargo -p loomgui_*` 命令失效。缓解：阶段 1 验证 `cargo build -p loomgui_core` 仍可用。
- **回滚粒度**：每阶段独立 commit，`git revert <commit>` 可单阶段回滚。

## 10. 不变保证

- **FFI ABI / .dll / exe 产物不变**：Rust 核心代码零改动（build.rs 解耦不改 ABI），`.dll` / exe 产物名不变。
- **package name 不变**：`cargo build -p loomgui_core` 等命令全保留。
- **crate 依赖拓扑不变**：`ffi → core`、`pkg → core`、`gui → pkg`。
- **Unity 包与工程的引用关系不变**：`manifest.json` `file:` 指向新位置的同一包。
- **运行时行为不变**：`FontAtlasPath` 抽取生成的字符串与裸拼接完全一致；bindings 生成内容不变（只改生成入口从 build.rs 到 xtask）。

