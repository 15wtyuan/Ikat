# LoomGUI 面向业务的运行时 API 与 HTML 围栏重构设计

> 日期：2026-07-13
>
> 状态：已完成逐节设计确认，待书面 spec 审阅
>
> 目标用户：游戏业务程序员
>
> 参考：FairyGUI、RmlUi、Unity UI Toolkit、标准 HTML/CSS、WAI-ARIA

## 1. 结论

LoomGUI 的唯一公共表面是面向游戏业务程序员的类型化 UI 对象树。`NodeId`、FFI、帧同步、后端镜像和引擎接入接口全部是内部实现。

设计期使用有明确围栏的标准 HTML/CSS；运行时使用 `UIContext`、`Node`、`Container` 和具体控件。HTML 的稳定语义决定对象类型，CSS 选择布局、滚动、裁剪等可变行为，但不改变对象类型。

本次采用自上而下的纵向重构：公共契约优先，core、pkg、FFI 和 Unity 后端均可为它重写。旧实现和迁移成本不得反向限制公共 API。

## 2. 调研结论

| 框架 | 值得采用 | 不直接照搬 |
|---|---|---|
| FairyGUI | 类型化对象、包和模板、对象事件、列表复用 | 绝对定位中心模型、字符串 URL、Controller/Gear 私有 DSL |
| RmlUi | Document/Element 树、HTML/CSS、DOM 事件与查询 | 面向浏览器的低层 DOM 操作感 |
| UI Toolkit | `VisualElement` 对象树、UQuery、ListView 虚拟化、USS 状态样式 | Unity 专属 API、默认 column flex 对标准 HTML 的偏离 |

现有 LoomGUI 已有标准 `id/class/overflow/flex` 的基础，但仍存在关键问题：

- 围栏只有 `div/span/img/button`。
- `div` 被硬编码为 Flex Column，缺少真正的 Block/Inline 语义。
- `display:block` 是 RichText desugar 暗号，而不是真正 Block。
- `FindNodeById` 在整个 Stage 返回第一个匹配，多实例不安全。
- 列表由业务 driver 手写可见区、节点创建和 `reuse_key`。
- `data-controller/data-page` 是私有状态协议。
- 用户直接接触 `uint NodeId`、哨兵、中央事件表和 FFI 时序。

## 3. 架构边界

```text
标准 HTML/CSS 子集
        │ pack + validate
        ▼
不可变 UITemplate / Package
        │ instantiate
        ▼
类型化语义对象树
        │ computed style
        ▼
布局、滚动、文本等内部 Behavior Strategy
        │ frame model / bridge
        ▼
Unity 后端 / 未来其他后端
```

公共层只表达语义和意图；内部层可使用 Strategy、State、Bridge、Pool、Identity Map 等模式。只有业务真正拥有决策权的策略才公开，例如 `TemplateSelector`。

### 3.1 采用的模式

- **Composite**：`Node/Container` 对象树。
- **Abstract Factory**：根据稳定 HTML 语义签名创建控件。
- **Strategy + Null Object**：CSS 在不改变对象类型的前提下切换 Block/Flex、Overflow 等行为。
- **Observer + 路由链**：控件语义事件与捕获/冒泡事件。
- **Bridge/Adapter**：隔离 core、FFI 和具体引擎后端。
- **Object Pool**：ListView 按模板分别复用实例。
- **Identity Map**：同一个内部节点始终对应同一个公共 `Node` 对象。

策略只持算法，不持节点状态。滚动位置、焦点、按压、选择等状态保存在稳定节点或独立 State 中，因此 CSS 策略切换不会意外丢状态。

## 4. 顶层上下文

`UIContext` 是显式顶层实例，拥有 Package、根节点、焦点、输入、时钟和后端连接。允许同进程存在多个独立上下文；可提供 `UIContext.Default` 作为小项目糖，但所有对象始终绑定明确上下文。

