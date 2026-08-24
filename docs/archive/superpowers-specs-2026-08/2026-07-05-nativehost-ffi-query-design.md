# NativeHost FFI 查询口子设计契约

> 日期：2026-07-05
> 范围：修 v1d.3 NativeHost-lite 的隐藏假设漏洞——空 div slot 不进渲染 blob，后端拿不到 transform。方案 3：NativeHost Sync 走 FFI 按 nodeId 查询，不再遍历 blob。
> 模式：core 加 3 个 getter（复用已有 world_transforms + 新快照 node_sort_keys）+ 3 个 FFI extern + C# Sync 改查询模式。**渲染管线（blob/FrameData/payload/dirty hash/merge）零改。**

## 1. 背景 / 根因

NativeHost-lite（v1d.3）契约：外部 GO 跟随某 div（slot）的 world transform + 显隐 + 排序。slot 通常是**空 div**（UI 占位框，3D GO 挂上去）。

**漏洞**：v1d.3 假设后端能从 `frame.nodes`（blob）拿到任意绑定节点的 transform。但 blob 是**渲染列表**，受 `merge_meshes` 影响——空 div（透明 Container）的 RenderNode 被 merge 合并到相邻透明 mesh → 独立 entry 从 blob 消失。验证（`dump_nativehost_slot` example）：

```
nh-stage runtime NodeId=Some(40961)        ← scene 里有
frame.nodes=10 (render blob), scene.nodes=17
结论：nh-stage NOT IN frame.nodes（blob）   ← blob 里没有（被 merge 吞了）
```

`nh-effect`/`nh-anim`（有 bg）进 blob，`nh-stage`（无 bg 空 div）不进。后端 `Sync` 遍历 blob 找不到 nh-stage → wrapper 永不设置 → 角色永远 `SetActive(false)`。

**深层问题**：blob 身兼二职——既是渲染指令列表（受 merge 优化），又是节点 transform 通道（NativeHost 用）。空 div 在前者被合并消失，但在后者存在（有布局位置 + world transform）。

## 2. 目标 / 非目标

**目标**
- NativeHost Sync 改用 FFI 按 nodeId 查询 world_matrix / sort_key / visible，不再遍历 blob。
- 空 div slot（被 merge 吞的）的 transform 通道恢复——NativeHost 任何 div 都能当 slot。
- blob / 渲染管线零改（merge 等优化不动）。

**非目标**
- 不改 blob 结构 / FrameData / payload / dirty hash / merge / FFI 契约（除加 3 个 getter）。
- 不做 NativeHost v2 完整版（hit/clip/size push——roadmap §5.3 仍 defer）。本 spec 只补 v1d.3 漏掉的 transform 通道。
- 不改 compute_world_transforms（已填全节点 world_transforms）。
- 不动 NativeHostManager Bind/Unbind/_container（含 Unbind reparent fix）。

## 3. 设计：FFI 查询口子（方案 3）

NativeHost Sync 的数据源从"blob 渲染列表"切到"按 nodeId 查询"。三个查询口子复用 core 已有数据 + 一个新快照：

### 3.1 world_matrix（复用已有）

`scene.world_transforms: Vec<Affine2>`（按 `NodeId.index()` 索引，compute_world_transforms 填**全节点**，含空 div）。独立于 merge。

```rust
// stage.rs 新增
pub fn get_node_world_matrix(&self, node: NodeId) -> Option<Affine2> {
    let scene = self.scene.as_ref()?;
    scene.get(node)?;                      // 校验 node 存在（gen 校验）
    scene.world_transforms.get(node.index()).copied()
}
```

### 3.2 sort_key（新快照）

`assign_sort_keys`（render/batch.rs:118）DFS 遍历 scene 给**每个节点**分配递增 sort_key（counter），但 `merge_meshes` 后空 div 的 sort_key 随 entry 消失。加 `Scene::node_sort_keys: Vec<u32>` 在 merge 前**快照**保存。

```rust
// scene/node.rs Scene 加字段
pub node_sort_keys: Vec<u32>,   // 按 NodeId.index()，assign_sort_keys 填（merge 前快照）

// render/batch.rs assign_sort_keys 写快照（签名加 scene_sort_keys: &mut Vec<u32> 或返回）
// DFS 内 line 166 附近：rn.sort_key = *counter; 同步 node_sort_keys[id.index()] = *counter;
```

