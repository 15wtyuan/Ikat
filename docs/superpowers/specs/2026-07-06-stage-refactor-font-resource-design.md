# LoomStage 重构 + 字体管理 + 资源整理 + 发布按钮统一

> 2026-07-06，代码 review 讨论纪要 + brainstorming 澄清定稿。覆盖问题 #1 #2 #3 #5 #6 #7。
> 问题 #8 确认，#4 保留，#9 #10 待讨论。
> 决策依据见文末「§6 澄清决策记录」。

---

## 0. 背景：用户 code review 提出的 10 个问题

| # | 问题 | 决策 |
|---|------|------|
| 1 | 改打包器代码后新 exe 没有关联到 `Editor/Tools/` | CI Windows artifact 加 `.exe` |
| 2 | `fence.md` 含大量【推断·待测】标记 | 清标记 + 留 TODO + roadmap 挂"围栏契约补完" |
| 3 | `preview-trust.md` / `SKILL.md` / `fence-rules.md` 过时（如说 absolute 不支持） | 立即修正已知过时 + roadmap 挂"skill 文档全量对齐" |
| 4 | `loomgui_stage_load_html` 是否死代码 | 保留（调试/原型入口，ABI 测试重度使用） |
| 5 | `LoomShowcaseDriver.cs` 放 UPM 包里不对（demo 代码） | 移到 `loomgui_unity/Assets/LoomUI/Demo/`，新建 `LoomGUI.Demo.asmdef` |
| 6 | `_fontFile` 应阻止手改 | 随着字体管理重构，`_font`/`_fontFile` 字段废弃，不需要单独处理 |
| 7 | LoomStage 不支持动态挂载、字体管理零散 | 本 spec 的核心内容 |
| 8 | `LoomSettings.asset` 是自动生成的吗 | 确认：`GetOrCreateDefault()` 自动创建 |

---

## 1. LoomStage / LoomStageDriver 拆分（严格拆分）

### 1.1 动机

- 当前 `LoomStage` 是 `[ExecuteAlways]` 的 MonoBehaviour，`Awake()` 里绑死了 `_font` Inspector 字段、UI 相机、根 transform。
- 运行时 `AddComponent` 挂载时 `_font` 为 null → 初始化失败。
- 需要支持动态创建 Stage、多 Stage 实例。
- 单体 MonoBehaviour 把"Rust 句柄/字体表/渲染池"（引擎无关）和"相机/transform/FPS"（Unity 依赖）混在一起，边界不清。

### 1.2 新结构（严格拆分：Unity 依赖全部留 Driver）

```
LoomStage（纯 C# class，非 UnityEngine.Object）
├── 构造: LoomStage(Vector2 designSize = default)
│   └── designSize 默认 (1080,1920)。内部调 loomgui_stage_new(w,h)（不收字体路径）
├── RegisterFont(family, bytes, unityFont, isDefault)
│   └── bytes 喂 Rust（loomgui_stage_register_font）；unityFont 存 stage 内 _unityFonts 表（光栅用，见 §2.5）
├── Tick(dt, driverCtx): 输入→tick→borrow_frame→pool.Sync→事件派发
│   └── driverCtx 传入 transform/Camera（Unity 依赖）；Font 资源走 _unityFonts 表（RegisterFont 时存）
├── 公开 API（不变）: CreateRoot/CreateNode/LoadPackage/Instantiate/
│   SetText/SetStyle/SetSrc/SetScrollPos/Tween/KillTween/ClearAnim/
│   FindNodeById/SetNodeDisabled/SetReuseKey/BindNativeHost/...
├── Dispose(): FreeStage + pool/mixer/nhm/sprites 清理
└── 无 Unity 依赖（Camera/transform 通过 Tick 的 driverCtx 参数注入；Font asset 走 _unityFonts 表）

LoomStageDriver（MonoBehaviour, [ExecuteAlways]）
├── [SerializeField] Vector2 _designSize = (1080,1920)
├── [SerializeField] Camera _uiCamera（可选，不设则动态建）
├── [SerializeField] bool _showFps, bool _safeArea = true
├── [SerializeField] LoomInputCollector _inputCollector（可选）
├── Awake():
│   1. new LoomStage(_designSize)
│   2. _sprites.Init(LoomSettings.GetOrCreateDefault())
│   3. RegisterFontsFromSettings()
│   4. EnsureCamera() + ConfigureTransforms()
│   5. Font.textureRebuilt += OnFontRebuilt（per-stage 版本号，见 §2.6）
│   6. 不再等 CreateRoot——tick 放行（Rust scene=None 返空帧不 panic，见 §1.4）
├── LateUpdate(): resize检测 + 输入采集 + _stage.Tick(dt, this)
├── OnGUI(): FPS 显示
├── OnDestroy(): _stage.Dispose() + 解绑静态事件
├── Stage 属性: public LoomStage Stage => _stage
├── RegisterFontsFromSettings(): protected virtual
│   └── 遍历 LoomSettings.fonts → 读 .bytes 字节 → stage.RegisterFont
├── LoadFontBytes(entry): protected virtual
│   └── 默认从 StreamingAssets/fonts/{sourceFileName} 读（见 §2.4）
└── 每 stage 一个独立相机（挂在 driver GO 的子节点）
```