```csharp
UIContext ui = new UIContext(backend);
UIPackage game = ui.LoadPackage("game-ui", bytes);
Component home = game.Instantiate("views/home");
ui.Root.AddChild(home);
```

## 5. 公共对象模型

```text
Node
├── Container
│   ├── Component
│   ├── TextElement / TextBlock / Label / Link
│   ├── Button
│   ├── ListView / ListItem
│   ├── Form / Disclosure / Dialog
│   └── 用户 Custom Element 生成类型
├── TextNode
├── Image
├── TextField / NumberField / Slider / Toggle / RadioButton
├── TextArea / Dropdown / ProgressBar / Meter
└── Canvas
```

- `Container` 才暴露子节点增删；叶子类没有无意义的 `AddChild()`。
- `Button`、`Link` 等可包含图标和文本，因此属于容器。
- 公共对象持有稳定身份，内部句柄不暴露。
- `input[type]` 和白名单内 `role` 是不可变结构属性；动态状态不改变对象类型。

树操作以对象为主语：`Parent`、`Children`、`ChildCount`、`AddChild`、`InsertChild`、`RemoveChild`。动态创建是次要逃生口，节点仍由 Context 工厂创建并绑定该 Context：

```csharp
Container panel = ui.Create<Container>(); // canonical <div>
Button button = ui.Create<Button>();       // canonical <button>
panel.AddChild(button);
```

第一阶段提供类型化对象树和 `Get<T>`；结构稳定后可生成强类型 View：

```csharp
HomeView home = game.Views.Home.Instantiate();
home.Start.Clicked += OnStart;
home.Templates.MailItem;
home.Styles.Compact;
```

## 6. 生命周期与树操作

- `RemoveFromParent()` 只摘树。对象可重新挂载，属性、状态和监听器保留。
- `Dispose()` 才递归销毁子树、内部资源和事件订阅。
- 已销毁对象上的任何操作抛 `ObjectDisposedException`。
- 父节点 `Dispose()` 递归销毁拥有的子树。
- detached 对象仍属于原 `UIContext`，不能跨 Context 挂载。

```csharp
panel.RemoveFromParent(); // 可重挂
other.AddChild(panel);
panel.Dispose();          // 永久失效
```

## 7. ID、查询和组件作用域

标准 HTML `id` 是业务代码的结构契约，不新增 `name` 查找语义。

```html
<button id="start">开始游戏</button>
```

```csharp
Button start = home.Get<Button>("start");
```

规则：

- `Get<T>(id)` 在当前组件实例内递归查找。
- 查询不穿透嵌套组件、Custom Element 或 List item 的组件边界。
- 每个组件实例和每个 item template 实例拥有独立 ID 作用域。
- 同一个模板作用域内重复 ID 在打包期报错。
- `id` 是模板结构契约，实例化后不可改；动态节点在创建时确定 ID。

失败语义：

- `Get<T>`：缺失或类型不匹配时抛 `UIContractException`。
- `TryGet<T>`：表达可选节点，不抛缺失错误。
- `Query<T>`：返回零到多个结果，空集合合法。

```csharp
home.TryGet<Button>("optional", out var optional);
IReadOnlyList<Button> actions = home.Query<Button>();
```

## 8. 新 HTML 元素围栏

围栏是面向游戏 UI、能够完整兑现语义的标准 HTML 子集，不是假装支持整个浏览器。

| 类别 | 元素 | 公共类型/语义 |
|---|---|---|
| 文档与样式 | `html/head/body/title/meta/style/link[rel=stylesheet]` | 打包和 authoring 元数据，不进入实时树 |
| 结构 | `div/main/section/header/footer/nav/article/aside` | `Container` |
| 文本 | `span/p/h1-h6/strong/em/small/br` | Inline/Text Block 语义 |
| 关联文本 | `label` | `Label` |
| 操作 | `button/a` | `Button/Link` |
| 图片与绘制 | `img/canvas` | `Image/Canvas` |
| 输入 | `input/textarea/select/option` | 见 §11 |
| 状态反馈 | `progress/meter` | `ProgressBar/Meter` |
| 列表 | `ul/ol/li` | `ListView/ListItem` |
| 模板 | `template` | 惰性 `UITemplate`，不进入实时树 |
| 展开与弹窗 | `details/summary/dialog` | `Disclosure/Dialog` |
| 表单分组 | `form/fieldset/legend` | `Form/Container` |
| 内容投影 | `slot` | Custom Element 的标准 Slot |

