# LoomGUI 主设计

> 跨引擎游戏 UI 框架。Rust 核心（引擎无关纯库）+ 多引擎后端（Unity 首发，Godot 等），标准 HTML/CSS 子集作设计期 DSL，类型化对象树作运行时 API，自绘渲染。
>
> **关联权威契约**：[fence.md](fence.md)（围栏）、[public-api.md](public-api.md)（公共 API 终态契约）、[projection-layer.md](projection-layer.md)（C# 投影层机制）。本文定总体架构与渲染管线；公共 API 以 public-api.md 为准。

## 1. 目标与非目标

### 1.1 目标

- **G1 编辑一次，多引擎一致**：同一份 HTML/资源包，在 Unity 及后续引擎上布局/文本/几何一致。
- **G2 标准 Web 语义**：HTML 围栏遵循标准 HTML/CSS 语义（Block/Flex/Inline），AI 读代码能正确预测渲染结果。
- **G3 类型化对象树**：运行时 API 是类型化的 Node 对象树（Container/Button/Slider/...），不是全局句柄 + 命令式 stage 调用。
- **G4 运行时动态**：UI 在运行时可任意增删改节点、跑动画、响应数据变化。
- **G5 渲染质量**：自绘、批合、遮罩/裁剪、九宫格、富文本；可挂引擎特效、世界空间 UI。
- **G6 可扩展**：标准控件 + 用户自定义业务组件（Web Components 约定）共存。

### 1.2 非目标

- 不做完整浏览器引擎（无完整 IFC、无 float、无 grid）。
- 不做 Unity UGUI/UIToolkit 兼容层。
- 编辑器单独项目，本文只定 DSL 规范、运行时 API 契约与渲染管线。

---

## 2. 总体架构

### 2.1 分层

```text
标准 HTML/CSS 子集（设计期 DSL，人/编辑器/工具链/AI 读写）
        │ pack + validate（打包期验证围栏，拒绝不支持的语法）
        ▼
不可变 UITemplate / Package（.pkg.bin + 图集）
        │ instantiate（克隆模板 → 类型化对象树）
        ▼
类型化语义对象树（Node / Container / Button / Slider / ...）
  - 公共 API：UIContext / Get<T> / 事件 / typed Style / 生命周期
        │ computed style（cascade + 伪类 rematch）
        ▼
布局、滚动、文本等内部 Behavior Strategy
  - Block/Flex/Overflow/Scroll 策略切换，不改变对象类型
        │ frame model
        ▼
渲染树（Vec<RenderNode>，意图化契约）
        │ FFI（SOA 扁平数组，引擎中立）
        ▼
引擎后端（Unity GameObject+MeshRenderer / Godot Node2D+canvas_item）
```

### 2.2 关键边界

- **公共语义层**：类型化 Node 对象树，是游戏业务程序员的唯一 API 表面。
- **内部行为层**：布局策略、滚动物理、文本排版、渲染状态计算。使用 Strategy/State/Bridge/Pool 等模式，不暴露给公共 API。
- **FFI 缝界**：SOA 扁平数组传渲染树 + 事件回传。NodeId 不出现在公共 API。
- **引擎后端**：输入采集、渲染树→原生对象镜像、资源加载。
- **不跨越的**：公共层不知道 GameObject/CanvasItem；后端不解析 HTML/CSS、不独立算布局、不生成几何。

### 2.3 架构原则

> **公共层暴露语义和意图；内部层实现变化。只有业务真正拥有决策权的策略才进入公共 API。**

- Composite：Node/Container 对象树。
- Abstract Factory：根据稳定 HTML 语义签名（tag + 结构属性）创建控件。
- Strategy + State：CSS 在不改变对象类型的前提下切换 Block/Flex、Overflow 等行为。策略只持算法，不持节点状态。
- Observer + 路由链：控件语义事件与捕获/冒泡事件。
- Bridge/Adapter：隔离 core、FFI 和具体引擎后端。
- Object Pool：ListView 按模板分别复用实例。
- Identity Map：同一个内部节点始终对应同一个公共 Node 对象。

---

## 3. HTML/CSS 围栏

> **权威清单 = `docs/design/fence.md`**（真相源是可执行测试 `crates/fence/src/schema/ + crates/fence/tests/`）。本节只写设计哲学与原则。

### 3.1 设计哲学：标准 HTML 语义 + AI 强先验

围栏是面向游戏 UI、能够完整兑现语义的标准 HTML 子集。不是假装支持整个浏览器，也不是四个标签的极小集。

**首要判据**：AI 读 HTML 能否正确预测渲染结果。所有围栏决策的第一判据。

**用标准 HTML 元素**：AI 训练数据海量、浏览器原生渲染。不自创框架 Widget 标签（如 `<scroll-view>`）——已有的标准 HTML/CSS 能力（如 `overflow`）不用自定义标签重复。

