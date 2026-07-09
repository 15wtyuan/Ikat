# LoomGUI Unity 运行时 C# 后端 深度代码审查

> 审查日期：2026-07-09 | 审查文件数：10 | 总行数：~2,300

---

## 一、FrameBlob.cs — Blob 解析

### 1.1 ✅ 正确使用 BitConverter（非 Marshal.PtrToStructure）

`FrameBlob` 操作的是 `byte[]` 托管数组，用 `BitConverter.ToUInt32`/`ToSingle` 读字段（line 185-186）。CLAUDE.md 禁止的 `Marshal.PtrToStructure` **不在本文件**。注释 line 17 已注明 "C# on Windows is little-endian"，当前正确。

### 1.2 🔴 严重 · LoomEventHandler.cs 仍使用 Marshal.PtrToStructure

**位置：** `LoomEventHandler.cs:205-208`、`LoomEventHandler.cs:262-265`

```csharp
// line 205
int recSize = System.Runtime.InteropServices.Marshal.SizeOf<LoomEvent>();
// line 208
var evt = System.Runtime.InteropServices.Marshal.PtrToStructure<LoomEvent>(ptr + i * recSize);

// line 262 (DispatchControllerChanged 同理)
int recSize = System.Runtime.InteropServices.Marshal.SizeOf<LoomControllerChangedEvent>();
// line 265
var evt = System.Runtime.InteropServices.Marshal.PtrToStructure<LoomControllerChangedEvent>(ptr + i * recSize);
```

**问题：** CLAUDE.md 明确禁止 `Marshal.PtrToStructure`——IL2CPP 移动端 struct 对齐不可预测（IL2CPP 的 managed struct layout 与 Mono 不同）。代码注释 line 202-203 也承认："桌面 Mono OK；IL2CPP 移动端对齐坑届时换 Span+BinaryPrimitives"。

**修复方向：** 用 `Span<byte>` + `BinaryPrimitives.ReadUInt32LittleEndian` / `ReadSingleLittleEndian` 逐字段手动读。`LoomEvent` 只有 7 个字段（20 字节），`LoomControllerChangedEvent` 3 个字段（12 字节），替换代价极小。

**严重级别：🔴 严重**（IL2CPP 构建崩溃风险）

### 1.3 ⚠️ 中等 · ColorMatrix 每帧分配 new float[20]

**位置：** `FrameBlob.cs:84-91`

```csharp
public float[] ColorMatrix(int i) {
    int off = ColOff(17) + i * 80;
    float[] m = new float[20];
    for (int j = 0; j < 20; j++) {
        m[j] = BitConverter.ToSingle(_buf, off + j * 4);
    }
    return m;
}
```

每次调用分配 `new float[20]`。LoomStage.cs:42 注释写"ReadMesh per-node alloc 留观察，撞墙再上 List 复用"——同理适用此处。若一帧有大量 color filter 节点，GC 压力显著。

**严重级别：⚠️ 中等**

### 1.4 ℹ️ 低 · ReadPath 线性扫描 path table

**位置：** `FrameBlob.cs:118-128`

```csharp
for (uint n = 1; n <= idx; n++) { ... }
```

每个节点每次调 `ReadPath` 都从头遍历。若多个节点共用同一 path_idx，可加一个 `Dictionary<uint, string>` 缓存（与 CLIP 表同样做法，entry 少）。

**严重级别：ℹ️ 低**

---

## 二、MirrorPool.cs — GO 镜像池

### 2.1 ✅ ChangeLevel 三分支正确处理

- **SKIP (0):** `MirrorPool.cs:78-82` — TryGetValue 清 Stale，继续。GO 保留，transform/材质完全不碰。
- **HEADER (1):** `MirrorPool.cs:111` — 调 `UpdateHeader`（position/sortingOrder/material/MPB uniform），**不调 `UploadMeshOrText`**，mesh 保持不变。
- **FULL (2):** `MirrorPool.cs:112` — `UpdateHeader` + `UploadMeshOrText`（重建 mesh）。

新 GO（`NewRenderObj`）强制设 `level = 2`（line 106），确保首帧必定 FULL 上传 mesh。

### 2.2 ✅ reuse_key 双 dict 正确切换

**位置：** `MirrorPool.cs:72-75`

