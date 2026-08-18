// RouteEventCore：typed event struct 的共享核心状态。
//
// 设计：sealed class（非 struct）—— D2 发现：Action<T> 按值传 struct，handler 拿到的是副本，
// 其上的 StopPropagation/PreventDefault 突变无法回传 Dispatch 的路由循环。改 class 后 typed
// event struct 的 _core 字段是引用，handler 副本与 Dispatch 局部 evt 共享同一 RouteEventCore
// 堆实例——突变经共享堆对象传播，DOM event.stopPropagation() 语义对齐。
//
// sealed：防派生。class 默认 LayoutKind.Auto——本类自身字段顺序对投影层不透明（投影层不
// 别名读 RouteEventCore，只经方法调用）。
//
// ⚠️ 暴露契约（EventBus.Dispatch 依赖）：每个 typed event struct（ClickEvent 等）实现
// IRouteEventCore.Core 返回 _core 引用，Dispatch<T> 经约束泛型调用取引用（零装箱）。
// 历史坑：曾用 Unsafe.As/__refvalue 别名 struct 首 field——Unsafe 类 Unity 2021.3 Mono
// corlib 没有（编译不过）；__refvalue（refanyval）Mono 运行时校验 TypedReference 类型
// 不符即抛 InvalidCastException。接口约束调用是唯一干净路径，struct 字段顺序不再受约束。

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：路由事件核心状态。每个 typed event struct（<see cref="ClickEvent"/>/
    /// <see cref="PointerDownEvent"/>/…）持一个 <c>RouteEventCore _core</c> 引用字段，把
    /// <see cref="IRouteEvent"/> 的 6 成员（Target/CurrentTarget/DefaultPrevented/
    /// PropagationStopped/StopPropagation/PreventDefault）转发给它——避免 18 个 struct 各写
    /// 一份相同实现（DRY）。
    ///
    /// 设计契约（spec §3.4）：
    /// - <see cref="Target"/> = 命中节点（dispatch 时由 D3 demux 从 <c>LoomEvent.nodeId</c>
    ///   经 <c>NodeRegistry</c> 翻译填入，一次 dispatch 不变）。
    /// - <see cref="CurrentTarget"/> = 当前路由到的节点（capture/bubble 各祖先节点依次刷新，
    ///   D2 填）。DOM/W3C 三阶段模型对齐。
    /// - <see cref="StopPropagation"/> 设 <c>_propagationStopped</c> → <see cref="EventBus"/>
    ///   路由循环 break（target→root 冒泡止；capture 阶段设则 bubble 全跳）。
    /// - <see cref="PreventDefault"/> 设 <c>_defaultPrevented</c> → 语义糖（如 <c>Clicked</c>）
    ///   收到后取消默认行为（具体语义由 D3 接线时按事件类型定）。
    ///
    /// <c>internal sealed class</c>：不出现在公共 API 表面；event struct 的 <c>_core</c> 字段
    /// 也是 <c>internal</c>，仅供投影层内部 set（D2 EventBus、D3 demux）。引用类型是 D2 决策——
    /// handler 收到 typed event struct 副本后突变 _core 经共享堆对象传播，匹配 DOM 事件可突变语义。
    /// </summary>
    internal sealed class RouteEventCore
    {
        /// <summary>命中节点（dispatch 全程不变；DOM event.target 对应物）。</summary>
        internal Node Target;

        /// <summary>当前路由到的节点（capture/bubble 各节点刷新；DOM event.currentTarget）。</summary>
        internal Node CurrentTarget;

        internal bool _defaultPrevented;
        internal bool _propagationStopped;

        /// <summary>止冒泡：<see cref="EventBus"/> 路由循环看到此 flag 后 break / skip bubble。</summary>
        internal void StopPropagation() => _propagationStopped = true;

        /// <summary>取消默认行为：语义糖据此 flag 决定是否执行默认动作（D3 接线时定）。</summary>
        internal void PreventDefault() => _defaultPrevented = true;
    }

    /// <summary>
    /// 投影层内部：typed event struct 暴露共享 core 引用的访问口。
    /// <see cref="EventBus.Dispatch{T}"/> 经 <c>where T : IRouteEvent, IRouteEventCore</c>
    /// 约束调用 <see cref="Core"/>（JIT 直呼 struct 实现，零装箱）读共享
    /// <see cref="RouteEventCore"/> 堆实例——mutation（StopPropagation/CurrentTarget 刷新）
    /// 经同一实例在 handler 副本与路由循环间传播。
    /// </summary>
    internal interface IRouteEventCore
    {
        RouteEventCore Core { get; }
    }
}
