# 工作流闭环 + 图集重做 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 editor/ Node 脚本迁 Unity C#（设计师只要 Unity）、删 samples/、图集改成配置驱动 + 显式路由 + 自动打包（修坑 104 / B2）。

**Architecture:** 全局一份 `LoomSettings` ScriptableObject（Runtime/，Editor+运行时同源）做配置中枢。`LoomSettingsWindow`（三 tab）编辑它。`SpriteResolver` 据 atlasEntries 建"文件夹→图集"路由表显式查图。`LoomStage` 用 `Resources.Load` 自动取配置。`LoomWorkspaceInitializer` 注围栏规则+skill+config.json，`AssetPostprocessor` 拦非资源文件。

**Tech Stack:** Unity 6.5 URP / C# / Rust loomgui_pkg CLI（随插件发布）。

## Global Constraints

- 全部重写不留兼容（开发阶段）。`LoomPackageSettings` / `LoomPackageManagerWindow` 删除，由 `LoomSettings` / `LoomSettingsWindow` 替代。
- 配置资产全局一份：`Assets/Resources/LoomGUI/LoomSettings.asset`（Resources/ 下，运行时 `Resources.Load` 可读）。
- path 归一化裁 `res/` 前缀不变（`res/icons/home.png` → `icons/home.png`），打包器归一化逻辑不动。
- 全局唯一 `res/` 在 `Assets/LoomUI/res/`（从 `showcase/res/` 提到 LoomUI 根）。
- config.json 全相对工作区根（`Assets/LoomUI/`），可移植。
- exe 随插件发布 `Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe`，不暴露路径给用户。
- 用户只读中文——问答用中文，代码/commit 英文。
- 改 Rust 后必重编 .dll + commit（家里机才能测）。本 plan 不改 Rust 源码（path 归一化不变），但末尾要 build .dll 确认无回归。
- 围栏契约门：`cargo test -p loomgui_core fence_contract` 必须绿。
- 问答/总结用中文；代码/commit 照旧英文。

---

## File Structure

**新建（Runtime）：**
- `loomgui_unity/Assets/LoomGUI/Runtime/LoomSettings.cs` — 全局配置 ScriptableObject + PackageEntry + AtlasEntry。
- `loomgui_unity/Assets/Resources/LoomGUI/LoomSettings.asset` — 配置资产（Resources.Load 可读）。

**新建（Editor）：**
- `loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs` — 三 tab 设置面板（替代 LoomPackageManagerWindow）。
- `loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceInitializer.cs` — 工作区初始化（注围栏规则+skill+config.json）。
- `loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceAssetPostprocessor.cs` — 拦工作区非资源文件导入。
- `loomgui_unity/Assets/LoomGUI/Editor/LoomConfigExporter.cs` — LoomSettings → config.json 导出（纯逻辑，可单测）。
- `loomgui_unity/Assets/LoomGUI/Editor/LoomAtlasSync.cs` — atlas packables 同步逻辑（纯逻辑，可单测）。
- `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md` — 围栏规则文本（注入用，迁自 editor/rules/claude/CLAUDE.md.tmpl）。
- `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/SKILL.md` + `references/{fence.md,preview-polyfill.html,preview-trust.md}` — skill 内容（迁自 editor/skill/）。
- `loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe` — 打包器（随插件发布）。

**重写（Runtime）：**
- `loomgui_unity/Assets/LoomGUI/Runtime/SpriteResolver.cs` — 显式路由 + miss 不缓存。
- `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs` — 砍 `_spriteAtlases` Inspector，改 Resources.Load 取 LoomSettings。

**删除：**
- `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.cs` + `.asset` + `.meta`
- `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs` + `.meta`
- `samples/`（整目录）
- `editor/`（整目录）

**迁移：**
- `loomgui_unity/Assets/LoomUI/showcase/res/` → `loomgui_unity/Assets/LoomUI/res/`
- `samples/design-systems/loomgui/` → `loomgui_unity/Assets/LoomUI/design-systems/`

---

## Task 1: LoomSettings 配置类 + 资产

**Files:**
- Create: `loomgui_unity/Assets/LoomGUI/Runtime/LoomSettings.cs`
- Create: `loomgui_unity/Assets/Resources/LoomGUI/LoomSettings.asset`
- Delete: `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.cs` (+.meta)
- Delete: `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.asset` (+.meta)

**Interfaces:**
- Consumes: 无（基础数据类）。
- Produces: `LoomSettings`（ScriptableObject）+ `LoomSettings.GetOrCreateDefault()` + `PackageEntry` + `AtlasEntry`。后续所有 task 依赖此。

- [ ] **Step 1: 写 LoomSettings.cs**

```csharp
using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI
{
    /// <summary>
    /// 全局配置资产（Editor+运行时同源）。放 Resources/LoomGUI/，运行时 Resources.Load 可读。
    /// LoomSettingsWindow 编辑它；LoomStage 运行时读它建图集路由表。
    /// </summary>
    [CreateAssetMenu(menuName = "LoomGUI/Settings", fileName = "LoomSettings")]
    public sealed class LoomSettings : ScriptableObject
    {
        [Tooltip("工作区根（Unity 工程相对路径，open-design import 此目录）")]
        public string workspaceDir = "Assets/LoomUI/";

        [Tooltip("资源目录名（打包器按此前缀归一化 img path，默认 res）")]
        public string resDirName = "res";

        [Tooltip("pkg.bin 输出目录（Unity 工程相对路径）")]
        public string pkgOutputDir = "Assets/StreamingAssets/";

        [Tooltip("包列表")]
        public List<PackageEntry> packages = new();

        [Tooltip("图集配置（path 顶层子目录 → 图集 路由）")]
        public List<AtlasEntry> atlasEntries = new();

        /// Resources 内相对路径（无扩展名），Resources.Load 用。
        public const string ResourcesPath = "LoomGUI/LoomSettings";

        /// 在 Resources 找配置资产；不存在则创建（Editor 下；运行时找不到返 null 调用方容错）。
        public static LoomSettings GetOrCreateDefault()
        {
            var existing = Resources.Load<LoomSettings>(ResourcesPath);
#if UNITY_EDITOR
            if (existing == null)
            {
                existing = CreateInstance<LoomSettings>();
                const string assetPath = "Assets/Resources/LoomGUI/LoomSettings.asset";
                UnityEditor.AssetDatabase.CreateAsset(existing, assetPath);
                UnityEditor.AssetDatabase.SaveAssets();
            }
#endif
            return existing;
        }
    }

    /// 单个包配置。sourceDir 相对工作区根（如 "showcase"）。
    [Serializable]
    public sealed class PackageEntry
    {
        public string pkgName = "";
        [Tooltip("源目录（相对工作区根，含 html + 引用 res 下图片）")]
        public string sourceDir = "";
        [Tooltip("html 文件名列表（相对 sourceDir）")]
        public List<string> htmlFiles = new();

        public PackageEntry() { }
        public PackageEntry(string pkgName, string sourceDir) { this.pkgName = pkgName; this.sourceDir = sourceDir; }
    }

    /// 图集配置。folders 拖文件夹当 packables；atlas 运行时引用（图集 tab 同步时绑）。
    [Serializable]
    public sealed class AtlasEntry
    {
        public string atlasName = "";
        [Tooltip("res 根图（path 无子目录）兜底走此图集")]
        public bool isDefault;
        [Tooltip("packables 文件夹（Unity 相对路径）")]
        public List<string> folders = new();
        [Tooltip("运行时图集引用（同步时自动绑）")]
        public SpriteAtlas atlas;
    }
}
```

