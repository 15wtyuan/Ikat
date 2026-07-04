# NativeHost 3D 角色 + UI 特效 demo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 showcase 新建 `page_nativehost`，把带骨骼动画的 3D 角色（Quaternius CC0 FBX）+ Kenney CC0 粒子特效挂到 NativeHost slot，压测 NativeHost-lite 在真实复杂资源下的边界，并修 `NativeHostManager` 对粒子 renderer 的遗漏。

**Architecture:** 纯后端 C# + showcase HTML/CSS（每页自带 `<style>`，D6 组件隔离）。`page_nativehost.html` 提供 `<div id="nh-stage">` 占位 + 两个按钮（放光效 / 切动画）；`LoomShowcaseDriver.cs` 加 `SubscribeNativeHost` —— 缓存一个「角色 prefab + 粒子 child」组合 GO，`BindNativeHost(nh-stage, instance)`，按钮 toggle 粒子 / 切 Animator state；`NativeHostManager.CacheRenderers` 加 `ParticleSystemRenderer` 让粒子 renderQueue 也被统一到 3000。

**Tech Stack:** Unity 6.5（URP）/ C# / HTML+CSS 围栏子集 / `cargo run -p loomgui_pkg` 重打 pkg.bin。

## Global Constraints

- **零 Rust/FFI/blob 改动**：NativeHost-lite 本就零 FFI；不重编 `.dll`，不 bump `PKG_FORMAT_VERSION`。
- **围栏**：HTML 只用 `div`/`span`/`img`/`button`；CSS 只用围栏内属性（showcase 惯例用 `<div class="btn">` 而非 `<button>`）。`cargo test -p loomgui_core fence_contract` 必须通过。
- **showcase HTML 模式**：每页独立 `.html` 自带 `<style>` 块（D6 组件隔离）；配色固定 `#1a1d2e`（root 底）/`#252839`（卡片刻）/`#5fb2c4`（强调青）。
- **角色用 FBX**（Unity 原生认，不加 glTFast 依赖）。
- **两机工作流**：当前家里机会话——Claude 执行下载 + 改代码 + 提交；用户执行 Unity 编辑器操作（Import unitypackage / 配 Animator / 配 Inspector）+ 重打 pkg.bin + PlayMode 验收。
- **C# 编译验证**：改完 `.cs` 由 Unity 编辑器下次 focus 时编译（Claude 不能直接 csc）；语法/类型对照现有代码自查，真实验证在 Task 6 PlayMode。
- **不删现有 §1.6 model-slot**（page_controls 保留作轻量对照）。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/particlePack_samples.unitypackage` | Kenney 6 个粒子 prefab（用户 Import） | Create（curl 下） |
| `loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/LICENSE.txt` | Kenney CC0 许可 | Create（zip 内提取） |
| `loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/LICENSE.txt` | Quaternius CC0 许可 | Create（手写） |
| `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs` | `CacheRenderers` 纳入 `ParticleSystemRenderer` | Modify（一行） |
| `loomgui_unity/Assets/LoomUI/showcase/page_nativehost.html` | 新 demo 页（nh-stage + 按钮） | Create |
| `loomgui_unity/Assets/LoomUI/showcase/home.html` | nav 加 `nav-nativehost` 入口 | Modify（插一张卡片） |
| `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs` | `SubscribeNativeHost` + 缓存实例 + 按钮回调 + switch case + home nav | Modify |

---

## Task 1: Kenney 粒子下载入库

**Files:**
- Create: `loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/particlePack_samples.unitypackage`
- Create: `loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/LICENSE.txt`
- Create: `loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/LICENSE.txt`

**Interfaces:**
- Produces: `particlePack_samples.unitypackage` 供 Task 6 用户 Import 得 6 个 prefab（Magic/Fire/Sparks/Smoke/Electricity/Hearts）。

- [ ] **Step 1: 下载 + 解压 Kenney zip（项目外临时目录）**

Run（Git Bash，需外网）:
```bash
TMP=$(mktemp -d)
OUT=loomgui_unity/Assets/LoomUI/res/effects/kenney_particles
mkdir -p "$OUT"
curl -L -o "$TMP/k.zip" "https://kenney.nl/media/pages/assets/particle-pack/f8fe0f8cb8-1677578741/kenney_particle-pack.zip"
unzip -o "$TMP/k.zip" -d "$TMP/x"
ls "$TMP/x"
```
Expected: 列出 `Unity samples/`、`License.txt`、PNG sprite 目录等。

- [ ] **Step 2: 提取 unitypackage + LICENSE 入库，清理临时目录**

Run:
```bash
TMP=$(ls -d /tmp/tmp.* 2>/dev/null | tail -1)   # 或用 Step 1 同一个 $TMP
OUT=loomgui_unity/Assets/LoomUI/res/effects/kenney_particles
mv "$TMP/x/Unity samples/particlePack_samples.unitypackage" "$OUT/"
mv "$TMP/x/License.txt" "$OUT/LICENSE.txt"
rm -rf "$TMP"
ls -la "$OUT"
```
Expected: `particlePack_samples.unitypackage`（~5.5 MB）+ `LICENSE.txt` 在 `kenney_particles/` 下。

- [ ] **Step 3: 建 Quaternius 角色 LICENSE 占位**

Write `loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/LICENSE.txt`:
```
Quaternius Animated Man — CC0 1.0 (Public Domain)
Source: https://quaternius.com/packs/animatedman.html
License: https://creativecommons.org/publicdomain/zero/1.0/

