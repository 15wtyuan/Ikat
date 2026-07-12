# 仓库目录重构设计

- **日期**：2026-07-12
- **状态**：待批准
- **关联**：`docs/report/triage/refactor.md`（结构债清单）、`CLAUDE.md`（构建/闭环命令、路径引用）

## 1. 背景与动机

当前仓库根目录平铺 4 个 Rust crate（`loomgui_core` / `loomgui_ffi_c` / `loomgui_pkg` / `loomgui_gui`）+ 2 个 Unity 目录（`loomgui_unity` / `loomgui_unity_package`）+ `docs` / `tests` / `showcase_project` / `temp` / `target`。根目录拥挤，"Rust 核心层" 与 "Unity 后端层" 两个关注域未分层，读者无法从目录一眼看出架构边界。

本次重构：

- Rust 代码收归 `crates/`，对齐 Rust 社区惯例（bevy 等）。
- Unity 代码收归 `unity/`，并把"打包工具家族"（CLI + GUI）归到 `crates/packer/`，体现"运行时核心 vs 构建期工具"的分层。
- 把 `loomgui_gui` 改成 Tauri 标准 `src-tauri/` + `frontend/` 布局。
- 顺手做几条与"位置/结构"沾边的轻量结构债（`refactor.md` 里筛选过的）。

**不是本次做的**：`refactor.md` 里的上帝方法（`tick_and_render` / `input::process` / `build_render_nodes`）、重复逻辑、死字段等纯代码内部债——它们另开 spec，配测试再动。

## 2. 范围

### In

- 目录搬迁：`crates/{core, ffi, packer/{pkg, gui}}` + `unity/{showcase-unity, package}`
- `loomgui_gui` 改 Tauri 标准 `src-tauri/` + `frontend/` 布局
- 全局路径引用同步：`Cargo.toml`、`manifest.json`、`CLAUDE.md`、`docs/`、`.github/`、`.gitignore`、csbindgen 输出路径
- 顺手 3 项 `refactor.md`：测试 helper 统一、`EventRouter.cs` 测试副本归位、`FontAtlasPath.Format()` 抽取
- Rust 测试整理：helper 归位到 `tests/common/`（ffi 测试位置**不动**，附理由）

### Out

- **package name 改名**：保持 `loomgui_core` / `loomgui_ffi_c` / `loomgui_pkg` / `loomgui_gui`，所有 `cargo -p loomgui_*` 命令不变。只改目录名。
- `refactor.md` 上帝方法 / 重复逻辑 / 死字段（含 `NodePayload` 死 enum + `program` magic number 一组）。
- `fence_contract.rs` 拆分（nice-to-have，本次不做）。
- 改用 `Samples~` 内嵌示例（保持独立 Unity 工程——需要完整工程跑 PlayMode 验收 + Kenney/Bundles 资源）。
- 任何行为 / 功能变更：纯结构搬迁，运行时行为应完全不变。

## 3. 目标目录结构

```
LoomGUI/
├── Cargo.toml          # workspace members 改为 crates/* + crates/packer/*
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
│   │   ├── build.rs    # csbindgen；输出路径核实并同步
│   │   └── src/        # lib.rs / tests.rs / abi_tests.rs / blob/ —— 位置不动
│   └── packer/
│       ├── pkg/        # ← loomgui_pkg
│       │   ├── Cargo.toml
│       │   ├── src/  tests/  examples/
│       └── gui/        # ← loomgui_gui
│           ├── Cargo.toml                 # loomgui_pkg = { path = "../pkg" }
│           ├── src-tauri/                 # ← 原 src/ + tauri.conf.json + capabilities/ + build.rs + gen/
│           └── frontend/                  # ← 原 dist/ + icons/ + templates/
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
├── tests/dotnet/       # 不动 (EventRouter.cs 改 <Link> 引用生产源)
├── docs/  temp/  target/   # 不动
```

## 4. 搬迁映射

| 旧 | 新 | 改动 |
|---|---|---|
| `loomgui_core/` | `crates/core/` | git mv |
| `loomgui_ffi_c/` | `crates/ffi/` | git mv + 改 path dep |
| `loomgui_pkg/` | `crates/packer/pkg/` | git mv + 改 path dep |
| `loomgui_gui/` | `crates/packer/gui/` | git mv + **拆 src-tauri/frontend** + 改 path dep |
| `loomgui_unity/` | `unity/showcase-unity/` | git mv + **改 manifest.json file: 路径** |
| `loomgui_unity_package/` | `unity/package/` | git mv |

**crate 间 path 依赖**（全改）：

- `crates/ffi/Cargo.toml`：`loomgui_core = { path = "../core" }`
- `crates/packer/pkg/Cargo.toml`：`loomgui_core = { path = "../../core" }`
- `crates/packer/gui/Cargo.toml`：`loomgui_pkg = { path = "../pkg" }`（gui 不直接依赖 core，只经 pkg 传递）

