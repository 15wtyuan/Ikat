# LoomGUI Editor 工具 & C# 测试 深度代码审查报告

> 审查日期：2026-07-09
> 审查范围：`loomgui_unity_package/Editor/`（7 文件）+ `Tests/`（10 文件）+ `loomgui_core/tests/fence_contract.rs`

---

## 一、LoomSettingsWindow.cs（577 行）

### 1.1 【严重】JSON 值无转义 → 生成的 config.json 可能是非法 JSON

**位置**：`LoomConfigExporter.cs:26-47`

```csharp
sb.Append($"\"name\": \"{p.pkgName}\", ");
sb.Append($"\"source\": \"{p.sourceDir}\", ");
// ...
sb.Append($"\"{p.htmlFiles[j]}\"");
```

`p.pkgName`、`p.sourceDir`、`p.htmlFiles[j]` 直接内插进 JSON 字符串。若任一值含 `"`、`\`、`\n` 等特殊字符，生成的 JSON 非法。目前 `pkgName` 取自目录名（通常不含特殊字符），但 `sourceDir` 和 `htmlFiles` 从文件系统读入，**路径或文件名含引号/反斜杠即崩**。

**修复方向**：用 `System.Text.Json.JsonSerializer.Serialize(s)` 序列化整个对象，或至少用 `Newtonsoft.Json`/`System.Web.HttpUtility.JavaScriptStringEncode` 对每个字符串值做 JSON 转义。

**严重级别**：🔴 严重（数据正确性）

---

### 1.2 【严重】命令行参数拼接无转义 → 路径含空格/引号时 exe 调崩

**位置**：`LoomSettingsWindow.cs:358-364`

```csharp
sb.Append('"').Append(absSrc).Append("\" ").Append(pkg.pkgName);
// ...
sb.Append(" --res-root \"").Append(resRoot).Append("\"");
```

Windows 路径标准确含空格时，`ProcessStartInfo` 的 `Arguments` 属性直接用空格分隔参数。此实现手动用引号包裹路径，但 `absSrc`、`resRoot`、`htmlFiles` 内部若含 `"`（程序文件目录名不太可能，但非零风险）会破坏引号配对。此外 `htmlArg`（`string.Join(",", pkg.htmlFiles)`）**完全没引号包裹**，文件名含特殊字符即崩。

**修复方向**：使用 `ProcessStartInfo` 的 `ArgumentList` 集合（.NET 6+）自动转义，或至少用 `System.CommandLine` 构造参数。

**严重级别**：🔴 严重（运行时正确性）

---

### 1.3 【中等】Tab 间存在重复的 SetDirty+Save+Export 模式，但未统一用 SaveSettings()

**位置**：
- `LoomSettingsWindow.cs:136-138`（SmartRecognizeDir）
- `LoomSettingsWindow.cs:184-186`（RefreshPackage）
- `LoomSettingsWindow.cs:223-225`（DrawAtlasEntry 同步此图集）

```csharp
EditorUtility.SetDirty(_settings);
AssetDatabase.SaveAssetIfDirty(_settings);
LoomConfigExporter.Export(_settings);
```

类内已有 `SaveSettings()` 封装（L453-458），但上述三处仍手写内联。字体 tab 的拖拽和删除用了 `SaveSettings()`，但 atlas 删除（L228-233）手写内联。不一致会增加漏写风险。

**修复方向**：所有改 `_settings` 的地方统一调 `SaveSettings()`；或者让 `LoomSettings.OnValidate()` / `ISerializationCallbackReceiver` 自动触发 Export。

**严重级别**：🟡 中等（可维护性）

---

### 1.4 【中等】`DirectoryDropField` 函数过长（41 行）、嵌套 4 层

**位置**：`LoomSettingsWindow.cs:465-507`