**标准布局语义**：
- `div/header/nav/p/ul/ol/li/option` 默认 `display:block`（标准浏览器默认）。
- `span/strong/em/label/button/a/img/canvas/input/textarea/select/progress/template/slot` 默认 inline。
- `display:flex` 默认 `flex-direction:row`（标准 CSS 默认）。
- 需要纵向堆叠明确写 `display:flex; flex-direction:column`。
- `display:block/flex/none` 选择内部布局 Strategy，**不改变节点类型**。
- `box-sizing:border-box` 作为 UA 样式例外（游戏 UI 友好），待 fence 补入 CSS 属性表并实现。

### 3.2 围栏元素

| 类别 | 元素 | 公共类型/语义 |
|---|---|---|
| 文档与样式 | `html/head/body/title/meta/style/link[rel=stylesheet]` | 打包和 authoring 元数据，不进入实时树 |
| 结构 | `div/header/nav` | `Container`（block 默认） |
| 文本 | `p/span/strong/em/br` | `TextBlock/TextElement/LineBreak` |
| 关联文本 | `label` | `Label` |
| 操作 | `button/a` | `Button/Link` |
| 图片与绘制 | `img/canvas` | `Image/Canvas` |
| 输入 | `input[type]` | `TextField/PasswordField/SearchField/NumberField/Slider/Toggle/RadioButton` |
| 输入 | `textarea/select/option` | `TextArea/Dropdown/OptionItem` |
| 列表 | `ul/ol/li` | `ListView/ListItem` |
| 模板 | `template` | 惰性 `UITemplate`，不进入实时树 |
| 内容投影 | `slot` | Custom Element 的标准 Slot |

`script` 不属于运行时围栏。

### 3.3 稳定语义签名

> **节点类型由稳定 HTML 语义签名决定：tag + 不可变结构属性。CSS 永远不决定类型。**

完整签名表见 [fence.md](fence.md) §3.1。类型分派只看 tag + `input[type]`：

- `<div>` / `<header>` / `<nav>` → `Container`
- `<p>` → `TextBlock`；`<span>` / `<strong>` / `<em>` → `TextElement`
- `<button>` → `Button`；`<a>` → `Link`；`<label>` → `Label`
- `<img>` → `Image`；`<canvas>` → `Canvas`；`<br>` → `LineBreak`
- `<input type="text">` → `TextField`（默认 type）
- `<input type="password">` → `PasswordField`
- `<input type="search">` → `SearchField`
- `<input type="number">` → `NumberField`
- `<input type="range">` → `Slider`
- `<input type="checkbox">` → `Toggle`；`<input type="radio">` → `RadioButton`
- `<textarea>` → `TextArea`；`<select>` → `Dropdown`；`<option>` → `OptionItem`
- `<ul>` / `<ol>` → `ListView`；`<li>` → `ListItem`
- `<progress>` → `ProgressBar`；`<template>` → `Template`；`<slot>` → `Slot`
- 含 `-` 的标签名 → `CustomElement`（R3 注册验证）

`input[type]` 是结构属性（Fence Gate 校验值域，Annotate 阶段决定最终类型），实例化后不能改成另一种控件类型。普通动态状态（`checked/selected/disabled`）可变。`role` / `aria-*` 是全局属性（Fence Gate 校验白名单值域），但不参与当前的 SemanticKind 分派——ARIA 复合控件类型是后续阶段（§3.4）。

### 3.4 WAI-ARIA 复合控件

HTML 没有原生 Tabs、Tree 等标签。此类控件采用白名单内的标准 WAI-ARIA Pattern，不使用 `data-widget` 或 `data-controller`：

```html
<div role="tablist" aria-label="设置">
    <button role="tab" aria-controls="graphics-panel" aria-selected="true">画面</button>
</div>
<div role="tabpanel" aria-labelledby="graphics-tab">...</div>
```

框架负责输入导航、`aria-selected`、`hidden` 同步。打包器验证 role 组合与 ARIA 关系。

### 3.5 失败策略

围栏外输入明确失败，不静默降级：

- 围栏外标签、属性、CSS 属性或属性值 → 打包期报错。
- 不支持的 `input[type]` 或 ARIA role → 打包期报错。
- `display:grid` 在真正实现前留在围栏外，不能降级成 Flex。
- 围栏外 CSS 不静默忽略。

### 3.6 围栏治理（防漂移）

单一真相源 = machine-readable schema（标签、属性、结构属性、CSS 值、运行时类型、后端需求）。解析器、打包器、绑定生成器、文档和测试不得各维护一份白名单。

防漂移门：`cargo test -p loomgui_fence`——改围栏后必跑。

---

## 4. 公共对象模型

### 4.1 对象层级

```text
Node
├── Container（内容模型 = 用户可编排的子节点）
│   ├── AbsolutePanel（语法糖，子节点自动 absolute）
│   ├── TextBlock（p）/ TextElement（span/strong/em）
│   ├── Label / Button / Link / Canvas
│   └── ListView（ul/ol）/ ListItem（li）
├── TextNode / Image / ProgressBar
├── TextField / PasswordField / SearchField / NumberField / Slider / Toggle / RadioButton
├── TextArea / Dropdown
├── LineBreak（br）/ OptionItem（option）/ Slot / CustomElement
```
> 注：`LineBreak`、`OptionItem`、`Slot`、`CustomElement` 在 Rust `NodeKind` 中已有对应变体，C# 投影类待后续控件束/复合束落地（当前 `NodeFactory` fallback 到 `Container`）。

