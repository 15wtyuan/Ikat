# loom CLI 闭环第一期：check / build / init / new / list / show / font·atlas add / version + 分发 + GUI 接线

- 日期：2026-08-19（v2：并入同日与维护者闭环对齐的七项决策——cli 管 workspace、两级查询、unity.json 基座、GUI 残余职责、新建向导、`loom` 命名、版本同轨确认）
- 状态：**设计定案，未实施**（本文档即实施依据；调研过程由 Tripawd 侧 AI 会话完成，四路调研：cargo/rustc、eslint/stylelint/prettier、gh/kubectl + 2025-26 agent-CLI 惯例、Godot/TexturePacker/Vite/FairyGUI/Bevy/Unity）
- 契约依据：fence.md（collect-all 诊断原则）、`crates/fence/src/diagnostic.rs`（Diagnostic 结构 = 输出契约的数据模型）、`docs/roadmap/roadmap.md` T2 dogfood（里程碑 2 主体）、T3 编辑器/工具链闭环
- 验收载体：Tripawd `ui/` 工作区——AI 无需打开 GUI、无需 LoomGUI 仓库 checkout，仅凭工作区内的 `.loom/loom.exe` 完成「摸现状 → 建包/配资源 → 编辑 → check → 修 → build → 产物落 Unity」全循环

## 1. 动机与范围

### 1.1 循环断点（为什么做）

AI 驱动 UI 的循环 = 摸现状 → 编排资源 → 编辑 → 验证 → 发布。当前断点：

1. **入口缺失**：`loom-pkg` CLI 只能 `cargo run -p loomgui_pkg`（限 LoomGUI 仓库 checkout）；游戏仓库侧唯一"官方"入口是 GUI 按钮。Tripawd 的 `ui/AGENTS.md` 模板里写的 build 入口对游戏仓库的 AI 等于不存在。
2. **输出不可机读 + 模板已在撒谎**：围栏诊断在库内是结构化的（`fence::Diagnostic`），但 CLI 错误路径把它 `{:?}` 拍平成 String（见 §4.1），AI 只能解析文本。更重的是：workspace-agent.md 模板 L10 已向用户承诺「diagnostics are collect-all, not fail-fast」——**打包器跨组件实际是首错即断**，模板承诺了不具备的性质；§4.1 修复后此承诺才成立。
3. **发布靠人**：GUI 是唯一入口，打包按钮人工按；且产物落点（Unity Assets）没有任何机器可用的配置通道。
4. **workspace 不可编排**：AI 进会话后无法得知工作区现状（有哪些包/字体/图集），改配置只能手编 JSON。量级背景（硬前提）：**一个 UI 工作区 = 整个游戏的所有 UI**——包是几十上百，页面/组件/图片是成千上万。这个量级下手编 workspace.json 是事故制造机，查询也不能全量吐（AI 上下文装不下）。

### 1.2 本期范围（定案）

- CLI 子命令：`check` / `build` / `init` / `new` / `list` / `show` / `font add` / `atlas add` / `version`（二进制名 `loom`，见 §3.1）
- 结构化诊断（`PackDiagnostic` + `BuildFailure`）+ 修复 collect-all 违约（§4.1）
- workspace 管理命令：写（new / font add / atlas add）+ 两级查询（list 摘要 / show 明细）
- 反向配置 `.loom/unity.json`（基座）+ output_dir 落点解析链（§3.4）
- 脚手架下沉（GUI 的 `write_agent_scaffold` 迁入 loomgui_pkg）+ 新增 `loom` skill 模板
- GUI：新建向导（选目录 + agent 勾选 → 一步初始化，砍独立初始化按钮）；build/init 走 `loom` 子进程；workspace 表单保留（人类检查 AI 配置）；warnings 渲染进日志
- 分发：release.yml 挂 `loom.exe` 资产；版本与 unity 包同轨（0.0.5 起）；Editor/Tools 双 exe（`loomgui_gui.exe` + `loom.exe`）

### 1.3 明确不做（后续期）

