# Unity 调试桥设计（adopt unity-cli-loop + LoomGUI 自定义层）

- **日期**：2026-08-08
- **状态**：已定稿，待 writing-plans 出实现计划
- **范围**：让 AI 在无人干预下与 Unity 运行时（及编辑器）交互——发代码、跑、取证状态、驱动 PlayMode，用于动态调试 LoomGUI 运行时

## 1. 背景与目标

调试 Unity 运行时（尤其 LoomGUI 的 PlayMode 行为）成本高：每次改动要重编、手点 PlayMode、目测。

**目标**：一个随 Unity 启动自动就绪的服务（unity-cli-loop 包内 TCP server 随 Unity 启停，非 MonoBehaviour 挂载），AI 能轻易连上，发一段代码由 Unity 运行时执行并返回值，从而：

- 动态确认运行时状态（节点树、布局、样式、虚拟列表 slot、事件）
- 控制编辑器（编译、播放、截图、模拟输入）
- 终极：无人控制下，AI 自主驱动「编译 → 播放 → 截图 → 探状态 → 模拟点击 → 断言」的调试闭环

**目标进程范围**：方案 C——服务挂在 **Editor 进程**，一个端口同时够到 `UnityEditor.*`（编辑器控制）和 PlayMode 的 `UnityEngine.*`（运行时状态）。Editor 里 PlayMode 跑在同一进程同一 Mono 运行时内。独立 player build（IL2CPP）out of scope。

## 2. 关键决策：采用 unity-cli-loop，不自建桥

### 2.1 决策

