# 独立打包器 + 自绘图集 + 工作区重构（设计）

> 代号 v1.8+（roadmap §3「v other — 编辑器工作流」，独立并行，不阻塞主线 v1.x）
> 日期 2026-07-10

## 1. 背景与目标

当前打包链的问题：
- **不符合 AI 工作流**：Unity 编辑器面板拼参数调 `loomgui_pkg.exe`，AI 加一个界面要把所有其他 html 一起带上，容易漏、耗 token。
- **绑死 Unity**：设置界面（`LoomSettingsWindow`）+ 图集（Unity Sprite Atlas）都在 Unity 编辑器里，工作区放在 `Assets/LoomUI/` 内。以后做 Godot/UE 后端时这套复用不了。
- **图集属于引擎**：运行时靠 Unity `SpriteAtlas.GetSprite` 拿纹理+UV，其他后端拿不到同一套图集。

**目标**：把「设计期」和「运行期」彻底分开，中间只靠**产物目录**这一个契约面通信。

- 打包器独立、跨平台、不依赖任何游戏引擎编辑器。
- 图集自绘（Rust 打包，产 PNG + JSON），一套产物任何后端可用。
- 工作区是磁盘上任意目录，自包含配置（AI 直接编辑）。
- 一条零参命令即可打包（AI 加完界面、改完配置，`build <workspace>` 完事）。
- 提供 Tauri GUI 壳，Unity「Open Packer」按钮拉起它。

## 2. 已定决策（真相源，实现期不得漂移）

| # | 决策 |
|---|------|
| 1 | 运行时一起改（打包 + 消费闭环，不留半套） |
| 2 | GUI = Tauri 2.x（Rust 后端 link `loomgui_pkg` + 原生 HTML/CSS/JS 前端），跨平台 |
| 3 | 工作区 = 磁盘目录 + 根下 `loom.workspace.json`；用户目录 `~/.loomgui/recent.json` 只存最近打开列表 |
| 4 | 图集独立于包；产 `<name>.png` + `<name>.atlas.json`（sprite_key→uv+orig） |
| 5 | 字体进工作区配置；打包器产 `loom.runtime.json` 给后端自举；废弃 `LoomSettings` ScriptableObject |
| 6 | 零参 `loom-pkg build <workspace>` CLI；GUI 与 CLI 共享同一 `build()` |
| 7 | 工作区完全出 Unity；Unity 退化成「加载产物 + 拉起 GUI 按钮」 |
| 8 | 全路径相对工作区根 + 正斜杠；img src 相对 html 文件；sprite_key = 图相对根路径（全局唯一） |
| 9 | 打包引擎保持 Rust，继续复用 `loomgui_core`（parse/cascade/build_scene/围栏），零漂移 |

## 3. 整体架构与数据流

```
┌─ 设计期（跨平台，独立于任何游戏引擎）──────────────────────┐
│  工作区目录/                                                  │
│    loom.workspace.json   ← 唯一真相源（AI 直接改）            │
│    ui/*.html *.css        ← 组件源（img src 相对 html 文件）  │
│    assets/**/*.png         ← 图集源图（原图，不进 Unity import）│
│    fonts/*.ttf|otf|ttc      ← 字体源                            │
│                                                                │
│         ┌────────────────────────────────┐                   │
│         │  loomgui_pkg (Rust lib)         │  复用 core:        │
│         │  ─ pack : parse→cascade→scene   │  parse/cascade/    │
│         │  ─ atlas: 读PNG→shelf打包→UV    │  build_scene/围栏  │
│         │  ─ fonts: 拷 .bytes             │                    │
│         │  ─ emit : runtime.json          │                    │
│         └────────────────────────────────┘                   │
│              ↑ 同一个 build() 函数 ↑                          │
│      ┌───────┴────────┐        ┌───────┴────────┐             │
│      │ loom-pkg CLI    │        │ loomgui_gui    │             │
│      │ build <ws>(零参)│        │ (Tauri 壳)     │             │
│      └────────────────┘        └────────────────┘             │
└──────────────────────┬─────────────────────────────────────────┘
                       │ 产出到 output_dir（契约面）
                       ▼
        output_dir/（拷进游戏工程可加载位置）
          ui/*.pkg.bin                ← 组件二进制
          atlas/*.png + *.atlas.json  ← 自绘图集
          fonts/*.bytes               ← 字体字节
          loom.runtime.json           ← 后端自举清单
                       │
                       ▼
┌─ 运行期（每后端一份，只读产物目录）──────────────────────┐
│  Unity/Godot/UE 后端：                                        │
│   读 loom.runtime.json → 知道包/图集/字体链                   │
│   加载 atlas.png 为普通纹理 + 读 atlas.json 拿 UV             │
│   （Unity 不再用 SpriteAtlas / LoomSettings SO）             │
└────────────────────────────────────────────────────────────────┘
```

