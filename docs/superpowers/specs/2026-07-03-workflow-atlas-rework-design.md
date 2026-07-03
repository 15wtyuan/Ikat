# 工作流闭环 + 图集重做 设计

> v1.4a（包加载模型重构）之后的三件事闭环：(1) editor/ Node 脚本迁 Unity C#，设计师只要 Unity；(2) samples/ 删除（角色被 Unity 内工作区取代）；(3) 图集从「全手动 + 猜身份 + 缓存 miss」改成「配置驱动 + 显式路由 + 自动打包」。
>
> 死代码 / 旧注释 / 版本编号清理是独立 spec（`2026-07-03-deadcode-cleanup-design.md`），不在本 spec 范围。

---

## 1. 目标与范围

### 1.1 目标

- **工作流闭环**：设计师装 LoomGUI Unity 插件 → 弹设置面板 → 指定工作区 → 初始化 → open-design import 工作区 → AI 生成 HTML/CSS + 调 exe 验证打包 → PlayMode 渲染正确。**全程不装 Node。**
- **图集闭环**：加新图到 `res/` → 图集 tab 同步 → 重打 pkg.bin → 图正确显示（不白块）。path→Sprite 路由不靠文件名猜。

### 1.2 不做（推后）

- **图集懒加载 + refcount**（生命周期优化下一轮）——本轮图集仍启动全加载，只改路由/配置/自动打包/缓存逻辑。
- **死代码 / 旧注释 / 版本编号清理**——独立 Spec B。

### 1.3 成功标准

1. 设计师无需 Node，Unity 插件内完成工作区初始化 + 打包 + 图集配置。
2. AI 在 open-design 里读 config.json 调 loomgui_pkg.exe 验证+打包，围栏违规非零退出 AI 自纠（验证闭环不丢）。
3. 图集 path→Sprite 显式路由（顶层子目录 → 图集表），不遍历猜；miss 不永久缓存（修坑 104）。
4. 打包时自动同步 atlas packables，加图不用手动维护 `.spriteatlas`。
5. samples/ 和 editor/ 删除，两台机串行工作流不受影响（家里机依赖全在 `Assets/` git 跟踪）。

---

## 2. LoomGUI 设置面板（统一入口）

### 2.1 入口

Unity 插件装上后自动弹出（首次），或菜单 `LoomGUI > Settings`。一个 `EditorWindow`，顶部 toolbar 切 tab。

### 2.2 三个 tab

| Tab | 职责 |
|---|---|
| **工作区** | 工作区目录 + 初始化按钮 + resDirName + pkgOutputDir（原全局配置并入） |
| **包管理** | 包列表编辑 + 刷新 + 校验 + 一键打包（现 `LoomPackageManagerWindow` 整体并入） |
| **图集** | 图集条目编辑（拖文件夹当 packables）+ 同步 + 校验 |

### 2.3 配置存储

一份 `LoomPackageSettings` ScriptableObject（扩展现有，加图集字段 + 工作区字段）。`LoomPackageManagerWindow` 改名 `LoomSettingsWindow`，内部按 tab 分区。tab 间共享同一份 `_settings`——改工作区 tab 的 resDirName → 图集 tab 立刻反映。

### 2.4 loomgui_pkg.exe 供给

**随插件发布，固定路径，不暴露给用户。** exe 放 `Assets/LoomGUI/Editor/Tools/loomgui_pkg.exe`。本机改 loomgui_pkg 源码后手动 `cargo build --release -p loomgui_pkg` + 覆盖插件里那份。设置面板无 exe 路径 UI（删 `loomPkgExePath` 字段）。

---

## 3. 工作区与初始化（迁自 editor/init.mjs）

### 3.1 工作区目录结构

工作区 = `Assets/LoomUI/`（设计师指定，默认这个）。open-design import 此目录，AI 的 cwd = 工作区根。

```
Assets/LoomUI/
├── CLAUDE.md                         ← 初始化注入的围栏规则（标签段包裹，增量合并）
├── .claude/skills/loomgui-editor/    ← 初始化分发的 skill
│   ├── SKILL.md
│   ├── references/fence.md
│   ├── references/preview-polyfill.html
│   ├── references/preview-trust.md
│   └── config.json                   ← 从 LoomPackageSettings 导出，AI 读此调 exe
├── res/                              ← 全局唯一资源根（PNG 当 Sprite 导入）
│   ├── icons/
│   └── skin/
└── showcase/                         ← 包源目录（HTML/CSS）
    ├── home.html
    └── page_*.html
```