```rust
// stage.rs
pub fn get_node_sort_key(&self, node: NodeId) -> Option<u32> {
    let scene = self.scene.as_ref()?;
    scene.get(node)?;
    scene.node_sort_keys.get(node.index()).copied()
}
```

### 3.3 visible

节点存在 + 非 `display:none`。

```rust
// stage.rs
pub fn get_node_visible(&self, node: NodeId) -> bool {
    let scene = match self.scene.as_ref() { Some(s) => s, None => return false };
    match scene.get(node) {
        None => false,                                  // RemoveNode / 无效 → false
        Some(n) => n.style.display != Display::None,    // display:none → false
    }
}
```

> overflow 裁剪 / 祖先 hidden 等更细的 visible 语义不在本 spec（YAGNI，demo 不需要）。RemoveNode 场景由 `get_node_world_matrix → None` 自然覆盖（Sync 见 §5）。

## 4. FFI 契约（csbindgen 自动生成 C# 绑定）

3 个 `#[no_mangle] extern "C"` getter（loomgui_ffi_c/src/lib.rs）。返回 i32 状态码（0=OK，-1=node 无效），out 参数填值。

```rust
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_world_matrix(
    h: *const StageHandle, node_id: u32, out: *mut f32,  // out[0..6] = a,b,c,d,tx,ty
) -> i32;

#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_sort_key(
    h: *const StageHandle, node_id: u32, out: *mut u32,
) -> i32;

#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_visible(
    h: *const StageHandle, node_id: u32, out: *mut u8,   // 0/1
) -> i32;
```

csbindgen 在 build.rs 自动生成 C# 绑定（`LoomGUIBindings.cs`）。**改 FFI 签名后 push 前自查 dll 导出**（坑 100）：`nm/findstr` 查新符号在 dll 里再 push。

## 5. C# 改造

### 5.1 NativeHostManager.Sync 改查询模式

**当前**（NativeHostManager.cs:124）：遍历 blob 找 binding 的 nodeId → 设 wrapper TRS from blob。

**改**：遍历 `_bindings` → 对每个 nodeId 调 3 个 FFI getter → 设 wrapper。不再收 blob。

```csharp
// NativeHostManager.Sync 签名改：收 StageHandle（FFI 句柄），不收 blob
public void Sync(IntPtr stageHandle) {
    float sf = Mathf.Abs(_root.localScale.y);   // root (sf,-sf,sf) → 取 |y|（现有逻辑保留）
    float[] m = new float[6];
    foreach (var kv in _bindings) {
        uint id = kv.Key;
        var go = kv.Value;
        if (go == null) continue;
        // world_matrix
        if (LoomGUIBindings.loomgui_stage_get_node_world_matrix(stageHandle, id, m) != 0) {
            go.SetActive(false); continue;   // node 无效（RemoveNode）→ 藏
        }
        // visible
        byte vis = 0;
        LoomGUIBindings.loomgui_stage_get_node_visible(stageHandle, id, out vis);
        if (vis == 0) { go.SetActive(false); continue; }
        // 设 wrapper TRS（复用现有 TRS 分解逻辑：line 135-146）
        float a=m[0], b=m[1], c=m[2], d=m[3];
        float rot = Mathf.Atan2(b, a) * Mathf.Rad2Deg;
        float sx = Mathf.Sqrt(a*a + b*b), sy = Mathf.Sqrt(c*c + d*d);
        var wrapper = _wrappers[id];
        wrapper.transform.localPosition = new Vector3(m[4], -m[5], 0);
        wrapper.transform.localRotation = Quaternion.Euler(0, 0, rot);
        wrapper.transform.localScale = new Vector3(sx, sy, sf > 0.0001f ? 1.0f/sf : 1.0f);
        // sort_key
        uint sk = 0;
        LoomGUIBindings.loomgui_stage_get_node_sort_key(stageHandle, id, out sk);
        foreach (var r in go.GetComponentsInChildren<Renderer>())
            if (r != null) r.sortingOrder = (int)sk;
        if (!go.activeSelf) go.SetActive(true);
    }
}
```

> 现有 Sync 的 TRS 分解（line 135-146）逻辑保留，仅数据源从 `blob.Ma(i)/Mb(i)/...` 切到 `m[0..5]`。

### 5.2 LoomStage.LateUpdate 调用点