```csharp
uint poolKey = reuseKey != 0 ? reuseKey : id;
Dictionary<uint, RenderObj> pool = reuseKey != 0 ? _poolByReuse : _poolByNodeId;
```

Slot 换绑：Frame N 的 Item A（reuseKey=5, nodeId=100）→ `_poolByReuse[5]`；Frame N+1 的 Item B（reuseKey=5, nodeId=200）→ 同一 `_poolByReuse[5]`，`Stale=false`，`LastNodeId=200`。GO 复用，不销毁重建。**逻辑正确。**

### 2.3 ⚠️ 中等 · reuseKey 从 0→>0 或 >0→0 过渡时 GO 销毁+重建

**场景：** 节点从普通节点（reuseKey=0）变为 slot（reuseKey=5）时：
- 旧 RenderObj 在 `_poolByNodeId[100]` 变为 stale → 该帧销毁
- 新 RenderObj 在 `_poolByReuse[5]` 创建 → 一帧内销毁+重建

对于虚拟列表初始化场景，这是可接受的（初始化后 reuseKey 不变）。但如果存在动态 reuseKey 变更场景（如 slot 复用键重编号），会产生不必要的 GO churn。

**严重级别：⚠️ 中等**（当前场景安全，文档应注明 reuseKey 绑定后不变）

### 2.4 ✅ 销毁路径无泄漏

**位置：** `MirrorPool.cs:115-121`（Sync 的 stale 销毁）、`MirrorPool.cs:288-309`（Clear/TearDown）

- stale GO 收集到 `dead1`/`dead2` 列表，统一调 `TearDown`（销毁 Mesh + GO）
- `TearDown` 用 `Application.isPlaying ? Destroy : DestroyImmediate` 适配 EditMode/PlayMode
- `Clear()` 遍历两个 dict 全量销毁

**无泄漏路径。**

### 2.5 ⚠️ 中等 · 同一 RenderObj 不可能同时出现在两个 dict

`pool` 变量按当前帧的 reuseKey 选择 dict。同一帧一个节点只产生一个 RenderObj，只插入一个 dict。上一帧的旧 dict 条目变为 stale → 销毁。不存在同一 RenderObj 同时在两个 dict 的情况，没有 double-free 风险。

**但存在隐患：** 如果核心在帧间同时改变了 node_id 和 reuse_key（如节点身份+slot 同时变更），旧 node_id 的 RenderObj 在 `_poolByNodeId` 变 stale 销毁，新 reuse_key 在 `_poolByReuse` 缺失则重建。两个 RenderObj 短暂共存一帧（stale 的在 pool 里未清理，新的也在 pool 里）。这是正确行为——Sync 末尾才清理 stale。

### 2.6 ℹ️ 低 · UploadMesh 中 Vector3(z=0) 转换可优化

**位置：** `MirrorPool.cs:249`

```csharp
v.Add(new Vector3(seg.Verts[i].x, seg.Verts[i].y, 0f));
```

每顶点分配一个 `Vector3`。当前用可复用 `List<Vector3>` 缓冲（clear 保留 capacity），warm-up 后零 alloc。**当前实现已优化。**

### 2.7 ℹ️ 低 · RemapMeshUvToSprite 分配临时 List<Vector2>

**位置：** `MirrorPool.cs:281-284`

```csharp
var uvs = new List<UnityEngine.Vector2>();
ro.Mesh.GetUVs(0, uvs);
```

可通过复用一个类级 `List<Vector2>` 消除此分配，与 VList/UvList 模式一致。

---

## 三、MaterialManager.cs — 材质缓存

### 3.1 ✅ Key 覆盖所有必要字段

**位置：** `MaterialManager.cs:68-83`

```csharp
readonly struct Key {
    readonly int _program;
    readonly Texture _tex;
    readonly uint _ctx;
    readonly bool _matrix;
}
```

- `program`: 区分 Text/Container+bg/Filter 等 5 种 shader keyword 组合
- `texture`: 区分不同图集/字体 atlas 页（Texture 引用同一性）
- `maskContext`: 每个裁剪 context 独立材质（`_ClipBox` uniform 不同）
- `matrixFlag`: 区分纯平移 vs 非纯平移（`OBJECT_MATRIX` keyword）

**覆盖完整。** tint/alpha 走顶点色+MPB per-renderer uniform（不在 Key 里），设计正确。