```csharp
string DirectoryDropField(string label, string value, string relativeBase)
{
    // 41 行，内嵌嵌套 if + drag-drop 状态机
    if (drop.Contains(Event.current.mousePosition))
    {
        if (Event.current.type == ...)
        {
            foreach (var p in DragAndDrop.paths)
            {
                if (!Directory.Exists(p)) continue;
                if (relativeBase != null)
                {
                    if (string.IsNullOrEmpty(relativeBase)) { ... }
                    // ...
                }
                else value = NormalizeDroppedDir(p);
```

内含 drag-updated、drag-perform、路径相对化、清除按钮 四个职责混在一体。`DrawPackageDropZone`（L96-114）和 `HandleFolderDrop`（L244-264）也有类似但不同的拖拽处理逻辑——三者共享"目录拖拽"模式但没提取公共 helper。

**修复方向**：提取 `DragDropDirectoryArea(Rect drop, Action<string> onDrop, Predicate<string> filter)`。

**严重级别**：🟡 中等（可维护性）

---

### 1.5 【低】`GUIUtility.ExitGUI()` 异常流在嵌套 `BeginHorizontal`/`BeginVertical` 中

**位置**：`LoomSettingsWindow.cs:162`（删除包）、`L237`（删除图集）、`L341`（删除字体）

```csharp
if (GUILayout.Button("删除", GUILayout.Width(80)))
{
    _settings.packages.RemoveAt(idx);
    EditorGUILayout.EndHorizontal();
    EditorGUILayout.EndVertical();
    GUIUtility.ExitGUI();
    return;
}
```

`GUIUtility.ExitGUI()` 抛异常跳出脚本执行，`EndHorizontal`/`EndVertical` 永远不会执行到。虽然 Unity 内部能处理，但这种模式是 **Unity Editor 常见反模式**（实际 Unity 官方已不再推荐 ExitGUI 用于此类场景）。此处 `EndHorizontal()`/`EndVertical()` 写在了 ExitGUI 之前是为了满足"看起来配对"——但 ExitGUI 抛异常后 Layout 事件不再继续，这些 End 调用是无效的。

**修复方向**：用 `EditorGUILayout.BeginHorizontal`/`EditorGUILayout.BeginVertical` 的返回值模式，把删除逻辑提到绘制循环外面；或用 `GUILayout.BeginHorizontal` 搭配 `GUILayout.EndHorizontal` 显式配对代替 ExitGUI 滥用。

**严重级别**：🟢 低（虽然丑，但 Unity 内部能处理）

---

### 1.6 【低】日志缓冲区截断逻辑丢失中间内容

**位置**：`LoomSettingsWindow.cs:567-568`

```csharp
if (_log.Length > 8000) _log.Remove(0, 4000);
```

从头部截断 4000 字符，但 `StringBuilder.Remove` 可能在 UTF-16 代理对中间切割。日志行可能被截断成乱码。且 `Remove(0,4000)` 后前 4000 直接丢弃——如果用户想回溯关键报错，恰好被截断。

**修复方向**：用 `Queue<string>` 存最近 N 行，或按换行符边界截断。

**严重级别**：🟢 低（调试体验，不影响功能）

---

## 二、LoomWorkspaceInitializer.cs（82 行）

### 2.1 【低】初始化幂等性依赖标签存在

**位置**：`LoomWorkspaceInitializer.cs:48-60`

```csharp
if (!File.Exists(target)) { File.WriteAllText(target, tagged, Encoding.UTF8); return null; }
string existing = File.ReadAllText(target);
if (!existing.Contains(BEGIN))
{
    File.WriteAllText(target, existing.TrimEnd('\n') + "\n\n" + tagged, Encoding.UTF8);
    return null;
}
```

三种情况处理正确：新文件 → 直接写；无标签 → 追加；有标签 → 正则替换。但若用户手动删了标签内的内容但保留标签，下次初始化会**用模板覆盖**用户修改——这是预期行为（由标签段管理）。并发安全：两次快速连续初始化会互相覆盖文件（`File.WriteAllText` 不是原子操作），**极罕见但理论存在竞争**。

