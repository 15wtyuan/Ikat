## Task 8 完成报告

**状态**: 完成。C# 代码已编写、自审、提交；Rust .dll 已重编、拷贝、md5 验证；所有测试绿色。Unity PlayMode 验收待家里机执行。

**Commit**: `5d3d2b3` on `worktree-render-refactor`
**测试**: 53 ffi_c + 495 core 测试全部通过，0 失败
**md5**: `986901abce696c11e405f38ceefbc1cc`（两文件一致，坑 10 通过）

---

## 1. 变更概要

### 1.1 FrameBlob.cs（v7 -> v8）
- `ExpectedVersion` 7 -> 8
- 列数 20 -> 21；新增 `20=change_level(u8, 0=Skip 1=Header 2=Full)` 列注释
- header 尺寸 124B -> 128B
- 所有 arena offset 常量 `20*4` -> `21*4`（MeshArenaOff 92->96, TextArenaOff 100->104, ClipTableOff 108->112, PathTableOff 116->120 等）
- 新增 `public byte ChangeLevel(int i) => _buf[ColOff(20) + i];`
- `clip_table_len` header 注释 @112 -> @116

### 1.2 MirrorPool.cs（二分支 -> 三分支）
- **Sync 主循环**：`byte level = blob.ChangeLevel(i)` 替代旧 `byte kind == 0` 逻辑
  - `level == 0`（SKIP）：`_pool.TryGetValue` 清 stale，continue
  - `level == 1`（HEADER）：调用 UpdateHeader（只更 transform/material/uniforms，不重建 mesh）
  - `level == 2`（FULL）：UpdateHeader + UploadMeshOrText
  - **兜底**：新建 RenderObj 无 GO/mesh -> 强制 `level = 2`（无视 blob 的 HEADER，避免只挪空 mesh）
- **UpdateHeader**（新实例方法）：设置 localPosition/rotation/scale + sortingOrder + clip box + 材质 + per-renderer MPB（_ObjM + _CF + _Alpha）
- **UploadMeshOrText**（新静态方法）：仅 FULL 路径调用；kind=1 走 UploadMesh + RecalculateBounds + Sprite UV 重映射；kind=2 走 BuildMesh + UploadMesh + RecalculateBounds
- **MPB 合并**：旧代码 `SetObjectMatrix` 与 `SetColorFilterMatrix` 各自独立调用 `SetPropertyBlock`（后者覆盖前者，_ObjM 丢失的隐 bug）。现已合并为一次 `GetPropertyBlock` -> 设 _ObjM / _CF / _Alpha -> 一次 `SetPropertyBlock`。`_Alpha` 每帧无条件设置为 `blob.Alpha(i)`。
- 移除废弃的 `SetObjectMatrix` / `SetColorFilterMatrix` 静态方法

### 1.3 .dll 重编
- `cargo build -p loomgui_ffi_c --release` 通过
- md5 匹配（坑 10）

---

## 2. 自审结果

- FrameBlob 列索引 0..20，ChangeLevel 读 ColOff(20) 正确
- arena offset 常量数学验证：`12 + 21*4 + 6*4 + 4 = 124`（PathTableLen），header 末尾 = 128 -- 正确
- MirrorPool 三分支逻辑：SKIP->跳过、HEADER->UpdateHeader only、FULL->UpdateHeader+UploadMeshOrText -- 正确
- 新建 GO 强制 level=2 兜底 -- 正确
- `_Alpha` MPB 在 SetPropertyBlock 前合并所有 uniform -- 正确
- `Texture2D` -> `Texture` 隐式转换（`sp.texture` -> `tex` 变量）在现有代码中同模式，预期兼容
- `ro.Mr.GetPropertyBlock(ro.Mpb)` 是标准 Unity API
- `UploadMeshOrText` 中 `sp` 参数仅 kind==1 分支使用（UV 重映射）；kind==2 不碰 sp

---

## 3. 问题/顾虑

1. **Unity PlayMode 未验证**：公司机无 Unity 工具链，无法运行时验证。所有 C# 修改仅为代码审阅级别确认，未在 PlayMode 实际运行。
2. **`_Alpha` shader 端未确认**：UpdateHeader 写入 `_Alpha` per-renderer MPB，假设 shader 有 `_Alpha` uniform 且 `[PerRendererData]` 属性。若 shader 未定义此 uniform，`SetFloat("_Alpha", ...)` 将无效果（Unity 静默忽略未知 property）。需验证 shader 侧已定义。
3. **`GetPropertyBlock` 可能读旧值**：UpdateHeader 在设置 _ObjM/_CF/_Alpha 前调用 `GetPropertyBlock` 读取当前 MPB。纯平移节点（`!pure`）不设 _ObjM——若节点从前帧非纯平移切换为纯平移，旧 _ObjM 可能残留在 MPB 中。虽然此现象在旧代码已存在（且更严重：_ObjM 与 _CF 互相覆盖），合并后仍需关注。
4. **工作树路径注意**：本次修改的文件位于 worktree 路径下（`...\render-refactor\loomgui_unity\...`）。合并到 main 时需确认路径正确。

