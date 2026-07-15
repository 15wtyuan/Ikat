# LoomGUI 靶子冻结：终态公共 API 设计

> 日期：2026-07-15
>
> 状态：**历史设计记录**。本 spec 是冻结初稿；经一轮 API grill（Q1–Q27）后，权威契约已转至 [docs/design/public-api.md](../../design/public-api.md)（公共 API）与 [docs/design/projection-layer.md](../../design/projection-layer.md)（投影层机制）。本文保留作设计过程记录，**不再维护**——以 design 文档为准。
>
> 原状态：已完成设计讨论，待书面审阅
>
> 范围：R2-R7 的验收靶子。只依赖 R1 围栏，现在就能冻结。
>
> 参考：FairyGUI、RmlUi、Unity UI Toolkit、标准 HTML/CSS、WAI-ARIA

## 1. 结论

本文档冻结 LoomGUI 面向游戏业务程序员的终态公共 API。它是自上而下设计的：公共契约优先，core/FFI/后端均可为它重写。R2-R7 的实现完成后，用这份签名作为验收靶子。

核心赌注：**AI 读 HTML/CSS 能正确预测渲染结果。** 动画定义也全在 CSS（不在 C# 代码里），与 AI 可预测性赌注完全自洽。

### 1.1 设计原则

- **积木优先**：框架只提供原子能力和底层原语；窗口系统、弹窗编排、拖放等逻辑层模式由用户用积木搭建。
- **CSS 优先**：布局、样式、动画的定义全在 CSS。C# 只负责触发、反应和数据注入。
- **类型化对象树**：稳定 HTML 语义决定对象类型；CSS 赋予行为能力，不改变类型。
- **引擎中立**：公共 API 不绑定任何引擎。Unity/Godot 通过消费同一套语义层接入。

### 1.2 与 2026-07-13 spec 的差异

- 围栏按 R1 spec 收窄：砍掉 dialog/details/form/meter，对应公共类型全部删除。
- Component 改名 Panel。
- 对象层级按 HTML 语义分容器与叶子。
- 新增 Node 基础层三分模型（Style/Transform/Geometry）。
- 新增动画系统（全 CSS，无命令式 tween）。
- 新增 AbsolutePanel、拖拽原语、焦点原语、坐标查询。
- 新增 StyleSheet 逃生舱、ItemExitClass（ListView 退场动画）。
- 不提供异步加载接口、data 挂载点、命令式 tween、!important。

## 2. 调研结论

| 框架 | 采用 | 不照搬 |
|---|---|---|
| FairyGUI | 类型化对象、包和模板、对象事件、列表复用、拖拽仲裁、焦点 | 绝对定位中心模型、Gear/Controller DSL、命令式 tween、全局单例、data 挂载点 |
| RmlUi | Document/Element 树、HTML/CSS、DOM 事件 | 低层 DOM 操作感、SetProperty 字符串风格 |
| UI Toolkit | Style/Transform/Geometry 分离、UQuery、CSS 动画 | Unity 专属 API、VisualElement 命名 |

LoomGUI 的定位模型是 CSS-flow 布局中心。位置/尺寸是布局产出的只读结果（Geometry），手动驱动的偏移走 Transform（渲染层），尺寸/定位走 Style（布局层）。

## 3. 对象层级

按 R1 围栏的 ContentModel 分容器与叶子。

```text
Node
+-- Container
|   +-- Panel（独立作用域，模板实例化产物）
|   +-- AbsolutePanel（语法糖，子节点自动 absolute）
|   +-- TextBlock（p）/ TextElement（span/strong/em）
|   +-- Label / Button / Link / Canvas
|   +-- ListView（ul/ol）/ ListItem（li）
+-- TextNode / Image / ProgressBar
+-- TextField / NumberField / Slider / Toggle / RadioButton
+-- TextArea / Dropdown
```

- Container 及子类暴露子节点增删；叶子节点没有。
- 容器子类的内容是设计期定义的，运行时通过 Get<T>("id") 访问。
- Custom Element 实例是 Panel。

### 3.1 稳定语义签名

节点类型由稳定 HTML 语义签名决定：tag + 不可变结构属性。CSS 永远不改变 C# 对象类型。

## 4. Node 基础层

三分模型：Style（可写/布局）、Transform（可写/渲染）、Geometry（只读/产物）。

```csharp
public abstract class Node {
    public UIContext Context { get; }
    public string Id { get; }
    public Container Parent { get; }
    public NodeStyle Style { get; }
    public NodeTransform Transform { get; }
    public NodeGeometry Geometry { get; }
    public bool Touchable { get; set; }
    public void RemoveFromParent();
    public void Dispose();
    public bool IsDisposed { get; }
    public T Get<T>(string id) where T : Node;
    public bool TryGet<T>(string id, out T node) where T : Node;
    public IReadOnlyList<T> Query<T>() where T : Node;
    public Animation Play(string animationName);
    public void Focus();
    public void Blur();
    public void OnUpdate(Action<float> callback);
    public void OffUpdate(Action<float> callback);
}
```

