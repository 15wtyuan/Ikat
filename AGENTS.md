# LoomGUI 开发指南

本文件为在本仓库工作的 AI 编码代理（Claude Code / Codex 等）提供指引。

## ⚠️ 模型禁令（硬规则，违者罚钱）

**subagent 严禁使用 `netease-codemaker/*` 系列模型**（如 `netease-codemaker/claude-opus-5`、`netease-codemaker/deepseek-v4-pro`、`netease-codemaker/glm-5.2` 等）。dispatch 任何 subagent（Agent 工具的 `model` 参数）必须避开整个 `netease-codemaker/` 前缀，改用直连 provider 的可用模型：`DeepSeek/deepseek-v4-pro`、`DeepSeek/deepseek-v4-flash`、`Zhipu/glm-5.2` 等。撞输出上限就换 provider/换更小模型，**绝不**退回 netease-codemaker。

## 这是什么

LoomGUI = 跨引擎游戏 UI 框架。标准 HTML/CSS 子集作设计期 DSL，类型化对象树作运行时 API，自绘渲染。核心目的：**AI 驱动的界面拼装**——标准 HTML 作 DSL，让 AI 既能编辑（文本）又能预测渲染结果（AI 对 HTML/CSS 有强先验）。

对标 FairyGUI、RmlUi、Unity UI Toolkit（参考实现在 `temp/FairyGUI-unity/`、`temp/RmlUi/`，只读）。差异化：标准 HTML/CSS（vs fgui 的 `.fui` 二进制 AI 看不懂）、类型化对象树（标准 HTML 元素决定稳定类型）、Rust 跨引擎共享核心、围栏验证器。Blitz（`temp/blitz/`）可作**参考源**但不作底层（跨引擎 FFI 立身之本/Stylo 与围栏哲学冲突/无打包期边界/节点模型不兼容/pre-alpha）——按子系统取材：CSS 装饰数学→blitz-paint、文本→Parley 生态、scroll 物理→fgui、布局→RmlUi。

## 构建 / 测试命令

```bash
# 核心（引擎无关纯库，可单测）
cargo build -p loomgui_core
cargo test  -p loomgui_core

# loom CLI（HTML+CSS+资源 → .pkg.bin + 自绘图集 + fonts；二进制名 loom，复用核心 parse 层）
# 命令面：check / build / init(--ui) / new / list / show / font add / atlas add / scaffold / version；退出码 0=干净（warning 不算失败）/ 1=数据性 / 2=工具性。
# 输出契约：stdout 单 JSON 文档、stderr 进度（clig.dev）；`format_version` 只增不改；写命令成功回显实体 JSON；`list` 摘要级纪律（全量吐会炸 AI 上下文）。
# watch / mcp 子命令 rejected by design（AI 循环用 check 轮询 + assets/ 是真相源；业界无 watch 先例）。
# 工作区拓扑：会话根（.loom/ = exe + config.json 双指针 ui_root/unity_root + skills）≠ ui 目录（loom.workspace.json）；
# 目录解析统一 config 发现（会话根 / ui 本体 / ui 直接子目录均可作参数或 cwd），详见 pkg/src/config.rs。
cargo build -p loomgui_pkg
cargo test  -p loomgui_pkg
# 运行：cargo run -p loomgui_pkg -- check <workspace-dir>    （loom check <workspace>）

# 独立打包器 GUI（Tauri 桌面应用；出 exe 见下方「GUI 打包器 exe 闭环」段，勿用 cargo build）
cargo tauri dev                  # 开发热重载（需 tauri-cli）
cargo tauri build --no-bundle    # 出 exe（cargo build --release 不 embed 前端 → localhost 白屏 exe）

# FFI（C ABI；csbindgen 在 build.rs 里重新生成 C# 绑定）
cargo build -p loomgui_ffi_c

# 整个 workspace
cargo test
```

**跑单个测试 / 围栏门**：
```bash
cargo test -p loomgui_fence                              # ← 围栏契约门
# (snapshot 测试已移至 fence crate)
```

**基准测试**：暂无 `[[bench]]` 目标（criterion 待后续配置）。

