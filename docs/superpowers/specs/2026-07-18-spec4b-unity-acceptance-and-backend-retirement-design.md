# Spec-4b：LoomStage 退役 + 多引擎后端分层 + 终点线2 Unity 验收（后端对象层 ④ 第 2 棒）

> **创建**：2026-07-18
> **状态**：设计已与用户分节确认（§1–§4），待写实现 plan
> **上下文**：④ 后端对象层的第 2 棒（共 2 棒）。上一棒 Spec-4a（C# 投影层 + core 便签层 + typed 事件层 + headless harness）已完成。本棒做三件事：**① LoomStage 旧命令式门面干净退役 + 多引擎后端分层重构；② 全仓库旧范式残留一次性清干净；③ 终点线2 Unity 真机端到端验收**。新范式端到端能用 = 摸黑结束。
> **权威契约**：[public-api.md](../../design/public-api.md)（公共签名）+ [projection-layer.md](../../design/projection-layer.md)（投影机制）+ [main-design.md](../../design/main-design.md)（§2.2 分层 / §16 每帧管线 / §17 后端契约）。本 spec 是这三份契约的"实现第 2 棒"，不改公共签名（公共表面 4a 已冻结实现）。

---

## 1. 范围分解：为什么这棒比"瘦棒"重

Spec-4a 预判"4a 比 4b 重"。实际进入 4b 设计后，因用户连续要求 **"LoomStage 干净退役（不留双壳）+ 多引擎复用 + 尽量清干净退役残留"**，本棒从最初设想的"接通 4a 接缝 + 最小页验收（瘦棒）"长成**中棒**：残留大清理 + LoomStage 退役 + 多引擎分层 + 最小页验收。

这不是范围蔓延——每一块都是用户显式要的"干净"。但为避免清理淹没验收，spec 按 **phase 组织**（§6），让"清理/分层"和"验收"各自独立可验。

三块工作各自的理由：

| 块 | 为什么在 4b 做 |
|---|---|
| LoomStage 退役 + 多引擎分层 | 终态契约（public-api §11.3 / main-design §2.2/§17）里**没有 LoomStage**——它是 v1 旧壳，业务 API 透传已被 4a UIContext 取代，留着是概念冗余。退役时按多引擎接缝拆，一次做对比"先双壳跑通再拆"省。 |
| 残留大清理 | subagent 三路扫描证实：RichText 死链 / Controller 全链半退役 / set_style 死路径 / 旧 demo / stale 文档散落全仓库。不清则"干净"是假的。 |
| 终点线2 验收 | 摸黑骨架链第一次在 Unity 真机端到端跑通——核心范式对不对在本机（4a headless）已锁，4b 验的是"投影层 + 集成层在真引擎里没歪"。 |

---

## 2. 关键查证发现（subagent 三路扫描）

派 3 个并行 subagent 扫全仓库（LoomStage 业务 API 调用点 / 旧机制残留 / 旧产物 stale 资产）。核心结论：

### 2.1 LoomStage 业务 API：30 方法，29 个零活体 caller

`unity/package/Runtime/LoomStage.cs`（589 行）的**业务 API 透传方法**（CreateNode/SetStyle/SetText/Tween/FindNodeById 那一类，§4.1 列全），**29 个零外部 caller**——旧 demo `unity/showcase-unity/Assets/Scripts/Demo/LoomShowcaseDriver.cs.bak` 是唯一曾经广泛调用者，已 `.bak`（Unity 不编译），属死代码。4a headless 测试全走 UIContext，不碰 LoomStage 业务 API。唯一有活体 caller 的业务方法是 `LoadPackage`（`LoomStageDriver.cs:188` bootstrap）。

⚠️ **别误以为 driver 只调 LoadPackage**——`LoomStageDriver` 还调了 ~10 个**生命周期/后端编排方法**（grep 核实：`Tick`/`RegisterFont`/`SetImageSizes`/`SetFallbackFamilies`/`InitSprites`/`UseSafeArea`/`SetNativeHostRoot`/`StagePtr`/`Dispose`）。这些**不在 §4.1 业务 API 删除表里**，而是按 §3 迁移到 LoomHost/UnityLoomBackend——它们是引擎后端编排，不是被 UIContext 取代的命令式业务 API。

