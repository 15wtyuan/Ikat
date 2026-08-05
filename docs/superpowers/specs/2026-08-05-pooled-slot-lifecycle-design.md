# 池化 slot 生命周期（Pooled Slot Lifecycle）设计

- **日期**：2026-08-05
- **状态**：设计已批准（brainstorming 完成），待 writing-plans 拆实现
- **分支**：`pool-slot-lifecycle`（仅本文档 + roadmap 交叉引用，无代码）
- **归属束**：复合束 ListView（roadmap §4）
- **关联**：坑 182（M1 slot GO churn）、`docs/superpowers/specs/2026-07-29-m1-listview-virtualization-design.md`（M1 前身）

---

## 0. 背景与问题

### 0.1 坑 182 回顾

M1 虚拟列表（mail 滚动）症状：上面 item 逐个消失 + 明显卡顿；inventory（固定高度网格）不显。MirrorPool churn 2-7 GO/帧。

**根因链**（commit `c762a5e` 记录 + 本设计调研复核）：
1. **core `reuse_key` 只挂 slot 根**（ListItem 容器），不到渲染叶子（text/image）→ MirrorPool 按 node_id 池化叶子，slot 根换绑时叶子 key 失配。
2. **`reuse_key` 用 slots vec 位置编码**（`slot_idx = slots.len()`），回收/重插时位置变 → key 旋转 → 同帧两 slot 共用 key / 跨帧同 NodeId 换 key。
3. **slot 回收走 detach/free 池**（`remove_child → parent=None → 下帧 reuse`），游离态 slot 的渲染状态有未知行为（"游离帧是否进 blob"单凭 core 代码未能独立证伪）。
4. **Unity MirrorPool stale=destroy**：游离/失配的 GO 被销毁，下次 reuse 重建 → churn。mail 文本 mesh 重建慢 → 1 帧 gap 可见（消失）。

### 0.2 为什么进化

当前 detach/free 池模型 + stale-destroy 是"拆了重建"，fgui/RmlUi/UITK 三家开源框架取证后确认：**detach 是多余概念**——成熟方案是"留挂 + 标记跳过"（RmlUi `display:none` = parked but attached）或"reparent 到休眠根永不 destroy"（fgui）。LoomGUI 有 Rust core + taffy layout 层，天生适合 RmlUi 的 flag 模型。

### 0.3 取证依据（设计前 stress test，3 个只读子代理）

| 风险 | 结论 | 关键证据 |
|---|---|---|
| 运行时 display:none 全链路 | GREEN | `set_inline_override`（scene/dynamic.rs:457）→ rematch 拷进 node.style → taffy 跳 + render `collect_display_none_subtree` 剪；toggle 零重建零丢状态；**Dropdown/TabList 每帧在用（control.rs:726/789）= battle-tested** |
| 选择器/scope 语义 | YELLOW（无 RED） | `:nth-child` 按 CSS 数全部 children（含 parked）→ 约束禁用；`>` 组合器 fence 不支持；后裔选择器结构匹配但 parked display:none 剪掉视觉惰性 |
| list.rs 重写面 + blob + reuse_key | GREEN | 重写集中 1 文件 ~6 函数；reuse_key 稳定化纯 Rust 零 ABI 破；blob `visible` 字节 bit1 塞 parked、零 version bump |

---

## 1. 架构总览

**一句话**：把虚拟列表 slot 从"detach 到 free 池"改成"留挂 ul 子树 + display:none 标记休眠"，core 决策池化状态、Unity 仅镜像；顺带把 `Get<T>("id")` 从全局首匹配修成子树查找（L1），让 slot 内部 id 可用。

### 1.1 分层与数据流（一帧 tick）