No attribution required. Character FBX + textures downloaded separately
(browser, Google Drive link) and placed in this folder by the user.
```

- [ ] **Step 4: 验证 gitignore 不会忽略入库文件**

Run:
```bash
git check-ignore loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/particlePack_samples.unitypackage loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/LICENSE.txt
```
Expected: 无输出（均未被忽略）。若有输出，查 `.gitignore` 是否误匹配（`*.unitypackage` 不该被忽略——LoomUI 资源白名单）。

- [ ] **Step 5: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/ loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/LICENSE.txt
git commit -m "$(cat <<'EOF'
feat(nativehost-demo): add Kenney particle pack (CC0) + Quaternius license

Download Kenney Particle Pack unitypackage (6 VFX prefabs) via curl;
add CC0 license placeholders for Kenney + Quaternius animatedman.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: NativeHostManager 纳入 ParticleSystemRenderer

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs`（`CacheRenderers` 方法，约 line 64-79）

**Interfaces:**
- Produces: `ParticleSystemRenderer` 的 material renderQueue 被 `Bind` 时设为 3000，与 `MeshRenderer`/`SkinnedMeshRenderer` 同队列——粒子 sortingOrder 跨 UI/GO 统一排序的前提。

**验证策略（务实 TDD）：** `NativeHostManager` 是 `internal sealed`（LoomStage 内部协作），项目测试惯例测 public API（见 `MaterialManagerTests.cs`）。一行编译时类型判断 + 改 sharedMaterial renderQueue（与现有 MeshRenderer 行为一致，风险已接受），单测 ROI 低。验证 = Unity 编译通过（类型检查）+ Task 6 PlayMode Frame Debugger 确认粒子 draw call 与 UI mesh 同 Transparent 队列、排序正确。

- [ ] **Step 1: 读 CacheRenderers 当前实现确认改动点**

Read `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs:64-79`。确认结构：
```csharp
static void CacheRenderers(GameObject go)
{
    foreach (var r in go.GetComponentsInChildren<Renderer>(true))
    {
        if (r == null) continue;
        if (r is MeshRenderer || r is SkinnedMeshRenderer)   // ← 本行改
        {
            foreach (var mat in r.sharedMaterials)
            {
                if (mat != null && mat.renderQueue != 3000) mat.renderQueue = 3000;
            }
        }
    }
}
```

- [ ] **Step 2: 加 ParticleSystemRenderer 到类型判断**

Edit `NativeHostManager.cs`，把：
```csharp
            if (r is MeshRenderer || r is SkinnedMeshRenderer)
```
改为：
```csharp
            // ParticleSystemRenderer 也纳入：粒子 material renderQueue 统一 3000（Transparent），
            // 否则粒子与 UI mesh 队列不一致 → sortingOrder 跨 UI/GO 排序错乱。
            if (r is MeshRenderer || r is SkinnedMeshRenderer || r is ParticleSystemRenderer)
```

- [ ] **Step 3: 更新类头注释提到粒子**

Edit `NativeHostManager.cs` 文件顶部 `<summary>`（约 line 6-19），在「MeshRenderer/SkinnedMeshRenderer material renderQueue=3000」相关说明后补一句「+ ParticleSystemRenderer（v1.4-b NativeHost demo）」。具体：找到 line 14 附近 `/// GO material renderQueue=3000`，扩写为 `/// GO（Mesh/SkinnedMesh/ParticleSystem Renderer）material renderQueue=3000`。

