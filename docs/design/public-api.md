# Ikat 公共 API 权威契约

> **单一真相源**：`unity/package/Runtime/Public/Ikat.*.cs`（冻结的 C# 签名，machine-readable）。本文档为人类可读契约，以签名文件为准。
>
> **防漂移门**：`tests/dotnet/Ikat.PublicApi` 编译校验——改公共签名后必编过。
>
> **定位**：面向游戏业务程序员的终态公共 API。自上而下设计，公共契约优先，core/FFI/后端为它服务。摸黑+三束的实现完成后，用本契约作验收靶子。实现机制（真身 Rust + C# 投影）见 [projection-layer.md](projection-layer.md)。

---

## 1. 设计哲学

### 1.1 核心赌注

**AI 读 HTML/CSS 能正确预测渲染结果。** 标准 HTML 作设计期 DSL，让 AI 既能编辑（文本）又能预测渲染（AI 对 HTML/CSS 有强先验）。动画定义也全在 CSS，与此赌注自洽。

所有 API 决策的第一判据：贴合 AI 的 HTML/DOM/CSS 先验。第二判据：贴合已验证的框架（FairyGUI/RmlUi/UI Toolkit）的成熟机制。

### 1.2 四条设计原则

- **积木优先**：框架只提供原子能力和底层原语；窗口系统、弹窗编排、拖放、RadioGroup 等逻辑层模式由用户用积木搭建。
- **CSS 优先**：布局、样式、动画的定义全在 CSS。C# 只负责触发、反应和数据注入。
- **类型化对象树**：稳定 HTML 语义决定对象类型；CSS 赋予行为能力，不改变类型。
- **引擎中立**：公共 API 不绑定任何引擎。Unity/Godot/UE 通过消费同一套语义层接入。引擎特定的东西（tick、输入采集、渲染目标、纹理注册）不进公共层，归引擎集成层。

### 1.3 定位模型

CSS-flow 布局中心。Node 三分模型：
- **Style**（可写/布局层）：尺寸、定位、flex、颜色。改它下帧 solve。
- **Transform**（可写/渲染层）：视觉偏移、缩放、旋转。改它不触发 solve，只刷命中几何 + world matrix。
- **Geometry**（只读/布局产物）：布局算出的 rect，读最近一次 solve 结果。

运行时移动走 Transform；尺寸/定位走 Style；读实际位置走 Geometry。

### 1.4 失败策略

- 打包期：围栏外输入明确报错，不静默降级（见 [fence.md](fence.md)）。
- 运行时：`UIContractException`（契约违反）/ `ObjectDisposedException`（操作已销毁节点）/ `UIStyleException`（运行时 CSS 解析失败）/ `UIPackageException`（包加载失败）。

---

## 2. 对象层级

按子树归属分容器与叶子（Container = 子树是用户内容；Node 叶子 = 子树是控件构造，见 §2.1）：

```text
Node
+-- Container（子树 = 用户内容，运行时可编排）
|   +-- AbsolutePanel（语法糖，子节点自动 absolute）
|   +-- TextElement（span）
|   +-- Link（a，富文本内链接）
|   +-- Button（button）
|   +-- ListView（role=list）/ ListItem（role=listitem）
|   +-- OptionItem（role=option，从属 Dropdown）
|   +-- Slot / CustomElement
+-- TextNode / Image                          （叶子：内容 / 绘制）
+-- 控件（叶子：子树 = 控件构造，公共 API 不暴露编排）
    +-- TextField（role=textbox）/ TextArea（role=textbox + aria-multiline）
    +-- NumberField（role=spinbutton）
    +-- Slider（role=slider）/ ProgressBar（role=progressbar）
    +-- Toggle（role=switch）/ RadioButton（role=radio）
    +-- Dropdown（role=combobox）
```

### 2.1 容器 vs 叶子的划线（不变量）

这条线 = **子树归属**（不是 HTML content model——role 化重构后所有控件的子节点都是作者写的，content model 不再区分）：

- **Container 子类 = 子树是「用户内容」的节点**：`<div>`、`<button>`、`<span>`、`role=listitem`、`role=option`。子树设计期写、运行时可增删。
- **Node 叶子 = 子树是「控件构造」的节点**：`role=slider`/`progressbar`/`combobox`/`textbox`/`spinbutton`/`switch`/`radio` + `<img>`。视觉上可能有层次（slider 的 thumb、progressbar 的 fill），那是控件构造，框架管理，公共 API 只给语义属性。

控件保持 `: Node` 叶子（不改 `: Container`、不引入 `Control` 中间层）：role 化改的是**谁写结构**（框架注入 → 作者写），没改**谁管结构**（仍是框架）。运行时让用户 `slider.Children.Remove(thumb)` 无合理用例，只会制造半残控件。`OptionItem`/`ListItem` 是 Container（装用户内容），其从属关系（服务于父控件）在文档说明，不做类型层次强化。

### 2.2 稳定语义签名

节点类型由稳定 HTML 语义签名决定：base 标签按 tag；控件/列表按 WAI-ARIA `role` + `aria-*`（如 `role=slider`、`role=textbox + aria-multiline`）。CSS（class、伪类、computed style）永远不改变 C# 对象类型。

### 2.3 无 Panel 类型

作用域不是类型，是运行时标记。模板实例化根 / 显式作用域根打 `IsScopeRoot` 标记；`Get<T>` 的边界判定看此标记，不看具体类型。`Instantiate` 返回模板根的**真实类型**（围栏限定模板根为容器类）。

---

## 3. Node 基础层