---

## 4. 家里机验收清单

1. hover 变色刷新、`:active` 缩放当帧可见（支柱1 变更检测）
2. 滑动列表：Profiler 确认无逐帧 UploadMesh（支柱3 HEADER 路径）
3. opacity tween：Profiler 无 UploadMesh（alpha uniform 走 MPB，不触发 FULL）
4. 图片/文字正常显示（v8 blob 解析正确：ExpectedVersion=8）
5. `_Alpha` uniform 生效：透明度变化只变 MPB、不重建 mesh
6. ColorFilter + _ObjectMatrix 节点：两个 uniform 同时生效（MPB 合并修复）

---

## Fix: text double-alpha

**Commit**: `19303bf` on `worktree-render-refactor`
**Root cause**: `UploadMeshOrText` text 分支调用 `BuildMesh(..., nodeAlpha, ...)` 将 node opacity 烤进 vertex color alpha，同时 `UpdateHeader` 已通过 `_Alpha` MPB uniform 设置了 node alpha。Shader `col.a *= _Alpha` 再次乘以 nodeAlpha，导致 double application（nodeAlpha^2）。
**Fix** (`MirrorPool.cs`): 将 `BuildMesh` 调用从 `nodeAlpha` 改为 `1f`，移除未使用的 `nodeAlpha` 局部变量。Alpha 现在完全由 `_Alpha` uniform（在 `UpdateHeader` 中对所有节点设置）承载。
**Test update** (`TextRasterizerTests.cs`): `BuildMesh_VertexColor_IsColorTimesAlpha` -> `BuildMesh_VertexColor_IsInputColor_AtAlphaOne`：传 `alpha=1f`，断言 `vertex.a == color.a`（不再烤 nodeAlpha）。文件顶部注释同步更新。
**grep**: 生产代码中 `BuildMesh` 只有一个调用点（MirrorPool.cs:182），现已修复。所有其他出现位置均为测试或文档。
**Rust tests**: loomgui_core (480 passed) + loomgui_ffi_c (53 passed) -- 全绿，无回归。

---

## Fix: strengthen text-alpha test (Option A)

**Commit**: `d7a251f` on `worktree-render-refactor`

### 问题
Commit `19303bf` 的测试 `BuildMesh_VertexColor_IsInputColor_AtAlphaOne` 是 vacuous 的：传 `alpha=1f`，断言 `vertex.a == color.a`。但 BuildMesh 内部仍做 `tinted.a *= alpha`，`1f` 乘法恒真——测试无法区分 "不烤了" 与 "烤了但倍数=1"。若有人回退 MirrorPool 的 1f 临时绕过，测试仍绿。

### 修改（Option A：彻底删除 BuildMesh 内部烘焙）

**`TextRasterizer.cs`**:
- 删除 `tinted.a *= alpha;`（第 45 行）。`tinted` 现直接等于 `color`，不再做任何 alpha 乘法。
- `alpha` 参数保留在签名中（避免侵入性 churn），标记为"现已废弃"。
- 方法文档注释更新：顶点色 = color（不烤 node opacity）。

**`TextRasterizerTests.cs`**:
- 测试重命名为 `BuildMesh_DoesNotBakeNodeAlpha`。
- 传 `alpha=0.5f`（非 1f），断言 `c.a == color.a`（0.8f，非 0.8×0.5=0.4f）。若 BuildMesh 仍乘 alpha，断言失败。
- 类级文档注释同步更新。

### grep 结果
- 生产代码中无残留 `tinted.a *=` 或 `color.a *= alpha` 模式。
- MirrorPool.cs 第 229 行注释 "BuildMesh 不再烤 nodeAlpha" 现与实现一致（之前注释说"不烤"但代码还在烤）。
- 所有其他 BuildMesh 测试调用点（几何测试）传 `1f`，不受影响。
- 无其他 bake-expectant 测试。

### Sanity
- Rust: core 495 passed, ffi_c 53 passed, 0 failures。无 Rust 改动，无需重编 .dll。
- Unity 测试需家里机运行（公司机无 Unity toolchain）。

### 设计决策
选择 Option A（删 BuildMesh 内部烘焙）而非 Option B（保留烘焙、测试传 1f 继续 vacuous）：
- Option B 保留死代码（MirrorPool 已不依赖烘焙，BuildMesh 内部乘 alpha 是纯浪费）。
- Option A 让 BuildMesh 行为与 _Alpha uniform 契约一致：alpha 参数彻底 vestigial，node opacity 独占 _Alpha uniform 通道。若未来有调用者需要烘焙（如 standalone 非框架使用），可显式传 pre-multiplied color（调用者自己做），BuildMesh 本身不做乘法。