- `watch`（eslint/prettier/stylelint 均不内置，官方推荐外挂循环调一次性 CLI；AI 循环本来就不需要）
- `fix`（诊断先预留 fix 字段位，围栏逐条补 machine-applicable 建议）、`explain`、`verify`（Unity batchmode 冒烟）、`mcp`（firebase 式薄层，CLI 先行）
- `publish` 子命令——**build 即发布**，产物落点全由 §3.4 基座链决定；「AI 询问用户是否直接发布」是 skill 教的 agent 礼仪，不是 cli 机制
- **sprite 查询/搜索命令**——量级上万，任何 list 形态都兜不住；agent 浏览文件系统（`assets/` 即真相源），引用漏覆盖由 check 的交叉校验兜底
- per-entity 引用深查询（「这个包引用了哪些图」需跑 analyze）——后续期按需升
- 跨平台二进制矩阵（本期 Windows，与本仓库 dll 分发现状一致）

## 2. 调研依据（业界收敛模式）

六个收敛模式 + 三个反面教材，全文见调研会话；此处只记结论与出处：

| 模式 | 出处 | 对本设计的直接输入 |
|---|---|---|
| check/build/fix 三分 | cargo check（"compile without codegen"，所有 watch 工具默认跑它）、eslint `--fix`/`--fix-dry-run`、prettier `--check`/`--write` | `check` 是 AI 迭代主命令；报告与变更分离，报告默认零副作用 |
| 每命令机读输出 | cargo `--message-format json`（NDJSON + reason 标签 + build-finished 哨兵）、rustc 诊断 JSON（level/code/spans/children/suggested_replacement/suggestion_applicability）、eslint `-f json`、gh 每读命令 `--json <fields>` | 诊断 JSON 抄 rustc 形状；`format_version` + 版本内只增不改（cargo metadata `--format-version=1` 模式） |
| 退出码分离数据性/工具性失败 | eslint/prettier 0/1/2；Arcjet agent-CLI（0/1/2/3/4） | 0 干净 · 1 有 Error 级诊断 · 2 用法/配置/工具错误 |
| watch 不进核心 | 三大 lint/format 工具源码级验证无 watch；cargo-watch 独立二进制 | 不做 |
| 发布到引擎 = 显式 headless 步骤 | Godot `--headless --import`、Unity `-batchmode -executeMethod` + 外部写入 Assets/ 须显式 `AssetDatabase.Refresh()`、TexturePacker `.tps` 即配置可被 CLI flag 覆盖 | `output_dir` 直指 Unity Assets 只是"写到位"；verify 留给后续期 |
| agent 专门约定 | clig.dev（stdout=数据/stderr=诊断、非 TTY 不交互）、Arcjet（"agent 开始用之后命令就是契约，只增不改"）、AGENTS.md 标准 | stdout JSON、stderr 进度；`.loom/` 拷贝让游戏仓库 agent 零安装；写命令参数全接、非 TTY 缺参即用法错 |

反面教材：**FairyGUI CLI 发布静默无输出**（ask #27446，自动化里等于没有——本设计每个命令必须响亮）；**Stylelint 自创退出码**（1=fatal 2=lint 与 eslint 相反，生态痛点——第一天锁死 0/1/2 写进文档）；**Godot `--headless` 无法渲染**（截图要 Xvfb——LoomGUI 的 Rust 核心只产 `Vec<RenderNode>` 几何不出像素，像素级预览不存在便宜做法，故本期不做 shot）。

## 3. 命令面与输出契约（定案）

### 3.1 命令表

**二进制名 `loom`**（`loomgui_pkg` 的 `[[bin]] name` 改为 `loom`；crate 名不动——改名是纯 churn）。命令面全顶级动词：

```
loom check  <dir> [--format human|json]        # 围栏+组件注册+资源覆盖校验，零写入
loom build  <dir> [--format human|json]        # 现有 build + 结构化输出（默认 human 保持现状）
loom init   <dir> [--agent claude|agents]... [--unity-root <path>] [--output <dir>] [--force]
                                               # 脚手架 + workspace.json 骨架 + 自拷贝到 <dir>/.loom/
loom new    <name>                             # 工作区内建 ui/<name>/main.html + 注册 package
loom list   pkg|atlas|font [--format json]     # 摘要级查询（见下）
loom show   <pkg> [--format json]              # 单包明细（见下）
loom font add <file> [--family <f>] [--default] [--fallback]
                                               # 拷文件进 fonts/ + 注册 fonts[]
loom atlas add <dir> [--name <n>] [--max-size <n>] [--padding <n>] [--standalone]
                                               # 注册 atlases[] 一条
loom version [--format json]
```