**根 `Cargo.toml` members**：

```toml
[workspace]
members = ["crates/core", "crates/ffi", "crates/packer/pkg", "crates/packer/gui"]
resolver = "2"
```

## 5. 路径引用同步（机械风险清单）

搬迁本身是 `git mv`，**真正的工作量和风险在引用同步**。逐项：

1. **根 `Cargo.toml` members**：见 §4。
2. **crate 间 `path =` 依赖**：见 §4，三个 crate 各自的 `Cargo.toml`。
3. **gui `src-tauri/` 布局重构**（非纯移动，要实测）：
   - `tauri.conf.json` 移进 `src-tauri/`
   - `frontendDist: "dist"` → `"../frontend"`
   - `build.rs`（tauri-build）移进 `src-tauri/`
   - `capabilities/` 移进 `src-tauri/`
   - `gen/`（生成物，gitignored）让其在新位置重生
   - 验证：`cargo tauri build --no-bundle` 出 exe，双击能开 GUI
4. **`unity/showcase-unity/Packages/manifest.json`**：`com.loomgui.unity` 的 `file:` 路径从 `file:../../loomgui_unity_package` 改为 `file:../../package`。Unity 的 `file:` 路径相对 `Packages/` 目录——工程（`unity/showcase-unity/`）与包（`unity/package/`）搬迁后仍是仓库内兄弟，从 `Packages/` 出发的相对层级 `../../` 不变，只换最后一段目录名。Unity 打开时 Library 缓存可能残留，必要时删 `unity/showcase-unity/Library` 重导。
5. **`CLAUDE.md`**（最大工作量）：构建命令、`.dll`/exe 拷贝目标路径、dump_ example 路径、所有旧目录路径——全量替换为新路径。这是漂移高发区，搬迁后必须逐条核对。
6. **`docs/`**：`report/triage/refactor.md`（行号路径）、`roadmap/`、`superpowers/specs/` 旧 spec 里提到的目录名——grep 扫描，按语义替换。
7. **`.github/workflows/`**：CI 里的 crate 路径 / artifact 路径。
8. **csbindgen 输出路径**：`crates/ffi/build.rs` 里若写死了 `loomgui_unity_package/Plugins/LoomGUI/LoomGUIBindings.cs`，改为 `unity/package/Plugins/LoomGUI/...`。执行时核实 `build.rs` 现状。
9. **`.gitignore` 白名单路径**（全量更新）：
   - `loomgui_unity/Assets/LoomUI/res/**/*.unitypackage` → `unity/showcase-unity/Assets/LoomUI/res/**/*.unitypackage`
   - `loomgui_gui/gen/` → `crates/packer/gui/src-tauri/gen/`
   - `**/Plugins/**/*.dll` 等通配规则可能不用改，但要核对。

## 6. 顺手项（refactor.md + Rust 测试整理）

| 项 | 做/不做 | 具体做法 |
|---|---|---|
| 测试 helper 统一 | ✅ | `crates/core/tests/common/mod.rs` 收 `load_html_css` / `font_path`（现 `snapshot` / `node_sort_keys` / `v1e_dirty` / `stage_getters` 4 处重复）；ffi 侧收 `stage_new_with_dejavu`（`abi_tests.rs` / `tests.rs` 2 处重复） |
| `EventRouter.cs` 测试副本归位 | ✅ | `tests/dotnet/LoomGUI.Tests.Core.csproj` 改用 `<Compile Include="..\unity\package\Runtime\LoomEventHandler.cs" Link="..." />` 引用生产源，删 `tests/dotnet/EventRouter.cs` 副本 |
| `FontAtlasPath.Format()` 抽取 | ✅ | `loomgui://font-atlas/p{n}` 现是 `LoomStage` 构造 + `SpriteResolver` 消费的隐式跨文件契约。Rust 侧 + C# 侧各抽一个 `Format(page)` 构造器，消除裸字符串拼接 |
| ffi 测试位置（`src/tests.rs` / `abi_tests.rs`） | ❌ 不动 | 核实后：是 `lib.rs` 的 `#[cfg(test)] mod tests; mod abi_tests;` **分离文件式内联测试**，能测 lib 私有项；挪到 `tests/` 会降级成"只能测公开 API 的集成测试"，丢失私有项覆盖 |
| `fence_contract.rs` 680 行拆分 | 🟡 Out | nice-to-have，本次不做 |
| examples 命名（`dump_rich` vs `dump_rich_showcase`） | 🟡 Out | 低优先，本次不做 |

## 7. 执行计划（分阶段 + worktree 隔离）

