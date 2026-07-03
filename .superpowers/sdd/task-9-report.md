# Task 9 报告：删 samples/editor + 文档同步 + 重打 pkg.bin + build .dll

**Status**: completed
**Commit**: `e128b72`（branch: `worktree-workflow-atlas-rework`）

## 执行摘要

完成工作流闭环收尾：删除已废弃的 `samples/` 和 `editor/` 目录，将 design-systems 组件库迁入 Unity Assets，同步所有文档引用，重打 showcase pkg.bin，确认 Rust 无回归。

## Step 1: 迁 design-systems 到 Unity 内

```bash
git mv samples/design-systems/loomgui loomgui_unity/Assets/LoomUI/design-systems
```

迁入 3 个文件：`DESIGN.md`、`components.html`、`tokens.css`。历史保留。

## Step 2: 删 samples/ + editor/

```bash
git rm -r samples/ editor/
```

删除 30 个文件：editor/（10 文件：init.mjs, init.test.mjs, rules/, skill/）+ samples/（20 文件：v1-showcase/, backpack/, dyn-mail/, ai-output/。design-systems/ 已在 Step 1 迁出）。

## Step 3-6: 文档同步

| 文件 | 改动 |
|---|---|
| `CLAUDE.md` L59, L93, L105 | Workspace 成员去 editor/samples；围栏分发改 LoomUI 工作区 + Settings 面板；加设计师工作区路径说明 |
| `README.md` L28, L36, L48-52 | pkg 描述去 "+ 图集"；示例路径改 LoomUI/showcase；项目结构表删 editor/samples 行 |
| `docs/roadmap/roadmap.md` 第 3 节 | 整段重写：open-design 壳描述 → Unity 内 C# 实现（LoomSettingsWindow + LoomWorkspaceInitializer + config.json + open-design import + loomgui_pkg.exe） |
| `docs/design/fence.md` 第 5 节 | editor 消费者行改为 "Unity 插件 Editor Resources 注入（`LoomWorkspaceInitializer`）" |

## Step 7: 重打 showcase pkg.bin

命令（PowerShell）：
```
loomgui_pkg.exe "Assets/LoomUI/showcase" showcase \
  --html home.html,mail.html,page_controls.html,page_dyntree.html,page_image.html,page_interact.html,page_scroll.html,page_text.html,page_tween.html,tips_toast.html \
  --res-root "Assets/LoomUI/res" \
  -o "Assets/StreamingAssets/showcase.pkg.bin"
```

输出：`wrote showcase.pkg.bin (408342 bytes, 10 components, 4 manifest paths)` -- exit code 0。

## Step 8: build .dll

```bash
cargo build -p loomgui_ffi_c --release   # Finished in 13.72s
cp target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
```

dll 大小：1886208 bytes，与 target 一致，无 stale。

## Step 9: 围栏门

```bash
cargo test -p loomgui_core --test fence_contract
```

**10 passed, 0 failed**。

## Self-Review

- design-systems 目录的 `.meta` 文件未生成（本机无 Unity），家里机 Unity 打开 Assets 会自动生成。届时需 add + commit 三个 .meta + 父目录 .meta。
- 旧 `loom_showcase.pkg.bin` 未删 -- 未在 brief 中要求删，且已在 git 追踪中。如果家里机 Load 逻辑读的是 `showcase.pkg.bin`（新名），旧的可后续手动 `git rm`。
- CLAUDE.md 只改了 editor/samples 相关描述，未动其它部分（架构/围栏/FFI/调试/API）。
- Rust 源码未改动，`.dll` 重建仅为确认编译不过期 + 二进制一致性检查。

## Concerns

1. **`.meta` 缺失**：新迁的 `Assets/LoomUI/design-systems/` 下三个文件无对应 `.meta`。家里机 Unity 打开项目后会自动生成，需届时 add + commit。
2. **旧 pkg.bin 残留**：`StreamingAssets/loom_showcase.pkg.bin`（旧名）仍在，后续可清理。
3. **progress.md 有未暂存改动**：`.superpowers/sdd/progress.md` 被 task runner 修改但未 stage，不影响本 task 内容。

## Fix: Task 9 review findings 修复

**Status**: completed
**Commit**: （见下）

### Finding 1: `samples/` 空目录残留

`samples/` 已 `git rm -r`，git 不再 tracking，但磁盘上残留空目录 `samples/design-systems/`。
**修复**：`rm -rf samples/` 从磁盘彻底删除。git status 确认 samples/ 不再出现。

### Finding 2: `docs/design/fence.md` §4 陈旧引用

fence.md 有两处引用已删的 `editor/` 目录：

| 行 | 旧文本 | 新文本 |
|---|---|---|
| 176 | `editor 的 CLAUDE.md.tmpl / fence.md 副本同步` | `Unity 插件 Editor Resources 的 fence-rules.md 同步（LoomWorkspaceInitializer 注入）` |
| 212 | `editor 的 CLAUDE.md.tmpl 是注入给设计师的` | `Editor Resources 的 fence-rules.md 由 LoomWorkspaceInitializer 注入给设计师工作区` |

§5 表格（第 208 行）已正确引用新机制，无需改动。
grep 验证：改后 fence.md 中 `CLAUDE.md.tmpl` 和 `editor 的` 均为 0 匹配。

### Finding 3: `loom_showcase.pkg.bin` stale 文件

**grep 证据**：`LoomShowcaseDriver.cs:42` 加载 `"loom_showcase.pkg.bin"`（旧名）。
`StreamingAssets/` 下两个文件并存：`loom_showcase.pkg.bin`（旧，git 追踪）+ `showcase.pkg.bin`（新，Task 9 重打）。

**判断**：包名 `ShowcasePkg = "showcase"`，Task 9 有意改用 `showcase.pkg.bin`（新名）。改 driver 加载新名、删旧文件。

**修复**：
1. `LoomShowcaseDriver.cs:42`：`"loom_showcase.pkg.bin"` → `"showcase.pkg.bin"`
2. `git rm loomgui_unity/Assets/StreamingAssets/loom_showcase.pkg.bin`（删除旧文件）
3. 同步更新 5 个 Rust example 的硬编码路径：
   - `dump_bg.rs`（注释 + 路径）
   - `dump_img.rs`
   - `dump_scroll.rs`
   - `dump_text.rs`
   - `verify_showcase_pkg.rs`（注释）
4. grep 验证：`loomgui_core/` 和 `loomgui_unity/` 下 `loom_showcase.pkg.bin` 残量为 0。

### 围栏门

`cargo test -p loomgui_core --test fence_contract`：**10 passed, 0 failed**。无回归。