细节约定：

- `check` **不要求** `output_dir` 非空（无写入）；`build` 保持要求（现状 build.rs:399-404）。
- `new` 在当前目录（须为工作区根，即存在 loom.workspace.json）执行；包目录默认 `ui/<name>`；`main.html` 为最小围栏合法页（div + 内联 style，保证 init+new 后 `check` 必绿——cargo new "hello world" 同款承诺）。重名包拒绝。
- `init` 已存在 workspace.json 时拒绝，`--force` 覆盖；`--output` 默认 `dist`；`--agent` 可重复（`claude` → CLAUDE.md + `.claude/skills/`，`agents` → AGENTS.md + `.agents/skills/`），不传默认 `agents`。自拷贝：`std::env::current_exe()` → `<dir>/.loom/loom(.exe)`。`--unity-root` 写 `.loom/unity.json`（§3.4；GUI 新建向导传参——GUI 从 Unity 菜单拉起，天然知道 Unity 工程根；内部做相对化，跨盘符等无法相对化的场景写绝对路径）。stderr 打印后续步骤提示（cargo new 风格：`loom new <pkg>`、指向 Unity Assets 的 output_dir 建议）。
- `list` **摘要级**（量级硬约束：包几百、页面/图上万，全量吐会炸 AI 上下文）：
  - `list pkg`：每包 name + 页面数 + 自定义组件数（一行一包）
  - `list atlas`：每图集 name + dirs + sprite 数（量级几十，字段全列）
  - `list font`：family + file + default/fallback（量级个位数到几十）
- `show <pkg>` **明细级**：该包的 HTML 文件列表、注册的自定义组件清单。纯配置 + 文件系统扫描（`resolve_html_list` 现成），**不跑 analyze**——重校验是 check 的事。
- 写命令（`new` / `font add` / `atlas add`）成功后**回显变更实体 JSON**——AI 加完即见结果，不重查。非 TTY 缺必要参数（如 `font add` 缺 `--family`）→ 用法错 exit 2（clig.dev：非 TTY 不交互）；TTY 交互提示留后续期，本期参数全接。
- `version` 输出 `{ "cli": "<ver>", "unity": "<ver>", "pkg_format": 38, "format_version": 1 }`；`cli == unity`，都取 `env!("CARGO_PKG_VERSION")`（§6.2 版本同轨）；`pkg_format` 引 `core/src/asset/mod.rs` 的 `PKG_FORMAT_VERSION` 常量（现值 38），不硬编码。
- main.rs 保持手写参数解析（不引 clap：仓库零 clap 风格 + 分发体积），重构为 `enum Cmd` 分发；`--help`/无参打印用法，退出 2。

### 3.2 退出码（第一天锁定，写入 --help 与 skill）

| 码 | 含义 |
|---|---|
| 0 | 成功；warning 不算失败（与 eslint 一致） |
| 1 | check/build 发现 Error 级诊断；写命令冲突（重名包/字体 family 重复注册等），带说明输出（**数据性失败**：输出里有全部信息） |
| 2 | 用法错误 / workspace.json 不可读不可解析 / output_dir 未配置 / `.loom/unity.json` 存在但指向的 Unity 工程路径不存在 / io 错误（**工具性失败**） |

### 3.3 JSON 输出 schema（format_version 1，只增不改）

`--format json` 时：**stdout 输出单个 JSON 文档**（eslint 式，非 NDJSON——loom 工作区构建是秒级，无需流式），进度日志走 stderr；成功失败都输出，退出码另行表达成败。`list` / `show` / 写命令回显同此约定（stdout 单文档 + `format_version`）。

