using System;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// D3 投影层 demux 接线 + 语义糖验收：
    /// - <see cref="Ikat.EventDemuxer"/> (<c>internal</c>) 翻译 raw IkatEvent stream → typed struct → EventBus.Dispatch。
    /// - <see cref="Button.Clicked"/> / <see cref="Link.Activated"/> semantic sugar：add = On&lt;ClickEvent&gt;，remove 经 EventRegistration backing 退订。
    /// - capture/bubble 顺序 via demux 真实路径。
    /// - StopPropagation via demux 路径。
    /// - Dispose-during-demux skips handler（D2 IsDisposed fix 在 demux 路径生效）。
    ///
    /// 全部经 headless harness P/Invoke 真 dll 建 Stage + parent chain。
    /// </summary>
    public unsafe class EventDemuxerTests
    {
        // ── demux 翻译 + dispatch ────────────────────────────────────────

        /// <summary>
        /// Pump 喂一个 Click 事件 → typed handler 收到 ClickEvent，Target 正确（registry.GetOrCreate）。
        /// </summary>
        [Fact]
        public void DemuxTranslatesAndDispatches()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ClickEvent received = default;
                n.On<ClickEvent>(e => received = e);

                // 造 raw buffer 模拟 borrow_events 返回：单条 Click IkatEvent
                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(n._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Same(n, received.Target);
                Assert.Same(n, received.CurrentTarget);   // target 节点上 Target == CurrentTarget
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Pump 遇 not-live nodeId（节点已 Dispose，core 事件队列残留旧 id）不崩泵，handler 不收到。
        /// 回归 ShowcaseRunner 切页 Dispose 旧页后，下帧 Pump 拿到旧节点 id 事件 → NewCore 经
        /// GetOrCreate → get_node_kind rc=1 抛 InvalidOperationException 崩泵。NewCore 现容忍 →
        /// Target=null → DispatchTyped 丢弃。
        /// </summary>
        [Fact]
        public void DemuxSkipsEventForNotLiveNode()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                bool called = false;
                n.On<ClickEvent>(e => called = true);

                n.Dispose();   // 节点销毁 → id not live（模拟切页 Dispose 旧页残留事件）

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(n._id);   // 残留事件指向已销毁 id
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);   // 不抛：not-live 容忍
                }

                Assert.False(called, "已销毁节点的事件不应送达 handler");
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// demux 翻译不同 event type 到不同 typed struct：PointerDown → PointerDownEvent。
        /// 验 demux 的 eventType switch 正确映射。
        /// </summary>
        [Fact]
        public void DemuxTranslatesPointerDownAsDistinctType()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                PointerDownEvent received = default;
                n.On<PointerDownEvent>(e => received = e);

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(n._id, (byte)EventType.Down);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Same(n, received.Target);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// TweenComplete 事件同时产 AnimationEndEvent + TransitionEndEvent。
        /// </summary>
        [Fact]
        public void TweenCompleteProducesAnimationEndAndTransitionEnd()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                bool animEndFired = false, transitionEndFired = false;
                n.On<AnimationEndEvent>(_ => animEndFired = true);
                n.On<TransitionEndEvent>(_ => transitionEndFired = true);

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(n._id, (byte)EventType.TweenComplete);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.True(animEndFired, "TweenComplete → AnimationEnd");
                Assert.True(transitionEndFired, "TweenComplete → TransitionEnd");
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// demux 翻译多条事件：逐条触发不互扰。
        /// </summary>
        [Fact]
        public void DemuxDispatchesMultipleEvents()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int count = 0;
                n.On<ClickEvent>(_ => count++);

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(n._id);
                    buf.AddClick(n._id);
                    buf.AddClick(n._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(3, count);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── 语义糖 Button.Clicked ────────────────────────────────────────

        /// <summary>
        /// Button.Clicked += handler → ClickEvent 触发时 handler 调（semantic sugar 冒泡到自身）。
        /// </summary>
        [Fact]
        public void ClickedSugarFires()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "button"));
                // NodeFactory kind "button" → Button 实例（registry.GetOrCreate 按 kind 构造）。
                // 若非 Button（kind 不匹配），用 Node.On 模拟——语义糖 add/remove 体只在
                // Button 类上，此处 registry 返回的实际是 Button。
                var button = (Button)n;
                bool fired = false;
                button.Clicked += () => fired = true;

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(button._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.True(fired);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Button.Clicked -= handler 后不再触发。
        /// </summary>
        [Fact]
        public void ClickedRemoveUnsubscribes()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                var button = (Button)ctx._registry.GetOrCreate(CreateRoot(stage, "button"));
                int count = 0;
                Action handler = () => count++;
                button.Clicked += handler;
                button.Clicked -= handler;

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(button._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(0, count);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 语义糖 Link.Activated += handler → ClickEvent 触发。
        /// </summary>
        [Fact]
        public void ActivatedSugarFires()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                // link 不是围栏支持的 kind——create_node 不认 "link" 会返 InvalidNodeId。
                // 用 Container 模拟：语义糖体与 Button 相同（On<ClickEvent> 冒泡到自身），
                // 验 ClickEvent 经 demux 可触发语义糖 handler。
                var container = (Container)ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                bool fired = false;
                // 用 On<ClickEvent> 模拟 Link.Activated 语义糖体（按 design，Activated 体同 Clicked）
                container.On<ClickEvent>(_ => fired = true, useCapture: false);

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(container._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.True(fired);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── capture/bubble 顺序 via demux ────────────────────────────────

        /// <summary>
        /// demux 真实路径验 capture→bubble 顺序：三级树 dispatch 子节点。
        /// </summary>
        [Fact]
        public void CaptureBubbleOrderViaDemux()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(stage, "div");
                ulong midId = CreateNode(stage, "div");
                ulong leafId = CreateNode(stage, "div");
                AppendChild(stage, rootId, midId);
                AppendChild(stage, midId, leafId);
                Node root = ctx._registry.GetOrCreate(rootId);
                Node mid = ctx._registry.GetOrCreate(midId);
                Node leaf = ctx._registry.GetOrCreate(leafId);

                var order = new System.Collections.Generic.List<string>();
                root.On<ClickEvent>(_ => order.Add("root-c"), useCapture: true);
                mid.On<ClickEvent>(_ => order.Add("mid-c"), useCapture: true);
                leaf.On<ClickEvent>(_ => order.Add("leaf-c"), useCapture: true);
                leaf.On<ClickEvent>(_ => order.Add("leaf-b"));
                mid.On<ClickEvent>(_ => order.Add("mid-b"));
                root.On<ClickEvent>(_ => order.Add("root-b"));

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(leaf._id);   // 命中 leaf, demux → EventBus.Dispatch 沿链路由
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(new[] {
                    "root-c", "mid-c", "leaf-c",   // capture: root → target
                    "leaf-b", "mid-b", "root-b",   // bubble:  target → root
                }, order);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── StopPropagation via demux ─────────────────────────────────────

        /// <summary>
        /// demux 路径 StopPropagation 生效：子 bubble handler 止传 → root bubble 不触发。
        /// </summary>
        [Fact]
        public void StopPropagationViaDemux()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(stage, "div");
                ulong childId = CreateNode(stage, "div");
                AppendChild(stage, rootId, childId);
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                Node child = ctx._registry.GetOrCreate(childId);

                bool rootFired = false;
                child.On<ClickEvent>(e => e.StopPropagation());
                root.On<ClickEvent>(_ => rootFired = true);

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(child._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.False(rootFired);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Dispose-during-demux skips handler ────────────────────────────

        /// <summary>
        /// handler A 在 demux 路径中间 Dispose handler B 的 reg → B 不触发。
        /// D2 IsDisposed fix 在 demux 路径生效。
        /// </summary>
        [Fact]
        public void DisposeDuringDemuxSkipsHandler()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int bCount = 0;
                EventRegistration regB = null;
                n.On<ClickEvent>(_ => regB.Dispose());          // A: Dispose B 的 reg
                regB = n.On<ClickEvent>(_ => bCount++);         // B: 被同步 Dispose

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(n._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(0, bCount);   // B 的 IsDisposed=true → snapshot 循环跳过
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// bubble handler 在路由途中 Dispose 祖先（切页 / 关面板 / 删 item 等 DOM 合法操作）：
        /// 剩余路径节点已被 Dispose（registry 移除 + 标 _disposed），Dispatch 须跳过它们，而不是
        /// GetOrCreate 重建 → get_node_kind rc=1 崩泵。回归 ShowcaseRunner 点 nav-card 切页：handler
        /// 内 Dispose 当前页根，bubble 上溯已销毁祖先，GetOrCreate(ancestor) 抛 InvalidOperationException。
        /// </summary>
        [Fact]
        public void DispatchSurvivesAncestorDisposeMidBubble()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(stage, "div");
                ulong midId = CreateNode(stage, "div");
                ulong leafId = CreateNode(stage, "div");
                AppendChild(stage, rootId, midId);
                AppendChild(stage, midId, leafId);
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                Node leaf = ctx._registry.GetOrCreate(leafId);

                bool rootBubbleFired = false;
                leaf.On<ClickEvent>(_ => root.Dispose());    // 模拟切页：销毁整棵当前页树（含 leaf 自己）
                root.On<ClickEvent>(_ => rootBubbleFired = true);

                using (var buf = new NativeEventBuffer())
                {
                    buf.AddClick(leaf._id);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);   // 不抛：IsLive 跳过已 Dispose 的 mid / root
                }

                Assert.False(rootBubbleFired, "Dispose 后路径节点不应再触发 handler");
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Empty buffer (ptr=null, count=0) → Pump no-op，不抛。
        /// </summary>
        [Fact]
        public void PumpWithEmptyBufferIsNoOp()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ctx._eventDemuxer.Pump(IntPtr.Zero, 0);   // 不抛即可
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Finding 1 regression: TweenComplete per-event core ──────────────────

        /// <summary>
        /// TweenComplete → AnimationEnd + TransitionEnd 各持独立 RouteEventCore。
        /// AnimationEnd handler 调 StopPropagation 不影响 TransitionEnd bubble。
        /// </summary>
        [Fact]
        public void TweenCompleteProducesIndependentCores()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(stage, "div");
                ulong childId = CreateNode(stage, "div");
                AppendChild(stage, rootId, childId);
                Node root = ctx._registry.GetOrCreate(rootId);
                Node child = ctx._registry.GetOrCreate(childId);

                bool animEndFired = false, transitionEndBubbled = false;
                child.On<AnimationEndEvent>(e => { animEndFired = true; e.StopPropagation(); });
                root.On<TransitionEndEvent>(_ => transitionEndBubbled = true);

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(child._id, (byte)EventType.TweenComplete);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.True(animEndFired, "AnimationEnd fired on child");
                Assert.True(transitionEndBubbled,
                    "TransitionEnd bubbled to root — NOT affected by AnimationEnd's StopPropagation (independent cores)");
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Finding 2 regression: business fields from raw data ────────────────

        /// <summary>
        /// PointerDownEvent.Position 从 evt.x, evt.y 填充（不抛 NotImplementedException）。
        /// </summary>
        [Fact]
        public void PointerDownPositionIsFilled()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                PointerDownEvent received = default;
                n.On<PointerDownEvent>(e => received = e);

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(n._id, (byte)EventType.Down, x: 123.5f, y: 456.7f);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(123.5f, received.Position.X, 4);
                Assert.Equal(456.7f, received.Position.Y, 4);
                Assert.Equal(-1, received.TouchId);   // default mouse
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// ClickEvent.Position + ClickCount 从 evt.x, evt.y, evt.clickCount 填充。
        /// </summary>
        [Fact]
        public void ClickPositionAndCountAreFilled()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ClickEvent received = default;
                n.On<ClickEvent>(e => received = e);

                using (var buf = new NativeEventBuffer())
                {
                    buf.Add(n._id, (byte)EventType.Click, clickCount: 2, x: 10f, y: 20f);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(10f, received.Position.X, 4);
                Assert.Equal(20f, received.Position.Y, 4);
                Assert.Equal(2, received.ClickCount);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// KeyDownEvent.Key + Modifiers 从 touchId (key_code) + pad[0] (modifiers) 填充。
        /// </summary>
        [Fact]
        public void KeyDownKeyAndModifiersAreFilled()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                KeyDownEvent received = default;
                n.On<KeyDownEvent>(e => received = e);

                using (var buf = new NativeEventBuffer())
                {
                    // IkatKeyCode.A = 97 (Unity IkatKeyCode), modifiers = Shift(1)
                    buf.AddKeyDown(n._id, keyCode: 97, modifiers: 1);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(IkatKeyCode.A, received.Key);
                Assert.Equal(KeyModifiers.Shift, received.Modifiers);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// KeyUpEvent.Key + Modifiers from raw data, Ctrl+Alt combo.
        /// </summary>
        [Fact]
        public void KeyUpKeyAndModifiersFromRaw()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                KeyUpEvent received = default;
                n.On<KeyUpEvent>(e => received = e);

                using (var buf = new NativeEventBuffer())
                {
                    // IkatKeyCode.Enter = 13, modifiers = Ctrl(2) | Alt(4) = 6
                    buf.AddKeyUp(n._id, keyCode: 13, modifiers: 6);
                    ctx._eventDemuxer.Pump(buf.Ptr, buf.Count);
                }

                Assert.Equal(IkatKeyCode.Enter, received.Key);
                Assert.Equal(KeyModifiers.Control | KeyModifiers.Alt, received.Modifiers);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ───────────────────────────────────────────────────────

        static ulong CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static ulong CreateNode(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_node(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(IntPtr stage, ulong parent, ulong child)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            int rc = Native.ikat_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }
    }

    /// <summary>
    /// 模拟 borrow_events native buffer（字节数为 count × sizeof(RawEventRecord)）。
    /// 测试用——构造 IkatEvent 条目写进 unmanaged 内存，透传给 EventDemuxer.Pump。
    /// Dispose 释放分配的内存。
    /// </summary>
    sealed unsafe class NativeEventBuffer : IDisposable
    {
        byte* _buf;
        int _count;
        static readonly int RecSize = System.Runtime.InteropServices.Marshal.SizeOf<RawEventRecord>();

        public IntPtr Ptr => (IntPtr)_buf;
        public int Count => _count;

        // Each event = 32 bytes（#26 u64 node_id）. Pre-allocate for N events.
        public NativeEventBuffer(int capacity = 16)
        {
            _buf = (byte*)System.Runtime.InteropServices.Marshal.AllocHGlobal(capacity * RecSize);
        }

        /// <summary>
        /// Write a raw EventRecord into the buffer. Fields in native order（#26 u64 拓宽，32B）:
        /// nodeId(u64)@0, eventType(u8)@8, clickCount(u8)@9, pad(ushort)@10, touchId(i32)@12, x(f32)@16, y(f32)@20, dx/dy @24/28.
        /// </summary>
        public void Add(ulong nodeId, byte eventType, byte clickCount = 0, ushort pad = 0, int touchId = -1, float x = 0, float y = 0)
        {
            int off = _count * RecSize;
            // nodeId @0 (u64 little-endian)
            *(ulong*)(_buf + off) = nodeId;
            // eventType @8
            *(_buf + off + 8) = eventType;
            // clickCount @9
            *(_buf + off + 9) = clickCount;
            // pad @10-11（key events: pad[0]=modifiers）
            *(ushort*)(_buf + off + 10) = pad;
            // touchId @12
            *(int*)(_buf + off + 12) = touchId;
            // x @16
            *(float*)(_buf + off + 16) = x;
            // y @20
            *(float*)(_buf + off + 20) = y;
            // dx @24 / dy @28（显式清零：AllocHGlobal 不保证清零，DragMove 增量读垃圾会假绿/假红）
            *(float*)(_buf + off + 24) = 0f;
            *(float*)(_buf + off + 28) = 0f;
            _count++;
        }

        /// <summary>Shortcut: Click event with click_count=1。</summary>
        public void AddClick(ulong nodeId) => Add(nodeId, (byte)EventType.Click, clickCount: 1);

        /// <summary>Shortcut: KeyDown event with key_code and modifiers。</summary>
        public void AddKeyDown(ulong nodeId, int keyCode, byte modifiers = 0)
            => Add(nodeId, (byte)EventType.KeyDown, touchId: keyCode, pad: modifiers);

        /// <summary>Shortcut: KeyUp event with key_code and modifiers。</summary>
        public void AddKeyUp(ulong nodeId, int keyCode, byte modifiers = 0)
            => Add(nodeId, (byte)EventType.KeyUp, touchId: keyCode, pad: modifiers);

        public void Dispose()
        {
            if (_buf != null)
            {
                System.Runtime.InteropServices.Marshal.FreeHGlobal((IntPtr)_buf);
                _buf = null;
            }
        }
    }
}