### 1.3 调用方

```csharp
// A: Inspector 预挂 → Awake 自动完成
var driver = GetComponent<LoomStageDriver>();
driver.Stage.CreateRoot("div", "...");

// B: 运行时 AddComponent
var go = new GameObject("UI");
var driver = go.AddComponent<LoomStageDriver>();
driver.Stage.CreateRoot("div", "...");

// C: 完全手动（不依赖 Driver）
var stage = new LoomStage();
stage.RegisterFont("Noto Sans SC", bytes, font, isDefault: true);
stage.CreateRoot("div", "...");
```

### 1.4 `_sceneBuilt` 闸门去除

现状 `LateUpdate` 用 `_sceneBuilt` bool 闸门：`CreateRoot` 成功才放行 tick，防 EditMode scene=None 时 Rust panic。

**去除依据**：Rust 侧 `tick_and_render` 已修（坑 102，`stage.rs:520` `match self.scene.as_mut() { None => return FrameData::default() }`），scene=None 返空帧不 panic。C# 闸门冗余。

改后：Awake 后 `LateUpdate` 每帧 tick；scene 未建时 Rust 返空帧（零副作用，多一次 FFI 调用可忽略）。`CreateRoot` 不再翻闸门。

**实现注意**：`LateUpdate` 收到空 blob（`borrow_frame` 返 null/len=0）时要早返，不调 `_pool.Sync`——现状 `if (ptr != null && len > 0)` 已 guard，保留。

### 1.5 迁移：SampleScene

`LoomStage` 从 MonoBehaviour 变纯 class，SampleScene 里挂在 GO 上的 `LoomStage` component 引用失效。迁移：

- SampleScene 该 GO 改挂 `LoomStageDriver`（新 MonoBehaviour）。
- `LoomShowcaseDriver._stage` 字段类型从 `LoomStage`（旧 MonoBehaviour）改为 `LoomStageDriver`，取 `.Stage` 拿纯 class。
- _designSize/_uiCamera/_showFps/_safeArea 在 Driver 上重配。
- 字体不再 Inspector 单字段，改走 LoomSettings.fonts（§2）。

EditMode 测试不实例化 LoomStage（现有测试都是纯逻辑单测），无测试迁移。

---

## 2. 字体管理（完整多字体选择）

### 2.1 现状与改动本质

现状：
- `Stage` 持 `Arc<Font>` 单字体；`solve`/`build_render_nodes` 把 `&self.font` 贯穿到 `measure_text(font: &Font)`。
- `font-family` CSS 已解析进 `ResolvedStyle.font_family` + 拷进 `TextState.font_family`，**但 measure/render 没读它**——单字体强贯穿。

