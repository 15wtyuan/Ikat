// EventBus：typed 事件订阅表 + capture/bubble/once 路由（投影层 D2）。
//
// 设计契约（spec §3.4 / public-api §5）：
// - 订阅表 key = (NodeId, EventType byte, capture flag)——EventType byte 来自 D1 每个 typed
//   event struct 的 `internal static byte EventType` 属性（ClickEvent.EventType 等）。
// - Dispatch<T>（D3 调）走 DOM 3 阶段路由：capture（root→target）→ bubble（target→root），
//   每节点查 capture=true / capture=false 订阅，触发后 once 自动退订。
// - StopPropagation 经 RouteEventCore._propagationStopped：bubble 循环前 pre-check（capture 止传
//   则 bubble 全跳）+ 循环内 break（target 止传则止上传）。
//
// 关键不变量：
// - 同一 NodeId 同一 EventType 同一 capture flag 的多次订阅都触发（list 顺序 = 注册顺序）。
// - once 只移除自身，不影响同 list 的其它 entry。
// - EventRegistration.Dispose 调闭包内的 Remove 从 list 移除 entry；list 空则移 key。
//
// D3 接线契约：D3 翻译 raw LoomEvent → typed struct（填 _core.Target = registry.GetOrCreate(nodeId)）
// 后调 Dispatch<T>(targetNodeId, evt)；EventBus 负责 ancestor chain 走 + capture/bubble 路由。

