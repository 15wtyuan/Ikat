# NativeHost 3D 角色 + UI 特效压测 demo 设计契约

> 日期：2026-07-04
> 范围：在 showcase 新建 `page_nativehost`，挂真实带骨骼动画的 3D 角色（Quaternius CC0 FBX）+ UI 粒子特效（Kenney CC0）到 NativeHost slot，压测 NativeHost-lite 在复杂资源下的边界。
> 模式：纯后端 C# + showcase HTML/CSS（每页自带 `<style>`，D6 组件隔离）+ pkg 重打（用户侧）。**零 Rust/FFI/blob 改动**（NativeHost-lite 本就零 FFI）。

## 1. 背景 / 动机

NativeHost-lite（v1d.3，`NativeHostManager.cs`）已实现「外部 GO 跟随 UI 节点 transform/显隐/sortingOrder」，但仅用 1m³ Cube 验收过（坑 72 handedness flip 已修——`_container localScale=(1,-1,1)` 翻正）。**真实复杂资源从未压过**，潜在塌点：

- **SkinnedMesh + Animator**：骨骼动画每帧重算 bone matrix，wrapper transform 跟随是否每帧生效？`_container (1,-1,1)` y-flip 是否致角色上下颠倒 / 绕轴反转？scale 放大对 skin 是否正确？
- **粒子 `ParticleSystemRenderer`**：`CacheRenderers` 目前只处理 `MeshRenderer/SkinnedMeshRenderer` 的 renderQueue=3000——**粒子 renderer 不在列表，队列不被统一**，与 UI mesh 的透明排序可能错。
- 切 clip / 显隐联动是否正常。

本 demo 用最小工程把这些边界压出来，并顺手修粒子 renderer 遗漏。

## 2. 目标 / 非目标

**目标**
- 新建 `page_nativehost`：大尺寸 NativeHost slot 挂角色 + 粒子，两个交互按钮（放光效 / 切动画）。
- 验证 SkinnedMesh + Animator + 粒子在 wrapper 下的渲染/动画/排序正确性。
- 修 `NativeHostManager.CacheRenderers`：纳入 `ParticleSystemRenderer`。

**非目标**
- 不做完整 NativeHost v2（hit/clip/尺寸 push——roadmap §5.3 仍 defer）。
- 不加 glTFast 依赖（角色用 FBX，Unity 原生）。
- 不动 Rust 核心 / FFI / blob / `PKG_FORMAT_VERSION`。
- 不删现有 §1.6 model-slot（page_controls 保留，作轻量对照）。

## 3. 资源

| | 角色 | 粒子 |
|---|---|---|
| 名称 | Quaternius Animated Man | Kenney Particle Pack v1.1 |
| 下载页 | `quaternius.com/packs/animatedman.html` | `kenney.nl/assets/particle-pack` |
| 直链 | Google Drive（**浏览器下**，脚本下不了——沙箱验证 Drive 被拦） | `https://kenney.nl/media/pages/assets/particle-pack/f8fe0f8cb8-1677578741/kenney_particle-pack.zip` |
| 许可 | CC0 | CC0 |
| 大小 | ~几 MB（zip：fbx + 贴图 + 动画 clip） | 14.3 MB |
| 格式 | FBX（Unity 原生认，零依赖） | zip：193 PNG sprite + `Unity samples/particlePack_samples.unitypackage`（6 prefab） |
| 内容 | 人形 + 骨骼动画（Walk/Run/Survey 等 clip） | 6 prefab：Magic / Fire / Sparks / Smoke / Electricity / Hearts |

家里机已 `curl -sI` 验证：Kenney 直链 `200 / application/zip / 15001764`；Quaternius 站点可达、下载按钮指向 Drive（用户浏览器下）。

## 4. 入库结构

```
loomgui_unity/Assets/LoomUI/res/models/quaternius_animatedman/
  ├── <fbx + 贴图>            （用户浏览器下 zip → 解压）
  └── LICENSE.txt             （CC0，注明 Quaternius 来源）
loomgui_unity/Assets/LoomUI/res/effects/kenney_particles/
  ├── particlePack_samples.unitypackage  （我 curl 下 zip → 解压提取）
  └── LICENSE.txt             （CC0，注明 Kenney 来源）
```

> unitypackage 不会被 Unity 自动 import。用户须在编辑器 `Assets > Import Package` 导入，得 6 个 prefab + Materials + Sprites。

## 5. 实现设计

### 5.1 新页 `showcase/page_nativehost.html`（自带 `<style>`，D6 隔离）

