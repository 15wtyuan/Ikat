# LoomGUI 路线图

> v1 架构验证完成（v1a-v1e + showcase + v1.1-v1.8），桌面 Mono 可演示。
> 当前进入 **API 范式重构（R 系列）**：从"stage 全局 + NodeId 句柄"重做为"类型化对象树 + 标准 HTML 围栏"。
> 设计契约见 `docs/design/main-design.md`；重构 spec 见 `docs/superpowers/specs/2026-07-13-api-refactor-design.md`；围栏权威见 `docs/design/fence.md`。

---

## 0. 当前状态（TL;DR）

- **v1 = 架构走通 + 桌面可演示**（demo-grade，非上线）。底层算法（渲染批合、文本测量、滚动物理、字体自绘）已验证。
- **R 系列 = API 范式重构**：公共 API 从"stage 全局 + NodeId + SetX"重做为"类型化 Node 对象树 + 标准 HTML 围栏"。底层算法保留复用，接口形状全部重写。
- **R 系列完成后**，功能线（控件/特效/平台）在新基座上继续推进到 v2.0 毕业。
- **差异化已立**（别丢）：AI 可预测性（标准 HTML/CSS，AI 能编辑+预测渲染）+ flexbox（超 fgui Relations）+ Rust 跨引擎共享核心 + 围栏验证器。

---

## 1. v1 已交付（旧范式，底层算法资产）

> 以下能力的**算法实现**（渲染批合、文本测量/光栅、滚动物理、字体图集、FFI SOA 等）是可复用资产。它们的**公共接口形状**会在 R 系列中被重写。

### 1.1 能力清单

**渲染**：贴图 quad + 纯文本 + 硬矩形裁剪（rect mask）；FairyBatching 重排 + 显式 mesh 合并（真 N→1 draw call）；Unity 后端 GameObject 镜像 + DrawState 缓存 + 提交。

**文本**：核心自绘字体（ttf-parser outline + ab_glyph 光栅 + etagere 图集）；kerning 已开；text 可合批；跨引擎一致。

**事件**：命中（按等效绘制顺序逆序）+ click/hover/leave + 拖拽；多触摸（5 槽）+ CaptureTouch + 拖拽/滚动仲裁 + 键盘/焦点/Tab。

**布局**：taffy flexbox；参考分辨率缩放；safe-area。

**滚动**：ScrollPane：惯性 + 回弹 + 滚动条 + 鼠标滚轮（自维护可变 target tween）。

**资源**：打包器（HTML+CSS+资源→.pkg.bin+自绘图集）；独立工作区 + Tauri GUI 打包器。

**FFI**：csbindgen + SOA+多 arena 渲染树同步。

**状态/样式**：`:hover/:active/:disabled/:focus`；cascade 继承 + 合并 + 出现顺序。

**动态树**：代际 NodeId + slotmap + 命令式 API。

### 1.2 v1.x 版本历史

| 版本 | 内容 | 状态 |
|---|---|---|
| v1.1 | background-image | ✅ |
| v1.2 | border-radius（圆角 mesh） | ✅ |
| v1.3 | ColorFilter + 九宫格 slice + profiling + 动态树 | ✅ |
| v1.4 | 虚拟化列表（driver 层）+ position:absolute | ✅ 旧范式，R6 吸收 |
| v1.5 | Controller（data-controller/data-page） | 🛑 停止，R5 ARIA 替代 |
| v1.6 | 核心自绘字体 | ✅ 算法保留 |
| v1.7 | 富文本（display:block desugar） | 🛑 停止，R7 标准子树替代 |
| v1.8 | 文字效果 + 装饰视觉 | ✅ 算法保留 |

### 1.3 旧范式退役清单（R 系列清除）

以下旧设计在 R 系列中全部退役：