不支持的标签、属性、CSS 或结构组合在打包期报错，不静默降级。

`script` 不属于运行时围栏；浏览器预览所需适配代码由预览工具注入，不能混进生产组件源文件。`DOCTYPE`、文档壳和样式元数据允许作者写正常 HTML，但只有 `body` 内的 UI 内容进入组件语义树。

### 8.1 稳定语义签名

节点类型由稳定 HTML 语义签名决定：`tag + 不可变结构属性`。

- `<input type="range">` → `Slider`
- `<input type="checkbox">` → `Toggle`
- `<div role="tablist">` → `TabList`
- CSS class、伪类、computed style 永远不改变 C# 对象类型。

## 9. 标准布局语义与 CSS Behavior

取消“`div` 永远是 Flex Column”的旧铁律。

- `div/main/section/...` 默认 `display:block`。
- `span/a/strong/em/...` 默认 inline。
- `display:flex` 使用标准默认 `flex-direction:row`。
- 纵向堆叠明确写 `display:flex; flex-direction:column`。
- `display:block/flex/none` 选择内部布局 Strategy，不改变节点类型。
- `display:grid` 在真正实现前留在围栏外，不能降级成 Flex。
- 继续采用游戏 UI 友好的 `box-sizing:border-box` 默认值；这是围栏中明确记录的 UA 样式例外。

```css
.stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
}
```

### 9.1 滚动是能力，不是节点类型

HTML 没有 ScrollView 元素。任意 `Container` 通过标准 `overflow:auto/scroll` 获得滚动行为，对象类型保持不变。

```css
#inventory { overflow-y: auto; }
```

```csharp
Container inventory = view.Get<Container>("inventory");
inventory.ScrollTo(y: 300, behavior: ScrollBehavior.Smooth);
inventory.Scrolled += OnScrolled;
```

内部 Overflow Strategy 可以在 Visible、Clip、AutoScroll 和 Scroll 间切换；`ScrollState` 独立保存。非滚动态调用滚动 API 遵循 DOM，位置被钳制或不产生视觉滚动，不改变对象类型。

## 10. 样式公共 API

三条路径各司其职：

1. authored HTML/CSS 是主要布局来源；
2. class 用于离散状态切换；
3. typed `Style` 用于运行时数值变化。

```csharp
panel.Classes.Add(HomeStyles.Compact);       // 生成的 StyleClass token
panel.Style.Width = Length.Px(320);
panel.Style.OverflowY = Overflow.Auto;
```

项目 class 不能穷举成框架 enum。生成器从项目 CSS 产生 `StyleClass` token；无生成代码时保留 `AddClass("compact")` 和 raw style 逃生口。raw string 解析失败抛 `UIStyleException`，围栏支持的属性均应有 typed API。

## 11. 标准控件