→ **业务 API 透传层可整层删除**（§4.1），driver 的 ~10 个编排调用随 §3 分层迁移。

### 2.2 引擎无关层 4a 已建好

- `unity/package/Runtime/Public/`：UIContext/Node/Button/Style/ClassList/Geometry（**零 `using UnityEngine`**）
- `unity/package/Runtime/Projection/`：NodeRegistry/EventDemuxer/EventBus/RouteEventCore（**零 `using UnityEngine`**）

→ 多引擎接缝**不用从零搭**。LoomStage 退役只需把"stage 驱动核心（tick/borrow 序）"也从 Unity 剥出来提到引擎无关层，和 Unity 后端之间用 main-design §17 后端契约隔开。

### 2.3 Controller 是"干净"的关键锁眼

Controller（`data-controller`/`data-page`，v1.5 停止）新表层干净（Public/EventType/fence 全无），但**全链半退役**：core state 机 + 4 FFI + 6 测试 + C# wrapper + `LoomEventHandler` 全套 ControllerChanged + packer bridge attr parse + **pkg.bin v18 schema（ControllerEntry + data_controller 序列化）**。

**`LoomEventHandler`（旧 demux）有两个运行时事件源**（`LoomStage.cs:220-229` 核实）：
1. `borrow_events` → `DispatchPending`（喂旧 `AddListener` 路径，:221）
2. `borrow_controller_changed_events` → `DispatchControllerChanged`（喂 Controller 切页，:229）

删 `LoomEventHandler` 的真正门是**两个源都没**：
- **源 1 gate = AddListener 零业务 caller**——`:224` DEPRECATION 注释明写"待所有 callers 从 AddListener 迁走后删 DispatchPending"。grep 核实：`AddListener`/`AddCapture` 仅 `LoomEventHandler.cs` 自身定义 + `LoomEventHandlerTests.cs` 测试调用，**业务/demo/driver 零 caller**（.bak 不编译不计）→ gate 已满足。
- **源 2 gate = Controller 删**——删 Controller 全链 → `borrow_controller_changed_events` FFI 消失 → DispatchControllerChanged 无源。

两源都没 → `LoomEventHandler` 整体可删（+ `LoomEventHandlerTests.cs`）。**不删 Controller = 源 2 还在 = 旧 demux 清不干净**。

→ 决策：Controller 全链删 + bump pkg v19（详见 §4）。

### 2.4 其它残留（低风险死链）

- **RichText 死链**：主体（NodeKind::RichText / set_rich_text / SetRichText / display:block desugar）4a 前已彻底删干净。但 `loomgui_stage_rich_link_at` FFI + `stage.rs rich_link_at` + `scene.rich_fragments` 字段（恒空，注释自承"RichText retired, always empty"）+ 回写循环是死路径。**`text/rich.rs` 算法本体要留**（复合束要用，layout/render 全在用）。
- **set_style 写 base_style 死路径**：FFI + `Stage::set_style` + `dynamic::set_style` + 2 测试，active C# 零 caller（4a StyleMirror.cs:17 已严禁，走 `set_inline_override`）。**`apply_css` 本体要留**（create_node 烘焙 base_style 要用）。
- ✅ **HEAD 编译断已修**（commit `b03929a` "fix: unity 编译通过"，2026-07-18 20:26）：HEAD `LoomStage.cs:545` 曾调已删的 `loomgui_stage_set_rich_text` 导致 C# 编译失败，working tree 删 `SetRichText` + 7 文件 namespace 消歧已提交。spec 写于该 commit 之后。→ **4b 无需再处理**。
- **旧 demo 目录**：`unity/showcase-unity/Assets/Scripts/Demo/`（.bak + 孤立 asmdef + Demo.meta）+ `SampleScene.unity:608` broken MonoBehaviour ref。
- **文档漂移**：`projection-layer.md:35,50` 还说 flush 走 `set_style/apply_css`（与 4a 便签层 `set_inline_override` 矛盾）；`roadmap.md:48` stale dll 描述过时。

### 2.5 stale dll/bindings：当前同步

`loomgui_ffi_c.dll` + `LoomGUIBindings.cs` 与 Rust 源码**当前同步**（截至 commit 4aa8b3c），无 stale 符号。`showcase.pkg.bin` 已删（commit ccfe800），仅剩 HeadlessTests fixture（活）。

---