- [ ] **Step 2: 删旧 LoomPackageSettings**

```bash
# Unity 必须关着（删 .asset.meta）
rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.cs
rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.cs.meta
rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.asset
rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.asset.meta
```

注意：此步会让 `LoomPackageManagerWindow.cs` 编译失败（它引用旧 LoomPackageSettings）。Task 5 会重写该文件。**为避免中间编译炸**，Task 1 完成后立即做 Task 5（重写 window），或在 Task 1 同 commit 里临时把 `LoomPackageManagerWindow.cs` 注释掉——推荐前者：Task 1+5 同一批。

- [ ] **Step 3: 建 Resources 目录 + 资产**

Unity 打开后，菜单 `LoomGUI > Settings`（Task 5 加菜单）首次触发 `GetOrCreateDefault` 自动建资产。或手动：Unity 里 `Assets/Resources/LoomGUI/` 右键 Create > LoomGUI > Settings。

- [ ] **Step 4: 跑 fence_contract 确认 Rust 无回归**

```bash
cargo test -p loomgui_core fence_contract
```
Expected: 10 passed。

- [ ] **Step 5: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/LoomSettings.cs loomgui_unity/Assets/Resources/
git rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.cs loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.cs.meta loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.asset loomgui_unity/Assets/LoomGUI/Editor/LoomPackageSettings.asset.meta
git commit -m "feat(unity): LoomSettings 全局配置资产（替代 LoomPackageSettings）"
```

---

## Task 2: res 提到 LoomUI 根

**Files:**
- Move: `loomgui_unity/Assets/LoomUI/showcase/res/` → `loomgui_unity/Assets/LoomUI/res/`

**Interfaces:**
- Consumes: 无。
- Produces: 全局唯一 `Assets/LoomUI/res/`。后续图集配置 folders 指向此。

- [ ] **Step 1: 移动 res 目录**

Unity 关着。`showcase/res/` 整体移到 `Assets/LoomUI/res/`（含 icons/ + LoomShowcaseAtlas.spriteatlas）。showcase HTML 的 `src="res/icons/..."` **不变**——打包器归一化裁 `res/` 前缀后都是 `icons/...`，物理位置变了但 path 不变。

```bash
# Git mv 保留历史（PowerShell 用 Move-Item，bash 用 mv）
mv loomgui_unity/Assets/LoomUI/showcase/res loomgui_unity/Assets/LoomUI/res
```

- [ ] **Step 2: 更新 .meta 引用（如有）**

检查 showcase 里是否有引用 `showcase/res/` 路径的 .meta 或脚本（grep `showcase/res`）。LoomShowcaseAtlas.spriteatlas 的 packables GUID 引用不变（文件跟着移，GUID 不变）。

```bash
grep -rn "showcase/res" loomgui_unity/Assets/ || echo "(no refs)"
```

- [ ] **Step 3: Commit**

```bash
git add -A loomgui_unity/Assets/LoomUI/
git commit -m "refactor(unity): res 提到 LoomUI 根（全局唯一资源目录）"
```

---

## Task 3: SpriteResolver 重写（显式路由 + miss 不缓存）

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/SpriteResolver.cs`
- Test: `loomgui_unity/Assets/LoomGUI/Tests/SpriteResolverTests.cs`

**Interfaces:**
- Consumes: `LoomSettings.atlasEntries`（Task 1）。
- Produces: `SpriteResolver.Init(LoomSettings)` + `Sprite GetSprite(string path)`（接口不变，MirrorPool 不改）。

- [ ] **Step 1: 写失败测试 SpriteResolverTests.cs**

```csharp
using System.Collections.Generic;
using NUnit.Framework;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI.Tests
{
    public class SpriteResolverTests
    {
        // 构造一个不依赖真实 SpriteAtlas 资产的 SpriteResolver：直接注入 folder→atlas 映射。
        // Init(LoomSettings) 走 atlasEntries；测试用 InitWithMap 直接注入映射表。

        [Test]
        public void Route_ByTopLevelSubdir()
        {
            var resolver = new SpriteResolver();
            var atlasIcons = ScriptableObject.CreateInstance<SpriteAtlas>(); // 空 atlas
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas> { { "icons", atlasIcons } }, atlasIcons);
            // atlasIcons 无 sprite → 返 missingSprite（null），但路由到 icons（不抛）。
            // 验证：不遍历别的 atlas（无 NRE），miss 返 null。
            Assert.IsNull(resolver.GetSprite("icons/home.png"));
        }

        [Test]
        public void Route_RootImage_FallsBackToDefault()
        {
            var resolver = new SpriteResolver();
            var defaultAtlas = ScriptableObject.CreateInstance<SpriteAtlas>();
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas>(), defaultAtlas);
            // path 无子目录 → 走 default atlas。
            Assert.IsNull(resolver.GetSprite("home.png"));
        }

        [Test]
        public void Miss_NotCached()
        {
            var resolver = new SpriteResolver();
            var atlas = ScriptableObject.CreateInstance<SpriteAtlas>();
            resolver.InitWithMap(new Dictionary<string, SpriteAtlas> { { "icons", atlas } }, atlas);
            // 首次 miss。
            Assert.IsNull(resolver.GetSprite("icons/missing.png"));
            // 假装 atlas 后来 pack 好——但空 atlas.GetSprite 仍 miss。
            // 关键断言：miss 不进缓存。用内部 CacheCount 验证（miss 不增）。
            Assert.AreEqual(0, resolver.CacheCount, "miss 不应进缓存");
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
# Unity 里 Test Runner 跑 SpriteResolverTests，或：
# 编辑器菜单 Run All Tests
```
Expected: FAIL（SpriteResolver 无 InitWithMap / CacheCount）。

- [ ] **Step 3: 重写 SpriteResolver.cs**