| HTML | 公共类型 | 主要实时 API |
|---|---|---|
| `button` | `Button` | `Disabled`, `Clicked` |
| `a[href]` | `Link` | `Href`, `Activated` |
| `input[type=text/password/search]` | `TextField` | `Value`, `Placeholder`, `ReadOnly`, `ValueChanged`, `Submitted` |
| `input[type=number]` | `NumberField` | `Value`, `Min`, `Max`, `Step`, `ValueChanged` |
| `input[type=range]` | `Slider` | `Value`, `Min`, `Max`, `Step`, `ValueChanged`, `ChangeCommitted` |
| `input[type=checkbox]` | `Toggle` | `IsChecked`, `IsIndeterminate`, `CheckedChanged` |
| `input[type=radio]` | `RadioButton` | `IsChecked`, `Name`, `CheckedChanged` |
| `textarea` | `TextArea` | `Value`, `Selection`, `ValueChanged` |
| `select/option` | `Dropdown` | `SelectedIndex`, `SelectedValue`, `SelectionChanged` |
| `progress` | `ProgressBar` | `Value`, `Max`, `IsIndeterminate` |
| `meter` | `Meter` | `Value`, `Min/Max/Low/High/Optimum` |
| `details/summary` | `Disclosure` | `IsOpen`, `OpenChanged` |
| `dialog` | `Dialog` | `Show`, `ShowModal`, `Close`, `Cancelled`, `Closed` |

HTML 属性提供初始值；C# 属性表示实时状态。用户输入和代码修改走同一状态通道，`:checked/:disabled/:focus/:open` 匹配实时状态。RadioButton 以标准 `name` 在当前组件或 Form 作用域分组。

## 12. WAI-ARIA 复合控件

HTML 没有原生 Tabs、Tree 等标签。此类控件采用白名单内的标准 WAI-ARIA Pattern，不使用 `data-widget` 或 `data-controller`。

```html
<div id="settings-tabs" role="tablist" aria-label="设置">
    <button id="graphics-tab" role="tab"
            aria-controls="graphics-panel" aria-selected="true">画面</button>
</div>
<section id="graphics-panel" role="tabpanel"
         aria-labelledby="graphics-tab">...</section>
```

```csharp
TabList tabs = view.Get<TabList>("settings-tabs");
tabs.SelectedIndex = 0;
tabs.SelectionChanged += OnTabChanged;
```

框架负责输入导航、`aria-selected`、`hidden` 和 ID 关系同步。打包器验证 role 组合与 `aria-controls/aria-labelledby/for`。普通运行时状态可变，`role` 不可变。

## 13. 文本与 Inline Formatting

删除 `display:block` RichText desugar 和特殊公共 `RichText` 类型。富文本就是正常 HTML 子树：

```html
<p id="description">
    对敌人造成 <strong id="damage">120</strong> 点伤害
    <img src="fire.png" alt="火焰">
    <a id="details" href="skill://fireball">详情</a>
</p>
```

公共对象树保留 `TextNode/TextElement/Image/Link` 的 ID、样式和事件；内部文本布局将最近 Inline Formatting Context 编译成 TextRun、ImageRun 和 LinkRun，用于统一换行、baseline、测量与几何构建。

- 裸文本形成叶子 `TextNode`。
- inline 元素是语义容器。
- `p/h1-h6` 建立文本 block。
- `TextContent` 与 DOM 一样，用纯文本替换当前全部子内容。
- 修改 inline 子树只使最近文本上下文失效。

公共语义树与内部布局/渲染树可以不同。

## 14. 组件、模板与复用

每个独立 HTML 资产都编译为不可变 `UITemplate`。界面、弹窗、业务组件和列表项只是模板被使用时扮演的角色。

### 14.1 内联模板

```html
<ul id="mails">
    <template id="normal-mail">
        <li class="mail"><span id="title"></span></li>
    </template>
</ul>
```

内联 `<template>` 只属于当前组件：

```csharp
UITemplate normal = view.GetTemplate("normal-mail");
```

### 14.2 包级共享模板

独立 `templates/mail-item.html` 可被多个界面和多个 ListView 引用：

```csharp
UITemplate item = common.GetTemplate("templates/mail-item");
// 生成后：CommonUI.Templates.MailItem
```

模板资产只编译、缓存一份；每次实例化生成独立对象树、状态、事件和 ID 作用域。资源路径与 CSS 以模板所属 Package 为作用域。

### 14.3 用户业务 Custom Elements

框架基础能力不得发明自定义标签。只有 HTML 没有对应概念的用户业务组件，才使用标准 Web Components 约定：

