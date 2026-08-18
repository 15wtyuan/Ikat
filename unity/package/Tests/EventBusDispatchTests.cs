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
        /// 测试用 typed event struct：遵守 D1 契约（IRouteEvent + IRouteEventCore +
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
    }
}
