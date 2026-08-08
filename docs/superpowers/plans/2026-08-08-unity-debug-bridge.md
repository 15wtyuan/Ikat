# Unity 调试桥 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 AI（pi）通过 unity-cli-loop 与 showcase-unity 的 Unity 运行时交互——发 C# 代码、取证 LoomGUI 节点树/MirrorPool 状态、驱动 PlayMode 调试闭环。

**Architecture:** 采用 hatayama/unity-cli-loop（Node CLI + Unity TCP 包）做核心桥，不自建。LoomGUI 侧只加两个薄 public 诊断方法（`LoomHost.DumpSceneJson()` / `LoomStageDriver.DumpMirrorPoolState()`，包内，复用已有 FFI/backend dump），加一个 showcase 侧 Editor-only 静态 `LoomBridge` 编排类（被 `execute-dynamic-code` 调）。零新 FFI、零 Rust 改动、零 .dll 重编。

**Tech Stack:** Unity 6 (6000.5.0f1) / C# / unity-cli-loop (`uloop` CLI + `io.github.hatayama.uloopmcp` UPM 包) / 现有 `loomgui_stage_dump_scene` FFI。

## Global Constraints

- **编码机**：Windows 本机（仓库 AGENTS.md 约定）。所有 Unity/`uloop` 步骤需 showcase-unity 这个 Unity 工程处于打开状态（TCP server 随 Unity 启停）。
- **不改 Rust、不加新 FFI、不重编 .dll**（spec §9 不变量）。本计划只动 C#（LoomGUI 包 Runtime + showcase Assets/Editor）+ 配置文件。
- **pi 调用约定**：从仓库根 `E:/workspace/LoomGUI` 跑，**每条 `uloop` 命令必须带 `--project-path unity/showcase-unity`**（cwd 是仓库根，省略会探测失败）。
- **Domain Reload 坑**：`uloop compile` 后 Unity 域重载，C# TCP server 强制断开几秒，期间连接必失败——pi 要重试/等待，勿立即判失败。
- **不进发布包**：`LoomBridge` 编排类放 showcase-unity（dev-only）；诊断方法挂 LoomGUI.Runtime 但属调试 FFI 的薄 wrapper，不进冻结公共签名 `LoomGUI.*.cs`。
- **v1 范围**：仅 `DumpScene` + `DumpMirrorPool`。`FindControl`/`DumpNode` 按 spec §6 defer（AI 可 grep DumpScene 文本或 execute-dynamic-code 直写 C# 顶替）。

## File Structure

| 文件 | 责任 | 新/改 |
|---|---|---|
| `unity/showcase-unity/Packages/manifest.json` | 加 unity-cli-loop UPM 依赖 | 改 |
| `unity/showcase-unity/.gitignore` | 忽略 `.uloop/` 运行产物 | 改/建 |
| `unity/package/Runtime/Host/LoomHost.cs` | 加 `DumpSceneJson()` public 诊断方法 | 改 |
| `unity/package/Tests/LoomHostDumpTests.cs` | `DumpSceneJson()` marshalling EditMode 测试 | 新 |
| `unity/package/Runtime/LoomStageDriver.cs` | 加 `DumpMirrorPoolState()` public forwarder | 改 |
| `unity/showcase-unity/Assets/Editor/LoomBridge/LoomBridge.asmdef` | showcase Editor-only asmdef，引用 LoomGUI.Runtime | 新 |
| `unity/showcase-unity/Assets/Editor/LoomBridge/LoomBridge.cs` | 静态编排类（DumpScene/DumpMirrorPool） | 新 |
| `AGENTS.md` | 桥使用手册（启动/调用约定/坑/LoomBridge 速查/调试闭环模板） | 改 |

---

## Task 1: 安装 unity-cli-loop + uloop CLI，验证端到端 eval

**Files:**
- Modify: `unity/showcase-unity/Packages/manifest.json`
- Create/Modify: `unity/showcase-unity/.gitignore`

**Interfaces:**
- Consumes: 无
- Produces: showcase-unity 工程内可用的 unity-cli-loop 包 + 全局 `uloop` CLI（后续所有 Task 的验证依赖此）