当前 `_nhm.Sync(blob)`（LoomStage.cs LateUpdate 内，MirrorPool.Sync 后）。改 `_nhm.Sync(_ffiHandle)`（传 Stage FFI 句柄）。blob 仍传给 MirrorPool（渲染管线不动）。

### 5.3 不变
- NativeHostManager.Bind / Unbind / Clear / _container / CacheRenderers（含 ParticleSystemRenderer）
- driver（LoomShowcaseDriver）调用点不变（BindNativeHost/UnbindNativeHost）

## 6. 验证

### 6.1 dump_nativehost_slot example 改造验 core 数据
example 已存（loomgui_core/examples/dump_nativehost_slot.rs）。扩展打印：
- `scene.world_transforms[nh_stage_id.index()]` 非零（位置在 nh-stage 框处，非 (0,0)）
- assign_sort_keys 后 `node_sort_keys[nh_stage_id.index()]` 非零（DFS 序号）
- 对比 frame.nodes（blob）仍不含 nh-stage（merge 吞了——证明 blob 不该当通道）

### 6.2 单元测试（core）
- `get_node_world_matrix`：建场景 + 空 div slot → compute_world_transforms → 查 slot 矩阵非 identity 且 tx/ty = slot layout 位置。
- `get_node_sort_key`：建场景 + 空 div slot → tick → 查 slot sort_key 非零 + 与兄弟节点序号符合 DFS。
- `get_node_visible`：display:none 节点 → false；正常 → true；RemoveNode 后 → false（get 返回 None）。
- 无效 NodeId（gen 失效）→ 所有 getter 返回 None。

### 6.3 PlayMode 验收（用户家里机）
- page_nativehost：角色显示在 nh-stage 框位置（不再在屏幕角上）+ scale 对（_characterScale）
- 切页回归：返回 home → 角色/粒子消失；再进 page_nativehost → 复现（Unbind fix + 缓存实例）
- page_controls §1.6 model-slot：同样显示角色实例
- 粒子：放光效 toggle + 朝向未被 y-flip 带歪 + sortingOrder 与 UI mesh 排序
- Frame Debugger：外部 GO 在 Transparent(3000) 队列

## 7. 风险 / 坑

- **assign_sort_keys 签名改动**：当前 `(scene: &Scene, nodes, id_to_pos)`，要写 `node_sort_keys` 需 `&mut` 或返回。改签名 + 在 render/mod.rs:260 调用点把 sort_keys 存进 scene（或 build_render_nodes 返回 + Stage tick 存）。注意 borrow（scene 与 nodes 同时 &mut）。
- **node_sort_keys 容量**：按 NodeId.index()，需和 world_transforms 同步扩容（节点增删时）。compute_world_transforms 已处理 world_transforms 扩容，node_sort_keys 同模式。
- **FFI 入口绝不 panic**（坑 102）：getter 用 `match ... None → 返回 -1`，不 `.expect`/`unwrap`。无效 node_id（含 gen 失效）→ -1，C# SetActive(false)。
- **dll 重编 + push 自查**（坑 100）：改 FFI 后 `cargo build -p loomgui_ffi_c --release` + 拷 dll + `nm/findstr` 查 3 新符号。家里机当前会话能编 dll（cargo 在）。
- **改 FFI 签名后 csbindgen 重新生成 C# 绑定**（坑 35）：build.rs 跑后 LoomGUIBindings.cs 自动更新 3 个新 P/Invoke 签名。C# 镜像（FrameBlob/Sync wrapper）手补。
- **NativeHostManager.Sync 签名 Breaking**：从 `Sync(FrameBlob)` 改 `Sync(IntPtr stageHandle)`。LoomStage 调用点改。其他 caller？grep 确认只有 LoomStage.LateUpdate 调。

## 8. 不做的事（YAGNI）

- 不做 hit / clip / size push（NativeHost v2，roadmap §5.3）。
- 不改 blob 渲染管线（merge 优化保留——它是渲染优化，不该因 NativeHost 让步）。
- 不做 overflow 裁剪感知的 visible（display:none 足够 demo）。
- 不做"外部 GO 与渲染邻居精确对齐排序"——sort_key 用 DFS 序号（slot 在树的 DFS 位置），不追求与 merge 后 mesh 的精确邻居关系（demo 不需要）。
