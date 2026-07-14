// LoomGUI Frozen Public API: Node hierarchy & controls
// See docs/superpowers/specs/2026-07-15-frozen-public-api-design.md

using System;
using System.Collections.Generic;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    public abstract class Node
    {
        public UIContext Context { get { throw NE(); } }
        public string Id { get { throw NE(); } }
        public Container Parent { get { throw NE(); } }

        public NodeStyle Style { get { throw NE(); } }
        public NodeTransform Transform { get { throw NE(); } }
        public NodeGeometry Geometry { get { throw NE(); } }

        public bool Touchable { get { throw NE(); } set { throw NE(); } }

        public bool IsDisposed { get { throw NE(); } }
        public void RemoveFromParent() { throw NE(); }
        public void Dispose() { throw NE(); }

        public T Get<T>(string id) where T : Node { throw NE(); }
        public bool TryGet<T>(string id, out T node) where T : Node { throw NE(); }
        public IReadOnlyList<T> Query<T>() where T : Node { throw NE(); }

        public Animation Play(string name) { throw NE(); }

        public void Focus() { throw NE(); }
        public void Blur() { throw NE(); }

        public void OnUpdate(Action<float> cb) { throw NE(); }
        public void OffUpdate(Action<float> cb) { throw NE(); }

        public EventRegistration On<T>(Action<T> handler, bool useCapture = false) where T : IRouteEvent { throw NE(); }
        public void Off<T>(Action<T> handler) where T : IRouteEvent { throw NE(); }

        public ClassList Classes { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

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
        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class NodeTransform
    {
        public Vector2 Position { get { throw NE(); } set { throw NE(); } }
        public Vector2 Scale { get { throw NE(); } set { throw NE(); } }
        public float Rotation { get { throw NE(); } set { throw NE(); } }
        public Vector2 Origin { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

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

    public class Container : Node
    {
        public int ChildCount { get { throw NE(); } }
        public IReadOnlyList<Node> Children { get { throw NE(); } }
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
        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class AbsolutePanel : Container { }

    public class Panel : Container
    {
        public UITemplate GetTemplate(string name) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Leaf & content nodes
    public class TextNode : Node { }
    public class Image : Node { }
    public class TextBlock : Container { }
    public class TextElement : Container { }
    public class Label : Container { }
    public class Canvas : Container { }
    public class ListItem : Container { public int Index { get { throw NE(); } } static NotImplementedException NE() => new NotImplementedException(); }

    // Controls
    public class Button : Container
    {
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public string TextContent { get { throw NE(); } set { throw NE(); } }
        public event Action Clicked;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Link : Container
    {
        public string Href { get { throw NE(); } set { throw NE(); } }
        public event Action Activated;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextField : Node
    {
        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        public event Action<string> Submitted;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class NumberField : Node
    {
        public double Value { get { throw NE(); } set { throw NE(); } }
        public double? Min { get { throw NE(); } set { throw NE(); } }
        public double? Max { get { throw NE(); } set { throw NE(); } }
        public double Step { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<double>> ValueChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Slider : Node
    {
        public double Value { get { throw NE(); } set { throw NE(); } }
        public double Min { get { throw NE(); } set { throw NE(); } }
        public double Max { get { throw NE(); } set { throw NE(); } }
        public double Step { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<double>> ValueChanged;
        public event Action<double> ChangeCommitted;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Toggle : Node
    {
        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public bool IsIndeterminate { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class RadioButton : Node
    {
        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public string Name { get { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextArea : Node
    {
        public string Value { get { throw NE(); } set { throw NE(); } }
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
        public double Value { get { throw NE(); } set { throw NE(); } }
        public double Max { get { throw NE(); } set { throw NE(); } }
        public bool IsIndeterminate { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

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

    public sealed class ClassList
    {
        public void Add(string n) { throw NE(); }
        public void Remove(string n) { throw NE(); }
        public bool Contains(string n) { throw NE(); }
        public void Toggle(string n) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class StyleSheet
    {
        public void Add(string css) { throw NE(); }
        public void AddClass(string cls, string css) { throw NE(); }
        public void Remove(string css) { throw NE(); }
        public void Clear() { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class UITemplate
    {
        public string Name { get { throw NE(); } }
        public Panel Instantiate() { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class UIContext
    {
        public Container Root { get { throw NE(); } }
        public Node FocusedNode { get { throw NE(); } }
        public StyleSheet StyleSheet { get { throw NE(); } }
        public UIPackage LoadPackage(string name, byte[] bytes) { throw NE(); }
        public T Create<T>() where T : Node { throw NE(); }
        public void CallLater(float d, Action cb) { throw NE(); }
        public void CallNextFrame(Action cb) { throw NE(); }
        public bool IsPointerOnUI { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class UIPackage
    {
        public string Name { get { throw NE(); } }
        public Panel Instantiate(string path) { throw NE(); }
        public UITemplate GetTemplate(string path) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }
}