- [ ] **Step 1: 加 UPM 依赖到 manifest.json**

在 `unity/showcase-unity/Packages/manifest.json` 的 `"dependencies"` 对象里加一行（git URL 包，不需 scoped registry）：

```json
"io.github.hatayama.uloopmcp": "https://github.com/hatayama/unity-cli-loop.git?path=/Packages/src"
```

- [ ] **Step 2: 装 uloop CLI（全局）**

Run:
```bash
npm install -g uloop-cli
```
验证：`uloop --version`（或 `uloop --help`）能打印版本/帮助。

- [ ] **Step 3: 加 .gitignore 忽略 .uloop 运行产物**

在 `unity/showcase-unity/.gitignore`（不存在则建）加：

```gitignore
# unity-cli-loop 运行产物（CLI 缓存/工具注册/输出）。团队共享配置可按需取消注释保留。
/.uloop/
```

- [ ] **Step 4: 让 Unity 解析包 + 启动 TCP server（需 Unity 打开）**

打开 showcase-unity 工程（已开则切回前台触发刷新）。Unity 自动 fetch git URL 包并编译。观察 Console 无编译错。`Window > Unity CLI Loop > Settings` 确认 CLI 按钮蓝色（已连）。

> 注：本步需 Unity GUI 在场，是人工/在场步骤。AI 无法替你点 Unity。

- [ ] **Step 5: 端到端验证 eval 通**

Run（从仓库根）:
```bash
uloop execute-dynamic-code --code "return 1+1;" --project-path unity/showcase-unity
```
> 若 `--code` 不是正确 flag，先 `uloop execute-dynamic-code --help` 看参数名（README 用 tool-call 风格 `execute-dynamic-code (Code: "...")`）。

Expected: 返回结果包含 `2`（成功求值）。若报连接失败，确认 Unity showcase-unity 已打开且 Settings 窗口 CLI 已连。

- [ ] **Step 6: Commit**

```bash
git add unity/showcase-unity/Packages/manifest.json unity/showcase-unity/.gitignore
git commit -m "feat(bridge): install unity-cli-loop into showcase-unity"
```

---

## Task 2: 包内诊断访问器 `LoomHost.DumpSceneJson()` + `LoomStageDriver.DumpMirrorPoolState()`

**Files:**
- Modify: `unity/package/Runtime/Host/LoomHost.cs`
- Modify: `unity/package/Runtime/LoomStageDriver.cs`
- Create: `unity/package/Tests/LoomHostDumpTests.cs`

**Interfaces:**
- Consumes: 已有 FFI `Native.loomgui_stage_dump_scene(StageHandle*, nuint*)`（internal，LoomHost 经 InternalsVisibleTo 可调）；已有 `LoomStageDriver._backend.DumpMirrorState()`（F8 诊断已用）。
- Produces:
  - `LoomHost.DumpSceneJson() : string`——整树 JSON（node_id/parent/tag/id/classes/kind/layout/world_matrix/anim/visible）；未 instantiate 返 `"[]"`。
  - `LoomStageDriver.DumpMirrorPoolState() : string`——MirrorPool 状态文本（forward backend.DumpMirrorState）。

- [ ] **Step 1: 写失败测试（DumpSceneJson marshalling + 空场景 "[]" 分支）**

Create `unity/package/Tests/LoomHostDumpTests.cs`：