```
┌─ Rust core（决策层，引擎无关）─────────────────────────────┐
│ stage.tick_and_render:                                      │
│   3.6 plan_visible  → 算可见区；标 slot active/parked        │
│       （不再 detach；parked slot 留挂 ul，set display:none） │
│   3.7 execute_visible → 高水位 slot 复用：parked→active 翻   │
│       display + BindItem 换绑；不够则 clone 新 slot 扩容    │
│       reuse_key = 永久 ordinal（0..high_water），不旋转     │
│   rematch → apply display:none（parked）进 node.style       │
│             （execute 在 rematch 前 → 同帧生效，零延迟）     │
│   solve → taffy 跳 Display::None（parked 零尺寸不占位）     │
│   render → collect_display_none_subtree 剪 parked 子树      │
│   build_blob → active 条目（全字段）+ 追加 parked keepalive │
│                （极简：node_id + reuse_key + parked bit）   │
└──────────────────────────────────────────────────────────────┘
                          │ blob（21 列 SOA，visible 字节双用）
                          ▼
┌─ Unity MirrorPool（镜像层，纯执行 core 决策）───────────────┐
│ Sync 遍历 blob 节点：                                        │
│   active  → 正常渲染（UpdateHeader + UploadMesh if Full）   │
│   parked  → SetActive(false)、清 stale（不 destroy）、跳过   │
│             header/mesh 上传；GO + Mesh + buffer 全保留     │
│   gone    → stale → TearDown（真没了才销毁）                │
│ → 稳态滚动零 GO create/destroy（parked 池永不驱逐）          │
└──────────────────────────────────────────────────────────────┘
                          │
┌─ Scope（L1，正交于池化）────────────────────────────────────┐
│ find_node_by_id_in_subtree(root, id)：DFS 只搜 root 子树    │
│    Get<T> 从"全局首匹配+IsInSubtree 后过滤"→"直接子树 DFS"  │
│    → slot.Get<T>("badge") 命中本 slot，不撞别 slot          │
└──────────────────────────────────────────────────────────────┘
```

### 1.2 三条核心不变量（本次重构建立）

1. **slot 永驻 ul 子树**：从 `enter_data_driven` 预分配到组件销毁，slot 的 NodeId + parent 永不变。parked 只 toggle display + 标记位，不 detach。
2. **reuse_key = 永久 ordinal**：slot 0..N 各持唯一 key，跨帧跨滚动不旋转。MirrorPool 同 key→同 GO 永驻。
3. **池化决策在 core，后端只镜像**：blob 的 parked bit 是唯一信号源。换 Godot/UE backend，逻辑零改（约束 d）。

### 1.3 不碰的东西（边界）

- 公共 API 签名零改动（约束 b）。
- tick 时序不变量（process→rematch→solve→refresh_content→compute_world→build）保持（约束 c）；execute_visible 仍卡在 step 3.6（rematch 前）。
- material/texture/mesh 池化——已池化（`MaterialManager` DrawState 缓存 + `SpriteResolver` 页纹理缓存 + per-RenderObj Mesh buffer 复用），不动。
- L3（IsScopeRoot 完整边界 + Shadow DOM）、CSS `--*`、其他复合束项。

---

## 2. Core slot 生命周期

全部改动在 `crates/core/src/list.rs`（+ 极少 `scene/dynamic.rs` 触点）。重写面 Medium，集中 ~6 函数。

### 2.1 数据结构

```rust
struct Slot {
    node: NodeId,         // 永驻（高水位生命周期）
    item_index: usize,    // 当前绑的 item；parked 时保留上次值（stale，复用参考）
    parked: bool,         // true=休眠（display:none override set）
    // reuse_key 不存这——永久 ordinal，create 时算一次写进 node.reuse_key
}

struct ListState {
    slots: Vec<Slot>,     // 高水位：0..high_water，永缩
    // free: Vec<NodeId>  ← 删除（detach 模型遗物）
    heights: HeightCache, // 按 item_index 缓存（不变；parked slot 不贡献测量）
    pending_binds: Vec<(NodeId, usize)>,  // 不变
    visible: Range, anchoring_active, ... // 不变
}
```

**slot ordinal = `slots` 里的 index**，create 时分配，永不复用给别人。high_water = `slots.len()`，只增不减（约束 e：无驱逐）。

### 2.2 分配模型（高水位）