### 3.2 AssetPostprocessor（禁止 Unity 导入非资源文件）

`Assets/LoomUI/` 下的 `.html` / `.css` / `.claude/` / `CLAUDE.md` / `design-systems/` / `.od-skills/` 是给 AI/open-design 用的纯文本，不能被 Unity 当资产导入（否则 Unity 生成多余 .meta、尝试导入 .css）。

`AssetPostprocessor.OnPreprocessAsset`：若 assetPath 在工作区目录下且匹配上述后缀/目录 → 跳过导入（`context.ImportActivity = ImportActivity.None` 或设 `assetImporter.SaveAndReimport` 跳过）。PNG 正常导入为 Sprite（`TextureImporter.textureType = Sprite`），进 SpriteAtlas。

### 3.3 初始化按钮（C# 重写 init.mjs 三件事）

1. **注围栏规则** → `Assets/LoomUI/CLAUDE.md`。标签段 `<!-- loomgui-editor-begin -->...<!-- loomgui-editor-end -->` 包裹，增量合并（有则替换段、无则新建/追加），不覆盖设计师已有内容。围栏规则内容从插件 Editor Resources 读出（迁自 `editor/rules/claude/CLAUDE.md.tmpl`）。
2. **分发 skill** → `Assets/LoomUI/.claude/skills/loomgui-editor/`。拷 SKILL.md + references（迁自 `editor/skill/loomgui-editor/`）。**砍 pack.mjs**——skill 改为教 AI 读 config.json 调 loomgui_pkg.exe 验证+打包。
3. **写 config.json**（见 §3.4）。

### 3.4 config.json（LoomPackageSettings 导出快照）

路径：`Assets/LoomUI/.claude/skills/loomgui-editor/config.json`。**全相对工作区根**（可移植，换机器/换目录不炸）：

```json
{
  "exe_path": "../LoomGUI/Editor/Tools/loomgui_pkg.exe",
  "res_dir": "res",
  "output_dir": "../../StreamingAssets/",
  "packages": [
    {"name": "showcase", "source": "showcase", "html": ["home.html", "page_controls.html"]}
  ]
}
```

- `exe_path`：相对工作区根的 exe 路径（`Assets/LoomUI/` → `Assets/LoomGUI/Editor/Tools/`，相对 `../LoomGUI/...`）。LoomUI/LoomGUI 同在 Assets/ 下，相对关系稳定。
- `output_dir`：相对工作区根的 pkg.bin 输出目录（`Assets/LoomUI/` → `Assets/StreamingAssets/`，相对 `../../StreamingAssets/`）。
- `packages`：包列表（name + source 相对工作区根 + html 文件名列表）。

**自动同步**：面板改 LoomPackageSettings 任意字段（resDirName / outputDir / 包列表增删 / 图集配置）→ 自动重写 config.json（`EditorUtility.SetDirty` 后的持久化回调或显式 MarkDirty 钩子）。设计师改完面板，AI 立刻读到最新。

### 3.5 AI 工作流（open-design 里）

1. 读围栏规则（`CLAUDE.md`）+ skill（`SKILL.md` + references）。
2. 生成 HTML/CSS（守围栏）。
3. 读 `config.json` → 拼 `loomgui_pkg.exe` 命令行（sourceDir / pkgName / --html / --res / -o）→ 跑验证+打包。
4. 非零退出 = 围栏违规 → 读 stderr 自纠 → 重跑。
5. 零退出 = pkg.bin 产出到 output_dir。

### 3.6 Unity 面板工作流（开发者/设计师回 Unity）

同一个 exe、同一份 LoomPackageSettings 配置。打包 tab 按钮也调同一个 exe。AI 调 vs 按钮调，两条路都通。

---

## 4. 图集配置与显式路由

### 4.1 资源布局

**全局唯一 `Assets/LoomUI/res/`**（从包内 `showcase/res/` 提到 LoomUI 根）。所有包的 HTML 引用的图片都在此。path 归一化裁 `res/` 前缀，如 `icons/home.png`，**全局不撞**（res 全局唯一、子目录全局唯一）。