## 3. 设计 §1：多引擎后端分层（核心）

**YAGNI 平衡**：留接缝，不预实现——定义引擎无关抽象（驱动核心 + 后端契约），但只写 Unity 一个实现，不预先造 Godot backend。接缝是抽象的存在，呼应 main-design §17"新后端只需实现：消费 RenderNode + 输入注入 + 资源加载"。

```
[引擎无关 · C# 共享 · Unity+Godot-C# 复用]
  Public/         UIContext/Node/Button/Style（业务 API，4a 已有）
  Projection/     NodeRegistry/EventDemuxer/EventBus（4a 已有）
  Host/     (新)  LoomHost        ← stage 宿主 + 每帧驱动序（零 UnityEngine）
                  LoomBackend      ← 抽象基类（后端契约：§17 三件事）

[Unity 特定 · 各引擎各写]
  UnityLoomBackend : LoomBackend   ← 持 MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector
  LoomStageDriver (MonoBehaviour)  ← 瘦宿主：Unity 生命周期 + 资源 IO + 创建 Host/Backend
```

### 3.1 LoomHost（引擎无关，新，放 `Runtime/Host/`）

持 stage handle（IntPtr）+ UIContext + LoomBackend。零 `using UnityEngine`。

- **构造** `LoomHost(Vector2F designSize, LoomBackend backend)`：调 `loomgui_stage_new` → 建 `UIContext(stageHandle)`（复用 4a `internal UIContext(IntPtr stage)`）→ 接 `EventDemuxer`（4a typed 路径）。
- **每帧驱动** `Step(float dt)`（严格按 main-design §16 时序）：
  1. `backend.CollectInput(stage)` → set_input（Unity: InputCollector）
  2. （UIContext flush 脏属性——最小通关用 4a 即时过桥 seam，setter 立即调 `set_inline_override`；升级攒批时此处加帧末 `Flush()` 在 tick 前）
  3. `loomgui_stage_tick` FFI
  4. `borrow_frame` FFI → `backend.SyncFrame(stage, framePtr, frameLen)`（Unity: MirrorPool 镜像；stage 透传以便 backend 需要时复用）
  5. `borrow_events` FFI → `EventDemuxer.Pump` → EventBus（typed `On<T>` 路由）
- **资源 FFI**（引擎中立，放这）：`RegisterFont(family, bytes, isDefault)` / `SetImageSizes(paths, ws, hs)` / `SetFallbackFamilies`。byte[] 引擎中立，FFI 调用在 LoomHost。
- **Dispose**：`loomgui_stage_free` + 递归清理。
- 暴露 `Context`（UIContext，业务表面）+ `Backend`。

### 3.2 LoomBackend（引擎无关抽象基类，新，放 `Runtime/Host/`）

契约 = main-design §17 三件事：

```csharp
public abstract class LoomBackend {
    // 采集引擎输入（Unity: InputCollector）+ 调 set_input FFI（FFI 引擎中立，backend 可调）。
    public abstract void CollectInput(IntPtr stage);
    // 消费 borrow_frame 的 blob 做镜像渲染——不调 borrow FFI（LoomHost 已 borrow，把 stage + ptr + len 传进来）。
    public abstract void SyncFrame(IntPtr stage, IntPtr framePtr, int frameLen);
    // 资源 byte[] 由 Driver 读、LoomHost 调 FFI；backend 只负责引擎特定的资源对象（如 Texture2D 上传）。
}
```

**接缝决策**（用户认可）：
- **borrow_frame 的 FFI 调用放 LoomHost**——产生引擎特定镜像对象（Unity GameObject）的 FFI 归引擎无关驱动核心，backend 只消费 blob 做镜像，不碰 borrow FFI。Godot-C# 复用驱动序，只重写镜像渲染。
- **set_input 的 FFI 调用可在 backend**——采集引擎输入是引擎特定（Unity InputSystem），但 set_input FFI 本身引擎中立（数据是 PointerEvent struct），backend 采集后直接调 set_input 不破坏引擎无关性，省掉"采集填 struct → 传 LoomHost → 调 FFI"的多一次交互。
- **NativeHost（GameObject 绑定 3D 模型）不进 LoomBackend 通用契约**——它是 Unity 专属概念（Godot 是 canvas_item + 3D），作为 `UnityLoomBackend` 的额外方法，由 Driver 或 UnityLoomBackend 内部持 `NativeHostManager`。