```jsonc
{
  "command": "check",            // "check" | "build" | "list" | "show" | "new" | "font add" | "atlas add" | ...
  "format_version": 1,
  "success": false,
  "summary": { "errors": 3, "warnings": 1 },
  "diagnostics": [
    {
      "severity": "error",       // "error" | "warning"
      "code": "FenceUnknownTag", // fence DiagnosticCode 的 Debug 名（现状即 format!("{:?}", code)，字符串稳定）或打包器合成码，见 §4.1
      "component": "main",       // 组件名（页面名或 components/<tag>.html 的 tag）
      "file": "ui/battle/main.html",
      "line": 12, "column": 3,
      "message": "tag `p` is not in the fence",
      "help": "use div, or a role-driven control"   // 可空
      // "fix": { "range": [46, 47], "text": "div", "applicability": "machine" }  // 预留，本期恒不发
    }
  ],
  "report": { /* BuildReport 原样（packages/atlases/fonts/log/warnings），仅 build 成功时出现 */ },
  "entity": { /* 写命令回显的变更实体 / list / show 的查询结果，仅相应命令出现 */ }
}
```

人类模式（默认）：错误用新渲染器（rustc 风格 `error[FenceUnknownTag]: message (file:line:col)` + help 缩进行，替换现状 `{:?}` Debug 输出）；warning 保持 `PackWarning::render` 格式；`OK:` 汇总行不变。`list`/`show` 人类模式打表格/对齐行。

### 3.4 输出落点：`.loom/unity.json` 基座链（新）

- 新文件 `.loom/unity.json`：`{ "unity_root": "../unity" }`——**相对路径优先**（相对工作区根；Tripawd 式 ui/ 与 unity/ 同父布局下此文件可入库，整团队 clone 即用），无法相对化（Windows 跨盘符等）fallback 绝对（机器绑定，团队各自重建）。解析一条：`is_absolute()` 直用，否则 `workspace_root.join()`。
- `output_dir` 语义从「相对工作区根」升级为「**相对基座**」：
  - 有 `.loom/unity.json` → 落点 = `unity_root.join(output_dir)`（workspace.json 写 `Assets/Bundles` 即直达 Unity 工程）
  - 无 → 基座 = 工作区根，**行为与现状完全一致**（老工作区、CI、dist 场景零破坏）
  - `unity_root` 指向的路径不存在（Unity 工程挪了/同事没这文件）→ **不静默 fallback**，exit 2 明确报「反向配置指向的 Unity 工程不存在；重开 GUI 重建，或删 .loom/unity.json 回退本地输出」。注意区分：文件**不存在**=自动本地输出（合法形态）；文件**存在而路径失效**=报错。
- `output_dir` 留在 `workspace.json` 里继续入库——机器无关的团队约定（「产物落 Assets/Bundles」）；机器差异全部隔离在 `.loom/`。
- gitignore 建议：`.loom/*` + `!.loom/unity.json`（ignore 二进制、保留配置；入不入库最终由游戏仓库自定，skill 里两种都写）。
- `check` 零写入、与落点无关，不读 unity.json。

## 4. 代码改造点（file:line 基准：2026-08-19 main；均已逐条核过属实）

### 4.1 诊断地基：PackDiagnostic + BuildFailure + 修 collect-all 违约

**问题**：两处把 `Vec<Diagnostic>` 用 `{:?}` 拍平成 String 且**首个含 Error 的组件即中断**（后续组件不再解析——fence 的 collect-all 意图止步于单文件内，打包器跨组件循环把它断了；且 workspace-agent.md 模板 L10 已向用户承诺 collect-all，现状承诺不成立）：

- `crates/packer/pkg/src/build.rs:152-161`（`pack_components_inner`，页面组件）
- `crates/packer/pkg/src/expand.rs:87-96`（`ComponentRegistry::register`，自定义元素组件）——两处近似复制

**改造**：

