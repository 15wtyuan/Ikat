# Cascade 收尾（③）+ 后端对象层（④）合并设计

- **日期**：2026-07-17
- **范围**：摸黑一程的最后两棒——③ cascade 收尾（headless 加固）+ ④ 后端对象层（冲 Unity 终点线2）。一并设计、分两阶段实现。
- **依赖**：Spec-1（cascade spike）、Spec-2（core 类型化重构）、Spec-3（IrTree 桥 + 打包编排）均已完成；终点线1 smoke 已绿。
- **权威契约**：`docs/design/public-api.md`（公共 API 终态，三分模型 + typed 类型树已冻结）、`docs/design/projection-layer.md`（C# 投影机制）、`docs/design/main-design.md`（架构）。
- **状态**：设计草案，待 review。

---

## 1. 背景与动机

摸黑一程前三棒已完成：spike 探路（手搓选择器解析器 + 通用继承 pass）→ ① core 类型化（NodeKind 扩容 + struct 拆分 + pkg v18）→ ② IrTree 桥 + 打包编排。终点线1 smoke 绿：HTML→pkg→Stage→rect，断言了 class 命中、display:none 剪枝、flex column。

### 1.1 探路 spike 的事实输入

为本设计先做了一次探路（`crates/packer/pkg/tests/fixtures/cascade-probe.html` + `tests/cascade_probe.rs`）：一段严格落在当前 cascade 子集内的手写 HTML（class/tag/id/后代/伪类选择器 + 中文文本 + 全量代表性控件）端到端跑通，3 测试绿。

**绿证明**：含 `<style>` cascade + 中文文本 + 控件（input range/checkbox、select/option、progress、ul/li）的 HTML 经 fence→桥→pkg→Stage→layout（中文字体测量）→render 全链不炸，且 `#root { width:320 }` 经 id 选择器 cascade 生效。

**绿没能证明的，全卡在同一缺口**：继承 font-size/color 是否正确传播、`.title` vs `#root .title` 的 specificity、控件是否真保 kind 不塌成 Container——Stage public API 只有 `find_node_by_id` / `get_node_layout_rect(Rect)` / `get_node_visible`，**没有 computed-style 查询、没有 node-kind 查询**。这正是 smoke 注释「font/继承/computed style/kind 保真推 ③」的根因：不是引擎没做，是**没有观测出口**。

### 1.2 ③ 认知修正

| 原以为的 ③ | 探路后的 ③ |
|---|---|
| 把 cascade 接到全量标签（引擎不完整） | 引擎已在子集内跑通全量标签；产品化尾巴很小 |
| 继承 / specificity 要新写 | 继承 pass + rematch 已在跑，缺的是**能断言它对** |
| — | **真缺口 = computed-style / node-kind 查询出口（core public + FFI）+ 集成断言锁死 + 核实 base_style 每帧基线契约** |

③ 的重心从「重写 cascade 引擎」翻成「补可观测性 + 锁集成断言」。

### 1.3 ③ 与 ④ 的耦合点

④ 后端对象层为了 `Instantiate` 后造出正确 C# 子类，需 node-kind 查询（`docs/design/projection-layer.md`）。这跟 ③ 要的 node-kind 查询是**同一个 FFI**。查询出口一次补齐，③（断言）④（投影）共用——这是合并设计的根据。

---

## 2. 目标 / 非目标

### 目标
- **③**：补 computed-style + node-kind 查询出口（core public + FFI + C# 投影只读快照）；把 probe HTML 升级成完整 cascade 集成断言（继承/specificity/class 命中/kind 保真）；核实 base_style 每帧基线 + 双源 set-ness。本机锁死核心范式。
- **④**：实现 `Public/LoomGUI.*.cs` 已冻结的 typed 对象树（填 NotImplementedException 壳）+ 事件 typed glue + headless C# harness；冲终点线2 真机端到端。
- **验收终点**：冲 Unity 终点线2（两阶段：③ headless 先、④ Unity 后），验收页用 probe 设置面板（无 @keyframes/transform，绕开 §3.5 两个雷）。

