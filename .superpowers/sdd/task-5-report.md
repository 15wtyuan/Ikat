# Task 5 Report: LoomSettingsWindow 三 tab 面板

## Status: done

## Commit

`dbf694d` -- `feat(unity): LoomSettingsWindow 三 tab 设置面板（替代 PackageManager）`

## Files changed

| File | Action |
|---|---|
| `loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs` | **Created** (207 lines) |
| `loomgui_unity/Assets/LoomGUI/Editor/LoomExePath.cs` | **Created** (16 lines) |
| `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs` | Already deleted (Task 1, commit 334cef4) |
| `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs.meta` | Already deleted (Task 1) |

## 三处桩注释位置

| 位置 | 文件行 | TODO |
|---|---|---|
| LoomSettingsWindow.cs:62 | `OnGUI()` changed block | `// LoomConfigExporter.Export(_settings);` -- Task 7 |
| LoomSettingsWindow.cs:78 | `DrawWorkspace()` init button | `// LoomWorkspaceInitializer.Initialize(_settings);` -- Task 8 |
| LoomSettingsWindow.cs:131 | `DrawAtlas()` sync button | `// LoomAtlasSync.SyncAll(_settings);` -- Task 6 |

## 语法核对

- **namespace**: LoomSettingsWindow/LoomExePath 在 `LoomGUI.Editor`；LoomSettings/PackageEntry/AtlasEntry 在 `LoomGUI`。C# 子 namespace 可解析父 namespace 类型，无需 `using LoomGUI;`。
- **asmdef 引用**: `LoomGUI.Editor.asmdef` 引用 `LoomGUI.Runtime`，能访问 LoomSettings 等 Runtime 类型。
- **using 清单**: LoomSettingsWindow 有完整 using（System/Diagnostics/IO/Text/UnityEditor/UnityEngine）；LoomExePath 有 System.IO。
- **未引用未实现类**: LoomConfigExporter、LoomAtlasSync、LoomWorkspaceInitializer 三处调用已注释。

## fence_contract

`cargo test -p loomgui_core --test fence_contract` -- **10 passed, 0 failed**。Rust 核心无回归。

## 测试未跑说明

本机无 Unity，C# 无法编译。依赖项：
- 语法核对：done（grep 核对 enum/namespace/using/方法签名）
- Unity 编译：需家里机 PlayMode 验收时 Unity 自动编译
- .meta 文件：Unity 首次打开项目时自动生成（未手动创建，避免 GUID 冲突）

## Self-review

1. LoomSettingsWindow 完全按 brief 代码写入，三处调用已注释并标注 TODO Task 6/7/8。
2. LoomExePath 完整实现，无外部依赖。
3. 旧 LoomPackageManagerWindow 已在 Task 1 删除，无需重复操作。
4. fence_contract 10/10 通过。

## Concerns

无。代码按 brief 精确写入，三处桩注释清晰标注对应 task，Task 6/7/8 实现后取消注释即可。

## Fix

修复 review 两个 finding。

### Finding 1: `--res` → `--res-root`

**根因**: Task 2 把 CLI 从 `--res <name>` 改成了 `--res-root <path>`，但 `PackPackage` 仍传 `--res`，CLI 会报 `unknown arg` exit(2) 打包失败。

**修法**: 删除 `sb.Append(" --res ").Append(_settings.resDirName);`，改为传 res-root 绝对路径：

```csharp
string resRoot = ToAbs(Path.Combine(_settings.workspaceDir, _settings.resDirName));
sb.Append(" --res-root \"").Append(resRoot).Append("\"");
```

CLI 默认 `source_dir.join("res")` 在 res 不在 sourceDir 下时找不到（如 `showcase/res`），必须显式传工作区根下的 res 绝对路径。

**验证**: `grep -n "res-root\|--res " LoomSettingsWindow.cs` 确认仅 `--res-root`，无残留 `--res `。

### Finding 2: Process stdout/stderr 死锁

**根因**: stdout 设了 `RedirectStandardOutput=true` 但从没读。大包 stdout 超 4KB 缓冲 → 进程写 stdout 阻塞 → C# 等 stderr EOF → 死锁。

**修法**: 先读 stdout 再读 stderr，最后 WaitForExit：

```csharp
string stdout = p.StandardOutput.ReadToEnd();
string stderr = p.StandardError.ReadToEnd();
p.WaitForExit();
if (!string.IsNullOrEmpty(stdout)) AppendLog($"  stdout: {stdout.Trim()}");
AppendLog(p.ExitCode == 0 ? $"[pack] {pkg.pkgName}: OK" : $"[pack] {pkg.pkgName}: FAIL\n{stderr}");
```

### Commit

`e299737` -- fix(unity): PackPackage --res → --res-root + stdout/stderr 死锁修复