```csharp
using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI
{
    /// <summary>
    /// path → Sprite 显式路由。核心不知图集，path 是归一化后的相对路径（如 "icons/home.png"）。
    ///
    /// 路由：path 顶层子目录 → folder→atlas 映射表 → atlas.GetSprite(文件名去扩展)。
    /// res 根图（无子目录）或子目录不在表 → 走 isDefault atlas。
    /// miss 不缓存（修坑 104）——atlas 启动全加载，重查成本可控。
    /// </summary>
    public sealed class SpriteResolver
    {
        readonly Dictionary<string, SpriteAtlas> _folderToAtlas = new();
        readonly Dictionary<string, Sprite> _cache = new();
        SpriteAtlas _defaultAtlas;
        Sprite _missingSprite;

        public Sprite MissingSprite { set => _missingSprite = value; }
        public int AtlasCount => _folderToAtlas.Count;
        /// 测试用：缓存条目数（miss 不增）。
        public int CacheCount => _cache.Count;

        /// 从 LoomSettings.atlasEntries 建 folder→atlas 映射表。
        public void Init(LoomSettings settings)
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            _defaultAtlas = null;
            if (settings == null) return;
            foreach (var entry in settings.atlasEntries)
            {
                if (entry == null || entry.atlas == null) continue;
                if (entry.isDefault && _defaultAtlas == null) _defaultAtlas = entry.atlas;
                foreach (var folder in entry.folders)
                {
                    if (string.IsNullOrEmpty(folder)) continue;
                    // folder 是 Unity 路径如 Assets/LoomUI/res/icons → 子目录 key = 最末段 "icons"。
                    string key = folder.TrimEnd('/', '\\');
                    int sep = key.LastIndexOfAny(new[] { '/', '\\' });
                    if (sep >= 0) key = key.Substring(sep + 1);
                    if (!string.IsNullOrEmpty(key))
                        _folderToAtlas[key] = entry.atlas;
                }
            }
        }

        /// 测试用：直接注入映射表。
        public void InitWithMap(Dictionary<string, SpriteAtlas> map, SpriteAtlas defaultAtlas)
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            foreach (var kv in map) _folderToAtlas[kv.Key] = kv.Value;
            _defaultAtlas = defaultAtlas;
        }

        public Sprite GetSprite(string path)
        {
            if (string.IsNullOrEmpty(path)) return null;
            if (_cache.TryGetValue(path, out var cached)) return cached;

            string spriteName = System.IO.Path.GetFileNameWithoutExtension(path);
            SpriteAtlas atlas = ResolveAtlas(path);
            Sprite found = atlas != null ? atlas.GetSprite(spriteName) : null;

            if (found != null)
            {
                _cache[path] = found;   // 只缓存命中
                return found;
            }
            // miss 不缓存（修坑 104）。
            return _missingSprite;
        }

        /// path → atlas。顶层子目录查表；无子目录或 miss → default。
        SpriteAtlas ResolveAtlas(string path)
        {
            string p = path.Replace('\\', '/');
            int slash = p.IndexOf('/');
            if (slash <= 0)
            {
                // 无子目录 → default
                return _defaultAtlas ?? FirstAtlas();
            }
            string topDir = p.Substring(0, slash);
            if (_folderToAtlas.TryGetValue(topDir, out var atlas)) return atlas;
            return _defaultAtlas ?? FirstAtlas();
        }

        SpriteAtlas FirstAtlas()
        {
            foreach (var kv in _folderToAtlas) return kv.Value;
            return null;
        }

        public void Clear()
        {
            _folderToAtlas.Clear();
            _cache.Clear();
            _defaultAtlas = null;
            _missingSprite = null;
        }
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
# Unity Test Runner: SpriteResolverTests
```
Expected: 3 passed。

- [ ] **Step 5: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/SpriteResolver.cs loomgui_unity/Assets/LoomGUI/Tests/SpriteResolverTests.cs
git commit -m "feat(unity): SpriteResolver 显式路由 + miss 不缓存（修坑 104）"
```

---

## Task 4: LoomStage 自动取配置

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs`

**Interfaces:**
- Consumes: `LoomSettings`（Task 1）+ `SpriteResolver.Init(LoomSettings)`（Task 3）。
- Produces: LoomStage 不再依赖 `_spriteAtlases` Inspector 字段。

- [ ] **Step 1: 改 LoomStage.cs**

砍 `_spriteAtlases` 字段（LoomStage.cs:48），改 `_sprites` 初始化调 `Init(LoomSettings)`。

把第 45-48 行：
```csharp
        // v1.4-a T8：Sprite Atlas 接入。开发者建 SpriteAtlas asset（把 res/ 下 Sprite 划进去），
        // Inspector 拖入此列表。LoomStage Awake 时注册进 SpriteResolver，MirrorPool 按 path 查 Sprite。
        // 多图集：path 路由到对应 atlas 是 Unity 内部事（核心不感知）。
        [SerializeField] List<SpriteAtlas> _spriteAtlases = new();
```
替换为（无字段，运行时自动取配置）：
```csharp
        // 图集路由：Awake 时从全局 LoomSettings（Resources.Load 自动找）建 folder→atlas 映射。
        // 不在 Inspector 手配（配置总会遗忘），配置资产由 LoomSettingsWindow 维护。
```

把 Awake 里第 349-353 行：
```csharp
            // v1.4-a T8：path→Sprite 查询（替代 LoadAtlas/_texMap）。注册 Inspector 配的 SpriteAtlas。
            // 开发者建 SpriteAtlas asset（res/ 下 Sprite 划进去），Inspector 拖入 _spriteAtlases。
            // MirrorPool 按 blob path_idx→path→GetSprite 查 Sprite（懒查 + 缓存）。
            _sprites = new SpriteResolver();
            if (_spriteAtlases != null) _sprites.RegisterAtlases(_spriteAtlases);
```
替换为：
```csharp
            // 图集路由：从全局 LoomSettings 建 folder→atlas 映射（Resources.Load 自动找，不手配）。
            _sprites = new SpriteResolver();
            _sprites.Init(LoomSettings.GetOrCreateDefault());
```

- [ ] **Step 2: 确认 MirrorPool 调用不变**

`_pool.Sync(blob, transform, _mm, _sprites, ...)`（LoomStage.cs:528）签名不变——SpriteResolver 接口 `GetSprite` 不变。无需改 MirrorPool。

- [ ] **Step 3: 删 SpriteResolver 旧 RegisterAtlases 引用（如有）**

grep 确认无其它地方调 `RegisterAtlases`（已从 SpriteResolver 删）。

```bash
grep -rn "RegisterAtlases\|_spriteAtlases" loomgui_unity/Assets/LoomGUI/ || echo "(clean)"
```

- [ ] **Step 4: 跑测试确认无回归**

```bash
# Unity Test Runner: All Tests
```
Expected: 既有测试过（SpriteResolver 新测试 + 旧测试）。注意 AtlasMirrorPoolTests.cs 若引用旧 SpriteResolver API 会编译失败——Spec B 会删它，本 task 若撞编译，临时删该测试文件（它本就是空壳占位）。