### 3.2 ⚠️ 中等 · SetClipBox 的 O(n) 遍历

**位置：** `MaterialManager.cs:50-55`

```csharp
public void SetClipBox(uint maskContext, Vector4 clipBox)
{
    _clipBoxByCtx[maskContext] = clipBox;
    foreach (var kv in _cache)
        if (kv.Key.Ctx == maskContext) kv.Value.SetVector("_ClipBox", clipBox);
}
```

每 ctx 每帧遍历全部缓存材质。当前 mask context 数量极少（通常 ≤5），O(n) 可接受。**优化方向：** 加一个 `_materialsByCtx: Dictionary<uint, Material>` 索引（Insert/Remove 维护），或等 mask context 数量增长时再优化。代码注释也承认 "few entries，O(n) 足够"。

**严重级别：⚠️ 中等**（当前安全，量增大时需重构）

### 3.3 ✅ 首帧 SetClipBox→Get 顺序解耦

注释 line 48-49 清晰说明两路覆盖：`_clipBoxByCtx` 先写 dict（新建材质时读），再遍历已缓存材质 `SetVector` 刷新。这使 MirrorPool 调用 `SetClipBox`/`Get` 的顺序不受约束。设计正确。

### 3.4 ℹ️ 低 · Texture 作为 Key 字段依赖引用同一性

`MaterialManager.cs:72` — `Texture` 作为 readonly struct 的字段，`GetHashCode` 用 `HashCode.Combine` 组合引用哈希。Unity 的 `UnityEngine.Object` 的 `GetHashCode` 基于 `InstanceID`（一个稳定递增的 int）。当前语义正确——不同纹理实例不会碰撞。

---

## 四、LoomEventHandler.cs — 事件路由

### 4.1 🔴 严重 · Marshal.PtrToStructure 违规

见 §1.2。两个方法均受影响：`DispatchPending`（line 208）和 `DispatchControllerChanged`（line 265）。

### 4.2 ✅ Capture/Bubble 阶段正确处理

**位置：** `LoomEventHandler.cs:272-294`

- **Capture 阶段（line 278-283）：** 根→target 反向（`i = chain.Count-1 → 0`），全跑不检查 `_stopsPropagation`（fgui 语义：capture 不可止）。
- **Bubble 阶段（line 285-293）：** target→root 正向，`_stopsPropagation` break（标准 DOM 语义）。`StopImmediatePropagation` 在 `EventBridge.CallBubble` 的 enumerator 中 break（line 120）。
- **CaptureTouch** 分两阶段记录（`_captureNodeCap`/`_captureNodeBub`），Down 事件后各加一个 touch monitor（line 214-218）。

**路由逻辑与 DOM/W3C 标准一致，与 fgui 对标。**

### 4.3 ✅ 事件类型分流正确

**位置：** `LoomEventHandler.cs:210-250`

| 事件类型 | 路由方式 | 理由 |
|---------|---------|------|
| Down/Up/Click/Drag*/LongPress/Key*/Focus* | `BubbleRoute` | 支持祖先监听 |
| Move（已由核心 diff 多目标） | `DirectDispatch` | 直派，不沿链 |
| RollOver/RollOut | `DirectDispatch` | 核心已 diff，每个命中目标单独派 |
| TweenComplete | `DirectDispatch` | target-specific |

**分流正确。** Click 事件中嵌入富文本超链接检测（line 223-228），命中 link 则走 `DispatchLinkClick`（直派），否则正常 bubble。设计合理。

### 4.4 ℹ️ 低 · AncestorChain 每事件分配新 List

**位置：** `LoomEventHandler.cs:309-315`

```csharp
var chain = new System.Collections.Generic.List<uint>();
```

用可复用 `List<uint>` 消除 GC——复用后调用 `Clear()`，`BubbleRoute` 中再用（零 alloc）。当前实现每事件 new，高事件量时有 GC 压力。

### 4.5 ℹ️ 低 · _parentCache 不会自动失效

**位置：** `LoomEventHandler.cs:139`、`LoomEventHandler.cs:319`

