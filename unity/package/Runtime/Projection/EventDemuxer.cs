// EventDemuxer：raw LoomEvent stream → typed event struct dispatch（投影层 D3）。
//
// 设计契约（spec §3.4 task D3）：
// - Pump(ptr,count) 每 tick 调（LoomHost.Step 内，复用 borrow_events FFI 的同一 buffer）。
// - 逐条 LoomEvent 翻译为 typed event struct：
//     * _core.Target = _ctx._registry.GetOrCreate(nodeId)（投影层 Node 身份）。
//     * 业务字段（Position/ClickCount/TouchId/Key/Modifiers）从 raw EventRecord 填充。
//       不可从 raw 直接填充的字段（Button/DeltaX/DeltaY/StartPosition/Repeat/
//       PreviousFocused/NewFocused/Scroll*/AnimationName/PropertyName/IterationCount）
//       留在 default——后续接线补齐（D3 焦点是 demux 接线 + 路由正确性）。
// - 调 _ctx._eventBus.Dispatch<T>(targetNodeId, evt) 走 D2 capture/bubble/once 路由。
//
// 5 无核心 source struct 处理（D1 EventType 17-21）：
// - AnimationEnd (20) / TransitionEnd (21)：接 TweenComplete (type=16) 源。
//   core TweenComplete EventRecord 的 click_count=TweenProp(u8)、touch_id=tag(i32)。
//   TweenComplete 事件同时产 AnimationEndEvent + TransitionEndEvent（按 prop 名称分流推后）。
// - ScrollChanged (17) / AnimationStart (18) / AnimationIteration (19)：defer（无 core event source）。
//   ScrollPane 物理自维护、tween 启动/循环无对应 FFI。Defer 标记见下文注释。
//
// RawEventRecord：读 raw byte* 解包 EventRecord（与 Rust input::EventRecord 布局一致——20 字节）。
// 自足 struct，不依赖任何外部 LoomEvent 镜像——headless 测试编译链和 Unity 生产链共用此定义。

using System;
using System.Runtime.InteropServices;
using LoomGUI.Bindings;

namespace LoomGUI
{
    /// <summary>
    /// Rust <c>loomgui_core::input::EventRecord</c> C# 镜像（20 字节）。
    /// 字段序：node_id:u32 @0 → event_type:u8 @4 → click_count:u8 @5 → pad [2] → touch_id:i32 @8 → x:f32 @12 → y:f32 @16。
    /// 自足 struct（headless 测试编译链用 unsafe 读 byte*，不依赖任何外部的 LoomEvent 镜像）。
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    struct RawEventRecord
    {
        public uint nodeId;
        public byte eventType;
        public byte clickCount;
        internal ushort _pad;      // pad[2] @6-7（key events 的 modifiers 在 pad[0]）
        public int touchId; // -1=鼠标，>=0=触摸；key 复用装 key_code
        public float x;
        public float y;
    }

    /// <summary>
    /// 投影层内部：每 tick 把 core borrow_events 的 raw <c>EventRecord[]</c> stream
    /// 翻译为 typed event struct 并喂 <see cref="EventBus.Dispatch{T}"/>。
    /// <see cref="UIContext"/> 持单实例；<see cref="LoomHost.Step"/> 调 <see cref="Pump"/>。
    /// </summary>
    internal sealed class EventDemuxer
    {
        readonly UIContext _ctx;

        internal EventDemuxer(UIContext ctx) => _ctx = ctx;

