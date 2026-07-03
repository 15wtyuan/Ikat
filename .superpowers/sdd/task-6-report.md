# Task 6 报告: FrameBlob v9 + MirrorPool 双 dict reuse_key 复用

## 状态: done

Commit: `645f45afa278f1b4080d5a83dfeb482eca4592ed`

## 改的文件 (3)

### 1. `loomgui_unity/Assets/LoomGUI/Runtime/FrameBlob.cs`
- **ExpectedVersion** 8→9，注释改"v9：加 reuse_key 列（第 22 列）"
- **header** 128B→132B，21列→22列
- **列注释** 末尾加 `21=reuse_key(u32, 0=无复用 >0=slot 复用键)`
- **ReuseKey reader**: `public uint ReuseKey(int i) => ReadU32(ColOff(21) + i * 4);`
- **arena offset 8 处全部 `21*4`→`22*4`**: MeshArenaOff/TextArenaOff/TextArenaLen/ClipTableOff/ClipTableLen/PathTableOff/PathTableLen（注释中的行内偏移也更新）

### 2. `loomgui_unity/Assets/LoomGUI/Runtime/MirrorPool.cs`
- **_pool** 单 dict → **_poolByNodeId** + **_poolByReuse** 双 dict
- **Count**: `_poolByNodeId.Count + _poolByReuse.Count`
- **Sync 方法**: 
  - stale 标记：两个 dict 全标 true
  - 遍历节点：`poolKey = reuseKey != 0 ? reuseKey : id`；`pool = reuseKey != 0 ? _poolByReuse : _poolByNodeId`
  - SKIP/HEADER/FULL 查找均按 poolKey + pool
  - 新建 RenderObj 时写 `pool[poolKey] = ro`（非 `_pool[id]`）
  - stale 清理：两个 dict 各自收集+销毁
- **Clear**: 两个 dict 各自 TearDown+Clear
- **图资源段** (path_idx→Sprite) 原逻辑完整保留
- **UpdateHeader / UploadMeshOrText** 不变

### 3. `loomgui_unity/Assets/LoomGUI/Tests/MirrorPoolTests.cs`
- 新增 class `MirrorPoolReuseKeyTests`（独立于 Ignore 的 MirrorPoolTests）
- 新增 helper `OneNodeBlobV9`: 构造 v9 1 节点 Mesh blob（22 列 SOA，含 program/color_matrix/change_level/reuse_key）
- 新增 test `SlotReuseKeyRecyclesGoAcrossNodeChange`:
  - 帧1: node_id=100, reuse_key=5 → 建 GO
  - 帧2: node_id=200, reuse_key=5 → 复用同一 GO（ReferenceEquals）
  - 反射验证 `_poolByReuse[5]` 的 LastNodeId==200
  - 验证 position 已更新到帧2坐标 (30,40)

## grep 验证

```
FrameBlob.cs:
  ExpectedVersion = 9           ✓
  ReuseKey reader 在 line 49    ✓
  22*4 8 处（全 arena offset）   ✓
  21*4 残留 0                   ✓

MirrorPool.cs:
  _poolByNodeId + _poolByReuse  ✓
  旧 _pool (不含 By) 残留 0     ✓
  poolKey 4 处引用              ✓
```

## MirrorPool stale 逻辑审查

**slot 换绑场景（核心路径）**:
- 帧1: node_id=100, reuse_key=5 → poolKey=5, pool=_poolByReuse → _poolByReuse[5]=new GO
- 帧2: node_id=200, reuse_key=5 → poolKey=5, pool=_poolByReuse → TryGetValue 命中 → Stale=false → GO 复用 ✓
- 帧2 无 node_id=100（slot 换绑）→ _poolByReuse[5] 已清 stale → 不销毁 ✓

**普通节点不变**: reuse_key=0 → poolKey=id, pool=_poolByNodeId → 行为同 v1 ✓

**reuse_key 变更**: 旧 reuse_key 对应的 GO 在 _poolByReuse 中 stale → 销毁，新 reuse_key 建 GO。正确 ✓

**无 concerns** — 双 dict stale 逻辑正确，slot 换绑走 poolKey 命中路径不销毁。

## 家里机待编译/验证项

1. **Unity 编译**：C# 语法（反射 IDictionary cast、BindingFlags 全限定名）需 Unity 编辑器编译验证
2. **EditMode 测试**: `MirrorPoolReuseKeyTests.SlotReuseKeyRecyclesGoAcrossNodeChange` 跑通
3. **PlayMode 验收**: 配合 Rust T5 产出的 v9 blob 端到端验证 slot 滚动复用
4. **v4 旧测试仍 Ignore**: MirrorPoolTests/MirrorPoolFlattenTests 类级 `[Ignore]` 未动

## 偏差

无。严格按 brief Step 1-5 实现。

## Fix: LastNodeId 复用时更新

Commit: `94d3b5f`（在当前 branch 的顶部）

### 问题
T6 review 发现 1 Critical：`MirrorPool.cs` Sync 方法里 `ro.LastNodeId = id;` 在 `if (!pool.TryGetValue(...))` 块内，仅新建 GO 时赋值。slot 换绑（reuse_key 不变、node_id 变）复用 GO 时，LastNodeId 保持旧值。测试 `SlotReuseKeyRecyclesGoAcrossNodeChange` 断言复用后 LastNodeId==200 会 fail。

### 修改
`loomgui_unity/Assets/LoomGUI/Runtime/MirrorPool.cs` 第 108 行：
- 把 `ro.LastNodeId = id;` 从 `if` 块内（原 108 行）移到 `if` 块外（新 111 行），`ro.Stale = false;` 之前。
- 注释：`// v1.4-b：新建 + 复用均更新（slot 换绑时 node_id 变）`

grep 确认：
```
MirrorPool.cs:
  ro.LastNodeId = id; 在 line 111（if 块外）  ✓
  ro.Stale = false; 在 line 112               ✓
  残留 ro.LastNodeId = id 数: 1               ✓
```

### 家里机验证
- 跑 `SlotReuseKeyRecyclesGoAcrossNodeChange` 测试，确认复用后 `_poolByReuse[5].LastNodeId == 200`。
- 本机无 Unity，未跑测试。