```csharp
public abstract class Node {
    public UIContext Context { get; }
    public string Id { get; }
    public Container Parent { get; }          // Root.Parent == null

    public NodeStyle Style { get; }            // 可写，inline override 层
    public NodeTransform Transform { get; }    // 可写，渲染层
    public NodeGeometry Geometry { get; }      // 只读，布局产物

    public bool Touchable { get; set; }
    public bool Focusable { get; set; }        // 运行时改可获焦性（对齐 fgui focusable）
    public ClassList Classes { get; }

    public bool IsDisposed { get; }
    public void RemoveFromParent();            // 可重挂，不清订阅
    public void Dispose();                     // 递归永久销毁，清订阅

    public T Get<T>(string id) where T : Node;              // 作用域内查找，抛 UIContractException
    public bool TryGet<T>(string id, out T node) where T : Node;
    public IReadOnlyList<T> Query<T>() where T : Node;       // 按类型，文档序
    public IReadOnlyList<Node> Query(string selector);      // ".class" / "tag.class"，文档序

    public AnimationHandle Play(string name);
    public AnimationHandle Play(string name, float durationSeconds); // 无声明 keyframes 默认 1s，此重载覆盖
    public void Focus();
    public void Blur();
    public void SetPointerCapture(int touchId);  // DOM setPointerCapture 平价；Up 自动释放
    public void CancelClick(int touchId);        // 取消该指针待决 Click（长按后不点击等）

    public IDisposable OnUpdate(Action<float> cb);          // 逻辑驱动每帧更新钩子
    public EventRegistration On<T>(Action<T> handler, bool useCapture = false, bool once = false) where T : IRouteEvent;
}
```

**不变量**：
- `Get<T>`/`Query` 在当前作用域内查找，不穿透嵌套作用域根（Panel/List item 边界）。
  - **L3 已完整**：查找 DFS 遇 `LOOKUP_SCOPE` 子节点（实例根 / 组件展开域 host / List slot 根）检查其自身 id 后不再下钻——`component.Get<T>` 不会穿透 list item（含 parked）或嵌套组件实例。作用域根自身可被外层命中（同 Shadow DOM：host 在 light tree）。访问嵌套作用域内部：先 Get 作用域根，再在其上 Get。
  - **虚拟化 `<ul>` 禁止 `:nth-child`**：parked slot 留挂 ul 子树按 CSS 仍计 child count → `:nth-child` 序数不可控。用 item-index / `data-*` 属性替代。
- `Query<T>()` / `Query(selector)` 返回**文档序且稳定**（前序遍历）——逻辑层按序聚合（如自建 RadioGroup）依赖此。
- 已销毁节点上的操作抛 `ObjectDisposedException`。
- `OnUpdate`/`On<T>` 订阅随 `Dispose` 自动清理；`RemoveFromParent` 不清理。
- `OnUpdate` 是「逻辑驱动的每帧更新钩子」（数据插值、跟随、状态响应），非命令式动画系统。预定义视觉动画走 CSS/`Play`。

### 3.1 Style（可写，inline override 层）

```csharp
public sealed class NodeStyle {
    public Length Width/Height/MinWidth/MaxWidth/MinHeight/MaxHeight { get; set; }
    public DisplayMode Display { get; set; }         // Block | Flex | None
    public FlexDirection FlexDirection { get; set; }
    public FlexWrap/JustifyContent/AlignItems { get; set; }
    public Length Gap { get; set; }
    public Thickness Padding/Margin/BorderWidth { get; set; }
    public Overflow OverflowX/OverflowY { get; set; }
    public Length Left/Top/Right/Bottom { get; set; }
    public PositionMode Position { get; set; }
    public int ZIndex { get; set; }                   // 兄弟层叠序（CSS z-index）：绘制/命中层，不改 flex 排列
    public IkatColor BackgroundColor/TextColor { get; set; }   // TextColor = 文字色（CSS color 通道；旧名 IkatColor 已 Obsolete）
    public float Opacity { get; set; }
    public void SetVar(string name, Length/IkatColor/float/string value);
    public void RemoveVar(string name);
}
```

**不变量（inline override 层语义）**：
- Style 是**最高优先级的 inline override 层**，不是 cascade 的读取窗口。
- getter **只反映 C# setter 写过的属性**；未写过的返回 `Unset` 哨兵（`Length.Unset()` / `IkatColor.Unset` / enum 的 `Unset` 成员）。布局产物（rect/matrix）走 `Geometry`；computed 样式值（颜色/字号等）走只读 computed style 查询接口（时效：rematch 后有效、本帧 tick 后反映最新 cascade）。
- setter 写 `Unset` = 撤销该属性的 inline override，回落 CSS cascade。单属性撤销即用 `Style.X = Unset()`，无 `Clear`/`Reset`。
- `SetVar`/`RemoveVar` 管 CSS 自定义属性 `--*`；`--*` 跨作用域根传递。不提供 `GetVar`（var 不当状态存储读回）。
- 隐藏节点用 `Display = None`（不占位、不渲染、不命中，等同 fgui `visible=false`）；占位隐藏（保留布局空间）用 `Opacity = 0`。（`Visibility` API 已移除——fence CSS 子集无 `visibility` prop，无后盾；占位隐藏 `opacity:0` 覆盖。）

### 3.2 Transform（可写，渲染层，不触发 solve）

```csharp
public sealed class NodeTransform {
    public IkatVector2 Position { get; set; }   // 视觉偏移
    public IkatVector2 Scale { get; set; }
    public float Rotation { get; set; }      // 弧度
    public IkatVector2 Origin { get; set; }
}
```

最终渲染位置 = 布局位置 + Transform.Position。改 Transform 不触发 solve。

### 3.3 Geometry（只读，布局产物）

```csharp
public readonly struct NodeGeometry {
    public IkatRect LayoutRect { get; }          // 父坐标系
    public IkatRect WorldRect { get; }           // 全局坐标系
    public IkatVector2 LocalToGlobal(IkatVector2 point);
    public IkatVector2 GlobalToLocal(IkatVector2 point);
    public IkatRect LocalToGlobal(IkatRect rect);
    public IkatRect GlobalToLocal(IkatRect rect);
}
```