        /// <summary>
        /// 每 tick 调：读 <c>borrow_events</c> buffer（LoomHost.Step 已 byte* → IntPtr 透传）
        /// → 逐条翻译 → EventBus.Dispatch（typed On&lt;T&gt; 路径，单一订阅表）。
        /// </summary>
        /// <param name="ptr">borrow_events 返回的 native buffer（IntPtr=null 时 no-op）。</param>
        /// <param name="count">事件条数（非字节数；≤0 时 no-op）。</param>
        internal void Pump(IntPtr ptr, int count)
        {
            if (ptr == IntPtr.Zero || count <= 0) return;
            int recSize = Marshal.SizeOf<RawEventRecord>();
            for (int i = 0; i < count; i++)
            {
                var evt = Marshal.PtrToStructure<RawEventRecord>(ptr + i * recSize);
                uint nodeId = evt.nodeId;
                switch (evt.eventType)
                {
                    // ── Pointer 类（bubble 事件）─────────────────────────
                    case (byte)EventType.Down:
                        DispatchTyped(nodeId,
                            new PointerDownEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y), _touchId = evt.touchId });
                        break;
                    case (byte)EventType.Up:
                        DispatchTyped(nodeId,
                            new PointerUpEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y), _touchId = evt.touchId });
                        break;
                    case (byte)EventType.Move:
                        DispatchTyped(nodeId,
                            new PointerMoveEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y), _touchId = evt.touchId });
                        break;
                    case (byte)EventType.RollOver:
                        DispatchTyped(nodeId,
                            new PointerEnterEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y) });
                        break;
                    case (byte)EventType.RollOut:
                        DispatchTyped(nodeId,
                            new PointerLeaveEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y) });
                        break;
                    case (byte)EventType.Click:
                        DispatchTyped(nodeId,
                            new ClickEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y), _clickCount = evt.clickCount });
                        break;

                    // ── Drag 类（bubble 事件）────────────────────────────
                    case (byte)EventType.DragStart:
                        DispatchTyped(nodeId,
                            new DragStartEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y) });
                        break;
                    case (byte)EventType.DragMove:
                        DispatchTyped(nodeId,
                            new DragMoveEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y) });
                        break;
                    case (byte)EventType.DragEnd:
                        DispatchTyped(nodeId,
                            new DragEndEvent { _core = NewCore(nodeId),
                                _position = new Vector2(evt.x, evt.y) });
                        break;

                    // ── Keyboard 类（bubble 事件）────────────────────────
                    case (byte)EventType.KeyDown:
                        DispatchTyped(nodeId,
                            new KeyDownEvent { _core = NewCore(nodeId),
                                _key = (KeyCode)evt.touchId, _modifiers = (KeyModifiers)(byte)evt._pad });
                        break;
                    case (byte)EventType.KeyUp:
                        DispatchTyped(nodeId,
                            new KeyUpEvent { _core = NewCore(nodeId),
                                _key = (KeyCode)evt.touchId, _modifiers = (KeyModifiers)(byte)evt._pad });
                        break;

                    // ── Focus 类（bubble 事件）───────────────────────────
                    case (byte)EventType.FocusIn:
                        DispatchTyped(nodeId,
                            new FocusEvent { _core = NewCore(nodeId) });
                        break;
                    case (byte)EventType.FocusOut:
                        DispatchTyped(nodeId,
                            new BlurEvent { _core = NewCore(nodeId) });
                        break;

                    // ── TweenComplete → AnimationEnd + TransitionEnd ─────
                    // core TweenComplete EventRecord：click_count = TweenProp (u8)、
                    // touch_id = tag (i32)。两条 typed struct 各自独立 RouteEventCore——若共享，
                    // AnimationEnd handler 调 StopPropagation 会污染 TransitionEnd 的 bubble。
                    case (byte)EventType.TweenComplete:
                        {
                            DispatchTyped(nodeId,
                                new AnimationEndEvent { _core = NewCore(nodeId) });
                            DispatchTyped(nodeId,
                                new TransitionEndEvent { _core = NewCore(nodeId) });
                        }
                        break;

                    // ── 控件交互事件（22+，core EVT_*）──────────────────────
                    // payload 复用 EventRecord 现有字段（input.rs:80-85）：
                    //   VALUE_CHANGED(22) / CHANGE_COMMITTED(24)：x 装新 float 值（Slider 拖拽逐值 / 松手终值）。
                    //   CHECKED_CHANGED(23)：pad[0] 装布尔（Toggle 翻转 / Radio 新选中）。
                    // route struct（ControlValueChangedEvent 等）携 raw payload 经 EventBus 路由；控件类的
                    // ValueChanged/CheckedChanged 订阅它们并翻译为公共 ValueChangedEvent<*>。
                    case (byte)EventType.ValueChanged:
                        DispatchTyped(nodeId,
                            new ControlValueChangedEvent { _core = NewCore(nodeId), _value = evt.x });
                        break;
                    case (byte)EventType.CheckedChanged:
                        DispatchTyped(nodeId,
                            new ControlCheckedChangedEvent { _core = NewCore(nodeId), _checked = evt._pad != 0 });
                        break;
                    case (byte)EventType.ChangeCommitted:
                        DispatchTyped(nodeId,
                            new ControlChangeCommittedEvent { _core = NewCore(nodeId), _value = evt.x });
                        break;

                    // ── deferred（无 core source）────────────────────────
                    // LongPress (9)：无对应 typed event struct——跳过。
                    //
                    // ScrollChanged (17)：source 待补。ScrollPane 物理自维护 tween，
                    // 无 borrow_scroll_events FFI。后续需加 FFI 或 ScrollPane C# 回调
                    // 主动调 EventBus.Dispatch。
                    //
                    // AnimationStart (18)：source 待补。tween 启动无对应 EventRecord。
                    // 后续需在 TweenManager::update 的 started 标记位产 event 或
                    // C# 侧 hook Tween/Anim API 主动 dispatch。
                    //
                    // AnimationIteration (19)：source 待补。tween 循环无 FFI。
                    // 后续需 core expose loop-count 或 C# 侧补丁。

                    default:
                        // no-op: deferred or unknown event types
                        break;
                }
            }
        }

        /// <summary>
        /// 调 EventBus.Dispatch：target node 已由 _core.Target 指定（NewCore 已填）。
        /// Dispatch 内走 ancestor chain（capture→bubble），CurrentTarget 逐节点刷新。
        /// Target=null（NewCore 遇 not-live nodeId）时丢弃该条——节点已销毁，事件无人接收。
        /// </summary>
        void DispatchTyped<T>(uint targetNodeId, T evt) where T : IRouteEvent
        {
            if (evt.Target == null) return;   // not-live node（见 NewCore）：丢弃不崩泵
            _ctx._eventBus.Dispatch(targetNodeId, evt);
        }

        /// <summary>
        /// 从 raw nodeId 造 RouteEventCore，Target = registry.GetOrCreate(nodeId)。
        /// 若 registry 无缓存（本 tick 前未物化过该 NodeId），GetOrCreate 调 NodeFactory
        /// FFI 造 typed Node 并入缓存——首次触及时物化，后续 Dispatch 的 CurrentTarget
        /// 刷新也复用同一 Instance。
        ///
        /// not-live 容忍：nodeId 可能指向已销毁节点（切页 Dispose 旧页 / runtime 节点移除后，
        /// core 事件队列仍残留旧 id 的 hover/leave/click 等）。GetOrCreate → get_node_kind 此刻
        /// rc=1（node not live）抛 InvalidOperationException——节点不在，事件本就无人接收，
        /// 留 Target=null 让 DispatchTyped 丢弃，而不是让单条死事件崩整个事件泵。
        /// </summary>
        RouteEventCore NewCore(uint nodeId)
        {
            Node target = null;
            try
            {
                target = _ctx._registry.GetOrCreate(nodeId);
            }
            catch (InvalidOperationException)
            {
                // node not live：残留事件指向已销毁节点，丢弃（见方法注释）。
            }
            return new RouteEventCore { Target = target };
        }
    }
}