```csharp
using LoomGUI.Host;
using NUnit.Framework;

namespace LoomGUI.Tests
{
    public class LoomHostDumpTests
    {
        [Test]
        public void DumpSceneJson_Uninstantiated_ReturnsEmptyArray()
        {
            // 构造即建 stage（loomgui_stage_new），但未 load_package/instantiate → core scene=None → FFI 返 "[]"。
            using var host = new LoomHost(1920f, 1080f);
            string json = host.DumpSceneJson();
            Assert.AreEqual("[]", json);
        }

        [Test]
        public void DumpSceneJson_Disposed_ReturnsEmpty()
        {
            var host = new LoomHost(800f, 600f);
            host.Dispose();
            // Dispose 后 _stage=null，DumpSceneJson 必须早返不 deref 已释放指针。
            Assert.AreEqual("[]", host.DumpSceneJson());
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败（方法未定义）**

在 Unity Test Runner（或 `uloop run-tests --project-path unity/showcase-unity --filter LoomHostDumpTests`）跑 `LoomHostDumpTests`。
Expected: 编译失败——`DumpSceneJson` 未定义。

- [ ] **Step 3: 实现 `LoomHost.DumpSceneJson()`**

在 `unity/package/Runtime/Host/LoomHost.cs` 的 `LoomHost` 类内（`Dispose` 附近）加：

```csharp
/// dump 整树 JSON（调 loomgui_stage_dump_scene，UTF-8 marshal）。Rust 侧拥有 C 串，下 tick 失效——立即消费。
/// 未 instantiate（scene=None）/ 已 Dispose → 返 "[]"。dev 调试桥用，非冻结公共签名。
public string DumpSceneJson()
{
    if (_stage == null) return "[]";
    nuint len = 0;
    byte* ptr = Native.loomgui_stage_dump_scene(_stage, &len);
    if (ptr == null || len == 0) return "[]";
    int n = (int)len;
    var buf = new byte[n];
    Marshal.Copy((IntPtr)ptr, buf, 0, n);
    // FFI 的 out_len 含尾部 NUL（as_bytes_with_nul）；去掉避免 JSON 末尾多 \0。
    if (n > 0 && buf[n - 1] == 0) n--;
    return System.Text.Encoding.UTF8.GetString(buf, 0, n);
}
```

确认文件顶部有 `using System.Runtime.InteropServices;`（Marshal）。若无，加。

- [ ] **Step 4: 跑测试确认通过**

Run（Unity Test Runner 或 `uloop run-tests`）：`LoomHostDumpTests`。
Expected: 2 个测试 PASS。

- [ ] **Step 5: 实现 `LoomStageDriver.DumpMirrorPoolState()` forwarder**

在 `unity/package/Runtime/LoomStageDriver.cs` 的 `LoomStageDriver` 类内（`DumpDiagnostic` 附近）加：

```csharp
/// dev 调试桥用：返回 MirrorPool 状态文本（转发 backend.DumpMirrorState，同 F8 诊断）。
/// PlayMode 下有活跃 backend；无 driver/backend 时返提示串。
public string DumpMirrorPoolState() => _backend != null ? _backend.DumpMirrorState() : "backend null";
```

> 确认 `_backend` 字段名与 `DumpMirrorState()` 方法签名与现有 `DumpDiagnostic()` 调用一致（`DumpDiagnostic` 已 `_backend.DumpMirrorState()`，照抄即可）。

- [ ] **Step 6: 冒烟（MirrorPoolState 非空）——PlayMode 在场**

启动 showcase-unity PlayMode（`uloop control-play-mode --action Play --project-path unity/showcase-unity`），再：
```bash
uloop execute-dynamic-code --code "var d = UnityEngine.Object.FindAnyObjectByType<LoomGUI.LoomStageDriver>(); return d == null ? \"no driver\" : d.DumpMirrorPoolState();" --project-path unity/showcase-unity
```
Expected: 返回非空 MirrorPool 状态文本（含 active/parked slot 信息），非 `"backend null"`。

- [ ] **Step 7: PublicApi 门不回归**

Run:
```bash
dotnet test tests/dotnet/LoomGUI.PublicApi
```
Expected: PASS（新增方法在 `LoomHost.cs`/`LoomStageDriver.cs`，不在冻结面 `Runtime/Public/LoomGUI.*.cs`，不应触发门）。

- [ ] **Step 8: Commit**

```bash
git add unity/package/Runtime/Host/LoomHost.cs unity/package/Runtime/LoomStageDriver.cs unity/package/Tests/LoomHostDumpTests.cs
git commit -m "feat(runtime): diagnostic accessors DumpSceneJson + DumpMirrorPoolState for debug bridge"
```

---

## Task 3: showcase 侧 `LoomBridge` 编排类 + 冒烟

**Files:**
- Create: `unity/showcase-unity/Assets/Editor/LoomBridge/LoomBridge.asmdef`
- Create: `unity/showcase-unity/Assets/Editor/LoomBridge/LoomBridge.cs`

**Interfaces:**
- Consumes: Task 2 的 `LoomStageDriver.Host.DumpSceneJson()`、`LoomStageDriver.DumpMirrorPoolState()`。
- Produces: `Showcase.LoomBridge.LoomBridge` 静态类——`DumpScene() : string`、`DumpMirrorPool() : string`，被 `uloop execute-dynamic-code` 直接调。

- [ ] **Step 1: 建 asmdef**

Create `unity/showcase-unity/Assets/Editor/LoomBridge/LoomBridge.asmdef`：

```json
{
    "name": "Showcase.LoomBridge",
    "rootNamespace": "Showcase.LoomBridge",
    "references": ["LoomGUI.Runtime"],
    "includePlatforms": ["Editor"],
    "excludePlatforms": [],
    "allowUnsafeCode": false,
    "autoReferenced": true
}
```

> 仅引用 `LoomGUI.Runtime`，**不需 unsafe、不引用 unity-cli-loop**（是被 eval 调的纯静态类，不是 unity-cli-loop 自定义工具——后者留 P5）。

- [ ] **Step 2: 实现 `LoomBridge` 静态编排类**

Create `unity/showcase-unity/Assets/Editor/LoomBridge/LoomBridge.cs`：

```csharp
using LoomGUI;            // LoomStageDriver
using UnityEngine;