**修复方向**：加 try-catch over `File.ReadAllText` + `WriteAllText`；考虑用临时文件 + `File.Replace` 实现原子写入。

**严重级别**：🟢 低

---

### 2.2 【低】DistributeSkill 直接覆盖，不校验模板版本

**位置**：`LoomWorkspaceInitializer.cs:66-78`

每次初始化都无条件覆盖所有 skill 文件。没有版本标记或 hash 比对——如果用户修改了 skill 文件，下次初始化会静默丢修改。对比 CLAUDE.md 注入有标签段保护，skill 文件没有。

**修复方向**：skill 文件是框架分发的工具，应不可由用户修改——这是设计意图。若未来允许用户定制，需加版本号/标签段机制。

**严重级别**：🟢 低（设计如此）

---

## 三、LoomConfigExporter.cs（78 行）

### 3.1 【严重】手写 JSON 序列化无转义

（见 LoomSettingsWindow 1.1，此处不再重复）

---

### 3.2 【低】`RelativeFromWorkspace` 注释硬编码路径深度假设

**位置**：`LoomConfigExporter.cs:63`

```csharp
// 简化：工作区根 = Assets/LoomUI/（深度2）。targetDir 可能是 Assets/ 下或 Packages/ 下（插件包内）。
```

实际代码用 `Uri.MakeRelativeUri` 算相对路径，与深度无关——注释过时且误导。若用户把工作区设在 `Assets/Sub/LoomUI/`（深度 3），逻辑仍正确，但注释说"深度 2"会让维护者误以为有限制。

**修复方向**：删除注释中的"深度 2"说法。

**严重级别**：🟢 低（代码正确，注释过时）

---

### 3.3 【低】`pkgOutputDir + "ui/"` 硬编码字符串拼接

**位置**：`LoomConfigExporter.cs:22`

```csharp
string outRel = RelativeFromWorkspace(s.workspaceDir, s.pkgOutputDir + "ui/");
```

如果 `s.pkgOutputDir` 末尾没有 `/`，拼接后的 `Bundlesui/` 路径错误。目前配置默认 `Assets/LoomGUI/Bundles/` 恰好有 trailing slash，但无显式保证。

**修复方向**：用 `Path.Combine(s.pkgOutputDir, "ui")`。

**严重级别**：🟢 低

---

## 四、LoomAtlasSync.cs（202 行）

### 4.1 【中等】`SyncEntry` 每次全量替换 packables，不做增量 diff

**位置**：`LoomAtlasSync.cs:148-158`

```csharp
var oldPackables = atlas.GetPackables();
// ...
if (oldPackables != null && oldPackables.Length > 0)
    atlasAsset.Remove(oldPackables);
if (sprites.Count > 0)
    atlasAsset.Add(sprites.ToArray());
```

每次同步都 Remove 全部再 Add 全部，触发了不必要的 reimport。当图集有 100+ sprite 时，diff 优化（`L120` 定义了 `DiffPackables` 但 `SyncEntry` 没用它）可显著减少无效操作。

**修复方向**：收集现有 packables 的 GUID/路径，与扫描结果做 diff，仅 `Remove(toRemove)` + `Add(toAdd)`。

**严重级别**：🟡 中等（性能，大图集时拖慢编辑器）

---

### 4.2 【中等】`EnsureAtlasAsset` 不处理 `pkgOutputDir` 为空的情况——早返 null 但不报错

**位置**：`LoomAtlasSync.cs:65`

```csharp
if (string.IsNullOrEmpty(pkgOutputDir)) return null;
```

调用方 `SyncAll`（L48）拿 `null` 后 `failCount++` 但只在所有图集循环结束后才 `Debug.LogWarning`——单条日志无法分辨是哪个图集失败、失败原因（pkgOutputDir 空 vs 创建失败）。