- [ ] **Step 5: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs
git commit -m "feat(unity): LoomStage 自动取 LoomSettings（砍 _spriteAtlases Inspector）"
```

---

## Task 5: LoomSettingsWindow 三 tab 面板

**Files:**
- Create: `loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs`
- Delete: `loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs` (+.meta)

**Interfaces:**
- Consumes: `LoomSettings`（Task 1）+ `LoomConfigExporter`（Task 7）+ `LoomAtlasSync`（Task 6）。
- Produces: 菜单 `LoomGUI/Settings` + 三 tab 编辑面板。

- [ ] **Step 1: 删旧 LoomPackageManagerWindow**

```bash
rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs
rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs.meta
```

- [ ] **Step 2: 写 LoomSettingsWindow.cs**

```csharp
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// LoomGUI 设置面板（三 tab：工作区 / 包管理 / 图集）。菜单 LoomGUI > Settings。
    /// 共享全局 LoomSettings 资产。改任意字段 → 自动同步 config.json（Task 7 LoomConfigExporter）。
    /// </summary>
    public sealed class LoomSettingsWindow : EditorWindow
    {
        enum Tab { Workspace, Packages, Atlas }
        Tab _tab = Tab.Workspace;
        LoomSettings _settings;
        Vector2 _scroll;
        StringBuilder _log = new();

        [MenuItem("LoomGUI/Settings")]
        public static void Open()
        {
            var w = GetWindow<LoomSettingsWindow>(false, "LoomGUI Settings", true);
            w.minSize = new Vector2(720, 480);
        }

        void OnEnable()
        {
            _settings = LoomSettings.GetOrCreateDefault();
        }

        void OnGUI()
        {
            if (_settings == null) _settings = LoomSettings.GetOrCreateDefault();

            // tab toolbar
            _tab = (Tab)GUILayout.SelectionGrid((int)_tab, new[] { "工作区", "包管理", "图集" }, 3, EditorStyles.toolbarButton);
            EditorGUILayout.Space(8);

            EditorGUI.BeginChangeCheck();
            _scroll = EditorGUILayout.BeginScrollView(_scroll);
            switch (_tab)
            {
                case Tab.Workspace: DrawWorkspace(); break;
                case Tab.Packages: DrawPackages(); break;
                case Tab.Atlas: DrawAtlas(); break;
            }
            EditorGUILayout.EndScrollView();
            bool changed = EditorGUI.EndChangeCheck();

            EditorGUILayout.Space(8);
            DrawLog();

            if (changed)
            {
                EditorUtility.SetDirty(_settings);
                AssetDatabase.SaveAssetIfDirty(_settings);
                LoomConfigExporter.Export(_settings);  // 自动同步 config.json（Task 7）
            }
        }

        // ── 工作区 tab ──────────────────────────────────────────────
        void DrawWorkspace()
        {
            EditorGUILayout.LabelField("工作区配置", EditorStyles.boldLabel);
            _settings.workspaceDir = EditorGUILayout.TextField("工作区根", _settings.workspaceDir);
            _settings.resDirName = EditorGUILayout.TextField("res 目录名", _settings.resDirName);
            _settings.pkgOutputDir = EditorGUILayout.TextField("pkg.bin 输出目录", _settings.pkgOutputDir);

            EditorGUILayout.Space(8);
            if (GUILayout.Button("初始化工作区（注入围栏规则 + skill + config.json）", GUILayout.Height(28)))
            {
                LoomWorkspaceInitializer.Initialize(_settings);  // Task 8
                AppendLog("[init] 工作区初始化完成");
            }
        }

        // ── 包管理 tab（重写自旧 LoomPackageManagerWindow）──────────
        void DrawPackages()
        {
            EditorGUILayout.LabelField("包列表（" + _settings.packages.Count + "）", EditorStyles.boldLabel);
            for (int i = 0; i < _settings.packages.Count; i++) DrawPackageEntry(i);
            if (GUILayout.Button("+ 添加包", GUILayout.Width(120)))
            {
                _settings.packages.Add(new PackageEntry("new_pkg", ""));
            }
            EditorGUILayout.Space(8);
            if (GUILayout.Button("一键打包全部", GUILayout.Height(28))) PackAll();
        }

        void DrawPackageEntry(int idx)
        {
            var pkg = _settings.packages[idx];
            EditorGUILayout.BeginVertical(EditorStyles.helpBox);
            pkg.pkgName = EditorGUILayout.TextField("包名", pkg.pkgName);
            pkg.sourceDir = EditorGUILayout.TextField("源目录（相对工作区根）", pkg.sourceDir);
            EditorGUILayout.LabelField("html 文件（" + pkg.htmlFiles.Count + "）:");
            for (int j = 0; j < pkg.htmlFiles.Count; j++)
            {
                EditorGUILayout.BeginHorizontal();
                pkg.htmlFiles[j] = EditorGUILayout.TextField(pkg.htmlFiles[j]);
                if (GUILayout.Button("×", GUILayout.Width(24))) { pkg.htmlFiles.RemoveAt(j); break; }
                EditorGUILayout.EndHorizontal();
            }
            if (GUILayout.Button("+ 添加 html", GUILayout.Width(100))) pkg.htmlFiles.Add("");
            EditorGUILayout.BeginHorizontal();
            if (GUILayout.Button("打包", GUILayout.Width(80))) PackPackage(idx);
            if (GUILayout.Button("删除", GUILayout.Width(80))) { _settings.packages.RemoveAt(idx); }
            EditorGUILayout.EndHorizontal();
            EditorGUILayout.EndVertical();
        }

        // ── 图集 tab（Task 6 展开）──────────────────────────────────
        void DrawAtlas()
        {
            EditorGUILayout.LabelField("图集配置（" + _settings.atlasEntries.Count + "）", EditorStyles.boldLabel);
            for (int i = 0; i < _settings.atlasEntries.Count; i++) DrawAtlasEntry(i);
            if (GUILayout.Button("+ 添加图集", GUILayout.Width(120)))
            {
                _settings.atlasEntries.Add(new AtlasEntry { atlasName = "NewAtlas" });
            }
            EditorGUILayout.Space(8);
            if (GUILayout.Button("同步全部图集 packables", GUILayout.Height(28)))
            {
                LoomAtlasSync.SyncAll(_settings);  // Task 6
                AppendLog("[atlas] 同步完成");
            }
        }

        void DrawAtlasEntry(int idx)
        {
            var e = _settings.atlasEntries[idx];
            EditorGUILayout.BeginVertical(EditorStyles.helpBox);
            e.atlasName = EditorGUILayout.TextField("图集名", e.atlasName);
            e.isDefault = EditorGUILayout.Toggle("isDefault（res 根图兜底）", e.isDefault);
            EditorGUILayout.LabelField("folders（拖文件夹到此）:");
            var dropRect = GUILayoutUtility.GetRect(0, 30, GUILayout.ExpandWidth(true));
            GUI.Box(dropRect, "  拖文件夹当 packables", EditorStyles.helpBox);
            HandleFolderDrop(dropRect, e);
            for (int j = 0; j < e.folders.Count; j++)
            {
                EditorGUILayout.BeginHorizontal();
                e.folders[j] = EditorGUILayout.TextField(e.folders[j]);
                if (GUILayout.Button("×", GUILayout.Width(24))) { e.folders.RemoveAt(j); break; }
                EditorGUILayout.EndHorizontal();
            }
            EditorGUILayout.EndVertical();
        }

        void HandleFolderDrop(Rect rect, AtlasEntry e)
        {
            if (!rect.Contains(Event.current.mousePosition)) return;
            if (Event.current.type == UnityEngine.EventType.DragPerform)
            {
                DragAndDrop.AcceptDrag();
                foreach (string p in DragAndDrop.paths)
                    if (Directory.Exists(p) && !e.folders.Contains(p)) e.folders.Add(p);
                Event.current.Use();
            }
            if (Event.current.type == UnityEngine.EventType.DragUpdated)
                DragAndDrop.visualMode = DragAndDropVisualMode.Copy;
        }

        // ── 打包（复用 exe，固定路径）──────────────────────────────
        void PackAll() { for (int i = 0; i < _settings.packages.Count; i++) PackPackage(i); }

        void PackPackage(int idx)
        {
            var pkg = _settings.packages[idx];
            string exe = LoomExePath.Resolve();  // Task 7 的固定路径解析
            if (!File.Exists(exe)) { AppendLog($"[pack] exe 不存在: {exe}"); return; }
            string absSrc = ToAbs(Path.Combine(_settings.workspaceDir, pkg.sourceDir));
            string outPath = ToAbs(Path.Combine(_settings.pkgOutputDir, pkg.pkgName + ".pkg.bin"));
            Directory.CreateDirectory(Path.GetDirectoryName(outPath));
            string htmlArg = pkg.htmlFiles.Count > 0 ? string.Join(",", pkg.htmlFiles) : "";
            var sb = new StringBuilder();
            sb.Append('"').Append(absSrc).Append("\" ").Append(pkg.pkgName);
            if (pkg.htmlFiles.Count > 0) sb.Append(" --html ").Append(htmlArg);
            sb.Append(" --res ").Append(_settings.resDirName);
            sb.Append(" -o \"").Append(outPath).Append('"');
            try
            {
                var psi = new ProcessStartInfo(exe, sb.ToString())
                { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true,
                  StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8 };
                using var p = Process.Start(psi);
                string stderr = p.StandardError.ReadToEnd();
                p.WaitForExit();
                AppendLog(p.ExitCode == 0 ? $"[pack] {pkg.pkgName}: OK" : $"[pack] {pkg.pkgName}: FAIL\n{stderr}");
            }
            catch (Exception ex) { AppendLog($"[pack] {pkg.pkgName}: {ex.Message}"); }
            AssetDatabase.Refresh();
        }

        // ── 工具 ────────────────────────────────────────────────────
        string ToAbs(string unityRel)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, unityRel));
        }

        void AppendLog(string line) { _log.AppendLine(line); }

        void DrawLog()
        {
            EditorGUILayout.LabelField("日志", EditorStyles.boldLabel);
            EditorGUILayout.TextArea(_log.ToString(), GUILayout.Height(120));
        }
    }
}
```

- [ ] **Step 3: 确认编译**

`LoomConfigExporter` / `LoomAtlasSync` / `LoomWorkspaceInitializer` / `LoomExePath` 在 Task 6/7/8 建。本 task 先建最小桩让编译过：

`loomgui_unity/Assets/LoomGUI/Editor/LoomExePath.cs`：
```csharp
using System.IO;
namespace LoomGUI.Editor
{
    /// loomgui_pkg.exe 固定路径（随插件发布，不暴露给用户）。
    public static class LoomExePath
    {
        public static string Resolve()
        {
            string projRoot = Directory.GetParent(UnityEngine.Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, "Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe"));
        }
    }
}
```

`LoomConfigExporter` / `LoomAtlasSync` / `LoomWorkspaceInitializer` 在 Task 6/7/8 实现；本 task 先注释掉对它们的调用（`LoomConfigExporter.Export` / `LoomAtlasSync.SyncAll` / `LoomWorkspaceInitializer.Initialize` 三处），Task 6/7/8 实现后取消注释。

- [ ] **Step 4: 跑 fence_contract 确认 Rust 无回归**

```bash
cargo test -p loomgui_core fence_contract
```
Expected: 10 passed。

- [ ] **Step 5: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs loomgui_unity/Assets/LoomGUI/Editor/LoomExePath.cs
git rm loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs loomgui_unity/Assets/LoomGUI/Editor/LoomPackageManagerWindow.cs.meta
git commit -m "feat(unity): LoomSettingsWindow 三 tab 设置面板（替代 PackageManager）"
```

