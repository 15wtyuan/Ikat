# Spec-4b P1：旧范式残留清理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Spec-4b §2 查证出的旧范式残留（RichText 死链 / set_style 死路径 / Controller 全链 / LoomEventHandler 旧 demux / 旧 Demo / 文档漂移）一次性清干净，pkg 格式升 v19，全工程编译 + 测试不回归。

**Architecture:** 纯删除 + pkg schema bump，无新功能代码。按依赖序：Rust 侧（core/FFI/packer/schema）→ 重编 dll + sync bindings → C# 侧（wrappers/EventHandler）→ 产物/文档。deferred ①②（core bug 修复）留 P2，不在此 plan。

**Tech Stack:** Rust（core/ffi/packer crates）+ C#（Unity Runtime）+ csbindgen FFI + bincode pkg schema。

**对照 spec：** `docs/superpowers/specs/2026-07-18-spec4b-unity-acceptance-and-backend-retirement-design.md` §2 / §4.2-4.4 / §7。

## Global Constraints

- Rust edition 2021，依赖钉版本（CLAUDE.md）。
- 改 Rust FFI 后必须重编 dll + sync bindings + 拷贝：`cargo build -p loomgui_ffi_c --release` → `cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（**Unity 必须关着**）+ `cargo run -p xtask -- sync-bindings`。
- pkg 格式版本一刀切：`MIN_VERSION = MAX_VERSION = 19`，弃 v18，无迁移器（roadmap P0.3）。
- 删 FFI 连 csbindgen 生成的 C# binding 一起删（sync-bindings 后自动消失）。
- **保留**（别误删）：`text/rich.rs` 算法、`apply_css` 函数本体、`set_inline_override`/`unset_inline_override`、`EventRouter.cs`（算法参考）。
- 用户只读中文——问答用中文；代码/commit 英文。
- 每个 task 末尾 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` 必须清（CLAUDE.md CI 门）。

---

## File Structure（P1 涉及）

**Rust 删除/修改：**
- `crates/core/src/scene/node.rs` — 删 `rich_fragments` 字段、`Controller`/`ControllerChangedEvent` struct、`Scene.controllers`/`pending_controller_events`、`set_controller_selected`/`controller_selected`、`Node.data_controller` 字段
- `crates/core/src/stage.rs` — 删 `rich_link_at`、`rich_fragments` 回写、`Stage::set_style`、`get_controller`/`set_selected_index`/`get_selected_index`/`controller_changed_events`、instantiate 填 controller、tick 清 pending
- `crates/core/src/scene/dynamic.rs` — 删 `set_style` 函数（留 `apply_css`）
- `crates/core/src/render/mod.rs` — 删 `rich_fragments` 返回值 init + 回写（恒空）
- `crates/core/src/asset/mod.rs` — bump `PKG_FORMAT_VERSION` 18→19，删 `ControllerEntry`、`ComponentTemplate.controllers`、`TemplateNode.data_controller`、Controller schema 读写
- `crates/core/examples/dump_controller.rs` — 整文件删
- `crates/ffi/src/lib.rs` — 删 `rich_link_at`/`set_style`/4 个 Controller FFI
- `crates/ffi/src/tests.rs` — 删 Controller FFI 测试
- `crates/ffi/src/abi_tests.rs` — 删 set_style 测试
- `crates/packer/pkg/src/bridge.rs` — 删 data-controller attr parse + controllers 抽取

**C# 删除/修改（dll sync 后）：**
- `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` — sync-bindings 自动删（rich_link_at/set_style/Controller FFI binding）
- `unity/package/Runtime/LoomStage.cs` — 删 `SetStyle`/`GetController`/`SetSelectedIndex`/`GetSelectedIndex` wrapper + tick 内 ControllerChanged dispatch
- `unity/package/Runtime/LoomEventHandler.cs` — **整文件删**（Click 分支简化不再需要，Controller 删后无事件源）
- `unity/package/Tests/LoomEventHandlerTests.cs` — 整文件删
- `tests/dotnet/Stubs/LoomEventTypes.cs` / `tests/dotnet/EventRouter.cs` — 注释更新（EventRouter 保留作参考）

**产物/文档：**
- `unity/showcase-unity/Assets/Scripts/Demo/` — 整目录删（.bak + asmdef + meta）
- `unity/showcase-unity/Assets/Scenes/SampleScene.unity:608` — 清 broken MonoBehaviour ref
- `.gitignore:22-23` — 删死代码（LoomUI 目录例外）
- `docs/design/projection-layer.md:35,50` — flush 描述改 set_inline_override
- `docs/roadmap/roadmap.md:48` — stale dll 描述修