- `Container` 才暴露子节点增删；叶子类没有 `AddChild()`。
- `Button`、`Link` 等可包含图标和文本，因此属于容器。
- 公共对象持有稳定身份，内部句柄（NodeId）不暴露。
- `input[type]` 是不可变结构属性（决定控件类型）；`role` / `aria-*` 是全局属性（不参与当前类型分派，见 §3.3）。
- **无 Panel/Component 类型**：作用域是运行时标记（`IsScopeRoot`），非类型；`Instantiate` 返回模板根真实类型。完整层级与划线见 [public-api.md](public-api.md) §2。

### 4.2 顶层上下文

`UIContext` 是显式顶层实例，拥有 Package、根节点、焦点、输入、时钟和后端连接。允许同进程存在多个独立上下文。**UIContext 是「获取而非创建」**——无公共构造，由引擎集成层创建并驱动；业务程序员从集成层获取一个已跑起来的 UIContext（见 [public-api.md](public-api.md) §11.3）。

```csharp
UIContext ui = backend.Context;                  // 由集成层提供，非 new
UIPackage game = ui.LoadPackage("game-ui", bytes);
Container home = game.Instantiate("views/home"); // 返回模板根真实类型
ui.Root.AddChild(home);
```

### 4.3 ID 与查询

标准 HTML `id` 是业务代码的结构契约。查找在组件实例作用域内递归，不穿透嵌套组件边界：

```csharp
Button start = home.Get<Button>("start");      // 缺失/类型不匹配 → UIContractException
home.TryGet<Button>("optional", out var btn);   // 可选
IReadOnlyList<Button> actions = home.Query<Button>(); // 零到多个
```

- 同一模板作用域内重复 ID 在打包期报错。
- 组件实例和 List item 模板实例各自拥有独立 ID 作用域（Shadow DOM 风格）。

### 4.4 生命周期

- `RemoveFromParent()` 只摘树。对象可重挂，属性、状态和监听器保留。
- `Dispose()` 才递归销毁子树、内部资源和事件订阅。
- 已销毁对象上的任何操作抛 `ObjectDisposedException`。
- detached 对象仍属于原 `UIContext`，不能跨 Context 挂载。

### 4.5 树操作

以对象为主语：`Parent`、`Children`、`ChildCount`、`AddChild`、`InsertChild`、`RemoveChild`。动态创建是次要逃生口：

```csharp
Container panel = ui.Create<Container>(); // canonical <div>
Button button = ui.Create<Button>();       // canonical <button>
panel.AddChild(button);
```

### 4.6 后续：强类型 View

结构稳定后可生成强类型 View：

```csharp
HomeView home = game.Views.Home.Instantiate();
home.Start.Clicked += OnStart;
home.Templates.MailItem;
home.Styles.Compact;
```

---

## 5. 样式

### 5.1 三条路径

1. authored HTML/CSS 是主要布局来源。
2. class 用于离散状态切换。
3. typed `Style` 用于运行时数值变化。

```csharp
panel.Classes.Add(HomeStyles.Compact);       // 生成的 StyleClass token
panel.Style.Width = Length.Px(320);
panel.Style.OverflowY = Overflow.Auto;
```

### 5.2 项目 class

项目 class 不能穷举成框架 enum。生成器从项目 CSS 产生 `StyleClass` token；无生成代码时保留 `AddClass("compact")` 和 raw style 逃生口。

### 5.3 CSS Cascade

- Specificity：标准 CSS tuple a-b-c（`inline > id > class > tag`）。
- 属性选择器与伪类同归 class 级（b）。
- **CSS 规则表进包（不 bake 丢）**：逻辑层运行时大量用 CSS（`Classes.Add/Replace`、`StyleSheet.Add`、class 切换驱动动画），规则表必须活到运行时，否则对设计期未带该 class 的节点 `Classes.Add` 会失效。cascade 引擎是 core 的运行时唯一真相源；fence 只把 `<style>` 解析成规则表。
- 运行时 rematch 处理伪类 + class + Style override 变化，每帧从 `base_style` 重算基线（`base_style` = 每帧 cascade 基线，非首帧缓存）。
- 运行时样式 = `base_style + 命中动态规则的合并`。
- 详见 `docs/roadmap/roadmap.md` §8（cascade 归属决策）与 §2 阶段 S（选择器解析器是净新代码）。

### 5.4 组件样式边界（Shadow DOM 风格）

- 模板内部选择器只作用于模板内部。
- 父组件普通选择器不穿透边界。
- 标准可继承属性和 CSS 自定义属性 `--*` 跨边界传递。
- 后续可用标准 `part/::part()` 精确开放内部样式。

---

## 6. 标准控件

HTML 属性提供初始值；C# 属性表示实时状态。用户输入和代码修改走同一状态通道。