`_parentCache` 缓存 `node_parent` FFI 查询结果。`SetHandle`（scene 重建）时清缓存（line 150），但运行时动态删/加节点不会自动失效该缓存。如果业务运行中大量动态改树，缓存可能返回过期 parent。当前 FFI `loomgui_node_parent` 是纯查询（O(1) 的 slotmap 索引），缓存失效代价不大（一次核心查询），可考虑去掉缓存简化代码。

---

## 五、LoomInputCollector.cs — 输入采集

### 5.1 ⚠️ 中等 · #ifdef 分支导致大量重复代码

**位置：** `LoomInputCollector.cs:54-100`（Collect 中鼠标+触摸）、`LoomInputCollector.cs:154-159`（滚轮）、`LoomInputCollector.cs:163-167`（屏幕位置）、`LoomInputCollector.cs:182-193`（modifiers）

四段 `#if ENABLE_INPUT_SYSTEM / #else / #endif`，每段新旧代码几乎完全重复。可抽象为：

```csharp
interface IPointerSource {
    Vector2 MousePosition { get; }
    float ScrollDelta { get; }
    byte MouseKind { get; }  // 0=down/1=up/2=none
    IEnumerable<TouchData> Touches { get; }
}
```

两个实现（`LegacyInputSource` / `NewInputSource`），`Collect` 用统一接口。消除 4 段 `#ifdef`，新增输入系统支持只需加实现类。

**严重级别：⚠️ 中等**（代码质量/维护性）

### 5.2 ✅ ScreenToDesign 与根变换反算一致

**位置：** `LoomInputCollector.cs:26-43`

与 `LoomStageDriver.ComputeRootTransform:273-288` 完全逆映射，使用相同的 sf（shrink-to-fit 缩放）、offX/offYTop（居中偏移）。防御：safeArea 零宽高退回全屏（line 32）、除零保护（line 37）。**计算正确。**

### 5.3 ℹ️ 低 · CollectWheel 重复计算 sf/offX

**位置：** `LoomInputCollector.cs:169-171`

`ScreenToDesign` 内部重新计算 sf、offX——与 `LoomStageDriver.ComputeRootTransform` 内部计算重复。两处公式完全一致，但若将来改公式需同步两处。可将 `(sf, offX, offYTop)` 作为 struct 由 Driver 产出，Collector 直接使用。

### 5.4 ℹ️ 低 · CollectKeys 仅覆盖白名单键

**位置：** `LoomInputCollector.cs:198-208`

`KeyList` 是显式白名单（40 个键），不枚举全部 KeyCode（~400 个）。注释写"绝大多数键业务不关心，白名单够用且省 CPU"。若将来需求要求收集修饰键之外的键（如 F1-F12 功能键），需补充白名单。

---

## 六、SpriteResolver.cs — 图集查询

### 6.1 ✅ Miss 缓存去重正确

**位置：** `SpriteResolver.cs:96-98`

```csharp
if (_warned.Add(path))
    Debug.LogWarning(WarnMessage(path, atlasName, atlas, spriteName));
```

`_warned` 是 `HashSet<string>`，`Add` 返回 true 表示首次插入。同一 miss 路径只 warn 一次。命中后 `_warned.Remove(path)`（line 93），下次再 miss 会重 warn。**逻辑正确。**

### 6.2 ✅ 不会陷入"永远找不到不报错"的静默失败

- atlasName 为 null（line 150）：warn "顶层子目录无映射且未配 default 图集"
- atlas 加载失败（line 152）：warn "图集加载失败"
- atlas 有但 sprite 不存在（line 154）：warn "图集无此 sprite"
- 所有路径最终 return null → 调用方 fallback 到 `Texture2D.whiteTexture`（`MirrorPool.cs:176`）

**没有静默失败路径。**

### 6.3 ℹ️ 低 · atlas 缓存无过期机制

**位置：** `SpriteResolver.cs:130-137`

`_atlasCache` 缓存的 `SpriteAtlas` 引用永远不会清除。如果构建系统支持运行时热更新 atlas，需要新增 `InvalidateAtlas(string atlasName)` 方法。

### 6.4 ℹ️ 低 · 字体 atlas 路径硬编码契约

**位置：** `LoomStage.cs:236`（生成端）与 `SpriteResolver.cs:109`（消费端）

```csharp
// LoomStage.cs:236
string path = $"loomgui://font-atlas/p{page}";

// SpriteResolver.cs:109
public void RegisterFontAtlasPage(string path, Texture2D tex)
```

