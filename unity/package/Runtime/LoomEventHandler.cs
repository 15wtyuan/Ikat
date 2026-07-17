using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using LoomGUI.Bindings;

namespace LoomGUI
{
    // EventType enum 见 Public/LoomGUI.EventType.cs（PublicApi 冻结编译门需 Events.cs 能引用）。

    /// C# 镜像 Rust loomgui_core::input::EventRecord（#[repr(C)]）。
    /// 字段序：node_id:u32 @0 → event_type:u8 @4 → click_count:u8 @5 → pad 2 → touch_id:i32 @8 → x:f32 @12 → y:f32 @16（sizeof=20）。
    /// 与 Rust ABI 一致（StructLayout.Sequential 默认 pack=0；pad 在 C# 侧隐式——u32(4)+enum:byte(1)+byte(1)+[2 隐式对齐 int@8]+int(4)+float(4)+float(4)）。
    /// touch_id 用 snake_case 匹配 csbindgen 生成的字段名（Rust EventRecord 字段 snake_case）。
    /// modifiers 读 Rust EventRecord pad[0] @6（key 事件 modifiers；其余=0）。
    [StructLayout(LayoutKind.Sequential)]
    public struct LoomEvent
    {
        public uint nodeId;
        public EventType type;
        public byte clickCount;   // 1 或 2（仅 Click 有意义，其余=0；Rust EventRecord @5）
        public byte modifiers;    // Rust EventRecord pad[0] @6（key 事件 modifiers；其余=0）
        // @7 隐式 padding 对齐 touch_id @8
        public int touch_id;      // -1=鼠标，>=0=触摸（Rust EventRecord @8；key 事件复用装 key_code）
        public float x;
        public float y;
    }

    /// C# 镜像 Rust loomgui_core::scene::node::ControllerChangedEvent（#[repr(C)]）。
    /// 字段序：mount_node:u32 @0 → prev:i32 @4 → new:i32 @8（sizeof=12，紧凑无 pad）。
    /// 与 Rust ABI 一致（StructLayout.Sequential 默认 pack=0；u32+int+int 全 4 字节对齐）。
    /// new 是 C# 保留字，字段名用 new_index。
    [StructLayout(LayoutKind.Sequential)]
    public struct LoomControllerChangedEvent
    {
        public uint mount_node;
        public int prev;
        public int new_index;
    }

    /// 事件路由阶段（Capture/Target/Bubble），对齐 DOM/W3C 模型 + fgui capture/bubble 双组。
    public enum Phase : byte { Capture = 0, Target = 1, Bubble = 2 }

    /// 业务回调签名（listener 收 EventContext 读命中/坐标/止冒泡）。对齐 fgui EventCallback1。
    public delegate void EventCallback(EventContext ctx);

    /// Controller 切页回调签名（driver 订阅特定 mount_node，切页时收 prev/new_index）。
    /// 对齐 EventCallback 但更轻量——无坐标/无路由，单节点直派。
    public delegate void ControllerChangedCallback(uint mountNode, int prev, int newIndex);

    /// EventContext（照 fgui EventContext.cs，对象池复用）。
    public class EventContext
    {
        public uint target;            // 原始命中（EventRecord.node_id）
        public uint currentTarget;     // 路由当前节点
        public Phase phase;
        public EventType type;
        public int touchId;            // 事件所属触摸（-1=鼠标）
        public byte clickCount;          // 照 fgui InputEvent.clickCount（1=单击/2=双击）
        public uint keyCode;          // key 事件的 KeyCode（EventRecord.touch_id 复用）
        public byte modifiers;         // key 事件 modifiers（EventRecord.pad[0]）
        public bool isDoubleClick => clickCount > 1;   // 消费侧便利（照 fgui）
        public float x, y;
        internal bool _stopsPropagation, _defaultPrevented, _touchCapture, _stopsImmediatePropagation;
        public void StopPropagation() => _stopsPropagation = true;
        public void PreventDefault() => _defaultPrevented = true;
        /// 止当前节点剩余监听器 + 止冒泡（W3C stopImmediatePropagation；比 StopPropagation 多止同节点剩余）。
        public void StopImmediatePropagation() { _stopsImmediatePropagation = true; _stopsPropagation = true; }
        /// capture 当前触摸（照 fgui：设标志，BubbleRoute 消费即清，cap/bub 各加一 monitor）。
        public void CaptureTouch() => _touchCapture = true;

        static readonly System.Collections.Generic.Stack<EventContext> _pool = new();
        public static EventContext Get()
        {
            var ctx = _pool.Count > 0 ? _pool.Pop() : new EventContext();
            ctx._stopsPropagation = false; ctx._defaultPrevented = false; ctx._touchCapture = false; ctx._stopsImmediatePropagation = false;
            return ctx;
        }
        public static void Return(EventContext ctx) => _pool.Push(ctx);