### 3.3 UnityLoomBackend : LoomBackend（Unity，新/搬家）

持 MirrorPool + MaterialManager + NativeHostManager + SpriteResolver + InputCollector（**全部零改复用**，只是从 LoomStage 搬过来）。
- `CollectInput` → InputCollector.Collect
- `SyncFrame` → borrow_frame 后 MirrorPool 镜像同步 + font atlas dirty 上传
- Unity 资源 IO 辅助（Texture2D 上传 atlas 页、SpriteResolver 注册）

### 3.4 LoomStageDriver（MonoBehaviour，瘦下来）

- Awake：创建 UnityLoomBackend（注入 Unity 组件）→ `new LoomHost(designSize, backend)` → 读 `.ttf`/atlas 喂 `host.RegisterFont`/资源 → `ctx.LoadPackage`（迁移自旧 `_stage.LoadPackage`，driver:188 唯一活体 caller）
- Update：`host.Step(Time.unscaledDeltaTime)`
- 保留 Unity 特定：相机/safeArea/输入钩子/设计分辨率配置/NativeHost 根 transform
- 暴露 `Context`（业务从 driver 拿 UIContext）

**多引擎复用兑现**：未来 Godot-C# 写 `GodotLoomBackend : LoomBackend`，复用 LoomHost + 整个 Projection + Public。Driver 各引擎各写（Unity MonoBehaviour / Godot Node）。

---

## 4. 设计 §2：LoomStage 退役 + 删除清单

### 4.1 LoomStage 业务 API（A 组）

| 项 | 处置 |
|---|---|
| 29 个业务方法（CreateRoot/CreateNode/AppendChild/InsertBefore/RemoveChild/RemoveNode/SetText/SetSrc/SetStyle/Instantiate/FindNodeById/SetNodeDisabled/SetScrollPos/GetScrollPos/SetContentSize/ClearContentSizeOverride/GetNodeLayoutRect/SetReuseKey/Tween/KillTween/ClearAnim/ClearAnimProp/GetController/SetSelectedIndex/GetSelectedIndex/DumpScene/IsPointerOnUI/InsertBefore）| 直接删（零活体 caller，语义已被 UIContext 投影层覆盖） |
| `LoadPackage`（driver:188 唯一活体 caller） | 迁 `ctx.LoadPackage` |
| `BindNativeHost`/`UnbindNativeHost` | 搬 `UnityLoomBackend`（引擎后端，非 UIContext） |

### 4.2 低风险死链（B 组，无 ABI/schema 影响）

| 项 | 处置 |
|---|---|
| `loomgui_stage_rich_link_at` FFI + `stage.rs rich_link_at` + `scene.rich_fragments` 字段 + 回写循环（恒空死链）+ 两侧 binding | 删 |
| `loomgui_stage_set_style` FFI + `Stage::set_style` + `dynamic::set_style` + 2 测试 | 删（`apply_css` 本体留） |
| `dump_controller.rs` example | 删 |
| `showcase-unity/Assets/Scripts/Demo/` 整目录（.bak + 孤立 asmdef + Demo.meta）+ `SampleScene.unity:608` broken ref | 删（清场景引用） |
| `.gitignore:22-23` 死代码（LoomUI 目录已删） | 清 |
| ~~HEAD `set_rich_text` stale caller + 7 文件 namespace 消歧~~ | ✅ 已由 commit `b03929a` 提交，4b 无需处理 |

### 4.3 Controller 全链 + pkg v19（C 组，连锁清旧 demux）

| 项 | 处置 |
|---|---|
| core：`Node.data_controller` 字段 + `Controller`/`ControllerChangedEvent` struct + `Scene.controllers`/`pending_controller_events` + `set_controller_selected`/`controller_selected` + stage `get_controller`/`set_selected_index`/`get_selected_index`/`controller_changed_events` + instantiate 填 controller + tick 清 pending | 删 |
| FFI：4 个 Controller FFI（`get_controller`/`set_selected_index`/`get_selected_index`/`borrow_controller_changed_events`）+ `crates/ffi/src/tests.rs` 6 个 Controller 测试 | 删（连测一起） |
| packer：`bridge.rs:63` data-controller attr parse | 删 |
| pkg schema：`ControllerEntry` struct + `ControllerSection` 序列化 + `TemplateNode.data_controller`（`asset/mod.rs`） | 删 + **bump `PKG_FORMAT_VERSION` 18→19** |
| C#：bindings 4 个 + `LoomStage` 3 wrapper（GetController/SetSelectedIndex/GetSelectedIndex） | 随 LoomStage 退役删 |
| **连锁**：`LoomEventHandler`（业务侧零 caller，失去 ControllerChanged 最后事件源）+ `LoomEventHandlerTests.cs` | 整文件删 |
| `tests/dotnet/Stubs/LoomEventTypes.cs` + `tests/dotnet/EventRouter.cs` 注释更新 | 改（EventRouter 保留作算法参考实现） |

