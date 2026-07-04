# Task 4 Report — NativeHostManager.Sync 改 FFI 查询

**Status:** DONE_WITH_CONCERNS
**Commit:** `de6f9c2` (branch `worktree-nativehost-ffi-query`, parent `216741c`)

## 改动摘要

- `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs`
  - 顶部加 `using LoomGUI.Bindings;`（访问 `Native` + `StageHandle`）。
  - 删除字段 `_seenThisFrame`（不再用）。
  - `public void Sync(FrameBlob blob)` 整段 → `public unsafe void Sync(StageHandle* stage)`：
    遍历 `_bindings`，对每 nodeId 调 3 个 FFI（visible/world_matrix/sort_key）设 wrapper TRS
    + sortingOrder + SetActive。空 div slot / 无效 NodeId / display:none / RemoveNode →
    `get_node_visible` 返 0 → `SetActive(false)` 后 continue。
- `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs:587`
  - `_nhm.Sync(blob)` → `_nhm.Sync(_stage)`（`_stage` 是 `StageHandle*`，LoomStage 已是 unsafe 类）。
  - `blob` 变量保留（仍传 `_pool.Sync(blob, ...)`，渲染管线不动）。

## Bindings.cs 3 P/Invoke 实际签名（csbindgen 生成，**指针，非 ref**）

源：`loomgui_unity/Assets/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`

```csharp
// namespace LoomGUI.Bindings; internal static unsafe partial class Native
[DllImport(__DllName, EntryPoint = "loomgui_stage_get_node_world_matrix", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
internal static extern void loomgui_stage_get_node_world_matrix(StageHandle* h, uint node_id, float* out_a, float* out_b, float* out_c, float* out_d, float* out_tx, float* out_ty);

[DllImport(__DllName, EntryPoint = "loomgui_stage_get_node_sort_key", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
internal static extern void loomgui_stage_get_node_sort_key(StageHandle* h, uint node_id, uint* @out);

[DllImport(__DllName, EntryPoint = "loomgui_stage_get_node_visible", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
internal static extern void loomgui_stage_get_node_visible(StageHandle* h, uint node_id, byte* @out);
```

**注意：参数是 `float*` / `uint*` / `byte*`（裸指针），不是 brief 假设的 `ref float` / `ref uint` / `ref byte`。**
调用模式照 `LoomStage.GetNodeLayoutRect`（LoomStage.cs:147-152）：声明栈局部 unmanaged 标量 + unsafe 块 `&var` 传址。

## NativeHostManager 新 Sync 关键行

```csharp
public unsafe void Sync(StageHandle* stage)
{
    if (_bindings.Count == 0) return;
    if (stage == null) return;
    float sf = Mathf.Abs(_root.localScale.y);

    foreach (var kv in _bindings)
    {
        uint id = kv.Key;
        var go = kv.Value;
        if (go == null) continue;

        // visible skip
        byte vis = 0;
        Native.loomgui_stage_get_node_visible(stage, id, &vis);
        if (vis == 0) { if (go.activeSelf) go.SetActive(false); continue; }

        // world_matrix → TRS 分解
        float a = 0, b = 0, c = 0, d = 0, tx = 0, ty = 0;
        Native.loomgui_stage_get_node_world_matrix(stage, id, &a, &b, &c, &d, &tx, &ty);
        if (!_wrappers.TryGetValue(id, out var wrapper) || wrapper == null) continue;
        float rot = Mathf.Atan2(b, a) * Mathf.Rad2Deg;
        float sx = Mathf.Sqrt(a * a + b * b);
        float sy = Mathf.Sqrt(c * c + d * d);
        wrapper.transform.localPosition = new Vector3(tx, -ty, 0);
        wrapper.transform.localRotation = Quaternion.Euler(0, 0, rot);
        wrapper.transform.localScale = new Vector3(sx, sy, sf > 0.0001f ? 1.0f / sf : 1.0f);

        // sort_key → sortingOrder
        uint sk = 0;
        Native.loomgui_stage_get_node_sort_key(stage, id, &sk);
        foreach (var r in go.GetComponentsInChildren<Renderer>())
            if (r != null) r.sortingOrder = (int)sk;
        if (!go.activeSelf) go.SetActive(true);
    }
}
```

