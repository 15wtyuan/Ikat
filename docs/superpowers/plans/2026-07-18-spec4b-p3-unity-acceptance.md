# Spec-4b P3：Unity 真机验收（终点线2）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. **P3.1-P3.3 编码机可做（素材准备），P3.4-P3.5 家里机（Unity PlayMode）。**

**Goal:** 手写最小验收页 → 打 pkg v19 → Unity PlayMode 跑通 §5 四条 done 判据。新范式端到端在真引擎可用 = 摸黑结束（终点线2）。

**Architecture:** 最小页只含 div/button/img/text + flex + cascade + class（剥 @keyframes/transition/progress/input/hover transform）。ASCII 文本（避开 CJK v1.6 待查项）。经 P2 的 LoomHost + UnityLoomBackend 驱动，UIContext 投影层暴露 Get<Button>/Clicked/Geometry。

**Tech Stack:** HTML/CSS（围栏子集）+ loom-pkg（打包 v19）+ Unity PlayMode（家里机）。

**对照 spec：** `docs/superpowers/specs/2026-07-18-spec4b-unity-acceptance-and-backend-retirement-design.md` §5。

**前置：** P1（清理 + pkg v19）+ P2（LoomHost + UnityLoomBackend + Driver + deferred ①②）完成。dll synced。

## Global Constraints

- 最小页**不含**：@keyframes / animation / transition / progress / input / data-controller / hover transform / filter（纯静态 cascade + flex）。
- 文本 ASCII（英文标签），注册一个 ASCII 字体。CJK 留 v1.6 待查项。
- 两台机：P3.1-P3.3 编码机（素材 + pkg），P3.4-P3.5 家里机（PlayMode）。编码机把 pkg.bin + dll + 字体 + 场景配置 commit，家里机 pull 后跑 PlayMode。
- 用户只读中文；代码/commit 英文。

---

## Task 1: 建最小验收页 workspace + HTML（编码机）

**Files:**
- Create: `spec4b-acceptance/loom.workspace.json`
- Create: `spec4b-acceptance/spec4b-acceptance.html`
- Create: `spec4b-acceptance/res/icons/item-wand.png` + `item-chest.png`（复用 showcase res，或纯色占位）

**Interfaces:** 产 pkg.bin（Task 2）。

- [ ] **Step 1: 建 workspace 目录 + loom.workspace.json**

`spec4b-acceptance/loom.workspace.json`（仿 showcase 结构，具体 schema 以 loom-pkg 实际为准）：
```json
{
  "name": "spec4b-acceptance",
  "components": ["spec4b-acceptance"]
}
```
（实现时对照 `showcase/loom.workspace.json` 的实际 schema 调整。）

- [ ] **Step 2: 写最小验收页 HTML**

`spec4b-acceptance/spec4b-acceptance.html`：
```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Spec-4b Acceptance</title>
  <style>
  .root { display:flex; flex-direction:column; width:1280px; height:720px; background-color:#1a1a2e; }
  .header { display:flex; flex-direction:row; align-items:center; gap:12px; padding:16px 24px; background-color:#16213e; }
  .header-title { color:#e0e6ec; font-size:22px; font-weight:700; }
  .btn-back { color:#8ec5d8; font-size:14px; }
  .body { display:flex; flex-direction:column; flex-grow:1; padding:24px; gap:16px; }
  .page-title { color:#e0e6ec; font-size:20px; font-weight:700; }
  .grid { display:flex; flex-direction:row; flex-wrap:wrap; gap:16px; }
  .card { display:flex; flex-direction:row; align-items:center; width:300px; background-color:#0f3460; padding:16px; gap:12px; }
  .card-img { width:48px; height:48px; background-color:#533483; }
  .card-text { color:#e0e6ec; font-size:15px; flex-grow:1; }
  .btn-buy { background-color:#e94560; color:#ffffff; font-size:13px; padding:6px 14px; }
  .card.highlight .card-text { color:#e94560; }
  </style>
</head>
<body>
  <div class="root">
    <div class="header">
      <button class="btn-back" id="btn-back">Back</button>
      <span class="header-title">Shop</span>
    </div>
    <div class="body">
      <p class="page-title">Products</p>
      <div class="grid">
        <div class="card" id="card-1">
          <img class="card-img" src="res/icons/item-wand.png" alt="item">
          <span class="card-text">Wand</span>
          <button class="btn-buy">Buy</button>
        </div>
        <div class="card highlight" id="card-2">
          <img class="card-img" src="res/icons/item-chest.png" alt="item">
          <span class="card-text">Chest</span>
          <button class="btn-buy">Buy</button>
        </div>
      </div>
    </div>
  </div>
</body>
</html>
```

