// LoomGUI Frozen Public API: Events
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)
//
// ⚠️ 关键不变量——每个 typed event struct 的 `RouteEventCore _core` 字段必须是该 struct 的
// 首 field（offset 0）。EventBus.Dispatch 经 `Unsafe.As<T, RouteEventCore>(ref evt)` 把 evt
// 首 field 别名为 ref RouteEventCore；_core 不在首位会让 Unsafe.As 读错字段 → 静默内存损坏。
// 新增 typed event struct 时必须保持 `internal RouteEventCore _core;` 为首字段。此不变量由
// EventTypeCache<T> 静态 ctor 的 Marshal.OffsetOf 断言强制（fail-fast）。

using System;

// 下文每个 typed event struct 持 internal static byte EventType 属性，名字与 core 的 EventType
// enum 同名——struct 成员查找会遮蔽外层 enum。别名 LoomEventType 让属性表达式体内能解析到 enum。
using LoomEventType = LoomGUI.EventType;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    public interface IRouteEvent
    {
        Node Target { get; }
        Node CurrentTarget { get; }
        bool DefaultPrevented { get; }
        bool PropagationStopped { get; }
        void StopPropagation();
        void PreventDefault();
    }

    /// <summary>
    /// 订阅句柄（<see cref="Node.On{T}"/> 返回值）。Dispose 退订对应 handler。
    ///
    /// 设计：EventBus.Subscribe 在录入订阅表后 new EventRegistration(unsubscribeAction)
    /// 把退订闭包交回调用方；Dispose 调闭包 → EventBus.Remove 从订阅表移 entry。
    /// 幂等：二次 Dispose no-op（<c>_disposed</c> flag 拦）。订阅随 Node.Dispose 自动清理
    /// （public-api §5.4）—— Node.Dispose 走 evict 路径不调本类 Dispose，但订阅表通过
    /// NodeId 查询命中已 evict 节点是无效订阅；EventBus 不会主动清，由 GC 回收 Node 后
    /// 弱引用路径清理（roadmap 项，4a 不做：业务侧 Dispose reg 即可）。
    /// </summary>
    public sealed class EventRegistration : IDisposable
    {
        Action _unsubscribe;
        bool _disposed;

        /// <summary>
        /// 投影层内部：EventBus.Subscribe 调，传退订闭包。公共 API 无构造（业务从 On&lt;T&gt; 拿现成 reg）。
        /// </summary>
        internal EventRegistration(Action unsubscribe) { _unsubscribe = unsubscribe; }

        /// <summary>
        /// 退订。幂等（二次调 no-op）。不抛——handler 内调 Dispose（罕见但合法）也安全。
        /// </summary>
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _unsubscribe?.Invoke();
            _unsubscribe = null;   // 释放闭包引用（防闭包捕获的 handler/target 长寿）
        }
    }

    // Pointer
    public struct PointerDownEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        internal PointerButton _button;
        internal int _touchId;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        /// <summary>D2 EventBus 订阅表 key（对齐 core <see cref="EventType"/>）。</summary>
        internal static byte EventType => (byte)LoomEventType.Down;
        public Vector2 Position { get { return _position; } }
        public PointerButton Button { get { return _button; } }
        public int TouchId { get { return _touchId; } }
    }

    public struct PointerUpEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        internal PointerButton _button;
        internal int _touchId;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Up;
        public Vector2 Position { get { return _position; } }
        public PointerButton Button { get { return _button; } }
        public int TouchId { get { return _touchId; } }
    }

    public struct PointerMoveEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        internal float _deltaX;
        internal float _deltaY;
        internal int _touchId;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Move;
        public Vector2 Position { get { return _position; } }
        public float DeltaX { get { return _deltaX; } }
        public float DeltaY { get { return _deltaY; } }
        public int TouchId { get { return _touchId; } }
    }

    public struct PointerEnterEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.RollOver;
        public Vector2 Position { get { return _position; } }
    }

    public struct PointerLeaveEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.RollOut;
        public Vector2 Position { get { return _position; } }
    }

    public struct ClickEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        internal int _clickCount;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Click;
        public Vector2 Position { get { return _position; } }
        public int ClickCount { get { return _clickCount; } }
    }

    // Drag
    public struct DragStartEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        internal Vector2 _startPosition;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragStart;
        public Vector2 Position { get { return _position; } }
        public Vector2 StartPosition { get { return _startPosition; } }
    }

    public struct DragMoveEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        internal float _deltaX;
        internal float _deltaY;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragMove;
        public Vector2 Position { get { return _position; } }
        public float DeltaX { get { return _deltaX; } }
        public float DeltaY { get { return _deltaY; } }
    }

    public struct DragEndEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Vector2 _position;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragEnd;
        public Vector2 Position { get { return _position; } }
    }

    // Keyboard
    public struct KeyDownEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal KeyCode _key;
        internal KeyModifiers _modifiers;
        internal bool _repeat;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.KeyDown;
        public KeyCode Key { get { return _key; } }
        public KeyModifiers Modifiers { get { return _modifiers; } }
        public bool Repeat { get { return _repeat; } }
    }

    public struct KeyUpEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal KeyCode _key;
        internal KeyModifiers _modifiers;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.KeyUp;
        public KeyCode Key { get { return _key; } }
        public KeyModifiers Modifiers { get { return _modifiers; } }
    }

    // Focus
    public struct FocusEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Node _previousFocused;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.FocusIn;
        public Node PreviousFocused { get { return _previousFocused; } }
    }

    public struct BlurEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal Node _newFocused;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.FocusOut;
        public Node NewFocused { get { return _newFocused; } }
    }

    // Scroll
    public struct ScrollChangedEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal float _scrollX;
        internal float _scrollY;
        internal float _deltaScrollX;
        internal float _deltaScrollY;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // 无 LoomEvent 源（ScrollPane 物理自维护 tween）——D3 接 ScrollPane 回调。
        internal static byte EventType => (byte)LoomEventType.ScrollChanged;
        public float ScrollX { get { return _scrollX; } }
        public float ScrollY { get { return _scrollY; } }
        public float DeltaX { get { return _deltaScrollX; } }
        public float DeltaY { get { return _deltaScrollY; } }
    }

    // Animation lifecycle
    public struct AnimationStartEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal string _animationName;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // 无 LoomEvent 源（tween 回调产）——D3 接 TweenManager 回调。
        internal static byte EventType => (byte)LoomEventType.AnimationStart;
        public string AnimationName { get { return _animationName; } }
    }

    public struct AnimationEndEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal string _animationName;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // v1 经 TweenComplete（core 产，prop 名装 click_count）——D3 按 prop 名分流到本类型。
        internal static byte EventType => (byte)LoomEventType.AnimationEnd;
        public string AnimationName { get { return _animationName; } }
    }

    public struct AnimationIterationEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal string _animationName;
        internal int _iterationCount;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // 无 LoomEvent 源（tween 回调产）——D3 接 TweenManager 回调。
        internal static byte EventType => (byte)LoomEventType.AnimationIteration;
        public string AnimationName { get { return _animationName; } }
        public int IterationCount { get { return _iterationCount; } }
    }

    public struct TransitionEndEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal string _propertyName;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // v1 经 TweenComplete（core 产，prop 名装 click_count）——D3 按 prop 名分流到本类型。
        internal static byte EventType => (byte)LoomEventType.TransitionEnd;
        public string PropertyName { get { return _propertyName; } }
    }

    // ── 控件交互事件（internal route struct，D3）──────────────────────────
    // ValueChangedEvent<T> 是冻结公共 struct 但不实现 IRouteEvent（泛型 + 无 _core），不能直接走
    // EventBus。这三个 internal route struct 携 raw payload 经 EventBus 路由；控件类（Slider/Toggle/
    // RadioButton）的 ValueChanged/CheckedChanged 事件访问器订阅它们，翻译为公共 ValueChangedEvent<*>。
    // 这套 internal→public 翻译是 Button.Clicked backing-dict 模式的控件对应（On<ClickEvent> e=>value()）。
    //
    // payload 来自 core EVT_* EventRecord：VALUE_CHANGED/CHANGE_COMMITTED 用 x（float），
    // CHECKED_CHANGED 用 pad[0]（0/1）。core stream 不携旧值，故这些 struct 只装新值；
    // 翻译出的 ValueChangedEvent<*>.OldValue 留 default（core 契约同 web change 事件只给新值）。
    internal struct ControlValueChangedEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal float _value;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.ValueChanged;
        internal float Value { get { return _value; } }
    }

    internal struct ControlCheckedChangedEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal bool _checked;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.CheckedChanged;
        internal bool Checked { get { return _checked; } }
    }

    internal struct ControlChangeCommittedEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        internal float _value;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.ChangeCommitted;
        internal float Value { get { return _value; } }
    }

    // 单行文本框 Enter 提交（core EVT_SUBMITTED，type=25）。无 raw payload（node_id 指向提交控件）——
    // 提交时的当前 value 由控件类的 Submitted 访问器在触发时回读 get_control_text（文本值不进
    // EventRecord，同 ControlValueChangedEvent 的文本框语义）。TextArea 不订阅此事件（多行框
    // Enter 插换行，不提交）。
    internal struct ControlSubmittedEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Submitted;
    }
}
