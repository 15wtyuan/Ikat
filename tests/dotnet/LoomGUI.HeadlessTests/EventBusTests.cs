using System;
using System.Text;
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// D2 投影层事件 typed 订阅层验收：
    /// - <see cref="LoomGUI.EventBus"/>(<c>internal</c>) 持订阅表 <c>Dictionary&lt;(nodeId,eventType,capture), List&lt;HandlerEntry&gt;&gt;</c>。
    /// - <see cref="Node.On{T}"/>(<c>public frozen</c>) → <c>ctx._eventBus.Subscribe&lt;T&gt;</c>，返 <see cref="EventRegistration"/>。
    /// - <see cref="EventRegistration.Dispose"/> 退订（幂等）。
    /// - <see cref="LoomGUI.EventBus.Dispatch{T}"/> 走 capture/bubble 路由 + StopPropagation halt + once auto-remove。
    ///
    /// D2 自己构造 typed event 喂 Dispatch（模拟 D3 demux 翻译 raw LoomEvent → typed struct 后调 Dispatch）。
    /// D3 接线后真实 demux 会替换本测试的手工 evt 构造，但 Dispatch 行为契约不变。
    ///
    /// 全部经 headless harness P/Invoke 真 dll 建 Stage + parent chain（验真 FFI 路径，非 mock）。
    /// </summary>
    public unsafe class EventBusTests
    {
        // lib.rs create_root 失败哨兵（与 parent 哨兵同值）。
        private const ulong InvalidNodeId = ulong.MaxValue;

        // ── 订阅 + 触发 ─────────────────────────────────────────────────

        /// <summary>
        /// On&lt;ClickEvent&gt; 订阅 + Dispatch 同节点 → handler 收到 typed event，Target/CurrentTarget 都填好。
        /// 验：订阅表录入 + Dispatch 找到 entry + 触发 + _core.Target/CurrentTarget 经路由正确填充。
        /// </summary>
        [Fact]
        public void OnSubscribeAndDispatchFiresHandler()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                ClickEvent received = default;
                n.On<ClickEvent>(e => received = e);

                var evt = new ClickEvent { _core = new RouteEventCore { Target = n } };
                ctx._eventBus.Dispatch(n._id, evt);

                Assert.Same(n, received.Target);
                Assert.Same(n, received.CurrentTarget);   // target 节点上：Target == CurrentTarget
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 未订阅的 eventType 不触发——验订阅表 key 含 eventType byte（ClickEvent.EventType != PointerDownEvent.EventType）。
        /// </summary>
        [Fact]
        public void DispatchDoesNotFireOtherEventType()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int clickCount = 0;
                n.On<ClickEvent>(_ => clickCount++);

                // Dispatch PointerDown（不同 EventType byte）→ Click 订阅不该触发。
                var pdown = new PointerDownEvent { _core = new RouteEventCore { Target = n } };
                ctx._eventBus.Dispatch(n._id, pdown);

                Assert.Equal(0, clickCount);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── Dispose 退订 ───────────────────────────────────────────────

        /// <summary>
        /// EventRegistration.Dispose 后再 Dispatch → handler 不触发（订阅已从表移除）。
        /// 验：Dispose 调 unsubscribe Action → Remove 从 list 移除 entry。
        /// </summary>
        [Fact]
        public void DisposeUnsubscribesBeforeDispatch()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int count = 0;
                var reg = n.On<ClickEvent>(_ => count++);
                reg.Dispose();

                DispatchClick(ctx, n);
                Assert.Equal(0, count);   // Dispose 在 Dispatch 前 → 永不触发
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// Dispose 在 handler 触发后调用 → 后续 Dispatch 不再触发。验 Dispose 幂等（二次调 no-op）。
        /// </summary>
        [Fact]
        public void DisposeAfterFireStopsFurtherDispatch()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int count = 0;
                var reg = n.On<ClickEvent>(_ => count++);

                DispatchClick(ctx, n);
                Assert.Equal(1, count);

                reg.Dispose();
                reg.Dispose();   // 幂等：不抛、不重复退订

                DispatchClick(ctx, n);
                Assert.Equal(1, count);   // 第二次 dispatch 不再触发
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── once 语义 ──────────────────────────────────────────────────

        /// <summary>
        /// once:true handler 触发一次后自动退订——二次 Dispatch 不再触发。
        /// 验：once flag 触发后从订阅表移除（"fire once then auto-unsubscribe"）。
        /// </summary>
        [Fact]
        public void OnceFiresOnceOnlyAndAutoRemoves()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int count = 0;
                n.On<ClickEvent>(_ => count++, once: true);

                DispatchClick(ctx, n);
                DispatchClick(ctx, n);
                DispatchClick(ctx, n);

                Assert.Equal(1, count);   // 三次 Dispatch 仅第一次触发
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// once 与普通 handler 共存：once 触发后退订，普通 handler 每次都触发。
        /// 验 once auto-remove 只移除自身，不影响同节点同 eventType 的其它 entry。
        /// </summary>
        [Fact]
        public void OnceRemovesOnlyItself()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int onceCount = 0, normalCount = 0;
                n.On<ClickEvent>(_ => onceCount++, once: true);
                n.On<ClickEvent>(_ => normalCount++);

                DispatchClick(ctx, n);
                DispatchClick(ctx, n);

                Assert.Equal(1, onceCount);
                Assert.Equal(2, normalCount);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── capture/bubble 路由顺序 ────────────────────────────────────

        /// <summary>
        /// 父子树 dispatch 子节点：父节点 capture handler 先于父节点 bubble handler 触发。
        /// 验：capture 阶段（root→target）先于 bubble 阶段（target→root）。
        /// </summary>
        [Fact]
        public void CapturePhaseRunsBeforeBubblePhase()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong parentId = CreateRoot(stage, "div");
                ulong childId = CreateNode(stage, "div");
                AppendChild(stage, parentId, childId);
                Container parent = (Container)ctx._registry.GetOrCreate(parentId);
                Node child = ctx._registry.GetOrCreate(childId);

                var order = new System.Collections.Generic.List<string>();
                parent.On<ClickEvent>(_ => order.Add("parent-capture"), useCapture: true);
                parent.On<ClickEvent>(_ => order.Add("parent-bubble"), useCapture: false);

                var evt = new ClickEvent { _core = new RouteEventCore { Target = child } };
                ctx._eventBus.Dispatch(child._id, evt);

                // child 是 target，parent 是祖先：capture 路径根→target 经过 parent 先，bubble 路径 target→根经过 parent 后。
                Assert.Equal(new[] { "parent-capture", "parent-bubble" }, order);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 三层树 root→parent→child，dispatch child：capture 顺序 root-parent-child，bubble 顺序 child-parent-root。
        /// 验完整 DOM 3 阶段路由（不含 target 区分——target 同时在 capture 末尾和 bubble 开头）。
        /// </summary>
        [Fact]
        public void ThreeTierAncestorChainRoutesInOrder()
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

                var evt = new ClickEvent { _core = new RouteEventCore { Target = leaf } };
                ctx._eventBus.Dispatch(leaf._id, evt);

                Assert.Equal(new[] {
                    "root-c", "mid-c", "leaf-c",   // capture: root → target
                    "leaf-b", "mid-b", "root-b",   // bubble:  target → root
                }, order);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// CurrentTarget 在路由过程中正确刷新：父节点 capture handler 看到 CurrentTarget=父，bubble handler 同。
        /// Target 全程不变（= 命中节点）。
        /// </summary>
        [Fact]
        public void CurrentTargetUpdatesPerPhaseTargetStaysConstant()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong parentId = CreateRoot(stage, "div");
                ulong childId = CreateNode(stage, "div");
                AppendChild(stage, parentId, childId);
                Container parent = (Container)ctx._registry.GetOrCreate(parentId);
                Node child = ctx._registry.GetOrCreate(childId);

                Node capTarget = null, capCurrent = null, bubTarget = null, bubCurrent = null;
                parent.On<ClickEvent>(e => { capTarget = e.Target; capCurrent = e.CurrentTarget; }, useCapture: true);
                parent.On<ClickEvent>(e => { bubTarget = e.Target; bubCurrent = e.CurrentTarget; }, useCapture: false);

                var evt = new ClickEvent { _core = new RouteEventCore { Target = child } };
                ctx._eventBus.Dispatch(child._id, evt);

                Assert.Same(child, capTarget);
                Assert.Same(parent, capCurrent);   // 路由到 parent 时 CurrentTarget = parent
                Assert.Same(child, bubTarget);
                Assert.Same(parent, bubCurrent);
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── StopPropagation halt ───────────────────────────────────────

        /// <summary>
        /// 父节点 bubble handler 调 StopPropagation → 祖先节点 bubble handler 不触发。
        /// 验：bubble 路由循环看到 _propagationStopped 后 break。
        /// </summary>
        [Fact]
        public void StopPropagationInBubbleHaltsFurtherBubble()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(stage, "div");
                ulong childId = CreateNode(stage, "div");
                AppendChild(stage, rootId, childId);
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                Node child = ctx._registry.GetOrCreate(childId);

                bool rootBubbleFired = false;
                // child（target）先 bubble：调 StopPropagation 止住上传。
                child.On<ClickEvent>(e => e.StopPropagation());
                root.On<ClickEvent>(_ => rootBubbleFired = true);

                var evt = new ClickEvent { _core = new RouteEventCore { Target = child } };
                ctx._eventBus.Dispatch(child._id, evt);

                Assert.False(rootBubbleFired);   // child 止传 → root 不触发
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// capture 阶段调 StopPropagation → bubble 阶段全部跳过。
        /// 对齐 EventRouter.cs：bubble 循环前检查 _stopsPropagation flag。
        /// </summary>
        [Fact]
        public void StopPropagationInCaptureSkipsBubbleEntirely()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                ulong rootId = CreateRoot(stage, "div");
                ulong childId = CreateNode(stage, "div");
                AppendChild(stage, rootId, childId);
                Container root = (Container)ctx._registry.GetOrCreate(rootId);
                Node child = ctx._registry.GetOrCreate(childId);

                bool childBubbleFired = false;
                root.On<ClickEvent>(e => e.StopPropagation(), useCapture: true);   // 祖先 capture 止传
                child.On<ClickEvent>(_ => childBubbleFired = true);                // target bubble 不该触发

                var evt = new ClickEvent { _core = new RouteEventCore { Target = child } };
                ctx._eventBus.Dispatch(child._id, evt);

                Assert.False(childBubbleFired);
            }
            finally { StageHarness.Destroy(stage); }
        }

        /// <summary>
        /// 节点上无订阅者的 dispatch → 静默 no-op（不抛、不影响别的节点）。
        /// </summary>
        [Fact]
        public void DispatchWithNoSubscribersIsSilentNoOp()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                var evt = new ClickEvent { _core = new RouteEventCore { Target = n } };
                ctx._eventBus.Dispatch(n._id, evt);   // 不抛即可
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── dispatch 期间同步 Dispose ───────────────────────────────────

        /// <summary>
        /// handler A 在 dispatch 期间同步 Dispose handler B 的 EventRegistration → B 的 handler
        /// 不触发。验：Remove 置 entry.IsDisposed=true，InvokeHandlers 循环到 B 的 snapshot entry
        /// 时跳过（"Dispose 后不再触发"契约）。snapshot 防 list 边遍边改，但 snapshot 内含已 Dispose
        /// 的 entry 仍需 IsDisposed flag 拦截——Remove 的 MarkDisposed 是关键。
        /// </summary>
        [Fact]
        public void DisposingOtherHandlersRegistrationDuringDispatchSkipsIt()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                int bCount = 0;
                EventRegistration regB = null;
                // 注册顺序决定 snapshot 顺序——A 在 B 之前注册，A 先触发。A 闭包捕获 regB 引用变量，
                // dispatch 时 regB 已被 B 的注册赋值。
                n.On<ClickEvent>(_ => regB.Dispose());             // A：Dispose B 的 reg
                regB = n.On<ClickEvent>(_ => bCount++);            // B：被同步 Dispose

                DispatchClick(ctx, n);

                Assert.Equal(0, bCount);   // B 的 IsDisposed=true → snapshot 循环跳过
            }
            finally { StageHarness.Destroy(stage); }
        }

        // ── helpers ─────────────────────────────────────────────────────

        static void DispatchClick(UIContext ctx, Node target)
        {
            var evt = new ClickEvent { _core = new RouteEventCore { Target = target } };
            ctx._eventBus.Dispatch(target._id, evt);
        }

        static ulong CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }

        static ulong CreateNode(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.loomgui_stage_create_node(h, kp, (nuint)k.Length, null, 0);
        }

        static void AppendChild(IntPtr stage, ulong parent, ulong child)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            int rc = Native.loomgui_stage_append_child(h, parent, child);
            if (rc != 0)
                throw new InvalidOperationException($"append_child(parent={parent}, child={child}) failed rc={rc}");
        }
    }
}