验收点对齐 §5 四条：
- div flex（column/row）+ p/span text + img + button（渲染门）
- `id="btn-back/card-1/card-2"`（Get<T> 作用域查找门）
- `class="card highlight"`（class 命中 computed style 门——card-2 的 card-text 应是 #e94560）
- button（Clicked 事件门）

- [ ] **Step 3: 准备 res 图标**

复用 `showcase/res/icons/item-wand.png` + `item-chest.png` 拷到 `spec4b-acceptance/res/icons/`。或纯色 PNG 占位（48×48）。

- [ ] **Step 4: Commit**

```bash
git add spec4b-acceptance/
git commit -m "feat(spec4b): minimal acceptance page (div/button/img/text + flex + cascade, ASCII, no animation)"
```

---

## Task 2: 打 pkg v19（编码机）

**Files:**
- Generate: `spec4b-acceptance/out/spec4b-acceptance.pkg.bin`（v19）

- [ ] **Step 1: 打包**

Run: `cargo run -p loomgui_pkg -- build spec4b-acceptance`
Expected: 产 `spec4b-acceptance/out/spec4b-acceptance.pkg.bin` + atlas。无 fence 报错（最小页用围栏子集，应通过）。

- [ ] **Step 2: 验证 pkg v19**

写一次性 dump 脚本或用现有 dump example 读 pkg header：
```bash
cargo run -p loomgui_core --example dump_pkg -- spec4b-acceptance/out/spec4b-acceptance.pkg.bin 2>/dev/null
```
（若无 dump_pkg example，临时加一个读 PKG_MAGIC + version 断言 = 19。或 core 已有 pkg 读取测试工具。）
Expected: version=19。

- [ ] **Step 3: Commit pkg.bin**

```bash
git add spec4b-acceptance/out/spec4b-acceptance.pkg.bin spec4b-acceptance/out/atlas/
git commit -m "chore(spec4b): build minimal acceptance pkg v19 + atlas"
```

---

## Task 3: 配 Unity 场景 + 字体 + runtime.json（编码机准备）

**Files:**
- Modify: `unity/showcase-unity/Assets/Bundles/ui/spec4b-acceptance.pkg.bin`（拷 pkg）
- Modify: `unity/showcase-unity/Assets/Bundles/atlas/`（拷 atlas）
- Modify: `unity/showcase-unity/Assets/Bundles/fonts/`（ASCII 字体）
- Modify: `unity/showcase-unity/Assets/Bundles/loom.runtime.json`（指向 spec4b-acceptance 包 + 字体）
- Modify: `unity/showcase-unity/Assets/Scenes/`（建 Spec4bAcceptance 场景 或 改 SampleScene）

- [ ] **Step 1: 拷 pkg + atlas 到 Bundles**

`cp spec4b-acceptance/out/spec4b-acceptance.pkg.bin unity/showcase-unity/Assets/Bundles/ui/`
`cp -r spec4b-acceptance/out/atlas/* unity/showcase-unity/Assets/Bundles/atlas/`

- [ ] **Step 2: 准备 ASCII 字体**

放一个 ASCII 字体到 `unity/showcase-unity/Assets/Bundles/fonts/`（如 `Inter-Regular.ttf.bytes` 或复用项目已有英文字体）。ASCII 字符覆盖最小页文字（Back/Shop/Products/Wand/Chest/Buy）。

- [ ] **Step 3: 改 loom.runtime.json**

`unity/showcase-unity/Assets/Bundles/loom.runtime.json`：
```json
{
  "packages": ["spec4b-acceptance"],
  "atlases": ["icons"],
  "fonts": [
    { "family": "default", "file": "Inter-Regular.ttf.bytes", "default": true }
  ]
}
```
（schema 以 RuntimeManifest.ParseRuntime 实际为准——对照 LoomStageDriver.cs:173-179 调整字段名。）

- [ ] **Step 4: 建 Spec4bAcceptance 场景（或清 SampleScene）**

新建 `unity/showcase-unity/Assets/Scenes/Spec4bAcceptance.unity`：
- GameObject "LoomStage" + `LoomStageDriver` 组件（P2 后用 LoomHost）
- Design Size = 1280×720（对齐最小页）
- UI 相机配置（LoomStageDriver 自建或指定）
- 业务脚本 `Spec4bAcceptanceRunner.cs`：Instantiate root + Get<Button>("btn-back") + On<ClickEvent> + 读 Geometry.LayoutRect + Debug.Log 断言