### 4.2 图集配置（图集 tab）

存 `LoomPackageSettings` 新字段 `atlasEntries: List<AtlasEntry>`：

```
atlasEntries:
  - atlasName: LoomShowcaseAtlas
    isDefault: true
    folders: [Assets/LoomUI/res/icons, Assets/LoomUI/res/skin]
  - atlasName: LoomItemAtlas
    folders: [Assets/LoomUI/res/items]
```

拖文件夹当 packables。一图集可拖多文件夹（都映射到该图集）。两图集拖同一子目录 = 配置错误，校验报红禁止。

### 4.3 path→Sprite 显式路由（SpriteResolver 重写）

运行时从 atlasEntries 构建 `Dictionary<string 子目录, SpriteAtlas>` 映射表。查询：

1. path = `icons/home.png`
2. 取 path **顶层子目录** `icons` → 查映射表 → 命中 LoomShowcaseAtlas
3. `atlas.GetSprite("home")`（文件名去扩展，此时安全——res 全局唯一、子目录全局唯一、atlas 内 sprite 名不撞）
4. **不再遍历所有 atlas 猜**，直接路由

**边界**：
- res 根下直接放图（path = `home.png`，无子目录）→ 走 `isDefault` 图集兜底。
- path 顶层子目录不在映射表 → 走 `isDefault` 图集。
- `isDefault` 图集未配置 → 走第一个 atlas。
- 查不到 sprite → fallback `missingSprite`（紫块不崩）。

### 4.4 自动打包（atlas packables 同步）

打 pkg.bin 时，面板读 atlasEntries，对每个图集：
1. 扫它 `folders` 下所有 PNG（递归）。
2. 与 atlas 当前 packables 比对。
3. 缺的 Sprite 加进 atlas packables（`AssetDatabase` 改 `.spriteatlas` 的 packables，存 Sprite 引用列表）。
4. 多余的（atlas 有但 folders 下没了）移除。
5. 触发 atlas 重新 pack。

**解决 B2 bug**：folder packable 在 Unity 6 静默打成空（`m_PackedSprites: []`）。改用**显式 Sprite 列表**而非文件夹引用，规避 Unity 6 folder packable 失效。

### 4.5 修坑 104（miss 不永久缓存）

现 `SpriteResolver.GetSprite` 第 92-93 行把 miss（null/MissingSprite）也缓存 → atlas 后来 pack 好也不重查。

修法：miss **不进缓存**。每次 miss 都重查（atlas 启动全加载，重查成本可控——atlas 数量小）。或缓存带「atlas 已加载」版本号，atlas 重建后清 miss 缓存。本轮选**miss 不缓存**（最简单，atlas 数量小重查无性能问题）。

### 4.6 删诊断 log

`SpriteResolver.cs:89` `[DBG-IMG]` log 删。

---

## 5. samples/ 与 editor/ 删除

### 5.1 samples/ 全删

`v1-showcase/` / `dyn-mail/` / `backpack/` / `ai-output/` 内容已迁 `Assets/LoomUI/` 或无用，删。`design-systems/loomgui/`（v-other 编辑器工作流的 open-design design-system picker 测试夹具）迁进 `Assets/LoomUI/design-systems/`（v-other 工作流仍需，夹具随工作区走，open-design import 工作区时能读到）。

### 5.2 editor/ 全删

Node 脚本（`init.mjs` / `pack.mjs` / `init.test.mjs`）逻辑迁 C#（§3）。围栏规则模板（`rules/*.tmpl`）+ skill 内容（`SKILL.md` + references）迁进 Unity 插件 Editor Resources（C# 读出注入工作区）。`rules/opencode/`、`rules/codex/` 砍（v-other 现只走 Unity，harness 多选推后）。

### 5.3 文档同步

- **CLAUDE.md**：§"在本仓库怎么干活"的 samples/editor 描述改（samples 删、editor 迁 Unity C#）。
- **README.md**：项目结构表（:36/:52）去 samples/editor 行；:48 "打包器 → pkg.bin + 图集"去掉"+ 图集"。
- **docs/roadmap/roadmap.md** §3（v-other editor）：段落改"Unity 内 C# 实现"，去掉 init.mjs/pack.mjs/三 harness 描述。
- **docs/design/fence.md** §5（围栏副本分发消费者表）：editor 行改"Unity 插件 Editor Resources 注入"。

