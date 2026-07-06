# LoomStage 重构 + 字体管理 + 资源整理 + 发布按钮统一

> 2026-07-06，代码 review 讨论纪要 + brainstorming 澄清定稿。覆盖问题 #1 #2 #3 #5 #6 #7。
> 问题 #8 确认，#4 保留，#9 #10 待讨论。
> 决策依据见文末「§7 澄清决策记录」。

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
| 7 | LoomStage 不支持动态挂载、字体管理零散、资源僵化 | 本 spec 的核心内容 |
| 8 | `LoomSettings.asset` 是自动生成的吗 | 确认：`GetOrCreateDefault()` 自动创建 |

---

## 1. 资源模型总则（本次主轴）

### 1.1 痛点

LoomGUI 现状把 `.pkg.bin`、字体、图集硬塞进 StreamingAssets / Resources，绑死 Unity 原生资源管线。项目有自己的资源策略（AssetBundle / Addressables / 自定义），LoomGUI 的资源成了钉子——项目接管不了，要么重复打包，要么被 Resources 锁死。

根因：`LoomSettings.asset` 放 `Resources/LoomGUI/`，它的 `atlasEntries[].atlas`（SpriteAtlas 引用）和 `fonts[].font`（Font asset 引用）是**序列化字段**。Unity build pipeline 的固有行为——**Resources 里的资产，它引用的所有资产会被强制打进 build**。所以图集和 Font asset 被 LoomSettings.asset 间接拖进 Resources 包，项目即使想用 AB 管，图集/字体也被 Resources 锁死 + 重复打包。

### 1.2 核心原则

**LoomGUI 只产出资源文件，不绑死资源管线。**

1. **自建产物目录**：所有 LoomGUI 产出的资源落工程内自建目录 `Assets/LoomGUI/Bundles/`，不进 StreamingAssets。
2. **LoomSettings.asset 纯配置零资产引用**：只存字符串/列表（pkg 名、路径、family 名），不存任何 Unity 资产引用。资产引用不序列化进 .asset → Resources 包不拖资产。
3. **资产引用编辑器临时加载**：编辑器面板打开时按配置里的名字/路径临时 `AssetDatabase.LoadAssetAtPath` 加载资产引用供显示/拖拽，`[NonSerialized]` 不写进 .asset。
4. **框架提供默认加载可覆写**：Driver 提供三个 virtual 加载函数，默认直读 Bundles/（editor 可跑），项目继承覆写换 AB/Addressables。框架零资源管线假设。

### 1.3 Bundles/ 目录结构

```
Assets/LoomGUI/Bundles/
├── atlas/    ← .spriteatlasv2 + .meta（图集，Unity 原生资产）
├── ui/       ← .pkg.bin（LoomGUI 自有格式）
└── fonts/    ← {源文件名}.{ext}.bytes（Rust 测量）+ {源文件名}.{ext} + .meta（Font asset，Unity 光栅）
```

三类产物物理上在同一目录树。项目用自己的 AB 策略打整个 Bundles/，或按子目录分别打。

**`LoomSettings.pkgOutputDir` 默认改 `Assets/LoomGUI/Bundles/`**（不再 `Assets/StreamingAssets/`）。所有产物归一到 Bundles/。

### 1.4 字体两份（同份 ttf）

- **Rust 测量**：读 `{源文件名}.{ext}.bytes`（ttf 字节喂 ttf-parser）。
- **Unity 光栅**：读 Font asset（引擎动态字体 API 取 glyph UV）。
- 两份必须是同一份 ttf，否则 advance 不一致 → 字距错位（坑 119 根因）。
- 两份都放 `Bundles/fonts/`，文件名同源（`NotoSansSC.ttc.bytes` + `NotoSansSC.ttc`）。

### 1.5 图集（Unity 原生资产）