**不变量**：Geometry 反映**最近一次完成的 solve** 的结果——不含本帧尚未 flush 的 Style 改动（滞后一帧，同 web reflow）。改 Style 后要读新布局结果，须等下一帧。不提供强制同步 solve。

---

## 4. Container 与树操作

```csharp
public class Container : Node {
    public int ChildCount { get; }
    public IReadOnlyList<Node> Children { get; }
    public string TextContent { get; set; }             // DOM 语义：读=拼接后代文字；写=清子节点换单文本
    public T AddChild<T>(T child) where T : Node;
    public T InsertChild<T>(T child, int index) where T : Node;
    public void RemoveChild(Node child);
    public Node GetChildAt(int index);
    public int GetChildIndex(Node child);
    public void SetChildIndex(Node child, int index);
    public void SwapChildren(Node a, Node b);
    public void SwapChildrenAt(int indexA, int indexB);
    public IkatVector2 ScrollPos { get; }   // 滚动容器当前滚动位置（非滚动容器返 (0,0)）；与 ScrollTo 成对
    public void RestartAnimations();     // 重启子树内声明式（class 触发）keyframes：player 原地重建，节点状态全保留；node.Play 程序化 player 不受影响
    public void ScrollTo(IkatVector2 pos, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public event Action<ScrollChangedEvent> Scrolled;
    public UITemplate GetTemplate(string name);          // 取内联 template
}
```

### 4.1 TextContent

对应 DOM `Element.textContent`（web 标准）。**写会清空所有子节点**换成单个文本——若容器内混有元素子节点（如 `<button>` 里的 `<img>`），写 `TextContent` 会一并清掉。要精细控制：把动态文本隔离进带 id 的行内元素，改那个元素：

实现注记：现有直系子恰为单个 TextNode 时走快路径**就地 set_text**（不清子重建，TextNode 句柄保持）——高频改写（OnUpdate 读数刷新）安全，不消耗 slotmap generation；其余形态按上述清子重建语义（真释放）。

```csharp
buy.Get<TextElement>("price").TextContent = "200";   // 只动 span，兄弟 img 不受影响
```

### 4.2 AbsolutePanel

自身 `position: relative`，`AddChild` 自动施加 `position: absolute` 到子节点。API 与 Container 一致。

### 4.3 生命周期

`RemoveFromParent` 可重挂，不清订阅；`Dispose` 递归销毁、清订阅、不可复用。游离节点（RemoveFromParent 后）由用户持引用并负责最终 Dispose，框架不引用计数、不追踪游离节点。

| 手段 | 效果 | 用途 |
|---|---|---|
| `Style.Display = None` | 隐藏 + 不占位 + 不渲染 + 不命中 | 日常开关（窗口/面板反复显隐） |
| `Style.Opacity = 0` | 隐藏 + 占位（加 `pointer-events:none` 不命中） | 占位隐藏（防布局跳动） |
| `Dispose()` | 永久销毁、释放 | 这个 UI 这辈子不再要了 |

频繁开关的窗口用 `Display = None`，不用 Dispose。

---

## 5. 事件与注册

### 5.1 两条路径

- **语义事件**（C# event）：`button.Clicked += handler`。便捷糖，只关心「发生了」。
- **路由事件**（`On<T>`）：`node.On<PointerDownEvent>(handler, useCapture: true)`。要坐标/冒泡控制/捕获。

语义事件是路由事件的糖（同源实现：`Clicked` 内部 = `On<ClickEvent>` 冒泡到自身），行为一致。

### 5.2 路由模型

DOM 三阶段：捕获 → 目标 → 冒泡。

```csharp
public interface IRouteEvent {
    Node Target { get; }
    Node CurrentTarget { get; }
    bool DefaultPrevented { get; }
    bool PropagationStopped { get; }
    bool ImmediatePropagationStopped { get; }
    void StopPropagation();
    void StopImmediatePropagation();   // 同节点剩余 handler 全跳 + 止后续传播（DOM 平价）
    void PreventDefault();
}
```

### 5.3 事件清单

| 类别 | 事件 |
|---|---|
| 指针 | PointerDown/Up/Move/Enter/Leave, Click, LongPress |
| 拖拽 | DragStart/Move/End |
| 键盘 | KeyDown/Up |
| 焦点 | Focus/Blur |
| 滚动 | ScrollChanged |
| 动画 | AnimationStart/End/Iteration, TransitionEnd |

指针键用 `PointerButton` 枚举（Left/Right/Middle）。

**路由语义细节**：capture 阶段不检 `StopPropagation`（root→target 全程跑）、bubble 前预检；target 节点在 capture 末尾与 bubble 开头各收一次。`Enter/Leave` 不冒泡——按祖先链 diff 逐节点直派（从父进子父不 Leave，对齐 CSS `:hover` 祖先语义）。`Click` 目标 = down 时的叶子节点（光标阈内漂移仍点中按下叶）；双击无独立事件——`ClickEvent.ClickCount` 在 1↔2 间循环（同位置+同键+时间窗）。`LongPress` 按住 ≥1.5s 触发、与 `Click` 独立——长按后松手不要点击的业务在 handler 里调 `CancelClick(evt.TouchId)`。`PointerMove` 需先 `SetPointerCapture`（monitor 机制）才有事件流。

### 5.4 退订

退订收敛为两类：
- 路由 / 每帧 / 样式表 = `IDisposable` 句柄（`On<T>` 返回 `EventRegistration`，`OnUpdate` 返回 `IDisposable`，`StyleSheet.Add` 返回 `IDisposable`），Dispose 撤销。
- 语义事件 = C# `+=` / `-=`。

`On<T>` 的 `once: true` = 触发一次自动退订（防「等一个结束事件」泄漏，如等 `AnimationEndEvent` 后 Dispose）。

### 5.5 全局查询