在 git worktree 里做（不阻塞 main，可随时停）。每阶段独立 commit，可单独验证 / 回滚。

- **阶段 0 — 开 worktree**：用 `superpowers:using-git-worktrees` 起隔离分支 `refactor/repo-restructure`。
- **阶段 1 — Rust 侧 `crates/` 搬迁**
  - `git mv` 4 个 crate 到新位（含 `packer/` 嵌套）
  - 改根 `Cargo.toml` members
  - 改三个 crate 间 `path =` 依赖
  - 验证：`cargo build --all-targets` + `cargo test`（workspace）全绿
  - commit
- **阶段 2 — Unity 侧 `unity/` 搬迁**
  - `git mv loomgui_unity unity/showcase-unity`、`git mv loomgui_unity_package unity/package`
  - 改 `manifest.json` 的 `file:` 路径（见 §5.4）
  - 验证：Unity 打开 `unity/showcase-unity`，Package Manager 能识别 `com.loomgui.unity`，PlayMode 不报 `.dll` 找不到
  - commit
- **阶段 3 — gui `src-tauri/` 布局**
  - 拆 `src-tauri/`（Rust 后端 + `tauri.conf.json` + `capabilities/` + `build.rs`）+ `frontend/`（`dist/` + `icons/` + `templates/`）
  - 改 `frontendDist` → `"../frontend"`
  - 验证：`cargo tauri build --no-bundle` 出 exe；拷到 `unity/package/Editor/Tools/loomgui_gui.exe` 实测能拉起
  - commit
- **阶段 4 — 全局路径引用同步**
  - `CLAUDE.md` / `docs/` / `.github/workflows/` / `.gitignore` / csbindgen `build.rs` 输出路径
  - 收尾扫描：grep 旧**目录路径**残留（如 `loomgui_core/`、`loomgui_unity_package/`、`loomgui_gui/`，即带路径分隔符的用法）在 `*.md` / `*.yml` / `*.toml` / `*.json` 无残留。**注意区分**：`cargo -p loomgui_core` 这种 **package name** 引用是合法的（包名不变），不是残留——只查"作为文件系统路径"的用法。
  - commit
- **阶段 5 — 顺手 refactor.md + Rust 测试整理**
  - helper 统一到 `tests/common/mod.rs`
  - `EventRouter.cs` 改 `<Link>`
  - `FontAtlasPath.Format()` 抽取（Rust + C#）
  - 验证：`cargo test` 全绿 + `tests/dotnet` 编译过
  - commit
- **阶段 6 — 合回 main**：fast-forward 合并 `refactor/repo-restructure`。

## 8. 验证标准（每阶段 Done 判据）

| 阶段 | Done 判据 |
|---|---|
| 1 | `cargo test`（workspace 全 crate）全绿 = Rust 侧搬迁 + 依赖正确 |
| 2 | Unity 打开 `unity/showcase-unity`，Package Manager 见 `com.loomgui.unity`，PlayMode 起得来不报 dll 缺失 |
| 3 | `cargo tauri build --no-bundle` 出 exe，双击开 GUI 能正常加载工作区 |
| 4 | grep 扫描无旧**目录路径**残留（`loomgui_core/` 等，区分于 `-p loomgui_core` 这种合法 package name 引用）；CI 配置路径正确 |
| 5 | `cargo test` 全绿；`tests/dotnet` 编译过且 `EventRouter` 测试引用生产源 |

## 9. 风险与回滚

- **最大风险：CLAUDE.md / docs 路径漂移**。缓解：阶段 4 的 grep 扫描兜底 + 用户 review。
- **Unity `manifest.json` 改错** → Library 缓存残留，删 `unity/showcase-unity/Library` 重导。
- **`src-tauri` 布局改错** → `tauri build` 失败，`git revert` 该 commit。
- **csbindgen 输出路径漏改** → `cargo build -p loomgui_ffi_c` 后 `.cs` 写到旧位置，Unity 拿不到绑定。缓解：阶段 1 编译后核对 `.cs` 落点。
- **package name 被误改** → `cargo -p loomgui_*` 命令失效。缓解：阶段 1 验证 `cargo build -p loomgui_core` 仍可用。
- **回滚粒度**：每阶段独立 commit，`git revert <commit>` 可单阶段回滚。

## 10. 不变保证

- **行为零变更**：纯 `git mv` + 引用同步，运行时行为完全不变。
- **package name 不变**：`cargo build -p loomgui_core` 等命令全保留。
- **crate 依赖拓扑不变**：`ffi → core`、`pkg → core`、`gui → pkg`。
- **Unity 包与工程的引用关系不变**：`manifest.json` `file:` 指向新位置的同一包。
- **FFI ABI / .dll 产物不变**：Rust 代码零改动，`.dll` / exe 产物名不变。