namespace Showcase.LoomBridge
{
    /// dev 调试桥 helper：被 unity-cli-loop 的 execute-dynamic-code 调用，编排 Task 2 的包内诊断方法。
    /// PlayMode-only（EditMode 无活 LoomStageDriver）。
    public static class LoomBridge
    {
        static LoomStageDriver FindDriver()
            => Object.FindAnyObjectByType<LoomStageDriver>();

        /// 整树 JSON：node_id/parent/tag/id/classes/kind/layout{x,y,w,h}/world_matrix/anim/visible。
        public static string DumpScene()
        {
            var driver = FindDriver();
            if (driver == null || driver.Host == null) return "no active LoomStageDriver (PlayMode?)";
            return driver.Host.DumpSceneJson();
        }

        /// MirrorPool 状态文本：active/parked GO slot + reuse_key（虚拟列表/pooled-slot 调试）。
        public static string DumpMirrorPool()
        {
            var driver = FindDriver();
            if (driver == null) return "no active LoomStageDriver (PlayMode?)";
            return driver.DumpMirrorPoolState();
        }
    }
}
```

- [ ] **Step 3: Unity 编译无误**

切回 showcase-unity 让 Unity 编译新 asmdef + cs。Console 无错。

- [ ] **Step 4: 冒烟 DumpScene——返合法 JSON**

启动 PlayMode（`uloop control-play-mode --action Play --project-path unity/showcase-unity`），导航到某页后：
```bash
uloop execute-dynamic-code --code "return Showcase.LoomBridge.LoomBridge.DumpScene();" --project-path unity/showcase-unity
```
Expected: 返回 `[{"node_id":...,"parent":...,...}]` 合法 JSON 数组，含节点字段。AI 可直接 grep 此文本定位 id/class。

- [ ] **Step 5: 冒烟 DumpMirrorPool——返 MirrorPool 文本**

```bash
uloop execute-dynamic-code --code "return Showcase.LoomBridge.LoomBridge.DumpMirrorPool();" --project-path unity/showcase-unity
```
Expected: 返回 MirrorPool.DumpState() 文本（含 active/parked slot）。

- [ ] **Step 6: Commit**

```bash
git add unity/showcase-unity/Assets/Editor/LoomBridge/
git commit -m "feat(bridge): Showcase LoomBridge helper (DumpScene/DumpMirrorPool) for execute-dynamic-code"
```

---

## Task 4: AGENTS.md 桥使用手册

**Files:**
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: Task 1–3 成果（uloop + LoomBridge）。
- Produces: pi 无需口头提示即可驱动调试闭环的文档。

- [ ] **Step 1: 在 AGENTS.md 加「Unity 调试桥」节**

在 `AGENTS.md` 合适位置（如「调试技巧」节附近）加新小节，内容要点（按此写，中英按 AGENTS.md 既有风格）：

```markdown
## Unity 调试桥（unity-cli-loop + LoomBridge）