**CI 门禁**（`.github/workflows/rust-ci.yml`，push main / PR 触发）：fmt 严（`cargo fmt --all -- --check`）+ clippy 严（`cargo clippy --all-targets -- -D warnings`）+ Win/Ubuntu matrix test + feature-gate check（`--no-default-features --all-targets`）+ Windows `.dll` artifact（release build）。**push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`**，否则 CI 红。clippy 各 crate root 有 `#![allow]` 放行可辩护的测试/FFI 模式 lint（`field_reassign_with_default` / `not_unsafe_ptr_arg_deref` / `too_many_arguments` 等，带理由注释），勿误清——新增可辩护模式 lint 在那里加。

**发版（tag 触发 Release workflow）**：三道硬门：tag 名 == `unity/package/package.json` 的 version，且 == `crates/packer/pkg/Cargo.toml` 的 version（不齐则 `loom version` 撒谎），且 CHANGELOG 有对应 `## [<ver>]` 段落——**打 tag 前先双 bump + 补段落 + `cargo run -p xtask -- release-check`**。git-URL 装包的版本号解析自 tag 指向 commit 的 package.json，漏 bump = 消费者装错版本号（不止 CI 红）。**CI 不编 .dll**——git URL 分布拉的是 tag commit 快照，.dll 必须已在 tag commit 内（Release workflow 只验证+出 artifact；release-check 只查 dll 存在性，字节 staleness 无法校验）。

### Rust → Unity .dll 闭环（Windows 本机是唯一的编码机）

按记忆/工作流：**任何** Rust 改动后必须重编 + commit `.dll`，否则测不了。

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
- **拷贝时 Unity 必须关着**（它锁 .dll）。
- **同步 C# 绑定**：build.rs 不再自动写 Unity 绑定。构建后运行 `cargo run -p xtask -- sync-bindings`，将 csbindgen 生成的 `LoomGUIBindings.cs` 同步到 `unity/package/Plugins/LoomGUI/Bindings/`。

**图集自绘**：图集由打包器自绘（`loom build` 或 GUI 产 `atlas/*.png`+`atlas/*.atlas.json`）

### GUI 打包器 + loom CLI 双 exe 闭环（Editor/Tools）

GUI 是 Tauri 2 桌面 app；打包/初始化语义走 `loom` CLI 子进程（定位链：GUI 同目录 → `<workspace>/.loom/` → dev fallback 进程内）。产物双 exe 同放 `unity/package/Editor/Tools/`：`loomgui_gui.exe`（Unity `LoomGUI > Open Packer` 拉起，拉起时传 `--unity-root`）+ `loom.exe`（版本与 GUI 配套）。

```bash
cargo build -p loomgui_pkg --release
cp target/release/loom.exe unity/package/Editor/Tools/loom.exe

npm install -g @tauri-apps/cli                 # 一次性装 tauri-cli（prebuilt，比 cargo install 快得多）
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)    # 出 exe（必须 tauri CLI！--no-bundle 跳 NSIS/MSI installer）
# tauri-cli 产物在 workspace 根 target/release/（不在 src-tauri/target/）。
cp target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```

**重出条件（收窄过）**：fence/pkg 的 build 语义变动、scaffold 模板/skill 内容变动**不要求**重出 GUI exe（build/init 走 loom.exe 子进程、模板嵌在 loom.exe 里）；仍要求重出的：`Workspace` struct（workspace JSON schema）变动、GUI 自身代码（commands/前端）变动。loom.exe 与 GUI exe 同 commit 重出，防版本错配。

- **前端手写 dist（无 npm 构建）**，靠 `app.withGlobalTauri:true` 注入 `window.__TAURI__`（tauri.conf.json）；缩略图靠 `app.security.assetProtocol` + Cargo `tauri` feature `protocol-asset`。

## 架构

权威契约全在 `docs/design/`，此处不复述（复述 = 漂移面）：

- **main-design.md** — 分层与每帧单向数据流、公共/内部/FFI/后端四层边界、架构不变量（语义签名决定类型、CSS 只换 Strategy 不换类型、tick 显式依赖拓扑、每帧一次 solve、单动画时钟、坐标系、公共树≠渲染树）、渲染管线、动画、资源、FFI、每帧管线。**违反不变量 = 隐 bug**。
- **fence.md** — HTML/CSS 子集权威清单（真相源 = `crates/fence/src/schema/` Rust const 表；改 schema 必同步 fence.md，防漂移门 `cargo test -p loomgui_fence` 含文档↔schema 交叉校验）。
- **public-api.md** — 公共 API 终态契约；**projection-layer.md** — 公共 API 的 C# 投影实现契约。

