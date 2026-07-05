# NativeHost FFI 查询口子 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** NativeHost Sync 改用 FFI 按 nodeId 查 world_matrix/sort_key/visible，不再遍历 blob——修空 div slot 被 merge_meshes 吞掉导致后端拿不到 transform 的漏洞。

**Architecture:** core 加 3 getter 复用已有 `world_transforms`（全节点）+ 新快照 `node_sort_keys`（assign_sort_keys merge 前填）；FFI 加 3 extern（对齐现有 `get_node_layout_rect` 的 out 参数 + 无状态码惯例）；C# `NativeHostManager.Sync` 改收 StageHandle 遍历 _bindings 查询。渲染管线（blob/FrameData/payload/dirty/merge）零改。

**Tech Stack:** Rust core（taffy/slotmap）+ csbindgen FFI + Unity 6.5 C#。家里机当前会话（cargo 可编 dll）。

## Global Constraints

- **渲染管线零改**：blob/FrameData/NodePayload/dirty hash/merge_meshes 不动。只加 3 getter + 1 Scene 字段 + 1 assign_sort_keys 输出参数。
- **FFI 入口绝不 panic**（坑 102）：getter 用 `match None → 写默认值 0`，不 `.expect`/`unwrap`。无效 node_id（含 gen 失效）→ 写默认（world_matrix identity+0/0、sort_key 0、visible 0）。
- **对齐现有 FFI 惯例**：参考 `loomgui_stage_get_node_layout_rect`（lib.rs:475）——`*const StageHandle` + 独立 `*mut` out 参数 + 无状态码 + null/无效写默认。
- **csbindgen 自动生成 C# 绑定**：新增 `#[no_mangle] extern "C"` 后 `cargo build -p loomgui_ffi_c` 重新生成 `LoomGUIBindings.cs`。`*mut f32` → C# `ref float`（`&x`）；`StageHandle*` 类型化指针。
- **dll 闭环**（坑 100）：改 FFI 后 `cargo build -p loomgui_ffi_c --release` + 拷 dll + `nm/findstr` 查 3 新符号在 dll 里再 push。拷 dll 时 Unity 须关着（锁 dll）。
- **node_sort_keys 扩容**：对齐 `compute_world_transforms` 的 `scene.nodes.capacity() + 1`（slotmap 删后 idx 不变，按 capacity 不按 len——坑：按 len 高 idx 越界 panic）。
- **当前工作树状态**：HEAD=`624a989`（spec commit）。工作树有未提交的 driver 修复（page_controls 实例化 / Animator 自动取 / 粒子 inactive，留本地）+ SampleScene/controller（用户 Unity 配置）+ dump_nativehost_slot.rs（诊断 example，本 plan 扩展用）。subagent 改动只 add 自己改的文件，不误 commit 这些。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `loomgui_core/src/scene/node.rs` | Scene 加 `node_sort_keys: Vec<u32>` 字段 | Modify |
| `loomgui_core/src/render/batch.rs` | `assign_sort_keys` 加 `sort_keys: &mut [u32]` 输出参数，DFS 填 | Modify |
| `loomgui_core/src/render/mod.rs` | `build_render_nodes` 加 sort_keys buffer 参数，传 assign_sort_keys，返回它 | Modify |
| `loomgui_core/src/stage.rs` | tick_and_render 建 sort_keys buffer 调 build_render_nodes，存进 scene.node_sort_keys；加 3 个 `get_node_*` getter | Modify |
| `loomgui_core/src/scene/transform.rs` | （扩容注释提一句 node_sort_keys 同模式，无代码改）| Modify（注释）|
| `loomgui_ffi_c/src/lib.rs` | 加 3 个 `loomgui_stage_get_node_*` extern | Modify |
| `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs` | `Sync` 改收 `StageHandle*` + 遍历 _bindings FFI 查询 | Modify |
| `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs` | LateUpdate `_nhm.Sync(blob)` 改 `_nhm.Sync(_stage)` | Modify |
| `loomgui_core/examples/dump_nativehost_slot.rs` | 扩展验 world_transforms + node_sort_keys | Modify |