- `<div>` 永远是 flex column → 标准 block/flex
- `display:block` RichText desugar 暗号 → 正常 HTML 子树
- 四标签围栏（div/span/img/button）→ 标准 HTML 子集
- `NodeKind` enum + `uint NodeId` 句柄 → 类型化 Node/Container/leaf 层级
- `data-controller/data-page` 私有状态协议 → 标准 WAI-ARIA Pattern
- `FindNodeById` 全局首匹配 → 组件作用域 ID 查找
- driver 手写虚拟列表 → ListView 内置虚拟化
- 内部 NodeKind 控件不暴露为标签 → 标准 HTML 元素

---

## 2. R 系列：API 范式重构

> 设计 spec：`docs/superpowers/specs/2026-07-13-api-refactor-design.md`
> 设计是自上而下，实现按合理路径，分阶段 merge 到 main。

### 2.1 阶段总览

> **顺序原则：靶子先行，护城河先验。** 项目的差异化只在设计期 HTML/CSS 的「AI 可预测性」上——即围栏内 HTML 渲染得像不像真浏览器。R2–R8 是让框架「成熟」（类型化 API、控件、ListView，FairyGUI/UITK 都有），不加宽护城河。所以在投入几个月运行时重写**之前**，先把目标冻结成可验收的靶子，并用一道差分门给核心赌注定性。

```text
R1（围栏 schema + 新解析器）✅ 核心完成
    │
    ├─ R1.1：架构清理 + 围栏终态化（showcase 驱动补全，见 §2.2.1）
    ▼
【靶子冻结】只依赖 R1，现在就能做，与运行时解耦
    ├─ 终态 showcase HTML/CSS ＝ 渲染目标（R8 的设计期半扇）
    └─ 终态 C# 公共 API 签名文件 ＝ 用户使用目标
    │      能编译即可，不搭 mock 运行时；作为 R2–R7 的验收靶子
    ▼
【护城河差分门】最小 IR→渲染接线 + Chromium 差分
    │      同一段围栏 HTML：浏览器截图 vs LoomGUI 渲染截图，比矩形/换行/位置
    │      先回答「AI 写的围栏 HTML，LoomGUI 渲染得像不像浏览器」，再投入大重写
    ▼
R2（Scene/Node 类型化对象树）
    │
    ▼
R3（Package 格式 + FFI 跟随核心）
    │
    ├───────────┬───────────┐
    ▼           ▼           ▼
R5（标准控件） R6（ListView） R7（文本模型）
    │           │           │
    └───────────┴───────────┘
                │
                ▼
              R4（Unity C# 公共 API）
                │
                ▼
              R8 合闸（showcase 运行时半扇跑通）
```

**关于差分门的成本诚实注脚**：R1 产物 `ParsedTemplate{ tree, styles, ... }`（`crates/fence/src/pipeline.rs`）只走到「验证 + 带语义的 IR 树」，**没有 IR→scene→layout→render 那段桥**（旧 core 仍用旧 `dom.rs`）。所以强版差分（LoomGUI 渲染 vs 浏览器，而非「HTML 在浏览器好不好看」）**不是零成本**——它需要先接一段最小的「新围栏 IR → 渲染」线（本质是 R2/R3 的一小片）。关键性质仍成立：差分门在 R2–R7 全面铺开**之前**，先冻结靶子、接一小段线定性护城河，成立了再投几个月重写。

**R8 拆成两半，依赖完全不同**：
- **设计期半扇（R8-A）**：把 showcase 改写成终态 HTML/CSS。只依赖 R1，属于「靶子冻结」阶段，现在就做。
- **运行时半扇（R8-B）**：showcase 用终态 C# API 跑起来、渲染正确。依赖 R2–R7 全部就位，是最后的合闸验收。二者不是一件事，B 不可能先于运行时完成（B 的定义就是「运行时把它跑起来」）。

R5/R6/R7 在 R3 完成后可以并行推进（多会话同时开工）。R4 依赖 R2+R3 的核心契约，但可以在 R5/R6/R7 之后做（开发期间无人使用，不需要先让上层可用）。

### 2.2 R1：围栏 schema 与新解析器