改动本质：让 `ts.font_family` 真正生效——Stage 持字体表，measure/render 按节点 `font_family` 选字体，无匹配回退 default。

### 2.2 Rust 核心：FontTable

```rust
/// 字体表：family → Font。无匹配回退 default。
/// Font 仍是 Face<'static>（Box::leak 字节，进程级单字体可接受，多字体同理——
/// 字体数量有限，leak 不释放可接受；真要回收改 Arc<Vec<u8>> 持字节，YAGNI）。
pub struct FontTable {
    fonts: HashMap<String, Arc<Font>>,
    default_family: String,   // register_font(is_default=true) 设；首次注册自动 default
}

impl FontTable {
    pub fn new() -> Self;
    pub fn register(&mut self, family: &str, bytes: Vec<u8>, is_default: bool) -> Result<(), String>;
    /// 按节点 font_family 选字体。None / 无匹配 → default。
    pub fn select(&self, family: Option<&str>) -> &Font;
}
```

`Stage` 字段：`font: Arc<Font>` → `fonts: FontTable`。

### 2.3 Rust 核心：measure 切口

`measure_text` 签名**不动**（仍收 `font: &Font`）——调用方先 `font_table.select(family)` 选好再传。切口收敛到两处调用点：

1. **layout/mod.rs measure 闭包**（`MeasureContext::Text` 分支）：
   - `MeasureContext::Text` 加 `family: Option<String>` 字段（build 时从 `s.font_family.clone()` 填）。
   - 闭包内 `let font = fonts.select(ctx.family.as_deref()); measure_text(..., font);`

2. **render/mod.rs `NodeKind::Text` 分支**（fallback measure，`text_layouts` miss 时）：
   - 从 `n.style.font_family` 取 family → `fonts.select(...)` → `measure_text(..., font)`。

3. **签名**：`solve(scene, &Font, ...)` → `solve(scene, &FontTable, ...)`；`build_render_nodes(scene, &Font, ...)` → `build_render_nodes(scene, &FontTable, ...)`。Stage.tick_and_render 内部传 `&self.fonts`。

### 2.4 字体字节加载策略（统一只走 .bytes）

**决策**：editor + build 都走 `StreamingAssets/fonts/{sourceFileName}.bytes`。不再 editor 直读 Font asset 源文件路径。

取舍：editor 迭代字体（换 ttf）要重新「发布」拷贝 .bytes 才能生效。换来代码统一一条路径，无 `#if UNITY_EDITOR` 分流。

```csharp
// LoomStageDriver 默认实现：
protected virtual void RegisterFontsFromSettings() {
    var settings = LoomSettings.GetOrCreateDefault();
    foreach (var entry in settings.fonts) {
        byte[] bytes = LoadFontBytes(entry);
        if (bytes != null)
            _stage.RegisterFont(entry.familyName, bytes, entry.font, entry.isDefault);
    }
}

protected virtual byte[] LoadFontBytes(FontEntry entry) {
    // 默认：从 StreamingAssets/fonts/{sourceFileName} 读。
    // sourceFileName = 源文件名（去路径，如 "NotoSansSC.ttc"）。
    // 发布时拷成 StreamingAssets/fonts/NotoSansSC.ttc.bytes（见 §3.3）。
    string path = Path.Combine(Application.streamingAssetsPath, "fonts", entry.sourceFileName + ".bytes");
    return File.Exists(path) ? File.ReadAllBytes(path) : null;
}
```

项目可 override `LoadFontBytes` 换 Addressables / Resources / 下载。

### 2.5 Unity 光栅侧字体（仍读 Font asset）

Unity 侧光栅（`TextRasterizer.BuildMesh`）仍用 `Font` asset（引擎动态字体 API），不走 .bytes。两份指向同一份 ttf：

- **Rust 测量**：读 `.bytes`（ttf 字节）→ ttf-parser。
- **Unity 光栅**：读 `FontEntry.font`（Font asset）→ `GetCharacterInfo`。