path 格式 `loomgui://font-atlas/p{n}` 在两个类之间无显式常量。若改格式需同步两处，建议提取为 `FontAtlasPath.Format(page)`。

---

## 七、ClipMath.cs — Clip 计算

### 7.1 ✅ 公式正确，防御完备

- 两角 design → world 经 `root.TransformPoint`（含 y-flip scale）统一处理（line 29-30）
- `SafeBlank` 返回 `(-2,-2,0,0)` 在 shader 端 step(2,1)=0 全 discard（line 21）
- 防除零：`hw==0 || hh==0` 返回 safe-blank（line 37）
- 注释中 shader 端验证公式逐行说明（line 13-17）

**无问题。**

---

## 八、NativeHostManager.cs — 外部 GO 绑定

### 8.1 ✅ Clear/Unbind 正确防止 GO 被连带销毁

**位置：** `NativeHostManager.cs:126-143`（Unbind）、`NativeHostManager.cs:145-167`（Clear）

销毁 wrapper GO 前，先将 user GO 重新 parent 到 `_container`（line 134、line 153-154），避免 `Destroy(wrapper)` 递归销毁 user GO。**保护调用方的 GO 所有权。**

### 8.2 ℹ️ 低 · SetLayerRecursive 双重设置 GO 自身 layer

**位置：** `NativeHostManager.cs:60-64`

```csharp
go.layer = layer;                                   // 第一次
foreach (Transform t in go.GetComponentsInChildren<Transform>(true))
    t.gameObject.layer = layer;                     // go 自身再设一次
```

`GetComponentsInChildren(true)` 包含自身。微小 inefficiency，不影响正确性。

### 8.3 ℹ️ 低 · Sync 每帧 FFI 三次调用

**位置：** `NativeHostManager.cs:189-213`

每个绑定节点每帧调 3 个 FFI：`get_node_visible`、`get_node_world_matrix`、`get_node_sort_key`。若绑定数百节点，FFI 调用开销显著。可合并为一个 FFI 调用返回三个值，或在 blob 中预留 native_host 列（类似现有 SOA 列）。当前场景绑定节点极少（通常 ≤5），暂不紧急。

---

## 九、LoomStage.cs — Stage 门面

### 9.1 ✅ ArrayPool 租用/归还正确

**位置：** `LoomStage.cs:167-170`（Rent）、`LoomStage.cs:539-543`（Return）、`LoomStage.cs:222-239`（字体 atlas 页 Rent/Return）

`_frameBuf` 和字体 atlas 页缓冲区均用 `ArrayPool<byte>.Shared.Rent`/`Return`，finally 中归还（atlas 页）。**无泄漏。**

### 9.2 ℹ️ 低 · SyncFontAtlas 每页创建新 Texture2D

**位置：** `LoomStage.cs:230-232`

```csharp
var tex = new Texture2D((int)w, (int)h, TextureFormat.R8, false);
```

同一页每帧脏时创建新 `Texture2D`，旧纹理由 GC 回收。注释 line 107-108 说 "旧 tex 无其他持有者，GC 释放不阻塞渲染"。可以用 `tex.Reinitialize(w, h)` 复用已有实例避免 GC。

---

## 十、与 Unity 的耦合度分析

### 10.1 各文件耦合情况

| 文件 | Unity 耦合度 | 可复用到 Godot 后端？ |
|------|-------------|----------------------|
| **FrameBlob.cs** | 无（纯数据解析） | ✅ 完全可复用（`BitConverter` 和 `System.Text` 是 BCL） |
| **LoomEventHandler.cs** | 低（仅 Native FFI 调用） | ✅ 可复用（替换 Native 调用为 Godot FFI） |
| **LoomSettings.cs** | 高（`ScriptableObject`） | ❌ 需替换为 Godot Resource/配置方案 |
| **SpriteResolver.cs** | 高（`SpriteAtlas`、`Sprite`、`Texture2D`） | ❌ 需替换为 Godot 图集 API |
| **MaterialManager.cs** | 高（`Material`、`Shader`、`MaterialPropertyBlock`） | ❌ Godot 用 `ShaderMaterial` 替代 |
| **MirrorPool.cs** | 高（`GameObject`、`MeshFilter`、`MeshRenderer`、`Mesh`） | ❌ Godot 用 `MeshInstance2D`/`Node2D` 替代 |
| **ClipMath.cs** | 中（`Transform.TransformPoint`） | ⚠️ 可复用公式，API 替换 |
| **LoomInputCollector.cs** | 高（`Input`、`Mouse`、`Touch`、`Screen`） | ❌ Godot 用 `InputEvent` 系统替代 |
| **NativeHostManager.cs** | 高（`GameObject`、`Renderer`、`Transform`） | ❌ 需整体重写 |
| **LoomStage.cs** | 高（`Texture2D`、`Shader`、`Native FFI`） | ❌ 需重写渲染和字体部分，透传 API 可复用 |
| **LoomStageDriver.cs** | 高（`MonoBehaviour`、`Camera`、`Screen`） | ❌ 需重写为 Godot Node |

