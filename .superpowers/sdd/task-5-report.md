# Task 5 Report — dump example 验证 + PlayMode Handoff

**Status:** DONE
**Commit:** `a7351d3` (branch `worktree-nativehost-ffi-query`, parent `de6f9c2`)

## 改动摘要

新增 `loomgui_core/examples/dump_nativehost_slot.rs`（133 行）——端到端验证 Task 1-4 的核心修复：
NativeHost 角色挂的空 div slot（nh-stage）即便被 `merge_meshes` 吞掉（不进 `frame.nodes`
渲染 blob），仍可直查 `scene.world_transforms` + `scene.node_sort_keys`，证明 FFI 查询通道
独立于 merge。

example 走真实 pkg.bin 路径（对齐 PlayMode 从 StreamingAssets 加载）：
`read_package` → `Stage::load_package` → `create_root` → `instantiate("showcase", "page_nativehost")`
→ `append_child` → `tick_and_render` → 同时查 (1) `frame.nodes` blob + (2) `scene` 并行数组。

## example 关键输出

```
pkg name="" components=13

=== 1) frame.nodes（渲染 blob 通道）===
frame.nodes count = 10
frame.nodes ids with id_attr = ["back-home", "nh-anim"]
nh-stage IN frame.nodes? -> false
  期望：false（空 div 无 mesh payload，被 merge_meshes 吞，不进 blob）

=== 2) scene 直查（绕 blob——FFI 查询通道）===
  world_transforms[10]=Some([1.0, 0.0, 0.0, 1.0, 240.0, 123.0]) -> tx,ty=Some((240.0, 123.0))
  node_sort_keys[10]=Some(9)
  期望：tx/ty 非零（slot 落在 nh-stage 框中部）+ sort_key>0（DFS 序号）

=== 结论 ===
  nh-stage NOT IN frame.nodes (merge 吞): true
  world_transforms tx/ty 非零: true
  node_sort_keys > 0: true
  -> PASS：FFI 查询通道独立于 merge——空 div slot 仍可查 world_transforms + sort_key
```

要点：
- `frame.nodes` blob 只有 10 个 entry（nh-stage 不在内）——空 div 无 mesh payload，被 `merge_meshes` 吞。这就是旧实现走 blob 查不到 nh-stage → 角色 fallback 屏幕角上的根因。
- 直查 `scene.world_transforms[10]`：`[1.0, 0.0, 0.0, 1.0, 240.0, 123.0]` → tx=240, ty=123（slot 落在 nh-stage 框位置，水平居中 (1080-600)/2=240，垂直 back-btn + title 之后 = 123）。**正是 NativeHost 角色该出现的位置。**
- `scene.node_sort_keys[10] = 9` —— DFS 序号 > 0，driver 用它设外部 GO 的 `sortingOrder`，保证角色/粒子在透明队列里跟 UI 的 DFS 顺序一致。
- 结论 PASS：FFI 查询通道（T2 getter / T3 extern）独立于 merge，空 div slot 可查。

## fence_contract 回归