---

## Task 1: 删 dump_controller.rs 诊断 example

**Files:**
- Delete: `crates/core/examples/dump_controller.rs`

**Interfaces:** 无（孤立 example，无 CI 引用）。

- [ ] **Step 1: 确认无引用**

Run: `grep -rn "dump_controller" crates/ docs/ --include="*.rs" --include="*.md"`
Expected: 仅 `crates/core/examples/dump_controller.rs` 自身 + AGENTS.md/CLAUDE.md 标注"R5 后退役"（文档引用，删 example 后改文档标注）。

- [ ] **Step 2: 删文件**

Run: `git rm crates/core/examples/dump_controller.rs`

- [ ] **Step 3: 确认 core 仍编译**

Run: `cargo build -p loomgui_core`
Expected: 编译通过（example 不进 lib）。

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "chore(core): drop dump_controller example (Controller retired)"
```

---

## Task 2: 删 RichText rich_link_at 死链（Rust 侧）

> rich_fragments side table 恒空（Spec-2 RichText 退役），rich_link_at 永远返 0。删整条死链。**保留 `text/rich.rs` 算法本体**（RichWeight/RichRun/RichFragment 类型，layout/render 在用）。

**Files:**
- Modify: `crates/core/src/scene/node.rs` — 删 `rich_fragments: Vec<Option<Vec<RichFragment>>>` 字段 + init
- Modify: `crates/core/src/stage.rs` — 删 `rich_link_at` 方法（~line 342-351）+ `rich_fragments` 回写循环（~line 829-850）
- Modify: `crates/core/src/render/mod.rs` — 删 `rich_fragments` 返回值 init（`Vec::new()`）+ 传递
- Modify: `crates/ffi/src/lib.rs` — 删 `loomgui_stage_rich_link_at` FFI（~line 1267-1279）

**Interfaces:**
- Produces: `rich_link_at` FFI 消失 → C# binding（Task 8 sync 后自动删）+ `LoomEventHandler` Click 分支调用变编译断（Task 9 修）。

- [ ] **Step 1: grep rich_fragments / rich_link_at 全引用**

Run: `grep -rn "rich_fragments\|rich_link_at" crates/ --include="*.rs"`
Expected: 命中 node.rs（字段+init）、stage.rs（方法+回写）、render/mod.rs（init+传递）、ffi/lib.rs（FFI）、可能的测试。记录全部命中点。

- [ ] **Step 2: 删 ffi/lib.rs 的 `loomgui_stage_rich_link_at` FFI**

删 `crates/ffi/src/lib.rs` 中整个 `#[no_mangle] pub extern "C" fn loomgui_stage_rich_link_at(...)` 函数（含 doc 注释，~line 1267-1279）。

- [ ] **Step 3: 删 stage.rs 的 `rich_link_at` 方法 + rich_fragments 回写**

删 `crates/core/src/stage.rs`：
- `pub fn rich_link_at(...)` 方法（~342-351，DFS 扫 scene.rich_fragments）
- tick/render 内 rich_fragments 回写循环（~829-850，把 render 返回的 rich_fragments 写回 scene——恒空）

- [ ] **Step 4: 删 render/mod.rs 的 rich_fragments 返回值**

`render` 函数返回类型含 `rich_fragments`（init 为 `Vec::new()`，从不写入）。删该返回字段 + 所有传递点（caller 解构）。`text/rich.rs` 的 `RichFragment`/`RichRun` 类型**保留**。

- [ ] **Step 5: 删 node.rs 的 rich_fragments 字段**

删 `crates/core/src/scene/node.rs` 的 `pub rich_fragments: Vec<Option<Vec<RichFragment>>>` 字段（~398）+ Scene::new / build 的 init（~511, ~541）。

- [ ] **Step 6: 删相关测试**

grep 测试中的 rich_fragments/rich_link_at 引用，删（这些测的是死路径）。

- [ ] **Step 7: 编译 + 测**

Run: `cargo build -p loomgui_core && cargo build -p loomgui_ffi_c && cargo test -p loomgui_core`
Expected: 编译通过，测试绿（rich_fragments 删除不影响活的 layout/render 文本路径，rich.rs 算法保留）。