1. 新增 `PackDiagnostic`（建议放 `pkg/src/diag.rs`）：`{ severity, code: String, component: String, file: String, line: u32, column: u32, message: String, help: Option<String> }`，derive `Serialize` + `render()`。它是 `PackWarning`（build.rs:29-67）的超集；**直接替换 PackWarning**，`BuildReport.warnings: Vec<PackWarning>` 改为 `Vec<PackDiagnostic>`（序列化字段是超集，GUI 前端兼容）。
2. 统一两处重复的 Warning→结构化转换（build.rs:169-187 与 expand.rs:97-115 → 一个共享函数；注意 build.rs:164-168 注释：file 须用 `html_rel` 覆盖，因 `parse_template(src, name)` 把 location.file 设为组件名）。
3. 拍平点改为：收集该组件全部诊断（Error+Warning 都要）→ **继续处理后续组件** → 全部收集完后若有 Error 则整体失败。`pack_components*` 返回 `Result<PackResult, BuildFailure>`。
4. 新增 `BuildFailure { exit_code: u8, message: String, diagnostics: Vec<PackDiagnostic> }`，derive `Serialize`（Tauri IPC 要求错误可序列化——GUI `run_build` 的 `Result<BuildReport, String>` 契约变为 `Result<BuildReport, BuildFailure>`，前端 reject 值从字符串变对象，GUI 侧消化，见 §5.4）。
5. `build()`（build.rs:397-581）及全链 ~20 个 `map_err(|e| format!(...))` 位点机械替换为 `BuildFailure::config(...)`（exit_code=2）或 `BuildFailure::validation(...)`（exit_code=1 带 diagnostics）。分类见 §4.3。
6. 合成诊断码（非 fence DiagnosticCode，打包器自造，与现状 `ComponentKeyframesNameCollision`（build.rs:203-216）同层）：
   - 资源覆盖校验（atlas/validate.rs:18/:24 现为中文 String 错误）→ `SpriteMissingFromAtlas` / `SpriteAtlasConflict`（message 保留现有中文文案）
   - 字体文件缺失（build.rs:470-484 的 Err）→ `FontFileMissing`（check 也要报它）
   - 图集溢出（atlas/pack.rs:29-34 超尺寸）→ `AtlasImageOverflow`
   - 组件重名（build.rs:246）→ `DuplicateComponentName`
   - config 类不进诊断：output_dir 未配置、workspace 解析失败、unity_root 路径失效 → exit_code=2 纯 message。

### 4.2 check 与 build 共享计算段（管线拆分）

从 build() 抽出 `analyze(root) -> Result<AnalyzeOutcome, BuildFailure>`（**零写入**）：

```
load_workspace（不查 output_dir、不读 unity.json）
→ scan_component_registry（components/ 注册表解析，诊断收集）
→ 逐包 resolve_html_list + parse_template（全量收集诊断，不首错即断）
→ 逐图集 collect_pngs + pack_atlas（只算不写——pack_atlas 本身不落盘，写页 PNG/atlas.json 在 build() 里，天然可复用）
→ assign_and_validate（HTML 引用 × 图集覆盖交叉校验 → 诊断化）
→ AnalyzeOutcome { workspace, atlas_manifests, per-package parse 产物, diagnostics }
```

