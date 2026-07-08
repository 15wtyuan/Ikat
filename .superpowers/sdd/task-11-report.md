# Task 11 Report: rich_link_at FFI + Unity 链接事件派发

## 状态
DONE

## 实现

### Step 1: core `Stage::rich_link_at`
`loomgui_core/src/stage.rs:314-354`：pull 查询方法。
- 取节点 world_matrix（复用 `get_node_world_matrix` 的 `world_transforms` 通路，独立于 merge blob）
- `transform::inverse` + `apply_point` 反变换 (world_x, world_y) → 节点本地 (lx, ly)
- 校验 `NodeKind::RichText`（非 RichText 早返 0）
- 扫 `scene.rich_fragments[node.index()]` 的本地坐标矩形 rect.contains → 返 link_id（0=无/越界/无 fragment）
- 导入：`stage.rs` 顶部 `use crate::scene::node::{..., NodeKind, ...}`（之前未导入）

### Step 2: FFI extern
`loomgui_ffi_c/src/lib.rs`（紧跟 `loomgui_stage_set_rich_text` 之后）：
```rust
#[no_mangle]
pub extern "C" fn loomgui_stage_rich_link_at(
    h: *const StageHandle,
    node_id: u32,
    x: f32,
    y: f32,
) -> u32
```
- `*const StageHandle`（只读 pull 查询）；null 句柄早返 0（不 panic，no-panic 约定）
- 不改 EventRecord ABI（pull 模式独立于 hit_test）

### Step 3: 重编 dll + nm + 拷贝 + 确认 binding
```
cargo build -p loomgui_ffi_c --release  → Finished release profile in 14.53s
```

**符号验证**（nm/findstr 不可用 → PowerShell byte-scan）：
```
$bytes = [IO.File]::ReadAllBytes("target/release/loomgui_ffi_c.dll")
([regex]::Matches([Text.Encoding]::ASCII.GetString($bytes), "loomgui_stage_rich_link_at")).Count
→ rich_link_at occurrences: 1
```

**md5 一致**：
```
edf262be7b52fb1cb35116290bdffebc *target/release/loomgui_ffi_c.dll
edf262be7b52fb1cb35116290bdffebc *loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

**csbindgen C# P/Invoke 生成**（`LoomGUIBindings.cs:454-455`）：
```csharp
[DllImport(__DllName, EntryPoint = "loomgui_stage_rich_link_at", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
internal static extern uint loomgui_stage_rich_link_at(StageHandle* h, uint node_id, float x, float y);
```
grep 计数：`grep -c loomgui_stage_rich_link_at LoomGUIBindings.cs` = 2（EntryPoint + 方法名）。

### Step 4: Unity LoomEventHandler Click 分支
`loomgui_unity_package/Runtime/LoomEventHandler.cs`：
1. **字段**（:144）：`readonly Dictionary<uint, Action<uint>> _linkListeners = new();`
2. **Clear()**（:154）：加 `_linkListeners.Clear();`
3. **API**（:180-184）：
   ```csharp
   public void AddLinkClickListener(uint linkId, Action<uint> cb) => _linkListeners[linkId] = cb;
   public void RemoveLinkClickListener(uint linkId) => _linkListeners.Remove(linkId);
   void DispatchLinkClick(LoomEvent evt, uint linkId) {
       if (_linkListeners.TryGetValue(linkId, out var cb)) cb.Invoke(linkId);
   }
   ```
4. **Click 分支**（:222-229）：拆 Up/Click（原合并 fallthrough）。Click 先查 `Native.loomgui_stage_rich_link_at((StageHandle*)_handle, evt.nodeId, evt.x, evt.y)` → >0 走 `DispatchLinkClick(evt, linkId)` 并 break；否则 fallthrough 到 `BubbleRoute(evt)`。evt.x/evt.y 是 design（世界）坐标直接传 core（core 内部反变换）。

link_id = 1-based 文档序（业务侧按 markup 里 `<a>` 出现顺序维护 link_id→href 映射，core 不存 href）。

## 测试

### 新增单测（`loomgui_core/src/stage/dynamic_tests.rs`，不依赖 parse feature）
1. `rich_link_at_returns_link_id_on_hit` — 命中 fragment 返 link_id；越界返 0
2. `rich_link_at_non_rich_text_returns_zero` — 非 RichText（div=Container）返 0
3. `rich_link_at_invalid_node_returns_zero` — 失效 NodeId 静默返 0（FFI no-panic 约定）
4. `rich_link_at_inverse_transforms_world_point` — 节点有 translate(100,50) transform，世界 (130,65) → 本地 (30,15) 命中 fragment

### 回归
```
cargo test  → 583 core + 30 fence_contract + 62 ffi + 17 pkg + ... 全 PASS（0 fail）
cargo fmt --all -- --check  → clean
cargo clippy --all-targets -- -D warnings  → clean
```

## Unity C# 一致性验证（本机无 Unity 编译）
grep 确认改动一致：
- `_linkListeners` 字段 + Clear() + AddLinkClickListener/RemoveLinkClickListener/DispatchLinkClick + Click 分支调 `Native.loomgui_stage_rich_link_at` → DispatchLinkClick
- 无 dangling 引用（所有新符号均定义+引用闭环）
- `StageHandle*` cast 与现有 `Native.loomgui_node_parent` 等同模式（unsafe 域内）

## Concerns
无。本机无 Unity 编译器，C# 编译正确性待家里机 PlayMode 验收（按 brief Step 5 验收清单 8 项逐项过）。