- [ ] **Step 8: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 清。

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor(core): drop RichText rich_link_at dead chain (rich_fragments always empty since Spec-2)"
```

---

## Task 3: 删 set_style FFI 死路径（Rust 侧，留 apply_css）

> active C# 零 caller（4a StyleMirror 走 set_inline_override）。删 FFI + Stage::set_style + dynamic::set_style + 测试。**保留 `apply_css` 函数**（create_node 烘焙 base_style 在用）。

**Files:**
- Modify: `crates/ffi/src/lib.rs` — 删 `loomgui_stage_set_style` FFI（~1307-1328）
- Modify: `crates/core/src/stage.rs` — 删 `Stage::set_style`（~563）
- Modify: `crates/core/src/scene/dynamic.rs` — 删 `pub fn set_style`（~266-268）
- Modify: `crates/ffi/src/abi_tests.rs` — 删 `set_style btn ok` 测试（~587-592）
- Modify: `crates/core/src/scene/dynamic.rs` 测试 — 删 `set_style_changes_base_style_marks_dirty`（~1068-1075）

**Interfaces:**
- Produces: `set_style` FFI 消失 → C# binding（Task 8 sync 后删）+ `LoomStage.SetStyle` wrapper 编译断（Task 9 删）。`apply_css` 保留给 `create_node`。

- [ ] **Step 1: 确认 apply_css 仍被 create_node 用**

Run: `grep -n "apply_css" crates/core/src/scene/dynamic.rs`
Expected: `apply_css` 定义（~43）+ `create_node` 调用（~75）+ `create_root` 调用 + `set_style` 调用（~268，即将删）。确认删 set_style 后 apply_css 仍有 caller（create_node/create_root）。

- [ ] **Step 2: 删 dynamic::set_style 函数**

删 `crates/core/src/scene/dynamic.rs` 的 `pub fn set_style(node: &mut Node, css: &str) { apply_css(&mut node.base_style, css); }`（~266-268）。**不删 apply_css**。

- [ ] **Step 3: 删 Stage::set_style**

删 `crates/core/src/stage.rs` 的 `pub fn set_style(&mut self, node: NodeId, css: &str)`（~563，转调 dynamic::set_style）。

- [ ] **Step 4: 删 FFI**

删 `crates/ffi/src/lib.rs` 的 `#[no_mangle] pub extern "C" fn loomgui_stage_set_style(...)`（~1307-1328，含 doc）。

- [ ] **Step 5: 删 2 个测试**

删 `crates/ffi/src/abi_tests.rs` 的 `set_style btn ok` 测试（~587-592）+ `crates/core/src/scene/dynamic.rs` 的 `set_style_changes_base_style_marks_dirty` 单测（~1068-1075）。

- [ ] **Step 6: 编译 + 测**

Run: `cargo build -p loomgui_ffi_c && cargo test -p loomgui_core && cargo test -p loomgui_ffi_c`
Expected: 绿。

- [ ] **Step 7: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "refactor(core): drop set_style FFI dead path (4a uses set_inline_override; keep apply_css for create_node)"
```

---

## Task 4: 删 Controller core 全链（Rust 侧）

> Controller（data-controller/data-page，v1.5 停止）core 全链。新表层已干净（fence 无 data-controller）。删 core state 机 + stage methods + instantiate 填充 + tick 清 pending + Rust 测试。**这是删 LoomEventHandler 的源 2 前置**（borrow_controller_changed_events FFI 在 Task 5 删）。

**Files:**
- Modify: `crates/core/src/scene/node.rs` — 删 `Node.data_controller` 字段（~242）+ `Controller` struct（~349-357）+ `ControllerChangedEvent` struct（~358-367）+ `Scene.controllers: HashMap`（~406）+ `Scene.pending_controller_events: Vec`（~409）+ `set_controller_selected`/`controller_selected`（~570, ~581）
- Modify: `crates/core/src/stage.rs` — 删 `get_controller`/`set_selected_index`/`get_selected_index`/`controller_changed_events`（~159-210）+ instantiate 填 `data_controller`（~672）+ instantiate 注册 controllers（~695-697）+ tick 清 pending_controller_events（~767）

**Interfaces:**
- Consumes: Task 6（pkg schema 删 data_controller）会改 TemplateNode，本 task 删 Node.data_controller 字段需配合（instantiate 不再从 template 填）。
- Produces: core 无 Controller state → Task 5 删 Controller FFI 有依据。

- [ ] **Step 1: grep Controller core 全引用**

Run: `grep -rn "data_controller\|ControllerChangedEvent\|controllers\b\|pending_controller_events\|set_controller_selected\|controller_selected\|get_controller\|set_selected_index\|get_selected_index\|controller_changed_events" crates/core/src/ --include="*.rs"`
Expected: 命中 node.rs / stage.rs / asset/mod.rs（schema，Task 6 处理）。记录 core 内命中。

- [ ] **Step 2: 删 stage.rs 的 4 个 Controller 方法**

删 `crates/core/src/stage.rs` 的 `get_controller` / `set_selected_index` / `get_selected_index` / `controller_changed_events`（~159-210）。

- [ ] **Step 3: 删 stage.rs instantiate 的 Controller 填充 + 注册**

删 instantiate 函数内：填 `node.data_controller`（~672）+ 注册 controllers 到 Scene.controllers（~695-697）。instantiate 不再处理 controller。

- [ ] **Step 4: 删 stage.rs tick 的 pending 清理**

删 tick 内清空 `pending_controller_events` 的行（~767）。

- [ ] **Step 5: 删 node.rs 的 Controller structs + Scene state + Node 字段**

删 `crates/core/src/scene/node.rs`：
- `pub data_controller: Option<String>` 字段（~242，在 Node struct）
- `pub struct Controller { ... }`（~349-357）
- `pub struct ControllerChangedEvent { ... }`（~358-367，`#[repr(C)]` POD）
- `Scene.controllers: HashMap<NodeId, Controller>`（~406）
- `Scene.pending_controller_events: Vec<ControllerChangedEvent>`（~409）
- `Scene::set_controller_selected`（~570）
- `Scene::controller_selected`（~581）
- Scene::new / build 内 controllers/pending_controller_events 的 init