---

## Task 6: 图集 tab 同步 + 校验（LoomAtlasSync）

**Files:**
- Create: `loomgui_unity/Assets/LoomGUI/Editor/LoomAtlasSync.cs`
- Test: `loomgui_unity/Assets/LoomGUI/Tests/LoomAtlasSyncTests.cs`

**Interfaces:**
- Consumes: `LoomSettings.atlasEntries`（Task 1）。
- Produces: `LoomAtlasSync.SyncAll(LoomSettings)` + `SyncEntry`。LoomSettingsWindow 图集 tab 调用。

- [ ] **Step 1: 写失败测试 LoomAtlasSyncTests.cs**

```csharp
using System.Collections.Generic;
using System.Linq;
using NUnit.Framework;

namespace LoomGUI.Tests
{
    public class LoomAtlasSyncTests
    {
        // 纯逻辑测试：ScanPngs（扫文件夹下 PNG，相对路径集合）+ DiffPackables（算增删）。
        // 不碰真实 AssetDatabase（那需要 Unity 编辑器环境）。

        [Test]
        public void DiffPackables_AddsMissingRemovesExtra()
        {
            // 现有 atlas packables = {a, b}，扫描出 {b, c} → 应加 c 删 a。
            var current = new HashSet<string> { "a.png", "b.png" };
            var scanned = new HashSet<string> { "b.png", "c.png" };
            var (toAdd, toRemove) = LoomAtlasSync.DiffPackables(current, scanned);
            Assert.That(toAdd, Is.EquivalentTo(new[] { "c.png" }));
            Assert.That(toRemove, Is.EquivalentTo(new[] { "a.png" }));
        }

        [Test]
        public void DiffPackables_NoChange()
        {
            var current = new HashSet<string> { "a.png" };
            var scanned = new HashSet<string> { "a.png" };
            var (toAdd, toRemove) = LoomAtlasSync.DiffPackables(current, scanned);
            Assert.IsEmpty(toAdd);
            Assert.IsEmpty(toRemove);
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
# Unity Test Runner: LoomAtlasSyncTests
```
Expected: FAIL（LoomAtlasSync 不存在）。

- [ ] **Step 3: 写 LoomAtlasSync.cs**