```
running 15 tests
test at_rule_media_skipped_by_parser ... ok
... (15 tests) ...
test fence_out_tags_rejected ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

15/15 通过。围栏契约无回归。

## Handoff 给用户（明早 PlayMode 验收）

### 用户工作树状态说明

你的**主工作树**（`E:/workspace/LoomGUI`，branch `main`）有未提交的 driver 修复（page_controls
实例化等）+ SampleScene 配置——**这些不会被本 worktree merge 覆盖**。本 worktree（branch
`worktree-nativehost-ffi-query`）只动：
- `loomgui_core`（Scene/Stage/FFI）
- `loomgui_ffi_c.dll` + `LoomGUIBindings.cs`（在 `Assets/Plugins/LoomGUI/`）
- `loomgui_unity/Assets/LoomGUI/Runtime/NativeHostManager.cs` + `LoomStage.cs`（T4 改）

merge 回 main 时，NativeHostManager.cs / LoomStage.cs 会和你的 driver 修复会合——
**无冲突**（你改的是 driver / page_controls / SampleScene，本 worktree 改的是 NativeHostManager
和 LoomStage 里的 FFI 查询调用点）。

### PlayMode 验收清单

1. **merge 前必做**（公司机/本机）：
   - 确认本 worktree 的 4 个核心 commit（`b16b12c` T1 / `f60df88` T2 / `216741c` T3 /
     `de6f9c2` T4）+ 本 T5 commit（`a7351d3`）已 push。
   - 用户主工作树：`git merge worktree-nativehost-ffi-query`（或 PR merge）——无冲突预期。
   - **重打 pkg.bin**（坑 66）：本 worktree 没改 parse-time 逻辑，但 page_nativehost.html
     在更早的 nativehost-3d-demo worktree 里建的，确保 showcase.pkg.bin 含 page_nativehost
     组件（dump example 已验含，components=13）。若主工作树 pkg.bin 不含 → `cargo run -p loomgui_pkg`。

2. **Unity 打开 `loomgui_unity/`** → 等 focus 编译：
   - Console 应**无 C# 错**。
   - 关键新增 P/Invoke（在 `LoomGUIBindings.cs`）：`loom_get_node_world_matrix` /
     `loom_get_node_sort_key` / `loom_get_node_visible`（T3 重编 dll + 重生成 bindings）。
   - **stale .dll 诊断**（坑 10）：若 PlayMode 全不渲 + Console 干净 →
     `md5sum target/release/loomgui_ffi_c.dll loomgui_unity/Assets/Plugins/LoomGUI/loomgui_ffi_c.dll`
     不等 = stale，重拷（Unity 关着拷）。

3. **PlayMode → 点 home 的「3D/特效」卡片** → 进 page_nativehost：
   - **角色显示在 nh-stage 框位置**（屏幕水平居中、垂直偏上）——不再屏幕角上。
     dump example 已证 world_transforms tx=240 ty=123，driver 会把它喂给 GO Transform。
   - **scale 对**：起步 `_characterScale (70,70,70)`，PlayMode 内手动调到视觉合适。
   - **「放光效」toggle**：点 → Kenney 粒子 prefab 实例化 + 显示；再点 → 隐藏。
     火苗朝上（y-flip 是根 Stage 一一次性变换，外部 GO 不带歪）。
   - **「切动画」**：切 Animator state（需你配 controller 的 state 名）。
   - **Frame Debugger**（ optional 深挖）：外部 GO 在 Transparent(3000) 队列，
     sortingOrder 符合 DFS 序（nh-stage sort_key=9）。

4. **切页回归**：
   - 点「← 返回」回 home → 角色/粒子 GO 应消失（NativeHostManager.Unbind + 销毁实例）。
   - 再点「3D/特效」→ 角色重现（同缓存实例，或新建——视 driver 缓存策略）。

5. **page_controls（点「控件」）**：若该页有 `model-slot`（§1.6 spec），角色实例也应显示
   在该 slot 位置——验证多 NativeHost slot 共存。

### 预期问题 + 排查

- **角色仍在屏幕角上**：dll stale（坑 10）→ 重拷 .dll；或 bindings 未重生成 → 查
  `LoomGUIBindings.cs` 含 `loom_get_node_world_matrix`；或 NativeHostManager.Sync 未被调
  → 查 LoomStage.tick 调用点（T4 改）。
- **角色显示但位置/scale 偏**：dump example 已证 core 数据正确，问题在 C# 应用层
  （Matrix4x4 列主序转置 / scale 应用顺序）——查 NativeHostManager.ApplyWorldMatrix。
- **粒子朝下/错位**：y-flip 带歪外部 GO——查 NativeHostManager 是否给外部 GO 加了
  `(1,-1,1)` scale（不该加，y-flip 只该在根 Stage）。
- **sortingOrder 错**：driver 未读 sort_key → 查 NativeHostManager.Sync 取 `sort_key` 字段。

## Concerns

无。example 验证完整，fence_contract 回归通过。所有 concerns 已在 T1-T4 report 列明
（csbindgen raw 指针 / Affine2 列主序 / visible=0 skip），用户 PlayMode 验收时按上表排查即可。
