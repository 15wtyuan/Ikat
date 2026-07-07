using System;
using System.Collections.Generic;
using Xunit;

namespace LoomGUI.Tests.Core
{
    public class EventRouterTests
    {
        // 构造 parent 表：nodeId → parentId。root 映射到 NO_PARENT。
        static Func<uint, uint> ParentLookup(Dictionary<uint, uint> parentMap)
            => id => parentMap.TryGetValue(id, out var p) ? p : EventRouter.NO_PARENT;

        [Fact]
        public void BuildAncestorChain_RootOnly_ReturnsRoot()
        {
            var chain = EventRouter.BuildAncestorChain(1, ParentLookup(new() { { 1, EventRouter.NO_PARENT } }));
            Assert.Equal([1u], chain);
        }

        [Fact]
        public void BuildAncestorChain_ChildToRoot_ReturnsFullChain()
        {
            var map = new Dictionary<uint, uint> { { 3, 2 }, { 2, 1 }, { 1, EventRouter.NO_PARENT } };
            var chain = EventRouter.BuildAncestorChain(3, ParentLookup(map));
            Assert.Equal([3u, 2u, 1u], chain);
        }

        [Fact]
        public void BubbleRoute_CaptureFiresReverseOrder()
        {
            var map = new Dictionary<uint, uint> { { 3, 2 }, { 2, 1 }, { 1, EventRouter.NO_PARENT } };
            var log = new List<(uint, Phase)>();
            EventCallback Make(uint n) => ctx => log.Add((n, ctx.phase));

            var ctx = EventContext.Get();
            ctx.target = 3;
            EventRouter.BubbleRoute(3, ParentLookup(map),
                (n, t) => t == EventType.Click ? Make(n) : null,
                (n, t) => null,
                ctx, EventType.Click, out _, out _);

            // capture: root(1)→parent(2)→target(3)
            Assert.Equal(3, log.Count);
            Assert.Equal(Phase.Capture, log[0].Item2); Assert.Equal(1u, log[0].Item1);
            Assert.Equal(Phase.Capture, log[1].Item2); Assert.Equal(2u, log[1].Item1);
            Assert.Equal(Phase.Capture, log[2].Item2); Assert.Equal(3u, log[2].Item1);
            EventContext.Return(ctx);
        }

        [Fact]
        public void BubbleRoute_BubbleFiresForwardOrder()
        {
            var map = new Dictionary<uint, uint> { { 3, 2 }, { 2, 1 }, { 1, EventRouter.NO_PARENT } };
            var log = new List<(uint, Phase)>();
            EventCallback Make(uint n) => ctx => log.Add((n, ctx.phase));

            var ctx = EventContext.Get();
            ctx.target = 3;
            EventRouter.BubbleRoute(3, ParentLookup(map),
                (n, t) => null,
                (n, t) => t == EventType.Click ? Make(n) : null,
                ctx, EventType.Click, out _, out _);

            // bubble: target(3)→parent(2)→root(1); target phase = Target, others = Bubble
            Assert.Equal(3, log.Count);
            Assert.Equal(Phase.Target, log[0].Item2); Assert.Equal(3u, log[0].Item1);
            Assert.Equal(Phase.Bubble, log[1].Item2); Assert.Equal(2u, log[1].Item1);
            Assert.Equal(Phase.Bubble, log[2].Item2); Assert.Equal(1u, log[2].Item1);
            EventContext.Return(ctx);
        }

        [Fact]
        public void BubbleRoute_StopPropagation_StopsAtCurrentNode()
        {
            var map = new Dictionary<uint, uint> { { 3, 2 }, { 2, 1 }, { 1, EventRouter.NO_PARENT } };
            var log = new List<(uint, Phase)>();
            EventCallback MakeStop(uint n) => ctx => {
                log.Add((n, ctx.phase));
                if (n == 2) ctx.StopPropagation();
            };

            var ctx = EventContext.Get();
            ctx.target = 3;
            EventRouter.BubbleRoute(3, ParentLookup(map),
                (n, t) => null,
                (n, t) => t == EventType.Click ? MakeStop(n) : null,
                ctx, EventType.Click, out _, out _);

            Assert.Equal(2, log.Count);  // 3, 2 — root(1) 没收到
            Assert.Equal(3u, log[0].Item1);
            Assert.Equal(2u, log[1].Item1);
            EventContext.Return(ctx);
        }

        [Fact]
        public void BubbleRoute_StopImmediatePropagation_SkipsRemainingOnSameNode()
        {
            var map = new Dictionary<uint, uint> { { 2, 1 }, { 1, EventRouter.NO_PARENT } };
            var log = new List<int>();

            // 在节点 2 上挂两个 bubble 回调：第0个设 stopImmediate，第1个不该被调
            EventCallback combined = null;
            combined = (EventCallback)Delegate.Combine(
                combined,
                (EventCallback)(ctx => { log.Add(0); ctx.StopImmediatePropagation(); }),
                (EventCallback)(ctx => { log.Add(1); })
            );

            var ctx = EventContext.Get();
            ctx.target = 2;
            EventRouter.BubbleRoute(2, ParentLookup(map),
                (n, t) => null,
                (n, t) => t == EventType.Click ? combined : null,
                ctx, EventType.Click, out _, out _);

            Assert.Equal([0], log);  // 只调了第一个
            EventContext.Return(ctx);
        }

        [Fact]
        public void DirectDispatch_FiresBothCaptureAndBubble()
        {
            var log = new List<(uint, Phase)>();
            EventCallback Make(uint n) => ctx => log.Add((n, ctx.phase));

            var ctx = EventContext.Get();
            ctx.target = 5;
            EventRouter.DirectDispatch(5,
                (n, t) => t == EventType.RollOver ? Make(n) : null,
                (n, t) => t == EventType.RollOver ? Make(n) : null,
                ctx, EventType.RollOver);

            Assert.Equal(2, log.Count);
            Assert.Equal(Phase.Target, log[0].Item2);
            Assert.Equal(Phase.Target, log[1].Item2);
            EventContext.Return(ctx);
        }

        [Fact]
        public void BubbleRoute_NoListeners_DoesNotCrash()
        {
            var map = new Dictionary<uint, uint> { { 1, EventRouter.NO_PARENT } };
            var ctx = EventContext.Get();
            ctx.target = 1;
            EventRouter.BubbleRoute(1, ParentLookup(map),
                (n, t) => null, (n, t) => null,
                ctx, EventType.Click, out _, out _);
            EventContext.Return(ctx);
        }

        [Fact]
        public void BubbleRoute_CaptureTouch_FlagsReturned()
        {
            var map = new Dictionary<uint, uint> { { 2, 1 }, { 1, EventRouter.NO_PARENT } };
            EventCallback MakeCapture(uint n) => ctx => ctx.CaptureTouch();

            var ctx = EventContext.Get();
            ctx.target = 2;
            EventRouter.BubbleRoute(2, ParentLookup(map),
                (n, t) => t == EventType.Down ? MakeCapture(n) : null,
                (n, t) => null,
                ctx, EventType.Down, out var capNode, out var bubNode);

            Assert.True(capNode.HasValue || bubNode.HasValue,
                "至少一个阶段消费了 CaptureTouch");
            EventContext.Return(ctx);
        }
    }
}
