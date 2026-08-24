using NUnit.Framework;
using UnityEngine;
using LoomGUI;

namespace LoomGUI.Tests
{
    /// EventBus.Dispatch 的 EditMode 回归：core 引用提取 + 路由 + StopPropagation 传播。
    /// 历史坑（本测试防复发）：Dispatch 曾用 Unsafe.As / __refvalue 别名 struct 首 field——
    /// Unsafe 类 Unity 2021.3 Mono corlib 没有（编译不过）；__refvalue（refanyval）编译过
    /// 但 Mono 运行时校验 TypedReference 类型不符即抛 InvalidCastException（PlayMode 首个
    /// 事件即炸，EditMode 不派发事件测不到）。本测试让 Dispatch 全路径在 EditMode 跑起来。
    public class EventBusDispatchTests
    {
        /// 测试用 typed event struct：遵守事件路由契约（IRouteEvent + IRouteEventCore +
        /// internal static byte EventType）。
        internal struct TestRouteEvent : IRouteEvent, IRouteEventCore
        {
            internal RouteEventCore _core;
            public Node Target => _core.Target;
            public Node CurrentTarget => _core.CurrentTarget;
            public bool DefaultPrevented => _core._defaultPrevented;
            public bool PropagationStopped => _core._propagationStopped;
            public void StopPropagation() => _core.StopPropagation();
            public void PreventDefault() => _core.PreventDefault();
            internal static byte EventType => 200;
            RouteEventCore IRouteEventCore.Core => _core;
        }

        [Test]
        public void Dispatch_ExtractsCore_RoutesAndPropagatesStop()
        {
            var go = new GameObject("eventbus_dispatch_test");
            try
            {
                var driver = go.AddComponent<LoomStageDriver>();
                var ctx = driver.Host.Context;
                Assert.AreNotEqual(Node.RootSentinel, ctx._rootId,
                    "Awake must have created scene root");
                var bus = ctx._eventBus;

                bool captureFired = false, bubbleFired = false;
                using (bus.Subscribe<TestRouteEvent>(ctx._rootId,
                           e => { captureFired = true; e.StopPropagation(); }, true, false))
                using (bus.Subscribe<TestRouteEvent>(ctx._rootId,
                           e => bubbleFired = true, false, false))
                {
                    bus.Dispatch(ctx._rootId, new TestRouteEvent { _core = new RouteEventCore() });
                }

                Assert.IsTrue(captureFired, "capture handler on target must fire");
                Assert.IsFalse(bubbleFired,
                    "capture 阶段 StopPropagation 必须经共享 core 传播到 bubble pre-check（" +
                    "core 提取错误 / struct 副本断裂都会让它失效）");
            }
            finally
            {
                Object.DestroyImmediate(go);
                var cam = GameObject.Find("LoomUICamera");
                if (cam != null) Object.DestroyImmediate(cam);
            }
        }

        /// <summary>
        /// Enter/Leave（RollOver/RollOut）必须 target-only 派发：core 按悬停链差分逐节点
        /// 发射，祖先冒泡会把「后代退链」误投给祖先订阅——指针仍在祖先子树内，祖先级
        /// hover 处理器被误触发（历史缺陷：与 enter/leave 驱动的抬升动画叠加成自激振荡，
        /// 悬停态在卡内闪烁/消失）。回归断言：祖先（capture 与 bubble 两种订阅）不收
        /// 后代 target 的 Leave；target 自身正常收；对照组——冒泡事件经 Dispatch 仍达祖先。
        /// </summary>
        [Test]
        public void DispatchTargetOnly_EnterLeave_DoesNotReachAncestors()
        {
            var go = new GameObject("eventbus_target_only_test");
            try
            {
                var driver = go.AddComponent<LoomStageDriver>();
                var ctx = driver.Host.Context;
                var bus = ctx._eventBus;

                var parent = ctx.Create<Container>();
                var child = ctx.Create<Container>();
                parent.AddChild(child);

                bool parentBubble = false, parentCapture = false, childFired = false;
                using (bus.Subscribe<PointerLeaveEvent>(parent._id,
                           _ => parentBubble = true, false, false))
                using (bus.Subscribe<PointerLeaveEvent>(parent._id,
                           _ => parentCapture = true, true, false))
                using (bus.Subscribe<PointerLeaveEvent>(child._id,
                           _ => childFired = true, false, false))
                {
                    var evt = new PointerLeaveEvent
                        { _core = new RouteEventCore { Target = child } };
                    bus.DispatchTargetOnly(child._id, evt);
                }

                Assert.IsTrue(childFired, "target 自身的订阅必须收到自己的 Leave");
                Assert.IsFalse(parentBubble, "祖先 bubble 订阅不得收到后代 target 的 Leave（链差分语义）");
                Assert.IsFalse(parentCapture, "祖先 capture 订阅不得收到后代 target 的 Leave");

                // 对照组：冒泡事件（DOM mouseleave 之外的事件族）仍沿链到祖先。
                bool ancestorGotBubbling = false;
                using (bus.Subscribe<TestRouteEvent>(parent._id,
                           _ => ancestorGotBubbling = true, false, false))
                {
                    bus.Dispatch(child._id, new TestRouteEvent
                        { _core = new RouteEventCore { Target = child } });
                }
                Assert.IsTrue(ancestorGotBubbling, "冒泡事件经 Dispatch 仍须达祖先（本测试只豁免 Enter/Leave）");
            }
            finally
            {
                Object.DestroyImmediate(go);
                var cam = GameObject.Find("LoomUICamera");
                if (cam != null) Object.DestroyImmediate(cam);
            }
        }
    }
}
