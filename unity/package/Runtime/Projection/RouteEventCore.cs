#pragma warning disable CS0649   // Target/CurrentTarget 由 D3 demux 在外部 set（struct 定义内不可见）。

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：路由事件核心状态。每个 typed event struct（<see cref="ClickEvent"/>/
    /// <see cref="PointerDownEvent"/>/…）持一个 <c>RouteEventCore _core</c> 字段，把
    /// <see cref="IRouteEvent"/> 的 6 成员（Target/CurrentTarget/DefaultPrevented/
    /// PropagationStopped/StopPropagation/PreventDefault）转发给它——避免 18 个 struct 各写
    /// 一份相同实现（DRY）。
    ///
    /// 设计契约（spec §3.4）：
    /// - <see cref="Target"/> = 命中节点（dispatch 时由 D3 demux 从 <c>LoomEvent.nodeId</c>
    ///   经 <c>NodeRegistry</c> 翻译填入，一次 dispatch 不变）。
    /// - <see cref="CurrentTarget"/> = 当前路由到的节点（capture/bubble 各祖先节点依次刷新，
    ///   D2 填）。DOM/W3C 三阶段模型对齐。
    /// - <see cref="StopPropagation"/> 设 <c>_propagationStopped</c> → <see cref="EventRouter"/>
    ///   路由循环 break（target→root 冒泡止）。
    /// - <see cref="PreventDefault"/> 设 <c>_defaultPrevented</c> → 语义糖（如 <c>Clicked</c>）
    ///   收到后取消默认行为（具体语义由 D3 接线时按事件类型定）。
    ///
    /// <c>internal struct</c>：不出现在公共 API 表面；event struct 的 <c>_core</c> 字段也是
    /// <c>internal</c>，仅供投影层内部 set（D2 EventBus、D3 demux）。
    /// </summary>
    internal struct RouteEventCore
    {
        /// <summary>命中节点（dispatch 全程不变；DOM event.target 对应物）。</summary>
        internal Node Target;

        /// <summary>当前路由到的节点（capture/bubble 各节点刷新；DOM event.currentTarget）。</summary>
        internal Node CurrentTarget;

        internal bool _defaultPrevented;
        internal bool _propagationStopped;

        /// <summary>止冒泡：<see cref="EventRouter"/> 路由循环看到此 flag 后 break。</summary>
        internal void StopPropagation() => _propagationStopped = true;

        /// <summary>取消默认行为：语义糖据此 flag 决定是否执行默认动作（D3 接线时定）。</summary>
        internal void PreventDefault() => _defaultPrevented = true;
    }
}