```csharp
using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;
using UnityEngine.U2D;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 图集 packables 同步：扫 atlasEntry.folders 下 PNG，与 atlas 当前 packables diff，
    /// 增删 Sprite 引用。改用显式 Sprite 列表（修 B2：folder packable Unity 6 静默打成空）。
    /// </summary>
    public static class LoomAtlasSync
    {
        /// 纯逻辑：算 packables 增删（可单测，不碰 AssetDatabase）。
        public static (HashSet<string> toAdd, HashSet<string> toRemove) DiffPackables(
            HashSet<string> current, HashSet<string> scanned)
        {
            var toAdd = new HashSet<string>(scanned);
            toAdd.ExceptWith(current);
            var toRemove = new HashSet<string>(current);
            toRemove.ExceptWith(scanned);
            return (toAdd, toRemove);
        }

        /// 同步所有图集。Unity Editor only。
        public static void SyncAll(LoomSettings settings)
        {
            if (settings == null) return;
            foreach (var entry in settings.atlasEntries)
            {
                SyncEntry(settings, entry);
            }
            EditorUtility.SetDirty(settings);
            AssetDatabase.SaveAssetIfDirty(settings);
        }

        /// 同步单个图集：确保 atlas 资产存在 + packables = folders 下所有 Sprite。
        public static void SyncEntry(LoomSettings settings, AtlasEntry entry)
        {
            if (entry == null || string.IsNullOrEmpty(entry.atlasName)) return;

            // 确保图集资产存在（不存在则创建）。
            if (entry.atlas == null)
            {
                string atlasPath = $"Assets/LoomUI/res/{entry.atlasName}.spriteatlas";
                entry.atlas = AssetDatabase.LoadAssetAtPath<SpriteAtlas>(atlasPath);
                if (entry.atlas == null)
                {
                    entry.atlas = new SpriteAtlas();
                    AssetDatabase.CreateAsset(entry.atlas, atlasPath);
                }
            }

            // 扫 folders 下 PNG → Sprite 引用集合。
            var scannedSprites = new HashSet<string>();
            var toAdd = new List<UnityEngine.Object>();
            foreach (var folder in entry.folders)
            {
                if (string.IsNullOrEmpty(folder)) continue;
                string absFolder = ToAbs(folder);
                if (!Directory.Exists(absFolder)) continue;
                foreach (var png in Directory.GetFiles(absFolder, "*.png", SearchOption.AllDirectories))
                {
                    string assetPath = ToAssetPath(png);
                    var importer = AssetImporter.GetAtPath(assetPath) as TextureImporter;
                    if (importer != null && importer.textureType != TextureImporterType.Sprite)
                    {
                        importer.textureType = TextureImporterType.Sprite;
                        importer.SaveAndReimport();
                    }
                    var sp = AssetDatabase.LoadAssetAtPath<Sprite>(assetPath);
                    if (sp != null) { scannedSprites.Add(assetPath); toAdd.Add(sp); }
                }
            }

            // 显式设 packables（替 folder packable，修 B2）。
            entry.atlas.SetPackables(toAdd.ToArray());
            EditorUtility.SetDirty(entry.atlas);
        }

        static string ToAbs(string unityRel)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            return Path.GetFullPath(Path.Combine(projRoot, unityRel));
        }

        static string ToAssetPath(string abs)
        {
            string projRoot = Directory.GetParent(Application.dataPath).FullName.Replace('\\', '/');
            return abs.Replace('\\', '/').Replace(projRoot + "/", "");
        }
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
# Unity Test Runner: LoomAtlasSyncTests
```
Expected: 2 passed。

- [ ] **Step 5: LoomSettingsWindow 取消 LoomAtlasSync.SyncAll 注释**

Task 5 注释的 `LoomAtlasSync.SyncAll(_settings);` 取消注释。

- [ ] **Step 6: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Editor/LoomAtlasSync.cs loomgui_unity/Assets/LoomGUI/Tests/LoomAtlasSyncTests.cs loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs
git commit -m "feat(unity): 图集 packables 自动同步（显式 Sprite 列表，修 B2）"
```

---

## Task 7: config.json 导出（LoomConfigExporter）+ exe 落位

**Files:**
- Create: `loomgui_unity/Assets/LoomGUI/Editor/LoomConfigExporter.cs`
- Test: `loomgui_unity/Assets/LoomGUI/Tests/LoomConfigExporterTests.cs`
- Copy: `target/release/loomgui_pkg.exe` → `loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe`

**Interfaces:**
- Consumes: `LoomSettings`（Task 1）。
- Produces: `LoomConfigExporter.Export(LoomSettings)` 写 config.json（全相对工作区根）。LoomSettingsWindow 改配置自动调。

- [ ] **Step 1: 编 loomgui_pkg.exe**

```bash
cargo build --release -p loomgui_pkg
```

- [ ] **Step 2: 拷 exe 到插件固定位置**

```bash
mkdir -p loomgui_unity/Assets/LoomGUI/Editor/Tools
cp target/release/loomgui_pkg.exe loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe
# exe 设 .meta：Unity 里设 import settings（或留 default）。.exe 加 .meta 后入库。
```

- [ ] **Step 3: 写失败测试 LoomConfigExporterTests.cs**

```csharp
using NUnit.Framework;
using UnityEngine;

namespace LoomGUI.Tests
{
    public class LoomConfigExporterTests
    {
        [Test]
        public void Export_PathsRelativeToWorkspace()
        {
            var s = ScriptableObject.CreateInstance<LoomSettings>();
            s.workspaceDir = "Assets/LoomUI/";
            s.resDirName = "res";
            s.pkgOutputDir = "Assets/StreamingAssets/";
            s.packages.Add(new PackageEntry("showcase", "showcase"));
            s.packages[0].htmlFiles.Add("home.html");

            string json = LoomGUI.Editor.LoomConfigExporter.BuildJson(s);
            // exe_path 相对工作区根：Assets/LoomUI/ → Assets/LoomGUI/Editor/Tools/ = ../LoomGUI/Editor/Tools/loomgui_pkg.exe
            StringAssert.Contains("\"exe_path\": \"../LoomGUI/Editor/Tools/loomgui_pkg.exe\"", json);
            // output_dir 相对工作区根：Assets/LoomUI/ → Assets/StreamingAssets/ = ../../StreamingAssets/
            StringAssert.Contains("\"output_dir\": \"../../StreamingAssets/\"", json);
            StringAssert.Contains("\"res_dir\": \"res\"", json);
            StringAssert.Contains("\"name\": \"showcase\"", json);
            StringAssert.Contains("\"source\": \"showcase\"", json);
            StringAssert.Contains("\"home.html\"", json);
        }
    }
}
```

- [ ] **Step 4: 跑测试确认失败**

```bash
# Unity Test Runner: LoomConfigExporterTests
```
Expected: FAIL（LoomConfigExporter 不存在）。

- [ ] **Step 5: 写 LoomConfigExporter.cs**

```csharp
using System.IO;
using System.Text;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// LoomSettings → config.json 导出（AI 在 open-design 里读此调 exe 验证+打包）。
    /// 全相对工作区根（可移植）。LoomSettingsWindow 改配置自动调 Export。
    /// </summary>
    public static class LoomConfigExporter
    {
        /// 纯逻辑：构建 config.json 字符串（可单测，不碰磁盘）。
        public static string BuildJson(LoomSettings s)
        {
            // exe_path：工作区根 → Assets/LoomGUI/Editor/Tools/。两者都在 Assets/ 下。
            // workspaceDir = Assets/LoomUI/ → 深度 2（Assets/, LoomUI/），回到 Assets/ 需 ../，再进 LoomGUI/...
            string exeRel = RelativeFromWorkspace(s.workspaceDir, "Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe");
            string outRel = RelativeFromWorkspace(s.workspaceDir, s.pkgOutputDir);

            var sb = new StringBuilder();
            sb.Append("{\n");
            sb.Append($"  \"exe_path\": \"{exeRel}\",\n");
            sb.Append($"  \"res_dir\": \"{s.resDirName}\",\n");
            sb.Append($"  \"output_dir\": \"{outRel}\",\n");
            sb.Append("  \"packages\": [");
            for (int i = 0; i < s.packages.Count; i++)
            {
                var p = s.packages[i];
                if (i > 0) sb.Append(",");
                sb.Append("\n    {");
                sb.Append($"\"name\": \"{p.pkgName}\", ");
                sb.Append($"\"source\": \"{p.sourceDir}\", ");
                sb.Append("\"html\": [");
                for (int j = 0; j < p.htmlFiles.Count; j++)
                {
                    if (j > 0) sb.Append(", ");
                    sb.Append($"\"{p.htmlFiles[j]}\"");
                }
                sb.Append("]}");
            }
            sb.Append(s.packages.Count > 0 ? "\n  ]\n" : "]\n");
            sb.Append("}\n");
            return sb.ToString();
        }

        /// 写 config.json 到工作区 .claude/skills/loomgui-editor/config.json。
        public static void Export(LoomSettings s)
        {
            if (s == null || string.IsNullOrEmpty(s.workspaceDir)) return;
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            string cfgPath = Path.GetFullPath(Path.Combine(projRoot, s.workspaceDir, ".claude/skills/loomgui-editor/config.json"));
            Directory.CreateDirectory(Path.GetDirectoryName(cfgPath));
            File.WriteAllText(cfgPath, BuildJson(s), Encoding.UTF8);
        }

        /// 算 from（工作区根）→ to 的相对路径。两者都是 Unity 工程相对（Assets/...）。
        static string RelativeFromWorkspace(string workspaceDir, string targetDir)
        {
            // 简化：工作区根 = Assets/LoomUI/（深度2）。targetDir 在 Assets/ 下。
            // 用 Uri.MakeRelativeUri 算相对路径。
            string projRoot = Directory.GetParent(Application.dataPath).FullName.Replace('\\', '/');
            string from = Path.GetFullPath(Path.Combine(projRoot, workspaceDir)).Replace('\\', '/');
            string to = Path.GetFullPath(Path.Combine(projRoot, targetDir)).Replace('\\', '/');
            if (!from.EndsWith("/")) from += "/";
            var uriFrom = new System.Uri(from);
            var uriTo = new System.Uri(to);
            return System.Uri.UnescapeDataString(uriFrom.MakeRelativeUri(uriTo).ToString());
        }
    }
}
```

- [ ] **Step 6: 跑测试确认通过**

```bash
# Unity Test Runner: LoomConfigExporterTests
```
Expected: 1 passed。

- [ ] **Step 7: LoomSettingsWindow 取消 LoomConfigExporter.Export 注释**

Task 5 注释的 `LoomConfigExporter.Export(_settings);` 取消注释。

- [ ] **Step 8: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Editor/LoomConfigExporter.cs loomgui_unity/Assets/LoomGUI/Tests/LoomConfigExporterTests.cs loomgui_unity/Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe* loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs
git commit -m "feat(unity): config.json 导出（AI 读此调 exe）+ exe 随插件发布"
```