**目标**：建立 machine-readable schema 作为标签/属性/CSS 值/运行时类型映射的单一真相源，重写 HTML parser 支持新围栏（全部标准元素），打包器做新围栏验证。

**状态**：✅ 核心已完成（6 阶段流水线 + schema + 855 测试全绿）。以下遗留项进入 R1.1。

**内容**：
- Schema 驱动的围栏注册表（标签 → 类型、结构属性、CSS 属性白名单、支持值）。
- 新 HTML parser（支持 div/section/header/footer/main/nav/article/aside、span/p/h1-h6/strong/em/small/br、label、button/a、img/canvas、input/textarea/select/option、progress/meter、ul/ol/li、template、details/summary/dialog、form/fieldset/legend、slot、html/head/body/title/meta/style/link）。
- 打包期验证（围栏外报错含文件/行列/原值/建议；ID 唯一性；ARIA 关系；template 根验证；Custom Element 注册验证）。
- 退役旧 `FENCE_TAGS = [div,span,img,button]` 和 `display:block` desugar。

**依赖**：无。
**验证**：fence contract tests 正例 + 反例全绿。

### 2.2.1 R1.1：R1 遗留项与架构清理

> R1 核心实现完成（commit `730d4dc`），但有几个架构问题和清理项需要先解决，再进入 R2。

**待讨论的架构问题：**

1. **围栏代码的归属 crate**：当前围栏验证流水线（tree_builder / fence_gate / css_resolve / structural / pipeline）全部放在 `crates/core` 里，靠 `#[cfg(feature = "parse")]` 门控。但 roadmap 原文说"打包器做新围栏验证"——围栏是打包期工具，运行时引擎只读 `.pkg.bin`，不需要验证代码。需要决定：
   - 围栏验证流水线搬到 `crates/packer`？
   - schema 定义（TagSpec / SemanticKind 等）留在 core（R2/R3 需要它）还是拆独立 `crates/schema`？
   - tree_builder 该跟着谁——它产出 IrTree，R2 要消费它构建运行时对象树。
   - `parse/dom.rs` 的旧 `FENCE_TAGS` 和 `apply_decl` 的 `feature = "parse"` 门控是否应该调整。

2. **围栏的闭环路径**：打包器初始化工程目录 → AI 在里面写 HTML/CSS → 打包器用围栏验证 → 验证通过后编译成 `.pkg.bin` → 运行时引擎消费。这条路径需要和打包器的实际流程对齐，围栏验证应该嵌入打包器的工作流，不是 core 的职责。

**待清理的旧代码：**

3. **退役旧 `FENCE_TAGS`**：`crates/core/src/parse/dom.rs` 里的 `FENCE_TAGS = [div,span,img,button]` 和 `display:block` desugar 逻辑仍在。roadmap 明确要求退役，R1 没做。

4. **`fence_contract.rs` 旧测试**：`crates/core/tests/fence_contract.rs` 测的是旧四标签围栏，需要用新 `r1_schema_contract.rs` + `r1_pipeline.rs` 替代或合并。

**待更新的文档：**

5. **`docs/design/fence.md`**：当前内容是旧四标签围栏（标了"R1 完成后重写"）。需要用新 schema（30 标签、三正交维度 CSS、六阶段流水线）完全重写。

6. **`crates/packer/gui/src-tauri/templates/`**：打包器模板（workspace-CLAUDE.md / SKILL.md）还引用旧围栏规则，需要更新为新围栏。

7. **`docs/design/main-design.md`**：确认围栏章节是否已经用新设计（之前讨论说"直接用新设计，不保留旧设计"），检查一致性。

**R1 未完成的验证项（Stage 5 缺口）：**

8. **ARIA 关系验证**：`aria-controls` / `aria-labelledby` 的 IdRef 目标存在性 + role 匹配（spec §3.4）。
9. **template 根验证**：ListView 内的 template 根必须是 li（spec §3.4）。
10. **Custom Element 注册验证**：自定义元素名称必须含 `-`（spec §3.4，当前只做 hyphen 检测，无注册机制）。
11. **label for 验证**：`label[for]` 指向的 ID 必须存在于当前组件作用域（spec §3.4，Stage 5 未实现）。