### 4.4 必须留（别误删）

- `crates/core/src/text/rich.rs` 算法 + caller（layout/render/text）——复合束要用，全在用
- `apply_css` 函数本体——create_node 烘焙 base_style 要用
- `set_inline_override`/`unset_inline_override` FFI + StyleMirror.cs——4a 新路径
- `EventRouter.cs`——EventBus 算法参考实现（文档标注非生产依赖）

---

## 5. 设计 §3：验收门（终点线2 = 摸黑结束）

家里机 Unity PlayMode，全绿 = 4b done = 摸黑结束：

1. **最小页渲染**：div/button/img/text 盒模型 + flex 布局 + cascade 视觉对人眼通过。
2. **button `Clicked` 触发**——typed `On<T>` 全链通：InputCollector → set_input → process → borrow_events → EventDemuxer → EventBus → `node.On<ClickEvent>`（验 4a 事件层在 Unity 生产路径跑通，非 headless）。
3. **rect 跨层断言**：Unity 读 `node.Geometry.LayoutRect`，与 4a headless 断言的 rect 一致（复用 4a NodeGeometry 直读 FFI，验投影层 + 集成层没歪）。
4. **事件 typed 路由**：capture/bubble 顺序 + `Clicked` 语义糖 + StopPropagation。

**最小验收页**（手写，仿 shop 结构剥超范围特性）：
- 结构：`header(div + button)` + `body(div flex column)` + 几个 `product(div flex row: img + text + button)`
- **不含**：@keyframes / transition / progress / input / data-controller / hover transform（纯静态 cascade + flex）
- **文本 ASCII**：英文标签（Buy/Title/Price），注册一个 ASCII 字体。避开 CJK fallback 复杂度（v1.6 PlayMode 待查项：CJK fallback / 标题蓝色块 / 文字成坨），CJK 独立留后。
- pkg.bin：手写最小页 HTML → `loom-pkg build` → v19 入库。

不依赖单一 rect 断言（roadmap §3.3）：同时覆盖渲染、typed 事件链、rect 跨层一致——每条都测"语义对"。

---

## 6. 设计 §4：deferred 收口 + phase 组织

### 6.1 deferred 5 项处置

| # | 项 | 处置 |
|---|---|---|
| ① | `dynamic.rs:231` remove_child 非直系子误清 parent（4a 加了 C# GetChildIndex 守卫，core 根因未修） | **P2 顺手修**（加直系子校验，机械） |
| ② | `create_node`/`set_text` null css → `from_raw_parts(null,0)` UB | **P2 顺手修**（core null-check） |
| ③ | `kind_from_tag` 4-tag vs `resolve_semantic` 23-tag（dynamic span→TextNode 不匹配 Query） | **推后**（4b 最小页走 pkg.bin 实例化 resolve_semantic 23-tag 全，不触发 dynamic 创建；归控件束/复合束 dynamic 创建路径） |
| ④ | Controller 全链 | **P1 全删 + bump v19**（§4.3） |
| ⑤ | LoomEventHandler 旧 demux 并行 | **P1 连锁删**（随 ④，§4.3） |

### 6.2 phase 组织（呼应两台机约束）

| Phase | 内容 | 验收机 | 验收方式 |
|---|---|---|---|
| **P1 清理** | B 组死链 + Controller 全删(bump v19) + LoomEventHandler 连锁删 + Demo 目录 + 文档漂移 + deferred ①② | 编码机 | 本机 `cargo test` + headless 测不回归 + PublicApi 编译门 |
| **P2 多引擎分层** | LoomHost/LoomBackend/UnityLoomBackend + LoomStage 退役 + NativeHost 搬 + driver LoadPackage 迁 | 编码机 | 本机编译 + headless 测（UIContext 经 LoomHost 驱动，验分层不破语义） |
| **P3 验收** | 手写最小页 pkg.bin(v19) + Unity PlayMode 跑 §5 四条 | 家里机 | PlayMode 真机 |

