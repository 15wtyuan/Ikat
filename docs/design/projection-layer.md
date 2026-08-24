# LoomGUI C# 投影层契约

> **定位**：公共 API（[public-api.md](public-api.md)）的实现机制契约。跨摸黑+三束有效——不是用过即丢的 spec，是实现层的长期不变量。
>
> **一句话**：对象树真身在 Rust 核心，C# 语义对象是它的 OOP 投影（裹 NodeId 的遥控器 + 攒批回写 + 稀疏镜像）。不推翻 v1 的 FrameBlob/MirrorPool 单向渲染管线，在其上加回写层。

---

## 1. 架构归属：真身在 Rust

### 1.1 为什么真身不在 C#

两个硬约束逼出此结论：

- **Godot / UE 是重心**（不只 Unity）。对象树真身若在 C#，UE（C++）、Godot（GDScript）复用不了，得各自重写一整棵树（结构 + cascade + 事件 + 生命周期 + 身份）。「跨引擎共享核心」在两个重心引擎上直接破产。
- **后端要薄**。对象树在后端语言层 = 后端厚。后端薄的唯一实现方式，就是把厚的东西（对象树、cascade、身份、事件、布局）全压进 Rust 核心，后端只做镜像渲染 + 输入采集。

### 1.2 分层

```
Rust 核心（真身）：parse → style(cascade) → scene(类型化 Node 树) → layout → render
  - 持有对象树、NodeId、样式、布局、world matrix、mesh 几何
  - 每帧 tick_and_render → build_blob（SOA 扁平帧数据）
C# 投影层（引擎无关）：语义对象树 = Rust 树的 OOP 投影
  - Node/Button/Style/... 裹 NodeId，方法转发 FFI
  - 稀疏镜像（只存写过的属性）+ 脏标记 + NodeId→对象缓存
引擎后端（Unity/Godot/UE）：MirrorPool 镜像渲染 + 输入采集 + 纹理注册
```

C# 投影层引擎无关，Unity 和 Godot-C# 共享；UE-C++ / Godot-GDScript 各写一份等价投影（同样薄，因真身在 Rust，投影只是镜像 + 转发）。

### 1.3 v1 现状（此模型已跑通大半）

- v1 渲染路径已是「真身 Rust + 薄后端 + 每帧一次 SOA blob 过桥」：`stage.Tick(dt)` → `build_blob` → C# `FrameBlob` 读定长列 SOA（列数随格式版本增列，以 `crates/ffi/src/blob.rs` 为准）→ `MirrorPool` 对齐 GameObject。**单向 Rust→C# 成熟**（change_level 三级、reuse_key 复用）。
- v1 回写走命令式 FFI 透传（结构操作 `AppendChild`/`Instantiate`、资源 `SetSrc`、动画 `Tween`、文本 `SetText`...）；**Style 属性走 inline override 便签层**（`set_inline_override`/`unset_inline_override`，4a 落地），**不走 `set_style`**——`set_style` 写 `base_style` 污染设计期基线，已退役（见 `unity/package/Runtime/Projection/StyleMirror.cs:17`）。
- **投影层的增量 = 在 v1 命令式 FFI 上加 OOP 封装 + 攒批回写**，不推翻管线。

---

## 2. 六点机制设计

### 2.1 回写时序 = 混合（Q30）

- **结构操作即时过桥**：`AddChild`/`InsertChild`/`RemoveChild`/`Instantiate`/`Dispose`。必须即时——要立刻拿到 Rust 分配的 NodeId 建立 C#↔Rust 映射；且低频，即时无所谓。
- **属性写攒批**：`Style.X = v` / `Transform.X = v` 只改 C# 镜像 + 标脏，帧末一次性 flush。高频（拖拽/动画/批量改样式），攒批把 N 次过桥压成每帧每脏节点一次。StyleMirror setter 标脏不立即调 `set_inline_override`，NodeTransform.Store 标脏不立即调 `set_transform`；帧末（`LoomHost.Step` flush seam / `UIContext.FlushPendingWrites`）遍历 NodeRegistry dirty 集中过桥。
- **flush 时机**：在 `LoomHost.Step(dt)` 中 tick **之前**——先 flush 脏属性 → tick（solve）→ borrow_frame 读回。与 tick 时序契合。