⚠️ ControllerChangedEvent 是 `#[repr(C)]` POD——删它连带 Task 5 的 borrow_controller_changed_events FFI（返回它）。

- [ ] **Step 6: 删 core 内 Controller 测试**

grep `crates/core/src/` 或 `crates/core/tests/` 的 Controller 测试，删。

- [ ] **Step 7: 编译（预期 borrow_controller_changed_events FFI 还在但 ControllerChangedEvent 删了 → FFI 编译断）**

Run: `cargo build -p loomgui_core`
Expected: **core lib 编译通过**（FFI 在 ffi crate，core 不依赖 FFI）。

Run: `cargo build -p loomgui_ffi_c`
Expected: **编译断**（borrow_controller_changed_events FFI 引用已删的 ControllerChangedEvent + controller_changed_events 方法）——这是预期的，Task 5 删该 FFI 后恢复。

- [ ] **Step 8: 暂不 commit（Task 5 删 FFI 后一起 commit，避免中间断态）**

记录进度，进 Task 5。

---

## Task 5: 删 Controller FFI（Rust 侧）

> 接 Task 4 的编译断——删 4 个 Controller FFI + FFI 测试，恢复编译。

**Files:**
- Modify: `crates/ffi/src/lib.rs` — 删 4 个 FFI（~301-392）
- Modify: `crates/ffi/src/tests.rs` — 删 6 个 Controller 测试 fn（~15-175）

**Interfaces:**
- Produces: Controller FFI 全消失 → Task 8 sync-bindings 后 C# binding 删 → Task 9 删 LoomStage wrappers + LoomEventHandler 的 ControllerChanged 路径。

- [ ] **Step 1: 删 4 个 Controller FFI**

删 `crates/ffi/src/lib.rs`：
- `loomgui_stage_borrow_controller_changed_events`（~309-333）
- `loomgui_stage_get_controller`（~340-361）
- `loomgui_stage_set_selected_index`（~368-379）
- `loomgui_stage_get_selected_index`（~385-392）

- [ ] **Step 2: 删 FFI Controller 测试**

删 `crates/ffi/src/tests.rs` 的 6 个 Controller test fn（~15-175，grep `controller` 定位）。

- [ ] **Step 3: 编译 + 测（应恢复绿）**

Run: `cargo build -p loomgui_ffi_c && cargo test -p loomgui_core && cargo test -p loomgui_ffi_c`
Expected: 全绿（Task 4 + 5 合起来 core/FFI 无 Controller 残留）。

- [ ] **Step 4: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 5: Commit（Task 4 + 5 合一）**

```bash
git add -A && git commit -m "refactor(core,ffi): drop Controller (data-controller v1.5 retired) — core state + 4 FFIs + tests"
```

---

## Task 6: 删 Controller pkg schema + bump v19

> pkg.bin v18 schema 还序列化 ControllerEntry + TemplateNode.data_controller。删 schema + bump PKG_FORMAT_VERSION 18→19（一刀切，弃 v18）+ 重打 HeadlessTests fixture。

