// EventDemuxer：raw LoomEvent stream → typed event struct dispatch。
//
// 设计契约：
// - Pump(ptr,count) 每 tick 调（LoomHost.Step 内，复用 borrow_events FFI 的同一 buffer）。
// - 逐条 LoomEvent 翻译为 typed event struct：
//     * _core.Target = _ctx._registry.GetOrCreate(nodeId)（投影层 Node 身份）。
//     * 业务字段（Position/ClickCount/TouchId/Key/Modifiers）从 raw EventRecord 填充。
//       不可从 raw 直接填充的字段（Button/DeltaX/DeltaY/StartPosition/Repeat/
//       PreviousFocused/NewFocused/Scroll*/AnimationName/PropertyName/IterationCount）
//       留在 default——后续接线补齐。
// - 调 _ctx._eventBus.Dispatch<T>(targetNodeId, evt) 走 EventBus capture/bubble/once 路由。
//
// 5 无核心 source struct 处理（EventType 17-21）：
// - AnimationEnd (20) / TransitionEnd (21)：接 TweenComplete (type=16) 源（旧路径）。
//   core TweenComplete EventRecord 的 click_count=TweenProp(u8)、touch_id=tag(i32)。
//   TweenComplete 事件同时产 AnimationEndEvent + TransitionEndEvent（按 prop 名称分流推后）。
// - AnimationStart (18) / AnimationIteration (19) / AnimationEnd (20)：真 core 事件源
//   （crates/core/src/event.rs）——见下方「@keyframes 动画事件」路由。
//
// - ScrollChanged (17)：defer（ScrollPane 物理自维护，无 core event source）。
//
// @keyframes 动画事件（双路由 + payload 编码）：
// - START/END/ITERATION（18/19/20）：node.EventBus 广播（On<AnimationXxxEvent>，class 触发
//   也能订阅）+ 按 playerKey 查 AnimationHandle 实例触发私有回调（onStart/onEnd）。
// - KEY/HOOK（27/28）：只按 playerKey 查句柄触发 onKey(pct)/onHook(name)，不广播 EventBus
//   （句柄私有）。
// - payload 解码（core event.rs）：name 表索引装 click_count+pad（24-bit LE）；PlayerKey u64
//   拆 touch_id（低 32）+ x（高 32 f32 bits）；y = 载荷（ITERATION=迭代序号 f32 bits /
//   KEY=percent / HOOK=hook_name 表索引 f32 bits）。字符串经 EventStrTable 索引读回
//   （loomgui_stage_get_event_string，双调法）。
//
// RawEventRecord：读 raw byte* 解包 EventRecord（与 Rust input::EventRecord 布局一致——20 字节）。
// 自足 struct，不依赖任何外部 LoomEvent 镜像——headless 测试编译链和 Unity 生产链共用此定义。