**修复方向**：在 null return 前打 `Debug.LogError` 并带上 entry.atlasName。

**严重级别**：🟡 中等（调试体验）

---

### 4.3 【低】`ToAbs` 与 `ToAssetPath` 重复定义

**位置**：`LoomAtlasSync.cs:186-190` / `LoomSettingsWindow.cs:546-550`

```csharp
// LoomAtlasSync.cs
static string ToAbs(string unityRel) { ... Directory.GetParent(Application.dataPath).FullName ... }

// LoomSettingsWindow.cs
static string ToAbs(string unityRel) { ... Directory.GetParent(Application.dataPath).FullName ... }
```

两处实现完全相同。LoomConfigExporter 和 LoomWorkspaceInitializer 中还有 `Directory.GetParent(Application.dataPath).FullName` 的内联调用（共 6 处）。

**修复方向**：提取到 `LoomEditorPaths` 工具类统一暴露 `ToAbs` 和 `ProjectRoot`。

**严重级别**：🟢 低（维护性）

---

## 五、LoomWorkspaceAssetPostprocessor.cs（36 行）

### 5.1 【中等】Settings 未加载时静默跳过 → PNG 不在 Sprite 模式但无提示

**位置**：`LoomWorkspaceAssetPostprocessor.cs:17-18`

```csharp
string ws = LoomSettings.GetDefault()?.workspaceDir;
if (string.IsNullOrEmpty(ws)) return;
```

`OnPreprocessAsset` 在 Asset Import 期间执行。如果 `LoomSettings` 资产还未创建（首次导入、工程刚 clone），`.workspaceDir` 为 null，**工作区下所有 PNG 都不会被设成 Sprite**。之后即使 settings 建了，已导入的 PNG 仍是 Default 类型，不会自动重导。

**修复方向**：对于"可能 settings 未建"的情况，至少加一条 `Debug.LogWarning`；或提供一个 Editor 菜单项 `LoomGUI > Fix PNG Import Settings` 用于事后修复。

**严重级别**：🟡 中等（静默失败）

---

### 5.2 【低】只处理 `.png` 扩展名——不处理其他潜在图集格式

**位置**：`LoomWorkspaceAssetPostprocessor.cs:23`

```csharp
if (norm.EndsWith(".png"))
```

若未来项目用 `.jpg`/`.tga` 等其他格式作为图集源（SpriteAtlas 支持），这些文件不会被设为 Sprite 类型。但规范中图集源明确为 PNG——当前不算问题。

**严重级别**：🟢 低

---

## 六、PkgManifestReader.cs（184 行）

### 6.1 【低】NodeBlock 跳过逻辑对截断包的诊断信息不足

**位置**：`PkgManifestReader.cs:88-100`

```csharp
for (ulong i = 0; i < totalNodes; i++)
{
    r.Skip(4 + 1);            // parent_idx + kind_tag
    uint styleLen = r.U32();
    r.Skip(styleLen);          // style_blob
    // ...
}
```

逐节点跳跃时若遇到截断包（`styleLen` 超大），`Skip` 会抛 `PkgManifestException`，但异常信息只含字节偏移，不含"处理到第几个节点"——排查 broken pkg 时无法快速定位损坏点。

**修复方向**：异常消息中加上 `i` 计数和 `totalNodes` 总数。

**严重级别**：🟢 低（调试体验）

---

### 6.2 【低】`totalNodes` 用 `ulong` 累加但 `nodeCount` 是 `uint`——溢出理论上不可能

**位置**：`PkgManifestReader.cs:84`

```csharp
totalNodes += nodeCount;
```

单个 pkg 不可能有超过 2³² 个节点，`ulong` 是防御性的。但若恶意构造的 pkg 让 `totalNodes > long.MaxValue`，循环 `for (ulong i = 0; i < totalNodes; i++)` 会超时（不是崩溃——`Skip` 最终会越界抛异常）。Bincode 输入可信时无风险。