### 非目标（YAGNI，明确推迟）
- 攒批回写 / set_transform 数值 FFI 高频通道——摸黑期即时过桥兜底，标 `ponytail:` 欠债，留第一个高频改值控件（roadmap §3.5 + §4 控件束）。
- @keyframes / animation / nth-child / 属性选择器 / 逗号多选——选择器子集保持现状，留视觉束 / 控件束。
- 渐变 / 多层阴影 / filter / 文字特效的 computed 读取——probe 面板不用，computed 快照子集不含。
- showcase 8 页真机全跑——留摸黑之后。
- 按属性字符串查询（候选 B）——无脚本桥需求，标 ponytail 推后。

---

## 3. 调研定论：computed-style 出口选型

三方调研（RmlUi 本地源码 / 浏览器 CSSOM + Unity UITK web / FairyGUI 本地源码）交叉验证，结论高度收敛。

| 来源 | 对外主读路径 | internal struct | 立场 |
|---|---|---|---|
| RmlUi（retained+cascade，最接近） | `GetComputedValues() → const ComputedValues&`（typed struct，热路径） | dirty 位集/定义缓存全锁 `private:` | C 主 + B 辅 |
| Unity UITK | `resolvedStyle : IResolvedStyle`（typed 只读接口子集） | `internal partial struct ComputedStyle` + `[VisibleToOtherModules]` 刻意不 public | C 首选，A 反对，B 不做 |
| FairyGUI（命令式，无 cascade） | typed 字段直读（`g.width`/`asButton`） | 无 style bag（`TextFormat` 是 C 样本） | C 推荐，A/B 反证 |
| 浏览器 CSSOM | `getComputedStyle(el).getPropertyValue("font-size") → "16px"`（live 属性集合按名查） | — | B（特例） |

**浏览器是 B、其他全是 C 的关键区分**：浏览器 B 是三件事叠加才合理——JS 动态语言（字符串 key 契合）、CSS 属性**开放集**（`--custom` 无限扩展，闭合 struct 容不下）、1990s 历史包袱。其他选 C 是静态语言 + 闭合字段 + 性能（零分配 / cache 友好 / FFI `#[repr(C)]` 直对应）+ 类型安全 + 不泄漏 internal。

**LoomGUI 落点**：Rust 静态 + 围栏闭合 CSS 子集（无 `--custom`）+ FFI 值类型 struct + 要类型安全——没有一条落在 B 场景，全部落在 C。

### 定论
- **主出口 = 候选 C**：`#[repr(C)]` 快照子集 struct，typed 字段，只含对外 cascade 解析值，**不含** internal set-ness 位图 / cascade 中间态，**不含** layout 几何（几何继续用现有 `get_node_layout_rect`，layout 后稳定值，对应 UITK resolvedStyle 几何部分，不重复造）。
- **node-kind 独立走 enum FFI**（`NodeKind` 已 `#[repr(u8)]`，Spec-2 产物）。kind 不是 style，不进快照。
- **B 推后**：摸黑期无脚本桥需求，标 ponytail，留动态查询场景再薄封装在 C 之上。
- **A 坚决不做**：Unity `[VisibleToOtherModules]` + `internal` 是反面铁证——全量导出会把 set-ness 位图（`ResolvedStyle.inherited_set`）/ cascade 中间态焊死进 ABI。
- **两个借鉴**：① 声明快照陈旧窗口（rematch 后有效，RmlUi `@lifetime` 做法）；② FFI 边界 flatten 成 owned 可拷贝 struct（RmlUi 的 ComputedValues 不可拷贝是 C++ 帧内借用，C# GC 边界要 owned）。

调研来源见 §11。

---

## 4. 核心设计决策

### 4.1 与 public-api.md 三分模型对齐（关键）

`LoomGUI.Nodes.cs` 已冻结三分模型，回答了 computed 出口的 C# 落点：