- [ ] **Step 4: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs
git commit -m "$(cat <<'EOF'
feat(nativehost): CacheRenderers handles ParticleSystemRenderer

Particle effects' material renderQueue now set to 3000 (Transparent),
matching Mesh/SkinnedMesh renderers — fixes cross UI/GO sort order for
particles bound via NativeHost.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

> 验证（Task 6）：Unity focus 触发编译无错；PlayMode Frame Debugger 确认粒子 draw call 在 Transparent(3000) 队列。

---

## Task 3: 新建 page_nativehost.html

**Files:**
- Create: `loomgui_unity/Assets/LoomUI/showcase/page_nativehost.html`

**Interfaces:**
- Produces: 三个 CSS id 契约供 Task 5 driver 用：
  - `nh-stage`（div，600×700，NativeHost slot 占位）
  - `nh-effect`（div.btn，toggle 粒子）
  - `nh-anim`（div.btn，切 Animator state）
  - `back-home`（复用语义 id，OpenPage 回首页）

- [ ] **Step 1: 写 page_nativehost.html**

Write `loomgui_unity/Assets/LoomUI/showcase/page_nativehost.html`:
```html
<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<title>NativeHost · 3D 角色 + 特效</title>
<style>
/* page_nativehost 组件 CSS（D6 隔离） */
.root{width:1080px;height:1920px;background-color:#1a1d2e;}
.header{background-color:#252839;border-bottom:2px solid #3a3f55;padding:20px;gap:14px;flex-direction:row;align-items:center;justify-content:space-between;}
.title{color:#e0e0e0;font-size:32px;font-weight:700;}
.back{background-color:#2d3148;border:1px solid #3a3f55;padding:10px 16px;color:#9aa0b4;font-size:18px;}
.back:hover{background-color:#3a4258;color:#5fb2c4;border:1px solid #5fb2c4;}
.back:active{transform:scale(0.96);}
.content{flex-grow:1;flex-direction:column;gap:24px;padding:32px;align-items:center;}
/* nh-stage：NativeHost 占位 div。角色 + 粒子 GO 由 driver BindNativeHost 挂此节点，
   wrapper 每帧 Sync 跟随其 world rect。尺寸 = design 空间角色的可视区。 */
.stage-wrap{background-color:#11131f;border:1px solid #3a3f55;border-radius:12px;padding:8px;}
.nh-stage{width:600px;height:700px;}
.controls{flex-direction:row;gap:20px;}
.btn{background-color:#2d3148;border:1px solid #3a3f55;padding:16px 32px;color:#e0e0e0;font-size:22px;}
.btn:hover{background-color:#3a4258;border:1px solid #5fb2c4;color:#5fb2c4;}
.btn:active{transform:scale(0.96);background-color:#5fb2c4;color:#1a1d2e;}
.hint{color:#9aa0b4;font-size:16px;}
</style>
</head>
<body>
<div class="root">
  <div class="header">
    <div class="title">NativeHost · 3D 角色 + 特效</div>
    <div class="back" id="back-home">← 返回</div>
  </div>
  <div class="content">
    <div class="stage-wrap">
      <div id="nh-stage" class="nh-stage"></div>
    </div>
    <div class="controls">
      <div class="btn" id="nh-effect">放光效</div>
      <div class="btn" id="nh-anim">切动画</div>
    </div>
    <div class="hint">角色 SkinnedMesh + 粒子挂 nh-stage，跟随 transform / 显隐 / 排序</div>
  </div>
</div>
</body>
</html>
```

- [ ] **Step 2: 围栏自查**

确认 HTML 只用围栏内：标签 `div`/`style`/`html`/`head`/`body`/`meta`/`title`（白名单）；CSS 属性 `width/height/background-color/border/border-radius/padding/gap/flex-direction/flex-grow/align-items/justify-content/color/font-size/transform`（全围栏内）。无 `position:absolute`、无 `button` 标签、无禁属性。

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/showcase/page_nativehost.html
git commit -m "$(cat <<'EOF'
feat(showcase): add page_nativehost (3D character + effects demo)

New page with nh-stage slot (600x700) for NativeHost binding + two
buttons (nh-effect toggle particles, nh-anim switch animator state).
Self-contained <style> per showcase D6 isolation convention.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: home.html 加 nav-nativehost 入口

**Files:**
- Modify: `loomgui_unity/Assets/LoomUI/showcase/home.html`（`.grid` 内 `nav-list` 后插一张卡片）