## 在本仓库怎么干活

- **设计文档 vs 踩坑 vs 工作项**：`docs/design/`（架构/围栏/API/投影层，见上节）、`docs/pitfalls.md`（踩坑规则手册 + 依赖 API 适配）。**活工作项 = GitHub issues**——milestone `M2 · Dogfood` / `M3 · 跨引擎与契约消化` / `v1.0 · 发版`（门控序列），label `t1-capability` / `t2-release` / `t3-expand` / `tx-debt`，非契约项带 `deferred`（触发判据在 issue 正文）；排活/查进度用 `gh issue list`，不开新文档清单。`docs/roadmap/` 是归档只读历史（stub 留北极星判据），别拿它干活。
- **Rust edition 2021**，依赖钉版本：`taffy 0.12`、`ttf-parser 0.20`、`slotmap 1.1`、`csbindgen 1`。CSS 选择器解析器手搓（零新依赖，spike 阶段推翻了"接 cssparser"前提）。
- `Cargo.lock` 入库（根级，尽管 `.gitignore` 有通用 `Cargo.lock` 行——它是被追踪的）。
- 设计师工作区是独立磁盘目录（含 `loom.workspace.json`、HTML/CSS 源文件、res 资源、design-systems 组件库）。打包用独立打包器 GUI（Tauri `loomgui_gui`）或 CLI `loom build <workspace>`。运行时引导由 `loom.runtime.json` 统管。
- 用户只读中文——问答/选项/总结用中文；代码/commit 照旧英文。
- **代码注释写上线品质**：自包含、精简（说 WHY）、不引用内部编号或暗语、不引用任何文档，如有引用文档必要，直接把文本拷贝上去。踩坑记录不进代码，也不写任何 plan/spec 的文档编号。
- **修根因，别贴补偿参数**：去源头修，别在下游加参数补偿。
- **防文档漂移**：文档写定性不写数字；关键 claim 加可执行测试；改代码后搜 docs/ 是否引用了改动的 struct/函数/列数。

## 调试技巧

**dump_*.rs 诊断 example**（pkg.bin 路径，验 core 实际状态而非猜代码）：
- `dump_text` — 文本换行（验 known.width 来源、行数、pen 坐标）
- `dump_rich_text` — rich-text-block inline flow（验 inline 子折进父一崽测、runs/lines、folded rect=0、公共树 ID 保留）
- `dump_img` — 图片尺寸（css.w/h、rect、tex、闭包 `known.w`）
- `dump_scroll` — 滚动（overlap、scroll_pos、content_size）
- `dump_bg` — 节点 base_style（验是否进 pkg）
- `dump_nativehost_slot` — NativeHost FFI 查询
- `dump_shop` — showcase/shop 命中/层叠诊断：dump 全节点 layout_rect/display/touchable + hit_test 探针（定位点击被谁接走、inline vs class cascade 谁赢）
- `dump_mail` — showcase/mail 帧计时 + 阶段拆分（定位低帧热点：solve/render/build 谁独占；验虚拟列表 slot 数不随 ItemCount 涨）
- `dump_mail_scroll` — 虚拟列表覆盖诊断：set_scroll_pos 驱动 + 量化视口顶部空白 gap_top（定位 active slot 是否覆盖视口顶，漏 flex gap 会留空）
- `dump_home_anim` — 入场交错动画延迟取证（验 `:nth-child` 步进延迟是否正确，漏 TextNode 计数会全 0）
- `dump_slot_projection` — slot 投影布局（喂 pkg.bin 路径；验投影行进 taffy 各占一行 vs 被折进祖先 rich inline 流）
- `dump_pkg_flags` — pkg TemplateNode 全表（验 rich_text_block 打包期烙印 / taffy display / parent 链，分类问题第一步取证）

**跨层特性 PlayMode 报错**先 example 实测 core 状态再改，避免盲改物理掩盖 layout 根因。