- `enter_data_driven`：克隆模板建**初始 batch**（`INITIAL_SLOTS` 个，全 parked），挂 ul（head/tail spacer 之间），全 `set_inline_override("display:none")`。
- 扩容：可见区 slot 不够 → clone 新 slot、attach ul、push `slots`、ordinal = `slots.len()-1`。**只增**。
- 不缩：组件销毁才整批释放（走现有 `remove_node(ul)` 释放含 parked 子树，`list.rs:1470` 测过的路径）。

### 2.3 plan_visible（重写 Phase B/C）

算可见 item 区 `[start, end)`（逻辑不变）。然后**不 detach**，改为标 active/parked：

```
对每个 slot s in slots:
    if s.item_index ∈ [start, end):  keep-active（不动）
    else:                            mark-to-park
对 [start, end) 里还没 slot 绑的 item：collect 待绑（to_bind）
```

**关键**：parked slot 不出 `slots`、不出 ul.children，只等 execute 翻 display。`to_free` / `remove_child` / `ls.free.extend` **全删**。

### 2.4 execute_visible（重写）

```
对每个待绑 item_index（to_bind）:
    优先挑 item_index 已等于它且 parked 的 slot → unpark（display 复位 + bind）
    否则挑任意 parked slot → unpark + rebind 到 item_index
    都没有 → clone 新 slot 扩容（2.2）+ bind
对每个 mark-to-park 的 slot:
    set_inline_override(slot, "display:none") + s.parked = true
对每个 keep-active 且 item 变了的 slot: rebind（走 pending_binds）
pending_binds 入队（tick-drain → C# BindItem）
```

### 2.5 reuse_key 稳定化

`encode_reuse_key(list_ordinal, ordinal) = ((list_ordinal + 1) << 16) | (ordinal & 0xFFFF)`——**ordinal 是 slots index，create 时定，永不复用**。`set_reuse_key` 在 clone 新 slot 时调一次，之后永不再调（park/unpark/rebind 都不碰它）。根治坑 182 子因②（slot_idx=slots.len() 旋转）。

全仓库无 reuse_key decoder（MirrorPool/FrameBlob 当 opaque u32），16-bit slot 半位绰绰有余。

### 2.6 display toggle 机制 + 时序

- **park**：`set_inline_override(scene, slot, "display:none")` → 写 inline_override + inline_set bit。
- **unpark**：`unset_inline_override(scene, slot, "display")` → 清 override，cascade 回落 slot 真实 display（作者写的 flex/block）。**不用 `set("display:block")`**——那会盖掉作者样式。
- **时序红利**：`execute_visible` 在 `stage.rs:933`（step 3.6），**早于** `rematch`(938) → 同帧 rematch 把 override 拷进 `node.style` → 同帧 solve(taffy 跳) + render(剪枝)。**零延迟**，比 Dropdown/TabList 的"下帧生效"还顺。

> **注意**：display:none 只管 **layout 跳过**。parked slot 的 blob 存在性由 build_blob 的 keepalive 追加段保证（§3），与 render 剪枝无关——display:none 剪掉的是 parked slot 的 mesh（正确，本就不渲染），keepalive 让 MirrorPool 知道"留 GO"。

### 2.7 notify_inserted / removed / moved（item 数据变化）

当前是 detach/recycle + shift item_index。新模型 **park/shift**：
- `notify_removed(range)`：受影响 slot → park（不 detach）；其余 slot 的 item_index 按 shift 规则重映射（>end 的 -= count）。
- `notify_inserted(at, count)`：item_index 重映射；需要新 item 进可见区的 → 找 parked slot unpark + bind。
- `notify_moved`：item_index 重映射 + 视情况 park/unpark。

算法骨架不变（heights/item_index shift），但"recycle=detach"全换成"recycle=park"。**语义锁定：item 变化绝不 detach slot**。算法细节留实现 plan。

### 2.8 collect_heights 改动

遍历 `slots` 时 **skip parked**（parked slot display:none → taffy 零尺寸 → layout_rect.h 无效）。HeightCache 仍按 item_index 缓存，active slot 测得的真高度更新它（逻辑不变）。

### 2.9 不变量与边界