图集是 Unity 原生 SpriteAtlas 资产，**不进 .pkg.bin、不进 Resources**。`.spriteatlasv2` 产物落 `Bundles/atlas/`，由项目 AB 接管。运行时通过 Unity 资产系统加载（Driver 的 `LoadSpriteAtlas` virtual，见 §4.3）。PNG Sprite 源（packables）仍在工作区 `res/`，LoomAtlasSync 同步时扫——只是图集**产物位置**从 `workspaceDir/atlas/` 改到 `Bundles/atlas/`。

---

## 2. LoomSettings 纯配置化

### 2.1 去掉序列化资产引用

现状 `AtlasEntry.atlas`（SpriteAtlas 引用）、`FontEntry.font`（Font asset 引用）是序列化字段 → 拖资产进 Resources 包。改：

```csharp
[Serializable]
public sealed class AtlasEntry {
    public string atlasName = "";
    public bool isDefault;
    public List<string> folders = new();
    // 删除：public SpriteAtlas atlas;  ← 序列化引用，拖资产进 Resources
}

[Serializable]
public class FontEntry {
    public string familyName;      // CSS font-family 值。拖入时默认=源文件名去扩展，可手改
    public string sourceFileName;  // 源文件名（如 "NotoSansSC.ttc"）。driver 拼 .bytes 路径用
    public bool isDefault;         // 默认回退字体
    // 删除：public Font font;  ← 序列化引用，拖资产进 Resources
}
```

LoomSettings.asset 剩下纯字符串/列表配置，进 Resources 不拖任何资产。

### 2.2 编辑器面板临时加载资产引用

面板打开时按配置名字/路径临时加载资产引用供显示/拖拽，不写进 .asset：

- **图集**：按 `atlasName` 临时 `AssetDatabase.LoadAssetAtPath<SpriteAtlas>("Bundles/atlas/{atlasName}.spriteatlasv2")` 显示"已同步/未同步"状态。
- **字体**：拖 Font asset 进面板 → 当场抽取 `sourceFileName`（源文件名去路径）+ `familyName`（源文件名去扩展）存进配置，**丢掉 Font 引用**。下次打开面板按 `sourceFileName` 临时 `LoadAssetAtPath<Font>` 预览。

面板内存引用用 `[NonSerialized]` 或纯局部变量，确保不序列化。

---

## 3. LoomStage / LoomStageDriver 拆分（严格拆分）

### 3.1 动机

- 当前 `LoomStage` 是 `[ExecuteAlways]` 的 MonoBehaviour，`Awake()` 里绑死了 `_font` Inspector 字段、UI 相机、根 transform。
- 运行时 `AddComponent` 挂载时 `_font` 为 null → 初始化失败。
- 需要支持动态创建 Stage、多 Stage 实例。
- 单体 MonoBehaviour 把"Rust 句柄/字体表/渲染池"（引擎无关）和"相机/transform/FPS"（Unity 依赖）混在一起，边界不清。

### 3.2 新结构（严格拆分：Unity 依赖全部留 Driver）

```
LoomStage（纯 C# class，非 UnityEngine.Object）
├── 构造: LoomStage(Vector2 designSize = default)
│   └── designSize 默认 (1080,1920)。内部调 loomgui_stage_new(w,h)（不收字体路径）
├── RegisterFont(family, bytes, unityFont, isDefault)
│   └── bytes 喂 Rust（loomgui_stage_register_font）；unityFont 存 stage 内 _unityFonts 表（光栅用，见 §5.3）
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
│   2. _sprites.Init(LoomSettings.GetOrCreateDefault())  ← SpriteResolver 改用名字映射（见 §5.4）
│   3. RegisterFontsFromSettings()  ← 默认实现，调 LoadFont
│   4. EnsureCamera() + ConfigureTransforms()
│   5. Font.textureRebuilt += OnFontRebuilt（per-stage 版本号，见 §5.5）
│   6. 不再等 CreateRoot——tick 放行（Rust scene=None 返空帧不 panic，见 §3.4）
├── LateUpdate(): resize检测 + 输入采集 + _stage.Tick(dt, this)
├── OnGUI(): FPS 显示
├── OnDestroy(): _stage.Dispose() + 解绑静态事件
├── Stage 属性: public LoomStage Stage => _stage
├── RegisterFontsFromSettings(): protected virtual
│   └── 遍历 LoomSettings.fonts → 调 LoadFont(entry) 拿 (bytes, Font asset) → stage.RegisterFont
├── LoadFont(entry): protected virtual，返 (byte[] bytes, Font asset)
│   └── 默认直读 Bundles/fonts/{sourceFileName}.bytes + LoadAssetAtPath<Font>（editor）
├── LoadPackageBytes(name): protected virtual，返 byte[]
│   └── 默认直读 Bundles/ui/{name}.pkg.bin
├── LoadSpriteAtlas(atlasName): protected virtual，返 SpriteAtlas
│   └── 默认 editor LoadAssetAtPath（Bundles/atlas/{atlasName}.spriteatlasv2）；build 后返 null 提示项目覆写
└── 每 stage 一个独立相机（挂在 driver GO 的子节点）
```