**Files:**
- Modify: `crates/core/src/asset/mod.rs` — bump version + 删 ControllerEntry struct + ComponentTemplate.controllers + TemplateNode.data_controller + Controller schema 读写 + PackageInput.controllers
- Modify: `crates/core/src/asset/mod.rs` 测试 — pkg schema 测试更新
- Regenerate: `tests/dotnet/LoomGUI.HeadlessTests/fixtures/test.pkg.bin`（v19）

**Interfaces:**
- Produces: pkg v19（ControllerEntry 不再序列化）→ 所有 v18 pkg.bin 拒载（含 fixture，需重打）。

- [ ] **Step 1: bump version**

改 `crates/core/src/asset/mod.rs:21-23`：
```rust
pub const PKG_FORMAT_VERSION: u32 = 19; // v19: drop Controller schema (ControllerEntry/controllers/data_controller) — data-controller v1.5 retired
pub(crate) const MIN_VERSION: u32 = 19;
pub(crate) const MAX_VERSION: u32 = 19;
```

- [ ] **Step 2: 删 ControllerEntry struct**

删 `crates/core/src/asset/mod.rs:46-54`（`pub struct ControllerEntry { name, mount_node_idx, initial_selected_index }`）。

- [ ] **Step 3: 删 ComponentTemplate.controllers + TemplateNode.data_controller**

删 `ComponentTemplate.controllers: Vec<ControllerEntry>` 字段（~43）+ `TemplateNode.data_controller: Option<String>` 字段（~67-69）+ 注释。

- [ ] **Step 4: 删 PackageInput.controllers**

`PackageInput` components tuple 含 `&[ControllerEntry]`（~76-82）。改 tuple 去掉 ControllerEntry 元素。**注意**：这改了 `write_package` 签名，packer caller（Task 7）要跟进。

- [ ] **Step 5: 删 write_package/read_package 的 Controller 序列化段**

grep `Controller` in asset/mod.rs，删：
- write_package 内 ControllerSection 写（subagent 报 ~218-221, ~280-281）
- read_package 内 ControllerSection 读（~430-439）
- ComponentTable 内 controller 相关字段（若有）

删干净后 pkg 布局不含 PerComponent Controllers（改 asset/mod.rs:5 注释）。

- [ ] **Step 6: 更新 pkg schema 测试**

`crates/core/src/asset/mod.rs` 内 schema 测试 + bincode 稳定性测试——更新断言（无 Controller 字段）。加 BadVersion 测试（v18 pkg 拒载，验 MIN=19）。

- [ ] **Step 7: 编译 + core 测**

Run: `cargo build -p loomgui_core && cargo test -p loomgui_core`
Expected: 绿（write_package 签名变了，packer 在 Task 7 修，core 内部一致）。

- [ ] **Step 8: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor(core): bump pkg v18→v19, drop Controller schema (ControllerEntry/controllers/data_controller)"
```

---

## Task 7: 删 packer bridge data-controller attr parse

> packer bridge 仍 parse data-controller attr 填 TemplateNode.data_controller（白抽不用）。Task 6 删了 TemplateNode.data_controller + PackageInput.controllers，bridge 要跟进。

**Files:**
- Modify: `crates/packer/pkg/src/bridge.rs` — 删 data-controller attr parse（~63）+ controllers 抽取 + 调 write_package 的新签名

**Interfaces:**
- Consumes: Task 6 的 write_package 新签名（无 ControllerEntry）。

- [ ] **Step 1: 读 bridge.rs 确认 data-controller 处理**

Run: `grep -n "data_controller\|controller\|ControllerEntry" crates/packer/pkg/src/bridge.rs`
Expected: ~13 注释（controllers 恒空）+ ~63 attr parse + write_package 调用处。

- [ ] **Step 2: 删 attr parse + controllers 抽取**

删 `crates/packer/pkg/src/bridge.rs`：data-controller attr 解析（~63）+ controllers Vec 构造 + 注释（~13）。

- [ ] **Step 3: 改 write_package 调用为新签名**

bridge 调 `write_package(PackageInput { components: Vec<(&str, &[TemplateNode], &DynamicRuleTable, &[ControllerEntry])> })`——去掉 ControllerEntry 元素，对齐 Task 6 的新 PackageInput。

- [ ] **Step 4: 编译 + packer 测**

Run: `cargo build -p loomgui_pkg && cargo test -p loomgui_pkg`
Expected: 绿（bridge 不再产 controller，pkg v19）。

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor(packer): drop data-controller attr parse + controllers (pkg v19)"
```

---

## Task 8: 【Rust gate】重编 dll + sync bindings + 重打 fixture

