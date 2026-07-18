# Spec-4a：C# 投影层 + core inline override 层（后端对象层 ④ 第 1 棒）

> **创建**：2026-07-17
> **状态**：设计已与用户分节确认（§1–§5），待写实现 plan
> **上下文**：④ 后端对象层的第 1 棒（共 2 棒）。上一棒 Spec-3 ③（cascade 收尾 + 查询出口）已完成。本棒做整个 C# 投影层 + 它依赖的 core inline override 层，全部本机 headless 验。第 2 棒 Spec-4b = 终点线2 Unity 真机验收。
> **权威契约**：[public-api.md](../../design/public-api.md)（公共签名）+ [projection-layer.md](../../design/projection-layer.md)（投影机制）+ [main-design.md](../../design/main-design.md)。本 spec 是这两份契约的"实现第 1 棒"，不改公共签名。

---

## 1. 范围分解：为什么 2 棒

④（后端对象层）原估很大（336 NIE + 事件层 + harness + Unity 验收），但查证后多数机械（旧命令式 FFI 全套可转发、Rust 缺口有存储是小工程）。按 **"本机 / Unity"边界**切 2 棒（= 两台机串行约束的自然分界）：

| | Spec-4a（本棒） | Spec-4b（下一棒） |
|---|---|---|
| 内容 | 整个 C# 投影层 + core inline override 层 + headless harness | 终点线2 Unity 真机验收 |
| 验收机 | 编码机（headless 驱动真 dll） | 家里机（PlayMode） |
| Unity 渲染/输入 | 不碰 | MirrorPool/InputCollector/EventHandler 零改复用 |

不在"事件层 / Unity"之间切——会让事件层本机能验却拖到 Unity，白费两台机往返。

---

## 2. 关键查证发现（修正文档假设）

三个查证改变了 4a 的性质（从"纯 C# 转发"变成"C# 投影层 + core 改动"）：

1. **`set_style` 写 `base_style`，不是 inline override 层**（`crates/core/src/scene/dynamic.rs:260`）：`apply_css` 逐条 `apply_decl` 增量合并进 `base_style`（打包期烘焙的 rematch 基线），**只会加/改，不会删**。core **没有独立的 inline override 维度**。projection §2.2"复用现有字符串 FFI 零改动"和 public-api §3.1"Style 是 inline override 层 + Unset 撤销"都假设 core 有这层——实际没有。
2. **FrameBlob 21 列无 rect**（`unity/package/Runtime/FrameBlob.cs:36`）：只有 world matrix 6 列 + mesh，没有 layout rect 列。Geometry blob 缓存要加列升版本（中等），4a 直读 FFI。
3. **core 有全部所需存储**（`crates/core/src/scene/node.rs:217`）：`children: Vec<NodeId>`、`classes: Vec<String>`、`parent/kind/id_attr` 都在。补 child 遍历 / class 操作 FFI 是小工程（补导出，非重构）。

**决策**：core inline override 层（便签层）**不推后，装进 4a**——set_style 写 base_style 是会埋雷的 hack（运行时改值污染设计期基线），便签层是 cascade 语义正确性的核心，一次做对比回头改两遍 rematch 省事（roadmap §3.3"语义要对、不能假绿"）。

> **期待校准**：④ 原被估成"填 718 行壳"，但冻结契约暗藏了 core 从没建的 inline override 层依赖——4a 在还这笔债，**注定比 4b（纯 Unity 验收 + 复用）重**。不是 spec 缺陷，是契约本来就该有的成本。

---

## 3. 设计

### 3.1 C# 投影壳架构