**三个不变量**：
1. **产物目录是唯一契约面**。设计期工具产出它，运行期后端消费它。后端永远不认识工作区、GUI、CLI。
2. **围栏 / cascade / 场景构建只有一份实现**（Rust core）。GUI 和 CLI 都走同一个 `build()`，零漂移。
3. **图集与包解耦**。包是逻辑组件库（`.pkg.bin`），图集是资源（png+json），分别产出、分别加载，一套图集任何后端可用。

## 4. 工作区格式（`loom.workspace.json`）

AI 直接编辑的真相源。原则：扁平、字段自解释、路径全相对工作区根 + 正斜杠。

```jsonc
{
  "version": 1,
  "output_dir": "../dist",              // 相对工作区根；打包产物落这

  "packages": [
    {
      "name": "showcase",
      "dirs": ["ui/showcase"],          // 相对根，收 html；支持多目录
      "html": []                        // 见「html 三态」
    }
  ],

  "atlases": [
    { "name": "ui",   "default": true, "dirs": ["assets/icons", "assets/buttons"], "max_size": 2048, "padding": 4 },
    { "name": "char", "dirs": ["ui/showcase/images"] }
  ],

  "fonts": [
    { "family": "NotoSansSC", "file": "fonts/NotoSansSC.ttc", "default": true, "fallback": true }
  ]
}
```

**字段名对齐**：收目录的场景统一用 `dirs`（数组）——package 收 html、atlas 收 png 都用它。字体是单文件不是目录，用 `file`，各自语义准确，不为对齐而对齐。

**img src 相对 html 文件**（浏览器原生行为，最符合 AI 可预测性）：
- `ui/showcase/main.html` 里写 `<img src="home.png">` → 指 `ui/showcase/home.png`（图跟 html 放一起，Chromium 直接可预览）。
- 也能写 `<img src="images/x.png">` → `ui/showcase/images/x.png`。
- 打包器把 img src 相对 html 解析成**图相对工作区根的路径**，这就是全局唯一的 `sprite_key`。
- 这自动满足「设计师图片跟 html 放一起」，无需 per-package 图片源配置。

**sprite_key = 图相对工作区根路径**（如 `assets/icons/home.png`）：天然全局唯一，消除不同目录同名文件撞车隐患（旧模型用裸文件名 `GetSprite("home")` 会撞）。

**atlas.dirs 递归**扫子目录 png（对齐现有 Unity `EnumeratePngs(AllDirectories)`）。

**html 三态**（字段本身承载状态，无需额外布尔）：
- `html` 缺失或空数组 `[]` = **自动态**：打包时扫 `dirs` 顶层所有 `.html`；GUI 里展示扫出来的全部（自动态）。
- GUI 里手动增删任一条 → 翻转为**显式态**：把当前列表固化写进 `html`，之后不再自动扫。
- 显式态想回自动：GUI「恢复自动扫」按钮清空回 `[]`。

**用户目录**（跨平台用 `dirs` crate 定位）：
```jsonc
// ~/.loomgui/recent.json
{ "recent": ["F:/proj/ui-ws", "D:/game2/ws"] }
```

## 5. 图集打包（Rust 自绘）

pkg crate 新模块。产 `<name>.png`（多页时 `<name>.png`/`<name>.1.png`…）+ `<name>.atlas.json`。

**依赖**：`image` crate 加回 **pkg crate**（PNG 解码 + 编码），**不进 core runtime**。`etagere 0.3`（core 已有，字体图集在用）复用作 shelf 分配。