---

## Task 8: 工作区初始化 + AssetPostprocessor

**Files:**
- Create: `loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceInitializer.cs`
- Create: `loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceAssetPostprocessor.cs`
- Create: `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md`（迁自 editor/rules/claude/CLAUDE.md.tmpl，去 `## 生成完必须跑验证` 段改 config.json 调 exe）
- Create: `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/SKILL.md` + `references/{fence.md,preview-polyfill.html,preview-trust.md}`（迁自 editor/skill/）

**Interfaces:**
- Consumes: `LoomSettings`（Task 1）+ 插件 Editor Resources 下围栏规则/skill 文本。
- Produces: `LoomWorkspaceInitializer.Initialize(LoomSettings)`。LoomSettingsWindow 工作区 tab 调用。

- [ ] **Step 1: 迁围栏规则文本到 Editor Resources**

`editor/rules/claude/CLAUDE.md.tmpl` 内容（去掉 `## 生成完必须跑验证` 段，改为 config.json 调 exe 说明）拷到 `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/fence-rules.md`（用 Write 工具，内容来自现有 tmpl + 改打包段）。

新版 fence-rules.md 末尾的"生成完必须跑验证"段改为：
```markdown
## 生成完跑验证+打包
生成 HTML+CSS 后，读 `.claude/skills/loomgui-editor/config.json` 拿 exe_path + 配置，调
`loomgui_pkg.exe <sourceDir> <pkgName> --html <list> --res <name> -o <out>` 验证+打包。
非零退出 = 围栏违规，读 stderr 自纠后重跑。零退出 = pkg.bin 产出。
```

- [ ] **Step 2: 迁 skill 内容到 Editor Resources**

拷 `editor/skill/loomgui-editor/{SKILL.md,references/fence.md,references/preview-polyfill.html,references/preview-trust.md}` → `loomgui_unity/Assets/LoomGUI/Editor/Resources/LoomGUI/skill/`（保持结构）。

SKILL.md 的"工作流"段改：第 3 步"跑 `node tools/pack.mjs`"改为"读 config.json 调 loomgui_pkg.exe"（同 fence-rules.md 末段）。砍 `tools/pack.mjs` 引用。

- [ ] **Step 3: 写 LoomWorkspaceInitializer.cs**

```csharp
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 工作区初始化（迁自 editor/init.mjs）：注围栏规则 + 分发 skill + 写 config.json。
    /// 围栏规则/skill 内容从插件 Editor Resources 读出注入工作区。
    /// </summary>
    public static class LoomWorkspaceInitializer
    {
        const string BEGIN = "<!-- loomgui-editor-begin -->";
        const string END = "<!-- loomgui-editor-end -->";

        public static void Initialize(LoomSettings s)
        {
            if (s == null || string.IsNullOrEmpty(s.workspaceDir)) return;
            string projRoot = Directory.GetParent(Application.dataPath).FullName;
            string ws = Path.GetFullPath(Path.Combine(projRoot, s.workspaceDir));
            Directory.CreateDirectory(ws);

            InjectFenceRules(ws);
            DistributeSkill(ws);
            LoomConfigExporter.Export(s);  // Task 7
            AssetDatabase.Refresh();
        }

        /// 注围栏规则到工作区 CLAUDE.md（标签段增量合并）。
        static void InjectFenceRules(string ws)
        {
            var tmpl = Resources.Load<TextAsset>("LoomGUI/fence-rules");
            if (tmpl == null) { Debug.LogError("[LoomGUI] fence-rules.md not found in Resources"); return; }
            string content = tmpl.text;
            string tagged = content.Contains(BEGIN) ? content : $"{BEGIN}\n{content}\n{END}\n";
            string target = Path.Combine(ws, "CLAUDE.md");
            if (!File.Exists(target)) { File.WriteAllText(target, tagged, Encoding.UTF8); return; }
            string existing = File.ReadAllText(target);
            if (!existing.Contains(BEGIN))
            {
                File.WriteAllText(target, existing.TrimEnd('\n') + "\n\n" + tagged, Encoding.UTF8);
                return;
            }
            string updated = System.Text.RegularExpressions.Regex.Replace(
                existing, $"{BEGIN}[^]*?{END}", tagged.TrimEnd());
            File.WriteAllText(target, updated, Encoding.UTF8);
        }

        /// 分发 skill 到工作区 .claude/skills/loomgui-editor/。
        static void DistributeSkill(string ws)
        {
            string dest = Path.Combine(ws, ".claude/skills/loomgui-editor");
            Directory.CreateDirectory(dest);
            CopyResource("LoomGUI/skill/SKILL", Path.Combine(dest, "SKILL.md"));
            string refs = Path.Combine(dest, "references");
            Directory.CreateDirectory(refs);
            CopyResource("LoomGUI/skill/references/fence", Path.Combine(refs, "fence.md"));
            CopyResource("LoomGUI/skill/references/preview-polyfill", Path.Combine(refs, "preview-polyfill.html"));
            CopyResource("LoomGUI/skill/references/preview-trust", Path.Combine(refs, "preview-trust.md"));
        }

        static void CopyResource(string resPath, string destFile)
        {
            var ta = Resources.Load<TextAsset>(resPath);
            if (ta == null) { Debug.LogWarning($"[LoomGUI] resource not found: {resPath}"); return; }
            File.WriteAllText(destFile, ta.text, Encoding.UTF8);
        }
    }
}
```

