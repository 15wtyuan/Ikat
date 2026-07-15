// LoomGUI Frozen Public API: Node hierarchy & controls
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)

using System;
using System.Collections.Generic;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    // ── Node 基础层 ──────────────────────────────────────────────────
    // 三分模型：Style（可写/布局层，下帧 solve）/ Transform（可写/渲染层，不触发 solve）/
    //           Geometry（只读/布局产物，读最近一次 solve 结果，滞后一帧）。
    // Style/Transform 是 class + 内部 owner 引用（投影层：写回经 owner 标脏到 NodeId）；
    // Geometry 是 readonly struct 快照（从每帧 blob 填充）。
    public abstract class Node
    {
        public UIContext Context { get { throw NE(); } }
        public string Id { get { throw NE(); } }
        public Container Parent { get { throw NE(); } }   // Root.Parent == null

        public NodeStyle Style { get { throw NE(); } }
        public NodeTransform Transform { get { throw NE(); } }
        public NodeGeometry Geometry { get { throw NE(); } }

        public bool Touchable { get { throw NE(); } set { throw NE(); } }
        public bool Focusable { get { throw NE(); } set { throw NE(); } }   // 运行时改可获焦性（对齐 fgui focusable）
        public ClassList Classes { get { throw NE(); } }

        public bool IsDisposed { get { throw NE(); } }
        public void RemoveFromParent() { throw NE(); }   // 可重挂，不清订阅
        public void Dispose() { throw NE(); }            // 递归永久销毁，清订阅

        public T Get<T>(string id) where T : Node { throw NE(); }   // 作用域内查找，未找到抛 UIContractException
        public bool TryGet<T>(string id, out T node) where T : Node { throw NE(); }
        public IReadOnlyList<T> Query<T>() where T : Node { throw NE(); }            // 按类型，文档序
        public IReadOnlyList<Node> Query(string selector) { throw NE(); }            // ".class" / "tag.class"，文档序

        public Animation Play(string name) { throw NE(); }

        public void Focus() { throw NE(); }
        public void Blur() { throw NE(); }

        public IDisposable OnUpdate(Action<float> cb) { throw NE(); }   // 逻辑驱动每帧更新钩子（返回句柄，Dispose 撤销）
        public EventRegistration On<T>(Action<T> handler, bool useCapture = false, bool once = false) where T : IRouteEvent { throw NE(); }

        static NotImplementedException NE() => new NotImplementedException();
    }

    // Style = inline override 层（最高优先级），不是 cascade 读取窗口。
    // getter 只反映 C# setter 写过的属性；未写过返回 Unset（要 computed 走 Geometry）。
    // setter 写 Unset = 撤销该属性 inline override，回落 CSS。
    public sealed class NodeStyle
    {
        public Length Width { get { throw NE(); } set { throw NE(); } }
        public Length Height { get { throw NE(); } set { throw NE(); } }
        public Length MinWidth { get { throw NE(); } set { throw NE(); } }
        public Length MaxWidth { get { throw NE(); } set { throw NE(); } }
        public Length MinHeight { get { throw NE(); } set { throw NE(); } }
        public Length MaxHeight { get { throw NE(); } set { throw NE(); } }
        public DisplayMode Display { get { throw NE(); } set { throw NE(); } }
        public FlexDirection FlexDirection { get { throw NE(); } set { throw NE(); } }
        public FlexWrap FlexWrap { get { throw NE(); } set { throw NE(); } }
        public JustifyContent JustifyContent { get { throw NE(); } set { throw NE(); } }
        public AlignItems AlignItems { get { throw NE(); } set { throw NE(); } }
        public Length Gap { get { throw NE(); } set { throw NE(); } }
        public Thickness Padding { get { throw NE(); } set { throw NE(); } }
        public Thickness Margin { get { throw NE(); } set { throw NE(); } }
        public Thickness BorderWidth { get { throw NE(); } set { throw NE(); } }
        public Overflow OverflowX { get { throw NE(); } set { throw NE(); } }
        public Overflow OverflowY { get { throw NE(); } set { throw NE(); } }
        public Length Left { get { throw NE(); } set { throw NE(); } }
        public Length Top { get { throw NE(); } set { throw NE(); } }
        public Length Right { get { throw NE(); } set { throw NE(); } }
        public Length Bottom { get { throw NE(); } set { throw NE(); } }
        public PositionMode Position { get { throw NE(); } set { throw NE(); } }
        public int ZIndex { get { throw NE(); } set { throw NE(); } }
        public Color BackgroundColor { get { throw NE(); } set { throw NE(); } }
        public Color Color { get { throw NE(); } set { throw NE(); } }
        public float Opacity { get { throw NE(); } set { throw NE(); } }
        public Visibility Visibility { get { throw NE(); } set { throw NE(); } }
        public void SetVar(string n, Length v) { throw NE(); }
        public void SetVar(string n, Color v) { throw NE(); }
        public void SetVar(string n, float v) { throw NE(); }
        public void SetVar(string n, string v) { throw NE(); }
        public void RemoveVar(string n) { throw NE(); }   // 撤销 inline var，回落 CSS
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Transform = 渲染层，不触发 solve。回写走独立数值 FFI（set_transform，纯 f32）。
    public sealed class NodeTransform
    {
        public Vector2 Position { get { throw NE(); } set { throw NE(); } }
        public Vector2 Scale { get { throw NE(); } set { throw NE(); } }
        public float Rotation { get { throw NE(); } set { throw NE(); } }
        public Vector2 Origin { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Geometry = 只读快照，从每帧 blob 填充（滞后一帧，同 web reflow）。
    public readonly struct NodeGeometry
    {
        public Rect LayoutRect { get { throw NE(); } }
        public Rect WorldRect { get { throw NE(); } }
        public Vector2 LocalToGlobal(Vector2 p) { throw NE(); }
        public Vector2 GlobalToLocal(Vector2 p) { throw NE(); }
        public Rect LocalToGlobal(Rect r) { throw NE(); }
        public Rect GlobalToLocal(Rect r) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── Container 与树操作 ──────────────────────────────────────────
    public class Container : Node
    {
        public int ChildCount { get { throw NE(); } }
        public IReadOnlyList<Node> Children { get { throw NE(); } }
        public string TextContent { get { throw NE(); } set { throw NE(); } }   // DOM 语义：读=拼接后代文字；写=清子节点换单文本
        public T AddChild<T>(T c) where T : Node { throw NE(); }
        public T InsertChild<T>(T c, int i) where T : Node { throw NE(); }
        public void RemoveChild(Node c) { throw NE(); }
        public Node GetChildAt(int i) { throw NE(); }
        public int GetChildIndex(Node c) { throw NE(); }
        public void SetChildIndex(Node c, int i) { throw NE(); }
        public void SwapChildren(Node a, Node b) { throw NE(); }
        public void SwapChildrenAt(int a, int b) { throw NE(); }
        public void ScrollTo(Vector2 p, ScrollBehavior b = ScrollBehavior.Smooth) { throw NE(); }
        public event Action<ScrollChangedEvent> Scrolled;
        public UITemplate GetTemplate(string name) { throw NE(); }   // 取内联 template（原 Panel.GetTemplate 上移）
        static NotImplementedException NE() => new NotImplementedException();
    }

    // AbsolutePanel：自身 relative，AddChild 自动施加 absolute 到子节点。API 与 Container 一致。
    public sealed class AbsolutePanel : Container { }

    // 注：无 Panel 类型。作用域是运行时标记（IsScopeRoot），非类型；Instantiate 返回模板根真实类型。

    // ── 叶子：内容/绘制 ──
    public class TextNode : Node
    {
        public string Text { get { throw NE(); } set { throw NE(); } }   // 对应 DOM Node.textContent / CharacterData.data
        static NotImplementedException NE() => new NotImplementedException();
    }
    public class Image : Node
    {
        public string Src { get { throw NE(); } set { throw NE(); } }   // 字符串 key（包内 or 运行时注册）；动态纹理注册归引擎后端
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 容器类文本/标签（TextContent 走 Container 继承）──
    public class TextBlock : Container { }      // p
    public class TextElement : Container { }    // span/strong/em
    public class Label : Container { }          // 退化为语义容器：不加 For、不自动聚焦（点标签聚焦用 On<ClickEvent>+Focus() 积木）
    public class Canvas : Container { }         // 引擎渲染挂载点，无绘图 API；集成层 Query<Canvas>() + 读 Geometry.WorldRect 摆摄像机
    public class ListItem : Container { public int Index { get { throw NE(); } } static NotImplementedException NE() => new NotImplementedException(); }

    // ── 控件（叶子：私有内部结构）──
    public class Button : Container
    {
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        // 文本走 Container.TextContent（删原 TextContent 特例）
        public event Action Clicked;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Link : Container
    {
        public string Href { get { throw NE(); } set { throw NE(); } }   // 仅存字符串，框架不自动导航
        public event Action Activated;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextField : Node
    {
        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public TextSelection Selection { get { throw NE(); } set { throw NE(); } }   // 单行也支持选区/光标控制
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        public event Action<string> Submitted;   // 单行回车=提交；多行（TextArea）不提交
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class NumberField : Node
    {
        public float Value { get { throw NE(); } set { throw NE(); } }
        public float? Min { get { throw NE(); } set { throw NE(); } }
        public float? Max { get { throw NE(); } set { throw NE(); } }
        public float Step { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<float>> ValueChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Slider : Node
    {
        public float Value { get { throw NE(); } set { throw NE(); } }
        public float Min { get { throw NE(); } set { throw NE(); } }
        public float Max { get { throw NE(); } set { throw NE(); } }
        public float Step { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<float>> ValueChanged;
        public event Action<float> ChangeCommitted;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Toggle : Node
    {
        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class RadioButton : Node
    {
        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public string Name { get { throw NE(); } }   // 只读：结构性，决定分组语义
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;   // 同组互斥框架自动做；只新选中项触发（对齐 web）
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextArea : Node
    {
        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public TextSelection Selection { get { throw NE(); } set { throw NE(); } }
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Dropdown : Node
    {
        public int SelectedIndex { get { throw NE(); } set { throw NE(); } }
        public string SelectedValue { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<SelectionChangedEvent> SelectionChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class ProgressBar : Node
    {
        public float Value { get { throw NE(); } set { throw NE(); } }
        public float Max { get { throw NE(); } set { throw NE(); } }   // 0 基底，照 <progress> 标准，无 Min
        public bool IsIndeterminate { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── ListView ────────────────────────────────────────────────────
    // 虚拟化是运行时实现决策，不进 HTML。首次设 ItemCount/ItemTemplate/BindItem 即数据驱动+清空设计期 li；
    // 静态/数据驱动强制互斥（越界抛 UIContractException）。
    public class ListView : Container
    {
        public int ItemCount { get { throw NE(); } set { throw NE(); } }
        public UITemplate ItemTemplate { get { throw NE(); } set { throw NE(); } }
        public Func<int, UITemplate> TemplateSelector { get { throw NE(); } set { throw NE(); } }
        public Action<ListItem, int> BindItem { get { throw NE(); } set { throw NE(); } }
        public int SelectedIndex { get { throw NE(); } set { throw NE(); } }
        public event Action<SelectionChangedEvent> SelectionChanged;
        public void ScrollToItem(int i, ScrollBehavior b = ScrollBehavior.Smooth) { throw NE(); }
        public void RefreshItem(int i) { throw NE(); }
        public void RefreshItems() { throw NE(); }
        public void NotifyInserted(int i, int c = 1) { throw NE(); }
        public void NotifyRemoved(int i, int c = 1) { throw NE(); }
        public void NotifyMoved(int f, int t) { throw NE(); }
        public string ItemExitClass { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 动画 ────────────────────────────────────────────────────────
    // Animation 句柄非长期对象，生命周期 = 那次播放；播放结束句柄失效、hook 自动释放。
    public sealed class Animation
    {
        public string Name { get { throw NE(); } }
        public bool IsPlaying { get { throw NE(); } }
        public float Time { get { throw NE(); } set { throw NE(); } }
        public void Pause() { throw NE(); }
        public void Resume() { throw NE(); }
        public void Stop() { throw NE(); }
        public Animation OnStart(Action cb) { throw NE(); }
        public Animation OnEnd(Action cb) { throw NE(); }
        public Animation OnKey(float pct, Action cb) { throw NE(); }
        public Animation OnHook(string n, Action cb) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 样式辅助 ────────────────────────────────────────────────────
    public sealed class ClassList
    {
        public void Add(string n) { throw NE(); }
        public void Remove(string n) { throw NE(); }
        public bool Contains(string n) { throw NE(); }
        public void Toggle(string n) { throw NE(); }
        public void Set(string n, bool on) { throw NE(); }            // 条件加/移除
        public void Replace(string oldName, string newName) { throw NE(); }  // 互斥状态切换
        static NotImplementedException NE() => new NotImplementedException();
    }

    // StyleSheet 逃生舱：Add 返回 IDisposable 句柄，撤销靠 Dispose（不靠原文匹配）。
    public class StyleSheet
    {
        public IDisposable Add(string css) { throw NE(); }
        public void Clear() { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 模板 ────────────────────────────────────────────────────────
    public sealed class UITemplate
    {
        public string Name { get { throw NE(); } }
        public Container Instantiate() { throw NE(); }   // 返回模板根真实类型（围栏限定模板根为容器类）
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 顶层上下文 ──────────────────────────────────────────────────
    // UIContext 是「获取而非创建」：无公共构造，由引擎集成层创建/驱动。业务程序员从集成层获取。
    public sealed class UIContext
    {
        public Container Root { get { throw NE(); } }
        public Node FocusedNode { get { throw NE(); } }
        public StyleSheet StyleSheet { get { throw NE(); } }
        public UIPackage LoadPackage(string name, byte[] bytes) { throw NE(); }   // 同名重复抛 UIContractException；失败抛 UIPackageException
        public void UnloadPackage(string name) { throw NE(); }   // 同 Unity prefab：删模板，已实例化活节点独立副本不受影响
        public T Create<T>() where T : Node { throw NE(); }   // 白名单：Container/AbsolutePanel/TextNode/Image；控件+作用域根只能 Instantiate；非法 T 抛 UIContractException
        public void CallLater(float d, Action cb) { throw NE(); }
        public void CallNextFrame(Action cb) { throw NE(); }
        public bool IsPointerOnUI { get { throw NE(); } }
        public Node Pick(Vector2 globalPoint) { throw NE(); }   // 命中测试：返回该点最上层可命中节点（drop 逻辑靠它 + 积木）
        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class UIPackage
    {
        public string Name { get { throw NE(); } }
        public Container Instantiate(string path) { throw NE(); }   // 返回模板根真实类型
        public UITemplate GetTemplate(string path) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }
}