- `build(root)` = `analyze` + 落点解析（§3.4 基座链）+ 写入段（mkdirs / clean_stale_outputs×3（保 .meta）/ 写图集页+atlas.json / 拷字体 / 写 ui/*.pkg.bin / 写 loom.runtime.json）+ 汇总 BuildReport。产物字节不变（pkg/tests/schema_lock.rs 的 LOCKED_HASH `0xdcc7_31a9_4d73_0975` 必须不动——它只锁 pkg.bin 字节，本重构不碰序列化）。
- `check(root)` = `analyze` 后直接输出诊断报告。**校验语义 check/build 单代码路径**，天然不漂移（GUI↔CLI 统一的同款论证，降一层）。

### 4.3 exit_code 归类

- `2`（config/tool）：参数错、workspace 读/解析错、output_dir 空（仅 build）、unity_root 路径失效（仅 build）、输出目录 io 错、字体拷贝 io 错。
- `1`（validation）：全部 fence Error 诊断 + §4.1(6) 合成码诊断 + 写命令冲突（重名/重复注册）。warning 永不抬退出码。

### 4.4 workspace 管理命令模块（新）

`new` / `list` / `show` / `font add` / `atlas add` 落 `pkg/src/workspace_cmd.rs`（新）：共享 `Workspace` struct 读改写（整文件重写、无锁）。与 GUI 表单的双写冲突容忍：都是整文件覆盖、编辑低频、最后写者赢——文档写明即可，不加锁。

### 4.5 受波及测试（更新而非放松）

- `pkg/tests/build.rs` 5 个 e2e：`build_fails_when_font_file_missing` / `build_fails_when_output_dir_empty` 断言从 Err(String) 改为断言 BuildFailure 字段（code / exit_code）。
- `pkg/src/build.rs:613-854` `package_tests` 单测：错误断言同步改。
- 新增：多文件多错误 collect-all 回归（两个组件各含错 → 两条诊断都在）；check 零写入断言（跑 check 后目录 mtime/内容不变）；JSON schema 快照（serde_json::Value 断言关键字段）；workspace_cmd 的注册/重名/回显断言；unity.json 基座链三态（无文件→本地 / 相对有效→拼 unity_root / 路径失效→exit 2）。
- 不受影响：schema_lock.rs、smoke_ir_bridge / cascade_probe / control_init_bridge / keyframes_bridge / rich_text_flag（走 pack_components 成功路径，签名 Err 类型变化需改 `let _ =` 处错误断言，逐个过）。

## 5. 脚手架、skill 与 GUI 接线

### 5.1 脚手架下沉到 loomgui_pkg

- `write_agent_scaffold`（GUI commands.rs:54-74）+ 模板（`gui/src-tauri/templates/workspace-agent.md`、`templates/skill/SKILL.md`）迁入 loomgui_pkg 新模块 `scaffold`（模板文件随迁；`loom init` 直接调用，GUI 不再 include_str 模板——见 §5.4 重出条件收窄）。
- scaffold 输出物四件：
  1. 指令文档（`CLAUDE.md` 或 `AGENTS.md`，按 `--agent` 选择，`{{SKILLS_DIR}}` 占位逻辑不变）
  2. `<skills_dir>/loomgui-editor/SKILL.md`（现状）
  3. `<skills_dir>/loom/SKILL.md`（CLI skill，见 §5.2）
  4. CLI 二进制 → `<workspace>/.loom/loom(.exe)`（CLI `init` 用 `current_exe` 自拷贝——同一 scaffold 模块带 `cli_source: Option<PathBuf>` 参数）

### 5.2 新模板：loom skill

内容骨架（与 loomgui-editor skill 同 frontmatter 风格）：

- description：构建/校验/编排 LoomGUI 工作区时使用；任何 check/build/init/new/list/需求。
- 命令表 + 退出码表 + JSON schema 摘要（诊断字段含义、format_version 只增不改承诺）。
- 二进制定位顺序：`.loom/loom`（工作区自带）→ PATH → LoomGUI 仓库 `cargo run`（兜底）。
- AI 循环配方：`loom list` 摸现状 → 没有字体时**先向用户要字体文件**再 `loom font add` → 图片放 `assets/` 后 `loom atlas add` 划目录 → `loom new` 建包 → 写 HTML/CSS → `loom check --format json` → **一轮修完全部诊断**（collect-all 保证一次给全）→ 征询用户后 `loom build --format json` → 报告产物落点。stdout=数据 / stderr=进度 约定；警告不阻断但应处理（W1/W2 是预览≠运行时陷阱）。**改 workspace.json 一律走 loom 命令，不手编**。

### 5.3 workspace-agent.md 模板更新

Build 入口章节改写：CLI 优先（`.loom/loom check/build <工作区>`，附退出码与 `--format json`），GUI 次之（打开 packer 按 Build）；删除「CLI 仅限 LoomGUI 仓库 checkout 内 cargo run」的旧说法。L10 的 collect-all 承诺在 §4.1 修复后成立，保留。补 workspace 编排段（list/show/font add/atlas add）与指向 loom skill 的一行。

### 5.4 GUI 改造

- **新建向导**（替代现状「打开后另按初始化」）：选目录 → agent 勾选弹窗（多选，沿用现状 claude/agents 语义）→ 一步调 `loom init <dir> --agent ... --unity-root <从 Unity 拉起时已知的工程根>`。`init_workspace` IPC 退役。GUI 从 Unity 菜单拉起时 Unity 工程根由 C# 启动侧传入；直接打开 GUI（无 Unity 上下文）则省略 `--unity-root`（不写 unity.json，纯本地 dist 流）。
- `run_build`（gui commands.rs:124-127）→ spawn `loom build <path> --format json`。定位顺序：
  1. `std::env::current_exe().parent()/loom(.exe)`（Editor/Tools 双 exe 同放，版本配套）
  2. `<workspace>/.loom/`
  3. **dev fallback：进程内直调 `loomgui_pkg::build()`**，stderr 注明 `dev fallback: in-process build`。原因：`tauri dev` 的 target 在 `crates/packer/gui/src-tauri/target/`，根 `cargo build -p loomgui_pkg` 产在根 `target/`，仓库无共享 target-dir 配置——dev 模式下前两级必 miss，没有此级 GUI 开发期 build 直接挂。dev 场景陈旧性风险自负（开发者自己）；release/用户路径仍走子进程，双 exe 契约不破。
- 解析 stdout JSON → 还原 `BuildReport` 返回前端（成功路径前端零改动）；失败路径把 diagnostics 渲染为 log 行 + reject `BuildFailure`。
- **顺手修 GUI 缺陷**：前端 `showBuildReport`（dist/editor.js:681-699）丢弃 `report.warnings`——W1/W2 一致性警告在 GUI 里从未可见。本次让 warnings 渲染进构建日志（log-warn 类已有）。
- **workspace 表单保留**：定位是「人类检查 AI 配置的驾驶舱」——直读直写 `loom.workspace.json`（经 loomgui_pkg 的 Workspace struct，GUI 库依赖收窄为 workspace 读写），与 cli 不挂钩；与 cli 的双写冲突容忍（§4.4）。表单不再是任何流程的必经路，但保留为低频人工微调入口。
- **GUI exe 重出条件改写**（收窄，替换 AGENTS.md「GUI exe 绑 fence crate」段）：fence/pkg 的 build 语义变动、scaffold 模板/skill 内容变动**不再要求**重出 GUI exe（build 走子进程、模板不嵌 GUI 字节）；仍要求重出的：workspace JSON schema（Workspace struct）变动、GUI 自身代码变动。
- `templates_sync.rs` 漂移门扩展：新 skill 的命令名/退出码与 main.rs 分发常量对账（简单 contains 断言即可，模式照抄现有 fence-sync 锚点测试）。

## 6. 分发与版本同轨

### 6.1 二进制分发

- `loomgui_pkg` 的 `[[bin]] name` 改 `loom` 后：`release.yml` gh-release 步骤（L56-62，现无 `files:`）前加 `cargo build -p loomgui_pkg --release`，步骤加 `files: target/release/loom.exe`。Windows-only（与 dll 现状一致）。
- `rust-ci.yml` 按期上传的 artifact 名同步改 `loom-exe-<sha>`（现 `loom-pkg-exe-<sha>`，L82-89 机制不动只改名）。
- Editor/Tools 拷贝仪式（仓库 AGENTS.md GUI exe 闭环段）增补 `loom.exe`，双 exe 同放（`loomgui_gui.exe` + `loom.exe`）。

### 6.2 版本同轨（定案，照 v1 确认）

loomgui_pkg crate 版本（现 0.1.0，与发版无关）改为**与 `unity/package/package.json` 版本同轨**（下一个 tag 0.0.5）。`loom version` 的 `unity` 字段 = `env!("CARGO_PKG_VERSION")`，无需第二常量。`crates/xtask/src/release_check.rs::run_release_check`（:117-139）新增断言：pkg/Cargo.toml 的 version == package.json 的 version（tag==package.json 已由 release.yml L22-31 保证，链条闭合：tag → package.json → pkg crate → `loom version`）。release.yml 已跑 release-check，断言自动成为发版门。

同 tag 产物等价：dll（含 pkg.bin 读取端的 MIN/MAX=38 门）与 loom.exe（写入端，`PKG_FORMAT_VERSION` 同一 core 常量）出自同一 commit，schema 错位不可能发生。

## 7. 实施分解（顺序即依赖）

| # | 任务 | 主要落点 | 验证 |
|---|---|---|---|
| T1 | PackDiagnostic + BuildFailure + 统一转换 + collect-all 修复 + 人类渲染器 | pkg/src/diag.rs（新）、build.rs、expand.rs、main.rs 错误输出 | `cargo test -p loomgui_pkg`；新增 collect-all 回归 |
| T2 | analyze 抽段 + check 子命令 + JSON 输出层 + main.rs 子命令分发重构 | pkg/src/{build.rs,main.rs,report.rs(新)} | `cargo test`；手跑 `loom check` 于 showcase |
| T3 | `[[bin]]` 改名 loom + init/new/version + list/show + font add/atlas add + unity.json 基座链 + workspace_cmd 模块 + 版本 bump 0.0.5 + xtask release-check 扩展 | pkg/src/{init.rs,workspace_cmd.rs(新),main.rs}、crates/xtask | `cargo test -p loomgui_pkg -p xtask`；临时目录全流程（init→new→font add→check 绿→build 落 unity_root） |
| T4 | 脚手架下沉 + loom skill 模板 + workspace-agent.md 更新 + templates_sync 扩展 | pkg/src/scaffold.rs（新）、模板迁移、gui 测试 | `cargo test -p loomgui_gui`（模板门） |
| T5 | GUI 新建向导 + build 子进程化（含 dev fallback）+ warnings 渲染 + 表单定位调整 | gui commands.rs、editor.js | GUI 冒烟一次打包（含故意违规页看诊断） |
| T6 | release.yml 资产 + rust-ci artifact 名 + Editor/Tools 双 exe 仪式 + 仓库 AGENTS.md（重出条件改写）/ CHANGELOG | workflows、docs | `cargo run -p xtask -- release-check` 本地过 |
| T7 | 全门禁 + tauri exe 重建 | fmt/clippy/test 全绿；`tauri build --no-bundle` + 双 exe 入库（Unity 须关） | CI 绿 |

## 8. 风险与回归点

1. **`pack_components*` 签名变更波及面**：pkg 7 个集成测试文件的错误断言需逐个更新（测试即规格，逐个过而非批量 sed）。
2. **collect-all 行为变化**：以前只报首个出错组件，现在全报——输出行数变多属预期改进；下游仅 CLI stderr 与 GUI，无依赖旧行为者；文档化即可。
3. **tauri exe 重建依赖**全局 tauri-cli 且 Unity 必须关闭（锁 exe）；环境不满足时 T7 拆出人工执行。
4. **`.loom/` 与 .gitignore**：ignore 二进制、保留 `unity.json`；游戏仓库若全 ignore 了 `.loom/`，团队其他人/CI 拿不到 CLI——skill 里写明规范下载位（GitHub Release 资产，tag 与 unity 包同轨）。
5. **相对 unity_root 挪窝**：UI 工程或 Unity 工程任一挪位 → 路径失效 → exit 2 报错引导重建（§3.4）；绝对路径形态同理。
6. **GUI/cli 双写 workspace.json**：无锁整文件覆盖，最后写者赢；编辑低频，容忍并文档化（§4.4）。
7. **量级下的查询输出**：`list` 必须守摘要级纪律（包级一行；文件明细只进 `show`）；skill 教 AI「先 show 后改」，别全量扫。
8. **dev fallback 双路径**：GUI `run_build` 存在子进程/进程内两分支，T5 测试两条都过（dev 模式冒烟 + release 模式冒烟）。
9. **JSON schema 承诺**：`format_version: 1` 内只增不改；后续加 fix 字段（占位已在 schema 注释里）不算破坏。

## 9. 验收清单（对照 Tripawd ui/）

- [ ] Tripawd `ui/`：AI 运行 `.loom/loom.exe check --format json` 得结构化诊断；修复后 exit 0
- [ ] `loom list pkg|atlas|font` / `loom show <pkg>` 在 Tripawd 上正常出摘要/明细
- [ ] `loom font add` / `loom atlas add` 注册后 `check` 绿、`build` 产物含新字体/图集
- [ ] `.loom/loom.exe build` 后产物落 Unity 工程 `Assets/Bundles`（unity.json 基座链生效），Unity 开着即自动 import
- [ ] GUI「新建工作区」向导（选目录 + agent 勾选）一步产出：指令文档、loomgui-editor skill、loom skill、`.loom/loom.exe`、`.loom/unity.json`
- [ ] GUI 打包走子进程（任务管理器可见 loom.exe），W1/W2 警告在 GUI 日志可见；GUI workspace 表单仍可打开检查配置
- [ ] `loom version --format json` 输出 cli==unity==0.0.5、pkg_format=38
- [ ] release tag v0.0.5 的 GitHub Release 挂 loom.exe 资产