```
┌─ header: title "NativeHost · 3D 角色 + 特效" + back-home ─┐
│                                                              │
│   ┌──── stage-wrap (暗框) ────────────────┐                  │
│   │   #nh-stage  600×700                  │  ← 角色+粒子 GO  │
│   │   （空 div，driver BindNativeHost）    │     挂此 slot    │
│   └────────────────────────────────────────┘                 │
│                                                              │
│        [放光效 #nh-effect]   [切动画 #nh-anim]               │
│                                                              │
│        hint: "角色 SkinnedMesh + 粒子跟随 transform/显隐/排序" │
└──────────────────────────────────────────────────────────────┘
```

结构遵循 `page_controls.html` 模式：`.root > .header(title + #back-home) > .content`。CSS 复用 showcase 配色（`#1a1d2e/#252839/#5fb2c4`）。`.nh-stage` 设暗底 + 圆角边框让 slot 区可见；空 div 不含子节点（角色 GO 由 C# 绑外部）。

按钮用 `<div class="btn">`（showcase 惯例，不用 `<button>` 标签——与 page_controls §1.5 一致）。

### 5.2 `showcase/home.html` 加 nav 入口

`.grid` 内追加一张卡片（复用现有 `.navbtn` 300×160 样式）：
```html
<div class="navbtn" id="nav-nativehost">
  <div class="nav-icon">♟</div>
  <div class="nav-label">3D/特效</div>
  <div class="nav-desc">角色+粒子 NativeHost</div>
</div>
```
grid 现有 8 张 → 9 张（3×3 整齐），无需调布局 CSS。

### 5.3 `LoomShowcaseDriver.cs`

**新增字段**：
```csharp
[SerializeField] GameObject _characterPrefab;       // animatedman 角色 prefab（用户拖）
[SerializeField] GameObject _effectPrefab;          // Kenney Magic 或 Fire prefab（用户拖）
[SerializeField] Vector3 _characterScale = new Vector3(70, 70, 70);  // ~1.7m×70≈120px，PlayMode 微调
[SerializeField] Animator _characterAnimator;       // 角色 Animator（切 clip；可空）
[SerializeField] string[] _animStates = { "Idle", "Walk", "Run" };   // Animator state 名（可空）

GameObject _characterInstance;   // 缓存（角色 root + 粒子 child），跨页存活，只 Instantiate 一次
int _animIdx;
bool _effectOn = true;
```

**`SubscribePage` switch 加 case**：`case "page_nativehost": SubscribeNativeHost(); break;`

**新方法**：
- `SubscribeNativeHost()`：`SubscribeBackHome()` → `EnsureCharacterInstance()` → `BindNativeHost(FindNodeById("nh-stage"), _characterInstance)` + 记 `_nativeBoundNode` → 订阅 `nh-effect`/`nh-anim` 按钮。
- `EnsureCharacterInstance()`：if null → `Instantiate(_characterPrefab)` → 设 `localScale = _characterScale` → `SetActive(false)`（BindNativeHost 后 wrapper Sync 会重新 SetActive(true)）→ if `_effectPrefab` != null，`Instantiate(_effectPrefab, instance.transform, false)` 挂 child。
- `OnNhEffect`：`GetComponentInChildren<ParticleSystem>()` → toggle `gameObject.SetActive` + `Play/Stop`。
- `OnNhAnim`：`_characterAnimator.Play(_animStates[++_animIdx % len])`。

**复用现有 OpenPage 的 Unbind 逻辑**（line 152-158）：离开 page_nativehost 时 `_nativeBoundNode != MaxValue` → `UnbindNativeHost`（wrapper 销毁、character SetActive(false)）。`_characterInstance` 缓存保留，再进页重新 Bind。

### 5.4 `NativeHostManager.cs`（修粒子 renderer 遗漏）

`CacheRenderers`（line 71）一行改：
```csharp
// 前：if (r is MeshRenderer || r is SkinnedMeshRenderer)
if (r is MeshRenderer || r is SkinnedMeshRenderer || r is ParticleSystemRenderer)
```
让粒子 material renderQueue 也被设 3000（Transparent，与 UI 同队列），sortingOrder 跨 UI/GO 统一排序。

### 5.5 下载脚本（Kenney；我在会话里直接执行）