- **对象身份 + 缓存**：`NodeRegistry`（UIContext 持）= `Dictionary<uint, Node>` 强引用。`Get`/`Query`/`Parent`/`Children` 返回缓存的同一对象（事件订阅/OnUpdate/稀疏镜像挂对象上，必须稳定，不能被 GC 回收）。子对象 `Style`/`Transform`/`ClassList` 挂 Node 上，每 Node 稳定单一实例（必须 class，否则 `node.Style.Width=X` 改副本写不回）。
- **节点子类工厂 + lazy 构造**：`Instantiate`/`Create` 返回根 NodeId → `get_node_kind` → `NodeKind`→C# Type 映射表 → 造根、入缓存。子节点 lazy：首次访问 `Children`/`Parent`/`Get` 时，按 `get_children` + `get_node_kind` 递归造子 Node 入缓存。省启动开销 + 契合"Get 返回同一对象"。
- **子对象投影**：
  - `NodeStyle`：sealed class + owner 引用 + 稀疏镜像（§3.2）
  - `NodeTransform`：sealed class + owner 引用 + 镜像；**4a 标脏不 flush**（set_transform 推后），getter 读镜像
  - `NodeGeometry`：readonly struct 快照，**4a 直接 FFI 读**（`get_node_layout_rect`/`get_node_world_matrix`），blob 缓存推后（§5 defer）
  - `ClassList`：class + owner 引用 → `add_class`/`remove_class`/`has_class` FFI
- **typed 值转换**（纯 C# 静态工具）：`Length`→`"100px"`/`"50%"`/`"auto"`；`Color`→`"#rrggbb"`/`"rgba()"`；`Thickness`→`"t r b l"`。setter 经 seam 拼成 CSS 串。
- **生命周期**：`Dispose()` 递归子 + Rust `remove_node` + 缓存移除 + `IsDisposed=true`，此后操作抛 `ObjectDisposedException`。`RemoveFromParent()` Rust `remove_child`，保留节点 + 订阅（可重挂），不 Dispose。
- **`Create<T>` 白名单**：仅 `Container`/`AbsolutePanel`/`TextNode`/`Image` → `create_node`；控件/作用域根只能 `Instantiate`，非法 T 抛 `UIContractException`。

### 3.2 稀疏镜像 + seam（即时过桥不推翻的核心）

- **镜像**：NodeStyle 持稀疏镜像（只存 setter 写过的 typed 属性，key=CSS prop 名）。**getter 只查镜像**——写过→typed 值；没写过→`Unset`。纯 C#，不查 core，契约（"只反映写过的"）满足。
- **seam = `FlushInline()`**：拼镜像全部 → 一次 `set_inline_override(node, css)` 过桥。即时过桥版：setter 写镜像 + 立即调 `FlushInline()`。
- **升级攒批**：setter 写镜像 + 标脏；加帧末 `Flush()` 调 `FlushInline()`。seam 两边共用，**只改 setter 调用时机**——公共签名零改动。这是"即时过桥不推翻"的落点，标 `ponytail:` 注释写清升级路径。

### 3.3 core inline override 层（便签层，新增）

> **机制核实**（`crates/core/src/style/dynamic.rs:329-456`）：rematch 用**单个** `set_map`（动态规则 `apply_decl` + OR 进 `InheritedSet` 位图，376-386）；`propagate_inherited_rec` 是 tree-order DFS，把**父 effective 值**拷给"set_map 无该 bit 的子"（424-442）。inline_override **折进同一个 set_map** 即让继承自动正确——不用扩 propagate（review 挖出的简化，已读源码核实）。

- **Node 加字段**：`inline_override: ResolvedStyle`（便签值）+ `inline_set: InheritedSet`（哪些字段被运行时盖了）。**纯运行时 transient，不进 pkg.bin**（设计期无 inline override 概念；`inline_set` 位图类型同 `inherited_set`，但不打包期 bake——持久化语义相反）。Rust 侧用固定 ResolvedStyle+位图（数据导向、热循环局部性），C# 侧 NodeStyle 用稀疏镜像（OOP 投影）——两边不对称有意，各自合理。
- **rematch 改动（小）**：动态规则应用之后、写 `node.style` 之前，加一步——按 `inline_set` 的 bit 把 `inline_override` 对应字段拷进 `new_style`，并把 `inline_set` OR 进该节点 `set_map` 的 `inh`。
  - 效果：`node.style = base + 规则 + inline`（inline 最后应用 = 最高优先级 ✓）；`set_map` 含 inline 的继承 bit（该节点自身不被父覆盖 ✓）。
  - **继承零改**：propagate 把父 effective（已含 inline 值）拷给未声明的子——子的 inline 继承自动正确。"最高优先级"与"继承正确"同一机制一次达成。
  - **③ probe 不回归**：`inline_set` 默认空，新增的 inline 应用步对没设 inline 的节点是 no-op，Spec-3 的静态/class 规则断言不受影响。