```csharp
bool hit = ui.IsPointerOnUI;
Node node = ui.Pick(globalPoint);   // 命中测试：返回该点最上层可命中节点（drop 逻辑靠它 + 积木搭）
```

---

## 6. 拖拽与焦点

### 6.1 拖拽

注册拖拽事件即参与仲裁，无 `Draggable` 属性。框架管阈值/捕获/仲裁，用户施加 delta。拖放是逻辑层模式，不提供——用 `DragEnd` + `ui.Pick(position)` 找 drop target，drop 逻辑用户自己搭。

`DragMoveEvent.DeltaX/DeltaY` 是**逐 Move 增量**（自上一条 DragMove；首条含阈值前行程——累加后元素精确贴指针）。累计偏移不进载荷：`DragStartEvent.StartPosition` + `DragMoveEvent.Position` 可推导。**路由指引**：视口平移/滚动（大内容小视口）用 `overflow:auto` 滚动容器（拖拽/滚轮/惯性/钳制/点击取消全自带，零拖拽数学）；Drag 事件 API 是对象拖拽（标题栏、拖道具入格）的低层积木。

### 6.2 焦点

`Focus()` / `Blur()` / `ui.FocusedNode`。每 UIContext 一个焦点。设计期 `tabindex` 声明可获焦性，运行时用 `Node.Focusable` 改。**Tab/Shift+Tab 自动焦点链导航内置**（tabindex 正整数升序先于 0 组、DOM 序、链尾 wrap；Tab 被导航消费、不发 keydown）——**方向键/手柄导航**才是逻辑层积木（`On<KeyDown>` + `Focus()`）。pointer-down 命中可聚焦节点自动聚焦、点不可聚焦区域清焦点（对齐 DOM 点空白 blur）；编程 `Focus()` 是强制语义（不查 tabindex，仅 disabled 拒）；`FocusIn/FocusOut` 只发焦点节点本身、不沿祖先链。

---

## 7. 标准控件

| HTML（role） | 类型 | 主要 API |
|---|---|---|
| button | Button : Container | Disabled, Clicked（文本走 Container.TextContent） |
| div role=textbox | TextField : Node | Value, Placeholder, Selection, MaxLength, ReadOnly, Disabled, ValueChanged, Submitted |
| div role=textbox aria-multiline=true | TextArea : Node | Value, Placeholder, Selection, MaxLength, ReadOnly, Disabled, ValueChanged |
| div role=spinbutton | NumberField : Node | Value, Min, Max, Step（float）, Disabled, ValueChanged |
| div role=slider | Slider : Node | Value, Min, Max, Step（float）, Disabled, ValueChanged, ChangeCommitted |
| div role=switch | Toggle : Node | IsChecked, Disabled, CheckedChanged |
| div role=radio | RadioButton : Node | IsChecked, Name（只读）, Disabled, CheckedChanged |
| div role=combobox | Dropdown : Node | SelectedIndex, SelectedValue, Disabled, SelectionChanged（open 时方向键移高亮**不提交**、跳过 disabled 项；Enter 与展开时刻快照**净变才发**事件——点已选项不发；Esc/外部点击/header 再点 = 取消回滚、不发事件） |
| div role=progressbar | ProgressBar : Node | Value, Max（float，0 基底）, IsIndeterminate |
| div role=option | OptionItem : Container | Value, Selected（只读）, Disabled, Index（只读，父 Dropdown 内序号） |
| div role=tablist | TabList : Container | SelectedIndex, SelectionChanged（方向键/click 切换；方向轴按 tablist 的 `flex-direction` 选轴、`*-reverse` 翻转方向、clamp 不 wrap；panel 靠 `aria-controls` 关联） |
| div role=tab | Tab : Container | （`aria-selected` 由父 TabList.SelectedIndex 跨节点合成，非字面存储） |
| a（富文本内） | Link : Container | Href（只读）, Clicked（复用既有语义事件） |

**不变量**：
- 控件数值（Slider/NumberField/ProgressBar 的 Value/Min/Max/Step）用 `float`，与几何/引擎统一。大数精度需求归业务层。
- RadioButton 同 `Name` 组框架自动互斥；只有新选中项触发 `CheckedChanged`（对齐 web，不触发被取消项）。RadioGroup（按 name 聚合、读选中 index）是逻辑层积木，不进公共层。
- 通用事件类型：`ValueChangedEvent<T>`, `SelectionChangedEvent`, `TextSelection`。

控件与列表的类型由 `role` 分派（见 [fence.md](fence.md) §2.3、§3.1）；控件视觉部件用 `data-slot`（如 slider 的 `data-slot=thumb`、progressbar 的 `data-slot=fill`）。WAI-ARIA 复合控件沿用同一 `role` 机制：**TabList/Tab 已落地**（见上表）；Tree 等按需单独立项。

### 7.1 Link（`<a>`，富文本内链接）

`<a>` 是 rich-text-block 上下文里的行内链接，投影为 `Link : Container`（含 `TextContent`，子只许文本/嵌 span）。契约：

- **仅富文本上下文**：`<a>` 只在 rich-text-block 内合法，围栏在打包期拦截非法用法（错误码见 [fence.md](fence.md)）。
- **href 是 opaque 标识符**：框架不解析、不 OpenURL，`Href` getter 原样回传，由游戏自解释（路由到界面/商店/任务等）。`Href` 只读——打包期从 href 属性烙印，运行时不可改。
- **点击 = 既有 `Clicked`**：指针命中细化到 a 节点（含嵌套 span 内文字），订阅走与 Button 同款的语义事件；无独立 OnLinkClicked 聚合。
- **UA 默认样式**：蓝（#0000EE）+ 下划线，作者 CSS 可覆盖（含 hover 等伪类）。
- **键盘激活 deferred**：键盘聚焦/Enter 激活归键盘导航项，本阶段不做。

---

## 8. ListView