```bash
TMP=$(mktemp -d)                                    # 项目外临时目录，避免污染 Assets（Unity 开着会扫中间文件）
OUT=loomgui_unity/Assets/LoomUI/res/effects/kenney_particles
mkdir -p "$OUT"
curl -L -o "$TMP/k.zip" "https://kenney.nl/media/pages/assets/particle-pack/f8fe0f8cb8-1677578741/kenney_particle-pack.zip"
unzip -o "$TMP/k.zip" -d "$TMP/x"
mv "$TMP/x/Unity samples/particlePack_samples.unitypackage" "$OUT/"
mv "$TMP/x/License.txt" "$OUT/LICENSE.txt"
rm -rf "$TMP"
```
> 下载期间 Unity 最好关着；解压完只留 `unitypackage` + `LICENSE.txt` 入库，PNG sprite 由用户 Import unitypackage 后进工程。

角色 FBX 用户浏览器下，spec 不脚本化（Drive 链接脚本不可达）。

## 6. 分工

| 谁 | 干啥 |
|---|---|
| Claude | 新建 `page_nativehost.html`；改 `home.html`（加 nav 卡片）；改 `LoomShowcaseDriver.cs`（字段 + 4 方法 + switch）；改 `NativeHostManager.cs`（一行）；curl 下 Kenney + 提取 unitypackage 入库；写操作指引 |
| 用户 | ① 浏览器下 Quaternius animatedman zip → 解压 fbx+贴图到 `res/models/quaternius_animatedman/`；② Unity Import `particlePack_samples.unitypackage`；③ 角色 fbx 配 Animator Controller（Idle/Walk/Run states）；④ Inspector 拖 `_characterPrefab`/`_effectPrefab`/`_characterAnimator` + 调 `_characterScale`；⑤ `cargo run -p loomgui_pkg`（家机有打包器）重打 `showcase.pkg.bin`；⑥ PlayMode 验收 |

## 7. 验收清单（PlayMode 实测，逐项勾）

1. **角色 SkinnedMesh + Animator**：进 page_nativehost 角色可见、idle 动画每帧播；`nh-anim` 切 Walk/Run 流畅；角色**未**上下颠倒/绕轴反转（验坑 72 翻正在 SkinnedMesh 上仍成立）；`_characterScale` 放大对 skin 无畸变。
2. **粒子**：`nh-effect` 放/收粒子正常；粒子朝向未被 y-flip 带歪（如 Fire 火苗朝上而非朝下）；粒子与 UI mesh 透明排序正确（粒子不被 stage-wrap 背景错误遮挡 / 不穿插 UI 按钮）；additive blend 视觉正常。
3. **切页回归**：back-home 离开 → 角色/粒子消失无残留（wrapper 销毁、GO SetActive(false)）；再进 page_nativehost → 角色/粒子复现（同 `_characterInstance` 缓存，无堆积）。
4. **sortingOrder**：Profiler 确认角色 SkinnedMesh + 粒子 + UI mesh 的 draw order 符合 `sort_key`（粒子纳入 CacheRenderers 后）。

## 8. 风险 / 已知坑

- **Quaternius animatedman 的 clip 名 / Animator 结构**：导入后才能确定 clip 名（可能 Walk/Run/Survey 或别名）。`_animStates` 由用户按实际 clip 名填；driver 不硬编码。
- **粒子 localPosition**：Kenney prefab 自带 transform，挂角色 child 后位置可能偏（不在手部/脚下）。PlayMode 看效果，必要时在 prefab 或 driver 调 `_effectPrefab` 的局部偏移（可加 `_effectLocalPos` 字段，初版不加，看效果再说）。
- **scale 链**：角色最终 worldScale = root(sf,-sf,sf) × container(1,-1,1) × wrapper(sx,sy,1/sf) × `_characterScale`。`_characterScale=(70,70,70)` 是粗估（1.7m×70≈120px 填 nh-stage 视觉区），PlayMode 调。
- **`ParticleSystemRenderer` renderQueue**：改 CacheRenderers 后，若粒子 material 用了 shader 的 renderQueue 覆盖（如 Additive=3000 本就一致），改动无副作用；若不一致，修后统一到 3000。回归测确认无视觉退化。
- **本机工作流**：家里机当前会话——我执行下载 + 改代码；用户执行 Unity 编辑器操作 + 重打 pkg + PlayMode 验收。无 .dll 重编（零 Rust 改动）。

## 9. 不做的事（YAGNI）

- 不做世界空间 UI / 多 slot / 多角色 / 3D 场景片段。
- 不暴露 `<l-3d>` 之类新标签（围栏只有 div/span/img/button；NativeHost 是普通 div + C# 绑定，与虚拟列表同理——核心不认识"3D"）。
- 不写独立的可复用下载脚本文件（会话内直接 curl；用户要可复用再封装）。