> Rust 侧（Task 2-7）删了 rich_link_at/set_style/Controller FFI。重编 dll + sync bindings 让 C# 侧 binding 同步消失。重打 HeadlessTests fixture pkg.bin 到 v19。

**Files:**
- Regenerate: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`
- Regenerate: `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`
- Regenerate: `tests/dotnet/LoomGUI.HeadlessTests/fixtures/test.pkg.bin`

**Interfaces:** 无（产物同步）。

- [ ] **Step 1: 确认 Unity 关着（dll 拷贝要求）**

问用户 / 检查 Unity 进程未运行。Unity 开着则等关。

- [ ] **Step 2: 重编 dll**

Run: `cargo build -p loomgui_ffi_c --release`
Expected: 编译通过，产 `target/release/loomgui_ffi_c.dll`。

- [ ] **Step 3: 拷贝 dll**

Run: `cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`

- [ ] **Step 4: sync bindings**

Run: `cargo run -p xtask -- sync-bindings`
Expected: `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` 重新生成，**不再含** `loomgui_stage_rich_link_at` / `loomgui_stage_set_style` / 4 个 Controller FFI 的 `[DllImport]`。

- [ ] **Step 5: 验证 binding 删除**

Run: `grep -c "rich_link_at\|set_style\|borrow_controller_changed_events\|get_controller\|set_selected_index\|get_selected_index" unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`
Expected: `0`（全删）。

- [ ] **Step 6: 重打 HeadlessTests fixture pkg.bin（v19）**

fixture 源 HTML 在 `tests/dotnet/LoomGUI.HeadlessTests/fixtures/`（确认源）。用 `loom-pkg build` 重打到 v19。具体：
```bash
cargo run -p loomgui_pkg -- build <fixture-workspace>
cp <output>/test.pkg.bin tests/dotnet/LoomGUI.HeadlessTests/fixtures/test.pkg.bin
```
（fixture 构建方式以实际 harness 为准——若 fixture 是手搓 TemplateNode 而非 HTML 打包，则改测试代码构造 v19 pkg。）

- [ ] **Step 7: dll md5 一致性检查**

Run: `md5sum target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`
Expected: 两个 md5 相等（CLAUDE.md stale .dll 诊断）。

- [ ] **Step 8: Rust 全测验证**

Run: `cargo test`
Expected: 全绿（core + ffi + fence + packer 全过）。

- [ ] **Step 9: Commit**

```bash
git add unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs tests/dotnet/LoomGUI.HeadlessTests/fixtures/test.pkg.bin
git commit -m "chore(ffi,pkg): rebuild dll v19 + sync bindings (drop rich_link_at/set_style/controller) + regenerate fixture pkg v19"
```

---

## Task 9: 删 LoomStage Controller/set_style/rich wrappers + EventHandler 连锁

> C# 侧：删 LoomStage 的 SetStyle/GetController/SetSelectedIndex/GetSelectedIndex wrapper + tick 内 ControllerChanged dispatch；删 LoomEventHandler 整文件 + tests（rich_link_at Click 分支 + Controller 删后无事件源）；改 LoomStage tick 去掉 EventHandler 调用。

**Files:**
- Modify: `unity/package/Runtime/LoomStage.cs` — 删 SetStyle/GetController/SetSelectedIndex/GetSelectedIndex wrapper + tick 内 `_eventHandler.DispatchPending`/`DispatchControllerChanged` + `_eventHandler` 字段/构造/EventHandler 属性 + Click 分支不再调 rich_link_at（rich_link_at FFI 已删）
- Delete: `unity/package/Runtime/LoomEventHandler.cs` — 整文件
- Delete: `unity/package/Tests/LoomEventHandlerTests.cs` — 整文件
- Modify: `tests/dotnet/Stubs/LoomEventTypes.cs` / `tests/dotnet/EventRouter.cs` — 注释更新（EventRouter 保留作参考实现）

**Interfaces:**
- Consumes: Task 8 sync 后 binding 已无 rich_link_at/set_style/controller。
- Produces: LoomStage 无业务 wrapper（剩后端编排，P2 LoomHost 接管）；旧 demux 消失。

> ⚠️ **LoomStage 类本身在 P2 退役**（§3 多引擎分层）。本 task 只删它依赖已删 FFI 的 wrapper + EventHandler 连锁，让 C# 编译通过。LoomStage 后端编排方法（Tick/RegisterFont/...）保留到 P2。

- [ ] **Step 1: 删 LoomStage 的 4 个 wrapper**

删 `unity/package/Runtime/LoomStage.cs`：`SetStyle`（~550）、`GetController`（~315）、`SetSelectedIndex`（~325）、`GetSelectedIndex`（~332）。

- [ ] **Step 2: 改 LoomStage.Tick 去掉 EventHandler 调用**

`LoomStage.cs` Tick（~217-229）原：
```csharp
byte* evPtr = Native.loomgui_stage_borrow_events(_stage, &evLen);
_eventHandler.DispatchPending((System.IntPtr)evPtr, (int)evLen);   // 旧 AddListener
_eventDemuxer?.Pump((System.IntPtr)evPtr, (int)evLen);             // D3 typed
// ...
byte* ccPtr = Native.loomgui_stage_borrow_controller_changed_events(_stage, &ccLen);
_eventHandler.DispatchControllerChanged((System.IntPtr)ccPtr, (int)ccLen);
```
改为（删 EventHandler 两行 + 整个 Controller 块，borrow_controller_changed_events FFI 已删）：
```csharp
byte* evPtr = Native.loomgui_stage_borrow_events(_stage, &evLen);
_eventDemuxer?.Pump((System.IntPtr)evPtr, (int)evLen);             // D3 typed 路径（On<T>）
```

- [ ] **Step 3: 删 LoomStage 的 _eventHandler 字段 + 构造 + EventHandler 属性**

删 `LoomStage.cs`：`readonly LoomEventHandler _eventHandler = new();`（~43）+ 构造内 `_eventHandler.SetHandle(...)`（~62）+ `public LoomEventHandler EventHandler => _eventHandler;`（~74）。

- [ ] **Step 4: 删 LoomEventHandler 整文件 + tests**

Run: `git rm unity/package/Runtime/LoomEventHandler.cs unity/package/Tests/LoomEventHandlerTests.cs`
（含 .meta 文件：`git rm unity/package/Runtime/LoomEventHandler.cs.meta unity/package/Tests/LoomEventHandlerTests.cs.meta`，若存在）

- [ ] **Step 5: 更新 Stubs/EventRouter 注释**

`tests/dotnet/Stubs/LoomEventTypes.cs` + `tests/dotnet/EventRouter.cs`：若有引用 LoomEventHandler 的注释/类型，改为"参考实现，非生产依赖"。EventRouter 本体保留（EventBus 算法参考）。

- [ ] **Step 6: C# 编译验证（编码机 dotnet build）**

Run: `cd tests/dotnet && dotnet build`（或 unity package 对应的 dotnet 编译命令）
Expected: 编译通过（无 LoomEventHandler 引用残留）。

- [ ] **Step 7: Headless 测验证**

Run: `cd tests/dotnet && dotnet test`
Expected: 300 HeadlessTests 绿（UIContext 路径不依赖 LoomEventHandler）。

- [ ] **Step 8: fmt + clippy（C# 无 clippy，跳过；确认无 Rust 改动）**

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor(c#): drop LoomEventHandler + LoomStage set_style/controller wrappers (rich_link_at/set_style/controller FFI gone)"
```