---

## Task 1: Scene 加 node_sort_keys + assign_sort_keys 填它

**Files:**
- Modify: `loomgui_core/src/scene/node.rs`（Scene struct 加字段，约 line 230 `world_transforms` 后）
- Modify: `loomgui_core/src/render/batch.rs:118-190`（assign_sort_keys 签名 + DFS 填）
- Modify: `loomgui_core/src/render/mod.rs:96-298`（build_render_nodes 加 buffer + 返回）
- Modify: `loomgui_core/src/stage.rs`（tick_and_render 建 buffer + 存 scene，约 line 473+）
- Test: `loomgui_core/tests/`（新增或既有 batch 测试文件）

**Interfaces:**
- Produces: `Scene::node_sort_keys: Vec<u32>`（按 NodeId.index()，assign_sort_keys 填全节点含空 div）。

- [ ] **Step 1: Scene 加 node_sort_keys 字段**

Edit `loomgui_core/src/scene/node.rs`，在 `pub world_transforms: Vec<crate::transform::Affine2>,`（约 line 230）后加：
```rust
    /// 每节点 sort_key 快照（assign_sort_keys 填，merge 前的 DFS 序号）。index = NodeId.index()。
    /// NativeHost FFI 查询用——merge_meshes 后空 div entry 消失，但 sort_keys 快照保留。
    /// 运行时态，不进 pkg。
    pub node_sort_keys: Vec<u32>,
```

- [ ] **Step 2: Scene::new / 构造点初始化 node_sort_keys**

grep Scene 构造点：`grep -rn "world_transforms:" loomgui_core/src/scene/`。每处 `world_transforms: Vec::new()`（或 `vec![]`）后加 `node_sort_keys: Vec::new(),`。若 compute_world_transforms 每帧重建 world_transforms（line 15 `let mut worlds: Vec<Affine2> = vec![IDENTITY; cap+1]` 后 `scene.world_transforms = worlds`），node_sort_keys 由 tick_and_render 单独管（Step 4）。

- [ ] **Step 3: assign_sort_keys 加 sort_keys 输出参数**

Edit `loomgui_core/src/render/batch.rs:118`，签名 + DFS 加输出。签名改：
```rust
pub fn assign_sort_keys(
    scene: &Scene,
    nodes: &mut [RenderNode],
    id_to_pos: &std::collections::HashMap<NodeId, usize>,
    sort_keys: &mut [u32],   // 新增：按 NodeId.index() 填，caller 建 cap+1 buffer
) -> Vec<ClipEntry> {
```
DFS 内（line 161-169 块，`rn.sort_key = *counter` 那里）同步填 sort_keys：
```rust
        {
            let pos = *id_to_pos.get(&id).expect("live node 在 id_to_pos 中");
            let rn = &mut nodes[pos];
            rn.sort_key = *counter;
            sort_keys[id.index()] = *counter;   // 新增：快照保存（merge 前的 DFS 序号）
            *counter += 1;
        }
```
DFS 递归函数签名（line 125 `fn dfs(...)`）也要传 `sort_keys: &mut [u32]`，递归调用（line 183）传它。

- [ ] **Step 4: build_render_nodes 建 sort_keys buffer + 返回**

Edit `loomgui_core/src/render/mod.rs:96`。签名加返回 `node_sort_keys`：
```rust
pub fn build_render_nodes(
    scene: &Scene,
    font: &Font,
    prev: &std::collections::HashMap<u32, (u64, u64)>,
    image_sizes: &ImageSizeTable,
) -> (FrameData, std::collections::HashMap<u32, (u64, u64)>, Vec<u32>) {
    // ... 现有代码 ...
    // 在调 assign_sort_keys 前（line 260 前）建 buffer：
    let mut sort_keys: Vec<u32> = vec![0u32; scene.nodes.capacity() + 1];   // 对齐 world_transforms 扩容
    let clips = batch::assign_sort_keys(scene, &mut nodes, &id_to_pos, &mut sort_keys);
    // ... 现有 merge 等代码 ...
    (FrameData { nodes, clips }, new_hashes, sort_keys)
}
```

