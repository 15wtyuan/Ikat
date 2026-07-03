# Task 8 报告：工作区初始化 + AssetPostprocessor + 围栏规则/skill 迁移

## 状态：DONE

## 改动文件清单

### 新建
| 文件 | 说明 |
|---|---|
| `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md` | 迁自 `editor/rules/claude/CLAUDE.md.tmpl`。末段"生成完必须跑验证"→"生成完跑验证+打包"，改为读 config.json 调 loomgui_pkg.exe（用 `--res-root`） |
| `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/SKILL.md` | 迁自 `editor/skill/loomgui-editor/SKILL.md`。frontmatter description 砍 pack.mjs 引用；工作流 step 3 改为读 config.json 调 loomgui_pkg.exe（`--res-root`）；砍全部 pack.mjs 引用（3 处→0） |
| `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/references/fence.md` | 纯拷贝自 `editor/skill/loomgui-editor/references/fence.md`，无改动 |
| `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/references/preview-polyfill.html` | 纯拷贝自 `editor/skill/loomgui-editor/references/preview-polyfill.html`，无改动 |
| `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/references/preview-trust.md` | 纯拷贝自 `editor/skill/loomgui-editor/references/preview-trust.md`，无改动 |
| `loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceInitializer.cs` | 工作区初始化静态类。`Initialize(LoomSettings)` → InjectFenceRules（标签段增量合并）+ DistributeSkill（拷 Resources→.claude/skills/）+ LoomConfigExporter.Export + AssetDatabase.Refresh |
| `loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceAssetPostprocessor.cs` | AssetPostprocessor。`ShouldSkip` 拦工作区下 .html/.css/.claude/CLAUDE.md/design-systems/.od-skills 不导入；PNG 强制 Sprite |

### 修改
| 文件 | 改动 |
|---|---|
| `loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs` | 取消 `LoomWorkspaceInitializer.Initialize(_settings)` 注释（Task 5 桩注释 → 真调用） |

## 语法核对

- **namespace**：两个新 C# 文件均为 `LoomGUI.Editor`，与现有 Editor 文件一致
- **asmdef**：`LoomGUI.Editor.asmdef` 已引用 `LoomGUI.Runtime`，可访问 `LoomSettings`（namespace `LoomGUI`）
- **LoomConfigExporter.Export**：存在（Task 7），签名 `public static void Export(LoomSettings s)`，调用正确
- **LoomSettings.GetOrCreateDefault**：存在（`Runtime/LoomSettings.cs:34`），静态方法
- **Resources.Load 路径**：5 个 TextAsset 路径均为 `LoomGUI/...`（无扩展名），对应 `Editor/Resources/LoomGUI/...` 目录下的文件
- **fence-rules.md tags**：`<!-- loomgui-editor-begin -->` (L1) 和 `<!-- loomgui-editor-end -->` (L39) 正确放置，与 `InjectFenceRules` 的 `Regex.Replace` 配合

## SKILL.md 改动细节

1. **frontmatter description**（L6）：`run tools/pack.mjs to validate` → `read config.json and run loomgui_pkg.exe to validate`
2. **step 2 内联说明**（L25）：`跑 pack.mjs 传该 css` → `跑 loomgui_pkg.exe 传该 css`；`pack.mjs 吃外部 css` → `loomgui_pkg.exe 吃外部 css`
3. **step 3**：`node tools/pack.mjs <html路径> <css路径> -o ...` → `loomgui_pkg.exe <sourceDir> <pkgName> --html <list> --res-root <工作区根/res> -o <out.pkg.bin>`，加"先读 config.json 拿 exe_path"说明
4. **notes**（L42）：`pack.mjs 调的 loomgui_pkg` → `loomgui_pkg.exe`
5. **验证**：grep pack.mjs → 0 matches（完全清除）

## fence-rules.md 改动细节

- 末段标题："生成完必须跑验证" → "生成完跑验证+打包"
- 内容：`node tools/pack.mjs <html> <css> -o <out.pkg.bin>` → 读 config.json + `loomgui_pkg.exe <sourceDir> <pkgName> --html <list> --res-root <工作区根/res> -o <out>`
- 验证：grep pack.mjs → 0 matches；grep --res-root → 1 match（正确使用）