        [UnityEngine.RuntimeInitializeOnLoadMethod(UnityEngine.RuntimeInitializeLoadType.SubsystemRegistration)]
        static void ClearPool() => _pool.Clear();   // Domain reload 清池（照 fgui EventContext.cs:96-102）
    }

    /// EventBridge（照 fgui EventBridge.cs）：capture + bubble 两组多播委托。
    public class EventBridge
    {
        EventCallback _bubble, _capture;
        public void Add(EventCallback cb) { _bubble -= cb; _bubble += cb; }       // -= 去重
        public void AddCapture(EventCallback cb) { _capture -= cb; _capture += cb; }
        public void Remove(EventCallback cb) => _bubble -= cb;
        public void RemoveCapture(EventCallback cb) => _capture -= cb;
        public bool isEmpty => _bubble == null && _capture == null;
        public void CallBubble(EventContext ctx)
        {
            if (_bubble == null) return;
            foreach (EventCallback cb in _bubble.GetInvocationList())
            {
                cb(ctx);
                if (ctx != null && ctx._stopsImmediatePropagation) break;
            }
        }
        public void CallCapture(EventContext ctx)
        {
            if (_capture == null) return;
            foreach (EventCallback cb in _capture.GetInvocationList())
            {
                cb(ctx);
                if (ctx != null && ctx._stopsImmediatePropagation) break;
            }
        }
    }

    /// C# 事件路由（照 fgui EventDispatcher）。listener 表 + bubble/capture 路由。
    /// unsafe：ParentOf 调 Native.loomgui_node_parent(StageHandle*, uint)，typed ptr 调用需 unsafe 域。
    public unsafe class LoomEventHandler
    {
        readonly System.Collections.Generic.Dictionary<uint, System.Collections.Generic.Dictionary<EventType, EventBridge>> _listeners = new();
        readonly System.Collections.Generic.Dictionary<uint, uint> _parentCache = new();   // node_parent 缓存
        // Controller 切页回调表：mount_node → 回调多播。无 bubble（单节点直派）。
        readonly System.Collections.Generic.Dictionary<uint, ControllerChangedCallback> _controllerChangedListeners = new();
        // v1.7 富文本超链接回调表：link_id → 回调。link_id 是 1-based 文档序（业务按 markup
        // 里 <a> 出现顺序维护 link_id→href 映射）。Click 命中 fragment 后直派，无 bubble。
        readonly System.Collections.Generic.Dictionary<uint, System.Action<uint>> _linkListeners = new();
        IntPtr _handle;   // node_parent FFI 用（LoomStage 初始化/load 后 SetHandle）
        uint? _captureNodeCap;   // capture 阶段调 CaptureTouch 的节点（消费即清）
        uint? _captureNodeBub;   // bubble 阶段调 CaptureTouch 的节点

        /// LoomStage 在 Awake/load 后调，传 (IntPtr)_stage。清 _parentCache（新 scene 的 parent 关系变了）。
        public void SetHandle(IntPtr handle) { _handle = handle; _parentCache.Clear(); }

        /// 清所有 listener（切 pkg 重建 scene 后，被删 NodeId 全失效，listener 指向悬空节点）。
        /// 业务 driver 切界面前调，避免 dict 堆积 + 重新 SubscribeAll 前干净态。
        public void Clear() { _listeners.Clear(); _parentCache.Clear(); _controllerChangedListeners.Clear(); _linkListeners.Clear(); }

        public void AddListener(uint nodeId, EventType type, EventCallback cb) => GetBridge(nodeId, type).Add(cb);
        public void AddCapture(uint nodeId, EventType type, EventCallback cb) => GetBridge(nodeId, type).AddCapture(cb);
        public void RemoveListener(uint nodeId, EventType type, EventCallback cb) => TryGetBridge(nodeId, type)?.Remove(cb);
        public void RemoveCapture(uint nodeId, EventType type, EventCallback cb) => TryGetBridge(nodeId, type)?.RemoveCapture(cb);

        /// 订阅 Controller 切页事件（mount_node 粒度）。driver 调：切页时收 prev/new_index。
        public void AddControllerChangedListener(uint mountNode, ControllerChangedCallback cb)
        {
            _controllerChangedListeners.TryGetValue(mountNode, out var existing);
            _controllerChangedListeners[mountNode] = (ControllerChangedCallback)
                System.Delegate.Combine(existing, cb);
        }
        public void RemoveControllerChangedListener(uint mountNode, ControllerChangedCallback cb)
        {
            if (_controllerChangedListeners.TryGetValue(mountNode, out var existing))
            {
                var updated = (ControllerChangedCallback)System.Delegate.Remove(existing, cb);
                if (updated == null) _controllerChangedListeners.Remove(mountNode);
                else _controllerChangedListeners[mountNode] = updated;
            }
        }