### 3.3 调用方

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

### 3.4 `_sceneBuilt` 闸门去除

现状 `LateUpdate` 用 `_sceneBuilt` bool 闸门：`CreateRoot` 成功才放行 tick，防 EditMode scene=None 时 Rust panic。

**去除依据**：Rust 侧 `tick_and_render` 已修（坑 102，`stage.rs:520` `match self.scene.as_mut() { None => return FrameData::default() }`），scene=None 返空帧不 panic。C# 闸门冗余。

改后：Awake 后 `LateUpdate` 每帧 tick；scene 未建时 Rust 返空帧（零副作用，多一次 FFI 调用可忽略）。`CreateRoot` 不再翻闸门。

**实现注意**：`LateUpdate` 收到空 blob（`borrow_frame` 返 null/len=0）时要早返，不调 `_pool.Sync`——现状 `if (ptr != null && len > 0)` 已 guard，保留。

### 3.5 迁移：SampleScene

`LoomStage` 从 MonoBehaviour 变纯 class，SampleScene 里挂在 GO 上的 `LoomStage` component 引用失效。迁移：

- SampleScene 该 GO 改挂 `LoomStageDriver`（新 MonoBehaviour）。
- `LoomShowcaseDriver._stage` 字段类型从 `LoomStage`（旧 MonoBehaviour）改为 `LoomStageDriver`，取 `.Stage` 拿纯 class。
- _designSize/_uiCamera/_showFps/_safeArea 在 Driver 上重配。
- 字体不再 Inspector 单字段，改走 LoomSettings.fonts（§5）。

EditMode 测试不实例化 LoomStage（现有测试都是纯逻辑单测），无测试迁移。

---

## 4. Driver 资源加载（默认可覆写）

### 4.1 原则

框架提供默认加载（直读 Bundles/，editor 可跑），项目继承 Driver 覆写换 AB/Addressables。三个加载函数都是 `protected virtual`：

| 函数 | 返回 | 默认实现 | 项目覆写 |
|---|---|---|---|
| `LoadFont(entry)` | `(byte[] bytes, Font asset)` | 直读 `Bundles/fonts/{sourceFileName}.bytes` + editor `LoadAssetAtPath<Font>` | AB/Addressables 加载 bytes + Font asset |
| `LoadPackageBytes(name)` | `byte[]` | 直读 `Bundles/ui/{name}.pkg.bin` | AB/Addressables 加载 pkg.bin |
| `LoadSpriteAtlas(atlasName)` | `SpriteAtlas` | editor `LoadAssetAtPath`（`Bundles/atlas/{atlasName}.spriteatlasv2`）；build 后返 null + 报错提示覆写 | AB/Addressables 加载 SpriteAtlas |

### 4.2 加载编排