```csharp
public class ListView : Container {
    public int ItemCount { get; set; }
    public UITemplate ItemTemplate { get; set; }
    public Func<int, UITemplate> TemplateSelector { get; set; }
    public Action<ListItem, int> BindItem { get; set; }
    public void ScrollToItem(int index, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public void RefreshItem(int index);
    public void RefreshItems();
    public void NotifyInserted(int index, int count = 1);
    public void NotifyRemoved(int index, int count = 1);
    public void NotifyMoved(int fromIndex, int toIndex);
    public string ItemExitClass { get; set; }
}
```

**契约**：`role=list → ListView`，`role=listitem → ListItem`。布局走 CSS。虚拟化全内部。已知限制：bind 滞后一帧——新进可见区的 item 第一帧显示模板原样/上一复用者内容，快速滚动会出现一帧旧内容（接受的代价）。

**静态 vs 数据驱动（运行时隐式锁定，强制互斥）**：
- 虚拟化是**运行时实现决策**（程序员按数据规模定），不进 HTML——它不改变渲染结果，不属 AI 可预测性范围。
- 首次设 `ItemCount`/`ItemTemplate`/`BindItem` 任一 → 进入数据驱动模式：清空设计期 listitem（预览占位）、虚拟化接管。此后 `AddChild`/`InsertChild`/`RemoveChild` 抛 `UIContractException`；`ChildCount` = `ItemCount`；`Children` 抛 `UIContractException`（虚拟化下无法返回全部实例化节点）。
- 不碰数据驱动属性 → 静态模式：li 是真内容，走容器 API。

**item 模板来源（优先级从高到低）**：
1. 运行时显式赋 `ItemTemplate`（单模板）或 `TemplateSelector`（多模板）——程序员完全控制。
2. 设计期 `<template id>` 声明（[fence.md](fence.md) 允许 `role=list` 含 `template`，打包期校验 template 根是 `role=listitem`）。多模板时配合 `TemplateSelector`：用户 `view.GetTemplate("name")` 取出 `UITemplate` 后塞进 lambda 闭包按 index 选（`TemplateSelector` 是纯 `Func<int, UITemplate>`，框架不自动收集 list 下 template）。
3. 都没有 → 设计期第一个 li 结构兜底当模板（instantiate 时先捕获再清空）。

**自动/报错规则**：未设 `ItemTemplate`/`TemplateSelector` 时——list 下有**单个** `<template id>` → 自动用它；有**多个** `<template id>` → 抛 `UIContractException`（有多个模板却没说怎么选）。`<template>` 与运行时 virtual 开关不冲突：virtual 管是否虚拟化，`<template>` 管 item 模板长什么样。

**退场动画**：`ItemExitClass` 设定后，`NotifyRemoved` 的 item 先加 class 等 `AnimationEnd` 再回收。

---

## 9. 动画

视觉定义全在 CSS（keyframes/transition）；运行时另提供一条**程序化 tween 通道**（TweenBuilder，#9）——服务 CSS keyframes 表达不了的逻辑驱动演出（跟随数值、动态目标、连续往复的 UI 反馈）。

### 9.1 三种触发

1. class 切换（声明式）：`node.Classes.Add("slide-out")`，结束用 `On<AnimationEndEvent>`。
2. `node.Play("name")` 返回 `AnimationHandle` 句柄（程序化，带 hook）。
3. `Style.SetVar`（动态值逃生舱）。

**时序不变量**：class/typed style 变更在**下一帧 tick 的 rematch 生效**（不即时 rematch），一帧内多次增删只看帧末最终 class 集合。transition 基线 = 上一帧该属性的 computed 值。

`Play`（触发 2）与 class 切换（触发 1）分工：Play 用于「程序化、要句柄控制」；class 用于「声明式、只需知结束」。class 触发不产 `AnimationHandle` 句柄，结束统一走 `AnimationEndEvent`。

**`Play` 的接管语义**：同节点再次 `Play` 时，同名旧动画确定性从头重播；旧动画不同名但动了相同通道（如都是 transform）也被新 `Play` 取代（不限旧动画是否播完）；通道不相交的旧动画（如 transform + opacity）共存继续播。即 `Play` 接管它所动的一切通道，不存在两个写同通道的动画叠加。

**`:nth-child(An+B|odd|even|N)` selector**（M2）配合 `animation-delay` 实现错峰入场——同一规则按子序号算 delay，常用于导航卡/列表项依次淡入（showcase `home.html` 7 条 `.nav-card:nth-child(N){animation-delay:...}`）。

### 9.2 TweenBuilder（程序化 tween，#9）

```csharp
node.Tween(TweenChannel.Opacity)      // 通道：Opacity/Translate/Scale/Rotation/BgColor/TextColor/Transform
    .From(0f).To(1f)                  // 分量数按通道（Transform=TRS 五元组 [tx,ty,sx,sy,rotRad]，恒 px/弧度）
    .Duration(0.3f).Delay(0.1f)
    .Ease(EaseKind.CubicOut)          // keyword 族；精确 CSS ease 用 EaseBezier(.25f,.1f,.25f,1f)
    .Repeat(2, yoyo: true)            // 额外重播次数 + 奇数轮反向（CSS alternate 同义）
    .Tag(7)                           // complete 事件路由键（可省，OnComplete 时自动分配）
    .OnComplete(n => ...)             // 一次性：全部轮次跑满触发一次，触发即注销
    .Start();
```

**语义不变量**：
- **replace-override**：与 CSS transition 同通道互踩——新 tween 覆写同节点同通道的旧值；kill 保留末值（`KillTween(channel)`）。
- **单一动画时钟**：TweenManager 与 keyframes player 同帧推进（tween 先写、player 后写——player 同通道覆盖 tween）。
- **OnComplete 走事件通道**：core TweenComplete 事件按 tag 路由（与 TransitionEnd 同源事件；tag 未注册的 TweenComplete 仍是 transition 旧路径，两路径互不干扰）。
- **缺省 ease = CSS `ease`**（精确 bezier(0.25,0.1,0.25,1)，与 CSS/fence 侧同一真值）。
- 值域约束：`EaseBezier` 的 x1/x2 ∈[0,1]（y 可越界表 overshoot），越界抛 `UIContractException`。