**core dump 复现 Unity solve**：PlayMode layout/视觉 bug 先编码机用对应 dump_*.rs example 喂同样的 pkg.bin 复现 core solve，定位 bug 在 core（dump 错）还是 Unity 后端（dump 对、渲染错）。core 和 Unity 是同一份 solve 的两面，dump 取证再改，别静态猜反复试。

**Unity 侧结构化诊断入口**：`LoomHost.DumpSceneJson()`（Runtime 包内 public，未 instantiate 返 `"[]"`）dump 全场景节点树；showcase 工程另有 dev-only 的 `LoomBridge`（PlayMode-only：DumpScene/DumpMirrorPool）。uloop 探针读空先想这两个入口。

**布局差分验收 = rect-diff 工具链**（`showcase/scripts/rect-diff/`）：Chrome DOM rect（browser-rect.mjs）vs core DFS rect（`dump_page --json`）按 id+tag+class 容差比对。要点：semanticTag 归一（browser 按 role 归一，否则桶永远配不上）；loom-preview.js 是 core 行为模拟器**必须保留**（全拦会制造假 diff，如空 textbox 高度）；reset.css/letterbox 系统性偏移先排查是不是假 diff；0×0 盒不进 idless 桶；退出码 0/1/2/3（3=infra 失败≠布局回归）；`--scene` 走 PlayMode 模式。

**Unity 渲染 vs HTML 浏览器颜色对比（Chrome headless 取证）**：颜色「发白/偏亮/偏色」问题不能盲信「渲染对得上 CSS」——CSS 半透明合成在 sRGB 编码空间，Unity Linear 项目在 linear 空间，同一 CSS 算出不同值。取证：Chrome headless 截 HTML（`"/c/Program Files/Google/Chrome/Application/chrome.exe" --headless=new --disable-gpu --force-color-profile=srgb --force-device-scale-factor=1 --window-size=1920,1080 --screenshot=out.png "file:///abs/path.html"`）→ PowerShell `System.Drawing.Bitmap.GetPixel(x,y)` 读像素 hex → 和 Unity uloop `screenshot --capture-mode rendering` 像素逐字节对比。**控制实验先校准**：截纯色 `#hex` HTML 确认 Chrome 截图对纯色准（暗部可能偏），坐标用 PNG top-left = design 坐标。这是定位「颜色对不上」类问题的铁证方法，比静态猜强。

**测浏览器 background-size/contain/cover 等“布局/背景”渲染用 playwright（headless --screenshot 不可靠）**：`chrome --headless=new --screenshot` 对 background-image 布局/平铺测量不可靠（抢截、忽略 background-size 假象）。用 `playwright` + `channel="chrome"`（真 Chrome）`page.goto(file://HTML)` + `wait_for_load_state("networkidle")` + 整页或 element 截图，再 PowerShell 扫非背景色像素 bbox。**务必加 `background-repeat:no-repeat`**——CSS 默认 repeat 会把 contain 缩后的图平铺填满盒，测量出“填满”假象。

**加新控件类型（新 NodeKind/ControlState 变体）必 grep 所有按 kind/变体 dispatch 的点**（render arm、measure_text_controls、on_pointer_down/on_text_pointer_down、cursor blink、FFI setter or-pattern、**render 侧 mesh 合并的控件排除 or-pattern**）逐一确认覆盖——各层 dispatch 独立写、独立测，漏一个 = 控件半残/不可见（空 mesh）。且单测验不了 CSS 语义集成（display 子树剪枝、继承传播、多 spec 解析），收尾必跑 showcase PlayMode 逐项过。

**subagent 协作（多 agent 分工的教训）**：
- 模型选型：默认 `DeepSeek/deepseek-v4-pro` 起步；opus 级在本 repo 反复撑爆 subagent 输出上限——撞了就换模型，别硬重试（顶部模型禁令是硬规则）。
- task 切分别太细：强耦合重构（删共享字段类）的 task 边界要么包含被牵连函数，要么预期 bridge 多一轮 fix。
- 分支上并行有用户 commit 时，review BASE 用 task commit 的实际 parent（`git rev-parse <taskhead>^`），否则 review 范围混入用户 commit。
- long-running 分支防 main 漂移：反向 merge（`git merge main` 进 feature 分支），合超集签名，用对方分支的测试当合并验收标准。
- subagent 被限流 kill 不回滚代码：先 `git status` + `cargo build` + `cargo test` 核实完整度，别假设白干。