Driver.Awake 编排（顺序固定，业务 driver 不需要管）：
1. `new LoomStage(_designSize)`
2. `_sprites.Init(LoomSettings.GetOrCreateDefault())` — SpriteResolver 按名字建映射（§5.4），不持 atlas 引用
3. `RegisterFontsFromSettings()` — 遍历 `LoomSettings.fonts`，每条调 `LoadFont(entry)` 拿 (bytes, Font asset)，调 `stage.RegisterFont(family, bytes, font, isDefault)`
4. `EnsureCamera()` + `ConfigureTransforms()`
5. `Font.textureRebuilt += OnFontRebuilt`

业务 driver（ShowcaseDriver）在 Start 调 `stage.LoadPackage` + `stage.CreateRoot`（用 `LoadPackageBytes` 读 pkg）。

### 4.3 图集运行时加载

图集是 Unity 资产，build 后加载方式（Resources/AB/Addressables）与 pkg/字体裸文件不同。Driver 提供 `LoadSpriteAtlas(atlasName)` virtual：

- **editor 默认**：`AssetDatabase.LoadAssetAtPath<SpriteAtlas>("Bundles/atlas/{atlasName}.spriteatlasv2")`，editor 即改即跑。
- **build 后默认**：返 null + `Debug.LogError` 提示"项目须继承 LoomStageDriver 覆写 LoadSpriteAtlas 走 AB/Addressables"。框架不绑图集运行时加载策略。
- **项目覆写**：走 AB.LoadAsset<SpriteAtlas> / Addressables。

`SpriteResolver` 不再从 LoomSettings.atlasEntries 拿 atlas 引用（引用已删），改从 Driver 的 `LoadSpriteAtlas` 按 atlasName 加载，建 folder→atlasName 映射（见 §5.4）。

---

## 5. 字体管理（完整多字体选择）

### 5.1 现状与改动本质

现状：
- `Stage` 持 `Arc<Font>` 单字体；`solve`/`build_render_nodes` 把 `&self.font` 贯穿到 `measure_text(font: &Font)`。
- `font-family` CSS 已解析进 `ResolvedStyle.font_family` + 拷进 `TextState.font_family`，**但 measure/render 没读它**——单字体强贯穿。

改动本质：让 `ts.font_family` 真正生效——Stage 持字体表，measure/render 按节点 `font_family` 选字体，无匹配回退 default。

### 5.2 Rust 核心：FontTable

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

### 5.3 Rust 核心：measure 切口

`measure_text` 签名**不动**（仍收 `font: &Font`）——调用方先 `font_table.select(family)` 选好再传。切口收敛到两处调用点：

1. **layout/mod.rs measure 闭包**（`MeasureContext::Text` 分支）：
   - `MeasureContext::Text` 加 `family: Option<String>` 字段（build 时从 `s.font_family.clone()` 填）。
   - 闭包内 `let font = fonts.select(ctx.family.as_deref()); measure_text(..., font);`

2. **render/mod.rs `NodeKind::Text` 分支**（fallback measure，`text_layouts` miss 时）：
   - 从 `n.style.font_family` 取 family → `fonts.select(...)` → `measure_text(..., font)`。

3. **签名**：`solve(scene, &Font, ...)` → `solve(scene, &FontTable, ...)`；`build_render_nodes(scene, &Font, ...)` → `build_render_nodes(scene, &FontTable, ...)`。Stage.tick_and_render 内部传 `&self.fonts`。

### 5.4 Unity 光栅侧 + SpriteResolver 改造

**光栅侧字体表**：`LoomStage` 持 `Dictionary<string, Font> _unityFonts`（family → Font asset）+ `defaultUnityFont`。`RegisterFont(family, bytes, unityFont, isDefault)`：bytes 喂 Rust，unityFont 存 `_unityFonts`。`MirrorPool.Sync` 光栅 text 节点时按节点 `font_family` 从 `_unityFonts` 选 Font asset（无匹配回退 default）。与 Rust 测量侧 select 对称。