### 2.2 攒批编码（Q31）

- **Style 属性**：flush 时把脏属性拼成 CSS 串（`"width:100px;left:20px"`），一次 `set_inline_override` 过桥（4a 新建，非 `set_style`），Rust `apply_css` parse 后**写入 inline override 便签层**——下帧 rematch 应用，优先级 > 动态规则 > `base_style`，故不污染设计期基线。撤销走 `unset_inline_override`。**标记为已知优化点**：字符串序列化+parse 往返若 profile 出热点，换二进制 batch FFI（`set_style_props(nodeId, propId[], values[])`，Rust 加绕过 parse 的直写路径）。
- **Transform**：走**独立数值 FFI `set_transform`（纯 f32：pos/scale/rot/origin）**，不走字符串、不触发 solve。这是必需通路（Transform 非 CSS 属性），非优化。C# `NodeTransform.Store` 标脏 + 帧末 `FlushTransform` 送全 9-arg（含 origin）。

### 2.3 C# 镜像 = 稀疏 override 层（Q32）

- `NodeStyle` 内部只存 setter 写过的属性（+值），用稀疏 map/bitset 记「哪些被写过」。大多数节点用户没 C# 写过任何属性 → 零缓存，镜像极轻。
- Rust 侧对应面是数据导向的固定 `ResolvedStyle` + inline_set 位图（纯运行时 transient，不进 pkg.bin——设计期无此概念）；与 C# 稀疏镜像的形态不对称是**有意设计**（两端各自惯用表达）。
- 读写过的 → 返回缓存值（即时，读到刚写的）。读没写过的 → 返回 `Unset`（Style getter 只反映 inline override，见 public-api §3.1）。要 computed 走 Geometry。
- 脏集 = 写过且未 flush 的属性。flush 后清脏标记，但**保留值**（inline override 一直生效，下次读还返回它）。
- `Style.X = Unset()` 撤销 → 从稀疏 map 移除该键 + 标脏，flush 时告诉 Rust 移除该 inline override 回落 CSS。

### 2.4 对象身份稳定（Q33）

- Stage 持 `NodeId → Node` **强引用**缓存。`Get`/`Query`/`Parent`/`Children` 返回同一对象。
- 对象上挂着活状态（事件订阅、OnUpdate、稀疏 style 镜像、ClassList）——**必须稳定**，否则再次 Get 就丢。强引用不能被 GC 悄悄回收（回收 = 订阅丢失）。
- 生命周期绑 Rust 节点：节点在 → C# 对象在；节点销毁（Dispose/RemoveNode）→ 显式从缓存移除 + 置 `IsDisposed=true`，此后操作抛 `ObjectDisposedException`。

### 2.5 子对象投影（Q34）

- `NodeStyle` / `NodeTransform` = **class + 内部 owner Node 引用 + 每 Node 稳定单一实例**。`node.Style` 返回挂在 Node 上的同一实例（稀疏镜像存此）。写操作经 owner 引用标脏到正确 NodeId。（若设成 struct，`node.Style.Width=X` 改副本、写不回——故必须 class。）
- `NodeGeometry` = **readonly struct 快照**。只读、纯值、不需回指 owner。值优先从**每帧 blob 镜像**读（零过桥，读上帧 solve 结果）；blob 缺的字段退即时 FFI（`get_node_layout_rect`/`get_node_world_matrix` 已有）。具体哪些字段进 blob R2 实现时定。

### 2.6 读时序（Q35）