- **`NodeStyle`**：inline override 层（最高优先级），getter 只反映 C# setter 写过的，未写过返回 Unset。**不是 cascade 读取窗口**。
- **`NodeTransform`**：渲染层，不触发 solve，走独立数值 FFI。
- **`NodeGeometry`**：只读 struct 快照，从每帧 blob 填充，滞后一帧。注释明示「要 computed 走 Geometry」。

即 computed 值的 C# 出口 = **只读快照层**（与 Geometry 同类），不是 Style（那是写向 inline override）。

**决策：并列 `NodeComputedStyle` 只读 struct**（不扩 `NodeGeometry`）：
- `NodeGeometry` 语义保持纯几何（rect/matrix/坐标变换），塞 color/font-size 语义不纯。
- 并列 struct 字段集独立演进，不污染几何快照。
- public-api.md「computed 走 Geometry」读作「computed 走只读快照层」（与可写 Style 对立），并列 struct 符合该原则。

这是冻结签名（`Public/LoomGUI.*.cs`）第一次扩展，**需过 `tests/dotnet/LoomGUI.PublicApi` 防漂移门，并在 public-api.md 落一行**（新增 `node.ComputedStyle` 只读 struct）。

### 4.2 typed 类型树形态已定

`LoomGUI.Nodes.cs` 已是 fgui 式「每类型独立 class」（`Button : Container`、`Slider : Node`、`TextField : Node`…）。④ 不重新设计类型树，只**实现壳**。node-kind FFI → 工厂 switch（`NodeKind::Button → new Button()`）造正确子类。

### 4.3 分层：快照只放 cascade 解析值

computed-style 快照只含 cascade 解析后的**非几何**样式值（font-size 经继承折算成绝对 px、color、display、flex-direction、background、overflow…）。依赖 layout 才定的几何（auto 宽高）用现有 rect 出口。对应 UITK 把 computedStyle（cascade 后）藏起来、只暴露 resolvedStyle（layout 后）几何的做法——LoomGUI 已有 rect 出口担此任。

---

## 5. 阶段一 · ③ cascade 收尾（headless）

### 5.1 查询出口（core public + FFI）

新增 Stage public 方法（照现有 `get_node_layout_rect` / `get_node_visible` 模式）：

```rust
impl Stage {
    /// 节点语义类型（围栏 tag + 结构属性决定，CSS 不改变）。None = 节点不存在。
    pub fn get_node_kind(&self, node: NodeId) -> Option<NodeKind>;

    /// cascade 解析后的非几何样式快照（owned，可拷贝）。None = 节点不存在。
    /// 陈旧窗口：rematch 后有效；本帧 tick_and_render 后反映最新 cascade。
    pub fn get_node_computed_style(&self, node: NodeId) -> Option<ComputedNodeStyle>;
}
```

FFI（`crates/ffi/src/lib.rs`，照 `loomgui_stage_get_node_layout_rect` 模式）：

```rust
// return-code + out-param（与 get_node_computed_style 一致）：返回 0 = ok 且 *out = kind，
// 非 0 = 节点不存在。不用 `-> u8` + 0 哨兵——NodeKind 首变体 Container 判别值 = 0，
// 0 哨兵会和每个 div（Container）撞，无法区分「不存在」与「Container」。
#[no_mangle] pub extern "C" fn loomgui_stage_get_node_kind(
    h: *const StageHandle, node_id: u32, out: *mut u8,
) -> i32;
#[no_mangle] pub extern "C" fn loomgui_stage_get_node_computed_style(
    h: *const StageHandle, node_id: u32, out: *mut ComputedNodeStyleRepr,
) -> i32;  // 0 = ok, 非 0 = 节点不存在
```

`ComputedNodeStyleRepr` 是 `#[repr(C)]` owned struct，FFI 边界 flatten（owned 可拷贝，C# GC 友好）。csbindgen 不为 struct 自动生成 C# stub，须手补 C# 镜像（`crates/ffi/build.rs` 之后 `cargo run -p xtask -- sync-bindings`）。

### 5.2 ComputedNodeStyle 字段子集（curated）

