## Task 7 Report: LoomStage.cs 虚拟列表 driver API

**Status**: done
**Commit**: `33e8aa5` (branch `worktree-v1.4b-absolute-virtual-list`)

### 修改的文件

`loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs` -- 在 `SetScrollPos`（line 126）后新增 25 行（5 个 public 方法）。

### 5 个 driver API

```csharp
// v1.4-b：虚拟列表 driver API（转调 FFI，T5 生成）。
public void SetContentSize(uint node, float w, float h)
    => Native.loomgui_stage_set_content_size(_stage, node, w, h);

public void ClearContentSizeOverride(uint node)
    => Native.loomgui_stage_clear_content_size_override(_stage, node);

public (float x, float y) GetScrollPos(uint node)
{
    float x = 0f, y = 0f;
    unsafe { fixed (float* px = &x, py = &y) Native.loomgui_stage_get_scroll_pos(_stage, node, px, py); }
    return (x, y);
}

public (float x, float y, float w, float h) GetNodeLayoutRect(uint node)
{
    float x = 0f, y = 0f, w = 0f, h = 0f;
    unsafe { fixed (float* px = &x, py = &y, pw = &w, ph = &h)
        Native.loomgui_stage_get_node_layout_rect(_stage, node, px, py, pw, ph); }
    return (x, y, w, h);
}

public void SetReuseKey(uint node, uint key)
    => Native.loomgui_stage_set_reuse_key(_stage, node, key);
```

### 5 个 P/Invoke 签名核对（LoomGUIBindings.cs）

| LoomStage 方法 | 调的 P/Invoke | out 参数 |
|---|---|---|
| `SetContentSize` (L130) | `loomgui_stage_set_content_size(StageHandle*, uint, float, float)` | 无 |
| `ClearContentSizeOverride` (L133) | `loomgui_stage_clear_content_size_override(StageHandle*, uint)` | 无 |
| `GetScrollPos` (L138) | `loomgui_stage_get_scroll_pos(StageHandle*, uint, float*, float*)` | `float*` |
| `GetNodeLayoutRect` (L146) | `loomgui_stage_get_node_layout_rect(StageHandle*, uint, float*, float*, float*, float*)` | `float*` x4 |
| `SetReuseKey` (L151) | `loomgui_stage_set_reuse_key(StageHandle*, uint, uint)` | 无 |

全部 5 个 P/Invoke 用 `float*` 非 `ref float`（与 brief 差异已纠正）。

### grep 验证

```
$ grep -n 'SetContentSize\|ClearContentSizeOverride\|GetScrollPos\|GetNodeLayoutRect\|SetReuseKey' LoomStage.cs
129:        public void SetContentSize(uint node, float w, float h)
132:        public void ClearContentSizeOverride(uint node)
135:        public (float x, float y) GetScrollPos(uint node)
142:        public (float x, float y, float w, float h) GetNodeLayoutRect(uint node)
150:        public void SetReuseKey(uint node, uint key)
```

5 个方法全部在 LoomStage.cs 中，各自调正确的 `Native.loomgui_stage_*` P/Invoke。

### concerns

- 无 `_stage == null` guard（3 个 expression-bodied 方法直接透传 `_stage` 给 FFI）。依赖 Rust FFI 侧 null 守卫（坑 102 要求 FFI 入口不 panic）。如果 Rust 侧未对 null `StageHandle` 做防御，null _stage 时 SetContentSize/ClearContentSizeOverride/SetReuseKey 会传空指针进 FFI 导致 crash。建议后续在 Rust 侧确认这些 FFI 入口有 null 检查，或在 C# 侧补 guard。
- `GetScrollPos` 与已有 `SetScrollPos` 方法名相邻但功能不同（读写分离），无命名冲突。
- LoomStage.cs class 已标 `sealed unsafe class`，`fixed` 语句在 `unsafe { }` 块内语法正确。

### 家里机待编译项

- Unity 打开 `loomgui_unity/` → PlayMode 验证编译通过 + 无运行时异常。
- 确认 `loomgui_ffi_c.dll` 已是最新（T5 产出），不然 5 个 API 调用会 DllNotFoundException/EntryPointNotFound。