| HTML | 公共类型 | 主要实时 API |
|---|---|---|
| `button` | `Button` | `Disabled`, `Clicked` |
| `a[href]` | `Link` | `Href`, `Activated` |
| `input[type=text]` | `TextField` | `Value`, `Placeholder`, `ReadOnly`, `ValueChanged`, `Submitted` |
| `input[type=password]` | `PasswordField` | `Value`, `Placeholder`, `ReadOnly`, `ValueChanged`, `Submitted` |
| `input[type=search]` | `SearchField` | `Value`, `Placeholder`, `ReadOnly`, `ValueChanged`, `Submitted` |
| `input[type=number]` | `NumberField` | `Value`, `Min`, `Max`, `Step`, `Disabled`, `ValueChanged` |
| `input[type=range]` | `Slider` | `Value`, `Min`, `Max`, `Step`, `Disabled`, `ValueChanged`, `ChangeCommitted` |
| `input[type=checkbox]` | `Toggle` | `IsChecked`, `Disabled`, `CheckedChanged` |
| `input[type=radio]` | `RadioButton` | `IsChecked`, `Name`, `Disabled`, `CheckedChanged` |
| `textarea` | `TextArea` | `Value`, `Placeholder`, `Selection`, `ReadOnly`, `Disabled`, `ValueChanged` |
| `select/option` | `Dropdown` | `SelectedIndex`, `SelectedValue`, `Disabled`, `SelectionChanged` |
| `progress` | `ProgressBar` | `Value`, `Max`, `IsIndeterminate` |

伪类 `:checked/:disabled/:focus` 匹配实时状态。RadioButton 同 `name` 组框架自动互斥（只新选中项触发 `CheckedChanged`）；按 name 聚合的 RadioGroup 是逻辑层积木，作用域边界由 `IsScopeRoot` 标记决定。控件数值（Slider/NumberField/ProgressBar）用 `float`。完整控件契约见 [public-api.md](public-api.md) §7。

`ValueChanged` 表示实时变化；`ChangeCommitted` 表示拖动结束、回车或失焦确认。所有控件仍保留通用路由事件（`node.On<PointerDownEvent>(...)`）。

---

## 7. 模板、组件与复用

### 7.1 UITemplate

每个独立 HTML 资产都编译为不可变 `UITemplate`。界面、弹窗、业务组件和列表项只是模板被使用时扮演的角色。

### 7.2 内联模板

```html
<ul id="mails">
    <template id="normal-mail">
        <li class="mail"><span id="title"></span></li>
    </template>
</ul>
```

内联 `<template>` 只属于当前组件。打包期验证 item template 根是 `<li>`。

### 7.3 包级共享模板

独立 `templates/mail-item.html` 可被多个界面引用：

```csharp
UITemplate item = common.GetTemplate("templates/mail-item");
```

模板资产只编译、缓存一份；每次实例化生成独立对象树、状态、事件和 ID 作用域。模板与实例化产物的关系同 Unity prefab：卸载模板不影响已实例化的活节点（独立副本）。

### 7.4 用户业务 Custom Elements

框架基础能力不得发明自定义标签。只有 HTML 没有对应概念的用户业务组件，才使用标准 Web Components 约定：

```html
<game-item-card id="sword" rarity="legendary">
    <button slot="action">装备</button>
</game-item-card>
```

- 名称必须包含 `-`。
- Package 注册表承担 `customElements.define()` 的角色。
- 标准 `<slot>` 提供内容投影。
- 未注册元素、无效 slot 在打包期报错。

---

## 8. ListView

声明使用标准 `ul/ol/li/template`：

```html
<ul id="mails">
    <template id="normal-mail"><li>...</li></template>
    <template id="reward-mail"><li>...</li></template>
</ul>
```

```csharp
ListView mails = view.Get<ListView>("mails");
UITemplate normal = view.GetTemplate("normal-mail");
UITemplate reward = view.GetTemplate("reward-mail");

mails.ItemCount = data.Count;
mails.TemplateSelector = index => data[index].HasReward ? reward : normal;
mails.BindItem = (item, index) => {
    item.Get<TextElement>("title").TextContent = data[index].Title;
};
```

契约：
- `ul/ol` → `ListView`，`li` → `ListItem`。
- 虚拟化是运行时实现决策（不进 HTML）；首次设 `ItemCount`/`ItemTemplate`/`BindItem` 即数据驱动 + 清空设计期 li。静态/数据驱动强制互斥（越界抛 `UIContractException`）。
- item 模板来源优先级：显式 `ItemTemplate`/`TemplateSelector` > 设计期 `<template id>` > 第一个 li 兜底。未设且 ul 下单个 `<template>` 自动用、多个 `<template>` 抛 `UIContractException`。
- `TemplateSelector` 是纯 `Func<int, UITemplate>`；用户 `view.GetTemplate("name")` 取 template 后塞 lambda 闭包按 index 选，框架不自动收集。
- `TemplateSelector` 返回 `UITemplate` 对象，不返回字符串。
- ListView 按模板分别池化。
- 虚拟化、可见区、测量补偿、content size 和后端 reuse key 全部是内部实现。

刷新 API：`RefreshItem(index)`、`RefreshItems()`、`NotifyInserted/Removed/Moved`。完整契约见 [public-api.md](public-api.md) §8。