调研现成轮子后，**采用 [hatayama/unity-cli-loop](https://github.com/hatayama/unity-cli-loop)**（原 uLoopMCP，MIT，Unity 2022.3+，Node CLI，活跃维护），不自建 HTTP/Roslyn 桥。

### 2.2 调研结论（为何不自建）

| 候选 | ★ | 性质 | 结论 |
|---|---|---|---|
| **hatayama/unity-cli-loop** | 500 | Node CLI + Unity 包，TCP；标语「AI Drive Unity, Editor→Play Mode」 | **选中**。自由 C# eval + 截图 + PlayMode 输入模拟 + 编译/测试 + 录制回放，最贴合终极需求 |
| youngwoocho02/unity-cli | 310 | Go CLI + Unity HTTP 连接器 | 备选。HTTP + 自由 eval，最小，但缺 PlayMode 输入模拟/测试 |
| CoplayDev/unity-mcp | 13k | MCP 协议，47 工具，需 Python | 标准 MCP，但离散工具（非自由 REPL）+ Python 依赖，对 pi 走 bash 反而多一层仪式 |
| 自建（Roslyn CSharpScript / xLua / Jint） | — | — | **否决**。重造活跃维护的轮子，且背 Roslyn-on-Unity 集成风险 |

自建路径的否决理由：核心桥（HTTP/TCP + 主线程 marshalling + 代码 eval + 序列化 + 安全层）是通用工程，已有人维护；LoomGUI 独有的是 Node/Stage/RenderTree dump，这才是该自建的部分。~5% 代码量拿 ~95% 功能，并省掉 Roslyn 在 Unity 运行时上的程序集冲突风险（原 from-scratch 方案的 Day-0 spike 风险点）。

## 3. unity-cli-loop 已提供（不自建）

以下能力全部由 unity-cli-loop 内置，LoomGUI 自定义层不重复：

- **编辑器自动化**：`compile`、`run-tests`（EditMode/PlayMode）、`get-logs`/`clear-console`、`find-game-objects`（含 Selected 模式、组件过滤）、`get-hierarchy`（嵌套 JSON，**自动存文件省 token**，支持 PlayMode）、`focus-window`
- **截图**：`screenshot`（任意 EditorWindow；`CaptureMode:rendering` 可标注可点击坐标 / raycast-grid，配 `simulate-*` 用）
- **播放控制**：`control-play-mode`（Play/Stop/Pause）
- **自由 C# 求值**：`execute-dynamic-code`（async/await、CancellationToken、**两级安全**：L1 挡 File.Delete/网络/进程/Assembly.Load；L2 全开）
- **PlayMode 输入模拟**：`simulate-mouse-ui`（EventSystem）/ `simulate-mouse-input`（Input System）/ `simulate-keyboard` / `raycast` / `record-input` / `replay-input`

**架构关键点**：

- Node CLI（`uloop-cli`，`npm i -g`）+ Unity 包（UPM git URL），包内自动起 TCP server
- 工具跑在 **Unity 主线程**（Unity API 天然安全）——绕开自建方案里最头疼的主线程 marshalling
- 调用方式三选一：CLI（`uloop <tool>`）/ MCP / Skills。**pi 走 CLI**（pi 跑 bash 最直接）

## 4. 接入与集成

### 4.1 安装（showcase-unity 工程，dev 工具不进 LoomGUI 发布包）

1. UPM git URL 装：`https://github.com/hatayama/unity-cli-loop.git?path=/Packages/src`（scope `io.github.hatayama.uloopmcp`）
2. `npm i -g uloop-cli`（开发机已有 Node）
3. Unity `Window > Unity CLI Loop > Settings`，装 CLI + 按需装 Skills

### 4.2 配置与安全

- uLoopMCP 窗口开 **Allow Third Party Tools**（LoomGUI 自定义工具需要）
- `execute-dynamic-code` 安全级 **默认 L1（Restricted）**；查 LoomGUI 公共 API 够用，需反射私有字段时临时升 L2
- `.gitignore` 加推荐模式：忽略 `.uloop/*`，可选留 `settings.permissions.json` / `settings.tools.json` 做团队共享
- `UserSettings/UnityMcpSettings.json` 本地-only

### 4.3 pi 工作区不变，靠 `--project-path`

pi 工作区保持仓库根 `E:/workspace/LoomGUI` 不变。`uloop` CLI 全局选项 **`--project-path <path>`** 指向 showcase-unity 工程实例：

```bash
uloop compile                          --project-path unity/showcase-unity
uloop control-play-mode --action Play  --project-path unity/showcase-unity
uloop screenshot --window Game         --project-path unity/showcase-unity
uloop execute-dynamic-code --code "return LoomBridge.DumpScene();" \
                           --project-path unity/showcase-unity
```

**原理**：`--project-path` 读 `unity/showcase-unity/UserSettings/UnityMcpSettings.json` 拿端口，走 TCP 连活 Unity。**必须显式带**（pi 的 cwd 是仓库根，省略后按 cwd 自动探测会失败）。

**两个必须写进 AGENTS.md 的坑**：

1. **前置条件**：showcase-unity 这个 Unity 工程**必须开着**，TCP server 才活着（随 Unity 启停）。可 `/uloop-launch` 或手动开。
2. **`compile` 后 Domain Reload 断连几秒**：`compile` 触发 Unity 域重载，强制断开 C# TCP server，**接下来几秒连接必失败**（unavoidable）。pi 在 `compile` 之后要重试/等待，别立即判失败。

> 注：unity-cli-loop 是 **TCP**（非 youngwoocho02 的 HTTP），故**必须用 `uloop` CLI，不能裸 curl**。pi 跑 bash 无碍。

## 5. LoomGUI 自定义层（唯一要写的代码）

### 5.1 重大发现：introspection 运行时已基本全暴露

| 已有面 | 来源 | 覆盖 |
|---|---|---|
| `loomgui_stage_dump_scene` | FFI，已 C# 绑定（`LoomGUIBindings.cs:101`），产出 `loomgui_core::dump::dump_scene_json` | 整树 JSON：node_id/parent/tag/id/classes/kind/layout{x,y,w,h}/world_matrix/anim_tr/anim_op/visible |
| per-node getter | FFI，已绑定 | `get_node_layout_rect` / `get_node_world_matrix` / `get_node_kind` / `get_node_computed_style` / `get_node_visible` / `get_node_sort_key` / `get_node_scroll_pos` / `focused_node` / `find_node_by_id[_in_subtree]` |
| C# 运行时状态 | `LoomStageDriver` / `MirrorPool` / `NodeRegistry` / `StyleMirror` / `EventDemuxer` | 活跃/parked GO slot、reuse_key、样式镜像、事件 demux |

→ **不需要新 FFI、不需要重编 .dll、不需要 Rust 改动、不需要 csbindgen regen。** 自定义层是纯 C# dev 工具。

### 5.2 诊断方法挂 `LoomHost`（不暴露 ptr）

**实现现实（规划期发现，纠正原 §5.2 设想）**：FFI 绑定 `Native` 是 `internal`（`LoomGUI.Bindings` 程序集），`Plugins/LoomGUI/AssemblyInfo.cs` 只 `InternalsVisibleTo("LoomGUI.Runtime")` + `("LoomGUI.Tests")`。showcase-unity 的 `LoomBridge` 程序集两者都不是——**即便暴露 ptr 也调不到 FFI 绑定**。原设想的「`GetDiagnosticStagePtr()` + LoomBridge 直调 FFI」编译不过。

**决策（修正）**：把 FFI-wrapping 诊断方法直接挂 `LoomHost`（Runtime，本就能调 `Native`），expose 高层 public 方法：

```csharp
/// dump 整树 JSON（调 loomgui_stage_dump_scene，UTF-8 marshal）。未 instantiate 返 "[]"。
public string DumpSceneJson();
```

（`DumpNode(id)` 原拟含 computed_style/scroll_pos，但 computed_style 是 `ComputedNodeStyleRepr` repr(C) 结构，marshal 有实工作量且 v1 边际收益低——移到 §6 defer。dump_scene 已覆盖 layout/wm/kind/classes/anim。）

比原方案更干净：**不暴露 native ptr、showcase 侧不用 unsafe、不向外部程序集漏 InternalsVisibleTo**。`MirrorPool.DumpState()` 已是 public（`MirrorPool.cs:356`），`LoomBridge.DumpMirrorPool()` 直接调。代价：诊断方法进发布包 Runtime——可接受，`loomgui_stage_dump_scene` 本就是调试 FFI，`LoomHost` 已包所有 stage FFI，多一个 string wrapper 无害（不是 server/bridge，不影响冻结公共签名 `LoomGUI.*.cs`）。

### 5.3 v1 层：薄静态 `LoomBridge` helper

**放置**：`unity/showcase-unity/Assets/Editor/LoomBridge/`（dev-only，**不进 LoomGUI 发布包**；showcase `Assets/` 未被 gitignore，可提交）。asmdef 仅引用 `LoomGUI.Runtime`。**不需 `allowUnsafeCode`**——helper 只调 public string 方法。**v1 不引用 unity-cli-loop**——是被 `execute-dynamic-code` 调的纯静态类；P5 提升为自定义工具时才加该引用。

**PlayMode-only**：运行时状态仅在 PlayMode 存在（EditMode 无活 stage / driver）。编辑器级调试走 unity-cli-loop 的 compile/find-game-objects。

helper 表面（薄编排层，只调 public 方法）：

| 方法 | 实现 | 用途 |
|---|---|---|
| `LoomBridge.DumpScene()` | `FindAnyObjectByType<LoomStageDriver>().Host.DumpSceneJson()` | **主入口**，必做 |
| `LoomBridge.FindControl(...)` | 按 role / id / class 任一查找（重载），走公共 Node registry API（不直调 FFI） | 操作前定位控件 |
| `LoomBridge.DumpMirrorPool()` | 经 driver/backend 拿到 `MirrorPool` → 调已 public 的 `DumpState()` | 本仓 pooled-slot / 虚拟列表调试刚需 |

AI 在 `execute-dynamic-code` 里直接 `return LoomBridge.DumpScene();`。

### 5.4 可选升级（YAGNI 先不做）

把高频 helper 提升为 unity-cli-loop 自定义工具（`[McpTool]` 继承 `AbstractUnityTool<Schema,Response>` + `Skill/SKILL.md` 自动发现）。先用静态 helper + AGENTS.md 文档；某个用顺了再提升。

## 6. 明确 defer（真要再加 FFI 才做）

- **单节点聚焦 dump**（`DumpNode`：computed_style `ComputedNodeStyleRepr` marshal + scroll_pos）：v1 移除。急用可 `execute-dynamic-code` 走公共 Node API 查。
- **Render-tree dump**（内部 `Vec<RenderNode>`）：批合/几何诊断。需新 FFI。急用可 `execute-dynamic-code` + 反射顶上。
- **Text/glyph metrics dump**（spec4b 风格）：文本换行/字形诊断。defer。
- **独立 player build（IL2CPP）支持**：Roslyn 不能 JIT，C 方案锁定下 out of scope。
- **pi 专用 Skills 打包**：unity-cli-loop 的 Skills 装到 Claude Code/Codex 格式；pi 走 CLI 即可，pi 专用 skill 包装按需再说。

## 7. 测试

- **核心 dump**：已有单测（`crates/core/src/dump.rs`），不重复。
- **`LoomHost.DumpSceneJson()` marshalling**：包内 EditMode 测试（`LoomGUI.Tests`，有 InternalsVisibleTo）——构造 `LoomHost`（未 instantiate）断言返 `"[]"`（null/空 scene 分支）；用 `loomgui_make_test_pkg` 加载最小场景 + tick 后断言非空 JSON 含节点字段。
- **`LoomBridge`（showcase）**：纯编排，不单测；smoke：PlayMode 下 `uloop execute-dynamic-code "return LoomBridge.DumpScene();"` 返合法 JSON、`DumpMirrorPool()` 返 MirrorPool.DumpState() 文本。
- **端到端（手动/smoke）**：真起 showcase-unity PlayMode，走完整调试闭环（compile→play→screenshot→DumpScene→simulate→assert）。

## 8. 实现路线（粗粒度；细任务交 writing-plans）

| 阶段 | 内容 | 门 |
|---|---|---|
| **P0** | showcase-unity 装 unity-cli-loop + `npm i -g uloop-cli` + 开 Allow Third Party Tools + `.gitignore` | `uloop compile --project-path unity/showcase-unity` 通；能 `execute-dynamic-code "return 1+1;"` |
| **P1** | `LoomHost.DumpSceneJson()`（Runtime）+ 包内 EditMode 测 | 未 instantiate 返 `"[]"`；加载后非空含节点字段 |
| **P2** | `LoomBridge` asmdef（showcase）+ `DumpScene()` 编排 + smoke gate | `execute-dynamic-code "return LoomBridge.DumpScene();"` 返合法 JSON |
| **P3** | `FindControl` / `DumpMirrorPool`（showcase LoomBridge）+ smoke | 查找/MirrorPool 可读 |
| **P4** | AGENTS.md：桥使用手册（`--project-path`、Domain Reload 坑、LoomBridge 速查、调试闭环模板） | pi 不需口头提示即可驱动闭环 |
| **P5（按需）** | 高频 helper 提升为 unity-cli-loop 自定义工具 + SKILL.md | 自动发现 |

## 9. 不变量与风险

- **不自建桥、不加新 FFI、不改 Rust**：本设计的范围纪律。任何阶段若发现需要新 FFI，回到 brainstorming 评估（可能暗示 introspection 缺口值得单独立项）。
- **TCP 非 HTTP**：pi 必须用 `uloop` CLI；裸 curl 不可（区别于 youngwoocho02/unity-cli）。
- **Domain Reload 断连**：`compile` 后必断几秒，pi 要重试。
- **PlayMode-only 状态**：LoomBridge helpers 仅 PlayMode 有意义。
- **`execute-dynamic-code` L1 安全**：默认挡危险操作；LoomGUI 公共 API 查询不触发任何拦截。