        /// 订阅富文本超链接点击（link_id 粒度）。link_id 是 1-based 文档序——业务按 markup
        /// 里 <a> 出现顺序维护 link_id→href 映射，core 不存 href。Click 命中 fragment 后直派。
        public void AddLinkClickListener(uint linkId, System.Action<uint> cb) => _linkListeners[linkId] = cb;
        public void RemoveLinkClickListener(uint linkId) => _linkListeners.Remove(linkId);
        void DispatchLinkClick(LoomEvent evt, uint linkId)
        {
            if (_linkListeners.TryGetValue(linkId, out var cb)) cb.Invoke(linkId);
        }

        EventBridge GetBridge(uint nodeId, EventType type)
        {
            if (!_listeners.TryGetValue(nodeId, out var byType))
                _listeners[nodeId] = byType = new System.Collections.Generic.Dictionary<EventType, EventBridge>();
            if (!byType.TryGetValue(type, out var bridge))
                byType[type] = bridge = new EventBridge();
            return bridge;
        }
        EventBridge TryGetBridge(uint nodeId, EventType type)
            => (_listeners.TryGetValue(nodeId, out var byType) && byType.TryGetValue(type, out var bridge)) ? bridge : null;

        /// 读 EventRecord[] buffer → 按 type 分流：Down/Up/Move/Click → BubbleRoute；RollOver/Out → DirectDispatch。
        /// ptr = loomgui_stage_borrow_events 返回（LoomStage 已 byte* → IntPtr 透传）。
        ///
        /// **Marshal.PtrToStructure**：桌面 Mono OK；IL2CPP 移动端对齐坑届时换 Span+BinaryPrimitives。
        public void DispatchPending(IntPtr ptr, int count)
        {
            if (ptr == IntPtr.Zero || count <= 0) return;
            int recSize = System.Runtime.InteropServices.Marshal.SizeOf<LoomEvent>();
            for (int i = 0; i < count; i++)
            {
                var evt = System.Runtime.InteropServices.Marshal.PtrToStructure<LoomEvent>(ptr + i * recSize);
                _captureNodeCap = null; _captureNodeBub = null;   // 每事件重置
                switch (evt.type)
                {
                    case EventType.Down:
                        BubbleRoute(evt);
                        // Down 路由后，capture 阶段 + bubble 阶段各消费一次 → 各加一 monitor（照 fgui）
                        if (_captureNodeCap.HasValue)
                            Native.loomgui_stage_add_touch_monitor((StageHandle*)_handle, evt.touch_id, _captureNodeCap.Value);
                        if (_captureNodeBub.HasValue)
                            Native.loomgui_stage_add_touch_monitor((StageHandle*)_handle, evt.touch_id, _captureNodeBub.Value);
                        break;
                    case EventType.Up:
                        BubbleRoute(evt); break;
                    case EventType.Click:
                        // v1.7 富文本超链接命中（pull FFI，复用 evt.x/evt.y design 坐标）。
                        // 命中节点级 AABB 后调 core 细分到 fragment：>0 走链接直派，否则原 bubble。
                        {
                            uint linkId = Native.loomgui_stage_rich_link_at((StageHandle*)_handle, evt.nodeId, evt.x, evt.y);
                            if (linkId != 0) { DispatchLinkClick(evt, linkId); break; }
                        }
                        BubbleRoute(evt); break;
                    // drag + longpress 走 BubbleRoute（core 算好 node_id，bubble 让祖先接）
                    case EventType.DragStart:
                    case EventType.DragMove:
                    case EventType.DragEnd:
                    case EventType.LongPress:
                        BubbleRoute(evt); break;
                    // keydown/up + focus/blur 走 BubbleRoute（core 算好 node_id，bubble 让祖先接）
                    case EventType.KeyDown:
                    case EventType.KeyUp:
                    case EventType.FocusIn:
                    case EventType.FocusOut:
                        BubbleRoute(evt); break;
                    case EventType.Move:
                        DirectDispatch(evt); break;   // Move 直派（核心算好的 monitor 目标）
                    case EventType.RollOver:
                    case EventType.RollOut:
                        DirectDispatch(evt); break;
                    // tween 完成 → 直派（target-specific，不 bubble；listener 读 ctx.clickCount=prop、ctx.touchId=tag）。
                    case EventType.TweenComplete:
                        DirectDispatch(evt); break;
                }
            }
        }