**流程**：
1. **收集**：遍历每个 atlas 的 `dirs`，递归扫 `.png`，得 `sprite_key`（相对根）列表。同时收 html 里 img src 引用的图，交叉验证。
2. **解码**：`image` crate 解码每张 PNG 到 RGBA8。
3. **打包**：`etagere` shelf 分配。超 `max_size` 开多页。`padding` 防 bleed。**第一版禁旋转/trim**（对齐现有 `EnsureAxisAlignedPacking` 轴对齐约束）；旋转/trim 留后续优化。
4. **blit**：子图像素拷到 atlas 大图槽位。
5. **编码**：`image` crate 写 `atlas.png`。
6. **产清单**：`atlas.json`。

**`atlas.json` 格式**：
```jsonc
{
  "pages": ["ui.png", "ui.1.png"],
  "sprites": {
    "assets/icons/home.png": {
      "page": 0,
      "uv":   [0.012, 0.048, 0.137, 0.170],  // [u0,v0,u1,v1] 归一化，直接喂后端
      "orig": [64, 64],                        // 原图像素（measure + 九宫格基准）
      "px":   [4, 4, 64, 64]                   // 可选，调试用；可从 uv×page 尺寸反推
    }
  }
}
```

**九宫格不进 atlas.json**：九宫格边（CSS `border-image-slice`）是 per-element 属性，烤进 `Node.base_style`（pkg.bin）。同一张图在不同元素上可有不同 slice。运行时用 CSS slice + `orig` + `uv` 算九宫格（后端已有这套）。

**尺寸真相源 = atlas.json（全图），彻底脱离包**：
- 已核实：图尺寸是**运行期** `solve` 时消费（`ImageSizeTable = HashMap<String,(u32,u32)>`，Image measure 三档 CSS > 真实像素 > 64×64），**打包期 `build_scene` 不需要尺寸**。
- **为什么不能用 pkg.bin 当尺寸源**：运行时动态图标（道具 icon 等）不写在任何 html 里，是运行时 `set_src` 进去的。pkg.bin 的 manifest 只收 html 静态引用的图，永远缺这些动态图标的尺寸 → 用 pkg.bin 当源则动态图标全 fallback 64×64 渲染错。atlas.json 含图集里**全部**图（含未被 html 引用的），是唯一完整来源。
- **pkg.bin 的 `asset_manifest` 段整个删除**（格式版本 v12 → v13 + 迁移器；同步删 `PkgManifestReader`、`read_png_size`、`PackageInput.asset_manifest`、`Package.asset_manifest`、`Stage.load_package` 里的自建尺寸逻辑）。项目早期，做干净不留冗余段。
- **core 新增批量入口** `Stage::set_image_sizes(&[(String, u32, u32)])` + FFI `loomgui_stage_set_image_sizes(...)`（批量单次，非逐条——上万张图逐条跨 FFI 太慢）。
- **规模无风险**：尺寸表是元数据（每条 ~48B，1 万张 ~500KB HashMap，启动一次性全灌，O(1) 查）。真正的内存压力在 atlas.png 纹理页，那是**按页懒加载**（现状 `_atlasCache` 已如此），用到哪页加载哪页，与本设计无关。

**交叉验证只做单向**（关键——别反向）：
- **验**：html 静态引用的图必须能在某 atlas 找到；找不到且无 `default` → 报错（列出无归属 key）。
- **不验**：atlas 里的图不要求都被 html 引用（道具图标就是运行时才用，反向要求会把它们全判为无用报错）。
- 一张图被多个 atlas 覆盖 → 报错（列出冲突）。
- 「超大图单独一张不适合拼合」：atlas 支持 `standalone` 模式（每图独立成页、不拼合，`uv=[0,1]`），照样有 atlas.json 条目。

## 6. 运行时消费改造（Unity 后端）

**核心**：替换 `SpriteResolver` 的数据来源（吃 Unity `SpriteAtlas` → 吃 atlas.png + atlas.json），`MirrorPool` 的 UV 重映射简化。