```html
<game-item-card id="sword" rarity="legendary">
    <button slot="action">装备</button>
</game-item-card>
```

- 名称必须包含 `-`。
- Package 注册表承担 `customElements.define()` 的角色。
- 标准 `<slot>` 提供内容投影。
- 未注册元素、无效 slot 或重复 slot 在打包期报错。
- 这不允许框架用 `<scroll-view>` 等元素复制已有标准 HTML/CSS 能力。

### 14.4 组件样式边界

组件实例采用 Shadow DOM 风格隔离：

- 模板内部选择器只作用于模板内部。
- 父组件普通选择器不穿透边界。
- 标准可继承属性和 CSS 自定义属性 `--*` 跨边界传递。
- 后续可用标准 `part/::part()` 精确开放内部样式。

## 15. ListView

声明使用标准 `ul/ol/li/template`，不使用列表专用 DSL：

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
mails.ItemTemplate = normal;
mails.TemplateSelector = index => data[index].HasReward ? reward : normal;
mails.BindItem = (item, index) => {
    item.Get<TextElement>("title").TextContent = data[index].Title;
};
```

契约：

- `ul/ol` 稳定映射为 `ListView`，`li` 映射为 `ListItem`。
- 静态模式允许直接 `li`；数据驱动模式只允许 template，不允许与直接 `li` 混用。
- `ItemTemplate` 与 `TemplateSelector` 首版同时提供。
- `TemplateSelector` 返回 `UITemplate` 对象，不返回字符串。
- 没有 `TemplateSelector` 时必须设置 `ItemTemplate`；两者并存时 Selector 返回的模板优先，返回空则使用 `ItemTemplate`。
- ListView 按模板分别池化。
- item 模板必须恰有一个 `li` 根，打包期验证。
- 虚拟化、可见区、测量补偿、content size 和后端 reuse key 全部是内部实现。

刷新 API：

```csharp
mails.RefreshItem(index);
mails.RefreshItems();
mails.NotifyInserted(index, count);
mails.NotifyRemoved(index, count);
mails.NotifyMoved(from, to);
```

`Refresh*` 用于数据内容变化；`Notify*` 用于非 observable 数据源发生结构变化并让 ListView 保留滚动位置和最小化重绑。后续 Observable 数据源可自动转发这些通知。

## 16. 事件

控件优先提供语义事件：

```csharp
button.Clicked += OnStart;
slider.ValueChanged += OnVolumeChanged;
```

所有节点同时提供类型化路由事件：

```csharp
node.On<PointerDownEvent>(OnPointerDown);
```

- 事件支持捕获、目标和冒泡。
- `Target` 与 `CurrentTarget` 都是公共 `Node`。
- 节点 `Dispose()` 自动清理其订阅。
- `RemoveFromParent()` 不清理订阅。
- 内部后端事件不得泄漏 NodeId 或 FFI 结构。

## 17. 错误与验证

### 17.1 打包期

以下情况拒绝打包，并报告文件、行列、原值和修改建议：

- 围栏外标签、属性、CSS 属性或属性值。
- 不支持的 `input[type]` 或 ARIA role。
- 重复 ID、损坏的 ID/ARIA/label 引用。
- List item template 根不是 `li`。
- 未注册 Custom Element、无效 slot。
- 不可兑现的后端能力。

禁止未知 CSS 静默忽略、`display:grid` 降级 Flex 等行为。

### 17.2 运行时

- 结构契约错误：`UIContractException`。
- 已销毁对象：`ObjectDisposedException`。
- raw style 解析错误：`UIStyleException`。
- `TryGet`、空 `Query` 和 DOM 约定的非滚动态滚动不视为错误。

## 18. 围栏单一真相源与测试

标签、属性、结构属性、CSS 值、运行时类型和后端需求由一份机器可读 schema 驱动。解析器、打包器、绑定生成器、文档和测试不得各维护一份白名单。

验证分层：

1. Fence contract：每个允许项有正例，每类围栏外输入有反例。
2. Package tests：模板、组件边界、ID、ARIA、slot、资源作用域。
3. 公共 API tests：身份、查询、生命周期、事件、控件状态和错误。
4. ListView tests：多模板池化、插删移动、等高/不等高、滚动锚定。
5. Chromium 差分：围栏内 HTML/CSS 的关键矩形、换行和状态样式做容差比较。
6. Backend contract：所有后端消费同一语义 frame model。
7. Showcase：只使用真实公共 API，不允许手写虚拟化、全局 NodeId 查询或预览 polyfill。

## 19. Core、FFI 与兼容策略

旧 core/FFI 不是约束。实现可重构 Scene、Package 格式、事件协议、布局策略、文本布局、FFI 和 Unity 镜像层，以兑现公共契约。

- 旧 `LoomStage`、公开 NodeId、哨兵和中央 EventHandler 退出用户表面。
- 不要求用 facade 把旧结构永久包起来。
- 后端接入接口保留为内部 SPI，不作为游戏业务 API 发布。
- 包格式允许破坏性升级并重打现有资产。
- 迁移 showcase 是验收工作，不是新设计向旧 driver 妥协的理由。

## 20. 北极星示例

```html
<main id="home" class="screen">
    <button id="start">开始游戏</button>

    <label for="volume">音量</label>
    <input id="volume" type="range" min="0" max="100" value="80">

    <ul id="mails">
        <template id="mail-item">
            <li class="mail"><span id="title"></span></li>
        </template>
    </ul>