- **新 FFI**：
  - `set_inline_override(node, css)`：`apply_css` 到 `inline_override` + 置 `inline_set` bit
  - `unset_inline_override(node, prop)`：从 `inline_set` 清该 bit → 下帧 rematch 自动回落到 base/rules
- **C# seam 落点**：Style setter 走 `set_inline_override`（**严禁走 set_style**——set_style 写 base_style 会污染设计期基线）；`Style.X = Unset()` 走 `unset_inline_override`——**契约完整，不再 throw**。

### 3.4 事件 typed 层

复用旧 `LoomEventHandler`（332 行，完整 demux：`EventType:byte` + `LoomEvent{nodeId,type,...}` + `DispatchPending` 分流 + listener 表）和 `tests/dotnet/EventRouter.cs`（纯 managed 路由算法）。**事件层是"在 demux 之上加 typed struct + On<T> 订阅"，不重写 demux。**

- **16 typed event struct 减重复**：每个持一个 `RouteEventCore`（Target/CurrentTarget/flags + StopPropagation/PreventDefault），IRouteEvent 6 成员转发给它——6 成员实现只写一次，16 struct 各加业务属性。
- **`On<T>` 订阅**：注册到订阅表（key = nodeId + EventType + capture/bubble），返回 `EventRegistration`（IDisposable 退订）。T→EventType 靠 typed struct 关联。once 触发后自动退订。
- **接线**：demux 来的 `LoomEvent` → 翻译成 typed struct（`Target = NodeRegistry[nodeId]`）→ 复用 `EventRouter` 路由 → 命中节点的 `On<T>` handler。
- **语义糖**：`button.Clicked +=`（= `On<ClickEvent>` 冒泡到自身）、`link.Activated`、`container.Scrolled`。

### 3.5 headless harness（破两台机瓶颈）

- **形态**：`tests/dotnet` 新建 xUnit csproj，链接 `Public/*.cs` + `Bindings/LoomGUIBindings.cs`，**直接 P/Invoke `loomgui_stage_new`** 建 native handle，不碰 Unity 渲染（MirrorPool/MaterialManager 一概不链接）。
- **UIContext internal 构造**：public-api §11.3 说 UIContext 无公共构造、由集成层创建。harness 扮演集成层——4a 给 UIContext internal 构造（接 Stage handle），harness 造 context；**Unity 集成层（4b）复用同一构造**。跨 4a/4b 接缝。
- **字体先不注册**：4a 验收全是盒模型/结构/cascade/生命周期，不涉文本测量。
- **pkg 来源**：低层 `create_node`/`append_child` 直接造树（测投影壳逻辑）+ `loom-pkg` 预打 1–2 个 fixture pkg.bin 入库（测 LoadPackage/Instantiate 链）。
- **依赖兜底**：`Public/*.cs` 若 `using UnityEngine`，用 Stubs（Tests.Core 先例）；否则免。实现时读 Public 文件定。

### 3.6 Rust FFI 缺口（纳入 4a，全是小工程）

| 新 FFI / 类型 | 依赖 | 难度 |
|---|---|---|
| `get_children` + `get_child_count` | Container.Children/ChildCount、Get/Query | 小（读 `Node.children` Vec） |
| `add_class`/`remove_class`/`has_class` | ClassList | 小（改 `Node.classes` Vec + 标 rematch 脏） |
| `set_inline_override`/`unset_inline_override` | Style 写/撤销 | 小（FFI + rematch 加 inline 应用步；propagate 复用不改，见 §3.3） |
| C# `NodeKind` enum（对齐 Rust u8） | 子类工厂 | 纯 C# |

---

## 4. 验收门（本机 headless，全绿 = 4a done）