**6.1 `loom.runtime.json`（打包器产，替代 `LoomSettings` SO）**
```jsonc
{
  "version": 1,
  "packages": ["showcase", "hud"],          // .pkg.bin 文件名（不含扩展）
  "atlases": ["ui", "char"],                 // 每个对应 <name>.atlas.json + png
  "fonts": [
    { "family": "NotoSansSC", "file": "NotoSansSC.ttc.bytes", "default": true, "fallback": true }
  ]
}
```
后端启动读这一份，就知道加载哪些包/图集/字体链。**图集路由消失**（打包时已把每图归好），后端把所有 atlas.json 的 sprites 合并成一张全局 `sprite_key → (atlasName, page, uv, orig)` 表。

**6.2 `SpriteResolver` 重写**
- 现在：`GetSprite(name)` → Unity `SpriteAtlas.GetSprite` → `sp.texture` + `sp.uv`。
- 改后：`Init` 时读所有 atlas.json 建全局表 + 懒加载 atlas png 为 `Texture2D`（每 page 一张）。`GetSprite(key)` → 查表得 `(page Texture2D, uv_rect, orig)`，返回轻量结构（不再是 Unity `Sprite`）。
- **UV 已是打包算好的最终值**，线性映射 core 产的 `[0,1]` UV 到 `uv_rect`。`MirrorPool.RemapMeshUvToSprite` 里「取 sp.uv 包围盒」的 workaround（绕 Unity Sprite.rect 语义坑）**删除**。

**6.3 atlas.png 当普通 Texture2D**：完全脱离 Unity Sprite/SpriteAtlas 系统。Godot/UE 后端同样是「加载纹理 + 查 UV 表」，一套逻辑跨后端。

**6.4 `image_sizes` 表来源**：后端启动时读所有 atlas.json → 合并成一个扁平数组 → **一次性** `loomgui_stage_set_image_sizes` 灌进 core（在首帧 solve 前，启动加载阶段天然满足）。不再从 pkg.bin（manifest 段已删）。

**6.5 加载路径**：atlas.png / .pkg.bin / .bytes / runtime.json 都在产物目录。沿用现有 driver 钩子模式，把 `LoadSpriteAtlas` 换成 `LoadTexture` + `LoadTextFile`（Resources / StreamingAssets / AB 由 driver 决定）。

**6.6 废弃**：`LoomSettings` SO、`LoomAtlasSync`（Unity 图集同步整个删）、`LoomConfigExporter`、`LoomWorkspaceInitializer`、`PkgManifestReader`（整个删——manifest 段已从 pkg.bin 移除）。

## 7. Tauri GUI 壳

**架构**：Rust 后端（link `loomgui_pkg` lib）+ 原生 HTML/CSS/JS 前端，打成单个跨平台原生 app。前端调 Tauri command，command 直接调 `loomgui_pkg` 函数——不经 shell、不拼参数、不序列化中间态。GUI 和 CLI 走同一份 `build()`。

**Tauri commands（前端↔Rust 全部接口）**：
```
recent_workspaces() -> [path]              // 读 ~/.loomgui/recent.json
open_workspace(path) -> WorkspaceConfig    // 读 loom.workspace.json + 校验
create_workspace(path) -> WorkspaceConfig  // 新建目录骨架 + 默认配置 + 注入 CLAUDE.md/skill
save_workspace(path, config)               // 写回 loom.workspace.json
scan_html(pkg_dir) -> [html]               // 自动扫，GUI 展示用
build(path) -> BuildReport                 // 全量打包（= CLI 同一函数），返回日志/错误/产物清单
build_one(path, pkg_name) -> BuildReport   // 单包快迭代（可选）
```

**前端（web，UX 对齐旧 settings 面板但更好用）**：
- **启动**：最近工作区列表（卡片）+ 新建/打开按钮（常见编辑器那套）。
- **四区**（对齐旧四 tab）：工作区总览 / 包 / 图集 / 字体。
- **拖拽**：Tauri 原生 `onDragDrop` 给真实文件系统路径（不像浏览器只给上传流）——拖目录建包、拖目录进 atlas.dirs、拖字体文件，都拿绝对路径，归一化成相对工作区根存配置。
- **改配置即存**：任一字段改动 → `save_workspace` 写回 json（对齐旧面板「改字段自动同步」）。
- **打包按钮**：调 `build`，日志区显示结果（对齐旧「发布」+ 日志区）。