**偶现/时序 bug**光读代码定位不了——加诊断 log 运行时取证，别静态猜根因反复改。

**uloop/CLI 驱动 PlayMode 前先 `Application.runInBackground = true`**：编辑器窗口失焦（终端拿焦点）+ Run In Background 关 = 播放器循环整个冻结——帧率 1-2、OnUpdate 时钟不走、frameCount 钉死，像极了性能崩坏/调度器坏了。先排这个再查真性能（ShowcaseRunner 已内置设置）。

**FFI panic 取证用站点标签 + 释放审计，别信 release 行号**：release dll 内联后 panic 行号不可靠。`Scene::get_live`（全库 21 处 live 查取带函数名站点标签，`模块/函数` 格式、勿用行号——行号随编辑漂移会把取证指向错误位置）+ `Scene::free_log`（最近 32 笔释放审计：死 id 距今几笔、走没走漏斗）已常驻——「快照后死亡」类 panic（如 rematch live node）一行日志定位。Rust 压测复现不了时先算量级差（用户几分钟 60fps churn ≈ 上万次 vs 压测几百次）。

**uloop 取证自相矛盾（截图正常但探针读空 / 同会话数据反复打架）先查编辑器会话状态**：① `tasklist | grep Unity.exe` 数实例——launch 超时会再起一个，命令轮流命中不同实例；② PlayMode 期间触发过编译（domain reload）会把原生 stage 打裂（渲染正常但 DumpScene 全零）——重启编辑器复测再怀疑产品。规矩：**PlayMode 验收期间绝不编译**。另：`File.WriteAllText` 被 uloop 安全策略拦，大 JSON 用 execute-dynamic-code 的 `return` 值带出。

**圆角/小尺寸视觉差异别信目测或视觉模型**——python 像素取样 + 数学验证（如像素中心到角圆心距离 vs 半径）；「渲染出来的圆角比预期小」先算正确视觉该是什么样再下结论。

**改 parse-time 逻辑必重打 pkg**：`Node.base_style` 是打包期产物。改 cascade/mapping/parse 只重编 .dll 不够，须 `cargo run -p loomgui_pkg -- build <ws>` 重打 pkg。纯 runtime 改 .dll 即可。

csbindgen 不为 `#[repr(C)]` struct 生成 C# stub，须手补 C# 镜像文件。

**C# 投影层 `throw NE()` 是 stub，非真限制**：`LoomGUI.Nodes.cs` 里 get/set 都 `throw NotImplementedException()` 的 API（`Image.Src`/`Touchable`/`Focusable`/`OnUpdate`…）是**未接线 stub**，底层 core + FFI 多半已支持。遇注释/demo 写"运行时不可变 / by-design"先查 `crates/ffi/src/lib.rs` + core 源码确认，别信注释（例：背包图标"不可变"实为 C# 没接 `set_src`，core+FFI+Unity MirrorPool 全通）。判断框架能力看 core+FFI，不看 C# wrapper。

## API 适配方法论

- **plan/草稿的 API 常与 crate/Unity 实际不符**——遇编译错按实际源码调，勿硬改依赖版本。Rust crate 差异速查 `docs/pitfalls.md` §1；Unity API 查安装目录 `Editor/Data/Managed/UnityEditor.xml`。
- **Unity Mono 的 API surface 落后于 .NET**——headless `dotnet test`（net10.0）编过的版本门控 API 在 Unity Mono 可能 CS0117。改 C# 别只跑 headless；优先版本无关写法（指针重解释 `*(uint*)&v`、手动位运算）。
- **FFI 边界**：C-like enum 必须 `#[repr(uN)]`；ABI struct 永远 `size_of::<T>()` 断言；返字符串一律 ptr+len（不靠 NUL）。
- **移植 fgui 算法**：带数字后缀的变量名不能望文生义——按源码逐行 trace 验。

## 踩坑记录

`docs/pitfalls.md` = 精炼规则手册（依赖适配 / 跨层闭环 / Unity 平台 / 动态契约，按主题归位）。新坑只在「可复用规则」级才进；bug 编年史不记——代码 + git history 是载体。