---

## Task 10: 删旧 Demo 目录 + SampleScene ref + gitignore 死代码

**Files:**
- Delete: `unity/showcase-unity/Assets/Scripts/Demo/`（LoomShowcaseDriver.cs.bak + .meta + LoomGUI.Demo.asmdef + .meta + Demo.meta）
- Modify: `unity/showcase-unity/Assets/Scenes/SampleScene.unity:608` — 清 broken MonoBehaviour ref
- Modify: `.gitignore:22-23` — 删 LoomUI 目录死例外

**Interfaces:** 无（死代码/死引用）。

- [ ] **Step 1: 删 Demo 目录**

Run: `git rm -r unity/showcase-unity/Assets/Scripts/Demo/`
（含 .meta：`git rm unity/showcase-unity/Assets/Scripts/Demo.meta`）

- [ ] **Step 2: 清 SampleScene broken ref**

`unity/showcase-unity/Assets/Scenes/SampleScene.unity:608` 引用 `LoomGUI.Demo::LoomGUI.LoomShowcaseDriver` MonoBehaviour（GUID）。用文本编辑/YAML 改场景文件，删该 MonoBehaviour 组件条目（fileId + GUID 引用）。或场景在 Unity 打开后手动删 missing script（但编码机 Unity 关着——文本改）。

- [ ] **Step 3: 删 gitignore 死例外**

`.gitignore:22-23` 的 `!unity/showcase-unity/Assets/LoomUI/res/**/*.unitypackage`（LoomUI 目录已不存在）删掉。

- [ ] **Step 4: 确认无残留引用**