---

## 9. 事件

### 9.1 控件语义事件

```csharp
button.Clicked += OnStart;
slider.ValueChanged += OnVolumeChanged;
```

### 9.2 类型化路由事件

所有节点同时提供类型化路由事件（捕获 → 目标 → 冒泡）：

```csharp
node.On<PointerDownEvent>(OnPointerDown);
```

- `Target` 与 `CurrentTarget` 都是公共 `Node`。
- 节点 `Dispose()` 自动清理其订阅。
- `RemoveFromParent()` 不清理订阅。
- 内部后端事件不得泄漏 NodeId 或 FFI 结构。

### 9.3 命中测试

命中按等效绘制顺序逆序（后画的先命中）。命中几何 = `layout_rect` 经累计 world_matrix（含父链 transform）变换后的 AABB。

### 9.4 拖拽与滚动仲裁

拖拽与滚动通过阈值 + 退让机制仲裁。先达者赢，另一方查全局 `dragging_node`/`scrolling_pane` 主动退让。

### 9.5 引擎输入桥

核心定义 `InputProvider` trait（指针/键/触摸/IME character），后端实现并每帧注入。坐标核心左上原点；翻转在后端根一次性做。

### 9.6 UI 输入消费

```csharp
bool hit = ui.IsPointerOnUI;
```

极简：核心命中后存当前指针命中的节点，暴露事实查询。不做消费策略/consume 标志/每指针数组。

---

## 10. 文本与 Inline Formatting

### 10.1 正常 HTML 子树

删除旧 `display:block` RichText desugar 暗号和特殊公共 `RichText` 类型。富文本就是正常 HTML 子树：

```html
<p id="description">
    对敌人造成 <strong id="damage">120</strong> 点伤害
    <img src="fire.png" alt="火焰">
    <a id="details" href="skill://fireball">详情</a>
</p>
```

### 10.2 公共对象树

公共树保留 `TextNode/TextElement/Image/Link` 的 ID、样式和事件。

### 10.3 内部文本布局

内部文本布局将最近 Inline Formatting Context 编译成 TextRun、ImageRun 和 LinkRun，用于统一换行、baseline、测量与几何构建。

- 裸文本形成叶子 `TextNode`。
- inline 元素是语义容器。
- `p` 建立文本 block。
- `TextContent` 与 DOM 一样，用纯文本替换当前全部子内容。
- 修改 inline 子树只使最近文本上下文失效。

公共语义树与内部布局/渲染树可以不同。

### 10.4 文本测量

taffy 对"尺寸取决于内容"的节点回调 `MeasureFunc(known_dimensions) -> measured_size`：给定约束宽返回 `(text_width, text_height)`。必须廉价、无副作用（auto-size/shrink 反复调用）。建在核心自绘字体地基上（ttf-parser outline + ab_glyph 光栅 + etagere 图集）。

---

## 11. 布局层

### 11.1 布局策略

`display:block/flex/none` 选择内部布局 Strategy：

- **Block**：标准块级布局（子元素垂直堆叠，margin collapse）。
- **Flex**：flexbox（标准 CSS flexbox 规范子集）。默认 `flex-direction:row`（标准 CSS）。
- **None**：`display:none`，不参与布局和渲染。

布局策略切换不改变节点类型。策略只持算法，不持节点状态。

### 11.2 taffy 集成

场景图 Container 树 ↔ taffy 节点树一一对应。增删 Container 同步增删 taffy 节点；改 style 同步改 taffy style 并标记子树 layout dirty。

taffy 0.5 同时支持 Flex 和 Block 布局算法。`display:block` 使用 `compute_block_layout`，`display:flex` 使用 `compute_flexbox_layout`。

### 11.3 尺寸模型 → 映射

| CSS | 布局算法 |
|---|---|
| `width/height`(px/%) | `size` |
| `min/max` | `min_size`/`max_size` |
| `flex-basis` / `flex-grow/shrink` | 同名（flex 模式） |
| `flex-direction/wrap/gap` / `justify/align-*` | 同名（flex 模式） |
| `padding/border-width/margin` | `padding`/`border`/`margin` |
| `position:relative`+insets | `Relative`+`inset`（视觉偏移，不影响兄弟布局）。注：taffy `Style::DEFAULT.position` 已是 `Relative`，显式写 `position:relative` 为 no-op |
| `position:absolute` | taffy `Absolute` + inset（脱离流） |
| 内容自适应（文本/图片） | `MeasureFunc` 回调（§10.4） |

### 11.4 响应式与异形屏

- **resize**：屏幕尺寸变 → 根节点 size 变 → 整树 solve。
- **safe-area**：后端把 insets 注入核心；CSS 用百分比 + 环境变量表达避让。
- **动态内容/数据变化**：改文本/增删子节点 → 置 dirty → 下帧 solve。

### 11.5 参考分辨率 / DPI 缩放

设计稿 1080×1920 在 1440×2560 整体等比放大。引擎集成层（C# Driver）持设计分辨率，据屏幕实际尺寸计算 scale 后设置 `Stage.root_size`，核心按此根尺寸布局。`match_mode`（shrink-to-fit 策略）同样在引擎集成层配置。