**SpriteResolver 改造**：现状从 `LoomSettings.atlasEntries[].atlas`（SpriteAtlas 引用）建 folder→atlas 映射。引用删除后，改为：
- `Init(LoomSettings)` 建 folder→atlasName 映射（纯字符串，不持引用）。
- 运行时 `GetSprite(path)` 按 folder 查 atlasName → 调 `driver.LoadSpriteAtlas(atlasName)` 拿 SpriteAtlas → `atlas.GetSprite(spriteName)`。
- SpriteResolver 持 Driver 引用（或 Driver 注入一个 `Func<string, SpriteAtlas>` 加载委托），不直接持 atlas 引用。
- atlas 加载缓存（atlasName → SpriteAtlas）避免每帧重载。

### 5.5 textureRebuilt：per-stage 版本号

现状 `TextRasterizer.OnRebuilt` 是全局 `static s_fontVersion++`，多 Stage 时一个 stage 字体 atlas rebuild 会脏所有 stage 的 text。

**改 per-stage**：`TextRasterizer` 不再持全局版本号，改为 `LoomStage`（或 Driver）持 `int _fontVersion` + 实例方法 `OnFontRebuilt`。`Font.textureRebuilt` 静态事件绑当前 stage 的实例方法（`Font.textureRebuilt += _stage.OnFontRebuilt`），`OnDestroy` 解绑。MirrorPool.Sync 比对该 stage 自己的版本号。

结构改动：`TextRasterizer.FontVersion`（静态）→ `LoomStage.FontVersion`（实例）。`MirrorPool.Sync` 读 stage 的版本号而非静态。

---

## 6. 「发布」按钮

### 6.1 现状

当前面板散落多个独立按钮：
- 工作区 tab：初始化工作区
- 包管理 tab：单个"打包" + "一键打包全部"
- 图集 tab："同步此图集" + "同步全部图集 packables"

分散、顺序不明确，容易漏步骤。

### 6.2 改后：保留独立按钮 + 底部统一「发布」

**决策**：各 tab 原有独立按钮**保留**（单步调试用），底部加统一「发布」按钮（一键全流程）。独立=迭代单包/单图集，发布=全量。

```
┌─ 发布（底部统一按钮）────────────────────────────────┐
│                                                     │
│  1. 同步全部图集 packables（LoomAtlasSync.SyncAll）   │
│     └─ .spriteatlasv2 → Bundles/atlas/               │
│     └─ 扫 folders(res/) → Sprite → atlas packables   │
│                                                     │
│  2. 打包全部 package（loomgui_pkg.exe × N）           │
│     └─ .pkg.bin → Bundles/ui/{name}.pkg.bin          │
│                                                     │
│  3. 发布字体                                         │
│     └─ 拷 Font asset 源文件 → Bundles/fonts/          │
│     └─ {源文件名}.{ext}（Font asset）+ .bytes（Rust）  │
│                                                     │
│  4. 导出 config.json（现有逻辑，不变）                  │
│                                                     │
│  [ 发布 ]                                           │
└─────────────────────────────────────────────────────┘
```

### 6.3 发布字体（两份）

```csharp
void PublishFonts() {
    string fontsDir = ToAbs(Path.Combine(_settings.pkgOutputDir, "fonts"));
    Directory.CreateDirectory(fontsDir);
    foreach (var entry in _settings.fonts) {
        // 临时按 sourceFileName 加载 Font asset 拿源文件路径（不持引用）
        string fontAssetPath = FindFontAssetPath(entry.sourceFileName);
        if (string.IsNullOrEmpty(fontAssetPath)) { AppendLog($"[发布] 字体 {entry.sourceFileName} 找不到源 asset，跳过"); continue; }
        string absSrc = Path.GetFullPath(fontAssetPath);
        string fileName = entry.sourceFileName;  // 如 NotoSansSC.ttc
        // 1. Font asset 源文件（Unity 光栅用，AB 打包）
        File.Copy(absSrc, Path.Combine(fontsDir, fileName), overwrite: true);
        // 2. .bytes（Rust 测量用）
        File.Copy(absSrc, Path.Combine(fontsDir, fileName + ".bytes"), overwrite: true);
    }
    AppendLog($"[发布] Fonts: {_settings.fonts.Count} 个 → {fontsDir}");
}

// 按 sourceFileName 找 Font asset 源路径（编辑器面板临时加载，不序列化）
string FindFontAssetPath(string sourceFileName) {
    var guids = AssetDatabase.FindAssets(sourceFileName + " t:Font");
    foreach (var g in guids) {
        var p = AssetDatabase.GUIDToAssetPath(g);
        if (Path.GetFileName(p) == sourceFileName) return p;
    }
    return null;
}
```