- [ ] **Step 4: 写 LoomWorkspaceAssetPostprocessor.cs**

```csharp
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LoomGUI.Editor
{
    /// <summary>
    /// 拦工作区下非资源文件（.html/.css/.claude/CLAUDE.md/design-systems/.od-skills）不让 Unity 导入。
    /// 这些是给 AI/open-design 用的纯文本，导入会生成多余 .meta + 尝试导入 .css。
    /// PNG 正常导入为 Sprite（进 SpriteAtlas）。
    /// </summary>
    public sealed class LoomWorkspaceAssetPostprocessor : AssetPostprocessor
    {
        static bool ShouldSkip(string assetPath)
        {
            // 工作区根从 LoomSettings 拿（运行时配置资产）。
            var s = LoomSettings.GetOrCreateDefault();
            if (s == null || string.IsNullOrEmpty(s.workspaceDir)) return false;
            string ws = s.workspaceDir.Replace('\\', '/').TrimEnd('/') + "/";
            string p = assetPath.Replace('\\', '/');
            if (!p.StartsWith(ws)) return false;

            string name = Path.GetFileName(p);
            if (name == "CLAUDE.md") return true;
            if (p.Contains("/.claude/")) return true;
            if (p.Contains("/.od-skills/")) return true;
            if (p.Contains("/design-systems/")) return true;
            if (p.EndsWith(".html") || p.EndsWith(".css")) return true;
            return false;
        }

        void OnPreprocessAsset()
        {
            if (ShouldSkip(assetPath))
            {
                // 跳过导入：让 Unity 不生成 importer / 不尝试解析。
                var importer = assetImporter as AssetImporter;
                if (importer != null) importer.SetNonAsset();  // Unity 6：标记为非资产不入库
            }
            // PNG 强制 Sprite 导入（进 SpriteAtlas）。
            if (assetPath.EndsWith(".png"))
            {
                var ti = assetImporter as TextureImporter;
                if (ti != null && ti.textureType != TextureImporterType.Sprite)
                {
                    ti.textureType = TextureImporterType.Sprite;
                }
            }
        }
    }
}
```

注：`SetNonAsset()` 是 Unity 6 API；若版本不符，退回 `importer.SaveAndReimport()` 跳过 + `AssetDatabase.MoveAssetToTrash` 或忽略。plan 执行时按 Unity 6.5 实际 API 验。

- [ ] **Step 5: LoomSettingsWindow 取消 LoomWorkspaceInitializer 注释**

Task 5 注释的 `LoomWorkspaceInitializer.Initialize(_settings);` 取消注释。

- [ ] **Step 6: 跑全部测试**

```bash
# Unity Test Runner: All Tests
```
Expected: 全过（SpriteResolver 3 + LoomAtlasSync 2 + LoomConfigExporter 1 + 既有）。

- [ ] **Step 7: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceInitializer.cs loomgui_unity/Assets/LoomGUI/Editor/LoomWorkspaceAssetPostprocessor.cs loomgui_unity/Assets/LoomGUI/Editor/Resources/ loomgui_unity/Assets/LoomGUI/Editor/LoomSettingsWindow.cs
git commit -m "feat(unity): 工作区初始化 C# + AssetPostprocessor 拦非资源文件"
```

---

## Task 9: 删 samples/editor + 文档同步 + 重打 pkg.bin + build .dll

**Files:**
- Delete: `samples/`（整目录）
- Delete: `editor/`（整目录）
- Move: `samples/design-systems/loomgui/` → `loomgui_unity/Assets/LoomUI/design-systems/`
- Modify: `CLAUDE.md` / `README.md` / `docs/roadmap/roadmap.md` / `docs/design/fence.md`

**Interfaces:**
- Consumes: 全部前置 task。

- [ ] **Step 1: 迁 design-systems 到 Unity 内**

```bash
mv samples/design-systems/loomgui loomgui_unity/Assets/LoomUI/design-systems
```

- [ ] **Step 2: 删 samples/ + editor/**

```bash
rm -rf samples editor
```

- [ ] **Step 3: 改 CLAUDE.md**

`## 在本仓库怎么干活` 段：删 `editor/` 和 `samples/` 的描述，改为说明工作区在 `loomgui_unity/Assets/LoomUI/`（设计师工作区 + res + 包源），编辑器工作流用 `LoomGUI > Settings` 面板。

- [ ] **Step 4: 改 README.md**

- 项目结构表：删 `editor/` 和 `samples/` 行。
- `:48`：`loomgui_pkg/ | 打包器 CLI（HTML+CSS+资源 → .pkg.bin + 图集...）` → 去掉"+ 图集"（图集归 Unity SpriteAtlas）。

- [ ] **Step 5: 改 docs/roadmap/roadmap.md §3（v-other editor）**

段落改"Unity 内 C# 实现"——去掉 init.mjs / pack.mjs / 三 harness 描述，改为 LoomSettingsWindow 工作区 tab 初始化 + config.json + open-design import 工作区。

- [ ] **Step 6: 改 docs/design/fence.md §5（围栏副本分发消费者表）**

editor 行改"Unity 插件 Editor Resources 注入（LoomWorkspaceInitializer）"。

- [ ] **Step 7: 重打 showcase pkg.bin**

Unity 打开 → `LoomGUI > Settings` → 包管理 tab → 打包 showcase → 产 `Assets/StreamingAssets/showcase.pkg.bin`。

或手动：
```bash
cargo build --release -p loomgui_pkg
./target/release/loomgui_pkg.exe <abs showcase dir> showcase --html home.html,page_*.html --res res -o <abs>/Assets/StreamingAssets/showcase.pkg.bin
```

- [ ] **Step 8: build .dll（确认 Rust 无回归）**

本 plan 没改 Rust 源码，但末尾确认 .dll 不回归：
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
```
（Unity 关着拷。）

- [ ] **Step 9: 跑 fence_contract**

```bash
cargo test -p loomgui_core fence_contract
```
Expected: 10 passed。

- [ ] **Step 10: Commit + push（家里机验收）**

```bash
git add -A
git commit -m "chore: 删 samples/editor + 文档同步 + 重打 pkg.bin

- samples/editor 删（角色被 Unity 内工作区取代）
- design-systems 迁 Assets/LoomUI/design-systems/
- CLAUDE.md/README/roadmap/fence.md 同步
- 重打 showcase pkg.bin

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push
```

家里机 pull → Unity PlayMode 验收：showcase 各页图片正常显示（不白块）+ 工作区初始化 + AI 调 exe 打包闭环。