**Interfaces:**
- Produces: `nav-nativehost` div（home nav 卡片）→ Task 5 driver `AddNavListener("nav-nativehost", "page_nativehost")`。

- [ ] **Step 1: 在 nav-list 卡片后插入 nav-nativehost**

Edit `home.html`，找到（约 line 68-72）：
```html
      <div class="navbtn" id="nav-list">
        <div class="nav-icon">☰</div>
        <div class="nav-label">虚拟列表</div>
        <div class="nav-desc">等高+不等高/复用</div>
      </div>
```
在其**后面**插入：
```html
      <div class="navbtn" id="nav-nativehost">
        <div class="nav-icon">♟</div>
        <div class="nav-label">3D/特效</div>
        <div class="nav-desc">角色+粒子 NativeHost</div>
      </div>
```

- [ ] **Step 2: 确认 grid 布局无需调**

`home.html` 的 `.grid{flex-direction:row;flex-wrap:wrap;gap:20px;}`，每张 `.navbtn` 300×160，content 宽 1080-48=1032，一行容 3 张（300×3+20×2=940<1032）。原 8 张 → 现 9 张 = 3×3 整齐。**无需改 CSS。**

- [ ] **Step 3: Commit**

```bash
git add loomgui_unity/Assets/LoomUI/showcase/home.html
git commit -m "$(cat <<'EOF'
feat(showcase): add home nav entry for page_nativehost

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: LoomShowcaseDriver 加 NativeHost 订阅

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs`（三处：字段、SubscribePage switch、SubscribeHome nav、新方法块）

**Interfaces:**
- Consumes: Task 3 的 id 契约（`nh-stage`/`nh-effect`/`nh-anim`）；Task 4 的 `nav-nativehost`；现有 `LoomStage.BindNativeHost(uint, GameObject)` / `FindNodeById` / `UnbindNativeHost`。
- Produces: page_nativehost 完整订阅——角色 + 粒子挂 nh-stage，按钮交互。

- [ ] **Step 1: 加字段（在现有 _nativeScale 后，约 line 22 之后）**

Edit `LoomShowcaseDriver.cs`，找到：
```csharp
        // Cube 1m³ 在 UI design 空间天然小，设 scale 放大填 slot（NativeHost Sync 不动用户 GO scale）。
        [SerializeField] Vector3 _nativeScale = new Vector3(120, 120, 120);
```
在其**后面**插入：
```csharp

        // === page_nativehost：3D 角色 + 粒子（NativeHost 压测）===
        [SerializeField] GameObject _characterPrefab;       // animatedman 角色 prefab（Inspector 拖）
        [SerializeField] GameObject _effectPrefab;          // Kenney Magic/Fire prefab（Inspector 拖）
        // ~1.7m fbx × 70 ≈ 120px 填 nh-stage 视觉区；PlayMode 微调。NativeHost Sync 不动用户 GO scale。
        [SerializeField] Vector3 _characterScale = new Vector3(70, 70, 70);
        [SerializeField] Animator _characterAnimator;       // 角色 Animator（切 clip；可空）
        [SerializeField] string[] _animStates = { "Idle", "Walk", "Run" };   // Animator state 名（按实际 clip 填；可空）

        // 角色 + 粒子 child 缓存实例：跨页存活只 Instantiate 一次。
        // 离开页 Unbind 只 SetActive(false) 不销毁 → 复用同一 GO，避免反复进出页堆积。
        GameObject _characterInstance;
        int _animIdx;
        bool _effectOn = true;
```

- [ ] **Step 2: SubscribePage switch 加 case**

找到 `SubscribePage`（约 line 212-226）：
```csharp
                case "page_list": SubscribeList(); break;
            }
```
改为：
```csharp
                case "page_list": SubscribeList(); break;
                case "page_nativehost": SubscribeNativeHost(); break;
            }
```

- [ ] **Step 3: SubscribeHome 加 nav 监听**

找到 `SubscribeHome`（约 line 232-239 区域）：
```csharp
            AddNavListener("nav-list", "page_list");
```
在其**后面**加一行：
```csharp
            AddNavListener("nav-nativehost", "page_nativehost");
```

- [ ] **Step 4: 加 4 个新方法（在 SubscribeControls 方法后，约 line 282 之后）**