using System;
using System.Collections.Generic;
using System.Reflection;
using LoomGUI.Bindings;

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：typed 事件订阅 + DOM 3 阶段路由。
    /// <see cref="UIContext"/> 持单实例；<see cref="Node.On{T}"/> 经 <c>_ctx._eventBus.Subscribe</c>
    /// 录订阅；D3 demux 经 <see cref="Dispatch{T}"/> 触发。
    /// </summary>
    internal sealed unsafe class EventBus
    {
        readonly UIContext _ctx;

        // 订阅表：同 (node, type, capture) 三元可多 entry（list 保序）。空 list 移 key 防 dict 膨胀。
        readonly Dictionary<(uint nodeId, byte eventType, bool capture), List<IHandlerEntry>> _subs
            = new();

        internal EventBus(UIContext ctx) => _ctx = ctx;

        /// <summary>
        /// 订阅 typed handler。<paramref name="capture"/> = true 时进 capture 阶段订阅表，
        /// false 进 bubble 阶段。<paramref name="once"/> = true 触发后自动退订（防"等一个结束事件"泄漏）。
        /// 返 <see cref="EventRegistration"/>——Dispose 退订。
        /// </summary>
        internal EventRegistration Subscribe<T>(uint nodeId, Action<T> handler, bool capture, bool once)
            where T : IRouteEvent
        {
            // T.EventType 来自 D1 per-struct static 关联（ClickEvent.EventType 等）。泛型无法直接
            // T.EventType（C# 静态成员不进接口约束，除非用 static abstract），经 EventTypeCache<T>
            // 反射读一次并 cache 到泛型静态字段——后续 Dispatch<T>/Subscribe<T> 零反射开销。
            byte eventType = EventTypeCache<T>.Value;
            var key = (nodeId, eventType, capture);
            var entry = new HandlerEntry<T>(handler, once);
            if (!_subs.TryGetValue(key, out var list))
            {
                list = new List<IHandlerEntry>();
                _subs[key] = list;
            }
            list.Add(entry);

            // EventRegistration 持退订闭包：调 Remove(key, entry)。同一 reg 多次 Dispose 幂等
            // （EventRegistration 内 _disposed flag 拦二次调，本闭包不会被重复调）。
            return new EventRegistration(() => Remove(key, entry));
        }

        /// <summary>
        /// Dispatch typed event 走 DOM 3 阶段路由。D3 喂已构造好的 evt（_core.Target 由 D3 填）；
        /// EventBus 走 ancestor chain + 每节点查订阅表 + 触发 + once 退订。
        ///
        /// 路由算法对齐 <c>tests/dotnet/EventRouter.cs</c>（纯 managed 路由参考实现）：
        /// capture 阶段 root→target 全跑（不检 stop）；bubble 阶段 target→root——pre-check stop
        /// flag 决定是否进 bubble，循环内 stop 即 break。target 节点同时在 capture 末尾和 bubble 开头
        /// 出现，capture-listener 和 bubble-listener 都触发（DOM target 阶段等价）。
        /// </summary>
        /// <typeparam name="T">typed event struct（D1 的 18 个之一）。</typeparam>
        /// <param name="targetNodeId">命中节点 NodeId（dispatch 全程 Target 不变）。</param>
        /// <param name="evt">typed event——_core.Target 必须已由调用方（D3 / 测试）填。</param>
        internal void Dispatch<T>(uint targetNodeId, T evt) where T : IRouteEvent, IRouteEventCore
        {
            // D1 契约：每个 typed event struct 持 RouteEventCore _core。RouteEventCore 是
            // sealed class（D2 修订：struct 版下 Action<T> 按值传 handler，StopPropagation 突变副本
            // 不回传路由循环）——_core 字段是引用槽，handler 副本与 Dispatch 局部 evt 共享同一堆实例。
            //
            // evt.Core 经约束泛型调用读共享 core 引用（JIT 直呼 struct 实现，零装箱）——
            // CurrentTarget 每节点刷新、_propagationStopped 由 handler 经 StopPropagation
            // 写入同一堆实例传播。
            // 历史坑：曾用 Unsafe.As<T, RouteEventCore> 别名首 field——Unity 2021.3 Mono
            // corlib 无 Unsafe 类（编译不过）；换 __refvalue（refanyval）编译过但 Mono 运行时
            // 校验 TypedReference 类型不符即抛 InvalidCastException（PlayMode 首个事件即炸，
            // EditMode 不派发事件测不到）。接口约束调用是唯一零分配且跨版本语义一致的路径。
            RouteEventCore core = evt.Core;
            byte eventType = EventTypeCache<T>.Value;

            // Build ancestor chain [target, ..., root]：逐层 node_parent 上溯直到 RootSentinel。
            // 同 IsInSubtree 风格的 10k 防御上限（围栏闭合下树深有界，10k 兜底）。
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            // 链容量预估 8（典型 UI 深度）；深度大时 List 自动扩容。
            var chain = new List<uint>(8) { targetNodeId };
            uint current = targetNodeId;
            for (int i = 0; i < 10_000; i++)
            {
                uint parent = Native.loomgui_node_parent(h, current);
                if (parent == Node.RootSentinel) break;   // 走出根 / target 不 live
                if (parent == current) break;             // 防御：自循环（理论不达）
                chain.Add(parent);
                current = parent;
            }

            // 预物化 chain 上所有节点：dispatch 入口整条路径 live（NewCore 已确认 target live，此处同步
            // 无 Dispose），GetOrCreate 必成功且 wrapper 入 registry 缓存。之后 capture/bubble 不再造节点，
            // 改用 IsLive 判活——handler 可能在路由途中 Dispose 路径节点（关面板 / 切页 / 删 item 等 DOM
            // 合法操作），Dispose 经 registry.Remove 移缓存 + 标 _disposed，IsLive 即此判据。
            foreach (uint nid in chain) _ctx._registry.GetOrCreate(nid);

            // ── capture 阶段：root → target（chain 反向遍历，不检 stop——对齐 EventRouter.cs）。
            for (int i = chain.Count - 1; i >= 0; i--)
            {
                uint nodeId = chain[i];
                if (!IsLive(nodeId, out var node)) continue;   // 路由中被 Dispose → 跳过（DOM：移除节点不再触发）
                core.CurrentTarget = node;
                InvokeHandlers(nodeId, eventType, capture: true, ref evt);
            }

            // ── bubble 阶段：target → root（chain 正向）。
            // pre-check：capture 阶段若已 StopPropagation，bubble 全跳（对齐 EventRouter.cs）。
            if (core._propagationStopped) return;

            for (int i = 0; i < chain.Count; i++)
            {
                uint nodeId = chain[i];
                if (!IsLive(nodeId, out var node)) continue;
                core.CurrentTarget = node;
                InvokeHandlers(nodeId, eventType, capture: false, ref evt);
                if (core._propagationStopped) break;
            }
        }

        /// <summary>
        /// Dispatch 非冒泡事件（DOM mouseenter/mouseleave 语义）：只触发 target 节点自身的
        /// 订阅（capture 与 bubble 两张表在 target 上都属 target 阶段），不沿祖先链路由。
        ///
        /// Enter/Leave（core RollOver/RollOut）必须走本路径：core 按悬停链差分**逐节点**
        /// 发射——进出边界的每个节点各得一条自己的事件，语义已完备。若再沿祖先链冒泡，
        /// 「后代退出悬停链」会误投给祖先订阅——指针仍在祖先子树内、祖先并未离链，
        /// 祖先级 hover 处理器被误触发；与按 enter/leave 切换视觉态（抬升动画改命中几何）
        /// 的用法叠加会形成 enter→leave 自激振荡（悬停风暴）。
        /// </summary>
        /// <typeparam name="T">typed event struct。</typeparam>
        /// <param name="targetNodeId">事件目标节点 NodeId。</param>
        /// <param name="evt">typed event——_core.Target 必须已由调用方（D3 / 测试）填。</param>
        internal void DispatchTargetOnly<T>(uint targetNodeId, T evt) where T : IRouteEvent, IRouteEventCore
        {
            RouteEventCore core = evt.Core;
            byte eventType = EventTypeCache<T>.Value;
            if (!IsLive(targetNodeId, out var node)) return;
            core.CurrentTarget = node;
            InvokeHandlers(targetNodeId, eventType, capture: true, ref evt);
            InvokeHandlers(targetNodeId, eventType, capture: false, ref evt);
        }

        /// <summary>
        /// chain 节点是否仍 live。预物化后 live 节点必在 registry 缓存；Dispose 经 registry.Remove 移缓存
        /// + 标 _disposed，故 TryGet 未命中或 _disposed=true 即 not-live——dispatch 路由跳过它，对齐 DOM
        /// 「事件派发中移除的节点不再触发 listener」。out 节点供 CurrentTarget 赋值；not-live 时 null
        /// （调用方 continue 不使用）。
        /// </summary>
        bool IsLive(uint nodeId, out Node node)
        {
            if (_ctx._registry.TryGet(nodeId, out node) && !node._disposed) return true;
            node = null;
            return false;
        }

        /// <summary>
        /// 触发指定 (nodeId, eventType, capture) 上的全部订阅。snapshot list 防 handler 内 Dispose
        /// 或 once auto-remove 改 list 边遍历边改。once entry 触发后收集 → 循环后统一移除。
        /// </summary>
        void InvokeHandlers<T>(uint nodeId, byte eventType, bool capture, ref T evt)
        {
            var key = (nodeId, eventType, capture);
            if (!_subs.TryGetValue(key, out var list) || list.Count == 0) return;

            // snapshot 防 list mutation（once auto-remove / handler 内 Dispose 改 list）边遍边改。
            // 测试场景 list 通常 ≤ 几个 entry，ToArray 开销可忽略；热路径优化推后（roadmap D3+ 性能 tuning）。
            IHandlerEntry[] snapshot = list.ToArray();
            List<IHandlerEntry> toRemove = null;

            for (int i = 0; i < snapshot.Length; i++)
            {
                var entry = snapshot[i];
                // 已 Dispose 的 entry（handler 触发前可能被前面 entry 同步 Dispose）跳过——
                // 健壮性兜底，理论场景：handler A 内调 handler B 的 reg.Dispose。
                if (entry.IsDisposed) continue;

                ((HandlerEntry<T>)entry).Invoke(ref evt);

                if (entry.Once)
                {
                    (toRemove ??= new List<IHandlerEntry>()).Add(entry);
                }
            }

            if (toRemove != null)
            {
                for (int i = 0; i < toRemove.Count; i++) list.Remove(toRemove[i]);
                if (list.Count == 0) _subs.Remove(key);
            }
        }

        /// <summary>
        /// 从订阅表移除 entry。list 空则移 key（防 dict 膨胀）。
        /// 由 EventRegistration.Dispose 经闭包调；幂等（多次移同一 entry 后续 no-op）。
        /// 同步置 entry.IsDisposed=true——Dispatch 的 snapshot 可能含 dispatch 过程中被
        /// 同步 Dispose 的 entry（如 handler A 内 Dispose handler B 的 reg），IsDisposed flag
        /// 让 InvokeHandlers 在循环到该 entry 时跳过（"Dispose 后不再触发"契约）。
        /// </summary>
        void Remove((uint nodeId, byte eventType, bool capture) key, IHandlerEntry entry)
        {
            // 先置 flag 再移 list——flag 是 Dispatch snapshot 跳过判据，list.Remove 仅做表清理。
            entry.MarkDisposed();
            if (_subs.TryGetValue(key, out var list))
            {
                list.Remove(entry);
                if (list.Count == 0) _subs.Remove(key);
            }
        }

        // ── 内部：HandlerEntry 类型化回调 + once flag ─────────────────────

        interface IHandlerEntry
        {
            bool Once { get; }
            bool IsDisposed { get; }
            /// <summary>
            /// 置 IsDisposed=true。EventRegistration.Dispose 经 Remove 调——Dispatch snapshot
            /// 含 dispatch 过程中被同步 Dispose 的 entry 时跳过该 entry（"Dispose 后不再触发"契约）。
            /// </summary>
            void MarkDisposed();
        }

        /// <summary>
        /// typed handler + once flag。<see cref="Invoke"/> 转调 <see cref="_handler"/>；
        /// once 触发后由 <see cref="EventBus.InvokeHandlers{T}"/> 收集移除。
        /// <see cref="IsDisposed"/> 标记：EventRegistration.Dispose 经 <see cref="Remove"/>
        /// 调 <see cref="MarkDisposed"/> 置 true——dispatch 时 snapshot 内的已退订 entry 跳过
        /// （handler 触发前可能被前面 entry 同步 Dispose——如 handler A 内 Dispose handler B 的 reg）。
        /// </summary>
        sealed class HandlerEntry<T> : IHandlerEntry
        {
            readonly Action<T> _handler;
            public bool Once { get; }
            public bool IsDisposed { get; private set; }

            internal HandlerEntry(Action<T> handler, bool once)
            {
                _handler = handler;
                Once = once;
            }

            public void MarkDisposed() => IsDisposed = true;

            internal void Invoke(ref T evt) => _handler(evt);
        }

        // ── EventTypeCache<T>：D1 per-struct static `EventType` byte 的反射 cache ──────────
        //
        // 泛型 Subscribe<T>/Dispatch<T> 无法直接 T.EventType（C# 静态成员不进 IRouteEvent 约束）。
        // 用泛型静态类做"per-T 一次性反射 + cache"——CLR 对每个封闭类型（EventTypeCache<ClickEvent> 等）
        // 跑静态 ctor 一次，之后 EventTypeCache<T>.Value 是直接字段读，零反射。

        private static class EventTypeCache<T> where T : IRouteEvent
        {
            public static readonly byte Value = Resolve();

            static byte Resolve()
            {
                // D1 契约：每个 typed event struct 有 internal static byte EventType 属性。
                var p = typeof(T).GetProperty(
                    "EventType", BindingFlags.Static | BindingFlags.NonPublic);
                if (p == null)
                    throw new InvalidOperationException(
                        $"typed event {typeof(T).Name} missing internal static byte EventType " +
                        "(D1 contract: each IRouteEvent struct declares it as D2 subscription key)");

                if (!typeof(T).IsValueType)
                    throw new InvalidOperationException(
                        $"event {typeof(T).Name} must be a struct (D1 contract)");

                return (byte)p.GetValue(null);
            }
        }
    }
}
