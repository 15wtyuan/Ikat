# 仓库目录重构 + 绑定架构重组 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Rust crate 收归 `crates/`、Unity 收归 `unity/`、gui 改标准 src-tauri 布局、新增 xtask 解耦绑定生成，同时做两笔小重构（helper 统一 + FontAtlasPath 抽取）。

**Architecture:** 纯目录搬迁 + 路径引用同步为主，绑定架构重组（xtask）为辅。10 个 task、每 task 独立 commit、可单 task 回滚。所有 package name 不变，`cargo -p loomgui_*` 命令全保留。

**Tech Stack:** Rust workspace (cargo), Tauri 2 (src-tauri), csbindgen (C# 绑定生成), xtask (构建编排), Unity (UPM 包 + 工程), .NET 10 (测试)

**Spec:** `docs/superpowers/specs/2026-07-12-repo-restructure-design.md`

---

## File Structure

### 新建文件

| 文件 | 职责 |
|---|---|
| `crates/xtask/Cargo.toml` | xtask crate 清单（bin-only，dep csbindgen） |
| `crates/xtask/src/main.rs` | CLI 入口：`sync-bindings` 子命令分发 |
| `crates/xtask/src/bindings.rs` | csbindgen 生成 + 分发到 `unity/package/...` |
| `crates/core/tests/common/mod.rs` | core 集成测试共享 helper |
| `crates/ffi/src/test_helpers.rs` | ffi 内联测试共享 helper |

### 修改文件（路径引用同步）

| 文件 | 改动 |
|---|---|
| `Cargo.toml`（根） | workspace members |
| `crates/ffi/Cargo.toml` | path dep |
| `crates/packer/pkg/Cargo.toml` | path dep |
| `crates/packer/gui/src-tauri/Cargo.toml` | path dep |
| `crates/packer/gui/src-tauri/tauri.conf.json` | frontendDist |
| `crates/ffi/build.rs` | 删 Unity 直写分支 |
| `crates/ffi/src/lib.rs` | 加 `mod test_helpers`（Task 8） |
| `unity/showcase-unity/Packages/manifest.json` | file: 路径 |
| `CLAUDE.md` | 全量路径替换 + xtask 命令 |
| `.github/workflows/unity-smoke.yml` | 6 处路径 + xtask 步骤 |
| `.gitignore` | 白名单路径 |
| `tests/dotnet/LoomGUI.Tests.Core.csproj` | FrameBlob.cs `<Link>` 路径 |
| `crates/core/src/render/mod.rs` | `font_atlas_path()` 抽取（Task 9） |
| `unity/package/Runtime/LoomStage.cs` | `FontAtlasPath.Format()` 调用（Task 9） |
| `unity/package/Runtime/SpriteResolver.cs` | `FontAtlasPath` 静态类（Task 9） |

---

## Task 0: 开 worktree

**Files:** 无

- [ ] **Step 1: 创建 worktree 分支**

```bash
git worktree add ../LoomGUI-restructure -b refactor/repo-restructure
cd ../LoomGUI-restructure
```

Expected: 新 worktree 在 `../LoomGUI-restructure`，分支 `refactor/repo-restructure`。

- [ ] **Step 2: 验证起点干净**

Run: `git status`
Expected: clean working tree。

Run: `cargo test --workspace`
Expected: 全绿（确认搬迁前基线）。

---

## Task 1: Rust 侧 crates/ 搬迁 — core / ffi / pkg

纯 git mv + 改 path dep。gui 单独在 Task 2 做（因为要拆 src-tauri）。

**Files:**
- Move: `loomgui_core/` → `crates/core/`
- Move: `loomgui_ffi_c/` → `crates/ffi/`
- Move: `loomgui_pkg/` → `crates/packer/pkg/`
- Modify: `Cargo.toml`（根）
- Modify: `crates/ffi/Cargo.toml`
- Modify: `crates/packer/pkg/Cargo.toml`

- [ ] **Step 1: 创建 crates/ 目录结构**

```bash
mkdir crates
mkdir crates/packer
```

- [ ] **Step 2: git mv core**

Run: `git mv loomgui_core crates/core`
Expected: 目录移动，git 追踪 rename。

- [ ] **Step 3: git mv ffi**

Run: `git mv loomgui_ffi_c crates/ffi`

- [ ] **Step 4: git mv pkg**

Run: `git mv loomgui_pkg crates/packer/pkg`

- [ ] **Step 5: 改根 Cargo.toml members**

暂时只改已搬的 3 个 crate（gui 在 Task 2）：

```toml
[workspace]
members = ["crates/core", "crates/ffi", "crates/packer/pkg", "loomgui_gui"]
resolver = "2"
```

- [ ] **Step 6: 改 ffi path dep**

`crates/ffi/Cargo.toml`，把 `loomgui_core = { path = "../loomgui_core", default-features = false }` 改为：

```toml
loomgui_core = { path = "../core", default-features = false }
```

- [ ] **Step 7: 改 pkg path dep**

`crates/packer/pkg/Cargo.toml`，把 `loomgui_core = { path = "../loomgui_core", features = ["parse"] }` 改为：

```toml
loomgui_core = { path = "../../core", features = ["parse"] }
```

- [ ] **Step 8: 临时修 ffi build.rs 的 Unity 路径**

`crates/ffi/build.rs` 第 19 行附近，Unity 直写路径从 `"../loomgui_unity_package/..."` 改为：

```rust
let unity_bindings = "../../loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs";
```

（多一层 `..`，因为 crate 根从 `loomgui_ffi_c/` 变成了 `crates/ffi/`。Task 5 xtask 接管后删除整段。）

- [ ] **Step 9: 编译验证**

Run: `cargo build --all-targets`
Expected: 全 crate 编译通过。

Run: `cargo test --workspace`
Expected: 全绿。

- [ ] **Step 10: 验证 package name 不变**

Run: `cargo build -p loomgui_core && cargo build -p loomgui_ffi_c && cargo build -p loomgui_pkg`
Expected: 3 个都成功（package name 未被改动）。

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "refactor: move core/ffi/pkg into crates/"
```

---

## Task 2: gui 改标准 src-tauri 布局 + 搬入 crates/packer/gui/

**Files:**
- Move: `loomgui_gui/` → `crates/packer/gui/`（内部拆 src-tauri）
- Modify: `Cargo.toml`（根）
- Modify: `crates/packer/gui/src-tauri/Cargo.toml`
- Modify: `crates/packer/gui/src-tauri/tauri.conf.json`

- [ ] **Step 1: 搬 gui 到 crates/packer/gui/**

Run: `git mv loomgui_gui crates/packer/gui`

- [ ] **Step 2: 创建 src-tauri/ 子目录**

```bash
cd crates/packer/gui
mkdir src-tauri
```

- [ ] **Step 3: git mv Rust 后端文件进 src-tauri/**

Run（在 `crates/packer/gui/` 内）：

```bash
git mv Cargo.toml src-tauri/Cargo.toml
git mv build.rs src-tauri/build.rs
git mv tauri.conf.json src-tauri/tauri.conf.json
git mv capabilities src-tauri/capabilities
git mv icons src-tauri/icons
git mv templates src-tauri/templates
git mv src src-tauri/src
```

注意：
- `dist/` **不动**——留在 `crates/packer/gui/dist/`（与 src-tauri 平级）
- `gen/` 是 gitignored 产物，不 git tracked，手动 `rm -rf gen` 删掉

- [ ] **Step 4: 删旧 gen/ 目录**

Run: `rm -rf crates/packer/gui/gen`

- [ ] **Step 5: 改根 Cargo.toml members**

```toml
[workspace]
members = [
    "crates/core",
    "crates/ffi",
    "crates/packer/pkg",
    "crates/packer/gui/src-tauri",
]
resolver = "2"
```

- [ ] **Step 6: 改 gui Cargo.toml path dep**

`crates/packer/gui/src-tauri/Cargo.toml`，把 `loomgui_pkg = { path = "../loomgui_pkg" }` 改为：

```toml
loomgui_pkg = { path = "../../pkg" }
```

- [ ] **Step 7: 改 tauri.conf.json frontendDist**

把 `"frontendDist": "dist"` 改为：

```json
"frontendDist": "../dist",
```

（`bundle.icon: ["icons/icon.ico"]` 不用改——icons 随 Cargo.toml 进了 src-tauri，路径不变。）

- [ ] **Step 8: 编译验证**

Run: `cargo build --all-targets`
Expected: 全 crate 编译通过（含 gui）。

Run: `cargo test --workspace`
Expected: 全绿。

- [ ] **Step 9: Windows 上验证 tauri build（如可行）**

```bash
cd crates/packer/gui/src-tauri
tauri build --no-bundle
```

Expected: 出 exe，双击能开 GUI。

如果当前环境没装 tauri-cli，跳过此步并在 commit message 注明。

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor: move gui into crates/packer/gui with standard src-tauri layout"
```

---

## Task 3: Unity 侧 unity/ 搬迁

**Files:**
- Move: `loomgui_unity/` → `unity/showcase-unity/`
- Move: `loomgui_unity_package/` → `unity/package/`
- Modify: `unity/showcase-unity/Packages/manifest.json`

- [ ] **Step 1: 创建 unity/ 目录**

Run: `mkdir unity`

- [ ] **Step 2: git mv Unity 工程**

Run: `git mv loomgui_unity unity/showcase-unity`

- [ ] **Step 3: git mv UPM 包**

Run: `git mv loomgui_unity_package unity/package`

- [ ] **Step 4: 改 manifest.json file: 路径**

`unity/showcase-unity/Packages/manifest.json`，把 `"file:../../loomgui_unity_package"` 改为：

```json
"file:../../package"
```

- [ ] **Step 5: 临时修 ffi build.rs Unity 路径**

`crates/ffi/build.rs`，把 Task 1 Step 8 的临时路径改为：

```rust
let unity_bindings = "../../unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs";
```

（Task 5 xtask 接管后这行会被删除。）

- [ ] **Step 6: 编译验证**

Run: `cargo build -p loomgui_ffi_c`
Expected: 编译通过，csbindgen best-effort 写到新 Unity 路径。

Run: `cargo test --workspace`
Expected: 全绿。

- [ ] **Step 7: Unity 验证（手动，如环境可用）**

在 Unity Hub 打开 `unity/showcase-unity/`。验证 Package Manager 识别 `com.loomgui.unity`，PlayMode 不报 dll 缺失。Library 缓存残留时删 `unity/showcase-unity/Library` 重导。

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: move Unity project + package into unity/"
```

---

## Task 4: 新建 crates/xtask — 绑定编排 crate

**Files:**
- Create: `crates/xtask/Cargo.toml`
- Create: `crates/xtask/src/main.rs`
- Create: `crates/xtask/src/bindings.rs`
- Modify: `Cargo.toml`（根，加 xtask 到 members）

- [ ] **Step 1: 创建 xtask 目录结构**

Run: `mkdir -p crates/xtask/src`

- [ ] **Step 2: 写 crates/xtask/Cargo.toml**

```toml
[package]
name = "xtask"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "xtask"
path = "src/main.rs"

[dependencies]
csbindgen = "1"
```

- [ ] **Step 3: 写 crates/xtask/src/main.rs**

```rust
//! xtask: 构建编排工具。
//! 用法: cargo run -p xtask -- <subcommand>
//! 当前子命令: sync-bindings

mod bindings;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: cargo run -p xtask -- <subcommand>");
        eprintln!("  sync-bindings  Generate C# bindings and distribute to engine backends");
        std::process::exit(1);
    }
    match args[0].as_str() {
        "sync-bindings" => {
            if let Err(e) = bindings::sync_bindings() {
                eprintln!("sync-bindings failed: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 4: 写 crates/xtask/src/bindings.rs**

```rust
//! csbindgen 绑定生成 + 分发。
//! 路径推算: 从 xtask 的 CARGO_MANIFEST_DIR (= crates/xtask) 出发。
//! ffi 源 = ../ffi/src/lib.rs; Unity 目标 = ../../unity/package/Plugins/LoomGUI/Bindings/

use std::path::PathBuf;

fn ffi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("ffi")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("..").join("..")
}

pub fn sync_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let ffi_lib = ffi_dir().join("src").join("lib.rs");
    if !ffi_lib.exists() {
        return Err(format!("ffi lib.rs not found at {}", ffi_lib.display()).into());
    }

    let unity_target = repo_root()
        .join("unity").join("package").join("Plugins")
        .join("LoomGUI").join("Bindings").join("LoomGUIBindings.cs");

    if let Some(parent) = unity_target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    csbindgen::Builder::default()
        .input_extern_file(&ffi_lib)
        .csharp_dll_name("loomgui_ffi_c")
        .csharp_namespace("LoomGUI.Bindings")
        .csharp_class_name("Native")
        .csharp_use_function_pointer(false)
        .generate_csharp_file(&unity_target)?;

    println!("sync-bindings: Unity -> {}", unity_target.display());

    // TODO(future): cbindgen -> loomgui.h for Godot/Unreal backends

    Ok(())
}
```

- [ ] **Step 5: 加 xtask 到根 Cargo.toml**

members 末尾加 `"crates/xtask"`：

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

- [ ] **Step 6: 编译验证 xtask**

Run: `cargo build -p xtask`
Expected: 编译通过。

- [ ] **Step 7: 运行 sync-bindings 验证**

Run: `cargo run -p xtask -- sync-bindings`
Expected: 输出 `sync-bindings: Unity -> ...`，文件生成在 `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`。

- [ ] **Step 8: 验证生成的 .cs 内容正确**

Run: `head -5 unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`
Expected: csbindgen 生成的 C# 代码（`namespace LoomGUI.Bindings` 等）。

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat: add crates/xtask for binding generation orchestration"
```

---

## Task 5: ffi build.rs 解耦 — 删 Unity 直写分支

**Files:**
- Modify: `crates/ffi/build.rs`

- [ ] **Step 1: 确认 xtask sync-bindings 可独立生成绑定**

Run: `cargo run -p xtask -- sync-bindings`
Expected: 成功写入 Unity bindings。

- [ ] **Step 2: 改 build.rs 只保留 OUT_DIR**

`crates/ffi/build.rs` 改为：

```rust
fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    // 生成到 OUT_DIR（Rust 测试/编译用）。正式分发到引擎后端由 xtask 管:
    //   cargo run -p xtask -- sync-bindings
    csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .csharp_dll_name("loomgui_ffi_c")
        .csharp_namespace("LoomGUI.Bindings")
        .csharp_class_name("Native")
        .csharp_use_function_pointer(false)
        .generate_csharp_file(format!("{}/LoomGUIBindings.cs", out_dir))
        .expect("csbindgen csharp gen");
}
```

（删掉了原来的 best-effort Unity 直写分支——那段逻辑已搬到 xtask。）

- [ ] **Step 3: 编译验证**

Run: `cargo build -p loomgui_ffi_c`
Expected: 编译通过。只写 `OUT_DIR/LoomGUIBindings.cs`，不写 Unity 目录。

- [ ] **Step 4: 验证 Unity 目录不被 build.rs 碰**

```bash
# 记录 bindings 文件时间戳，重编后再比较
stat unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs
cargo clean -p loomgui_ffi_c
cargo build -p loomgui_ffi_c
stat unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs
```

Expected: 两次 stat 时间戳相同（build.rs 没碰它）。

- [ ] **Step 5: 验证 xtask 仍能正常分发**

Run: `cargo run -p xtask -- sync-bindings`
Expected: 成功更新 Unity bindings 文件（时间戳变化）。

Run: `cargo test --workspace`
Expected: 全绿。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: decouple ffi build.rs from Unity backend (xtask takes over)"
```

---

## Task 6: 全局路径引用同步

这是最大工作量的机械替换。逐文件做，每步 grep 验证。

**Files:**
- Modify: `CLAUDE.md`
- Modify: `.github/workflows/unity-smoke.yml`
- Modify: `.gitignore`
- Modify: `tests/dotnet/LoomGUI.Tests.Core.csproj`
- Modify: `docs/`（grep 扫描按语义替换）

- [ ] **Step 1: CLAUDE.md — Rust→Unity .dll 闭环**

`.dll` 拷贝命令路径 `cp target/release/loomgui_ffi_c.dll loomgui_unity_package/...` 改为 `cp target/release/loomgui_ffi_c.dll unity/package/...`。

同段的 stale .dll 诊断路径、bindings 路径说明也全改：`loomgui_unity_package/Plugins/LoomGUI/` → `unity/package/Plugins/LoomGUI/`。

新增 xtask 说明：`cargo build -p loomgui_ffi_c` 后跑 `cargo run -p xtask -- sync-bindings` 同步绑定。

- [ ] **Step 2: CLAUDE.md — GUI 打包器 exe 闭环**

`(cd loomgui_gui && tauri build --no-bundle)` 改为 `(cd crates/packer/gui/src-tauri && tauri build --no-bundle)`。

`cp target/release/loomgui_gui.exe loomgui_unity_package/Editor/Tools/...` 改为 `cp target/release/loomgui_gui.exe unity/package/Editor/Tools/...`。

- [ ] **Step 3: CLAUDE.md — 其余旧目录路径**

全局搜索 CLAUDE.md 里的 `loomgui_core/`、`loomgui_ffi_c/`、`loomgui_pkg/`、`loomgui_gui/`、`loomgui_unity/`、`loomgui_unity_package/`（带路径分隔符的用法），按语义替换。保留 `cargo -p loomgui_core` 这种 package name 引用不改。

验证：`grep -n "loomgui_core/\|loomgui_ffi_c/\|loomgui_pkg/\|loomgui_gui/\|loomgui_unity/\|loomgui_unity_package/" CLAUDE.md`
Expected: 替换后无输出（或只剩 `-p` package name 引用）。

- [ ] **Step 4: unity-smoke.yml — 6 处路径替换**

在两个 job（editmode-tests / playmode-tests）里各做相同替换：

| 旧 | 新 |
|---|---|
| `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` | `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` |
| `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` | `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` |
| `loomgui_unity/Library` | `unity/showcase-unity/Library` |
| `loomgui_unity/Packages/manifest.json` | `unity/showcase-unity/Packages/manifest.json` |
| `projectPath: loomgui_unity` | `projectPath: unity/showcase-unity` |

- [ ] **Step 5: unity-smoke.yml — 新增 xtask sync-bindings 步骤**

在每个 job 的 dll staging 步骤之后、`git add` bindings 之前，加：

```yaml
      - name: sync bindings via xtask
        run: cargo run -p xtask -- sync-bindings
```

（build.rs 不再直写 Unity bindings，CI 必须显式调 xtask。）

- [ ] **Step 6: rust-ci.yml — 核对 exclude**

gui 的 workspace member 路径变了但 package name (`loomgui_gui`) 没变，`--exclude loomgui_gui` 仍有效。xtask 是纯 Rust 无系统依赖，不用排除。

验证：`cargo clippy --all-targets --workspace --exclude loomgui_gui -- -D warnings`
Expected: 通过。

- [ ] **Step 7: .gitignore — 白名单路径**

`!loomgui_unity/Assets/LoomUI/res/**/*.unitypackage` 改为 `!unity/showcase-unity/Assets/LoomUI/res/**/*.unitypackage`。
`!loomgui_unity/Assets/LoomUI/res/**/*.unitypackage.meta` 改为 `!unity/showcase-unity/Assets/LoomUI/res/**/*.unitypackage.meta`。
`loomgui_gui/gen/` 改为 `crates/packer/gui/src-tauri/gen/`。

- [ ] **Step 8: tests/dotnet csproj — FrameBlob.cs Link 路径**

`<Compile Include="..\..\loomgui_unity_package\Runtime\FrameBlob.cs" .../>` 改为 `<Compile Include="..\..\unity\package\Runtime\FrameBlob.cs" .../>`。

验证：`dotnet test tests/dotnet/LoomGUI.Tests.Core.csproj`
Expected: 编译通过 + 测试全绿。

- [ ] **Step 9: docs/ 路径扫描**

Run: `grep -rn "loomgui_core/\|loomgui_ffi_c/\|loomgui_pkg/\|loomgui_gui/\|loomgui_unity/\|loomgui_unity_package/" docs/`

逐条按语义替换。注意：历史 spec/plan 是当时快照，如果路径引用是历史描述可保留；当前路径引用则替换。

- [ ] **Step 10: 收尾全量扫描**

```bash
grep -rn "loomgui_core/\|loomgui_ffi_c/\|loomgui_pkg/\|loomgui_gui/\|loomgui_unity/\|loomgui_unity_package/" \
  --include="*.md" --include="*.yml" --include="*.toml" --include="*.json" --include="*.csproj" \
  . | grep -v "target/" | grep -v ".superpowers/"
```

Expected: 无输出（或只剩有意识保留的历史引用）。

Run: `cargo test --workspace`
Expected: 全绿。

Run: `cargo clippy --all-targets --workspace --exclude loomgui_gui -- -D warnings`
Expected: 通过。

Run: `cargo fmt --all -- --check`
Expected: 通过。

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "refactor: sync all path references to new directory structure"
```

---

## Task 7: core 测试 helper 统一到 tests/common/mod.rs

**Files:**
- Create: `crates/core/tests/common/mod.rs`
- Modify: `crates/core/tests/snapshot.rs`
- Modify: `crates/core/tests/node_sort_keys.rs`
- Modify: `crates/core/tests/stage_getters.rs`
- Modify: `crates/core/tests/v1e_dirty.rs`

- [ ] **Step 1: 写 tests/common/mod.rs**

```rust
//! 集成测试共享 helper。各 tests/*.rs 用 `mod common; use common::*;` 引入。

use loomgui_core::parse::css::parse_css;
use loomgui_core::parse::dom::parse_html;
use loomgui_core::scene::node::build_scene;
use loomgui_core::stage::Stage;
use loomgui_core::style::cascade::resolve_styles;

/// 测试字体路径: 仓库内 DejaVuSans.ttf，跨平台一致。
pub fn font_path() -> String {
    format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// 缺字体时 skip (返 true = 跳过，不算失败)。
pub fn skip_if_no_font(font: &str) -> bool {
    if std::fs::read(font).is_err() {
        eprintln!("skip: no font at {}", font);
        return true;
    }
    false
}

/// HTML+CSS -> scene (parse_html + resolve_styles + build_scene)，直注入 Stage。
pub fn load_html_css(stage: &mut Stage, html: &str, css: &str) {
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    stage.tweens.clear();
    if let Some(scene) = stage.scene.as_mut() {
        scene.scroll.clear();
    }
    stage.prev_node_hashes.clear();
    stage.scene = Some(build_scene(&tree, &styles));
}
```

- [ ] **Step 2: 改 snapshot.rs — 删本地 helper，用 common**

删掉文件内的 `test_font_path()`、`skip_if_no_font()`、`load_html_css()` 和顶部 `use loomgui_core::parse::...` import（common 模块已内含）。

在文件顶部加：

```rust
mod common;
use common::*;
```

调用处 `test_font_path()` 全改成 `font_path()`（统一命名）。

- [ ] **Step 3: 改 node_sort_keys.rs**

删 `font_path()`、`load_html_css()`、顶部 use import。加 `mod common; use common::*;`。

- [ ] **Step 4: 改 stage_getters.rs**

删 `font_path()`、`load_html_css()`、顶部 use import。加 `mod common; use common::*;`。

注意：如果测试体直接引用 `NodeId`，保留 `use loomgui_core::scene::node::NodeId`。

- [ ] **Step 5: 改 v1e_dirty.rs**

v1e_dirty 的 `font_path()` 返回 `(String, usize)`（路径 + 长度），签名不同。删它和 `load_html_css()`、顶部 use import。加 `mod common; use common::*;`。

调用处原来 `let (font, n) = font_path()` 改成：

```rust
let font = common::font_path();
let n = font.len();
```

- [ ] **Step 6: 编译验证**

Run: `cargo build -p loomgui_core --tests`
Expected: 编译通过（common 模块被 4 个集成测试正确引用）。

- [ ] **Step 7: 测试验证**

Run: `cargo test -p loomgui_core`
Expected: 全绿（所有集成测试行为不变）。

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: unify core test helpers into tests/common/mod.rs"
```

---

## Task 8: ffi 测试 helper 统一到 src/test_helpers.rs

ffi helper 访问 crate 私有项 (`StageHandle`)，不能放进 `tests/`。用 `src/` 内 `#[cfg(test)]` 模块。

**Files:**
- Create: `crates/ffi/src/test_helpers.rs`
- Modify: `crates/ffi/src/lib.rs`（加 mod 声明）
- Modify: `crates/ffi/src/tests.rs`
- Modify: `crates/ffi/src/abi_tests.rs`

- [ ] **Step 1: 写 src/test_helpers.rs**

```rust
//! ffi 内联测试共享 helper。用 `use crate::*` 访问 extern "C" 函数 + 私有 StageHandle。

use crate::*;

/// 通过 FFI 创建 Stage 并注册测试默认 DejaVu 字体。Panic 即测试失败。
pub(crate) fn stage_new_with_dejavu(w: f32, h: f32) -> *mut StageHandle {
    let h = loomgui_stage_new(w, h);
    assert!(!h.is_null(), "stage_new must succeed");
    let font_bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../core/tests/fixtures/DejaVuSans.ttf"
    ))
    .expect("DejaVuSans.ttf fixture must exist");
    let family = b"DejaVu";
    let rc = loomgui_stage_register_font(
        h,
        family.as_ptr(),
        family.len(),
        font_bytes.as_ptr(),
        font_bytes.len(),
        1,
    );
    assert_eq!(rc, 0, "register_font DejaVu must return 0");
    h
}
```

注意 fixture 路径：原来 `/../loomgui_core/tests/fixtures/...` 改成 `/../core/tests/fixtures/...`（core 与 ffi 是 `crates/` 下兄弟，一层 `..`）。

- [ ] **Step 2: 在 lib.rs 加 mod 声明**

`crates/ffi/src/lib.rs` 底部（`#[cfg(test)] mod abi_tests;` 之后）加：

```rust
#[cfg(test)]
mod test_helpers;
```

- [ ] **Step 3: 改 tests.rs**

删 `stage_new_with_dejavu` 定义。文件顶部 `use super::*;` 之后加：

```rust
use crate::test_helpers::stage_new_with_dejavu;
```

- [ ] **Step 4: 改 abi_tests.rs**

删 `stage_new_with_dejavu` 定义。文件顶部加：

```rust
use crate::test_helpers::stage_new_with_dejavu;
```

- [ ] **Step 5: 编译验证**

Run: `cargo build -p loomgui_ffi_c --tests`
Expected: 编译通过。

- [ ] **Step 6: 测试验证**

Run: `cargo test -p loomgui_ffi_c`
Expected: 全绿（fixture 路径正确找到 DejaVuSans.ttf）。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: unify ffi test helper into src/test_helpers.rs"
```

---

## Task 9: FontAtlasPath 构造器抽取

**Files:**
- Modify: `crates/core/src/render/mod.rs`
- Modify: `unity/package/Runtime/SpriteResolver.cs`
- Modify: `unity/package/Runtime/LoomStage.cs`

- [ ] **Step 1: Rust 侧 — 加 font_atlas_path() 函数**

`crates/core/src/render/mod.rs`，在模块顶部合适位置加：

```rust
/// Font-atlas image_path for a given page index. Consumed verbatim by the
/// Unity backend's SpriteResolver (LoomStage.RegisterFontAtlasPage) — this
/// string format is an ABI-level contract across the FFI boundary.
pub(crate) fn font_atlas_path(page: usize) -> String {
    format!("loomgui://n/p{page}")
}
```

- [ ] **Step 2: Rust 侧 — 替换两处裸拼接**

`crates/core/src/render/mod.rs` 里：
1. `let path0 = format!("loomgui://n/p{}", page0);` → `let path0 = font_atlas_path(page0);`
2. `let sub_path = format!("loomgui://n/p{}", page);` → `let sub_path = font_atlas_path(page);`

验证：`grep -n 'loomgui://n/p' crates/core/src/render/mod.rs`
Expected: 无裸拼接残留（只剩 `font_atlas_path()` 定义）。

- [ ] **Step 3: Rust 侧验证**

Run: `cargo test -p loomgui_core`
Expected: 全绿（生成的字符串一模一样，snapshot 无变化）。

Run: `cargo test -p loomgui_core --test snapshot`
Expected: snapshot 测试全绿（证明 image_path 字符串未变）。

- [ ] **Step 4: C# 侧 — 加 FontAtlasPath 静态类**

`unity/package/Runtime/SpriteResolver.cs`，在 `SpriteResolver` 类之前加：

```csharp
/// <summary>
/// Font-atlas image_path construction. Must match Rust render::font_atlas_path
/// (image_path field in blob). Changing the format here requires changing both sides.
/// </summary>
public static class FontAtlasPath
{
    public static string Format(int page) => $"loomgui://n/p{page}";
}
```

- [ ] **Step 5: C# 侧 — LoomStage.cs 替换裸拼接**

行 268 附近 `string path = $"loomgui://n/p{page}";` 改为：

```csharp
string path = FontAtlasPath.Format(page);
```

- [ ] **Step 6: C# 侧验证**

Run: `cargo run -p xtask -- sync-bindings`（确保 bindings 最新），然后在 Unity 里跑 PlayMode 验证文本渲染正常。

Run: `cargo test --workspace`
Expected: 全绿。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: extract font_atlas_path() / FontAtlasPath.Format() constructors"
```

---

## Task 10: 合回 main

**Files:** 无

- [ ] **Step 1: 确认全量验证通过**

```bash
cargo test --workspace
cargo clippy --all-targets --workspace --exclude loomgui_gui -- -D warnings
cargo fmt --all -- --check
dotnet test tests/dotnet/LoomGUI.Tests.Core.csproj
```

Expected: 全绿。

- [ ] **Step 2: 确认路径扫描无残留**

```bash
grep -rn "loomgui_core/\|loomgui_ffi_c/\|loomgui_pkg/\|loomgui_gui/\|loomgui_unity/\|loomgui_unity_package/" \
  --include="*.md" --include="*.yml" --include="*.toml" --include="*.json" --include="*.csproj" \
  . | grep -v "target/" | grep -v ".superpowers/"
```

Expected: 无输出。

- [ ] **Step 3: 合回 main**

```bash
cd <main worktree>
git merge refactor/repo-restructure
```

Expected: fast-forward 合并。如果有冲突（main 在期间被推进），反向 merge main 进 feature 分支解冲突。

- [ ] **Step 4: 清理 worktree**

```bash
git worktree remove ../LoomGUI-restructure
git branch -d refactor/repo-restructure
```

- [ ] **Step 5: 最终验证**

在 main 分支上运行 `cargo test --workspace`，Expected: 全绿。