叠加顺序：先参考分辨率整体 scale → 再布局 → 最后 safe-area 避让。

### 11.6 滚动

任意 `Container` 通过标准 `overflow:auto/scroll` 获得滚动行为，对象类型保持不变（§3 设计哲学：CSS 赋予能力，不改变类型）。

```css
#inventory { overflow-y: auto; }
```

内部 Overflow Strategy 可以在 Visible、Clip、AutoScroll 和 Scroll 间切换；`ScrollState` 独立保存。非滚动态调用滚动 API 遵循 DOM，位置被钳制或不产生视觉滚动。

**惯性回弹物理**：ScrollPane 自维护可变 target 的 tween，content size 变化时按状态补偿 start、不突变。不走 GTween（content 异步变化时 GTween 的固定 end 会跳变）。tick 分两段：`advance_all`（惯性/回弹物理推进）在 solve 前消费指针输入并推进滚动位置；`refresh_content_sizes`（内容尺寸刷新）在 solve 后、compute_world_transforms 前。

能力：滚动类型、惯性+回弹、滚动条、鼠标滚轮。分页/吸附/下拉刷新后期。

---

## 12. 渲染层（自绘，渲染树契约）

> **核心原则**：渲染树契约描述**渲染意图**（画什么/遮罩意图/绘制顺序），**不规定**引擎实现机制。后端各自选择。

### 12.1 坐标系

核心唯一真相源：左上原点，y 向下。后端根 Stage 做翻转（如 Unity flips y）。

### 12.2 几何生成

非文本几何（图片 quad/形状/九宫格/填充）在 Rust 核心生成（确定性、跨引擎一致）。文本 mesh 同样在核心生成（核心自绘字体，v1.6+）。

### 12.3 DrawState

核心不算材质对象，只算 draw 所需状态：
- `DrawFlags`(u32)：`Clipped|Grayed` 等。
- `BlendMode`：Normal 等基础。
- `ProgramId`：Image/Text/BG_COMPOSITE/ColorFilter 等。
- 后端按 `(program+flags+blend+texture+mask_context)` 维护 DrawState 缓存。

### 12.4 批合（FairyBatching）

两元素能并入同批 ⟺ DrawState 相同（AABB 不相交则可重排聚拢；同 DrawState 相交仍可合）。DFS 遇 `clip_rect` 的 Container 强制其为 BatchingRoot；批合收集不下钻进 root 子树。core 显式合并 mesh → 真 N→1 draw call。

### 12.5 裁剪/遮罩

**rect mask**：核心给 clip_box；后端自选实现。`mask_context` 是批合边界。嵌套 `overflow:hidden`：clip 区域取祖先 clip 链的交集。

soft clip/shape mask/paintingMode 见 roadmap（机制草稿）。

### 12.6 RenderNode 契约

```rust
struct RenderNode {
    node_id: u32,                     // 与 scene.nodes 索引对齐（build 直填 n.id.0）
    parent_id: Option<u32>,
    visible: bool,
    alpha: f32,
    // grayed: bool — deferred（灰化禁用节点渲染，待视觉束落地）
    color_tint: [f32; 4],
    world_matrix: Affine2,            // 已累计的 world-space 仿射矩阵（过渡；终态用 NodeTransform）
    blend: BlendMode,
    mask_context: MaskContext,
    sort_key: u32,
    change_level: ChangeLevel,        // Skip=0 / Header=1 / Full=2
    reuse_key: u32,                   // MirrorPool GO 复用键
    effect: EffectBlock,              // 文字效果参数（128B 定长）
    payload: NodePayload,
}

enum NodePayload {
    Mesh { verts, uvs, colors, indices, image_path, program, color_matrix },
    // Mask / PaintTarget / NativeHost — 见 roadmap
}
```

> 注：`grayed` 灰化渲染待 visual beam 落地；`world_matrix: Affine2` 为 v1 过渡形态，终态替换为 `NodeTransform`（分解 Position/Scale/Rotation，对齐 public-api.md 三分模型）。

`ChangeLevel::Skip/Header/Full` 表达本帧变化程度。

---

## 13. 动画（单时钟）

### 13.1 TweenManager

整个核心只有一个动画时钟 `TweenManager::update(dt)`。

- `TweenManager { active, pool }`，池化。
- `Tweener`：统一 `TweenValue{x,y,z,w,d}` + `value_size(1..6)`。
- 链式 builder：`tween(start,end,dur).delay().ease().repeat(,yoyo).on_complete()`。
- 缓动：Linear/Sine/.../Elastic/Back/Bounce 的 In/Out/InOut + Custom。
- `prop_type` 分层：tween 写属性区分 "transform 属性"（x/y/scale/rotation，置 `transform_dirty`，不 solve）vs "layout 属性"（width/height/flex，置 `layout_dirty` 触发 solve）。

### 13.2 Transition

纯数据 `items: Vec<TransitionItem>`。`Play()` 把每个 item 翻译成 Tweener 提交 TweenManager。与控件状态（如 Toggle 切换、TabList 切换）正交，由状态变化触发。