</main>
```

```csharp
Component home = game.Instantiate("views/home");
ui.Root.AddChild(home);

home.Get<Button>("start").Clicked += StartGame;

Slider volume = home.Get<Slider>("volume");
volume.ValueChanged += value => audio.Volume = value / 100f;

ListView mails = home.Get<ListView>("mails");
mails.ItemTemplate = home.GetTemplate("mail-item");
mails.ItemCount = model.Mails.Count;
mails.BindItem = (item, index) =>
    item.Get<TextElement>("title").TextContent = model.Mails[index].Title;
```

这里没有整数句柄、哨兵、全局查找、私有 Widget DSL、手工 reuse key 或后端时序。

## 21. 已锁定决策

1. 唯一公共目标用户是游戏业务程序员。
2. 公共 API 优先，允许纵向重构 core/FFI。
3. 类型化对象树，并为生成强类型 View 留路。
4. 容器与叶子分离；叶子不暴露子节点操作。
5. `RemoveFromParent` 可重挂，`Dispose` 才销毁。
6. 语义事件 + 类型化路由事件。
7. typed Style + class 状态 + raw string 逃生口。
8. 项目 class 生成 `StyleClass` token，不是固定 enum。
9. 显式 `UIContext`，支持多上下文。
10. `Get/TryGet/Query` 使用标准 ID 和组件实例作用域。
11. 新围栏恢复标准 Block/Flex/Inline；CSS 行为不改变对象类型。
12. 滚动是 Container 能力，不是 ScrollView 类型。
13. 原生控件使用标准元素；复合控件使用白名单 WAI-ARIA。
14. 框架不造 Custom Element；用户业务组件可使用标准 Web Components/Slot。
15. 富文本是正常 HTML 语义树，内部扁平化为 runs。
16. 内联模板私有，独立 HTML 模板包级共享。
17. ListView 使用 `ul/ol/li/template`，首版支持 TemplateSelector。
18. 组件采用 Shadow DOM 风格样式边界。
19. 围栏外输入明确失败，不静默降级。
20. Showcase 是公共契约的端到端验收。

## 22. 后续阶段

本 spec 先实现手写类型化对象 API。结构稳定后再做：

- 强类型 View、Template 和 StyleClass 代码生成。
- Observable 数据源自动映射 ListView 增量通知。
- 标准 `part/::part()` 的跨组件精确样式开放。

这些后续能力的扩展点已在当前契约中保留，不要求当前用户 API 返工。