**围栏终态化（review 结论，2026-07-14）**：当前 `TAGS`（23 个）是能跑通验证的**骨架子集**，不是 spec §8 的终态。对比缺口——

- **纯别名标签**（便宜，语义已有）：`main/section/footer/article/aside`（→ `Container`）、`h1-h6/small`（→ `TextBlock`/`TextElement`）。
- **缺控件标签**：`meter`、`details/summary/dialog`、`form/fieldset/legend`（后两组需新 `SemanticKind`）。
- **结构性缺口（非加条目可解）**：`resolve_semantic(tag, input_type)` 只看 `input[type]`，**完全不看 `role`**，`SemanticKind` 里也无 `TabList` 等复合控件。但 spec §8.1/§12 明确 `<div role="tablist"> → TabList`。补它要改 `resolve_semantic` 签名 + annotate 逻辑，单独立项，别当成加标签。

**补全策略：showcase 驱动，不凭空补到理论终态。** 终态 showcase（靶子冻结阶段）会精确暴露实际需要哪些标签/role；缺什么补什么。唯一现在就该记账的是上面那个 role dispatch 结构缺口，因为它是签名级改动而非注册表增项。

**依赖**：无（可立即开始讨论）。
**验证**：上述每一项完成或明确 defer 后，R1.1 关闭，进入 R2。

### 2.3 R2：Scene/Node 类型化对象树

**目标**：在 core 里从 `NodeKind` enum + slotmap + `uint NodeId` 变为真正的类型化节点树。

**内容**：
- 类型化节点层级（Node / Container / TextElement / TextNode / Image / Button / ...）。
- 生命周期（`RemoveFromParent` 可重挂 / `Dispose` 递归销毁 / `ObjectDisposedException`）。
- 组件作用域 ID 查找（递归查找，不穿透嵌套组件边界）。
- 事件路由（捕获/目标/冒泡，Target/CurrentTarget 都是 Node，Dispose 自动清订阅）。
- `UIContext` 顶层实例（Package 池、根节点、焦点、输入、时钟）。
- CSS Behavior Strategy（display:block/flex/none 切换布局策略，不改变类型；overflow 切换滚动策略）。

**依赖**：R1（围栏 schema 定义类型映射）。
**验证**：core 单测覆盖对象树、生命周期、事件路由、ID 作用域。

### 2.4 R3：Package 格式与 FFI

**目标**：破坏性升级 package 格式和 FFI 以承载新语义树。

**内容**：
- Package 格式承载新 HTML 语义（标准元素、结构属性、template、slot、ARIA 关系）。
- FFI 从暴露 `NodeId + SetX` 变为暴露语义 frame model。
- 旧 `LoomStage/EventHandler/FindNodeById` 全部退役。

**依赖**：R2（类型化对象树定义 frame model 形状）。
**验证**：打包器 + core 集成测试。

### 2.5 R4：Unity C# 公共 API

**目标**：在新 FFI 之上实现公共 API 层。

**内容**：
- `UIContext/Node/Container/控件` 公共层。
- Identity Map（同一个内部节点始终对应同一个公共 Node 对象）。
- 事件订阅（语义事件 + 路由事件）。
- typed Style + StyleClass。
- 旧 C# 用户面退役。

**依赖**：R2 + R3。
**验证**：C# 单测 + 基本 Unity PlayMode。

### 2.6 R5：标准控件实现

**目标**：按新围栏逐个实现标准 HTML 控件。

**内容**：
- `input[type=range]` → Slider
- `input[type=checkbox]` → Toggle
- `input[type=radio]` → RadioButton
- `input[type=text/password]` → TextField
- `input[type=number]` → NumberField
- `textarea` → TextArea
- `select/option` → Dropdown
- `progress` → ProgressBar
- `meter` → Meter
- `details/summary` → Disclosure
- `dialog` → Dialog
- WAI-ARIA Pattern（TabList、Tree 等复合控件）
- 吸收旧 v1.9（TextInput/IME）、v1.12（滑块/进度条）、v1.13（DragDrop/Window/Popup）的大部分功能。