从 `ResolvedStyle`（`crates/core/src/style/resolved.rs`）投影一个对外子集。**排除**：`inherited_set`（internal set-ness 位图）、`taffy_style` 几何（size/min/max/margin/padding——几何走 rect）、复杂视觉（gradient/filter/box_shadow/transform/text_effects/transition——留视觉束）。

分类（代表性字段，完整集实现期对照 ResolvedStyle curated）：
- **布局指令**：`display_mode`、`flex_direction`、`overflow_x`、`overflow_y`
- **paint**：`color`、`background_color`、`opacity`、`border_color`
- **text**：`font_size`、`font_weight`、`text_align`、`line_height`、`letter_spacing`

这些覆盖 ③ 要断言的（继承 font-size/color、display、specificity）+ ④ 投影 typed 读所需。复杂字段随视觉束 / 控件束扩。

### 5.3 probe HTML 升级成完整 cascade 断言

把 `cascade_probe.rs` 从 3 个 rect/visible 断言升级，用新出口锁全部推迟的语义：
- **继承传播**：`#root` 设 `font-size:14 / color:#222`，后代文本节点读 computed 应继承（`.lbl` 被 `.row .lbl` 覆盖成 12，验证 specificity + 继承同时）。
- **specificity**：`.title { color:#114488 }` 被 `#root .title { color:#0066aa }`（id+class）覆盖——读 computed color 验证。
- **class 命中**：`.muted { color:#888 }` 命中 `#vol-val`。
- **display:none 剪枝**：`.hidden`（已有）。
- **kind 保真（§3.3 防「假绿」核心）**：`get_node_kind(#vol) == Slider`、`#mute == Toggle`、`#quality == Dropdown`、`#pb == ProgressBar`、`#save == Button`——控件**不塌成 Container**。这是 smoke「kind 保真推 ③」的直接兑现。

### 5.4 base_style / set-ness 核实

- **已核实**（`dynamic.rs:359-360`）：`rematch_pseudo_classes` 每节点每帧 `base_style.clone()` 重起 `new_style`，再 apply 命中动态规则——base_style 是每帧 cascade 基线，不是首帧缓存。set-ness 双源也已落地（`dynamic.rs:376` 从 `base_style.inherited_set` 起步 + 动态 cascade OR，坑 161 修法）。③ 无需重构 rematch，只需在全量标签下用集成断言验证 consumed 正确（继承不被父运行时值覆盖）。

### 5.5 选择器子集

保持现状（class/tag/id/后代/伪类）。@keyframes/animation、nth-child、属性选择器、逗号多选不进 scope（§3.5 + 非目标）。打包期对越界选择器 fail-fast（已有，`css_rules::parse_selector` 返 None → diagnostic）。

### 5.6 ③ 验收（终点线1 加固）

- `cascade_probe.rs` 全断言绿（继承/specificity/class/display:none/kind 保真）。
- 新增 core 单测：`get_node_kind` / `get_node_computed_style` 对 spike 已验语义回归。
- 全 workspace `cargo test` 绿 + fmt/clippy/feature-gate 清。
- 本机锁死：核心范式（cascade 正确性）在 headless 完全可断言，不再有「rect 对 ≠ 语义对」盲区。

---

## 6. 阶段二 · ④ 后端对象层（Unity，终点线2）

### 6.1 实现 typed 对象树（填壳）

`Public/LoomGUI.*.cs` 的 NotImplementedException 壳 → 转发到成熟旧 `LoomStage` 命令式 API（`docs/design/projection-layer.md`：真身 Rust，C# 是 OOP 投影 + 攒批回写）。形态已冻结，④ 是翻译层非从零造功能：
- **NodeId→Node 强引用缓存**（对象身份稳定，projection-layer §2.4）。
- **工厂**：`UIPackage.Instantiate` 返回 NodeId → 查 `get_node_kind` → switch 造正确子类（`Button`/`Slider`/...）。
- **computed 读 + 控件字段直读转发**：`node.ComputedStyle.*`（只读快照）和控件字段（`Slider.Value` 等）一次 FFI 读 core（学 fgui 直接字段 getter 形态），不缓存、不做派生（cascade 在 core 算）。注意 `NodeStyle` **不是**直读——它是 projection-layer §2.3 的稀疏写镜像（getter 返 C# setter 写过的、否则 Unset），两个不同层别混。
- **`underConstruct` 批量构造期标志**（学 fgui）：从 Rust 同步初始树时批量赋值不触发 N 次增量回写。
- **避免**：fgui 式 setter 本地副作用链（权威在 core，C# setter 只转发）。