**Unity 侧「Open Packer」**：一个极小 `[MenuItem("LoomGUI/Open Packer")]`，按平台定位可执行文件（`loom-gui.exe` / `.app` / bin）并 `Process.Start`。Unity 侧唯一保留的编辑器脚本。

**crate 结构**：
```
loomgui_pkg/           (lib, 已有, 复用 core)
  ├─ pack       (现有, 微调)
  ├─ atlas      (新, image + etagere)
  ├─ workspace  (新, workspace.json 读写 + 校验)
  ├─ build      (新, 编排: pack + atlas + fonts + runtime.json)
  └─ bin/loom-pkg  (CLI, 零参 build)
loomgui_gui/           (新 crate, Tauri app, 依赖 loomgui_pkg)
```

## 8. 错误处理

打包器面向 AI，报错要可诉诸行动：
- 围栏违规：沿用现有（parse 失败/打包器拒收），报到哪个 html 哪行。
- img 引用的图不在任何 atlas 且无 default → 报错列出哪些 key 无归属。
- 一张图被多个 atlas 覆盖 → 报错列出冲突。
- 单图比页（`max_size`）还大放不下 → 报错。
- 字体 `file` 找不到 → 报错。
- **CLI 退出码**：成功 0，配置/校验错非 0，AI 据此判断。

## 9. 测试

- `loomgui_pkg` 单测：atlas shelf 打包（已知输入 → UV 确定 + 不重叠 + 覆盖输入）、`workspace.json` 读写 round-trip、`build` 编排产物齐全、错误路径（冲突/缺图/超尺寸/缺字体）。
- 图集打包 `assert`-based 自检（小图集 → UV 不重叠且覆盖全部输入）。
- Unity 侧：`SpriteResolver` 重写后表查询单测、UV 线性映射单测（替换旧 Sprite.uv 测试）。
- 跨层集成：PlayMode 加载新产物渲染验收（照 CLAUDE.md SDD 要求，merge 前必跑 showcase 逐项过——CSS 语义集成只在 PlayMode 显现）。

## 10. 分阶段实现（一份 spec 盖全）

1. **阶段一 Rust 引擎层**：`workspace.json` 读写 + atlas 打包 + `build` 编排 + `runtime.json` + 零参 CLI。产物能出。
2. **阶段二 运行时消费**：pkg.bin 删 manifest 段（v12→v13）+ core `set_image_sizes` FFI + Unity `SpriteResolver` 重写 + 废 `LoomSettings`。闭环：CLI 打包 → PlayMode 渲染。
3. **阶段三 Tauri GUI**：GUI 壳 + 拖拽 + 最近工作区。
4. **阶段四 Unity 拆除**：删旧编辑器脚本，留「Open Packer」MenuItem。
5. **阶段五 文档更新防漂移**：main-design / CLAUDE.md / 工作区 skill / roadmap / 记忆全部同步（见 §11）。

阶段一二先拿到「命令行能打包 + 能渲染」的闭环；三四是易用性外壳；五收口防漂移。

## 11. Roadmap 落位 + 文档更新（防漂移）

**roadmap（`docs/roadmap/roadmap.md`）**：
- 归 §3「v other — 编辑器工作流」，代号 v1.8+（编辑器档，独立并行，不阻塞主线 v1.x）。
- 更新 §3：`LoomSettingsWindow`/`LoomWorkspaceInitializer`/`LoomConfigExporter` 那套 Unity 内实现标为**被本设计取代**。
- 更新 G1「打包器 `loomgui_pkg`（…+图集）」：图集打包归 Rust 自绘。

**文档**：
- `docs/design/main-design.md`：图集/资源管线那节改（图集 Unity 管 → Rust 自绘）。
- `CLAUDE.md`：新 GUI crate 构建命令、图集打包链、废弃 Unity 编辑器脚本说明。
- **工作区 `CLAUDE.md`/skill**（需求 6）：新配置格式 + `loom-pkg build` 一条命令用法 + img src 相对 html 的写法约定。

**记忆**：更新 `v1-4-package-refactor-direction`——图集从「交 Unity」改成「Rust 自绘」；工作区从 `Assets/LoomUI/` 内改成完全独立。