**修复方向**：加总量上限校验（如 10M 节点）。

**严重级别**：🟢 低（假设输入可信）

---

## 七、测试文件审查

### 7.1 测试分类

| 文件 | 类型 | 行数 | 质量 |
|------|------|------|------|
| MirrorPoolTests.cs | 集成测试 | 197 | ⭐⭐⭐ 手搓全量 v10 blob，heavy setup，覆盖 reuse_key 核心场景 |
| MaterialManagerTests.cs | 单元测试 | 65 | ⭐⭐⭐⭐ 纯逻辑，不依赖 Unity 场景，4 个测试互不依赖 |
| ClipBoxTests.cs | 单元测试 | 108 | ⭐⭐⭐⭐⭐ 纯数学测试，每个测试独立 create/destroy，覆盖四角+边缘 |
| LoomEventHandlerTests.cs | 集成测试 | 427 | ⭐⭐⭐⭐ 全面覆盖 BubbleRoute/DirectDispatch，所有事件类型，需 stage+字体 |
| LoomInputCollectorTests.cs | 单元测试 | 89 | ⭐⭐⭐⭐⭐ 纯数学，含 round-trip 验证 |
| LoomStageDriverTests.cs | 集成测试 | 37 | ⭐⭐ 只测 Awake 不抛，覆盖极浅 |
| LoomAtlasSyncTests.cs | 混合 | 71 | ⭐⭐⭐⭐ DiffPackables 纯单元 + EnsureAtlasAsset 需 AssetDatabase |
| LoomConfigExporterTests.cs | 单元测试 | 52 | ⭐⭐ 含脆弱的具体路径断言（依赖项目结构） |
| SpriteResolverTests.cs | 单元测试 | 139 | ⭐⭐⭐⭐ 用 mock delegate 替代真实 AssetDatabase，每个场景覆盖到位 |
| AtlasMirrorPoolTests.cs | 单元测试 | 30 | ⭐⭐ 仅 2 个测试，与 SpriteResolverTests 高度重叠 |

### 7.2 【中等】SpriteResolverTests 与 AtlasMirrorPoolTests 测试重叠

**位置**：

- `SpriteResolverTests.cs:117-123` — `Init_NullSettings_DoesNotCrash`
- `AtlasMirrorPoolTests.cs:23-28` — `SpriteResolver_InitNull_DoesNotCrash`

两个测试**完全等价**：都是 `new SpriteResolver() → Init(null, null) → Assert AtlasCount==0`。AtlasMirrorPoolTests 还额外有一个 `SpriteResolver_NoAtlas_ReturnsNull`，但 SpriteResolverTests 已有 `Miss_NotCachedInSpriteCache` 覆盖 miss 路径。

**修复方向**：合并 AtlasMirrorPoolTests 到 SpriteResolverTests，或删除 AtlasMirrorPoolTests。

**严重级别**：🟡 中等

---

### 7.3 【低】LoomEventHandlerTests 大量重复 `BuildStage()` + `Native.loomgui_stage_free` 模板

**位置**：`LoomEventHandlerTests.cs` 中 10+ 个测试都有：

```csharp
var (stage, h, root, parent, child) = BuildStage();
// ...
Native.loomgui_stage_free((StageHandle*)stage);
```

每个测试重复相同的 setup/teardown。可以用 `[SetUp]`/`[TearDown]` 或 NUnit 的 `[OneTimeSetUp]` 减少重复——但 `BuildStage()` 依赖字体文件和 FFI，一次构造复用给所有测试可加速。

**修复方向**：加 `[OneTimeSetUp]` 共享 stage；每个测试用 `[SetUp]` 重置 handler listener 表。

**严重级别**：🟢 低

---

### 7.4 【低】LoomConfigExporterTests 硬编码期望路径，依赖项目目录结构

**位置**：`LoomConfigExporterTests.cs:21-24`

