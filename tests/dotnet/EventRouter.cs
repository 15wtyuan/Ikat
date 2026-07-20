using System;
using System.Collections.Generic;

namespace LoomGUI
{
    /// 纯事件路由算法——不依赖 Unity、不依赖 FFI、不自建字典。
    /// 历史用途：曾被生产侧 LoomEventHandler 委托跑路由（Spec-4b P1 已退役 LoomEventHandler）。
    /// 现仅作算法参考实现，headless 测试 EventRouterTests 直接消费——非生产依赖。

    public static class EventRouter
    {
        public const uint NO_PARENT = 0xFFFF_FFFF;

        /// 沿 parent 链收集 [target, ..., root]。sentinel NO_PARENT 止。
        public static List<uint> BuildAncestorChain(uint target, Func<uint, uint> getParent)
        {
            var chain = new List<uint>();
            uint c = target;
            while (c != NO_PARENT) { chain.Add(c); c = getParent(c); }
            return chain;
        }

        /// bubble 类事件：capture(根→target 反向) + bubble(target→根 正向)，stop break。
        /// getCapture/getBubble 负责查指定节点的回调（可能为 null）。
        public static void BubbleRoute(
            uint targetId,
            Func<uint, uint> getParent,
            Func<uint, EventType, EventCallback> getCapture,
            Func<uint, EventType, EventCallback> getBubble,
            EventContext ctx,
            EventType evtType,
            out uint? captureNode, out uint? bubbleNode)
        {
            var chain = BuildAncestorChain(targetId, getParent);
            captureNode = null;
            bubbleNode = null;

            // capture 阶段：根→target 反向，全跑不检查 stop
            for (int i = chain.Count - 1; i >= 0; i--)
            {
                ctx.currentTarget = chain[i];
                ctx.phase = Phase.Capture;
                CallListeners(getCapture(chain[i], evtType), ctx);
                if (ctx._touchCapture) { ctx._touchCapture = false; captureNode = chain[i]; }
            }

            // bubble 阶段：target→根 正向
            if (!ctx._stopsPropagation)
            {
                for (int i = 0; i < chain.Count; i++)
                {
                    ctx.currentTarget = chain[i];
                    ctx.phase = (chain[i] == ctx.target) ? Phase.Target : Phase.Bubble;
                    CallListeners(getBubble(chain[i], evtType), ctx);
                    if (ctx._touchCapture) { ctx._touchCapture = false; bubbleNode = chain[i]; }
                    if (ctx._stopsPropagation) break;
                }
            }
        }

        /// 直派事件：单节点 capture + bubble 回调（不沿链）。
        public static void DirectDispatch(
            uint nodeId,
            Func<uint, EventType, EventCallback> getCapture,
            Func<uint, EventType, EventCallback> getBubble,
            EventContext ctx,
            EventType evtType)
        {
            ctx.currentTarget = nodeId;
            ctx.phase = Phase.Target;
            CallListeners(getCapture(nodeId, evtType), ctx);
            CallListeners(getBubble(nodeId, evtType), ctx);
        }

        static void CallListeners(EventCallback multicast, EventContext ctx)
        {
            if (multicast == null) return;
            foreach (EventCallback cb in multicast.GetInvocationList())
            {
                cb(ctx);
                if (ctx != null && ctx._stopsImmediatePropagation) break;
            }
        }
    }
}
