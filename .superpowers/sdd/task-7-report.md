# Task 7 Report: config.json 导出 + exe 随插件发布

**Status**: Done
**Commit**: `406e52a` on branch `worktree-workflow-atlas-rework`

---

## 改了什么

| 文件 | 操作 | 说明 |
|---|---|---|
| `loomgui_unity/Assets/LoomGUI/Editor/LoomConfigExporter.cs` | 新建 | BuildJson + Export + RelativeFromWorkspace |
| `loomgui_unity/Assets/LoomGUI/Tests/LoomConfigExporterTests.cs` | 新建 | Export_PathsRelativeToWorkspace 单测（1 个） |
| `loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs` | 修改 | 取消 `LoomConfigExporter.Export(_settings)` 桩注释（line 61） |
| `loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe` | 新建 | release build exe，1.3 MB |

## cargo build

```
cargo build --release -p loomgui_pkg -> Finished `release` profile in 27.93s
```

## exe 拷贝确认

```
ls -l loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe
-rwxr-xr-x ... 1309696 Jul 3 18:21 loomgui_pkg.exe
```

## .gitignore 检查

`.gitignore` 无 `.exe` 排除规则。`/**/*.dll` 规则只影响 `.dll`，不影响 `.exe`。exe 直接入库，无需白名单。

## 语法核对

代码按 brief 完整写入。`LoomConfigExporter` 在 `namespace LoomGUI.Editor`，引 `LoomSettings`（`namespace LoomGUI`，Runtime 层）。`BuildJson` 用 `RelativeFromWorkspace` 算相对路径，`Export` 写 `.claude/skills/loomgui-editor/config.json`。

## 取消桩注释 grep

```
loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs:61:
    LoomConfigExporter.Export(_settings);
```

已取消注释，`// TODO Task 7` 行已移除。

## fence_contract

```
cargo test -p loomgui_core --test fence_contract
running 10 tests
all 10 passed; 0 failed
```

## self-review

- C# 代码照 brief 逐字写入，无 deviation。
- `RelativeFromWorkspace` 用 `Uri.MakeRelativeUri`，Windows 路径转 Uri（`file://` 前缀 + 斜杠统一）正确。
- `.exe` 的 `.meta` 未生成（本机无 Unity），Unity 首次打开项目会自动生成。不影响功能。
- Unity 测试无法在本机跑（无 Unity），C# 编译由家里机 Unity 验证。代码结构与已有测试（LoomAtlasSyncTests.cs 等）一致。
- 无 Rust 回归（fence_contract 10/10 通过，纯 C# 改动不触 Rust）。

## Concerns

1. **Unity 测试未跑** -- 本机无 Unity，C# 编不了。`Export_PathsRelativeToWorkspace` 依赖于 `ScriptableObject.CreateInstance`（Unity API）+ `Application.dataPath`，须 Unity Test Runner 里验证。代码逻辑与 brief 一致，但家里机验收是必需步骤。
2. **exe 无 .meta** -- Unity 打开项目会自动生成 `.meta` 文件，届时需 commit。不影响运行。

---

## Fix: RelativeFromWorkspace trailing slash + test 断言修正

**Commit**: `(pending)`

### 根因

`RelativeFromWorkspace` 里 `Path.GetFullPath` 会剥末尾 `/`，导致 `MakeRelativeUri` 把 to 当文件而非目录：

- `from` 已补 trailing slash：`.../Assets/LoomUI/`（目录语义正确）
- `to` 原 `Assets/StreamingAssets/` 经 `Path.GetFullPath` 变成 `.../Assets/StreamingAssets`（无 trailing slash，被当文件）

`MakeRelativeUri` 产 `../StreamingAssets`（无 trailing slash）。

### 修复

**LoomConfigExporter.cs**：`to` 补 trailing slash 的逻辑——在 `Path.GetFullPath` 之前记录 `targetDir` 是否以 `/` 或 `\` 结尾（`toIsDir`），之后若 `toIsDir && !to.EndsWith("/")` 则补 `/`。

`exe_path` 的 to = `Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe` 不以 `/` 结尾 → `toIsDir=false` → 不补 slash，产 `../LoomGUI/Editor/Tools/loomgui_pkg.exe`（文件，正确）。

**LoomConfigExporterTests.cs**：`output_dir` 断言从 `../../StreamingAssets/` 改为 `../StreamingAssets/`。

### 逻辑推演

- `from` = `Assets/LoomUI/`（工作区根）
- `to` = `Assets/StreamingAssets/`（pkgOutputDir）
- 两者都在 `Assets/` 下，是兄弟目录
- 相对路径 = `../`（上一级到 `Assets/`）+ `StreamingAssets/` = `../StreamingAssets/`
- 1 级 `../`，非 2 级。brief 原断言 `../../StreamingAssets/` 是手工算错。

### 语法核对

- `targetDir.EndsWith("/")` / `targetDir.EndsWith("\\")` -- 读取参数原值（`Path.GetFullPath` 前），正确。
- `toIsDir` 在 `Path.GetFullPath` 调用**前**赋值 -- 变量在作用域内，C# 顺序执行，不会用未初始化值。
- `Uri.MakeRelativeUri` 接受 `file:///...` scheme URI -- Windows 路径转 URI 正确（`Path.GetFullPath` 产 `C:/...`，`new Uri(...)` 自动加 `file://`）。