### 9.3 AnimationHandle 句柄

```csharp
public sealed class AnimationHandle {
    public string Name { get; }
    public bool IsPlaying { get; }
    public float Time { get; set; }
    public void Pause(); public void Resume(); public void Stop();
    public AnimationHandle OnStart(Action cb);
    public AnimationHandle OnEnd(Action cb);
    public AnimationHandle OnKey(float percent, Action cb);
    public AnimationHandle OnHook(string name, Action cb);
}
```

**生命周期不变量**：AnimationHandle 句柄非长期对象，生命周期 = 那次播放。播放结束句柄失效、hook 自动释放（循环动画 `Stop()` 时释放）。

**事件双路由**（M2）：core `player.update` 检测阈值后 emit EventRecord，`borrow_events` 到 C# 双路由——

| typed 事件 | 句柄回调 | 触发 |
|---|---|---|
| `AnimationStartEvent` | `OnStart` | 播放开始（首个非 delay 帧）|
| `AnimationEndEvent` | `OnEnd` | 完成（最后一次 iteration 结束帧；class 触发也走此）|
| `AnimationIterationEvent` | — | 非最后一次的 iteration 边界跨越（最后一次只发 End，对齐浏览器 `animationiteration`）|
| `AnimationKeyEvent` | `OnKey(float pct)` | 时间轴跨越注册的百分比键 |
| `AnimationHookEvent` | `OnHook(string name)` | 时间轴跨越 `@ikat-hook` 命名键（见 §9.4）|
| `TransitionEndEvent` | — | transition 完成后发（type=TweenComplete 分流）|

class 触发的动画无句柄，只走 EventBus 全局 `On<T>` 广播；`Play` 触发的动画句柄回调与全局广播并存（同一事件两路由都触发）。

### 9.4 @ikat-hook

`/* @ikat-hook name */` 注释标记命名锚点。百分比 + 语义名双锚并存。

### 9.5 调度

```csharp
ui.CallLater(float delay, Action cb);    // one-shot 延迟（秒，帧级粒度；d≤0 视为下一帧）
ui.CallNextFrame(Action cb);             // one-shot 下一帧（帧头 fire，先于 solve——新挂载子树 Geometry 仍全零）
ui.CallAfterLayout(Action cb);            // one-shot 当帧 solve 之后 fire（刚 Instantiate 的子树 Geometry 已实测可读，免自旋）
node.OnUpdate(Action<float> cb);         // recurring 每帧（返回 IDisposable；dt = Step 帧时长）
```

**泵点与帧内时序**：调度器住在引擎投影层（`UIContext`），集成层 `Step` 帧头泵（输入采集后、攒批回写前）——回调内改 `Style`/数据走既有 flush seam，**当帧 solve 生效**。`CallNextFrame` = 下一次泵的开头 fire。计时与 `Step` 用同一帧时长累积（逻辑时钟与动画时钟同源不双钟）。单回调抛异常被隔离（诊断日志，不阻断其他回调与后续帧）。headless 测试手动泵一次（同 flush seam 模式）。

---

## 10. 样式

### 10.1 四条路径

1. HTML/CSS（设计期）
2. `Classes.Add/Remove/Set/Replace`
3. typed `Style`（inline override 层，见 §3.1）
4. `Style.SetVar`

```csharp
public sealed class ClassList {
    public void Add(string name);
    public void Remove(string name);
    public bool Contains(string name);
    public void Toggle(string name);
    public void Set(string name, bool on);          // 条件加/移除
    public void Replace(string oldName, string newName);
}
```

### 10.2 StyleSheet 逃生舱

```csharp
public class StyleSheet {
    public IDisposable Add(string css);   // 返回句柄，撤销靠 Dispose（不靠原文匹配）
    public void Clear();
}
```

解析失败抛 `UIStyleException`。与模板 CSS 同 cascade 优先级。

### 10.3 禁止 !important

打包期报错。与 AI 可预测性赌注冲突。

---

## 11. 顶层上下文与边界

```csharp
public sealed class UIContext {
    public Container Root { get; }
    public Node FocusedNode { get; }
    public StyleSheet StyleSheet { get; }
    public UIPackage LoadPackage(string name, byte[] bytes);
    public void UnloadPackage(string name);
    public T Create<T>() where T : Node;
    public TextMetrics MeasureText(string text, string fontFamily, float sizePx, float maxWidth = 0f);
    public void CallLater(float delay, Action callback);
    public void CallNextFrame(Action callback);
    public void CallAfterLayout(Action callback);  // tick 后泵（IkatHost.Step 在 stage tick 之后调）
    public bool IsPointerOnUI { get; }
    public Node Pick(IkatVector2 globalPoint);
}

public sealed class UIPackage {
    public string Name { get; }
    public Container Instantiate(string templatePath);   // 返回模板根真实类型
    public UITemplate GetTemplate(string templatePath);
}

public sealed class UITemplate {
    public string Name { get; }
    public Container Instantiate();
}

public readonly struct TextMetrics {        // MeasureText 输出：布局前纯文本预估
    public float W { get; }                 // px
    public float H { get; }                 // px
    public uint LineCount { get; }          // 断行后行数（无 maxWidth 恒 1）
}
```

### 11.0.1 文本测量（MeasureText）

无节点纯测量：字符串 + 字体 + 字号 → 宽高 + 行数（布局前预估——tips 预分行 /
飘字宽估 / 按钮自适应宽，消灭业务侧手数字数）。与 solve 内文本测量走同一条
`measure_text` 断行代码，预估即所见。`maxWidth > 0` 按该宽贪心断行；`<= 0`（缺省）
单行。行高 normal、字距 0、常规字重（与缺省样式文本节点一致）。`fontFamily` 必须
已注册——未注册抛 `UIContractException`（测量必须用将渲染的同款字体，静默
fallback 到默认字体会给出误导性宽度）。