### 10.2 可复用核心资产

1. **FrameBlob.cs** — 零依赖，直接复用
2. **LoomEventHandler.cs** — 事件路由逻辑纯 C#，替换 P/Invoke stub 即可
3. **LoomEvent / EventContext / EventBridge** — 纯数据结构和路由模型
4. **ClipMath 公式逻辑** — 抽离 TransformPoint 调用后可复用

---

## 十一、总结 — 发现清单

| 编号 | 严重级别 | 文件:行号 | 问题 | 修复方向 |
|------|---------|-----------|------|---------|
| R1 | 🔴 严重 | LoomEventHandler.cs:205-208,262-265 | `Marshal.PtrToStructure` 违规（IL2CPP 对齐坑） | 换 `Span<byte>` + `BinaryPrimitives` 手动读字段 |
| R2 | ⚠️ 中等 | FrameBlob.cs:84-91 | `ColorMatrix` 每次 new float[20] | 复用一个 float[20] 缓冲（与 MeshSegment alloc 一起审查） |
| R3 | ⚠️ 中等 | MirrorPool.cs:72-75 | reuseKey 0↔>0 过渡时 GO 销毁+重建 | 文档注明 reuseKey 绑定后不变即可，暂不需代码改动 |
| R4 | ⚠️ 中等 | MaterialManager.cs:50-55 | SetClipBox O(n) 遍历全部材质 | 加 `_materialsByCtx` 索引（maskContext 增多时再优化） |
| R5 | ⚠️ 中等 | LoomInputCollector.cs:54-100, etc | 4 段 `#ifdef` 重复代码 | 抽象 `IPointerSource` interface 消除分支 |
| R6 | ℹ️ 低 | FrameBlob.cs:118-128 | ReadPath 线性重扫 path table | 加 `Dictionary<uint,string>` 缓存 |
| R7 | ℹ️ 低 | LoomEventHandler.cs:311 | AncestorChain 每事件 new List | 用可复用 List 缓冲 |
| R8 | ℹ️ 低 | LoomEventHandler.cs:139 | _parentCache 动态树变更后可能过期 | 删缓存（FFI 查询本身极快）或加失效机制 |
| R9 | ℹ️ 低 | LoomStage.cs:230-232 | SyncFontAtlas 每页 new Texture2D | 用 `tex.Reinitialize(w,h)` 复用实例 |
| R10 | ℹ️ 低 | NativeHostManager.cs:60-64 | SetLayerRecursive 双重设自身 layer | 用 `for (int i=1; ...)` 跳过自身，或用 `GetComponentsInChildren` 语义 |
| R11 | ℹ️ 低 | MirrorPool.cs:281 | RemapMeshUvToSprite 分配临时 List | 用类级可复用 List<Vector2> |

### 必须修复（CI 阻塞级）

仅 **R1**（`Marshal.PtrToStructure`）是必须修的问题——IL2CPP 构建会崩。其余均为优化/健壮性改进。

### 整体评价

C# 后端整体质量较高：渲染管线（FrameBlob→MirrorPool→MaterialManager）数据流清晰，SOA blob 解析正确，双 hash 变更检测机制被正确消费（ChangeLevel 三分支），事件路由 Capture/Bubble 实现与 DOM/W3C/fgui 一致，GO 生命周期管理无泄漏（stale 销毁、ArrayPool 归还、EditMode safe destroy）。主要需关注 R1 的 IL2CPP 兼容性。