**依赖**：R3。**可与 R6/R7 并行**。

### 2.7 R6：ListView 吸收虚拟化

**目标**：`ul/ol/li/template → ListView`，把 driver 层虚拟化全部吸收进框架。

**内容**：
- `ul/ol` → `ListView`，`li` → `ListItem`。
- `ItemCount + BindItem + ItemTemplate + TemplateSelector`。
- 按模板分别池化。
- 虚拟化、可见区、不等高补偿、content size、后端 reuse key 全部内部化。
- 增量通知 `NotifyInserted/Removed/Moved`。
- 吸收旧 v1.4（虚拟列表）和 v1.11（列表强化：多列/翻页/吸附）。

**依赖**：R3。**可与 R5/R7 并行**。

### 2.8 R7：文本模型回归标准 HTML

**目标**：删除 `display:block` RichText 暗号，实现真正的 Block/Inline Formatting。

**内容**：
- 公共树保留语义节点（TextNode/TextElement/Image/Link），ID 和事件不丢。
- 内部文本布局编译成 TextRun/ImageRun/LinkRun。
- `p/h1-h6` 建立文本 block；inline 元素是语义容器。
- 跨标签换行、baseline、测量统一处理。
- 复用 v1.6 字体自绘 + v1.8 文字效果算法，换表达方式。
- 吸收旧 v1.7（富文本）。

**依赖**：R3。**可与 R5/R6 并行**。

### 2.9 R8：Showcase 端到端迁移

**目标**：showcase 全部改写为新 HTML 围栏 + 新公共 API，作为端到端验收。

**内容**：
- 删除所有预览 polyfill（`div{display:flex;flex-direction:column}` 等）。
- 删除 driver 手写虚拟列表、全局 NodeId 查找、`data-controller/data-page`。
- 每个页面改写为标准 HTML 元素 + 类型化对象 API。
- 作为公共契约的端到端测试。

**依赖**：R4 + R5 + R6 + R7。

---

## 3. R 系列之后：功能线

> R 系列完成后，以下功能在新基座上继续推进。旧编号保留对照，实际在新范式下实现。

| 功能 | 旧编号 | 新基座上的实现方式 | 状态 |
|---|---|---|---|
| TextInput / IME | v1.9 | `<input type=text>` + IME（R5） | R5 吸收 |
| 动画增强 | v1.10 | TweenManager 增强（@keyframes/ease/iteration） | R 后 |
| 列表强化（多列/翻页/吸附） | v1.11 | ListView 能力扩展（R6） | R6 吸收 |
| 滑块/进度条/锚点 | v1.12 | `<input type=range>`/`<progress>`/pivot（R5） | R5 吸收 |
| DragDrop/Window/Popup | v1.13 | 标准 DragDrop API + `<dialog>`（R5） | R5 吸收 |
| 离屏 RT 基础设施 | v1.14 | 不变（渲染层工作，与 API 范式无关） | R 后 |
| 高级滤镜 + BlendMode | v1.15 | 不变 | R 后 |
| 几何扩展 | v1.16 | 不变 | R 后 |
| 移动 + IL2CPP + WebGL | v1.17 | 不变 | R 后 |
| 编辑器/工具链闭环 | v other | 不变（独立于 runtime） | 并行 |

---

## 4. 机制草稿（实现期钉）

> 收留从主设计搬出的机制草稿——实现期才该定的细节。草稿不是契约：字段/算法实现时按真实约束调。

### 4.1 Shape mask + 两遍 DFS

RenderNode payload 加 `Mask{shape_ref, mode: MaskMode}`，MaskMode{Write,Content,Erase}。核心 DFS 算嵌套深度填 MaskContext。两遍 DFS sort_key 规则防批合越界。后端自选：Unity stencil / Godot canvas_group / 软件 alpha mask。

### 4.2 NativeHost