### 6.4 打包器 CLI（无 --fonts 参数）

```bash
loomgui_pkg.exe <sourceDir> <pkgName> \
  --html <list> \
  --res-root <工作区根/res> \
  -o <pkgOutputDir>/ui/<pkgName>.pkg.bin
```

打包器**没有** `--fonts` 参数（现状本就没有，字体由面板直接拷贝，不走 exe）。

### 6.5 LoomAtlasSync 输出路径改

`LoomAtlasSync` 把 `.spriteatlasv2` 产物从 `workspaceDir/atlas/` 改到 `pkgOutputDir/atlas/`（即 `Bundles/atlas/`）。PNG Sprite 源（packables）仍在工作区 `res/`，同步时扫——只改产物位置。

### 6.6 连带改：showcase driver 读路径

`LoomShowcaseDriver.LoadPkgBytes` 现读 `StreamingAssets/showcase.pkg.bin`，改后走 Driver 的 `LoadPackageBytes("showcase")` 默认读 `Bundles/ui/showcase.pkg.bin`。

### 6.7 面板实现

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
        // 1. 同步全部图集 → Bundles/atlas/
        LoomAtlasSync.SyncAll(_settings);
        AppendLog("[发布] Atlas: OK");

        // 2. 打包全部 package → Bundles/ui/
        for (int i = 0; i < _settings.packages.Count; i++) PackPackage(i);
        AppendLog("[发布] Pkg: OK");

        // 3. 发布字体 → Bundles/fonts/
        PublishFonts();

        // 4. 导出 config.json
        LoomConfigExporter.Export(_settings);
        AppendLog("[发布] Config: OK");
    } catch (Exception ex) {
        AppendLog($"[发布] FAIL: {ex.Message}");
    }
    AssetDatabase.Refresh();
}
```

`PackPackage` 改为输出到 `{pkgOutputDir}/ui/{pkgName}.pkg.bin`。

---

## 7. LoomSettings 字体 tab

```
现有 tab: 工作区 | 包管理 | 图集
新增 tab: 工作区 | 包管理 | 图集 | 字体
```

字体 tab 功能：
- 拖 Font asset → 当场抽 `sourceFileName`（源文件名去路径，如 `NotoSansSC.ttc`）+ `familyName`（源文件名去扩展，如 `NotoSansSC`）存进配置，丢掉 Font 引用。
- 第一个自动标 `isDefault`。
- 列表条目可编辑 `familyName`、切换 `isDefault`、删除。
- `sourceFileName` 自动填不暴露手改（拖 asset 时同步）。
- 无"同步"按钮——字体发布逻辑在「发布」按钮里。
- 面板显示时按 `sourceFileName` 临时 `LoadAssetAtPath<Font>` 预览（不序列化）。

---

## 8. 其他已确认项

| # | 项 | 动作 |
|---|----|------|
| 1 | CI .exe artifact | 改 CI yaml，加 `loomgui_pkg.exe` artifact |
| 2 | fence.md 推断标记 | 清掉【实证】【推断·待测】，推断项转 TODO；roadmap 加"围栏契约补完" |
| 3 | skill 文档过时 | 立即修正 absolute 相关描述；roadmap 加"skill 文档全量对齐" |
| 5 | ShowcaseDriver 落位 | 移到 `loomgui_unity/Assets/LoomUI/Demo/` + `LoomGUI.Demo.asmdef`（含 `VirtualListDriver`） |
| 6 | _fontFile / _font | 废弃（字体管理重构后走 LoomSettings.fonts） |

---

## 9. 澄清决策记录（brainstorming 定稿）

| 议题 | 决策 | 依据 |
|---|---|---|
| **资源模型主轴** | **LoomGUI 只产出资源，不绑资源管线** | 项目有自己的 AB/Addressables 策略，框架不能僵化 |
| **发布目录** | 工程内自建 `Assets/LoomGUI/Bundles/`，不进 StreamingAssets | 产物是 Unity 资产可被 AB 打，项目接管 |
| **LoomSettings 资产引用** | 纯配置零引用，资产引用编辑器临时加载不序列化 | Resources 引用会拖资产进包 |
| **Bundles 结构** | atlas/ + ui/ + fonts/ 三子目录 | 三类产物分明，项目可分层打 AB |
| **图集进 Bundles** | .spriteatlasv2 也放 Bundles/atlas/ | 与 pkg/字体一起被项目接管 |
| **字体两份** | .bytes（Rust 测量）+ Font asset（光栅）都放 Bundles/fonts/ | 同份 ttf，两份分工 |
| **框架加载职责** | Driver 提供默认（直读 Bundles/）可覆写 | 零配置能跑 + 资源策略可换 |
| **Driver 职责** | 生命周期 + 资源加载编排 + 三 virtual 加载函数 | 流畅 + 可覆写 |
| **图集运行时加载** | editor 直读 / build 覆写 | 不绑图集加载策略 |
| 字体选择范围 | 完整多字体：FontTable + select(family) | font_family 已在 TextState 备好，差接到 measure |
| textureRebuilt 多 stage | per-stage 版本号 | 结构干净，不怕改动大 |
| LoomStage 拆分 | 严格拆分：纯 class 无 Unity 依赖 | Camera/transform 留 Driver |
| `_sceneBuilt` | 去除 | 坑 102 已修，scene=None 返空帧不 panic |
| ShowcaseDriver 落位 | 移到 Demo/ + asmdef | demo 代码不进 UPM Runtime |
| tab 独立按钮 | 保留独立 + 加发布 | 独立=单步调试，发布=一键全流程 |
| bytes 文件名 | `{源文件名}.{ext}.bytes` | 与 familyName 解耦 |
| sourceFileName 字段 | FontEntry 加，面板拖 asset 自动填 | 不存 Font 引用，靠文件名找 |
| LoomAtlasSync 产物 | 改 Bundles/atlas/，PNG 源留 res/ | 只改产物位置 |
| v1.5 接口预留 | **挂起**——写 plan 时确认 v1.5 进度再定 | Controller/Animator/Gear 是否进 LoomStage 待定 |

---

## 10. 自审

- [x] 无 TBD/TODO 占位（v1.5 挂起项明确标记，非占位）
- [x] 资源模型主轴：Bundles/ 自建目录，零资产引用，Driver 默认可覆写
- [x] LoomSettings 纯配置零引用 → Resources 包不拖资产
- [x] 三类产物（atlas/ui/fonts）归一 Bundles/，项目 AB 接管
- [x] 字体两份（.bytes + Font asset）同份 ttf，都进 Bundles/fonts/
- [x] Driver 三 virtual 加载函数（LoadFont/LoadPackageBytes/LoadSpriteAtlas）默认直读可覆写
- [x] LoomStage 纯 C#，不绑路径、不绑 MonoBehaviour
- [x] 多 Stage 场景下 textureRebuilt per-stage 版本号
- [x] `_sceneBuilt` 去除（坑 102 已修，Rust scene=None 返空帧）
- [x] measure 切口收敛到两处调用点 + 两个签名（solve/build_render_nodes）
- [x] 打包器无 --fonts 参数（现状本就没有）
- [x] SpriteResolver 改名字映射 + driver.LoadSpriteAtlas，不持 atlas 引用
- [ ] 待实现时确认：Animator/Controller/Gear 等 v1.5+ feature 是否进 LoomStage（挂起，写 plan 时定）
