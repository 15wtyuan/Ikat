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
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        /// <summary>D2 EventBus 订阅表 key（对齐 core <see cref="EventType"/>）。</summary>
        internal static byte EventType => (byte)LoomEventType.Down;
        public Vector2 Position { get { throw NE(); } }
        public PointerButton Button { get { throw NE(); } }
        public int TouchId { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerUpEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Up;
        public Vector2 Position { get { throw NE(); } }
        public PointerButton Button { get { throw NE(); } }
        public int TouchId { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerMoveEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Move;
        public Vector2 Position { get { throw NE(); } }
        public float DeltaX { get { throw NE(); } }
        public float DeltaY { get { throw NE(); } }
        public int TouchId { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerEnterEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.RollOver;
        public Vector2 Position { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerLeaveEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.RollOut;
        public Vector2 Position { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct ClickEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Click;
        public Vector2 Position { get { throw NE(); } }
        public int ClickCount { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Drag
    public struct DragStartEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragStart;
        public Vector2 Position { get { throw NE(); } }
        public Vector2 StartPosition { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct DragMoveEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragMove;
        public Vector2 Position { get { throw NE(); } }
        public float DeltaX { get { throw NE(); } }
        public float DeltaY { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct DragEndEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragEnd;
        public Vector2 Position { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Keyboard
    public struct KeyDownEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.KeyDown;
        public KeyCode Key { get { throw NE(); } }
        public KeyModifiers Modifiers { get { throw NE(); } }
        public bool Repeat { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct KeyUpEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.KeyUp;
        public KeyCode Key { get { throw NE(); } }
        public KeyModifiers Modifiers { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Focus
    public struct FocusEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.FocusIn;
        public Node PreviousFocused { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct BlurEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.FocusOut;
        public Node NewFocused { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Scroll
    public struct ScrollChangedEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // 无 LoomEvent 源（ScrollPane 物理自维护 tween）——D3 接 ScrollPane 回调。
        internal static byte EventType => (byte)LoomEventType.ScrollChanged;
        public float ScrollX { get { throw NE(); } }
        public float ScrollY { get { throw NE(); } }
        public float DeltaX { get { throw NE(); } }
        public float DeltaY { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Animation lifecycle
    public struct AnimationStartEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // 无 LoomEvent 源（tween 回调产）——D3 接 TweenManager 回调。
        internal static byte EventType => (byte)LoomEventType.AnimationStart;
        public string AnimationName { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct AnimationEndEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // v1 经 TweenComplete（core 产，prop 名装 click_count）——D3 按 prop 名分流到本类型。
        internal static byte EventType => (byte)LoomEventType.AnimationEnd;
        public string AnimationName { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct AnimationIterationEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // 无 LoomEvent 源（tween 回调产）——D3 接 TweenManager 回调。
        internal static byte EventType => (byte)LoomEventType.AnimationIteration;
        public string AnimationName { get { throw NE(); } }
        public int IterationCount { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct TransitionEndEvent : IRouteEvent
    {
        internal RouteEventCore _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        // v1 经 TweenComplete（core 产，prop 名装 click_count）——D3 按 prop 名分流到本类型。
        internal static byte EventType => (byte)LoomEventType.TransitionEnd;
        public string PropertyName { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }
}