### 4.1 Style（可写，影响布局，下帧 solve）

```csharp
public sealed class NodeStyle {
    public Length Width/Height/MinWidth/MaxWidth/MinHeight/MaxHeight { get; set; }
    public DisplayMode Display { get; set; }       // Block | Flex | None
    public FlexDirection FlexDirection { get; set; } // Row（标准默认）| Column
    public FlexWrap/JustifyContent/AlignItems/Gap { get; set; }
    public Thickness Padding/Margin/BorderWidth { get; set; }
    public Overflow OverflowX/OverflowY { get; set; }
    public Length Left/Top/Right/Bottom { get; set; }
    public PositionMode Position { get; set; }      // Static | Relative | Absolute
    public int ZIndex { get; set; }
    public Color BackgroundColor/Color { get; set; }
    public float Opacity { get; set; }
    public Visibility Visibility { get; set; }      // Visible | Hidden
    public void SetVar(string name, Length/Color/float/string value);
}
```

### 4.2 Transform（可写，渲染层，不触发 solve）

```csharp
public sealed class NodeTransform {
    public Vector2 Position { get; set; }   // 视觉偏移
    public Vector2 Scale { get; set; }
    public float Rotation { get; set; }      // 弧度
    public Vector2 Origin { get; set; }
}
```

最终渲染位置 = 布局位置 + Transform.Position。

### 4.3 Geometry（只读，布局产物）

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

## 5. Container 与树操作

```csharp
public class Container : Node {
    public int ChildCount { get; }
    public IReadOnlyList<Node> Children { get; }
    public T AddChild<T>(T child) where T : Node;
    public T InsertChild<T>(T child, int index) where T : Node;
    public void RemoveChild(Node child);
    public Node GetChildAt(int index);
    public int GetChildIndex(Node child);
    public void SetChildIndex(Node child, int index);
    public void SwapChildren(Node a, Node b);
    public void SwapChildrenAt(int indexA, int indexB);
    public void ScrollTo(Vector2 pos, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public event Action<ScrollChangedEvent> Scrolled;
}
```

### 5.1 AbsolutePanel

自身 position: relative，AddChild 自动施加 position: absolute 到子节点。API 与 Container 一致。

### 5.2 Panel（独立作用域）

```csharp
public class Panel : Container {
    public UITemplate GetTemplate(string name);
}
```

- ID 作用域：Get<T> 不穿透嵌套 Panel/List item 边界。
- 样式隔离（Shadow DOM 风格）。
- CSS 自定义属性 --* 跨边界传递。

### 5.3 生命周期

RemoveFromParent 可重挂；Dispose 递归销毁。已销毁操作抛 ObjectDisposedException。

### 5.4 查询

Get<T> 抛 UIContractException；TryGet 可选；Query 返回零到多个。

## 6. 事件

### 6.1 两条路径

语义事件（C# event）：`button.Clicked += handler`
路由事件（On<T>）：`node.On<PointerDownEvent>(handler, useCapture: true)`

### 6.2 路由模型

DOM 三阶段：捕获 -> 目标 -> 冒泡。Target/CurrentTarget 都是 Node。

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

### 6.3 事件清单

指针：PointerDown/Up/Move/Enter/Leave/Click
拖拽：DragStart/Move/End
键盘：KeyDown/Up
焦点：Focus/Blur
滚动：ScrollChanged
动画：AnimationStart/End/Iteration, TransitionEnd

### 6.4 订阅清理

Dispose 自动清理订阅；RemoveFromParent 不清理。On<T> 返回 EventRegistration。

### 6.5 全局查询

`bool hit = ui.IsPointerOnUI;`

## 7. 拖拽

注册拖拽事件即参与仲裁，无 Draggable 属性。框架管阈值/捕获/仲裁（main-design S9.4）。用户施加 delta。

拖放是逻辑层模式，不提供。

## 8. 焦点

Focus()/Blur()/ui.FocusedNode。tabindex 决定可获焦性。每 UIContext 一个焦点。

## 9. 标准控件

| HTML | 类型 | 主要 API |
|---|---|---|
| button | Button | Disabled, TextContent, Clicked |
| a | Link | Href, Activated |
| input[text/password/search] | TextField | Value, Placeholder, ReadOnly, ValueChanged, Submitted |
| input[number] | NumberField | Value, Min, Max, Step, ValueChanged |
| input[range] | Slider | Value, Min, Max, Step, ValueChanged, ChangeCommitted |
| input[checkbox] | Toggle | IsChecked, IsIndeterminate, CheckedChanged |
| input[radio] | RadioButton | IsChecked, Name, CheckedChanged |
| textarea | TextArea | Value, Selection, ValueChanged |
| select | Dropdown | SelectedIndex, SelectedValue, SelectionChanged |
| progress | ProgressBar | Value, Max, IsIndeterminate |

通用类型：ValueChangedEvent<T>, SelectionChangedEvent, TextSelection