| 读 | 来源 | 时效 |
|---|---|---|
| `Style.X`（写过的） | C# 稀疏镜像 | 即时（同帧读回写入值） |
| `Style.X`（没写过） | — | 返回 `Unset` |
| `Transform.X` | C# 镜像 | 即时（同 Style） |
| `Geometry.*` | 每帧 blob 镜像 | 最近完成的 solve（滞后一帧，同 web reflow） |

改 Style 后立刻读 Geometry **不反映本帧改动**——要等帧末 flush → tick → solve 后的下一帧。不提供 `ForceLayout`/同步 solve（YAGNI，改这帧读下帧）。

---

## 3. 实现状态

投影层落地进度（4a 已完成核心通路，4b/后续补完）：

### 3.1 已完成

1. **`set_inline_override` / `unset_inline_override` FFI** ✅ — core 便签层折进 rematch 单 set_map，C# StyleMirror 过桥。撤销走 `unset_inline_override`（C# `Unset()` 已暴露，立即调清 bit）。
2. **NodeId→Node 缓存 + 生命周期** ✅ — `NodeRegistry` Dictionary\<uint,Node\> + `Dispose` 清缓存 + `IsDisposed` 守卫。NodeRegistry 同时持攒批 dirty 集合（§2.1）。
3. **Geometry 直读 FFI** ✅ — `get_node_layout_rect` / `get_node_world_matrix` 即时 FFI（blob 缓存推后，YAGNI）。
4. **`set_transform` FFI** ✅（Task 7）— 纯 f32 9-arg（tx,ty,sx,sy,rot,ox,oy），写 user_transform 不触发 solve。
5. **攒批 flush seam** ✅（Task 9）— StyleMirror setter 标脏不立即过桥；NodeTransform.Store 标脏 + 帧末 `FlushTransform`。`LoomHost.Step()` 中 flush 位在 tick 前（`UIContext.FlushPendingWrites` 遍历 dirty 集）。

### 3.2 待办（按优先级）

1. **标记优化点**：字符串 flush 若成热点换二进制 batch FFI（`set_style_props(nodeId, propId[], values[])`，Rust 加绕过 parse 的直写路径）——§2.2。

## 4. 对冻结签名的影响

投影层是纯内部机制，**不改公共签名**：

- `NodeStyle`/`NodeTransform` 已是 `sealed class` ✅、`NodeGeometry` 已是 `readonly struct` ✅——冻结现状恰好正确。
- 投影层只加内部字段（owner 引用、稀疏镜像、NodeId），不进公共表面。
- 新增公共表面（`Pick`、`Focusable`、`UnloadPackage` 等）来自 API 契约 grill（public-api.md），非投影层引入。

这验证：冻结签名在「真身 Rust + C# 投影」模型下结构成立，Style/Transform/Geometry 的 class/struct 选择恰好正确。

---

## 5. 已锁定决策

1. 对象树真身在 Rust 核心（被 Godot/UE 重心 + 薄后端逼出），C# 是 OOP 投影。
2. 不推翻 v1 FrameBlob/MirrorPool 单向管线，在其上加回写层。
3. 回写时序混合：结构即时（拿 NodeId）+ 属性攒批（帧末 flush，tick 前）。
4. Style flush 走 `set_inline_override` 便签层（非 set_style——后者写 base_style 污染设计期基线，4a 退役；标记优化点不变）；Transform 走独立数值 FFI。
5. C# 镜像 = 稀疏 override 层（只存写过的，读没写过返回 Unset）。
6. 对象身份稳定：NodeId→Node 强引用缓存，销毁时显式清除。
7. Style/Transform = class + owner 引用；Geometry = readonly struct blob 快照。
8. 读时序：Style/Transform 即时读镜像；Geometry 滞后一帧；无 ForceLayout。
9. 性能问题存在但走针对性优化（攒批 + 二进制 batch 是明确路径），非架构否决理由。