- **slot parent 永远是 ul**（从 create 到组件销毁）。
- **parked slot 在 ul.children 里的物理位置任意**（不保证序）→ `assert_all_slots_well_parented` 的"严格升序"断言放宽为"head spacer 第一、tail spacer 最后、中间 slot 任意"。
- **head/tail spacer 不受影响**（非 slot，定位锚点不变）。
- **Get/Query 在 slot 内调用**（driver 的 blessed 路径）：Query 子树 DFS 天然只搜本 slot；L1 后 Get 也是（子树 DFS 从 slot 起）。parked slot 的 id 由本 `slot.Get` 正确命中。
  - ⚠️ **L1 残留**：`component.Get<T>("id")` 会穿透进所有 slot（含 parked）返回首个匹配——这是 L1 纯子树 DFS 不识别 scope 边界所致，留 L3 解（详见 §5.4）。driver 应用 `slot.Get`/`slot.Query`，勿在组件级 Get slot 内部 id。

---

## 3. Blob 契约（parked bit）

### 3.1 双类 blob 条目

build_blob 产**两类**条目，node_count 含两者：

| 条目类 | 来源 | 内容 | MirrorPool 行为 |
|---|---|---|---|
| **active**（正常） | render_nodes（现有管线） | 全字段：mesh/header/transform/... | 正常渲染 |
| **parked keepalive**（新增） | `scene.lists` 遍历 `slot.parked==true` | 极简：`node_id` + `reuse_key` + parked bit；mesh_off=0/len=0；其余零 | 找 reuse_key 对应 GO → `SetActive(false)`、清 stale、跳过 header/mesh 上传 |

build_blob 在现有 render_nodes 循环后**追加一段 parked keepalive 循环**（~10 行），遍历各 list 的 parked slot，读 `node.reuse_key`（create 时 set 的稳定值），push 极简条目。render 管线**完全不动**（display:none 照常剪 parked slot 的 mesh）。

职责分离：layout=display:none（taffy 跳）；render=剪 mesh（正确）；blob=keepalive 让 MirrorPool 留 GO；pooling 决策在 core（约束 d）。

### 3.2 visible 字节 bit 布局（零 version bump）

`visible` 列（col 2，u8/节点）双用：

| bit | 含义 | active | parked | gone |
|---|---|---|---|---|
| bit0 | visible（要渲染） | 1 | 0 | — |
| bit1 | parked（留 GO 别渲染） | 0 | 1 | — |
| (absent from blob) | gone（销毁） | — | — | ✓ |

bit2-7 预留。

### 3.3 改动清单

**Rust `crates/ffi/src/blob.rs` build_blob**（追加 keepalive 循环）：
```rust
// 现有 render_nodes 循环不动。之后追加：
for ls in scene.lists.values() {
    for s in ls.slots.iter().filter(|s| s.parked) {
        let n = scene.get(s.node);
        col_node_id.push(s.node.into());
        col_reuse_key.push(n.reuse_key);
        col_visible.push(0b10);               // bit1=parked, bit0=not visible
        col_change_level.push(0);             // Skip
        col_payload_kind.push(0);             // 无 mesh
        col_mesh_off.push(0); col_mesh_len.push(0);
        // 其余列填零（MirrorPool 不读）
    }
}
// node_count = render_nodes.len() + parked_count
```

**C# `FrameBlob.cs`（3 行）**：
```csharp
public bool Visible(int i) => (_buf[ColOff(2) + i] & 0x01) != 0;   // 改：!= 0 → & 0x01
public bool Parked(int i)  => (_buf[ColOff(2) + i] & 0x02) != 0;   // 新增
```

**消费者**：`MirrorPool.cs`、`UnityLoomBackend.cs` 调 `blob.Visible(i)`——语义不变；新增 `Parked(i)` 分支。**零调用方意外改动**。

### 3.4 为什么不加列、不 bump version

- 不加列：parked 复用 visible 字节 bit1，列数仍 21，header 长度不变。
- 不 bump version：列 offset 公式 `ColOff(idx)` 不变，所有 arena offset 不移位，snapshot 测试零 rebake。
- `blob.rs` 的 `columns` 表（28-49 行）不改（visible 仍是 1 字节列）——双用是值语义。

---

## 4. Unity MirrorPool（parked 分支）