### 13.3 Timers

独立通用周期/延时回调（unscaled_dt），与动画解耦。`CallLater`（下一帧）、`AddUpdate`（每帧）。

---

## 14. 资源 / 包系统

### 14.1 双格式

- **编辑期/源**：HTML（结构）+ CSS（样式）+ 资源清单。
- **发布产物**：编译成**单一二进制 blob**（`.pkg.bin`）+ 图集（`atlas.png` + `atlas.json`）。
- 运行时**只认二进制**；HTML 解析只在打包器。

### 14.2 图集（Rust 自绘，打包器产出）

打包器 `loomgui_pkg` 自绘产出 `atlas.png` + `atlas.json`。核心只持图片归一化 `sprite_key` + 图片原始像素尺寸。后端 `SpriteResolver` 据 atlas.json 的 UV 字典取子区 UV。

### 14.3 运行时 Bootstrap

驱动启动时读 `loom.runtime.json`（声明包/图集/字体列表）→ 加载各 `.pkg.bin` + 图集 → 解析 atlas.json 中每张图的 `orig` 尺寸推入核心 → 初始化 SpriteResolver。

### 14.4 包格式

- Header（20B）：magic（`0x474B504C`，"LPKG" LE）+ formatVersion（u32）+ flags（u32，预留）+ component_count（u32）+ string_count（u32）。
- 组件描述分块，运行时只读需要的块。
- 全局 stringTable 去重。
- 跨资源引用存 id 不存内容。
- 版本协商：Header `formatVersion` + runtime 声明 `min/max_supported_version`。

### 14.5 纹理

核心只持 `TexId`（整数）。图集：一张大纹理 + N 个轻量 TextureView（只存 UV）。子 view 首引用连带 root；归零通知后端可卸载。GPU 生命周期全在后端。

---

## 15. FFI 与引擎后端

### 15.1 csbindgen

- Rust 端 `#[no_mangle] extern "C"` + `csbindgen` 生成 C# `[DllImport]`。
- `csharp_use_function_pointer(false)` 切 Mono 模式（IL2CPP 友好）。
- `[GroupedNativeMethods]` context 指针模式。

### 15.2 IL2CPP 注意

- 回调必须 `static` + `[MonoPInvokeCallback]`。
- string 永远走 UTF-8 `byte*`。
- 内存所有权严格隔离：跨边界传 POD/指针/扁平 buffer。
- 高频调用用扁平数组（pin 或拷贝）。

### 15.3 跨边界数据（SOA + per-frame arena）

每帧 FFI 传：
1. RenderNode 公共头 SOA（定长字段并行存储）。
2. 按类型分区的 per-frame arena（mesh 顶点/UV/颜色/索引、path 表等）。
3. ChangeLevel（Skip/Header/Full）：Skip/Header 不写 arena。

C# tick 内一次拷完。后端维护双 dict（`_poolByNodeId` + `_poolByReuse`）做 stale-mark-sweep 镜像同步，O(n) 每帧。

### 15.4 渲染对象镜像生命周期

- Rust 核心拥有场景图 + 渲染状态（真相源）；后端拥有渲染对象镜像（派生缓存）。
- 每帧脏增量同步：全标 stale → 遍历 render_nodes 按复用键查池 → 命中清 stale 并按 change_level 更新 → 仍 stale 的销毁。
- 无 double-free/use-after-free：Rust 只持整数 id。

### 15.5 原生库分发

编译产出多平台原生库（`.dll`/`.so`/`.dylib`/iOS `.a`/Android `.so`）。csbindgen 生成 C# 绑定源码。Unity Domain Reload 保护。

---

## 16. 更新循环（每帧管线）

```text
引擎 update（C# 投影层 + Rust 核心，见 projection-layer.md）:
  1. set_input()                       ← 后端采集指针/键/触摸/IME
  2. flush 脏属性回写                   ← C# 投影层：攒批的 Style(css 串)/Transform(数值) 推 Rust（tick 前）
  3. context.tick(dt) — 显式依赖拓扑：
     a. TweenManager.update(dt)        ← 唯一动画时钟（ScrollPane 物理是例外，自维护 tween）
     b. 消费 pending_focus_request
     c. process 指针输入               ← 多槽命中测试（用上帧 world）+ 拖拽/滚动/点击仲裁
     d. scroll.update + 消费 wheel      ← 惯性/回弹物理
     e. process_keys                    ← keydown/up（无自动 Tab 导航——方向键/手柄导航是逻辑层积木）
     f. rematch                         ← 伪类 :hover/:active/:focus/:disabled/:checked 重 cascade（class/style 变更下帧生效）
     g. transition drain                ← 消费 transition 请求，提交 tween（基线 = 上帧 computed）
     h. solve                           ← Block/Flex 各自算法（每帧一次，帧末一致）
     i. refresh_content_sizes           ← scroll content_size 刷新
     j. compute_world_transforms        ← DFS 累计 world matrix（含 Transform 渲染偏移，不触发 solve）
     k. build_render_nodes              ← 剪 display:none + dirty hash + 批合 + sort_key
     l. 输出 Vec<RenderNode>（SOA blob）
  4. 后端 borrow_frame → MirrorPool 同步镜像；borrow_events → 事件路由 → 业务回调
```