两份必须是同一份 ttf，否则 advance 不一致 → 字距错位（坑 119 根因）。`LoomSettings.fonts` 同时持有 `font`（asset，给 Unity）+ `sourceFileName`（给 driver 拼接 .bytes 路径喂 Rust）。

`LoomStage.RegisterFont(family, bytes, unityFont, isDefault)`：`bytes` 喂 Rust（测量）；`unityFont` 存进 stage 的 `Dictionary<string, Font> _unityFonts`（family → Font asset，光栅用）+ 记 `defaultUnityFont`。

`MirrorPool.Sync` 光栅 text 节点时按节点 `font_family` 从 `_unityFonts` 选对应 Font asset（无匹配回退 default）。`Tick(dt, driverCtx)` 内部把 `_unityFonts` + `FontVersion` 透传给 MirrorPool.Sync——不再单 `_font` 字段。

光栅侧选字体与 Rust 测量侧选字体**对称**：都按 `ts.font_family` 查表 + fallback default，两侧表内容一致（同一组 RegisterFont 注册）。

### 2.6 textureRebuilt：per-stage 版本号

现状 `TextRasterizer.OnRebuilt` 是全局 `static s_fontVersion++`，多 Stage 时一个 stage 字体 atlas rebuild 会脏所有 stage 的 text。

**改 per-stage**：`TextRasterizer` 不再持全局版本号，改为 `LoomStage`（或 Driver）持 `int _fontVersion` + 实例方法 `OnFontRebuilt`。`Font.textureRebuilt` 静态事件注册时绑当前 stage 的实例方法（`Font.textureRebuilt += _stage.OnFontRebuilt`），`OnDestroy` 解绑。MirrorPool.Sync 比对该 stage 自己的版本号。

结构改动：`TextRasterizer.FontVersion`（静态）→ `LoomStage.FontVersion`（实例）。`MirrorPool.Sync` 读 stage 的版本号而非静态。

---

## 3. LoomSettingsWindow 「发布」按钮

### 3.1 现状

当前面板散落多个独立按钮：
- 工作区 tab：初始化工作区
- 包管理 tab：单个"打包" + "一键打包全部"
- 图集 tab："同步此图集" + "同步全部图集 packables"

分散、顺序不明确，容易漏步骤。

### 3.2 改后：保留独立按钮 + 底部统一「发布」

**决策**：各 tab 原有独立按钮**保留**（单步调试用），底部加统一「发布」按钮（一键全流程）。独立=迭代单包/单图集，发布=全量。

```
┌─ 发布（底部统一按钮）────────────────────────────────┐
│                                                     │
│  1. 同步全部图集 packables（LoomAtlasSync.SyncAll）   │
│     └─ 确保 workspace/atlas/*.spriteatlasv2 存在     │
│     └─ 扫 folders → Sprite → atlas packables         │
│                                                     │
│  2. 打包全部 package（loomgui_pkg.exe × N）           │
│     └─ .pkg.bin → {pkgOutputDir}/ui/{name}.pkg.bin   │
│                                                     │
│  3. 发布字体                                         │
│     └─ 拷 workspace/fonts/*.{ttf,ttc,otf}            │
│     └─ → {pkgOutputDir}/fonts/{源文件名}.{ext}.bytes  │
│     └─ 改后缀加 .bytes（保留原扩展名作信息）          │
│                                                     │
│  4. 导出 config.json（现有逻辑，不变）                  │
│                                                     │
│  [ 发布 ]                                           │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### 3.3 资源输出目录结构

```
StreamingAssets/                   ← LoomSettings.pkgOutputDir
├── ui/                            ← .pkg.bin
│   ├── showcase.pkg.bin
│   └── battle.pkg.bin
└── fonts/                         ← .bytes（发布字体时拷贝）
    ├── NotoSansSC.ttc.bytes
    └── GameFont.ttf.bytes