改动面小，全部集中在 `MirrorPool.cs` 的 `Sync` 主循环。**UpdateHeader / UploadMesh / TearDown / stale 逻辑全不动**。

### 4.1 Sync 循环重构（核心改动）

当前（`MirrorPool.cs:72`）：`if (!blob.Visible(i)) continue;`——非 visible 直接跳过 → stale → 销毁。parked 节点 bit0=0 会被误杀。改成**三态优先**：

```csharp
for (int i = 0; i < n; i++)
{
    bool parked  = blob.Parked(i);     // bit1
    bool visible = blob.Visible(i);    // bit0

    // ① PARKED keepalive：留 GO、休眠、不渲染
    if (parked)
    {
        uint poolKey = blob.ReuseKey(i);
        if (_poolByReuse.TryGetValue(poolKey, out var ro))
        {
            ro.Stale = false;                              // 不销毁
            if (ro.Go.activeSelf) ro.Go.SetActive(false);  // 幂等休眠
        }
        // 无现有 GO（从未 active 过的 slot）→ 不创建，直接跳过（lazy，4.2）
        continue;
    }

    if (!visible) continue;   // 真正不可见的非 parked 节点（边缘，保持旧行为）

    // ② ACTIVE：现有逻辑不动（kind 检查 / NewRenderObj / UpdateHeader / UploadMesh）
    //    仅补：复用的 GO 若休眠态，唤醒
    ... existing active path ...
    if (!ro.Go.activeSelf) ro.Go.SetActive(true);   // reactivate（4.3）
}
```

### 4.2 Lazy GO 创建（不预建 high-water 个 GO）

parked keepalive 若 `_poolByReuse` 里没对应 GO → **不创建**，直接 `continue`。GO 仍按原逻辑在**首次 active** 时 `NewRenderObj` 创建。

理由：`enter_data_driven` 初始 batch 全 parked，但此时不该建任何 GO。GO 只在 slot 首次进可见区时建。parked 池里留的是"曾经 active 过、现在休眠"的 GO。→ 内存 = 实际曾可见 slot 数，不是 high-water 上限。和 fgui 的 lazy 一致。

### 4.3 Reactivate 契约（parked→active 转换）

slot 从 parked 翻 active 那帧：
- **MirrorPool 侧**：active 分支找到现有休眠 GO → `SetActive(true)` + 走 UpdateHeader（位置可能变了）+ UploadMesh（若 Full）。
- **core 侧契约**：**parked→active 转换帧，change_level 必须 = Full**。保证机制：parked keepalive 条目 mesh_off=0/len=0（无 mesh），reactivate 帧 active 条目带真 mesh → mesh payload hash 必变 → build_blob 自然算出 Full。**实现 plan 加断言**：reactivate 帧 change_level ≠ Full 即 panic（防 regression）。
- GO 的 Mesh/4×List buffer 在休眠期间原样保留，reactivate 后 UploadMesh 走 Clear+fill 复用（零新 alloc）。

### 4.4 不变的部分

- `UpdateHeader` / `UploadMeshOrText` / `UploadMesh` / `TearDown` / `NewRenderObj`：全不动。
- `_poolByNodeId` / `_poolByReuse` 双 dict keying：不动。slot 的 GO 永远在 `_poolByReuse`（reuse_key>0），跨 parked/active 稳定。
- `DumpState`（F8 诊断）：**顺手补一列** `active={ro.Go.activeSelf}`，让 dump 看出哪些 GO 在休眠（调试 parked 用）。

### 4.5 边界与不变量

- **稳态滚动零 GO create/destroy**：active 集合稳定时，parked 集合也稳定 → 既无 NewRenderObj 也无 TearDown。坑 182 直接验收点（约束 a）。
- **GO 永不主动销毁 parked**：只有 gone（blob 缺席）才 TearDown。组件销毁走 `MirrorPool.Clear()`（现有路径）。
- **SetActive 幂等守卫**：两分支都加 `if (activeSelf)` / `if (!activeSelf)` 守卫，避免每帧无谓调用。

---

## 5. Scope（仅 L1，L2/L3 defer）

### 5.1 范围说明