### 6.2 computed-style 只读快照（③ 出口的 C# 投影）

`node.ComputedStyle`（§4.1 已定：并列只读 struct）= 从 ③ 的 `get_node_computed_style` FFI 填充。typed 字段直读（`node.ComputedStyle.FontSize` / `.Color`），不解析字符串。这是 §3 调研定论 C 候选的 C# 兑现。

### 6.3 事件 typed glue（关键路径）

旧 `EventHandler` 是 `EventType(byte)+nodeId` 面，无 typed 分发。壳已定 `node.On<T>(Action<T>)` + `button.Clicked` 等 event。④ 新建 glue，dispatch 机制草图（plan 期对照现有 `EventHandler`/`borrow_events` 核实）：
- 每个 Node 持自己的 typed handler 列表（`On<T>` 注册时挂上；`button.Clicked` 等 `event` 是其语法糖）。
- glue 收 `borrow_events` 的 `(nodeId, EventType, payload)` SOA 流 → 查 NodeId→Node 缓存 → 按 EventType 查该 Node 的 typed handler 列表 → 按类型分发。

渲染/输入/底层事件路由管线（MirrorPool/InputCollector/borrow_events）零改复用。`Clicked` 触发是终点线2 硬要求，此块在关键路径。

### 6.4 headless C# harness（破两台机串行瓶颈）

④ 的多数方法可在**编码机**用 console test 直接驱动真 dll 的 `LoomStage` 验证（roadmap §2④），不必每次 commit-dll-push 去家里机跑 PlayMode。只把真正依赖渲染/输入的检查留给 Unity 机。

### 6.5 终点线2（Unity，家里机验收）

`UIContext → LoadPackage → Instantiate → Get<Button> → Clicked → 真机渲染`。验收页 = **probe 设置面板**（`cascade-probe.html` 的 Unity 投影）：含 Button（验 Clicked）、控件（验 kind 保真后 `Get<Slider>`/`Get<Toggle>` 拿到正确类型）、无 @keyframes/transform（绕开 §3.5 雷）。

### 6.6 不做（ponytail 欠债）

- 攒批回写 flush + `set_transform` 数值 FFI（projection-layer §2）——即时过桥兜底，标 `ponytail:`，留第一个高频控件。
- ⚠️ §3.5 两个可能在终点线2 爆的雷：① Transform 债（公共 `Transform` API 隐含逐帧动画值，走 cascade 表达不了）；② @keyframes 没进解析器 scope。**用 probe 面板（无动画/transform）当验收页即绕开**——不要让「transform 债留以后」和「含动画的 demo 页」硬撞。

---

## 7. 验收标准（总）

- **终点线1 加固（③，本机）**：`cascade_probe.rs` 全断言绿，含继承/specificity/class/display:none/kind 保真；core 新查询出口单测绿；全 workspace cargo test + fmt/clippy/feature-gate 清。
- **终点线2（④，Unity）**：probe 面板 `Get<Button>→Clicked` 真机端到端；headless C# harness 覆盖多数方法。
- **公共契约**：computed-style 出口的 C# 落点（`NodeComputedStyle` 并列 struct 或扩 `Geometry`）过 `tests/dotnet/LoomGUI.PublicApi` 防漂移门 + public-api.md 落行。
- **.dll 闭环**：任何 Rust 改动重编 + commit `.dll`（两台机串行约束）。

---

## 8. 风险与缓解