## LoomStage 调用点（line 585-587）

```csharp
var blob = new FrameBlob(_frameBuf);
_pool.Sync(blob, transform, _mm, _sprites, Texture2D.whiteTexture, _font);
_nhm.Sync(_stage);
```

`_stage` 类型 `StageHandle*`（LoomStage.cs:57），LoomStage 已 `public sealed unsafe class`，传指针 OK。
`blob` 仍传 `_pool.Sync`，渲染管线不动。

## 类型/语法自查

1. **unsafe 修饰**：NativeHostManager 类本身未标 unsafe（`internal sealed class NativeHostManager`）。
   `Sync` 方法标 `public unsafe void`（方法级 unsafe），允许方法体内用 `StageHandle*` + `&var`。
   与 LoomStage 的 `unsafe class` 不同模式（class-level vs method-level），均合法。
2. **`using LoomGUI.Bindings;`**：`Native`（`internal static unsafe partial class`）和 `StageHandle`
   （`internal unsafe partial struct`）都在 `LoomGUI.Bindings` 命名空间。同 assembly（`internal` 可见），
   加 using 后 `Native.loomgui_*` + `StageHandle*` 直接可用。
3. **指针参数匹配**：
   - `&vis`（byte）→ `byte*` ✓
   - `&sk`（uint）→ `uint*` ✓
   - `&a,&b,&c,&d,&tx,&ty`（6 个 float）→ 6 个 `float*` ✓
4. **栈局部取址（CS0213）**：unmanaged 标量（byte/uint/float）栈局部直接 `&` 取址，
   无需 `fixed` 块（与 LoomStage.cs:143,152 既有模式一致，已有注释确认）。
5. **删除项**：`_seenThisFrame` 字段 + 旧 blob 遍历逻辑（NodeCount/NodeId(i)/Ma/Mb/.../SortKey）
   全删。grep 确认 NativeHostManager.cs 无 `blob.` / `_seenThisFrame` / `FrameBlob` 残留代码
   （仅注释中一句"不再遍历 blob"）。
6. **调用点**：grep `_nhm\.Sync` 全 assembly 仅 1 处（LoomStage.cs:587），已改 `_nhm.Sync(_stage)`。

## Concerns（告知用户侧）

1. **P/Invoke 签名指针 vs ref**：brief 假设 `ref byte`/`ref uint`/`ref float`，实际 csbindgen 生成
   `byte*`/`uint*`/`float*`（裸指针）。我按实际签名用 `&var` 调用。若用户侧因 csbindgen 版本差异
   重新生成 Bindings.cs 时变成 `ref`，需相应改调用（`ref vis` 替 `&vis` 等）。当前入库 .cs 是
   指针版，与 .dll 配套——**不要单独重生成 Bindings.cs**（会与已 commit 的 dll 不一致）。
2. **未跑 Unity 编译**：worktree 无 Unity，无法 here 跑 C# 编译。语法/类型对照自查通过（P/Invoke 签名、
   unsafe 修饰、命名空间 using、栈局部取址模式均与既有代码一致）。真实编译验证在 Task 5 / 用户侧
   Unity focus。
3. **`_root` null 风险**：`Sync` 开头 `_root.localScale.y` 若 `_root` 未 Init 会 NPE——但旧 Sync 同样
   依赖此前提（无回归）。Init 必先于 Sync 被调（LoomStage.Awake 链）。
4. **遍历顺序变化**：旧 Sync 按 blob 顺序遍历，新 Sync 按 `_bindings` dict 顺序。每帧每节点独立设
   wrapper TRS + sortingOrder + SetActive，无跨节点依赖 → 顺序变化无副作用。
5. **blob 仍读但不传 _nhm**：LoomStage 仍 `borrow_frame` → `new FrameBlob` → `_pool.Sync(blob)`。
   `_nhm` 不再消费 blob。FrameBlob 类未删（仍服务于 MirrorPool）。
