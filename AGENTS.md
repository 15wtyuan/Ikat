# LoomGUI 开发指南

本文件为在本仓库工作的 AI 编码代理（Claude Code / Codex 等）提供指引。

## ⚠️ 模型禁令（硬规则，违者罚钱）

**subagent 严禁使用 `netease-codemaker/*` 系列模型**（如 `netease-codemaker/claude-opus-5`、`netease-codemaker/deepseek-v4-pro`、`netease-codemaker/glm-5.2` 等）——这是**公司账号**，乱用要罚钱。dispatch 任何 subagent（Agent 工具的 `model` 参数）必须避开整个 `netease-codemaker/` 前缀，改用直连 provider 的可用模型：`DeepSeek/deepseek-v4-pro`、`DeepSeek/deepseek-v4-flash`、`Zhipu/glm-5.2` 等。撞输出上限就换 provider/换更小模型，**绝不**退回 netease-codemaker。

## 这是什么

LoomGUI = 跨引擎游戏 UI 框架。标准 HTML/CSS 子集作设计期 DSL，类型化对象树作运行时 API，自绘渲染。核心目的：**AI 驱动的界面拼装**——标准 HTML 作 DSL，让 AI 既能编辑（文本）又能预测渲染结果（AI 对 HTML/CSS 有强先验）。

对标 FairyGUI、RmlUi、Unity UI Toolkit（参考实现在 `temp/FairyGUI-unity/`、`temp/RmlUi/`，只读）。差异化：标准 HTML/CSS（vs fgui 的 `.fui` 二进制 AI 看不懂）、类型化对象树（标准 HTML 元素决定稳定类型）、Rust 跨引擎共享核心、围栏验证器。

**当前状态**：API 范式重构（摸黑 + 三束）已完工，进入「完全体」阶段（能力补全 / 验收发布 / 跨引擎）。设计契约见 `docs/design/main-design.md`；**公共 API 终态契约见 `docs/design/public-api.md` C# 投影层机制见 `docs/design/projection-layer.md`（真身在 Rust，C# 是 OOP 投影 + 攒批回写）**；重构路线见 `docs/roadmap/roadmap.md`；早期探索 spec 见 `docs/superpowers/specs/2026-07-13-api-refactor-design.md`（历史草稿，大部分已吸收进上述 design，非活契约）。公共签名冻结在 `unity/package/Runtime/Public/LoomGUI.*.cs`，编译校验门 `tests/dotnet/LoomGUI.PublicApi`。

## 构建 / 测试命令

