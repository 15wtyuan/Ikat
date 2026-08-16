# LoomGUI 公共 API 权威契约

> **单一真相源**：`unity/package/Runtime/Public/LoomGUI.*.cs`（冻结的 C# 签名，machine-readable）。本文档为人类可读契约，以签名文件为准。
>
> **防漂移门**：`tests/dotnet/LoomGUI.PublicApi` 编译校验——改公共签名后必编过。
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

    public Animation Play(string name);
    public void Focus();
    public void Blur();

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
    public int ZIndex { get; set; }
    public Color BackgroundColor/Color { get; set; }
    public float Opacity { get; set; }
    public void SetVar(string name, Length/Color/float/string value);
    public void RemoveVar(string name);
}
```

**不变量（inline override 层语义）**：
- Style 是**最高优先级的 inline override 层**，不是 cascade 的读取窗口。
- getter **只反映 C# setter 写过的属性**；未写过的返回 `Unset` 哨兵（`Length.Unset()` / `Color.Unset` / enum 的 `Unset` 成员）。要 computed 值走 `Geometry`。
- setter 写 `Unset` = 撤销该属性的 inline override，回落 CSS cascade。单属性撤销即用 `Style.X = Unset()`，无 `Clear`/`Reset`。
- `SetVar`/`RemoveVar` 管 CSS 自定义属性 `--*`；`--*` 跨作用域根传递。不提供 `GetVar`（var 不当状态存储读回）。
- 隐藏节点用 `Display = None`（不占位、不渲染、不命中，等同 fgui `visible=false`）；占位隐藏（保留布局空间）用 `Opacity = 0`。（`Visibility` API 已移除——fence CSS 子集无 `visibility` prop，无后盾；占位隐藏 `opacity:0` 覆盖。）

### 3.2 Transform（可写，渲染层，不触发 solve）

```csharp
public sealed class NodeTransform {
    public Vector2 Position { get; set; }   // 视觉偏移
    public Vector2 Scale { get; set; }
    public float Rotation { get; set; }      // 弧度
    public Vector2 Origin { get; set; }
}
```

最终渲染位置 = 布局位置 + Transform.Position。改 Transform 不触发 solve。

### 3.3 Geometry（只读，布局产物）

```csharp
public readonly struct NodeGeometry {
    public Rect LayoutRect { get; }          // 父坐标系
    public Rect WorldRect { get; }           // 全局坐标系
    public Vector2 LocalToGlobal(Vector2 point);
    public Vector2 GlobalToLocal(Vector2 point);
    public Rect LocalToGlobal(Rect rect);
    public Rect GlobalToLocal(Rect rect);
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
    public Vector2 ScrollPos { get; }   // 滚动容器当前滚动位置（非滚动容器返 (0,0)）；与 ScrollTo 成对
    public void ScrollTo(Vector2 pos, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public event Action<ScrollChangedEvent> Scrolled;
    public UITemplate GetTemplate(string name);          // 取内联 template
}
```

### 4.1 TextContent

对应 DOM `Element.textContent`（web 标准）。**写会清空所有子节点**换成单个文本——若容器内混有元素子节点（如 `<button>` 里的 `<img>`），写 `TextContent` 会一并清掉。要精细控制：把动态文本隔离进带 id 的行内元素，改那个元素：

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
    void StopPropagation();
    void PreventDefault();
}
```

### 5.3 事件清单

| 类别 | 事件 |
|---|---|
| 指针 | PointerDown/Up/Move/Enter/Leave, Click |
| 拖拽 | DragStart/Move/End |
| 键盘 | KeyDown/Up |
| 焦点 | Focus/Blur |
| 滚动 | ScrollChanged |
| 动画 | AnimationStart/End/Iteration, TransitionEnd |

指针键用 `PointerButton` 枚举（Left/Right/Middle）。

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

### 6.2 焦点

`Focus()` / `Blur()` / `ui.FocusedNode`。每 UIContext 一个焦点。设计期 `tabindex` 声明可获焦性，运行时用 `Node.Focusable` 改。**不做自动 Tab 链导航**——方向键/手柄导航是逻辑层积木（`On<KeyDown>` + `Focus()`）。

---

## 7. 标准控件

| HTML（role） | 类型 | 主要 API |
|---|---|---|
| button | Button : Container | Disabled, Clicked（文本走 Container.TextContent） |
| div role=textbox | TextField : Node | Value, Placeholder, Selection, ReadOnly, Disabled, ValueChanged, Submitted |
| div role=textbox aria-multiline=true | TextArea : Node | Value, Placeholder, Selection, ReadOnly, Disabled, ValueChanged |
| div role=spinbutton | NumberField : Node | Value, Min, Max, Step（float）, Disabled, ValueChanged |
| div role=slider | Slider : Node | Value, Min, Max, Step（float）, Disabled, ValueChanged, ChangeCommitted |
| div role=switch | Toggle : Node | IsChecked, Disabled, CheckedChanged |
| div role=radio | RadioButton : Node | IsChecked, Name（只读）, Disabled, CheckedChanged |
| div role=combobox | Dropdown : Node | SelectedIndex, SelectedValue, Disabled, SelectionChanged |
| div role=progressbar | ProgressBar : Node | Value, Max（float，0 基底）, IsIndeterminate |
| div role=tablist | TabList : Container | SelectedIndex, SelectionChanged（方向键/click 切换；panel 靠 `aria-controls` 关联） |
| div role=tab | Tab : Container | （`aria-selected` 由父 TabList.SelectedIndex 跨节点合成，非字面存储） |

**不变量**：
- 控件数值（Slider/NumberField/ProgressBar 的 Value/Min/Max/Step）用 `float`，与几何/引擎统一。大数精度需求归业务层。
- RadioButton 同 `Name` 组框架自动互斥；只有新选中项触发 `CheckedChanged`（对齐 web，不触发被取消项）。RadioGroup（按 name 聚合、读选中 index）是逻辑层积木，不进公共层。
- 通用事件类型：`ValueChangedEvent<T>`, `SelectionChangedEvent`, `TextSelection`。

控件与列表的类型由 `role` 分派（见 [fence.md](fence.md) §2.3、§3.1）；控件视觉部件用 `data-slot`（如 slider 的 `data-slot=thumb`、progressbar 的 `data-slot=fill`）。WAI-ARIA 复合控件沿用同一 `role` 机制：**TabList/Tab 已落地**（见上表）；Tree 等按需单独立项。

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

**契约**：`role=list → ListView`，`role=listitem → ListItem`。布局走 CSS。虚拟化全内部。

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

全 CSS 定义，无命令式 tween。

### 9.1 三种触发

1. class 切换（声明式）：`node.Classes.Add("slide-out")`，结束用 `On<AnimationEndEvent>`。
2. `node.Play("name")` 返回 `Animation` 句柄（程序化，带 hook）。
3. `Style.SetVar`（动态值逃生舱）。

**时序不变量**：class/typed style 变更在**下一帧 tick 的 rematch 生效**（不即时 rematch），一帧内多次增删只看帧末最终 class 集合。transition 基线 = 上一帧该属性的 computed 值。

`Play`（触发 2）与 class 切换（触发 1）分工：Play 用于「程序化、要句柄控制」；class 用于「声明式、只需知结束」。class 触发不产 `Animation` 句柄，结束统一走 `AnimationEndEvent`。

**`:nth-child(An+B|odd|even|N)` selector**（M2）配合 `animation-delay` 实现错峰入场——同一规则按子序号算 delay，常用于导航卡/列表项依次淡入（showcase `home.html` 7 条 `.nav-card:nth-child(N){animation-delay:...}`）。

### 9.2 Animation 句柄

```csharp
public sealed class Animation {
    public string Name { get; }
    public bool IsPlaying { get; }
    public float Time { get; set; }
    public void Pause(); public void Resume(); public void Stop();
    public Animation OnStart(Action cb);
    public Animation OnEnd(Action cb);
    public Animation OnKey(float percent, Action cb);
    public Animation OnHook(string name, Action cb);
}
```

**生命周期不变量**：Animation 句柄非长期对象，生命周期 = 那次播放。播放结束句柄失效、hook 自动释放（循环动画 `Stop()` 时释放）。

**事件双路由**（M2）：core `player.update` 检测阈值后 emit EventRecord，`borrow_events` 到 C# 双路由——

| typed 事件 | 句柄回调 | 触发 |
|---|---|---|
| `AnimationStartEvent` | `OnStart` | 播放开始（首个非 delay 帧）|
| `AnimationEndEvent` | `OnEnd` | 完成（最后一次 iteration 结束帧；class 触发也走此）|
| `AnimationIterationEvent` | — | 非最后一次的 iteration 边界跨越（最后一次只发 End，对齐浏览器 `animationiteration`）|
| `AnimationKeyEvent` | `OnKey(float pct)` | 时间轴跨越注册的百分比键 |
| `AnimationHookEvent` | `OnHook(string name)` | 时间轴跨越 `@loom-hook` 命名键（见 §9.3）|
| `TransitionEndEvent` | — | transition 完成后发（type=TweenComplete 分流）|

class 触发的动画无句柄，只走 EventBus 全局 `On<T>` 广播；`Play` 触发的动画句柄回调与全局广播并存（同一事件两路由都触发）。

### 9.3 @loom-hook

`/* @loom-hook name */` 注释标记命名锚点。百分比 + 语义名双锚并存。

### 9.4 调度

```csharp
ui.CallLater(float delay, Action cb);    // one-shot 延迟
ui.CallNextFrame(Action cb);             // one-shot 下一帧
node.OnUpdate(Action<float> cb);         // recurring 每帧（返回 IDisposable）
```

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
    public void CallLater(float delay, Action callback);
    public void CallNextFrame(Action callback);
    public bool IsPointerOnUI { get; }
    public Node Pick(Vector2 globalPoint);
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
```

### 11.1 建树白名单

`Create<T>` 只能造纯结构容器和内容叶子：`Container`、`AbsolutePanel`、`TextNode`、`Image`。控件和作用域根只能从 HTML 模板实例化（`Instantiate`）——它们的类型语义 = HTML 签名 + 内部子树 + 默认行为，脱离 HTML 无稳定定义。非法 `T` 抛 `UIContractException`。

### 11.2 包生命周期

- `LoadPackage`/`Instantiate` 都同步（bytes 由逻辑层异步获取）。
- 同名重复 `LoadPackage` 抛 `UIContractException`（不静默覆盖）。
- 加载失败抛 `UIPackageException`。
- `UnloadPackage` 语义同 Unity prefab：卸载模板/释放包资源，已实例化的活节点是独立副本、不受影响（`UITemplate` = prefab asset，`Instantiate` = instance）。

### 11.3 边界与入口（引擎集成层职责）

公共 API 是引擎中立的语义层。以下**不进公共 API**，由引擎集成层（如 Unity 的 `LoomStageDriver`）实现：

- **UIContext 是「获取而非创建」**：无公共构造，由集成层创建、持有、驱动。业务程序员从集成层暴露的入口获取一个已跑起来的 UIContext。
- **tick / 输入采集 / 渲染产出**：集成层每帧驱动 tick、采集引擎输入喂入、把渲染树交后端镜像。
- **纹理注册**：`Image.Src` 是字符串 key（包内 or 运行时注册）。动态纹理的注册（`byte[]→Texture` 解码 + 注册 key）是引擎后端契约（Unity 侧 `SpriteResolver.Register(key, Texture2D)` 一类），用户自己解码塞入。查不到 key = 静默 error 态 + 警告一次，不抛。**每个引擎后端必须提供 runtime key 注册能力。**
- **原生渲染挂载**：3D 模型/粒子等非 UI 渲染挂载是引擎后端契约（Unity 侧 NativeHost），不进公共 API；集成层自行桥接。

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
mask.Style.BackgroundColor = new Color(0, 0, 0, 0.5f);
mask.Touchable = true;
mask.Style.ZIndex = 0;
layer.AddChild(mask);

Container titleBar = inventory.Get<Container>("title-bar");
titleBar.On<DragMoveEvent>(e => {
    inventory.Style.Left = Length.Px(baseLeft + e.DeltaX);
    inventory.Style.Top = Length.Px(baseTop + e.DeltaY);
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
12. 焦点：每 UIContext 一个；不做自动 Tab 导航；`Focusable` 运行时可改。
13. 坐标查询走 Geometry。
14. 引擎中立：tick/输入/渲染/纹理注册/原生渲染挂载归集成层，不进公共 API。
15. 不提供异步加载、data 挂载点、!important、GetVar、ForceLayout、命令式 tween。
16. StyleSheet 逃生舱：`Add` 返回 IDisposable 句柄。
17. ListView：虚拟化运行时隐式锁定，静态/数据驱动强制互斥；ItemExitClass 退场动画。
18. 控件数值用 float；文本读写走 TextNode.Text / Container.TextContent（DOM 语义）。
19. 事件退订两类：IDisposable 句柄 / C# +=-=；`On<T>` 支持 once。
20. 围栏外输入明确失败；运行时四异常（Contract/Disposed/Style/Package）。
21. Showcase 是端到端验收。