```csharp
StringAssert.Contains("\"exe_path\": \"../../Packages/com.loomgui.unity/Editor/Tools/loomgui_pkg.exe\"", json);
StringAssert.Contains("\"output_dir\": \"../LoomGUI/Bundles/ui/\"", json);
```

路径关系由 `Uri.MakeRelativeUri` 动态计算，依赖 `workspaceDir` 与 `pkgOutputDir` 的具体值。若未来目录结构变化（比如 LoomUI 移到别处），测试会失败但被测代码实际正确——**这是测试脆弱性**。

**修复方向**：测试改为正则匹配 `"exe_path": ".*/loomgui_pkg\.exe"` 或跳过精确路径验证，只验 JSON 结构可反序列化。

**严重级别**：🟢 低

---

### 7.5 【低】LoomStageDriverTests 只测"不抛"，覆盖极浅

**位置**：`LoomStageDriverTests.cs:18-36`

只验证 `Awake` 后 `Stage != null`。不覆盖以下关键行为：
- 字体注册是否成功
- `Tick(null)` / `Tick(0.016f)` 不崩
- `OnDestroy` 释放 stage 句柄
- pkg.bin 加载路径

**修复方向**：加 `Tick(0)`、加载空 pkg、加载有根 div 的 pkg 等测试。

**严重级别**：🟢 低（补充性建议）

---

## 八、fence_contract.rs 覆盖度分析

### 8.1 fence.md 标注【实证】但 fence_contract.rs 无对应测试的属性

| fence.md 属性 | fence.md 标注 | fence_contract.rs 状态 |
|---|---|---|
| `row-gap`/`column-gap` | 【推断·待测】 | ❌ 无测试（标注一致） |
| `align-self` | 【实证】 | ❌ 无测试（标注不一致） |
| `flex-grow` | 【实证】 | ❌ 无测试 |
| `flex-shrink` | 【实证】 | ❌ 无测试 |
| `flex-basis` | 【实证】 | ❌ 无测试 |
| `min-width`/`min-height` | 【实证】 | ❌ 无测试 |
| `max-width`/`max-height` | 【实证】 | ❌ 无测试 |
| `border`/`border-width` | 【实证·待测简写 color 丢弃】 | ❌ 无测试 |
| `border-color` | 【实证】 | ❌ 无测试 |
| `font-family` | 【实证】 | ❌ 无测试 |
| `line-height` | 【实证】 | ❌ 无测试 |
| `letter-spacing` | 【实证】 | ❌ 无测试 |
| `overflow-x`/`overflow-y` | 【实证】 | ❌ 无测试 |
| `filter` blur/drop-shadow 拒 | 【实证·待测 blur 拒】 | ❌ 无测试 |
| `border-image-slice: %` 渲染坍缩 | fence.md §2.3 勘误 | 不适用（渲染层 bug，非围栏层） |

### 8.2 未覆盖的围栏外选择器

| 选择器 | fence.md 标注 | fence_contract.rs |
|---|---|---|
| `:nth-child`/`:first-child`/`:nth-of-type` | 【推断·待测】 | ❌ 无测试 |
| `:not()` | 【推断·待测】 | ❌ 无测试 |
| 相邻兄弟 `+` / 后续兄弟 `~` | 【推断·待测】 | ❌ 无测试 |

### 8.3 fence_contract.rs 已覆盖项（共 27 个测试函数，覆盖充分项）

✅ 元素围栏（白名单 + 围栏外拒）、✅ 17 个 layout + visual 属性、✅ 9 个围栏外属性静默忽略、✅ position 全套（absolute/relative/fixed/sticky）、✅ inset 四边、✅ transform skew、✅ @media、✅ transition 解析（prop/duration/ease/delay/多 spec）、✅ 属性选择器（Exists/Eq/特异性/操作符降级/大小写归一）、✅ v1.7 display:block desugar / class-based 不触发、✅ display:grid→flex / display:block→Block mode / display:none