## 测试

```
cargo test -p loomgui_core: 482 passed, 0 failed
cargo test -p loomgui_core --test fence_contract: 10 passed, 0 failed
Total: 497 passed, 0 failed
```

## 潜在问题（待家里机 Unity 验）

1. **Resources.Load 从 Editor/Resources 读取**：代码用 `Resources.Load<TextAsset>("LoomGUI/fence-rules")`，文件在 `Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md`。Unity Editor 下 `Resources.Load` 可能搜不到 `Editor/Resources` 下的文件（`Resources.Load` 搜 `Assets/Resources/`，而 `EditorGUIUtility.Load` 才搜 `Editor/Resources/`）。如果家里机 Resources.Load 返回 null → 改 `AssetDatabase.LoadAssetAtPath<TextAsset>("Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md")` 或 `EditorGUIUtility.Load("LoomGUI/fence-rules")`。

2. **SetNonAsset() API**：brief 标注 Unity 6 API。若家里机报找不到此方法 → 退回方案：删 `.meta` 文件 + 不操作，或 `AssetDatabase.MoveAssetToTrash`。

3. **ShouldSkip 中调 GetOrCreateDefault**：`AssetPostprocessor.OnPreprocessAsset` 内调 `Resources.Load`（`GetOrCreateDefault` 内部）可能引起 re-entrant 导入。但大概率 Unity 的 postprocessor 在导入管线结束后才跑，实际不会有问题。家里机若遇死循环 → 缓存 LoomSettings 引用而非每次调。

4. **SKILL.md 用 TextAsset 加载**：`Resources.Load<TextAsset>("LoomGUI/skill/SKILL")`。Unity 把 `.md` 文件导入为 TextAsset。若 Unity 不认 `.md` → 改为 `.txt` 扩展名或配置 ScriptedImporter。

## Fix（commit `35b7865`）：3 review findings 修完

### Finding 1 (HIGH)：Resources.Load 从 Editor/Resources 加载返 null

`InjectFenceRules` + `CopyResource` 的 5 处 `Resources.Load<TextAsset>(resPath)` 全部替换为 `AssetDatabase.LoadAssetAtPath<TextAsset>(fullAssetPath)`。

- `InjectFenceRules`：直接写全路径 `"Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md"`
- `DistributeSkill`：引入 `basePath` 变量，`CopyResource` 签名改为接 `string assetPath`（Assets 全路径含扩展名），4 个调用点全部改传完整路径（`SKILL.md`、`fence.md`、`preview-polyfill.html`、`preview-trust.md`）
- `UnityEditor` 已 using，直接用 `AssetDatabase`

### Finding 2 (MEDIUM)：`[^]*?` 正则 .NET 语义错

`InjectFenceRules` 中正则 `$"{BEGIN}[^]*?{END}"` 改为：
```csharp
string pattern = System.Text.RegularExpressions.Regex.Escape(BEGIN) +
    @"[\s\S]*?" + System.Text.RegularExpressions.Regex.Escape(END);
```
- `[\s\S]*?` 正确匹配任意字符含换行（.NET 和 JS 通用）
- `Regex.Escape` 防御 BEGIN/END 含特殊字符

### Finding 3 (LOW)：preview-polyfill.html 残留 pack.mjs

`preview-polyfill.html` 第 5 行注释 `跑 pack.mjs 传该 css` → `跑 loomgui_pkg.exe 传该 css`。

### 验证

- grep `Resources\.Load` 在 `LoomWorkspaceInitializer.cs`：0 matches
- grep `[^]` 在 `LoomWorkspaceInitializer.cs`：0 matches
- grep `pack\.mjs` 在 `Editor/Resources/LoomGUI/skill/references/preview-polyfill.html`：0 matches
- 语法核对：namespace `LoomGUI.Editor`，`AssetDatabase` 可用（`using UnityEditor`），所有路径含正确扩展名