框架只提供机制——FFI 查询（world_matrix/sort_key/visible）、材质配置工具。业务（anchor/位置/scale）在 driver。

### 4.3 Controller / Transition（旧范式，已退役）

> v1.5 Controller 的 `data-controller/data-page` 已在 R5 中被标准 WAI-ARIA Pattern 替代。Transition 保留为时间线编排器，由控件状态变化触发。

### 4.4 包格式演进

集中式迁移器链；`nextPos` 长度前缀 forward-compat；branches（多语言）/highResolution（1x/2x/3x）；scaleLevel。R3 做破坏性升级。

### 4.5 契约版本化

公共头 `contract_version:u32` + `feature_flags:u64` + 可选扩展列。SemVer：加可选=minor，改必选=major。

### 4.6 其它

- **世界空间 UI**：NodeTransform 加 `Option<VertexMatrix>`。
- **DrawState 扩展**：DrawFlags 加 SoftClipped/Masked/AlphaMask/ColorFilter；BlendMode 全 12 种。
- **SRP 混合渲染**：自绘节点用 SRP RendererFeature 批合。

---

## 5. 借鉴 RmlUi / fgui 对标

> 完整结论见 `docs/roadmap/rmlui-research.md`。不能换 RmlUi 底层——核心三件套与 RmlUi retained 全量重画正面冲突。

- **可 port 的纯算法**：字体核心自绘（已做 v1.6）、滚动手感数学、transition 中断平滑。
- **别抄**：回弹/批合/filter/opacity（LoomGUI 已领先）；RmlUi 全 atlas 重建；Euler 滚动模型/分布式动画时钟（破不变量）；fgui Relations/Gears/可视化编辑/GTree/BMFont。

---

## 6. 关键决策

- **R 系列是范式转换，不是功能叠加**：公共 API 从"stage 全局"重做为"类型化对象树"，旧接口退役。底层算法（渲染/文本/滚动）保留复用，只重写接口形状。
- **靶子先行，护城河先验（2026-07-14 定）**：护城河只在设计期 HTML/CSS 的「AI 可预测性」上——R2–R8 是让框架成熟（FairyGUI/UITK 都有），不加宽护城河。故 R1 之后先冻结靶子（终态 showcase HTML + 终态 C# API 签名，不搭 mock 运行时），再用 Chromium 差分门（LoomGUI 渲染 vs 浏览器，非「HTML 在浏览器好不好看」）给核心赌注定性，然后才投入 R2–R7 大重写。差分门需先接一段最小 IR→渲染线（R2/R3 的一小片），不是零成本，但仍在大重写之前。
- **围栏终态化由 showcase 驱动**：现有 23 标签是骨架子集；缺什么由终态 showcase 暴露后补什么，不凭空补到理论清单。role dispatch（`<div role=tablist>→TabList`）是签名级结构缺口，单独立项。
- **设计自上而下，实现按合理路径**：设计是公共 API 优先，实现可以从内向外（R1→R2→R3 先骨架），不需要先让上层可用（开发期间无人使用）。
- **尽量并行**：R5/R6/R7 在 R3 完成后可多会话同时开工。
- **v1.5 Controller 停止**：旧 `data-controller/data-page` 路线停止，转入 R5 标准 ARIA Pattern。
- **编辑器并行**：编辑器工作流独立于 runtime，不阻塞 R 系列。
- **平台移植排最后**：功能+特效全做完之后。

---

## 7. 对标基线

- **对标 FairyGUI**：10 年沉淀，跨引擎，可视化编辑器。LoomGUI 精神继承 + 布局替换（flexbox 代 Relations）+ 类型化对象树（标准 HTML 元素决定类型）。
- **LoomGUI 差异化**：标准 HTML/CSS（AI 强先验，fgui .fui 二进制 AI 不能编辑）+ flexbox + Rust 跨引擎共享核心 + 围栏验证器 + 类型化对象树 API（标准 HTML 元素 → 稳定类型，CSS 赋予行为不改变类型）。