1. **类型保真**：Instantiate 返回根的真实 C# 类型（get_node_kind 驱动）。
2. **作用域查找**：`root.Get<Container>("id")` 找到、`TryGet` 找不到返回 false。
3. **写→读 Geometry**：`Style.Width = Px(100)` → `Tick` → `Geometry.LayoutRect.w ≈ 100`（滞后一帧，验 seam→set_inline_override→rematch→solve 链路）。
4. **Unset 撤销**：`Style.Width = Unset()` → `Tick` → Geometry 回落到 CSS/base 值（验便签层）。
5. **class 改 computed**：`Classes.Add("hi")` → `Tick` → `get_node_computed_style` 读到 `.hi` 规则属性变了。
6. **树结构**：`ChildCount` + `Children` + `GetChildAt` 与 HTML 一致。
7. **生命周期**：`Dispose()` → `IsDisposed` → 后续操作抛 `ObjectDisposedException`。
8. **事件**：构造 click `LoomEvent` 喂 dispatch → `Clicked` 触发 → typed `ClickEvent` Target 正确；capture/bubble 顺序；`StopPropagation`；`once`/`Dispose` 退订。
9. **inline 继承传播**：父节点 `Style.Color = 红` → `Tick` → 子节点（未自设）`get_node_computed_style` 的 color = 红（验 inline_override 折进 set_map 后，propagate 自动把含 inline 的父值传给子——这条坐实"propagate 不改"的简化成立）。

不依赖单一 rect 断言（roadmap §3.3）：同时覆盖类型保真、作用域查找、cascade 写/撤销、class 命中、树结构、生命周期、事件 typed 路由——每条都测"语义对"，不只"渲染出来"。

---

## 5. 推后项 / defer 清单（防会话丢失）

| 项 | 推到哪 | 为什么 / 升级路径 |
|---|---|---|
| `set_transform` FFI（NodeTransform 逐帧值） | 第一个高频/逐帧 transform 控件 | 4a NodeTransform 标脏不 flush；roadmap §3.5 transform 债 |
| 攒批回写 flush | 第一个高频改值控件 | seam 已留（§3.2），升级只改 setter 调用时机 + 加帧末 Flush |
| 控件业务事件（ValueChanged/SelectionChanged/CheckedChanged） | 控件投影（4b 或控件束） | 绑控件业务属性，跟控件一起做 |
| 字体注册 + 文本测量验收 | 4b | 4a 不涉文本 |
| Geometry blob 缓存 | Geometry 读成瓶颈时（YAGNI） | 要给 blob 加 rect 列升版本；4a 直读 FFI 够用 |
| `@keyframes`/`animation` 解析 | 终点线2 验收页避开 or 视觉束提前 | roadmap §3.5 债：home 动画是 @keyframes，③ 选择器 scope 没含 |
| set_style 旧路径清理 | 4b/后续 | set_style 仍写 base_style，4a 后 C# 不走它；旧路径是否废弃 4b 定 |

---

## 6. Spec-4b 范围（下一棒，防丢失）

- **终点线2 Unity 真机验收**：`UIContext→LoadPackage→Instantiate→Get<Button>→Clicked→真机渲染`。
- 复用 MirrorPool/InputCollector/EventHandler（零改）+ 4a 的 UIContext internal 构造接入 LoomStageDriver。
- 字体注册 + 文本验收。
- 验收页选择：避开 @keyframes/逐帧 transform 的 showcase 页（roadmap §3.5），或把 set_transform/@keyframes 提前拉进来（4b 开工时定）。
- set_style 旧路径清理评估。

---

## 7. 文档漂移待修（本 spec 挖出）

- `projection-layer.md §2.2`："复用现有字符串 FFI 零改动" → 改为 inline override 层需 core 新建（set_inline_override），set_style 写 base_style 不胜任。
- `public-api.md §3.1`：标注 Style inline override 语义依赖 core inline_override 层（4a 落地）。
- `main-design.md` 若有相关措辞同步。
- **roadmap**：4a 完成后更新进度 + 记录 4b 范围 + defer 项（写完 plan 后做）。