在 `SubscribeControls()` 方法的闭合 `}` 之后、`SubscribeText()` 之前插入：
```csharp

        // page_nativehost：back-home + 角色/粒子 NativeHost 绑定 + 放光效/切动画按钮。
        void SubscribeNativeHost()
        {
            SubscribeBackHome();
            EnsureCharacterInstance();
            if (_characterInstance != null)
            {
                uint stage = _stage.FindNodeById("nh-stage");
                if (stage != uint.MaxValue)
                {
                    _stage.BindNativeHost(stage, _characterInstance);
                    _nativeBoundNode = stage;   // 记下，离开页时 OpenPage 的 Unbind 摘 wrapper GO
                }
                else Debug.LogError("[Showcase] page_nativehost: id 'nh-stage' 未找到，跳过 NativeHost 绑定");
            }
            SubscribeLamp("nh-effect", EventType.Click, OnNhEffect);
            SubscribeLamp("nh-anim", EventType.Click, OnNhAnim);
            Debug.Log("[Showcase] page_nativehost 订阅完成（角色+粒子 NativeHost + 按钮）");
        }

        // 角色 + 粒子 child 缓存实例（只建一次）。Instantiate 后立即 SetActive(false)：
        // BindNativeHost 前角色默认 active 会显示在场景原点；藏起来等 wrapper Sync 重新 SetActive(true)。
        void EnsureCharacterInstance()
        {
            if (_characterInstance != null) return;
            if (_characterPrefab == null)
            {
                Debug.LogError("[Showcase] _characterPrefab 未配，page_nativehost 角色不显示");
                return;
            }
            _characterInstance = Instantiate(_characterPrefab);
            _characterInstance.transform.localScale = _characterScale;
            _characterInstance.SetActive(false);
            if (_effectPrefab != null)
            {
                // 粒子挂角色 child；局部位置由 prefab 自带 transform 决定。PlayMode 看偏了在 prefab 调。
                Instantiate(_effectPrefab, _characterInstance.transform, false);
            }
            else Debug.LogWarning("[Showcase] _effectPrefab 未配，page_nativehost 无粒子");
        }

        // toggle 角色 child 下的粒子（SetActive + Play/Stop）。
        void OnNhEffect(EventContext ctx)
        {
            if (_characterInstance == null) return;
            var ps = _characterInstance.GetComponentInChildren<ParticleSystem>();
            if (ps == null) { Debug.LogWarning("[Showcase] 角色下无 ParticleSystem"); return; }
            _effectOn = !_effectOn;
            if (_effectOn) { ps.gameObject.SetActive(true); ps.Play(); }
            else { ps.Stop(); ps.gameObject.SetActive(false); }
        }

        // 循环切 Animator state（Idle/Walk/Run）。
        void OnNhAnim(EventContext ctx)
        {
            if (_characterAnimator == null || _animStates == null || _animStates.Length == 0) return;
            _animIdx = (_animIdx + 1) % _animStates.Length;
            _characterAnimator.Play(_animStates[_animIdx]);
        }
```

- [ ] **Step 5: 语法/类型自查**

对照现有代码确认：
- `Instantiate(GameObject)` / `Instantiate(GameObject, Transform, bool)` —— MonoBehaviour 标准方法，driver 继承 MonoBehaviour ✅
- `GetComponentInChildren<ParticleSystem>()` —— UnityEngine 标准方法 ✅
- `EventType.Click` / `EventContext` —— 现有 driver 已用（SubscribeLamp/OnClickHit 同款）✅
- `SubscribeLamp(string, EventType, EventCallback)` —— 现有方法（line 325）✅
- `_stage.BindNativeHost(uint, GameObject)` —— 现有 public（LoomStage.cs:164）✅
- 真实编译验证在 Task 6（Unity focus 编译）。

- [ ] **Step 6: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/LoomShowcaseDriver.cs
git commit -m "$(cat <<'EOF'
feat(showcase): wire page_nativehost NativeHost subscription

Add SubscribeNativeHost + EnsureCharacterInstance (cached character +
particle child GO) + OnNhEffect/OnNhAnim button callbacks + home nav.
Character + particle bound to nh-stage via BindNativeHost; cached
instance reused across page visits (Unbind only SetActive(false)).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: 打包 + 用户操作 + PlayMode 验收（handoff）

**Files:** 无代码改动。用户执行 Unity 编辑器操作 + 重打 pkg + PlayMode。

- [ ] **Step 1: 用户下载 Quaternius animatedman**

用户浏览器打开 `https://quaternius.com/packs/animatedman.html` → Download → 下 zip → 解压 → 把 fbx + 贴图放进 `loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/`（与 LICENSE.txt 同目录）。