```bash
# 核心（引擎无关纯库，可单测）
cargo build -p loomgui_core
cargo test  -p loomgui_core

# 打包器 CLI（HTML+CSS+资源 → .pkg.bin + 自绘图集 + fonts；二进制名 loom-pkg，复用核心 parse 层）
cargo build -p loomgui_pkg
cargo test  -p loomgui_pkg
# 运行：cargo run -p loomgui_pkg -- build <workspace-dir>    （loom-pkg build <workspace>）

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

### Rust → Unity .dll 闭环（Windows 本机是唯一的编码机）

按记忆/工作流：**任何** Rust 改动后必须重编 + commit `.dll`，否则测不了。

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
- **拷贝时 Unity 必须关着**（它锁 .dll）。
- **同步 C# 绑定**：build.rs 不再自动写 Unity 绑定。构建后运行 `cargo run -p xtask -- sync-bindings`，将 csbindgen 生成的 `LoomGUIBindings.cs` 同步到 `unity/package/Plugins/LoomGUI/Bindings/`。

**图集自绘**：图集由打包器自绘（`loom-pkg build` 或 GUI 产 `atlas/*.png`+`atlas/*.atlas.json`）

### GUI 打包器 exe 闭环（loomgui_gui，Tauri 2）

GUI 是 Tauri 2 桌面 app，产物 `unity/package/Editor/Tools/loomgui_gui.exe`（Unity `LoomGUI > Open Packer` 拉起）。任何 GUI 改动、围栏改动后必须重出 exe + 拷贝 + 入库：

```bash
npm install -g @tauri-apps/cli                 # 一次性装 tauri-cli（prebuilt，比 cargo install 快得多）
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)    # 出 exe（必须 tauri CLI！--no-bundle 跳 NSIS/MSI installer）
cp crates/packer/gui/src-tauri/target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```

- **前端手写 dist（无 npm 构建）**，靠 `app.withGlobalTauri:true` 注入 `window.__TAURI__`（tauri.conf.json）；缩略图靠 `app.security.assetProtocol` + Cargo `tauri` feature `protocol-asset`。

## 架构（大局——权威契约读 `docs/design/main-design.md`）

> **当前状态**：摸黑结束（Spec-4b DONE），进入三束加宽阶段。下方同时列出**新范式（目标）**和**旧范式（v1 残留）**的架构不变量。以新范式为准；触碰尚未重构的旧代码时以旧范式为准。

### 新范式（目标——权威读 main-design.md）

**分层、单向数据流、引擎对象不进核心：**
```
标准 HTML/CSS 子集（设计期 DSL）
  → 打包器（构建期；schema 驱动围栏验证）
  → Rust 核心：parse → style(cascade) → scene(类型化 Node 树)
    → layout(Block/Flex Strategy) → render(Vec<RenderNode>)
  → FFI（csbindgen：SOA 扁平数组）
  → Unity 后端（GameObject+MeshRenderer 镜像渲染树；输入采集；资源加载）
```

**关键边界**：
- **公共语义层**：类型化 Node 对象树（Node/Container/Button/Slider/...），是游戏业务程序员的唯一 API 表面。NodeId 不出现在公共 API。
- **内部行为层**：布局策略（Block/Flex）、滚动、文本排版、渲染状态。使用 Strategy/State 模式，不暴露给公共 API。
- **引擎后端**：输入采集、渲染树→原生镜像、资源加载。不解析 HTML/CSS、不算布局、不生成几何。

**新范式架构不变量**（违反 = 隐 bug）：
- **标准 HTML 语义决定类型**：节点类型由稳定 HTML 语义签名（base 标签按 tag；控件/列表按 WAI-ARIA `role` + `aria-*`）决定。CSS（class、伪类、computed style）永远不改变 C# 对象类型。
- **CSS 赋予行为能力，不改变类型**：`display:block/flex/none` 选择内部布局 Strategy；`overflow:auto/scroll` 选择滚动 Strategy。策略切换不重建节点、不丢状态。
- **组件作用域 ID 查找（L3 已完整）**：`Get<T>("id")`/`Query` 在当前组件实例内递归查找，遇 `LOOKUP_SCOPE` 边界（实例根/组件 host/List slot 根）不再下钻；边界根自身仍可被外层命中。访问嵌套作用域内部：先 Get 作用域根，再在其上 Get。
- **Custom Element 是打包期展开（编译期糖）**：hyphen 标签由打包器查 `components/` 注册表（Package 注册表 = `customElements.define` 角色，main-design §7.4）展开成 host（CustomElement kind + `custom_tag` + `component_scope`）+ 组件模板子树；`<slot>` 投影在拼接位消费，产物无 Slot 节点。未注册 / 无效 slot / 页面裸 `<slot>` / 展开环 / 同域 id 撞车 = 打包错误。host 是**硬墙作用域**：投影内容归组件域（CSS/查找/id），host 自身归页面域（`HOST_IN_PARENT_SCOPE`），组件内部选择器经 pkg v35 锚定规则按展开实例隔离。spec：`docs/superpowers/specs/2026-08-14-component-system-design.md`。
- **transform 是渲染/命中层，不进布局**：改 transform 不触发 solve，只刷新命中几何 + world_matrix。
- **tick 时序 = 显式依赖拓扑**：`process(hit 用上帧 world) → rematch → solve → refresh_content → compute_world_transforms → build`。rematch 在 solve/compute 前。
- **所有布局帧末一致**：每帧一次 solve。
- **单一动画时钟**：`TweenManager::update(dt)` 是唯一时钟。ScrollPane 物理是例外（自维护 tween）。
- **坐标系**：核心 = 左上原点、y 向下。y-flip 是后端根一次性变换。
- **NodeFlags 是交互态**（process/rematch only，solve/world/build skip）：被 solve/render 读的字段（如 `rich_text_block`）用独立 `Node` 字段，不进 NodeFlags bit。
- **公共语义树与内部渲染树可以不同**：文本在公共层是正常 HTML 子树（TextNode/TextElement），内部扁平化为 runs。

### 旧范式（v1 残留——摸黑+三束重构中逐步消除）

> 以下不变量描述的是**尚未完全重构的当前代码**。碰到下面的代码，提醒用户重构或清理。

- ~~**`<div>` 永远是 flex 容器**~~（**P1 C2 已消除**）：`display:block` 和裸 block 默认标签现在都设 `taffy_style.display = taffy::Display::Block`（真 CSS 块流，垂直堆叠且忽略子 flex-grow），不再走 flex-column 伪 block。显式 `display:flex`/`display:none` 仍覆盖。（历史上 block 默认标签含 div/header/nav/p/ul/ol/li/option；控件 role 化重构后多数下线，当前围栏里 `div` 是唯一 block 默认 runtime 标签。）两处赋值：`crates/fence/src/css_resolve.rs` 铺默认、`crates/core/src/style/mapping.rs` 应用显式声明。
- **`NodeKind` enum + 代际 NodeId**：`NodeId(pub u32)` 对外透明句柄。19 变体（控件 role 化重构后）+ C# 类型化投影层（Node/Container/Button/...）已落地，但 Rust 侧 NodeKind/NodeId 仍在核心所有热路径中活跃。→ 类型化用户表面已兑现；内部表示重构在复合束推进时逐段迁移。
- ~~**`Get<T>("id")` 子树查找（L1 已落，L3 defer）**~~（**L3 已消除**）：DFS 现按 `LOOKUP_SCOPE` 剪枝，嵌套组件/list item 不穿透；`aria-controls` 解析改 scope 内查找（多实例不串）。
- **虚拟列表 slot 模型（parked-but-attached，已落）**：slot 永驻 ul 子树，离场 `display:none` 标记（parked，不 detach 到 free 池）；reuse_key 永久 ordinal；MirrorPool parked keepalive 持久 GO 池（仅 gone 才销毁）。坑 182 已解。driver 仍管 slot 映射/可见区间/不等高补偿（核心仍不完整“认识列表”，完整吸收留复合束 ListView）。

### 围栏

面向游戏 UI 的标准 HTML 子集（14 标签 = 8 shell + 6 runtime）。控件与列表无专属标签，作者在 `<div>` 上写 WAI-ARIA `role` 表达（`role=slider`/`role=list`/...），视觉部件用 `data-slot`。围栏外输入打包期报错，不静默降级。单一真相源 = crates/fence/src/schema/ Rust const 表。防漂移门：cargo test -p loomgui_fence（含文档↔schema 交叉校验）。权威文档：docs/design/fence.md。

## 在本仓库怎么干活

- **实现任何机制前，先对照 FairyGUI 源码和 RmlUi 源码**（`temp/FairyGUI-unity/` 和 `temp/RmlUi/`，只读）。LoomGUI 的渲染/对象模型/批合/事件/动画/资源管线全面借鉴 fgui，文本/布局借鉴 RmlUi/UITK。先读对应源码看它怎么做，再定设计。
- **设计文档 vs 踩坑**：`docs/design/main-design.md`（总体架构与渲染管线）、`docs/design/fence.md`（围栏）、`docs/design/public-api.md`（公共 API 终态契约）、`docs/design/projection-layer.md`（C# 投影层机制：真身在 Rust，C# 是 OOP 投影 + 攒批回写）、`docs/roadmap/roadmap.md`（路线图：完全体 north star + tracks + 里程碑；旧纪元史见 `docs/roadmap/roadmap_old.md`）、`docs/pitfalls.md`（踩坑全库 + 依赖 API 适配）。
- **Rust edition 2021**，依赖钉版本：`taffy 0.12`、`ttf-parser 0.20`、`slotmap 1.1`、`csbindgen 1`。CSS 选择器解析器手搓（零新依赖，spike 阶段推翻了"接 cssparser"前提）。旧版 snapshot 测试用 `insta` 已移除，换自维护。
- `Cargo.lock` 入库（根级，尽管 `.gitignore` 有通用 `Cargo.lock` 行——它是被追踪的）。
- 设计师工作区是独立磁盘目录（含 `loom.workspace.json`、HTML/CSS 源文件、res 资源、design-systems 组件库）。打包用独立打包器 GUI（Tauri `loomgui_gui`）或 CLI `loom-pkg build <workspace>`。运行时引导由 `loom.runtime.json` 统管。
- 用户只读中文——问答/选项/总结用中文；代码/commit 照旧英文。
- **代码注释写上线品质**：自包含、精简（说 WHY）、不引用内部编号或暗语。坑号只属于 `docs/pitfalls.md`，不进代码。
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
- `spec4b_dump` — Spec-4b 验收用：dump 全节点 layout_rect + img src + text metrics + glyph probe（验 core solve 跟 PlayMode 一致，定位 layout bug 在 core 还是 Unity 后端）
- `dump_shop` — showcase/shop 命中/层叠诊断：dump 全节点 layout_rect/display/touchable + hit_test 探针（定位点击被谁接走、inline vs class cascade 谁赢）
- `dump_mail` — showcase/mail 帧计时 + 阶段拆分（定位低帧热点：solve/render/build 谁独占；验虚拟列表 slot 数不随 ItemCount 涨）
- `dump_mail_scroll` — 虚拟列表覆盖诊断：set_scroll_pos 驱动 + 量化视口顶部空白 gap_top（定位 active slot 是否覆盖视口顶，漏 flex gap 会留空）
- `dump_home_anim` — 入场交错动画延迟取证（验 `:nth-child` 步进延迟是否正确，漏 TextNode 计数会全 0）

**跨层特性 PlayMode 报错**先 example 实测 core 状态再改，避免盲改物理掩盖 layout 根因。

**core dump 复现 Unity solve**：PlayMode layout/视觉 bug 先编码机用 `spec4b_dump` / 对应 dump_*.rs example 喂同样的 pkg.bin 复现 core solve，定位 bug 在 core（dump 错）还是 Unity 后端（dump 对、渲染错）。core 和 Unity 是同一份 solve 的两面，dump 取证再改，别静态猜反复试。

**Unity 渲染 vs HTML 浏览器颜色对比（Chrome headless 取证）**：颜色「发白/偏亮/偏色」问题不能盲信「渲染对得上 CSS」——CSS 半透明合成在 sRGB 编码空间，Unity Linear 项目在 linear 空间（见坑 197），同一 CSS 算出不同值。取证：Chrome headless 截 HTML（`"/c/Program Files/Google/Chrome/Application/chrome.exe" --headless=new --disable-gpu --force-color-profile=srgb --force-device-scale-factor=1 --window-size=1920,1080 --screenshot=out.png "file:///abs/path.html"`）→ PowerShell `System.Drawing.Bitmap.GetPixel(x,y)` 读像素 hex → 和 Unity uloop `screenshot --capture-mode rendering` 像素逐字节对比。**控制实验先校准**：截纯色 `#hex` HTML 确认 Chrome 截图对纯色准（暗部可能偏），坐标用 PNG top-left = design 坐标。这是定位「颜色对不上」类问题的铁证方法，比静态猜强。

**围栏真相源 = `crates/fence/src/schema/` Rust const 表，`docs/design/fence.md` 是人类可读权威副本**：围栏最终形态 = schema 注册表（14 标签 = 8 shell + 6 runtime + role 驱动控件 + CSS 子集 + `@keyframes`/`animation` 终态），fence.md 是它的可读镜像（改 schema 必同步 fence.md，防漂移门 `cargo test -p loomgui_fence` 含「文档↔schema 交叉校验」测试 `doc_schema_sync.rs`）。roadmap 决策「终点线2 scope 用哪些」（`:nth-child` / 多 selector / @keyframes runtime 已交付；剩余视觉缺口见 `docs/roadmap/roadmap.md` T1）。代码往围栏最终形态靠，围栏外的 showcase bug 跟围栏最终形态（showcase 整体打包挂留专门 task）。

**GUI exe 绑 fence crate**：fence 改动后必须重编 GUI exe（`loomgui_gui.exe` 静态链入 fence），否则 GUI stale 误报围栏外（pkg bump 时也触发，坑 158 同源）。打包器 exe 闭环见上方「GUI 打包器 exe 闭环」段。

**SDD per-task review 是代码质量门，不是集成正确性门**：单测验不了 CSS 语义集成（display 子树剪枝、继承传播、多 spec 解析）——SDD 后必跑 showcase PlayMode 逐项过。**跨层缺口 per-task review 必漏**：控件束 P3 加 NumberField 时，enum/FFI/C# 各 task 都绿，但 render/measure/cursor 的 type-dispatch arm 漏了 NumberField → 控件运行时不可见（空 mesh），是 final whole-branch review 才抓到。**教训：加新控件类型（新 NodeKind/ControlState 变体）时，强制 grep 所有按 kind/变体 dispatch 的点**（render arm、measure_text_controls、on_pointer_down/on_text_pointer_down、cursor blink、FFI setter or-pattern）逐一确认覆盖，别只验自己 task 的层——各层 dispatch 独立写、独立测，漏一个 = 控件半残/不可见。

**SDD long-running worktree 要防 main 漂移**：反向 merge（`git merge main` 进 feature 分支解冲突），合超集签名，用对方分支的测试当合并验收标准。

**subagent 撞 API 限流被 kill 不回滚代码**：先 `git status` + `cargo build` + `cargo test` 核实代码完整度，别假设白干。

**SDD 模型选型（本 repo 校准）**：opus 级模型在本 repo 反复撑爆 subagent 输出上限（过度读大文件 list.rs/blob.rs 触发，单次跑 3 次撞墙），改用 deepseek 级跑多数 implementer + 几乎全部 reviewer——速度快且抓到真问题（notify 漏过滤 / DFS root-inclusive 误用 / FFI 文档漂移 / fence 副本漏 cp）。SDD 默认 `DeepSeek/deepseek-v4-pro`（直连；**禁用 netease-codemaker 系列**，见顶部模型禁令）起步，opus 级仅 escalard（撞输出上限就换模型，别硬重试）。

**SDD task 切分别太细（强耦合重构）**：删一个共享字段（如 `ListState.free`）必然牵连所有消费者（plan/execute/notify），“最小 struct only” task 的 bridge 编辑会引入回归（T1 删 free 的 bridge 导致 active slot 乱序，reviewer 抓到）。强耦合重构的 task 边界要么包含被牵连函数，要么预期 bridge 多一轮 fix。

**SDD 工作区隔离（pi harness 适配）**：pi 的 subagent 共享 controller cwd——建独立 worktree 目录会让 subagent 仍在主 checkout 编辑、路径错位（与 harness 对抗，违背 "Never fight the harness"）。用 **feature 分支在主 checkout 隔离**（commit 层面保护 main），不用独立 worktree 目录。用户可能在分支上并行 commit 非 task 工作（如 showcase）→ 每个 task 的 review BASE 必须用 **task commit 的实际 parent**（`git rev-parse <taskhead>^`），而非 dispatch 前记录的 HEAD，否则 review 范围混入用户 commit。

**偶现/时序 bug**光读代码定位不了——加诊断 log 运行时取证，别静态猜根因反复改。

**改 parse-time 逻辑必重打 pkg**：`Node.base_style` 是打包期产物。改 cascade/mapping/parse 只重编 .dll 不够，须 `cargo run -p loomgui_pkg` 重打 pkg。纯 runtime 改 .dll 即可。

csbindgen 不为 `#[repr(C)]` struct 生成 C# stub，须手补 C# 镜像文件。

**C# 投影层 `throw NE()` 是 stub，非真限制**：`LoomGUI.Nodes.cs` 里 get/set 都 `throw NotImplementedException()` 的 API（`Image.Src`/`Touchable`/`Focusable`/`OnUpdate`…）是**未接线 stub**，底层 core + FFI 多半已支持。遇注释/demo 写"运行时不可变 / by-design"先查 `crates/ffi/src/lib.rs` + core 源码确认，别信注释（坑 191：背包图标"不可变"实为 C# 没接 `set_src`，core+FFI+Unity MirrorPool 全通）。判断框架能力看 core+FFI，不看 C# wrapper。

## API 适配方法论

**plan/草稿的 API 常与 crate 实际不符**——遇编译错按 crate 实际源码调，**勿硬改依赖版本**。具体 crate 差异见 `docs/pitfalls.md` §3。

**Unity API 同理别信记忆/草稿**——查 Unity 安装目录 `Editor/Data/Managed/UnityEditor.xml`。

**Unity Mono 运行时 API surface 落后于 .NET / net10.0**——`BitConverter.SingleToUInt32Bits`、新 `Span` API 等版本门控 API，headless `dotnet test`（net10.0）能编过但 Unity Mono 缺 → Unity 编 CS0117。改 C# 别只跑 headless；优先版本无关等价写法（指针重解释 `*(uint*)&v`、手动位运算）。见坑 188。

**FFI 边界 C-like enum 必须 `#[repr(uN)]`**。永远 `size_of::<T>()` 断言 ABI struct 尺寸。

**Rust FFI 返字符串一律 ptr+len**（不靠 NUL）。

**移植 fgui 算法**：带数字后缀的变量名不能望文生义——须读源码表达式确认。算法移植按源码逐行 trace 验。

## 坑索引

完整踩坑记录见 `docs/pitfalls.md`（v1.x 坑记录，摸黑重写后部分失效）。新踩坑继续编号递增，写法：症状/根因/解决/教训。