### 8.4 建议

**fence.md 标注应同步修正**：上述 8.1 表中的 12 项在 fence.md 标了【实证】但实际无 fence_contract 测试。应补测试后保留【实证】标注，或将标注降级为【推断·待测】直到补完测试。当前标注与真相不一致，**违反了 fence.md 开头声明的"测试是权威真相源"原则**。

**补测试优先级**：
1. `flex-grow`/`flex-shrink`/`flex-basis` — 日常最常用，易出 bug
2. `border`/`border-width` 简写 color/style 丢弃行为
3. `align-self` — 覆盖与 items 不同的场景
4. `min-width`/`min-height` — taffy 核心约束，漏测风险高

---

## 九、跨文件问题汇总

### 9.1 `ToAbs` 重复定义

**影响文件**：`LoomSettingsWindow.cs:546`、`LoomAtlasSync.cs:186`

**建议**：创建 `LoomEditorPaths.cs` 统一暴露 `ProjectRoot` 和 `ToAbs`。

### 9.2 `ProjectRoot` 计算模式内联 6 处

**影响文件**：`LoomSettingsWindow.cs:552`、`LoomConfigExporter.cs:54,65`、`LoomWorkspaceInitializer.cs:27`、`LoomAtlasSync.cs:188,194`

```csharp
string projRoot = Directory.GetParent(Application.dataPath).FullName;
```

**建议**：提取为 `LoomEditorPaths.ProjectRoot` 静态属性。

### 9.3 拖拽目录处理逻辑重复

**影响文件**：`LoomSettingsWindow.cs:96-114`（DrawPackageDropZone）、`:210`（HandleFolderDrop）、`:465-507`（DirectoryDropField）

三处都实现"拖目录到 Rect → 接受/拒绝"模式，但各自有细微差异（HandleFolderDrop 多 NormalizeDroppedDir、DirectoryDropField 多 RelativizeTo + 清除按钮）。

**建议**：提取 `DirectoryDragReceiver(Rect, Action<string>)` 公共函数。

### 9.4 SaveSettings 模式不统一

`LoomSettingsWindow.cs` 内有 `SaveSettings()`（L453），但 3 处手写内联。LoomAtlasSync `SyncAll` 也手写 `SetDirty + SaveAssetIfDirty`。

**建议**：统一为 `LoomSettings.Save()` 实例方法（自动 `SetDirty` + `SaveAssetIfDirty` + `LoomConfigExporter.Export`），Editor 工具各处都不手写。

---

## 十、严重级别汇总

| 严重级别 | 数量 | 关键项 |
|----------|------|--------|
| 🔴 严重 | 2 | JSON 无转义、命令行参数无转义 |
| 🟡 中等 | 5 | SyncEntry 全量替换、AssetPostprocessor 静默跳过、测试重复、fence.md 实证标注不一致、SetDirty 内联 |
| 🟢 低 | 13 | 代码重复、日志截断、GUIUtility.ExitGUI、注释过时、测试脆弱性等 |

---

## 十一、修复建议优先级

1. **立即**：`LoomConfigExporter.BuildJson` 用 `JsonSerializer` 替代手写字符串拼接（§1.1/§3.1）
2. **立即**：`PackPackage` 用 `ProcessStartInfo.ArgumentList` 替代手动引号包裹（§1.2）
3. **本迭代**：fence.md 标注与 fence_contract.rs 对齐——补测试或改标注（§8.4）
4. **本迭代**：提取 `LoomEditorPaths` + 消除 `ToAbs`/`ProjectRoot` 重复（§9.1/9.2）
5. **下迭代**：`LoomAtlasSync.SyncEntry` 用 diff 替代全量替换（§4.1）
6. **下迭代**：合并 `AtlasMirrorPoolTests` 到 `SpriteResolverTests`（§7.2）
7. **可选**：`LoomStageDriverTests` 加 Tick/加载测试（§7.5）