Run: `grep -rn "LoomShowcaseDriver\|LoomGUI.Demo" unity/ --include="*.cs" --include="*.unity" --include="*.asmdef"`
Expected: 空（除已删文件）。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "chore(unity): drop legacy showcase demo (LoomShowcaseDriver.cs.bak + asmdef) + clean SampleScene ref + gitignore dead rule"
```

---

## Task 11: 修文档漂移

**Files:**
- Modify: `docs/design/projection-layer.md:35,50` — flush 描述改 set_inline_override
- Modify: `docs/roadmap/roadmap.md:48` — stale dll 描述修

**Interfaces:** 无（文档）。

- [ ] **Step 1: 修 projection-layer.md §1.3 + §2.2**

`docs/design/projection-layer.md:35`：v1 回走命令式 FFI `SetStyle`/`SetText`/`AppendChild`/`Tween` 的描述——4a 后 Style 走 `set_inline_override`（便签层），不走 set_style。改为：
> v1 回写走命令式 FFI 透传（结构操作 `AppendChild`/`Instantiate`、资源 `SetSrc`、动画 `Tween`...）；**Style 属性走 inline override 便签层**（`set_inline_override`/`unset_inline_override`，4a 落地），不走 `set_style`（set_style 写 base_style 污染设计期基线，已退役）。

`:50` §2.2 "Style flush 复用字符串 set_style（零改动）"——改为：
> Style flush 拼 CSS 串过桥，Rust `apply_css` parse 到 **inline override 层**（`set_inline_override`，4a 新建，非 set_style）。标记优化点不变。

- [ ] **Step 2: 修 roadmap.md:48 stale 描述**

`docs/roadmap/roadmap.md:48`："dll + csbindgen 绑定比 Rust 源码过期（load_html / set_rich_text 已删源码、dll 里还在，旧 showcase 仍调）"——已过时（bindings 已删，b03929a 修了 caller）。改为：
> ~~dll/bindings stale~~ 已同步（4a 重编 + b03929a 修 stale caller）。P0.1 此项完成。

- [ ] **Step 3: Commit**

```bash
git add docs/design/projection-layer.md docs/roadmap/roadmap.md
git commit -m "docs: fix drift — projection-layer flush→set_inline_override, roadmap stale-dll resolved"
```

---

## Task 12: 【P1 gate】全量验证

> P1 清理完成后，全量验证不回归。这是 P1 完成的 gate。

- [ ] **Step 1: Rust 全测**

Run: `cargo test`
Expected: 全绿（core + ffi + fence + packer）。

- [ ] **Step 2: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 清。

- [ ] **Step 3: feature-gate check**

Run: `cargo clippy --all-targets --no-default-features -D warnings`（按 CLAUDE.md CI 门）
Expected: 清。

- [ ] **Step 4: C# Headless 测 + PublicApi 编译门**

Run: `cd tests/dotnet && dotnet test && dotnet build LoomGUI.PublicApi`（以实际 csproj 为准）
Expected: 300 HeadlessTests 绿 + PublicApi 编译通过。

- [ ] **Step 5: dll md5 一致**

Run: `md5sum target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`
Expected: 相等。

- [ ] **Step 6: pkg v19 验证**

Run: `cargo test -p loomgui_core pkg`（schema 测试）
Expected: v19 读写绿，v18 拒载（BadVersion）。

- [ ] **Step 7: 残留扫零**

Run:
```bash
grep -rn "rich_link_at\|set_rich_text\|data_controller\|ControllerEntry\|ControllerChanged\|LoomEventHandler\|AddListener" crates/ unity/package/Runtime/ unity/package/Tests/ --include="*.rs" --include="*.cs" | grep -v "\.bak"
```
Expected: 空（或仅 docs/pitfalls.md 历史索引 + EventRouter 参考注释）。

- [ ] **Step 8: P1 完成 commit（如有零散改动）**

```bash
git add -A && git commit -m "test: P1 cleanup gate green — cargo test + headless + PublicApi + dll synced + pkg v19 + zero residue" --allow-empty
```

---

## P1 完成标准

- ✅ cargo test 全绿 / fmt+clippy 清 / feature-gate 清
- ✅ HeadlessTests 300 绿 + PublicApi 编译门
- ✅ dll md5 synced / pkg v19 / fixture 重打
- ✅ 旧范式残留扫零（rich_link_at/set_style/Controller/EventHandler/Demo）
- ✅ 文档漂移修

**下一棒**：P2（多引擎分层 LoomHost/LoomBackend/UnityLoomBackend + LoomStage 退役 + deferred ①②）——待写 P2 plan。