### 11.1 建树白名单

`Create<T>` 只能造纯结构容器和内容叶子：`Container`、`AbsolutePanel`、`TextNode`、`Image`。控件和作用域根只能从 HTML 模板实例化（`Instantiate`）——它们的类型语义 = HTML 签名 + 内部子树 + 默认行为，脱离 HTML 无稳定定义。非法 `T` 抛 `UIContractException`。

### 11.2 包生命周期

- `LoadPackage`/`Instantiate` 都同步（bytes 由逻辑层异步获取）。
- 同名重复 `LoadPackage` 抛 `UIContractException`（不静默覆盖）。
- 加载失败抛 `UIPackageException`。
- `UnloadPackage` 语义同 Unity prefab：只卸载模板注册表，已实例化的活节点是独立副本、不受影响（`UITemplate` = prefab asset，`Instantiate` = instance）。**不触碰 atlas 纹理与字体**——它们是 workspace 级资源、与包注册表解耦（见 main-design §14.5）。

### 11.3 边界与入口（引擎集成层职责）

公共 API 是引擎中立的语义层。以下**不进公共 API**，由引擎集成层（如 Unity 的 `IkatStageDriver`）实现：

- **UIContext 是「获取而非创建」**：无公共构造，由集成层创建、持有、驱动。业务程序员从集成层暴露的入口获取一个已跑起来的 UIContext。
- **tick / 输入采集 / 渲染产出**：集成层每帧驱动 tick、采集引擎输入喂入、把渲染树交后端镜像。
- **纹理注册**：`Image.Src` 是字符串 key（包内 or 运行时注册）。动态纹理的注册（`byte[]→Texture` 解码 + 注册 key）是引擎后端契约（Unity 侧 `SpriteResolver.Register(key, Texture2D)` 一类），用户自己解码塞入。查不到 key = 静默 error 态 + 警告一次，不抛。**每个引擎后端必须提供 runtime key 注册能力。**
- **原生渲染挂载**：3D 模型/粒子等非 UI 渲染挂载是引擎后端契约（Unity 侧 NativeHost），不进公共 API；集成层自行桥接。
- **世界锚点（投影路世界 UI）**：`IkatStageDriver.SetWorldAnchor(Node, Camera, Vector3 worldPos, Vector2 offsetPx)` / `ClearWorldAnchor(Node)`——Driver 每帧（Step 前）把世界点经相机投到屏幕 → 换算设计坐标写 `node.Transform.Position`（跟随 3D 实体；跳字/血条类 HUD 的官方路）。节点直挂 stage 根且 `position:absolute; left/top:0`（布局位 (0,0)，transform 即绝对坐标）。出屏/相机背后自动隐藏：core 侧 `ikat_stage_set_node_visible`（渲染层开关，与 `display:none` 正交——不动布局/命中；**继承语义**同 CSS `visibility:hidden`，隐藏祖先 = 整子树行 visible=0，后端保留镜像对象仅 SetActive(false)）。跳字动画 = 业务侧 TweenBuilder（如 Opacity 通道）× 锚点组合，无框架 helper。
- **world-space 子树挂载（整棵子树进 3D 世界）**：`IkatStageDriver.BindWorldMount(Node mountRoot, Transform worldParent)` / `UnbindWorldMount(Node)`——core 把挂载子树渲染行顶点 re-base 到挂载根局部系（挂载根设计位置成为局部原点），行带 mount 槽位标注（blob `mount_id` 列），后端按槽位把镜像 GO SetParent 到业务 3D 变换（内层 y-flip 容器；行层随业务容器 → 场景相机渲染 + ZTest LEqual 吃 3D 深度遮挡）。布局/命中仍在屏幕系——挂载只改渲染归属。批不跨挂载（mesh_key 含挂载维）。v1 约束：**挂载根须成 stacking context（声明 z-index）；挂载内禁 dropdown / 滚动容器 / 外阴影根 / overflow clip**（clip 平面定义在屏幕系，挂到 3D 后无意义，core 在挂载根重置 mask）。
- **光标指针 affordance**：意图是核心契约——core 沿 hover 命中链上溯宿主控件判定光标意图（0=箭头 / 1=手型 pointer / 2=隐藏 cursor:none，含 `cursor` 声明），经 `IkatHost.CursorIntent` 属性 + `CursorIntentChanged` 事件（去抖，仅变化帧）交集成层；**渲染是引擎后端契约**，同纹理注册/NativeHost 一类。Unity 侧 `IkatStageDriver`：缺省 intent 0/1 = 系统光标（不内置皮肤），intent 2 = 内置全透明载体（藏指针是语义）；`SetCursorTexture(uint intent, Texture2D texture, Vector2 hotspot)` 供业务按意图注册贴图（null = 清除；hotspot 从纹理左上角量；实现契约详见 projection-layer.md §3.3）。浏览器 preview 走原生 `cursor`，无此层。
- **分辨率适配**：策略数学在核心（`ikat_compute_adaptation` 纯函数：design/screen/safe/mode → scale + root + offset，三模式 `letterbox` / `fit-width` / `fit-height`），集成层只消费——Driver 读 `ikat.runtime.json` 的 `design`/`match_mode`（workspace 透传，Inspector 字段是 fallback），屏幕/safe 区变化时调数学 + `IkatHost.SetRootSize` 喂画布（core 下帧重排，`vw/vh` 声明跟随），渲染根变换与输入逆映射共用同一组 scale/offset（不本地重推，防双源漂移）。适配语义详见 main-design §11.5。

### 11.4 变长内容范式（替代预置满额）