### 5.4 两台机串行工作流

不受影响。家里机依赖全在 `Assets/`（`LoomUI/` HTML/CSS/res、`StreamingAssets/` pkg.bin/字体、`Plugins/LoomGUI/` .dll + exe），均 git 跟踪。删 samples/editor 零影响。

---

## 6. 数据流（图集链路，修订后）

```
HTML <img src="res/icons/home.png">
  │
  │ 打包器（loomgui_pkg.exe，AI 或面板调）
  │  ├─ 裁 res/ 前缀 → path="icons/home.png"
  │  ├─ PNG IHDR → w, h
  │  └─ pkg.bin: manifest AssetEntry{path:"icons/home.png", w, h}
  │              + 节点 image_path:"icons/home.png"
  │
  │ Rust Stage（运行期）
  │  ├─ image_sizes["icons/home.png"]=(w,h) → measure / 九宫格
  │  └─ FrameBlob: path_table + path_idx
  │
  │ Unity 后端
  │  ├─ SpriteResolver.GetSprite("icons/home.png")
  │  │    ├─ key = 顶层子目录 "icons" → 查 folder→atlas 表 → LoomShowcaseAtlas
  │  │    ├─ atlas.GetSprite("home")  [启动时据 atlasEntries 注册全图集]
  │  │    └─ hit: sp.texture + sp.rect 算 packed UV；miss: missingSprite（不缓存）
  │  └─ MirrorPool: 重映射 UV [0,1] → sprite.rect packed UV + 提交
  │
  ↓ 屏幕像素
```

---

## 7. 错误处理 / 测试

### 7.1 打包期校验（图集 tab）

- 每个图集 `folders` 下 PNG 与 atlas packables 比对，缺红字列出。
- 两图集拖同一子目录 → 报红禁止（映射歧义）。
- `isDefault` 未配置但 res 根有图 → 黄字提示（走第一个 atlas 兜底）。

### 7.2 运行时容错

- path 路由到图集但 `atlas.GetSprite` miss → fallback `missingSprite`（紫块不崩），**不缓存 miss**。
- path 顶层子目录不在映射表 → 走 `isDefault` 图集兜底。
- 图集未注册（atlasEntries 空）→ 所有图走 missingSprite，不崩。

### 7.3 测试

- **SpriteResolver 显式路由单测**：mock SpriteAtlas + atlasEntries，验 (a) path→atlas 顶层子目录路由正确；(b) res 根图走 isDefault；(c) miss 不永久缓存（首次 miss → 加载 sprite → 二次查命中）。
- **atlas packables 同步单测**：mock folders + PNG 集合 + 现有 atlas packables，验增量增删正确。
- **config.json 导出单测**：mock LoomPackageSettings，验导出的 config.json 字段全相对工作区根、内容正确。
- **fence_contract 不受影响**：纯 Unity 侧 + 打包器归一化（path 裁 res 前缀不变）改动，围栏契约无触碰。

---

## 8. 实现顺序（建议）

1. **图集配置 + 显式路由 + 修坑 104 + 删诊断 log**（§4）——改 SpriteResolver + LoomPackageSettings 加 atlasEntries + 图集 tab UI。先闭环图集痛点。
2. **res 提到 LoomUI 根**（§4.1）——迁移 `Assets/LoomUI/showcase/res/` → `Assets/LoomUI/res/`，改 showcase HTML 的 src 路径（`res/icons/...` 不变，因为归一化后都是 `icons/...`，但物理位置变了）。
3. **LoomSettingsWindow 三 tab 架构**（§2）——PackageManager 并入 + 工作区 tab + 图集 tab。
4. **工作区初始化 C#**（§3）——注围栏规则 + 分发 skill + 写 config.json + 自动同步 + AssetPostprocessor。
5. **exe 随插件发布**（§2.4）——loomgui_pkg.exe 放 `Assets/LoomGUI/Editor/Tools/`，删 loomPkgExePath 配置。
6. **samples/ editor/ 删 + 文档同步**（§5）。
7. **重打 pkg.bin + build .dll + 家里机验收**。