- [ ] **Step 5: tick_and_render 存 node_sort_keys 进 scene**

Edit `loomgui_core/src/stage.rs` 的 `tick_and_render`（约 line 473）。找到 `build_render_nodes` 调用点（返回 FrameData 的地方），解构第三个返回值存进 scene：
```rust
let (frame, new_hashes, sort_keys) = crate::render::build_render_nodes(scene, &self.font, &self.prev_node_hashes, &self.image_sizes);
scene.node_sort_keys = sort_keys;
```
注意 borrow：若 build_render_nodes 借 scene 不可变，存 sort_keys 要在它返回后（scene 此时再可变借用）。tick_and_render 持 `&mut self`，scene = `self.scene.as_mut().unwrap()` —— 确认调用顺序不冲突（build_render_nodes 借 scene 结束后再写 scene.node_sort_keys）。

- [ ] **Step 6: 写测试**

在 `loomgui_core/tests/`（找 batch 相关测试文件，或新建）加测试：
```rust
#[test]
fn node_sort_keys_filled_for_empty_div_slot() {
    // 建 scene：root > content > nh-stage(空 div，无 bg) + nh-effect(有 bg)
    // 用 parse_html("<div><div id='nh-stage'></div><div id='nh-effect' style='background-color:#000'></div></div>")
    // + resolve_styles + build_scene + stage.tick_and_render()
    // 查 scene.node_sort_keys[nh_stage.index()] > 0（非零，DFS 序号）
    // 查 node_sort_keys[nh_effect.index()] > 0
    // 两者符合 DFS 序（nh-stage < nh-effect 因 DOM 顺序）
}
```
参考 `loomgui_core/src/render/batch.rs:287+` 现有 `assign_sort_keys` 测试模式建场景。

- [ ] **Step 7: 跑测试**

Run: `cargo test -p loomgui_core node_sort_keys`
Expected: PASS。

- [ ] **Step 8: 跑回归（fence_contract + 全 core）**

Run: `cargo test -p loomgui_core`
Expected: 全过（fence_contract 15 + snapshot 3 + v1e_dirty 2 + 新测试）。

- [ ] **Step 9: Commit**

```bash
git add loomgui_core/src/scene/node.rs loomgui_core/src/render/batch.rs loomgui_core/src/render/mod.rs loomgui_core/src/stage.rs loomgui_core/tests/
git commit -m "$(cat <<'EOF'
feat(core): node_sort_keys snapshot (assign_sort_keys pre-merge)

Add Scene.node_sort_keys (per NodeId.index()). assign_sort_keys fills it
alongside RenderNode.sort_key during DFS — preserving the pre-merge DFS
order for nodes whose RenderNode entries get merged away (empty-div slots).
build_render_nodes returns it; tick_and_render stores it in scene.

NativeHost FFI query (next task) reads this so empty-div slots keep a
sort_key even after merge_meshes collapses their render entry.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Stage 加 3 个 get_node_* getter

**Files:**
- Modify: `loomgui_core/src/stage.rs`（在 `get_node_layout_rect` 约 line 217 后加 3 getter）
- Test: `loomgui_core/tests/`（新增 getter 测试）

**Interfaces:**
- Consumes: `Scene::world_transforms`（Task 1 已存全节点）+ `Scene::node_sort_keys`（Task 1 新增）+ `Node.style.display`
- Produces: 3 个 `pub fn get_node_*`，签名：
  - `get_node_world_matrix(&self, node: NodeId) -> Option<[f32; 6]>`（Affine2 = [f32;6]）
  - `get_node_sort_key(&self, node: NodeId) -> Option<u32>`
  - `get_node_visible(&self, node: NodeId) -> bool`

- [ ] **Step 1: 写失败测试**

在测试文件加：
```rust
#[test]
fn get_node_world_matrix_returns_slot_position() {
    // 建 scene + 空 div slot 在 (100, 200)，tick
    // let wm = stage.get_node_world_matrix(slot_id);
    // assert!(wm.is_some());
    // let m = wm.unwrap();
    // assert_eq!(m[4], 100.0);  // tx
    // assert_eq!(m[5], 200.0);  // ty
}