```

图集是 Unity 原生 SpriteAtlas，不进 StreamingAssets——Unity build pipeline 自动包含。

**字节文件名**：`{源文件名}.{原扩展名}.bytes`（如 `NotoSansSC.ttc` → `NotoSansSC.ttc.bytes`）。源文件名与 `familyName`（CSS 值）解耦——familyName 改了不重复拷贝。driver 按 `FontEntry.sourceFileName` 拼 .bytes 路径。

### 3.4 打包器 CLI（无 --fonts 参数）

```bash
loomgui_pkg.exe <sourceDir> <pkgName> \
  --html <list> \
  --res-root <工作区根/res> \
  -o <pkgOutputDir>/ui/<pkgName>.pkg.bin
```

打包器**没有** `--fonts` 参数（现状本就没有，字体由面板直接拷贝，不走 exe）。spec §3.4 原文"去掉旧的 `--fonts` 参数"是基于错误假设，实际无需改动。

### 3.5 面板实现

```csharp
// LoomSettingsWindow 新增枚举 + 字体 tab
enum Tab { Workspace, Packages, Atlas, Fonts }

// 底部统一按钮（所有 tab 都显示）
void DrawPublishButton() {
    EditorGUILayout.Space(12);
    if (GUILayout.Button("发布", GUILayout.Height(36))) Publish();
}

void Publish() {
    AppendLog("[发布] 开始...");
    try {
        // 1. 同步全部图集
        LoomAtlasSync.SyncAll(_settings);
        AppendLog("[发布] Atlas: OK");

        // 2. 打包全部 package（输出到 pkgOutputDir/ui/）
        for (int i = 0; i < _settings.packages.Count; i++)
            PackPackage(i);
        AppendLog("[发布] Pkg: OK");

        // 3. 发布字体
        PublishFonts();

        // 4. 导出 config.json
        LoomConfigExporter.Export(_settings);
        AppendLog("[发布] Config: OK");
    } catch (Exception ex) {
        AppendLog($"[发布] FAIL: {ex.Message}");
    }
    AssetDatabase.Refresh();
}

void PublishFonts() {
    string fontsDir = ToAbs(Path.Combine(_settings.pkgOutputDir, "fonts"));
    Directory.CreateDirectory(fontsDir);
    foreach (var entry in _settings.fonts) {
        if (entry.font == null) continue;
        string src = AssetDatabase.GetAssetPath(entry.font);
        if (string.IsNullOrEmpty(src)) continue;
        // 源文件名（去路径）+ 原扩展名 + .bytes
        string fileName = Path.GetFileName(src);              // NotoSansSC.ttc
        string dst = Path.Combine(fontsDir, fileName + ".bytes");  // NotoSansSC.ttc.bytes
        File.Copy(Path.GetFullPath(src), dst, overwrite: true);
    }
    AppendLog($"[发布] Fonts: {_settings.fonts.Count} 个 → {fontsDir}");
}
```

`PackPackage` 改为输出到 `{pkgOutputDir}/ui/{pkgName}.pkg.bin`。

### 3.6 连带改：showcase driver 读路径

`LoomShowcaseDriver.LoadPkgBytes` 现读 `StreamingAssets/showcase.pkg.bin`，改后读 `StreamingAssets/ui/showcase.pkg.bin`。

---

## 4. 其他已确认项

| # | 项 | 动作 |
|---|----|------|
| 1 | CI .exe artifact | 改 CI yaml，加 `loomgui_pkg.exe` artifact |
| 2 | fence.md 推断标记 | 清掉【实证】【推断·待测】，推断项转 TODO；roadmap 加"围栏契约补完" |
| 3 | skill 文档过时 | 立即修正 absolute 相关描述；roadmap 加"skill 文档全量对齐" |
| 5 | ShowcaseDriver 落位 | 移到 `loomgui_unity/Assets/LoomUI/Demo/` + `LoomGUI.Demo.asmdef`（含 `VirtualListDriver`） |
| 6 | _fontFile / _font | 废弃（字体管理重构后走 LoomSettings.fonts） |

---

## 5. LoomSettings 字体配置

### 5.1 FontEntry 新增字段

```csharp
[Serializable]
public class FontEntry {
    public string familyName;      // CSS font-family 值。拖入时默认=源文件名去扩展，可手改
    public Font font;              // Unity Font asset（拖入；Unity 光栅用）
    public string sourceFileName;  // 源文件名（如 "NotoSansSC.ttc"）。拖入时自动填，driver 拼 .bytes 路径用
    public bool isDefault;         // 默认回退字体
}