- **L1（子树查找）**：修运行时 `find_node_by_id` 全局首匹配 bug。✅ 本次做。
- **L2（per-scope ID 去重）**：核实后发现打包期 `validate_id_uniqueness`（`structural.rs:107`）本就 per-template，克隆 slot 不 false-error，**不阻塞**。其真实意义是"嵌套组件各自 scope 内 id 唯一"——属 IsScopeRoot 边界系统，跟 L3 一起 defer。
- **L3（IsScopeRoot 完整边界 + Shadow DOM）**：defer（roadmap §4 已收录）。

### 5.2 L1：子树查找 FFI

**问题**：`TryGet<T>`（`Nodes.cs:210`）调全局 `find_node_by_id`（scene.nodes 首匹配）→ 可能命中别的 slot 的同名 id → `IsInSubtree` 后过滤失败 → 抛 "not found"。blessed 的 `Query<T>()`（子树 DFS）没这问题，但 `Get<T>("id")` 有。

**修法**：加子树起点查找 FFI，DFS 只搜 root 子树：

```rust
// crates/core/src/stage/ 或 scene/ — 新增
pub fn find_node_by_id_in_subtree(scene: &Scene, root: NodeId, id: &str) -> Option<NodeId> {
    // DFS from root through children；命中 root 自身或任一后代的 id_attr 即返
    // 纯结构遍历，不判 display:none（slot.Get 时不管目标 display 状态）
}
```

```c
// crates/ffi_c — 新增 FFI（照现有 find_node_by_id 签名加 root 参数）
pub extern "C" fn loomgui_stage_find_node_by_id_in_subtree(
    stage: StageHandle, root: u32, id_ptr: *const u8, id_len: usize,
) -> u32;   // NodeId 或 RootSentinel(not-found)
```

### 5.3 C# 投影层改（Nodes.cs TryGet/Get）

```csharp
public bool TryGet<T>(string id, out T node) {
    ...
    fixed (byte* p = idb)
        // 旧：candidate = Native.loomgui_stage_find_node_by_id(h, p, len);
        candidate = Native.loomgui_stage_find_node_by_id_in_subtree(h, _id, p, len);
    if (candidate == RootSentinel) return false;
    // IsInSubtree 后过滤现在冗余（子树 DFS 已保证）→ 删掉，或留 Debug.Assert 防回归
    ...
}
```

- **签名零改**（约束 b）：`Get<T>`/`TryGet<T>` 公共签名不变，只换底层 FFI。
- **旧 `loomgui_stage_find_node_by_id` FFI**：保留（实现 plan grep 确认无生产 caller 后可删）。

### 5.4 L1 已知残留（→ L3，文档标注）

L1 是**纯子树 DFS，不识别 scope 边界**。两个残留 L3 才彻底解：

1. **嵌套组件**：`slot.Get` 若 slot 内含嵌套组件，DFS 会穿进去（L3 的 IsScopeRoot 边界才会停）。列表场景 slot 一般不含嵌套组件，实际不触发。
2. **component.Get 命中 slot 内部 id**：`myComponent.Get<T>("badge")` 会 DFS 进所有 slot（含 parked），返回首个匹配。语义上"组件级 Get 不该穿透到 list item"是 L3 契约。blessed 路径是 `slot.Get`，不踩这个。

两个残留都**不影响列表虚拟化生产用法**（driver 调 `slot.Get`/`slot.Query`），标注进 fence.md/public-api.md scope 章节。

---

## 6. 测试策略

按"编码机能跑的先跑、真机 defer"分层，映射约束 (a)(b)(c)(d)。

### 6.1 Core 单测（Rust，`cargo test -p loomgui_core`，编码机）

| 测点 | 性质 |
|---|---|
| **改写 ~7 个旧 free-池测** | free→parked 语义迁移；断言从 `ls.free.len()` 改 `slot.parked` |
| **parked 行为新测** | slot park 后 `parent==ul` 不变、`slots.len()` 只增、display:none override 已 set |
| **collect_heights skip parked** | parked slot 不贡献高度测量 |
| **notify_* park/shift** | item insert/remove/move 不 detach slot，只 park/shift item_index |
| **taffy Display::None 保险** | 直接单测验 parked slot 零尺寸 + 兄弟重排（目前仅 Dropdown 间接证） |
| **insert_before 排序保险** | parked slot 留挂时 spacer 间排序仍成立 |
| **tick 时序不变量** | process→rematch→solve→...→build 顺序不变 + 一次 solve/帧 |