        /// 读 LoomControllerChangedEvent[] buffer → 按 mount_node 直派回调（无 bubble）。
        /// ptr = loomgui_stage_borrow_controller_changed_events 返回（LoomStage byte* → IntPtr 透传）。
        /// count 是记录数（非字节）。与 DispatchPending 同窗口（tick 后、下 tick 前）。
        ///
        /// **Marshal.PtrToStructure**：桌面 Mono OK；IL2CPP 移动端对齐坑届时换 Span+BinaryPrimitives。
        public void DispatchControllerChanged(System.IntPtr ptr, int count)
        {
            if (ptr == System.IntPtr.Zero || count <= 0) return;
            int recSize = System.Runtime.InteropServices.Marshal.SizeOf<LoomControllerChangedEvent>();
            for (int i = 0; i < count; i++)
            {
                var evt = System.Runtime.InteropServices.Marshal.PtrToStructure<LoomControllerChangedEvent>(ptr + i * recSize);
                if (_controllerChangedListeners.TryGetValue(evt.mount_node, out var cb))
                    cb?.Invoke(evt.mount_node, evt.prev, evt.new_index);
            }
        }

        /// bubble 类事件：capture(根→target 反向，不检查 stop) + bubble(target→root 正向，stop break)。照 fgui BubbleEvent。
        void BubbleRoute(LoomEvent evt)
        {
            var chain = AncestorChain(evt.nodeId);   // [target, ..., root]
            var ctx = EventContext.Get();
            ctx.target = evt.nodeId; ctx.type = evt.type; ctx.touchId = evt.touch_id; ctx.clickCount = evt.clickCount; ctx.keyCode = (uint)evt.touch_id; ctx.modifiers = evt.modifiers; ctx.x = evt.x; ctx.y = evt.y;
            // capture 阶段：根→target 反向，全跑（照 fgui line 302-311 不检查 stop）。消费 _touchCapture 记 _captureNodeCap。
            for (int i = chain.Count - 1; i >= 0; i--)
            {
                ctx.currentTarget = chain[i]; ctx.phase = Phase.Capture;
                TryGetBridge(chain[i], evt.type)?.CallCapture(ctx);
                if (ctx._touchCapture) { ctx._touchCapture = false; _captureNodeCap = chain[i]; }
            }
            // bubble 阶段：target→root 正向，stop break（照 fgui line 315-328）。消费 _touchCapture 记 _captureNodeBub。
            if (!ctx._stopsPropagation)
                for (int i = 0; i < chain.Count; i++)
                {
                    ctx.currentTarget = chain[i];
                    ctx.phase = (chain[i] == ctx.target) ? Phase.Target : Phase.Bubble;
                    TryGetBridge(chain[i], evt.type)?.CallBubble(ctx);
                    if (ctx._touchCapture) { ctx._touchCapture = false; _captureNodeBub = chain[i]; }
                    if (ctx._stopsPropagation) break;
                }
            EventContext.Return(ctx);
        }

        /// RollOver/Out 直派：核心已 diff 多目标，每条单节点跑 capture+bubble 回调（不沿链）。照 fgui DispatchEvent/InternalDispatchEvent。
        void DirectDispatch(LoomEvent evt)
        {
            var ctx = EventContext.Get();
            ctx.target = ctx.currentTarget = evt.nodeId; ctx.phase = Phase.Target;
            ctx.type = evt.type; ctx.touchId = evt.touch_id; ctx.clickCount = evt.clickCount; ctx.keyCode = (uint)evt.touch_id; ctx.modifiers = evt.modifiers; ctx.x = evt.x; ctx.y = evt.y;
            var bridge = TryGetBridge(evt.nodeId, evt.type);
            if (bridge != null) { bridge.CallCapture(ctx); bridge.CallBubble(ctx); }
            EventContext.Return(ctx);
        }

        /// target 起沿 node_parent 至 root 收集 [target, ..., root]。sentinel 0xFFFF_FFFF 止。
        System.Collections.Generic.List<uint> AncestorChain(uint target)
        {
            var chain = new System.Collections.Generic.List<uint>();
            uint c = target;
            while (c != 0xFFFF_FFFF) { chain.Add(c); c = ParentOf(c); }
            return chain;
        }
        uint ParentOf(uint nodeId)
        {
            if (!_parentCache.TryGetValue(nodeId, out var p))
                _parentCache[nodeId] = p = Native.loomgui_node_parent((StageHandle*)_handle, nodeId);   // csbindgen 绑定
            return p;
        }

        /// 主动释放某节点的 touch monitor（业务调，如拖拽结束）。转发核心 remove。
        public void RemoveTouchMonitor(uint nodeId) =>
            Native.loomgui_stage_remove_touch_monitor((StageHandle*)_handle, nodeId);

        /// 取消待 click（照 fgui CancelClick）。Down 后、Up 前调 → 该 touch 下个 Up 不发 Click。
        public void CancelTouch(int touchId) =>
            Native.loomgui_stage_cancel_click((StageHandle*)_handle, touchId);
    }
}