- **公共签名扩展 vs public-api.md grill**：computed-style 出口要加 C# 字段，触发冻结签名变更。缓解：§4.1 选并列 struct（最小侵入）+ review 时与 public-api.md 对齐，必要时补一行 grill 记录。
- **kind 保真「假绿」**（§3.3）：控件塌成 Container 时 rect 对、语义丢。缓解：③ 验收强制 `get_node_kind` 断言（§5.3），不靠 rect 间接判断。
- **base_style 契约不符**（§3.2）：rematch 每帧从 base_style 重启，若实现没填 base_style 会丢基线。缓解：③ §5.4 核实，不符则填或重构 rematch 契约。
- **§3.5 transform/动画雷在终点线2 爆**：缓解用 probe 面板（无动画）当验收页；若验收需求扩到含动画页，则把 @keyframes/set_transform 提前拉进 scope。
- **两台机串行**：④ headless C# harness 把多数验证留在编码机，仅渲染/输入检查搬家里机。

---

## 9. Out of scope / ponytail 欠债

- 攒批回写 / set_transform 高频通道（留第一个高频控件）。
- 按属性字符串查询（候选 B，留脚本桥）。
- @keyframes/animation/nth-child/属性选择器/逗号多选（留视觉束/控件束）。
- 渐变/filter/阴影/文字特效的 computed 读取（留视觉束）。
- showcase 8 页真机全跑（摸黑之后）。
- spike 标的 anim-text-color 跨节点 override ponytail 欠债（随通用继承 pass 丢弃，推后）。

---

## 10. 关键文件

- **③ 出口（core）**：`crates/core/src/stage.rs`（加 `get_node_kind` / `get_node_computed_style`）、`crates/core/src/style/resolved.rs`（`ResolvedStyle` 源，投影 `ComputedNodeStyle`）。
- **③ 出口（FFI）**：`crates/ffi/src/lib.rs`（加两个 `loomgui_stage_get_node_*`，照 `get_node_layout_rect` 模式）、`crates/ffi/build.rs` + `cargo run -p xtask -- sync-bindings`。
- **③ 断言**：`crates/packer/pkg/tests/cascade_probe.rs` + `fixtures/cascade-probe.html`（升级断言）。
- **④ 投影层**：`unity/package/Runtime/Public/LoomGUI.*.cs`（填壳）、`unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`（csbindgen 产物）。
- **公共契约**：`docs/design/public-api.md`（三分模型 + typed 树，§4.1 对齐点）、`docs/design/projection-layer.md`。
- **.dll**：`unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Rust 改动后重编 + commit）。

---

## 11. 调研来源

- **RmlUi**（本地 `temp/RmlUi/`）：`Include/RmlUi/Core/Element.h`（`GetComputedValues` :590、`GetProperty` :193、RTTI :49）、`Include/RmlUi/Core/ComputedValues.h`（Common/Inherited/Rare 三子 struct、惰性 getter）、`Source/Core/ElementStyle.h`（internal 锁 `private:` :165+）、`Source/Core/ElementStyle.cpp`（`ComputeValues` 增量补丁 :873）。结论：C 主 + B 辅，internal 不导出。
- **Unity UITK**（web + 参考源码）：`VisualElement.resolvedStyle : IResolvedStyle`（typed 只读接口子集）；参考源 `Modules/UIElements/Core/Style/ComputedStyle.cs`（`internal partial struct` + `[VisibleToOtherModules]`）、`ResolvedStyleAccess.cs`（逐字段转发）。结论：C 首选，A 反面铁证。
- **FairyGUI**（本地 `temp/FairyGUI-unity/`）：`GObject.cs`（属性直接字段、17 个 `asXxx` downcast :1695-1826、`underConstruct` :139）、`UIObjectFactory.cs`（工厂 switch）、`Core/Text/TextFormat.cs`（C 候选活样本）。结论：C 推荐，A/B 反证；④ 投影层借鉴 asXxx/懒事件/工厂/underConstruct。
- **浏览器 CSSOM**（web）：MDN `getComputedStyle`（live 属性集合、resolved value、字符串）、CSS Typed OM（`computedStyleMap` 类型化演进信号）。结论：B 是动态语言 + 开放属性集 + 历史包袱的特例，LoomGUI 不适用。