### 6.2 FFI/blob 测（Rust，`cargo test -p loomgui_ffi`，编码机）

| 测点 | 性质 |
|---|---|
| **parked bit 往返** | build_blob 写 bit1，FrameBlob 读回 Parked=true/Visible=false |
| **双类 node_count** | active + parked keepalive 总数正确；keepalive 条目 mesh_off=0/len=0 |
| **现有 blob 测不动** | `blob_v9_round_trips_reuse_key` 等（0 影响） |

### 6.3 L1 scope 测（Rust core + headless C#）

| 测点 | 层 |
|---|---|
| `find_node_by_id_in_subtree` 命中 root 自身/后代/外子树返 None | core 单测 |
| N slot 同名内部 id，`slot[i].Get<T>("id")` 各命中本 slot | headless（Spec-4a harness） |
| reactivate 帧 change_level=Full 断言（§4.3 契约） | core（防 regression） |

### 6.4 Unity MirrorPool 测（`MirrorPoolTests.cs`，Unity EditMode，编码机）

| 测点 | 性质 |
|---|---|
| **parked keepalive → SetActive(false) 留 GO** | park 后 GO active=false、pool 不缩 |
| **reactivate → SetActive(true) + mesh Full 重传** | park→active 帧 mesh 重上传 |
| **lazy：parked 无现有 GO 不预建** | 初始 batch 全 parked 时 GO 数=0 |
| **稳态滚动零 churn** | active/parked 集合稳定帧，NewRenderObj/TearDown 计数=0（坑 182 单元级验收） |
| **DumpState active 列** | F8 诊断输出含 active 标志 |

### 6.5 验收门（映射约束）

| 约束 | 验收 | 跑在哪 |
|---|---|---|
| **(a) mail 滚动零 GO churn** | Unity Profiler 实测稳态滚动帧 GO create/destroy=0 | **真机 defer**（PlayMode + showcase pkg） |
| **(b) 公共 API 冻结** | `tests/dotnet/LoomGUI.PublicApi` 编译门绿 | 编码机（dotnet） |
| **(c) core 全绿 + tick 不变量** | 6.1 全绿 + 时序断言 | 编码机（cargo） |
| **(d) 跨引擎可移植** | code review 验"池化决策全在 core、无 UnityEngine 进决策路径" | review（无运行时门） |

### 6.6 真机 defer 清单（家里机/Unity PlayMode）

- mail/inventory showcase 滚动 Profiler 取证（GO churn=0 + 帧时间）。
- showcase PlayMode 回归（坑 182 视觉症状：item 不消失、不卡顿）。
- F8 双层 dump 对比（好/坏态 blob + MirrorPool.DumpState）。

编码机把 6.1-6.4 + PublicApi 全绿锁死；真机只验集成层（6.5a + 6.6），和 Spec-4b 两机约束一致。

---

## 7. 迁移、文档同步、落盘

### 7.1 兼容性与迁移

| 项 | 影响 | 动作 |
|---|---|---|
| **pkg 版本** | 零 bump | blob 双用 visible 字节；list.rs 改动全运行时（data-driven 由 ItemCount setter 触发） |
| **.dll 重编** | 需要 | core(list.rs) + ffi(blob.rs + 新 find_in_subtree FFI) → `cargo build -p loomgui_ffi_c --release` + `xtask sync-bindings` + 拷 .dll |
| **C# 改动** | 3 文件 | `FrameBlob.cs`(3 行) + `Nodes.cs`(TryGet 换 FFI) + `MirrorPool.cs`(parked 分支) |
| **公共 API 冻结** | 零破 | Get/TryGet 签名不变，PublicApi 编译门绿（约束 b） |
| **showcase pkg 重打** | 不需要 | 改动纯 runtime；base_style 不变 |

### 7.2 文档同步清单

