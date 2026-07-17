// LoomGUI Frozen Public API: Node hierarchy & controls
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)

using System;
using System.Collections.Generic;
using LoomGUI.Bindings;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    // ── Node 基础层 ──────────────────────────────────────────────────
    // 三分模型：Style（可写/布局层，下帧 solve）/ Transform（可写/渲染层，不触发 solve）/
    //           Geometry（只读/布局产物，读最近一次 solve 结果，滞后一帧）。
    // Style/Transform 是 class + 内部 owner 引用（投影层：写回经 owner 标脏到 NodeId）；
    // Geometry 是 readonly struct 快照（从每帧 blob 填充）。
    public abstract unsafe class Node
    {
        // ── 投影层字段（internal）：owner 身份 + 生命周期标志 ─────────────
        // _id = Rust NodeId 的 u32 投影（slotmap key）；所有 FFI 调用经此转回 Rust 节点。
        // _ctx = 持有 stage handle + NodeRegistry 的 UIContext；本 Node 入 _ctx._registry 缓存。
        // _disposed = Dispose 后置 true；后续公共读操作抛 ObjectDisposedException。
        internal readonly uint _id;
        internal readonly UIContext _ctx;
        internal bool _disposed;

        // 投影层内部 ctor：经 NodeFactory 调（同 assembly 子类 base 链调）。公共 API 无构造路径
        // （业务从 Create<T> / Instantiate 拿现成 Node）。
        internal Node(UIContext ctx, uint id)
        {
            _ctx = ctx;
            _id = id;
        }

        public UIContext Context
        {
            get
            {
                ThrowIfDisposed();
                return _ctx;
            }
        }

        // Ponytail：暂无 FFI 读节点 id 属性（find_node_by_id 是反向：id 字符串 → NodeId）。
        // 返 numeric NodeId 作调试可读占位；待 get_id_attr FFI 加上后替换为真 id 属性读取。
        public string Id
        {
            get
            {
                ThrowIfDisposed();
                return _id.ToString();
            }
        }

        // Root.Parent == null（FFI node_parent 返 sentinel 0xFFFF_FFFF）。
        // 非根：registry.GetOrCreate(parent_id) → Container（围栏限定只容器型节点可为父）。
        public Container Parent
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                uint parentId = Native.loomgui_node_parent(h, _id);
                if (parentId == RootSentinel) return null;
                return (Container)_ctx._registry.GetOrCreate(parentId);
            }
        }

        public NodeStyle Style { get { throw NE(); } }
        public NodeTransform Transform { get { throw NE(); } }
        public NodeGeometry Geometry { get { throw NE(); } }

        public bool Touchable { get { throw NE(); } set { throw NE(); } }
        public bool Focusable { get { throw NE(); } set { throw NE(); } }   // 运行时改可获焦性（对齐 fgui focusable）
        public ClassList Classes { get { throw NE(); } }

        public bool IsDisposed => _disposed;

        /// <summary>
        /// 摘父：从父节点 children 移除，自身仍 live 可重挂。不清订阅、不 Dispose。
        /// 根节点（无父）no-op。
        /// </summary>
        public void RemoveFromParent()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            uint parentId = Native.loomgui_node_parent(h, _id);
            if (parentId == RootSentinel) return;   // 根：无父可摘
            Native.loomgui_stage_remove_child(h, parentId, _id);
        }

        /// <summary>
        /// 永久销毁：递归清子 + 调 Rust remove_node + evict 自身 + 标 _disposed。
        /// 后续公共读操作抛 ObjectDisposedException。幂等（二次调 no-op）。
        /// </summary>
        public void Dispose()
        {
            if (_disposed) return;

            // 先递归 evict C# 缓存中的后代（标 _disposed + 移出 registry），避免悬挂引用。
            // 走 Rust FFI 遍历子树——后代的 C# wrapper 可能尚未 GetOrCreate 过，跳过即可。
            DisposeDescendantsInRegistry(_id);

            // Rust 侧递归清子 + slotmap remove + anim/scroll/tween 联动（lib.rs:1230）。
            // 调用后 NodeId 失效（gen++）；后续该 id 的 FFI 调用是 no-op。
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_remove_node(h, _id);

            _ctx._registry.Remove(_id);
            _disposed = true;
        }

        public T Get<T>(string id) where T : Node { throw NE(); }   // 作用域内查找，未找到抛 UIContractException
        public bool TryGet<T>(string id, out T node) where T : Node { throw NE(); }
        public IReadOnlyList<T> Query<T>() where T : Node { throw NE(); }            // 按类型，文档序
        public IReadOnlyList<Node> Query(string selector) { throw NE(); }            // ".class" / "tag.class"，文档序

        public Animation Play(string name) { throw NE(); }

        public void Focus() { throw NE(); }
        public void Blur() { throw NE(); }

        public IDisposable OnUpdate(Action<float> cb) { throw NE(); }   // 逻辑驱动每帧更新钩子（返回句柄，Dispose 撤销）
        public EventRegistration On<T>(Action<T> handler, bool useCapture = false, bool once = false) where T : IRouteEvent { throw NE(); }

        // ── helpers ─────────────────────────────────────────────────────

        // node_parent 哨兵：根 / 越界 / 无 scene 均返 0xFFFF_FFFF（lib.rs:429）。
        internal const uint RootSentinel = 0xFFFF_FFFFu;

        /// <summary>
        /// Dispose 后访问抛 ObjectDisposedException。所有公共读操作入口都先调本方法。
        /// 异常消息用具体子类名（GetType().Name），帮助定位是哪种节点被误用。
        /// </summary>
        internal void ThrowIfDisposed()
        {
            if (_disposed) throw new ObjectDisposedException(GetType().Name);
        }

        /// <summary>
        /// 经 FFI 递归遍历 Rust 子树，对每个后代 NodeId：若 C# 侧有缓存 wrapper 则标 _disposed
        /// + 从 registry 移除。单次 remove_node（调用方 Dispose 末尾调）清 Rust 侧整棵子树；
        /// 本方法只维护 C# 缓存一致性，不调 remove_node per node。
        /// </summary>
        private void DisposeDescendantsInRegistry(uint subtreeRootId)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();

            // Snapshot 直系子（Rust 侧查询；下面的递归会改 Rust 树结构，不能边遍历边改）。
            int count = Native.loomgui_stage_get_child_count(h, subtreeRootId);
            if (count <= 0) return;
            uint[] buf = new uint[count];
            int written;
            fixed (uint* bp = buf)
            {
                written = Native.loomgui_stage_get_children(h, subtreeRootId, bp, (nuint)buf.Length);
            }
            // written < 0 = 节点刚被并发移除（理论单线程不达）；防御性早退防读越界。
            if (written < 0) return;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                uint childId = buf[i];
                // 深度优先：先清孙，再清子（清子后该 childId 在 Rust 侧可能已失效，但仍可读 cache）。
                DisposeDescendantsInRegistry(childId);
                if (_ctx._registry.TryGet(childId, out var cached))
                {
                    cached._disposed = true;
                    _ctx._registry.Remove(childId);
                }
            }
        }

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
    public unsafe class Container : Node
    {
        internal Container(UIContext ctx, uint id) : base(ctx, id) { }

        /// <summary>
        /// 直系子节点数。每次访问直读 Rust（get_child_count），不缓存——树可变
        /// （C6 写操作 AddChild/InsertChild/RemoveChild 会改子数），缓存会 stale。
        /// </summary>
        public int ChildCount
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int c = Native.loomgui_stage_get_child_count(h, _id);
                // -1 = 节点不 live（post-ThrowIfDisposed 理论不达，FFI 防御性兜底）。
                return c < 0 ? 0 : c;
            }
        }

        /// <summary>
        /// 直系子节点列表（typed）。每次访问 lazy 物化：调 get_children 拿 NodeId 数组 +
        /// 逐个 registry.GetOrCreate 包成 typed Node。不缓存 list 本身——树可变，缓存的 list
        /// 会 stale。但 list 内的 Node 引用稳定：GetOrCreate 走 registry 强引用缓存，同一 NodeId
        /// 永远返同一实例（订阅 / 镜像挂对象上不丢——projection §2.4）。
        /// </summary>
        public IReadOnlyList<Node> Children
        {
            get
            {
                ThrowIfDisposed();
                return MaterializeChildren();
            }
        }

        public string TextContent { get { throw NE(); } set { throw NE(); } }   // DOM 语义：读=拼接后代文字；写=清子节点换单文本
        public T AddChild<T>(T c) where T : Node { throw NE(); }
        public T InsertChild<T>(T c, int i) where T : Node { throw NE(); }
        public void RemoveChild(Node c) { throw NE(); }

        /// <summary>
        /// 取第 i 个直系子节点（按 append 顺序）。越界（负数 / ≥ ChildCount）抛
        /// ArgumentOutOfRangeException。物化路径同 <see cref="Children"/>：lazy + registry 缓存。
        /// </summary>
        public Node GetChildAt(int i)
        {
            ThrowIfDisposed();
            var kids = MaterializeChildren();
            // uint 强转一次校验 i ∈ [0, kids.Count)：负数变大正数越界，省一条分支。
            if ((uint)i >= (uint)kids.Count)
                throw new ArgumentOutOfRangeException(nameof(i), i,
                    $"index {i} out of range [0, {kids.Count})");
            return kids[i];
        }

        /// <summary>
        /// 查 c 在直系子中的索引（按 append 顺序）。未找到返 -1（.NET IndexOf 习惯，不抛）。
        /// null → ArgumentNullException；c 已 Dispose → ObjectDisposedException（stale 句柄不该再传 API）。
        /// 用 _id 比较：registry 保证同一 NodeId → 同一实例，_id 相等 ⇔ ReferenceEquals。
        /// </summary>
        public int GetChildIndex(Node c)
        {
            ThrowIfDisposed();
            if (c is null) throw new ArgumentNullException(nameof(c));
            if (c._disposed) throw new ObjectDisposedException(c.GetType().Name);

            var kids = MaterializeChildren();
            for (int i = 0; i < kids.Count; i++)
            {
                if (kids[i]._id == c._id) return i;
            }
            return -1;
        }

        public void SetChildIndex(Node c, int i) { throw NE(); }
        public void SwapChildren(Node a, Node b) { throw NE(); }
        public void SwapChildrenAt(int a, int b) { throw NE(); }
        public void ScrollTo(Vector2 p, ScrollBehavior b = ScrollBehavior.Smooth) { throw NE(); }
        public event Action<ScrollChangedEvent> Scrolled;
        public UITemplate GetTemplate(string name) { throw NE(); }   // 取内联 template（原 Panel.GetTemplate 上移）

        // ── helpers ─────────────────────────────────────────────────────

        /// <summary>
        /// 调 get_children 拿当前直系子 NodeId 数组 + 逐个 registry.GetOrCreate 包成 typed Node。
        /// FFI 调用模式复用 C1 <see cref="Node.DisposeDescendantsInRegistry"/>：先 get_child_count
        /// 定 cap，再 get_children 写入 fixed 钉住的 buffer（return-code + out-param，A6 cap 编码）。
        /// 单线程同步内 count 不会 stale；written 防御性 clamp 兜底 ABI 异常。
        /// </summary>
        private List<Node> MaterializeChildren()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();

            int count = Native.loomgui_stage_get_child_count(h, _id);
            var list = new List<Node>(count > 0 ? count : 0);
            if (count <= 0) return list;   // 0 子 / FFI err：返空 list（err post-ThrowIfDisposed 理论不达）

            uint[] buf = new uint[count];
            int written;
            fixed (uint* bp = buf)
            {
                written = Native.loomgui_stage_get_children(h, _id, bp, (nuint)buf.Length);
            }
            // written < 0 = 节点刚被并发移除（单线程理论不达）；防御性早退防读越界。
            if (written < 0) return list;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                // registry 缓存命中返同一实例；未命中走 NodeFactory 造 typed 子类（C1）+ 入缓存。
                list.Add(_ctx._registry.GetOrCreate(buf[i]));
            }
            return list;
        }

        static NotImplementedException NE() => new NotImplementedException();
    }

    // AbsolutePanel：自身 relative，AddChild 自动施加 absolute 到子节点。API 与 Container 一致。
    public sealed class AbsolutePanel : Container
    {
        internal AbsolutePanel(UIContext ctx, uint id) : base(ctx, id) { }
    }

    // 注：无 Panel 类型。作用域是运行时标记（IsScopeRoot），非类型；Instantiate 返回模板根真实类型。

    // ── 叶子：内容/绘制 ──
    public class TextNode : Node
    {
        internal TextNode(UIContext ctx, uint id) : base(ctx, id) { }

        public string Text { get { throw NE(); } set { throw NE(); } }   // 对应 DOM Node.textContent / CharacterData.data
        static NotImplementedException NE() => new NotImplementedException();
    }
    public class Image : Node
    {
        internal Image(UIContext ctx, uint id) : base(ctx, id) { }

        public string Src { get { throw NE(); } set { throw NE(); } }   // 字符串 key（包内 or 运行时注册）；动态纹理注册归引擎后端
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 容器类文本/标签（TextContent 走 Container 继承）──
    public class TextBlock : Container
    {
        internal TextBlock(UIContext ctx, uint id) : base(ctx, id) { }
    }      // p
    public class TextElement : Container
    {
        internal TextElement(UIContext ctx, uint id) : base(ctx, id) { }
    }    // span/strong/em
    public class Label : Container
    {
        internal Label(UIContext ctx, uint id) : base(ctx, id) { }
    }          // 退化为语义容器：不加 For、不自动聚焦（点标签聚焦用 On<ClickEvent>+Focus() 积木）
    public class Canvas : Container
    {
        internal Canvas(UIContext ctx, uint id) : base(ctx, id) { }
    }         // 引擎渲染挂载点，无绘图 API；集成层 Query<Canvas>() + 读 Geometry.WorldRect 摆摄像机
    public class ListItem : Container
    {
        internal ListItem(UIContext ctx, uint id) : base(ctx, id) { }
        public int Index { get { throw NE(); } } static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 控件（叶子：私有内部结构）──
    public class Button : Container
    {
        internal Button(UIContext ctx, uint id) : base(ctx, id) { }

        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        // 文本走 Container.TextContent（删原 TextContent 特例）
        public event Action Clicked;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Link : Container
    {
        internal Link(UIContext ctx, uint id) : base(ctx, id) { }

        public string Href { get { throw NE(); } set { throw NE(); } }   // 仅存字符串，框架不自动导航
        public event Action Activated;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextField : Node
    {
        internal TextField(UIContext ctx, uint id) : base(ctx, id) { }

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
        internal NumberField(UIContext ctx, uint id) : base(ctx, id) { }

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
        internal Slider(UIContext ctx, uint id) : base(ctx, id) { }

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
        internal Toggle(UIContext ctx, uint id) : base(ctx, id) { }

        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class RadioButton : Node
    {
        internal RadioButton(UIContext ctx, uint id) : base(ctx, id) { }

        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public string Name { get { throw NE(); } }   // 只读：结构性，决定分组语义
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;   // 同组互斥框架自动做；只新选中项触发（对齐 web）
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextArea : Node
    {
        internal TextArea(UIContext ctx, uint id) : base(ctx, id) { }

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
        internal Dropdown(UIContext ctx, uint id) : base(ctx, id) { }

        public int SelectedIndex { get { throw NE(); } set { throw NE(); } }
        public string SelectedValue { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<SelectionChangedEvent> SelectionChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class ProgressBar : Node
    {
        internal ProgressBar(UIContext ctx, uint id) : base(ctx, id) { }

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
        internal ListView(UIContext ctx, uint id) : base(ctx, id) { }

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
        // B3：headless harness / 引擎集成层建 UIContext 时持有的 Stage 句柄（raw FFI handle）。
        // 投影层（C1+）通过它转调 loomgui_stage_* FFI；公共 API 表面看不到本字段。
        internal IntPtr _stage;

        // C1：NodeId → typed Node 的强引用身份缓存（投影层 §2.4）。
        // NodeFactory 造节点入缓存；Node.Dispose 时 evict。公共 API 不见本字段。
        internal readonly NodeRegistry _registry;

        // B3：headless harness 工厂构造。public API 无构造（业务从集成层拿现成 instance）。
        // 建 NodeRegistry 持有自身反向引用（registry 转调 FFI 时需 stage handle）。
        internal UIContext(IntPtr stage)
        {
            _stage = stage;
            _registry = new NodeRegistry(this);
        }

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