// LoomSettings 加:
public List<FontEntry> fonts = new();
```

### 5.2 字体 tab

```
现有 tab: 工作区 | 包管理 | 图集
新增 tab: 工作区 | 包管理 | 图集 | 字体
```

字体 tab 功能：
- 拖 Font asset → 自动填 `sourceFileName`（源文件名去路径，如 `NotoSansSC.ttc`）+ `familyName`（源文件名去扩展，如 `NotoSansSC`）+ 加入列表
- 第一个自动标 `isDefault`
- 列表条目可编辑 `familyName`、切换 `isDefault`、删除
- `sourceFileName` 自动填不暴露手改（拖 asset 时同步）
- 无"同步"按钮——字体发布逻辑在「发布」按钮里

---

## 6. 澄清决策记录（brainstorming 定稿）

| 议题 | 决策 | 依据 |
|---|---|---|
| 字体选择范围 | 完整多字体：FontTable + select(family) | font_family 已在 TextState 备好，差接到 measure |
| 字节加载路径 | 统一只走 .bytes（editor 也走） | 统一换简单，editor 迭代字体要重发可接受 |
| textureRebuilt 多 stage | per-stage 版本号 | 结构干净，不怕改动大 |
| LoomStage 拆分 | 严格拆分：纯 class 无 Unity 依赖 | Camera/transform 留 Driver |
| `_sceneBuilt` | 去除 | 坑 102 已修，scene=None 返空帧不 panic |
| pkg 输出路径 | 改 `ui/` + `fonts/` 子目录 | 目录结构清晰 |
| ShowcaseDriver 落位 | 移到 Demo/ + asmdef | demo 代码不进 UPM Runtime |
| tab 独立按钮 | 保留独立 + 加发布 | 独立=单步调试，发布=一键全流程 |
| bytes 文件名 | `{源文件名}.{ext}.bytes` | 与 familyName 解耦，改 family 不重复拷贝 |
| sourceFileName 字段 | FontEntry 加，面板拖 asset 自动填 | build 后 asset 无文件路径，须序列化 |
| v1.5 接口预留 | **挂起**——写 plan 时确认 v1.5 进度再定 | Controller/Animator/Gear 是否进 LoomStage 待定 |

---

## 7. 自审

- [x] 无 TBD/TODO 占位（v1.5 挂起项明确标记，非占位）
- [x] 图集 = Unity 原生 SpriteAtlas，不进 StreamingAssets
- [x] 字体 = `{源文件名}.{ext}.bytes` 放 `pkgOutputDir/fonts/`，由面板发布时拷贝
- [x] 「发布」按钮 = 图集同步 + 打包 + 字体拷贝 + config 导出，四步统一
- [x] LoomStage 纯 C#，不绑路径、不绑 MonoBehaviour
- [x] 多 Stage 场景下 textureRebuilt per-stage 版本号（独立解绑安全）
- [x] `_sceneBuilt` 去除（坑 102 已修，Rust scene=None 返空帧）
- [x] measure 切口收敛到两处调用点 + 两个签名（solve/build_render_nodes）
- [x] 打包器无 --fonts 参数（现状本就没有，spec 原文假设错误已修正）
- [ ] 待实现时确认：Animator/Controller/Gear 等 v1.5+ feature 是否进 LoomStage（挂起，写 plan 时定）