#[test]
fn get_node_sort_key_returns_dfs_order() {
    // 建 scene + 两兄弟 div，tick
    // 第一个 sort_key < 第二个（DFS 序）
}

#[test]
fn get_node_visible_display_none_false() {
    // 建 scene + display:none div，tick
    // assert!(!stage.get_node_visible(node_id));
}

#[test]
fn get_node_invalid_returns_none() {
    // stage 不建 scene 或用 gen 失效的 NodeId
    // assert_eq!(stage.get_node_world_matrix(NodeId(0xFFFF_FFFF)), None);
    // assert!(!stage.get_node_visible(NodeId(0xFFFF_FFFF)));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core get_node_`
Expected: FAIL（方法不存在）。

- [ ] **Step 3: 加 3 getter**

Edit `loomgui_core/src/stage.rs`，在 `get_node_layout_rect`（line 217-219）后加：
```rust
/// 读节点 world transform（compute_world_transforms 产物，全节点含空 div）。
/// NativeHost FFI 用。node 无效 / scene 未建 → None。
pub fn get_node_world_matrix(&self, node: NodeId) -> Option<crate::transform::Affine2> {
    let scene = self.scene.as_ref()?;
    scene.get(node)?;   // gen 校验
    scene.world_transforms.get(node.index()).copied()
}

/// 读节点 sort_key（assign_sort_keys merge 前快照）。node 无效 → None。
pub fn get_node_sort_key(&self, node: NodeId) -> Option<u32> {
    let scene = self.scene.as_ref()?;
    scene.get(node)?;
    scene.node_sort_keys.get(node.index()).copied()
}

/// 节点可见性：存在 + 非 display:none。RemoveNode / scene 未建 / display:none → false。
pub fn get_node_visible(&self, node: NodeId) -> bool {
    let scene = match self.scene.as_ref() { Some(s) => s, None => return false };
    match scene.get(node) {
        None => false,
        Some(n) => !matches!(n.style.display, crate::style::resolved::Display::None),
    }
}
```
> 确认 `Display` enum 路径：grep `pub enum Display` in `loomgui_core/src/style/`。若 `Display::None` 路径不同（如 `style::resolved::Display::None`），按实际改。

- [ ] **Step 4: 跑测试通过**

Run: `cargo test -p loomgui_core get_node_`
Expected: PASS。

- [ ] **Step 5: 跑回归**

Run: `cargo test -p loomgui_core`
Expected: 全过。

- [ ] **Step 6: Commit**

```bash
git add loomgui_core/src/stage.rs loomgui_core/tests/
git commit -m "$(cat <<'EOF'
feat(core): Stage get_node_world_matrix/sort_key/visible getters

Read per-node world transform (from world_transforms, all nodes incl
empty divs), sort_key (from Task 1 node_sort_keys snapshot), and visible
(exists + non-display:none). NativeHost FFI query layer (next task) uses
these. Invalid NodeId (incl. gen-stale) → None/false, no panic.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: FFI 3 extern + 重编 dll + nm 查导出

**Files:**
- Modify: `loomgui_ffi_c/src/lib.rs`（在 `loomgui_stage_get_node_layout_rect` line 475 附近后加 3 extern）

**Interfaces:**
- Consumes: Task 2 的 `Stage::get_node_*`
- Produces: 3 个 `#[no_mangle] extern "C"` 符号 + csbindgen 重生成的 `LoomGUIBindings.cs`（C# `ref float`/`ref uint`/`ref byte`）。

- [ ] **Step 1: 加 3 个 extern**

Edit `loomgui_ffi_c/src/lib.rs`，在 `loomgui_stage_get_node_layout_rect`（line 488 结束）后加：
```rust
/// 读节点 world transform（compute_world_transforms 产物）。null/无效 → 写默认（identity+0/0）。
/// out: a,b,c,d,tx,ty（6 个 f32）。对齐 get_node_layout_rect 惯例（独立 out + 无状态码）。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_world_matrix(
    h: *const StageHandle, node_id: u32,
    out_a: *mut f32, out_b: *mut f32, out_c: *mut f32,
    out_d: *mut f32, out_tx: *mut f32, out_ty: *mut f32,
) {
    let m = if h.is_null() { None } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_world_matrix(NodeId(node_id))
    }.unwrap_or(crate::transform::IDENTITY);   // [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    if !out_a.is_null() { unsafe { *out_a = m[0]; } }
    if !out_b.is_null() { unsafe { *out_b = m[1]; } }
    if !out_c.is_null() { unsafe { *out_c = m[2]; } }
    if !out_d.is_null() { unsafe { *out_d = m[3]; } }
    if !out_tx.is_null() { unsafe { *out_tx = m[4]; } }
    if !out_ty.is_null() { unsafe { *out_ty = m[5]; } }
}

/// 读节点 sort_key（merge 前快照）。null/无效 → 写 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_sort_key(
    h: *const StageHandle, node_id: u32, out: *mut u32,
) {
    let sk = if h.is_null() { None } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_sort_key(NodeId(node_id))
    }.unwrap_or(0);
    if !out.is_null() { unsafe { *out = sk; } }
}

/// 读节点可见性（存在 + 非 display:none）。null/无效 → 写 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_visible(
    h: *const StageHandle, node_id: u32, out: *mut u8,
) {
    let vis = if h.is_null() { false } else {
        let sh = unsafe { &*h };
        sh.stage.get_node_visible(NodeId(node_id))
    };
    if !out.is_null() { unsafe { *out = if vis { 1 } else { 0 }; } }
}
```

- [ ] **Step 2: 重编 dll（release）**

Run: `cargo build -p loomgui_ffi_c --release`
Expected: 编译成功，csbindgen build.rs 重新生成 `loomgui_unity/Assets/Plugins/LoomGUI/LoomGUIBindings.cs`（含 3 新 P/Invoke）。

- [ ] **Step 3: nm/findstr 查 3 新符号在 dll（坑 100）**

Run:
```bash
NM=$(which nm 2>/dev/null)
if [ -n "$NM" ]; then
  nm target/release/loomgui_ffi_c.dll 2>/dev/null | grep -E "loomgui_stage_get_node_world_matrix|loomgui_stage_get_node_sort_key|loomgui_stage_get_node_visible"
else
  findstr /a target/release/loomgui_ffi_c.dll "loomgui_stage_get_node_world_matrix loomgui_stage_get_node_sort_key loomgui_stage_get_node_visible"
fi
```
Expected: 3 行命中（3 个符号都在 dll）。若无，dll stale（重编失败）——查 cargo build 输出。

- [ ] **Step 4: 拷 dll 到 Plugins（Unity 须关着——锁 dll）**

Run:
```bash
cp target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
```
> 若用户 Unity 开着，拷贝失败（锁）——告知用户关 Unity 再拷。当前用户睡了，Unity 应关着。

- [ ] **Step 5: 验 dll 非空 + 确认 LoomGUIBindings.cs 含 3 新签名**

Run:
```bash
ls -la loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
grep -c "loomgui_stage_get_node_world_matrix\|loomgui_stage_get_node_sort_key\|loomgui_stage_get_node_visible" loomgui_unity/Assets/Plugins/LoomGUI/LoomGUIBindings.cs
```
Expected: dll 大小合理（~MB）；Bindings.cs 命中 3（每符号 1 次 P/Invoke 声明）。

- [ ] **Step 6: md5 确认 dll 一致（坑 10 stale dll）**

Run:
```bash
md5sum target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll
```
Expected: 两 md5 相等。不等 = 拷贝没成功。

- [ ] **Step 7: Commit（Rust 改动 + dll + Bindings.cs）**

```bash
git add loomgui_ffi_c/src/lib.rs loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/LoomGUIBindings.cs
git commit -m "$(cat <<'EOF'
feat(ffi): get_node_world_matrix/sort_key/visible extern ports

Three read-only query ports for NativeHost Sync (replaces blob iteration).
Aligns with get_node_layout_rect convention: *const StageHandle +
independent *mut out params + no status code + null/invalid writes default
(identity matrix / 0 / 0). Rebuilds dll + regenerates C# bindings.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: C# NativeHostManager.Sync 改 FFI 查询 + LoomStage 调用点

**Files:**
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs`（Sync 方法，约 line 118-163）
- Modify: `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs:587`（`_nhm.Sync(blob)` → `_nhm.Sync(_stage)`）

**Interfaces:**
- Consumes: Task 3 的 3 个 `Native.loomgui_stage_get_node_*` C# 绑定（csbindgen 生成，签名 `ref float`/`ref uint`/`ref byte`，第一参 `StageHandle*`）。
- Produces: `NativeHostManager.Sync(StageHandle* stage)`——遍历 `_bindings` 查询设 wrapper，不再收 blob。

- [ ] **Step 1: 改 NativeHostManager.Sync 签名 + 实现**

Edit `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs`。把现有 `public void Sync(FrameBlob blob)`（line 124）整体替换：
```csharp
/// <summary>
/// 每帧 MirrorPool.Sync 后调：遍历 _bindings，FFI 查每个 nodeId 的 world_matrix/sort_key/visible，
/// 设 wrapper TRS + GO sortingOrder + visible。用户 GO 自身 transform 不动。
/// 不再遍历 blob——空 div slot 被 merge_meshes 吞后仍可查 world_transforms（FFI 独立于 merge）。
/// </summary>
public unsafe void Sync(LoomGUIBindings.StageHandle* stage)
{
    if (_bindings.Count == 0) return;
    float sf = Mathf.Abs(_root.localScale.y);  // root (sf,-sf,sf) → 取 |y|
    float a = 0, b = 0, c = 0, d = 0, tx = 0, ty = 0;
    uint sk = 0;
    byte vis = 0;
    foreach (var kv in _bindings)
    {
        uint id = kv.Key;
        var go = kv.Value;
        if (go == null) continue;
        // visible（含 RemoveNode / display:none → 0）
        Native.loomgui_stage_get_node_visible(stage, id, ref vis);
        if (vis == 0) { if (go.activeSelf) go.SetActive(false); continue; }
        // world_matrix
        Native.loomgui_stage_get_node_world_matrix(stage, id,
            ref a, ref b, ref c, ref d, ref tx, ref ty);
        if (!_wrappers.TryGetValue(id, out var wrapper) || wrapper == null) continue;
        float rot = Mathf.Atan2(b, a) * Mathf.Rad2Deg;
        float sx = Mathf.Sqrt(a * a + b * b);
        float sy = Mathf.Sqrt(c * c + d * d);
        wrapper.transform.localPosition = new Vector3(tx, -ty, 0);
        wrapper.transform.localRotation = Quaternion.Euler(0, 0, rot);
        wrapper.transform.localScale = new Vector3(sx, sy, sf > 0.0001f ? 1.0f / sf : 1.0f);
        // sort_key → sortingOrder
        Native.loomgui_stage_get_node_sort_key(stage, id, ref sk);
        foreach (var r in go.GetComponentsInChildren<Renderer>())
            if (r != null) r.sortingOrder = (int)sk;
        if (!go.activeSelf) go.SetActive(true);
    }
}
```
> 确认 `Native` 类型名（csbindgen 生成的内部类型，看 LoomGUIBindings.cs 顶部 `internal static partial class Native`）。确认 `LoomGUIBindings.StageHandle*` 路径（看现有 LoomStage.cs 怎么引 `StageHandle*`）。

- [ ] **Step 2: 删 Sync 旧签名的 FrameBlob 参数用法（如有残留）**

旧 Sync 用了 `blob.NodeCount`/`blob.NodeId(i)`/`blob.Ma(i)` 等——全删（新 Sync 不遍历 blob）。`_seenThisFrame` HashSet 也删（不再用）。grep 确认无 blob 残留：`grep -n "blob\.\|_seenThisFrame" NativeHostManager.cs` 应只在注释或全删。

- [ ] **Step 3: LoomStage.LateUpdate 调用点改**

Edit `loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs:587`，把 `_nhm.Sync(blob)` 改：
```csharp
_nhm.Sync(_stage);
```
> `_stage` 是 `StageHandle*`（LoomStage.cs:57）。LoomStage 是 unsafe 类，直接传 `_stage`。blob 仍传 MirrorPool（渲染管线不动），确认 blob 变量仍传 `_pool.Sync(blob)`（不应删 blob 整体）。

- [ ] **Step 4: 确认 NativeHostManager 用 `using` 引 Native（如需）**

NativeHostManager.cs 顶部确认 `using LoomGUI;` 或 Native 的可见性。若 Native 是 `LoomGUIBindings.Native`，加 `using static LoomGUIBindings.Native;` 或全限定。读 LoomGUIBindings.cs 确认 Native 类型位置 + StageHandle 声明。

- [ ] **Step 5: 语法/类型自查**

对照 LoomGUIBindings.cs 确认 3 新 P/Invoke 签名（csbindgen 生成）：
- `loomgui_stage_get_node_world_matrix(StageHandle*, uint, ref float, ref float, ref float, ref float, ref float, ref float)`
- `loomgui_stage_get_node_sort_key(StageHandle*, uint, ref uint)`
- `loomgui_stage_get_node_visible(StageHandle*, uint, ref byte)`
若 csbindgen 生成的 ref 数量/类型不同（如 `out` 而非 `ref`），按实际调。

- [ ] **Step 6: Unity 编译验证（用户侧，或家里机当前 Unity）**

家里机当前 Unity 应关着（用户睡了）。Unity 下次打开 focus 时编译——Console 应无 C# 错。若家里机能启动 Unity headless 编译验证更好；否则 Task 5 handoff 时用户验。

- [ ] **Step 7: Commit**

```bash
git add loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs loomgui_unity/Assets/LoomGUI/Runtime/LoomStage.cs
git commit -m "$(cat <<'EOF'
feat(unity): NativeHostManager.Sync queries FFI per-node (not blob)

Replace blob iteration with per-binding FFI queries: get_node_visible
(decides SetActive; false for RemoveNode/display:none), get_node_world_matrix
(sets wrapper TRS), get_node_sort_key (sets sortingOrder). Empty-div slots
now keep working even after merge_meshes collapses their render entry.
Sync signature: FrameBlob → StageHandle*.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: dump example 验证 + handoff PlayMode

**Files:**
- Modify: `loomgui_core/examples/dump_nativehost_slot.rs`（扩展打印 world_transforms + node_sort_keys）

**Interfaces:**
- Consumes: Task 1 的 `Scene::node_sort_keys` + Task 2 的 getter（直接读 scene 字段或调 stage getter）。

- [ ] **Step 1: dump example 扩展打印 world_transforms + node_sort_keys**

Edit `loomgui_core/examples/dump_nativehost_slot.rs`，在"结论：nh-stage NOT IN"附近加：
```rust
// 直查 scene 数据（绕 frame.nodes blob——验 world_transforms + node_sort_keys 含空 div）
let scene = s.scene.as_ref().expect("scene");
let nh_id = NodeId(scene.find_by_id_attr("nh-stage").expect("nh-stage").0);
let wm = scene.world_transforms.get(nh_id.index()).copied();
let sk = scene.node_sort_keys.get(nh_id.index()).copied();
println!("\nnh-stage 直查（绕 blob）：");
println!("  world_transforms[{}]={:?} (tx,ty={:?})", nh_id.index(), wm, wm.map(|m| (m[4], m[5])));
println!("  node_sort_keys[{}]={:?}", nh_id.index(), sk);
println!("  → world_transforms 非零（位置在 nh-stage 框）+ sort_key 非零（DFS 序）= NativeHost FFI 可查");
```

- [ ] **Step 2: 跑 example 验证**

Run: `cargo run -p loomgui_core --example dump_nativehost_slot -- loomgui_unity/Assets/StreamingAssets/showcase.pkg.bin 2>&1 | tail -20`
Expected: nh-stage 的 `world_transforms[...]` tx/ty ≠ 0（slot 在屏幕中部）+ `node_sort_keys[...]` > 0（DFS 序号）。frame.nodes 仍 NOT IN（merge 吞了——证明 blob 不该当通道，FFI 查 world_transforms 才对）。

- [ ] **Step 3: fence_contract 回归**

Run: `cargo test -p loomgui_core --test fence_contract`
Expected: 15 passed。

- [ ] **Step 4: Commit example**

```bash
git add loomgui_core/examples/dump_nativehost_slot.rs
git commit -m "$(cat <<'EOF'
diag(example): dump_nativehost_slot verifies world_transforms/sort_keys for empty div

Confirms root cause fix: nh-stage (empty div, merged out of frame.nodes)
still has nonzero world_transforms + node_sort_keys (readable via FFI),
while frame.nodes still excludes it (blob is a render list — not the
transform channel).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: handoff PlayMode 验收（用户）**

写 handoff 给用户（明早验收）：
1. 确保工作树 driver 修复（page_controls 实例化等）+ controller/SampleScene 配置未被误覆盖。
2. Unity 打开 loomgui_unity/ → focus 编译（Console 应无 C# 错，含新 3 P/Invoke）。
3. PlayMode → 点「3D/特效」卡片：
   - 角色显示在 nh-stage 框位置（不再屏幕角上）
   - scale 对（_characterScale (70,70,70) 起步，PlayMode 调）
   - 「放光效」toggle 粒子（Kenney prefab）+ 火苗朝上（y-flip 未带歪）
   - 「切动画」切 Animator state（需用户配 controller state 名）
   - Frame Debugger：外部 GO 在 Transparent(3000) 队列 + sortingOrder 符合 DFS 序
4. 切页回归：返回 home → 角色/粒子消失；再进 → 复现（同缓存实例）。
5. page_controls（点「控件」）：§1.6 model-slot 也显示角色实例。

---

## Self-Review 已做

- **Spec 覆盖**：spec §3.1 world_matrix → T2 getter + T3 FFI；§3.2 node_sort_keys → T1 数据 + T2 getter + T3 FFI；§3.3 visible → T2 getter + T3 FFI；§4 FFI → T3；§5 C# Sync → T4；§6.1 dump 验 → T5；§6.2 单测 → T1/T2 含；§6.3 PlayMode → T5 Step 5。全覆盖。
- **Placeholder**：每步含完整代码 + 命令 + 期望。无 TBD。
- **类型一致**：`StageHandle*`（C# unsafe 指针）跨 T3/T4 一致；`ref float`/`ref uint`/`ref byte`（csbindgen）跨 T3/T4 一致；`node_sort_keys: Vec<u32>` 跨 T1 各步一致；`Affine2 = [f32;6]` 跨 T2 getter 返回 + T3 FFI out 一致。