| 文档 | 改动 |
|---|---|
| **设计文档**（本文档） | `docs/superpowers/specs/2026-08-05-pooled-slot-lifecycle-design.md` |
| **roadmap §4 坑 182 条目** | 加交叉引用：指向本文档；注明 L1 本次做、L2/L3 留复合束 |
| **fence.md / public-api.md scope 章** | 标注 L1 残留：① 虚拟化 ul 禁 `:nth-child`；② `Get<T>("id")` slot 内可用但 component.Get 穿透 list item（L3 解）；③ slot 寻址用 `slot.Get`/`Query`（blessed） |
| **pitfalls.md 坑 182** | 实现完成后更新状态：根因（reuse_key 旋转 + GO stale-destroy）→ 解法（parked-but-attached + 稳定 ordinal + 持久 GO 池） |

### 7.3 后续（writing-plans）

设计 approved 后，调 writing-plans skill 把 §2-§6 拆成可执行 task 序列（core 生命周期 → blob 契约 → Unity MirrorPool → L1 scope → 测试 → .dll 闭环），带 review checkpoint。

---

## 8. Deferred（防遗忘，双重记录）

| 项 | 为什么 defer | 谁来接 | 触发条件 |
|---|---|---|---|
| **L2 per-scope ID 去重** | 打包期 per-template 校验已够（克隆不 false-error）；真语义是嵌套组件 scope，属 IsScopeRoot 系统 | roadmap §4 复合束（已收录） | 第一个嵌套组件同 id 用例 |
| **L3 IsScopeRoot 完整边界** | L1 子树查找已解 list 场景；完整边界（不穿透嵌套组件/list item）是独立 feature | roadmap §4 复合束（已收录） | 嵌套组件 / component.Get 进 list item 成痛点 |
| **L3 Shadow DOM 样式隔离** | 与 IsScopeRoot 同系统；模板内部选择器作用域边界 | roadmap §4 复合束（已收录） | 第一个需样式隔离的组件系统 |
| **L1 残留①嵌套组件穿透** | slot 一般不含嵌套组件，实际不触发 | L3 | slot 内出现嵌套组件 |
| **L1 残留②component.Get 进 slot** | blessed 路径是 slot.Get，不踩 | L3 | 作者在组件级 Get slot 内部 id |
| **内存驱逐（约束 e 已认可不做）** | 无上限 dormant 池可接受；mail high-water ~50 slot 内存可忽略 | 后续可选 | 长会话 + 海量 slot 场景出现 |

---

## 9. 参考框架取证

| 框架 | 池化思路 | 借鉴点 |
|---|---|---|
| **FairyGUI-Unity**（`temp/`，已查） | GO 池 = reparent 到休眠 `_manager` Transform + 两级复用；永不 destroy（只 Dispose） | Unity 侧 GO 池"永不 destroy"语义；lazy GO 创建 |
| **RmlUi**（`temp/`，已查） | `display:none` = parked but attached，元素永驻树，layout + render 一处 early-return 跳过 | core 侧"不 detach，标 + 跳"模型；display:none 机制复用 |
| **Unity UI Toolkit**（知识） | `RecyclePool` + bind/unbind 生命周期 | slot 换绑数据 = BindItem（已有） |
| **Unity UGUI**（知识） | `ObjectPool<T>` capacity cap + 驱逐 | 本次不做（约束 e），留作后续内存治理参考 |

---

## 10. 决策日志

- **为何 (B) 一步到位而非 (A) Unity-only**：core detach/free 模型是定时炸弹（游离帧渲染行为未独立证伪）；一步到位根治 + 终态最干净。
- **为何方案1（Flag-in-place）而非方案2（Dormant-child）**：RmlUi 实证 flag+skip 最干净；slot parent 永稳简化大量推理；display:none 机制现成。
- **为何 L2 移出本次**：核实打包期不 false-error，不阻塞；YAGNI。
- **为何 blob 双用 visible 字节而非加列**：零 version bump、零 snapshot rebake、3 行 C# 补丁。
- **为何 unpark 用 unset_inline_override 而非 set("display:block")**：让 cascade 回落作者真实 display，不盖样式。
