// LoomGUI Frozen Public API: Events
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)
//
// ⚠️ 关键契约——每个 typed event struct 实现 IRouteEvent + IRouteEventCore（暴露 _core 引用）
// 并声明 internal static byte EventType 属性（订阅表 key，EventTypeCache<T> 反射解析）。
// EventBus.Dispatch 经约束泛型调用 evt.Core 读共享 core 引用（零装箱零别名）。
// 历史坑：曾要求 _core 为首 field 并用 Unsafe.As/__refvalue 做 offset-0 别名——Unsafe 类
// Unity 2021.3 Mono corlib 没有；__refvalue（refanyval）Mono 校验类型不符抛
// InvalidCastException。接口约束调用后 struct 字段顺序不再受约束。

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
    public struct PointerDownEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
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
        public LoomVector2 Position { get { return _position; } }
        public PointerButton Button { get { return _button; } }
        public int TouchId { get { return _touchId; } }
    }

    public struct PointerUpEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        internal PointerButton _button;
        internal int _touchId;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Up;
        public LoomVector2 Position { get { return _position; } }
        public PointerButton Button { get { return _button; } }
        public int TouchId { get { return _touchId; } }
    }

    public struct PointerMoveEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
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
        public LoomVector2 Position { get { return _position; } }
        public float DeltaX { get { return _deltaX; } }
        public float DeltaY { get { return _deltaY; } }
        public int TouchId { get { return _touchId; } }
    }

    public struct PointerEnterEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.RollOver;
        public LoomVector2 Position { get { return _position; } }
    }

    public struct PointerLeaveEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.RollOut;
        public LoomVector2 Position { get { return _position; } }
    }

    public struct ClickEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        internal int _clickCount;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Click;
        public LoomVector2 Position { get { return _position; } }
        public int ClickCount { get { return _clickCount; } }
    }

    // Drag
    public struct DragStartEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        internal LoomVector2 _startPosition;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragStart;
        public LoomVector2 Position { get { return _position; } }
        public LoomVector2 StartPosition { get { return _startPosition; } }
    }

    public struct DragMoveEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        internal float _deltaX;
        internal float _deltaY;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragMove;
        public LoomVector2 Position { get { return _position; } }
        public float DeltaX { get { return _deltaX; } }
        public float DeltaY { get { return _deltaY; } }
    }

    public struct DragEndEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomVector2 _position;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.DragEnd;
        public LoomVector2 Position { get { return _position; } }
    }

    // Keyboard
    public struct KeyDownEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomKeyCode _key;
        internal KeyModifiers _modifiers;
        internal bool _repeat;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.KeyDown;
        public LoomKeyCode Key { get { return _key; } }
        public KeyModifiers Modifiers { get { return _modifiers; } }
        public bool Repeat { get { return _repeat; } }
    }

    public struct KeyUpEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal LoomKeyCode _key;
        internal KeyModifiers _modifiers;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.KeyUp;
        public LoomKeyCode Key { get { return _key; } }
        public KeyModifiers Modifiers { get { return _modifiers; } }
    }

    // Focus
    public struct FocusEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    public struct BlurEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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
    public struct ScrollChangedEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    // AnimationHandle lifecycle
    // 18/19/20 = M2 真 core 事件源（crates/core/src/event.rs，T9）：class 触发 + node.Play
    // 都发，demux 直读 stream 填 AnimationName（字符串表索引读回）。END 另兼容 v1 的
    // TweenComplete（type=16）→ AnimationEnd 分流（transition 旧路径，既有测试锁定）。
    public struct AnimationStartEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    public struct AnimationEndEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    public struct AnimationIterationEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    // ── AnimationHandle 句柄私有事件（spec §7.5）──────────────────────────────
    // OnKey 跨越 / @loom-hook 跨越。不广播 EventBus——demux 按 playerKey 查 AnimationHandle 实例
    // 直接触发 OnKey(pct)/OnHook(name) 回调（回调是 Action，无事件参数）；struct 仅作载荷
    // 载体（字段供句柄路由读取 / 调试）。同其它 typed event struct 保持 _core 首字段约定。
    public struct AnimationKeyEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal string _animationName;
        internal float _percent;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.AnimationKey;
        public string AnimationName { get { return _animationName; } }
        public float Percent { get { return _percent; } }
    }

    public struct AnimationHookEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal string _animationName;
        internal string _hookName;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.AnimationHook;
        public string AnimationName { get { return _animationName; } }
        public string HookName { get { return _hookName; } }
    }

    public struct TransitionEndEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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
    internal struct ControlValueChangedEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    internal struct ControlCheckedChangedEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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

    internal struct ControlChangeCommittedEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
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
    internal struct ControlSubmittedEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.Submitted;
    }

    // Dropdown 选中项变更（core EVT_SELECTION_CHANGED，touch_id=新 selected_index）。route struct 携 raw
    // payload 经 EventBus 路由；Dropdown.SelectionChanged 订阅它并翻译为公共 SelectionChangedEvent。
    // core stream 不携 OldIndex（同 ControlValueChangedEvent 的「只报新值」语义）——翻译出的
    // SelectionChangedEvent.OldIndex 留 sentinel -1（NewIndex 由 demux 解出的 index 填）。
    internal struct ControlSelectionChangedEvent : IRouteEvent, IRouteEventCore
    {
        internal RouteEventCore _core;
        RouteEventCore IRouteEventCore.Core => _core;
        internal int _newIndex;
        public Node Target => _core.Target;
        public Node CurrentTarget => _core.CurrentTarget;
        public bool DefaultPrevented => _core._defaultPrevented;
        public bool PropagationStopped => _core._propagationStopped;
        public void StopPropagation() => _core.StopPropagation();
        public void PreventDefault() => _core.PreventDefault();
        internal static byte EventType => (byte)LoomEventType.SelectionChanged;
        internal int NewIndex { get { return _newIndex; } }
    }
}