关键：
- **flush 在 tick 前**：C# 投影层攒批的属性写（Style/Transform）在 tick 之前一次性推 Rust，与 set_input 合并过桥。见 [projection-layer.md](projection-layer.md) §2.1。
- **rematch 在 solve 和 compute 之前**——伪类/class/style 变更当帧全部生效。class 切换驱动动画的下帧 rematch + 上帧 computed 做 transition 基线见 [public-api.md](public-api.md) §9.1。
- **hit_test 用上帧 world_transforms**（1 帧延迟）；scroll_pos 同帧进 world。
- **事件回调里改的布局属性延迟到下帧 solve**（避免反馈环）；Geometry 读的是最近完成的 solve（滞后一帧，同 web reflow）。
- **单一动画时钟**：TweenManager.update(dt) 是唯一时钟；OnUpdate 是逻辑驱动每帧钩子（非动画系统）。
- transform 动画不改布局，不触发 solve。

---

## 17. 跨引擎扩展

引擎集成层分两层（Spec-4b 落地，commit `8e2df1c..d4c0f28`，branch `spec4b`）：

```
[引擎无关 · C# 共享 · Unity+Godot-C# 复用]
  Public/         UIContext/Node/Button/Style（业务 API，4a 已有）
  Projection/     NodeRegistry/EventDemuxer/EventBus（4a 已有）
  Host/           LoomHost        ← stage 宿主 + 每帧驱动序（零 UnityEngine）
                  LoomBackend      ← 抽象契约（本 § 三件事）

[Unity 特定 · 各引擎各写]
  UnityLoomBackend : LoomBackend   ← 持 MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector
  LoomStageDriver (MonoBehaviour)  ← 瘦宿主：Unity 生命周期 + 资源 IO + 创建 Host/Backend
```

- **LoomHost（引擎无关，`Runtime/Host/`）**：持 stage handle (IntPtr) + UIContext + LoomBackend。零 `using UnityEngine`。每帧驱动 `Step(dt)` 严格按 §16 五步序：(1) `backend.CollectInput(stage)` → set_input；(2) UIContext flush 脏属性（4a 即时过桥 seam）；(3) `loomgui_stage_tick` FFI；(4) `borrow_frame` FFI → `backend.SyncFrame(stage, framePtr, frameLen)`；(5) `borrow_events` FFI → EventDemuxer → EventBus typed `On<T>` 路由。资源 FFI 引擎中立（RegisterFont / SetImageSizes / SetFallbackFamilies）放此层。`borrow_frame` 的 FFI 调用归 LoomHost（产生引擎特定镜像对象的 FFI 仍归引擎无关驱动核心），backend 只消费 blob 做镜像。
- **LoomBackend（引擎无关抽象契约，`Runtime/Host/`）**：契约 = 2 个 abstract 方法——`CollectInput(stage)` / `SyncFrame(stage, framePtr, frameLen)`。`set_input` FFI 在 backend（采集引擎特定但 FFI 引擎中立，省一次交互）。资源对象上传（如 Texture2D 上传 atlas 页）是引擎特定实现细节，不进入抽象契约（由 `UnityLoomBackend` 内部方法如 `InitSprites`/`SyncFontAtlas` 承担）。
- **UnityLoomBackend : LoomBackend**：持 MirrorPool + MaterialManager + NativeHostManager + SpriteResolver + InputCollector（零改复用，从退役的 LoomStage 搬过来）。NativeHost（GameObject 绑定 3D 模型）作为 UnityLoomBackend 额外方法，不进通用契约（Unity 专属概念）。
- **LoomStageDriver（Unity MonoBehaviour，瘦宿主）**：Awake 创建 UnityLoomBackend（注入 Unity 组件）→ `new LoomHost(designSize, backend)` → 读 .ttf/atlas 喂 `host.RegisterFont`/资源 → `ctx.LoadPackage`。Update 调 `host.Step(Time.unscaledDeltaTime)`。保留 Unity 特定（相机 / safeArea / 输入钩子 / 设计分辨率 / NativeHost 根 transform）。

> **LoomStage 退役**：v1 的 `LoomStage`（业务 API 透传层）在 Spec-4b clean break 整层删，无双壳——业务 API 透传已被 4a UIContext 取代，driver 的 ~10 个生命周期/后端编排调用按上述分层迁移。终态契约里只有 LoomHost/LoomBackend/UnityLoomBackend，无 LoomStage。

- **Godot 后端**：镜像成 Node2D + RenderingServer canvas_item 自绘。否决 Control 路线（与核心布局双系统冲突）。遮罩用 canvas_group/clip。复用 LoomHost + 整个 Projection + Public，只写 `GodotLoomBackend : LoomBackend`。
- **SRP 混合渲染**（Unity 增强）：自绘节点用自定义 SRP RendererFeature 批合绘制。
- 新后端只需实现：消费 `Vec<RenderNode>` + 输入注入 + 资源加载。契约引擎中立。