- [ ] **Step 2: 用户在 Unity Import Kenney unitypackage**

Unity 打开 `loomgui_unity/` 工程 → `Assets > Import Package > Custom Package` → 选 `Assets/LoomUI/res/effects/kenney_particles/particlePack_samples.unitypackage` → Import。得 `Kenney/ParticlePack/Prefabs/`（Magic/Fire/Sparks/Smoke/Electricity/Hearts）+ Materials + Sprites。

- [ ] **Step 3: 用户配角色 Animator Controller**

- 选 animatedman fbx → Inspector → Rig → Animation Type = Generic（或 Humanoid，视 fbx 骨骼）→ Apply。
- fbx 的 Animations tab 确认 clip 名（Walk/Run/Survey 等）。
- 建 Animator Controller（如 `quaternius_animatedman.controller`）→ 拖入 clip 作 state（Idle/Walk/Run，名字填 driver `_animStates`，默认 `{ "Idle", "Walk", "Run" }`，按实际 clip 改 Inspector）。
- 场景里拖 fbx 进 SampleScene → 加 Animator 组件 → 绑 Controller → 存成 prefab（如 `animatedman.prefab`）放 `res/models/quaternius_animatedman/`。

- [ ] **Step 4: 用户配 Inspector + 重打 pkg**

- SampleScene 选 LoomShowcaseDriver GO → Inspector：
  - `_Character Prefab` → 拖 `animatedman.prefab`
  - `_Effect Prefab` → 拖 Kenney `Magic.prefab`（或 Fire/Sparks）
  - `_Character Animator` → 拖 prefab 的 Animator
  - `_Anim States` → 按实际 clip 名调（默认 Idle/Walk/Run）
  - `_Character Scale` → 默认 (70,70,70)，PlayMode 看效果微调
- 重打 `showcase.pkg.bin`：`LoomGUI > Settings` 面板打包，或 `cargo run -p loomgui_pkg`（参数见 `.claude/skills/loomgui-editor/config.json`；确保 `--html` 列表含 `page_nativehost.html`）。产物落 `Assets/StreamingAssets/showcase.pkg.bin`。

- [ ] **Step 5: PlayMode 验收清单（spec §7）**

进 PlayMode → home 点「3D/特效」卡片 → 进 page_nativehost，逐项勾：

1. **角色 SkinnedMesh + Animator**：角色可见、idle 动画每帧播；点「切动画」流畅切 Walk/Run；**未**上下颠倒/绕轴反转（坑 72 翻正在 SkinnedMesh 仍成立）；`_characterScale` 放大对 skin 无畸变。
2. **粒子**：点「放光效」放/收正常；粒子朝向未被 y-flip 带歪（Fire 火苗朝上）；粒子与 stage-wrap 背景/UI 按钮透明排序正确（不互相穿插/遮挡）；additive blend 视觉正常。
3. **切页回归**：点「← 返回」→ 角色/粒子消失无残留；再进 page_nativehost → 复现（同缓存实例，无堆积）。
4. **sortingOrder / 队列**：Frame Debugger（Window > Analysis > Frame Debugger）确认角色 SkinnedMesh + 粒子 + UI mesh 的 draw order 符合 `sort_key`，粒子在 Transparent(3000) 队列。

- [ ] **Step 6: 验收记录 + 收尾 commit（如有调整）**

PlayMode 若调了 `_characterScale` / `_animStates` / 粒子 prefab 选择 → 场景文件或 prefab 变更，按需 commit。坑（如发现新 lite 版边界）记 `docs/pitfalls.md`（编号递增）。

---

## Self-Review 已做

- **Spec coverage**：spec §3 资源 → Task 1+Step1；§4 入库 → Task 1；§5.1 页面 → Task 3；§5.2 nav → Task 4；§5.3 driver → Task 5；§5.4 NativeHostManager → Task 2；§5.5 下载脚本 → Task 1；§6 分工 → Task 6；§7 验收 → Task 6 Step 5。全覆盖。
- **Placeholder**：无 TBD/TODO；每步含完整代码或精确命令。
- **Type/命名一致**：`nh-stage`/`nh-effect`/`nh-anim`/`nav-nativehost` 跨 Task 3/4/5 一致；`_characterPrefab`/`_effectPrefab`/`_characterAnimator`/`_animStates`/`_characterInstance` 跨 Step 一致；`EnsureCharacterInstance`/`SubscribeNativeHost`/`OnNhEffect`/`OnNhAnim` 定义与调用一致。