P1/P2 是纯重构（不改行为），编码机本机可验；P3 才需搬家里机跑 Unity。核心风险（范式对不对）在 4a headless 已锁，4b 验的是集成层。

---

## 7. 文档漂移待修（本 spec 挖出）

- **`docs/design/projection-layer.md:35,50`**：flush 描述还说"一次 `set_style` 过桥，Rust `apply_css` parse"+"复用 v1 现有字符串 FFI 零改动"。与 4a 实际矛盾——`StyleMirror.cs:17` 已严禁 set_style，走 `set_inline_override` 便签层。改为 inline override 层需 core 新建（4a 已落地），set_style 写 base_style 不胜任。
- **`docs/roadmap/roadmap.md:48`**：stale dll 描述（"`loomgui_stage_load_html`/`set_rich_text` 已删源码、dll 里还在"）已与现实不符——bindings 已双侧删。改为"`LoomStage.cs:545` HEAD stale caller，working tree 已修"或直接删该段。
- **`crates/ffi/src/lib.rs:305`**：doc 注释 `\ ControllerChangedEvent` 应为 `///`（随 Controller 删顺带没了）。
- **`docs/design/main-design.md`**：✅ §17 跨引擎扩展段已补 LoomHost/LoomBackend/UnityLoomBackend 分层图 + LoomStage 退役说明（Spec-4b P3.5 落地）。
- **roadmap**：✅ 4b 完成后 §2 终点线2 DONE + §8 决策记录 + §4 tech-debt 段；deferred ①②已修 / ③仍推后 / card-img bg 机制 tech-debt 留。

---

## 8. 推后项 / ponytail（防会话丢失）

| 项 | 推到哪 | 为什么 / 升级路径 |
|---|---|---|
| 攒批回写 flush | 第一个高频改值控件 | 4a seam 已留（StyleMirror 稀疏镜像 + FlushInline），LoomHost.Step 时序留了 flush 位（tick 前），升级只改 setter 调用时机 + 加帧末 Flush |
| `set_transform` FFI（逐帧 transform） | 第一个高频/逐帧 transform 控件 | roadmap §3.5 transform 债；最小页不验逐帧 transform |
| `@keyframes`/`animation` 解析 | 视觉束/控件束提前 | roadmap §3.5；home 动画是 @keyframes，③ 选择器 scope 没含。4b 最小页不含 |
| CJK 字体 + fallback 验收 | 字体专项（v1.6 待查项） | 4b 最小页 ASCII；CJK fallback / 标题蓝色块 / 文字成坨待查 |
| deferred ③ `kind_from_tag` 4-tag vs 23-tag | 控件束/复合束（dynamic 创建路径） | 4b 最小页走 pkg.bin 实例化（resolve_semantic 23-tag），不触发 |
| 控件业务事件（ValueChanged/SelectionChanged） | 控件投影（控件束） | 绑控件业务属性，跟控件一起做 |
| `Node.Id` 返 numeric 占位 → 真 id_attr | get_id_attr FFI 加上后 | `Nodes.cs:46-54` ponytail 注释已标 |
| Geometry blob 缓存 | Geometry 读成瓶颈时 | 要给 blob 加 rect 列升版本；4b 直读 FFI 够用 |

---

## 9. Spec-4b 范围外（下一棒线索 / 摸黑之后）

4b done = 摸黑结束（终点线2 通）。之后按 roadmap §4 三束加宽：
- **控件束**：progress → input 全家 → select/textarea → 滑块/开关（第一个高频控件出现时还攒批/set_transform 债）。
- **复合束**：ListView 虚拟化 / 文本模型回归标准子树 / Custom Element + slot。
- **视觉/特效束**：渐变 / box-shadow / filter / 文字特效 / transform 视觉 / border-radius / 九宫格。

终态验收（roadmap §4）：showcase 8 页全在 Unity 真机跑通 + 布局与浏览器 rect 比对一致。@keyframes/transform 动画届时随视觉束/控件束落地。