WAI-ARIA 复合控件（TabList 等）使用白名单 role。R1 围栏首版不含 role/aria-*，单独立项。

## 10. ListView

```csharp
public class ListView : Container {
    public int ItemCount { get; set; }
    public UITemplate ItemTemplate { get; set; }
    public Func<int, UITemplate> TemplateSelector { get; set; }
    public Action<ListItem, int> BindItem { get; set; }
    public int SelectedIndex { get; set; }
    public event Action<SelectionChangedEvent> SelectionChanged;
    public void ScrollToItem(int index, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public void RefreshItem(int index);
    public void RefreshItems();
    public void NotifyInserted(int index, int count = 1);
    public void NotifyRemoved(int index, int count = 1);
    public void NotifyMoved(int fromIndex, int toIndex);
    public string ItemExitClass { get; set; }
}
```

契约：ul/ol -> ListView，li -> ListItem。静态/数据驱动模式不混用。布局走 CSS。虚拟化全内部。

退场动画：ItemExitClass 设定后，NotifyRemoved 的 item 先加 class 等 AnimationEnd 再回收。

## 11. 动画

全 CSS 定义，无命令式 tween。

### 11.1 三种触发

1. class 切换（声明式）
2. node.Play("name") 返回 Animation 句柄（程序化，带 hook）
3. Style.SetVar（动态值逃生舱）

### 11.2 Animation 句柄

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

### 11.3 @loom-hook

`/* @loom-hook name */` 注释标记命名锚点。百分比 + 语义名双锚并存。

### 11.4 调度层

ui.CallLater(delay, cb) / ui.CallNextFrame(cb) / node.OnUpdate(dt => ...)

### 11.5 生命周期路由事件

AnimationEndEvent / TransitionEndEvent / AnimationIterationEvent

## 12. 样式

### 12.1 四条路径

1. HTML/CSS（设计期）2. Classes.Add/Remove 3. typed Style 4. SetVar

### 12.2 StyleSheet 逃生舱

```csharp
public class StyleSheet {
    public void Add(string css);
    public void AddClass(string className, string css);
    public void Remove(string css);
    public void Clear();
}
```

解析失败抛 UIStyleException。与模板 CSS 同 cascade 优先级。

### 12.3 禁止 !important

打包期报错。与 AI 可预测性赌注冲突。

## 13. 模板与复用

```csharp
public sealed class UITemplate {
    public string Name { get; }
    public Panel Instantiate();
}
```

三种来源：包级共享模板、内联 template、Panel 实例化。Custom Element 用 Web Components 约定。

## 14. 顶层上下文

```csharp
public sealed class UIContext {
    public Container Root { get; }
    public Node FocusedNode { get; }
    public StyleSheet StyleSheet { get; }
    public UIPackage LoadPackage(string name, byte[] bytes);
    public T Create<T>() where T : Node;
    public void CallLater(float delay, Action callback);
    public void CallNextFrame(Action callback);
    public bool IsPointerOnUI { get; }
}

public sealed class UIPackage {
    public string Name { get; }
    public Panel Instantiate(string templatePath);
    public UITemplate GetTemplate(string templatePath);
}
```

不提供异步加载——bytes 由逻辑层异步获取。LoadPackage 和 Instantiate 都是同步。

## 15. 错误与验证

打包期：围栏外输入明确失败。运行时：UIContractException / ObjectDisposedException / UIStyleException。

## 16. 北极星示例

```csharp
AbsolutePanel layer = ui.Create<AbsolutePanel>();
layer.Style.Width = Length.Pct(100);
layer.Style.Height = Length.Pct(100);
ui.Root.AddChild(layer);

Panel inventory = game.Instantiate("views/inventory");
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

closeButton.Clicked += () => {
    inventory.Classes.Add("slide-out");
    inventory.On<AnimationEndEvent>(e => inventory.Dispose());
};
```

## 17. 已锁定决策

1. 唯一公共目标用户是游戏业务程序员。
2. 积木优先：窗口/弹窗/拖放等逻辑层不提供。
3. CSS 优先：布局、样式、动画定义全在 CSS。
4. 类型化对象树，HTML 语义决定类型。
5. Node 三分：Style/Transform/Geometry。
6. 运行时移动走 Transform；尺寸走 Style。
7. 对象层级按 HTML 语义分容器与叶子。
8. Component 改名 Panel。
9. AbsolutePanel：语法糖，子节点自动 absolute。
10. 动画全 CSS，无命令式 tween；三种触发 + hook 双锚。
11. 拖拽：注册事件即参与仲裁。
12. 焦点：每 UIContext 一个。
13. 坐标查询走 Geometry。
14. 引擎中立。
15. 不提供异步加载、data 挂载点、!important。
16. StyleSheet 逃生舱。
17. ItemExitClass：ListView 退场动画。
18. Panel Shadow DOM 风格隔离。
19. 围栏外输入明确失败。
20. Showcase 是端到端验收。