> ⚠️ 场景文件（.unity）是 YAML，编码机可文本编辑加 LoomStageDriver GameObject + 组件引用。但 MonoBehaviour 组件的脚本（Spec4bAcceptanceRunner）要 .cs 文件存在。runner 脚本编码机写（下一个 step）。

- [ ] **Step 5: 写 Spec4bAcceptanceRunner.cs（验收断言脚本）**

`unity/showcase-unity/Assets/Scripts/Spec4bAcceptanceRunner.cs`：
```csharp
using UnityEngine;
using LoomGUI;

public class Spec4bAcceptanceRunner : MonoBehaviour
{
    LoomStageDriver _driver;
    Container _root;
    bool _verified;

    void Start()
    {
        _driver = GetComponent<LoomStageDriver>();
        // 等 driver Awake 完成 + Instantiate
        Invoke(nameof(Boot), 0.1f);
    }

    void Boot()
    {
        var ctx = _driver.Context;  // P2 后 driver 暴露 UIContext
        _root = ctx.Instantiate("spec4b-acceptance", "spec4b-acceptance");

        // 门 2：作用域查找（Get by id）
        var backBtn = _root.Get<Button>("btn-back");
        Debug.Log($"[Spec4b] Get<Button>(btn-back) = {(backBtn != null ? "OK" : "FAIL")}");

        // 门 4：Clicked 事件
        backBtn.Clicked += e => Debug.Log("[Spec4b] btn-back Clicked fired ✓");

        // 门 3：class 命中（card-2 有 highlight → card-text color 应变）
        var card2 = _root.Get<Container>("card-2");
        // 读 card-text 子节点的 computed color（get_node_computed_style，4a 出口）
        // ... 具体：card2.Children 找 span.card-text，读 computed style color，断言 = #e94560

        // 门 1 + rect 跨层：card-1 width ≈ 300
        Invoke(nameof(VerifyRect), 0.2f);  // 等 1-2 帧 solve
    }

    void VerifyRect()
    {
        if (_verified) return;
        _verified = true;
        var card1 = _root.Get<Container>("card-1");
        var rect = card1.Geometry.LayoutRect;
        Debug.Log($"[Spec4b] card-1 LayoutRect = {rect.w}x{rect.h} (expect w≈300)");
        // 断言 rect.w ≈ 300（CSS width:300px）+ 与 4a headless 断言一致
    }
}
```
（runner 挂在 LoomStage GameObject 上，和 LoomStageDriver 同 GO。具体 API 以 P2 后 UIContext/Container 实际签名为准——Get<T>/Clicked/Geometry.LayoutRect 4a 已实现。）

- [ ] **Step 6: Commit**

```bash
git add unity/showcase-unity/Assets/Bundles/ unity/showcase-unity/Assets/Scenes/Spec4bAcceptance.unity unity/showcase-unity/Assets/Scripts/Spec4bAcceptanceRunner.cs
git commit -m "feat(spec4b): Unity acceptance scene + runner (instantiate + Get<Button> + Clicked + Geometry)"
```

---

## Task 4: PlayMode 验收 4 条（家里机）

> 家里机执行。编码机 pull 最新（含 P1+P2+P3.1-3.3）后跑。

- [ ] **Step 1: 家里机 pull + 确认 dll/pkg/字体就位**

Run: `git pull` + 确认 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（P2 重编）+ `Assets/Bundles/ui/spec4b-acceptance.pkg.bin`（v19）+ 字体在位。

- [ ] **Step 2: dll md5 一致（防 stale）**

Run: `md5sum target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`
（家里机若没 Rust 编译环境，跳过——编码机已 commit synced dll。但确认 dll 时间戳新于 pkg。）

- [ ] **Step 3: Unity 打开 + 跑 Spec4bAcceptance 场景 PlayMode**

打开 Unity → `Spec4bAcceptance` 场景 → Play。

- [ ] **Step 4: 门 1 — 渲染（人眼）**

观察 Game 视图：
- root 全屏深蓝背景（#1a1a2e）
- header 横排（Back 按钮 + "Shop" 标题）
- body 竖排（"Products" 标题 + grid 横排 2 张 card）
- card 横排（img 方块 + 文字 + Buy 按钮）
- card-2（highlight）的文字偏红（#e94560），card-1 文字浅（#e0e6ec）