using System;
using System.Runtime.InteropServices;
using System.Text;
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
        internal ushort _pad;      // pad[2] @6-7（key events 的 modifiers 在 pad[0]；动画事件 name 表索引高 16 位）
        public int touchId; // -1=鼠标，>=0=触摸；key 复用装 key_code；动画事件 = PlayerKey 低 32 位
        public float x;
        public float y;
    }

    /// <summary>
    /// 投影层内部：每 tick 把 core borrow_events 的 raw <c>EventRecord[]</c> stream
    /// 翻译为 typed event struct 并喂 <see cref="EventBus.Dispatch{T}"/>。
    /// <see cref="UIContext"/> 持单实例；<see cref="LoomHost.Step"/> 调 <see cref="Pump"/>。
    /// </summary>
    internal sealed unsafe class EventDemuxer
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
                    case (byte)EventType.Down:
                        DispatchTyped(nodeId,
                            new PointerDownEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y), _touchId = evt.touchId });
                        break;
                    case (byte)EventType.Up:
                        DispatchTyped(nodeId,
                            new PointerUpEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y), _touchId = evt.touchId });
                        break;
                    case (byte)EventType.Move:
                        DispatchTyped(nodeId,
                            new PointerMoveEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y), _touchId = evt.touchId });
                        break;
                    // Enter/Leave 不沿祖先链路由：core 按悬停链差分逐节点发射（每个进出
                    // 边界的节点各得一条），冒泡会把「后代退链」误投给祖先订阅——指针仍在
                    // 祖先子树内，祖先级 hover 处理器被误触发（详见 DispatchTargetOnly）。
                    case (byte)EventType.RollOver:
                        DispatchTyped(nodeId,
                            new PointerEnterEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y) },
                            routeChain: false);
                        break;
                    case (byte)EventType.RollOut:
                        DispatchTyped(nodeId,
                            new PointerLeaveEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y) },
                            routeChain: false);
                        break;
                    case (byte)EventType.Click:
                        DispatchTyped(nodeId,
                            new ClickEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y), _clickCount = evt.clickCount });
                        break;

                    case (byte)EventType.DragStart:
                        DispatchTyped(nodeId,
                            new DragStartEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y) });
                        break;
                    case (byte)EventType.DragMove:
                        DispatchTyped(nodeId,
                            new DragMoveEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y) });
                        break;
                    case (byte)EventType.DragEnd:
                        DispatchTyped(nodeId,
                            new DragEndEvent { _core = NewCore(nodeId),
                                _position = new LoomVector2(evt.x, evt.y) });
                        break;

                    case (byte)EventType.KeyDown:
                        DispatchTyped(nodeId,
                            new KeyDownEvent { _core = NewCore(nodeId),
                                _key = (LoomKeyCode)evt.touchId, _modifiers = (KeyModifiers)(byte)evt._pad });
                        break;
                    case (byte)EventType.KeyUp:
                        DispatchTyped(nodeId,
                            new KeyUpEvent { _core = NewCore(nodeId),
                                _key = (LoomKeyCode)evt.touchId, _modifiers = (KeyModifiers)(byte)evt._pad });
                        break;

                    case (byte)EventType.FocusIn:
                        DispatchTyped(nodeId,
                            new FocusEvent { _core = NewCore(nodeId) });
                        break;
                    case (byte)EventType.FocusOut:
                        DispatchTyped(nodeId,
                            new BlurEvent { _core = NewCore(nodeId) });
                        break;

                    // core TweenComplete EventRecord：click_count = TweenProp (u8)、
                    // touch_id = tag (i32)。两条 typed struct 各自独立 RouteEventCore——若共享，
                    // AnimationEnd handler 调 StopPropagation 会污染 TransitionEnd 的 bubble。
                    // （旧路径保留；真 AnimationEnd=20 源见下段。）
                    case (byte)EventType.TweenComplete:
                        {
                            DispatchTyped(nodeId,
                                new AnimationEndEvent { _core = NewCore(nodeId) });
                            DispatchTyped(nodeId,
                                new TransitionEndEvent { _core = NewCore(nodeId) });
                        }
                        break;

                    // 解码（core event.rs）：name 表索引 24-bit LE（click_count+pad）；PlayerKey u64
                    // 拆 touch_id（低 32）+ x（高 32 f32 bits）；y = 载荷（ITERATION=迭代序号 /
                    // KEY=percent / HOOK=hook_name 表索引，均 f32 bits）。
                    // START/END/ITERATION：EventBus 广播 + 按 playerKey 查句柄触发私有回调
                    // （class 触发的动画无句柄——广播是它们唯一路径；句柄查 null 即跳过）。
                    case (byte)EventType.AnimationStart:
                        {
                            string name = ReadEventString(NameIndex(evt));
                            DispatchTyped(nodeId,
                                new AnimationStartEvent { _core = NewCore(nodeId), _animationName = name });
                            _ctx.ResolveAnimation(PlayerKeyOf(evt))?.FireStart();
                        }
                        break;
                    case (byte)EventType.AnimationIteration:
                        {
                            string name = ReadEventString(NameIndex(evt));
                            DispatchTyped(nodeId,
                                new AnimationIterationEvent
                                {
                                    _core = NewCore(nodeId),
                                    _animationName = name,
                                    _iterationCount = unchecked((int)FloatBitsToUInt(evt.y)),
                                });
                        }
                        break;
                    case (byte)EventType.AnimationEnd:
                        {
                            string name = ReadEventString(NameIndex(evt));
                            DispatchTyped(nodeId,
                                new AnimationEndEvent { _core = NewCore(nodeId), _animationName = name });
                            // onEnd 先触发再失效（FireEnd 内 finally 保证 Invalidate）。
                            _ctx.ResolveAnimation(PlayerKeyOf(evt))?.FireEnd();
                        }
                        break;
                    // KEY/HOOK：句柄私有，不广播 EventBus——只按 playerKey 查
                    // AnimationHandle 实例触发 onKey(pct)/onHook(name)。回调是 Action（无事件参数），
                    // struct 不在此构造（无消费方；字段供测试/调试直读）。
                    case (byte)EventType.AnimationKey:
                        _ctx.ResolveAnimation(PlayerKeyOf(evt))?.FireKey(evt.y);
                        break;
                    case (byte)EventType.AnimationHook:
                        _ctx.ResolveAnimation(PlayerKeyOf(evt))?.FireHook(ReadEventString(HookIndex(evt)));
                        break;

                    // payload 复用 EventRecord 现有字段：
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
                    // 单行文本框 Enter 提交（core EVT_SUBMITTED，25）。无 payload（node_id 指向提交控件）。
                    // 控件类的 Submitted 访问器订阅 ControlSubmittedEvent，在触发时回读当前 value 填
                    // Action<string>（文本值不进 EventRecord）。TextArea 不订阅此事件（多行框 Enter 插换行）。
                    case (byte)EventType.Submitted:
                        DispatchTyped(nodeId,
                            new ControlSubmittedEvent { _core = NewCore(nodeId) });
                        break;
                    // Dropdown 选中项变更（core EVT_SELECTION_CHANGED，26）。payload = 新 selected_index
                    // 装在 EventRecord.touch_id（core 侧 commit_dropdown_selection，与 Slider 装新值到 x
                    // 不同——Dropdown 的 index 是整数，复用 touch_id:i32 位，避免浮点往返精度损失）。
                    // Dropdown.SelectionChanged 订阅本 route struct 翻译为公共 SelectionChangedEvent。
                    case (byte)EventType.SelectionChanged:
                        DispatchTyped(nodeId,
                            new ControlSelectionChangedEvent { _core = NewCore(nodeId), _newIndex = evt.touchId });
                        break;

                    // LongPress (9)：无对应 typed event struct——跳过。
                    //
                    // ScrollChanged (17)：source 待补。ScrollPane 物理自维护 tween，
                    // 无 borrow_scroll_events FFI。后续需加 FFI 或 ScrollPane C# 回调
                    // 主动调 EventBus.Dispatch。
                    //
                    // AnimationStart/Iteration/End (18/19/20) 的 source 已接通
                    // （core event.rs，见上方动画事件路由）——不再 defer。

                    default:
                        // no-op: deferred or unknown event types
                        break;
                }
            }
        }

        /// <summary>
        /// 调 EventBus：target node 已由 _core.Target 指定（NewCore 已填）。
        /// routeChain=true 走 Dispatch（ancestor chain capture→bubble，CurrentTarget 逐节点刷新）；
        /// false 走 DispatchTargetOnly（Enter/Leave 专用，见 EventBus.DispatchTargetOnly）。
        /// Target=null（NewCore 遇 not-live nodeId）时丢弃该条——节点已销毁，事件无人接收。
        /// </summary>
        void DispatchTyped<T>(uint targetNodeId, T evt, bool routeChain = true) where T : IRouteEvent, IRouteEventCore
        {
            if (evt.Target == null) return;   // not-live node（见 NewCore）：丢弃不崩泵
            if (routeChain) _ctx._eventBus.Dispatch(targetNodeId, evt);
            else _ctx._eventBus.DispatchTargetOnly(targetNodeId, evt);
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

        /// <summary>
        /// float bit-pattern → uint：替代 <c>BitConverter.SingleToUInt32Bits</c>
        /// （该 API 自 .NET Core 2.0 才有，Unity Mono 运行时无——headless net10.0 测试
        /// 能编过但 Unity 报 CS0117）。类已 unsafe，指针重解释零分配、保留逐 bit 语义。
        /// </summary>
        static uint FloatBitsToUInt(float v) => *(uint*)&v;


        /// <summary>
        /// 解码 PlayerKey u64：touch_id = 低 32 位（core 侧 <c>u32 as i32</c> 往返）、
        /// x = 高 32 位 f32 bit pattern。与 core <c>player_key_as_u64</c>（slotmap
        /// <c>KeyData::as_ffi</c>：<c>(version &lt;&lt; 32) | idx</c>）互逆。
        /// </summary>
        static ulong PlayerKeyOf(RawEventRecord evt) =>
            ((ulong)(uint)evt.touchId) | ((ulong)FloatBitsToUInt(evt.x) << 32);

        /// <summary>
        /// 解码动画名表索引（24-bit 小端：click_count @5 | pad[0]&lt;&lt;8 | pad[1]&lt;&lt;16；
        /// _pad 是 pad[0..2] 的 ushort 镜像，一次移位覆盖高 16 位）。
        /// </summary>
        static uint NameIndex(RawEventRecord evt) => evt.clickCount | ((uint)evt._pad << 8);

        /// <summary>HOOK 载荷：hook_name 的表索引（f32 bits，core event.rs）。</summary>
        static uint HookIndex(RawEventRecord evt) => FloatBitsToUInt(evt.y);

        /// <summary>
        /// 按表索引读回字符串（EventStrTable；loomgui_stage_get_event_string
        /// 双调法：探大小 → 扩容 → 真读）。越界/无 scene 返空串（防御——正常路径索引恒由
        /// core intern 产生，越界只可能来自 ABI 漂移）。
        /// </summary>
        string ReadEventString(uint idx)
        {
            if (_ctx._stage == IntPtr.Zero) return "";
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            nuint len = 0;
            int rc = Native.loomgui_stage_get_event_string(h, idx, null, 0, &len);
            if (rc == -1 || len == 0) return "";   // 越界/无 scene/空串
            byte[] buf = new byte[(int)len];
            fixed (byte* bp = buf)
            {
                nuint written = 0;
                rc = Native.loomgui_stage_get_event_string(h, idx, bp, len, &written);
                if (rc != 0) return "";
                return Encoding.UTF8.GetString(buf, 0, (int)written);
            }
        }
    }
}