战斗飘字、奖励列表、buff 图标行这类**数量运行时才知道**的内容，官方答案不是「页面预置满额节点 + 显隐轮转」（id 契约膨胀、上限靠猜、大量常驻 display:none）——三条路按场景选：

1. **数据驱动 ListView（列表/网格类首选）**：行数 = 数据源条目数，虚拟化自动建删 slot，id 契约零膨胀。列表形态的内容一律先想这条路。
2. **模板实例化（复合行/弹窗/toast）**：设计期把外观写进包（独立页面或 `<template>`），运行时 `GetTemplate(path).Instantiate()` / `pkg.Instantiate(path)` 克隆 → `AppendChild` 挂树 → 用完 `Dispose`。适合非列表结构的变长内容。
3. **`Create<T>`（无须设计稿外观的临时结构）**：纯容器包装、动态文本叶子。白名单见 §11.1。

**生命周期语义**：实例 = 模板的独立活副本（`UnloadPackage` 不影响已实例化节点）；`RemoveFromParent` 只摘不删（重挂复用，是手写池化的官方原语）；`Dispose` 真删（连带 tween/定时器清理）。

**create/destroy vs 池化怎么选**：模板实例化是 memcpy + slotmap 分配（无 IO、无解析），中低频（弹窗/toast/飘字，每秒几十个）直接实例化 + Dispose 即可；确证每帧数百级的高频极端才手写池（`RemoveFromParent` 摘下缓存，重挂后改内容，不 Dispose）。先测再池化——池化是优化不是范式。

**id 语义**：运行时创建的子树里，模板内静态 id 每实例一份；跨实例重名由作用域根隔离（`IsScopeRoot`）——从实例根（`Instantiate` 返回值）向下 `Get`，不从全局根跨作用域查。

Unity 集成层的接入手册（IkatStageDriver 挂载、加载钩子覆写、UI↔3D 互通、输入门控）随 ikat CLI 的 workspace 脚手架分发：`ikat-runtime` skill（scaffold 落各工作区会话根的 `.agents/skills/` / `.claude/skills/`；模板源在打包器 crate 的 `templates/runtime/SKILL.md`）。

模板根、作用域根用 `IsScopeRoot` 运行时标记（非类型），`Get<T>` 边界据此判定。

---

## 12. 北极星示例

```csharp
AbsolutePanel layer = ui.Create<AbsolutePanel>();
layer.Style.Width = Length.Pct(100);
layer.Style.Height = Length.Pct(100);
ui.Root.AddChild(layer);

Container inventory = game.Instantiate("views/inventory");   // 返回真实根类型
layer.AddChild(inventory);
inventory.Style.Left = Length.Px(300);
inventory.Style.Top = Length.Px(200);
inventory.Style.ZIndex = 1;

Container mask = ui.Create<Container>();
mask.Style.BackgroundColor = new IkatColor(0, 0, 0, 0.5f);
mask.Touchable = true;
mask.Style.ZIndex = 0;
layer.AddChild(mask);

Container titleBar = inventory.Get<Container>("title-bar");
float panX = 0f, panY = 0f;   // Delta 是逐 Move 增量——消费方自持累计态
titleBar.On<DragMoveEvent>(e => {
    panX += e.DeltaX; panY += e.DeltaY;
    inventory.Style.Left = Length.Px(baseLeft + panX);
    inventory.Style.Top = Length.Px(baseTop + panY);
});

// 日常开关：隐藏用 Display.None（保留节点+状态+订阅），不销毁
closeButton.Clicked += () => inventory.Style.Display = DisplayMode.None;

// 一次性弹窗：播完退场动画永久销毁（once 防泄漏）
dismissButton.Clicked += () => {
    popup.Classes.Add("slide-out");
    popup.On<AnimationEndEvent>(_ => popup.Dispose(), once: true);
};
```

---

## 13. 已锁定决策

1. 唯一公共目标用户是游戏业务程序员。
2. 积木优先：窗口/弹窗/拖放/RadioGroup 等逻辑层不提供。
3. CSS 优先：布局、样式、动画定义全在 CSS。
4. 类型化对象树，HTML 语义决定类型；CSS 不改类型。
5. Node 三分：Style（inline override 层）/Transform（渲染层）/Geometry（布局产物，滞后一帧）。
6. 运行时移动走 Transform；尺寸走 Style；读位置走 Geometry。
7. Container=子树是用户内容 / Node 叶子=子树是控件构造（按子树归属划线，见 §2.1）。
8. 无 Panel 类型；作用域是运行时标记（IsScopeRoot），Instantiate 返回真实根类型。
9. AbsolutePanel：语法糖，子节点自动 absolute。
10. 动画全 CSS，无命令式 tween；三种触发 + hook 双锚；class 切换下帧 rematch 生效、上帧 computed 做 transition 基线。
11. 拖拽：注册事件即参与仲裁；drop 靠 `ui.Pick` + 积木。
12. 焦点：每 UIContext 一个；Tab/Shift+Tab 焦点链导航内置，方向键/手柄导航是逻辑层积木；`Focusable` 运行时可改。
13. 坐标查询走 Geometry。
14. 引擎中立：tick/输入/渲染/纹理注册/原生渲染挂载归集成层，不进公共 API。
15. 不提供异步加载、data 挂载点、!important、GetVar、ForceLayout、命令式 tween。
16. StyleSheet 逃生舱：`Add` 返回 IDisposable 句柄。
17. ListView：虚拟化运行时隐式锁定，静态/数据驱动强制互斥；ItemExitClass 退场动画。
18. 控件数值用 float；文本读写走 TextNode.Text / Container.TextContent（DOM 语义）。
19. 事件退订两类：IDisposable 句柄 / C# +=-=；`On<T>` 支持 once。
20. 围栏外输入明确失败；运行时四异常（Contract/Disposed/Style/Package）。
21. Showcase 是端到端验收。