showcase-unity 装了 unity-cli-loop（TCP），pi 经 `uloop` CLI 从仓库根驱动 Unity 运行时，
动态取证 LoomGUI 状态，免重编/手点 PlayMode。设计见
`docs/superpowers/specs/2026-08-08-unity-debug-bridge-design.md`。

**前置**：showcase-unity 这个 Unity 工程必须打开（TCP server 随 Unity 启停）。

**调用约定（硬规则）**：从仓库根跑，**每条 `uloop` 命令必带 `--project-path unity/showcase-unity`**。

**坑：`uloop compile` 后 Domain Reload 断连几秒**——C# TCP server 被域重载强断，期间连接必失败。
compile 后要重试/等待，勿立即判失败。

**常用命令**：
- `uloop compile --project-path unity/showcase-unity` — 编译（后断连几秒）
- `uloop control-play-mode --action Play|Stop|Pause --project-path unity/showcase-unity`
- `uloop screenshot --window Game --project-path unity/showcase-unity`
- `uloop execute-dynamic-code --code "..." --project-path unity/showcase-unity` — 任意 C#
- `uloop simulate-mouse-ui --action Click --x <i> --y <i> --project-path unity/showcase-unity`
  （坐标取自 `screenshot --capture-mode rendering` 的标注）

**LoomBridge 速查**（showcase 侧 helper，execute-dynamic-code 内调）：
- `Showcase.LoomBridge.LoomBridge.DumpScene()` — 整树 JSON（node_id/tag/id/classes/kind/layout/world_matrix/anim）
- `Showcase.LoomBridge.LoomBridge.DumpMirrorPool()` — MirrorPool active/parked slot（虚拟列表调试）
- 找控件：grep DumpScene 文本按 id/class 定位；改状态直接写 C# 调公共 Node API。
- 包内还有 `LoomHost.DumpSceneJson()` / `LoomStageDriver.DumpMirrorPoolState()`（F8 诊断同源）。

**调试闭环模板**：
compile（等断连恢复）→ control-play-mode Play → screenshot 看画面 →
DumpScene 取证节点树 → 若虚拟列表问题 DumpMirrorPool → simulate-mouse-ui 点 → DumpScene 再取证 → 断言。
```

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs(agents): Unity debug bridge usage (unity-cli-loop + LoomBridge)"
```

---

## Out of Scope（spec §6 defer，本计划不做）

- **`FindControl(role|id|class)` / `DumpNode(id)`**：AI 直接 grep `DumpScene()` 文本定位，或 `execute-dynamic-code` 内写 C# 调公共 Node API。高频再升级为 helper。
- **Render-tree dump / text-glyph metrics dump**：需新 FFI，单独立项。
- **unity-cli-loop 自定义工具提升（`[McpTool]` + SKILL.md 自动发现）**：P5 按需，先把静态 helper + AGENTS.md 用顺。
- **独立 player build（IL2CPP）支持**：out of scope。

## Self-Review Notes

- **Spec coverage**：spec §3–§5 由 Task 1（接入）+ Task 2（包诊断方法）+ Task 3（LoomBridge）+ Task 4（AGENTS.md）覆盖；spec §4.3 的 `--project-path`/Domain Reload 写进 Task 4 文档；spec §7 测试由 Task 2 EditMode 测 + 各 Task 冒烟覆盖；spec §6 defer 见 Out of Scope。
- **机制修正（已同步 spec §5.2/§5.3）**：原 spec 的 `GetDiagnosticStagePtr()` 不可行（Native internal + 无 IVT 给 showcase 程序集），改为把 FFI wrapper 挂 `LoomHost`、MirrorPool 走 driver forwarder——LoomBridge 只调 public 方法，不用 unsafe/IVT。架构（adopt + 薄层 + 零新 FFI）不变。
- **类型一致**：`DumpSceneJson() : string`（Task 2 定义）→ `LoomBridge.DumpScene()` 调它（Task 3）；`DumpMirrorPoolState() : string`（Task 2）→ `LoomBridge.DumpMirrorPool()` 调它（Task 3）。命名一致。