✅ 布局结构对 = 渲染门过。若全黑/全白/错位 → 排查（dll stale / pkg 没加载 / 字体没注册 → Console 看错误）。

- [ ] **Step 5: 门 2 — Get<Button> 作用域查找**

Console 应有 `[Spec4b] Get<Button>(btn-back) = OK`。
✅ 找到 = 作用域查找门过。

- [ ] **Step 6: 门 4 — Clicked 事件链**

点 Game 视图的 Back 按钮（或 card 的 Buy 按钮）。Console 应有 `[Spec4b] btn-back Clicked fired ✓`。
✅ 点中触发 = typed On<T> 全链通（InputCollector → set_input → process → borrow_events → EventDemuxer → EventBus → On<ClickEvent>）。

- [ ] **Step 7: 门 3 — rect 跨层一致**

Console 应有 `[Spec4b] card-1 LayoutRect = 300x... (expect w≈300)`。
✅ rect.w ≈ 300（CSS width:300px）= 投影层 + 集成层没歪。对照 4a headless 同结构断言的 rect 一致。

- [ ] **Step 8: capture/bubble + StopPropagation（可选深验）**

runner 加一个祖先-子 click 测试（祖先 + 子都 On<ClickEvent>，子调 StopPropagation → 祖先不收）。Console 验证顺序 + 止传播。若时间紧可推后（核心是门 4）。

- [ ] **Step 9: 截图存档 + 记录**

截图 PlayMode Game 视图 + Console。记录 4 条门结果。

- [ ] **Step 10: 若有门 fail — 排查（systematic-debugging）**

任一门 fail：用 superpowers:systematic-debugging（不盲改）。常见：
- 渲染全黑：dll stale（md5 查）/ pkg 没载（Console 找 LoadPackage 错误）/ 字体没注册（文字消失）
- Clicked 不触发：输入坐标映射（safeArea）/ EventDemuxer 接线（P2 host.Step 内 Pump）
- rect 错：Geometry 直读 FFI（4a）/ 滞后一帧（多等 1 帧）

---

## Task 5: 终点线2 完成 + 收尾

- [ ] **Step 1: 4 条门全绿确认**

✅ 渲染 / Get<Button> / Clicked / rect 跨层 — 全过。

- [ ] **Step 2: 更新 roadmap（摸黑结束）**

`docs/roadmap/roadmap.md` §2 「🏁 终点线2」+ §8 决策记录：加 Spec-4b DONE 记录（LoomStage 退役 + 多引擎分层 + 残留清理 + 终点线2 Unity 真机通）。摸黑结束。

- [ ] **Step 3: 更新 main-design（LoomHost 落地）**

`docs/design/main-design.md`：若有 LoomStage 作为集成层门面的措辞，降级为 LoomHost/LoomStageDriver（§3 落地后）。补 LoomHost/LoomBackend/UnityLoomBackend 分层图。

- [ ] **Step 4: spec §3.2 签名精化同步**

spec `2026-07-18-spec4b-...md` §3.2 的 `SyncFrame(IntPtr framePtr, int frameLen)` 改为 `SyncFrame(IntPtr stage, IntPtr framePtr, int frameLen)`（P2 plan Task 1 已精化，spec 对齐）。

- [ ] **Step 5: session 沉淀（踩坑/CLAUDE.md）**

用 session-summary skill 把 4b 经验沉淀（多引擎分层接缝、LoomStage 退役坑、PlayMode 验收要点）。

- [ ] **Step 6: 最终 commit**

```bash
git add docs/roadmap/roadmap.md docs/design/main-design.md docs/superpowers/specs/2026-07-18-spec4b-*.md
git commit -m "docs: Spec-4b DONE — backend retired + multi-engine layering + Unity acceptance green (摸黑结束)"
```

---

## P3 完成标准（= 4b 完成 = 摸黑结束）

- ✅ 最小页 pkg v19 在 Unity PlayMode 渲染正确（div/button/img/text + flex + cascade）
- ✅ Get<Button>("id") 作用域查找 + Clicked typed 事件全链通
- ✅ rect 跨层一致（Unity Geometry vs CSS 期望 + 4a headless）
- ✅ LoomHost/LoomBackend/UnityLoomBackend 在真引擎跑通（多引擎分层落地）
- ✅ roadmap/main-design 更新

**摸黑结束。下一阶段：roadmap §4 三束加宽（控件束 / 复合束 / 视觉特效束）。**
