// NodeRegistry：投影层对象身份缓存。
//
// 投影层契约（docs/design/projection-layer.md §2.4）：对同一 Rust NodeId，C# 侧必须返同一
// Node 实例——订阅 / 镜像状态挂在对象上，若每次返不同实例则丢失。强引用 Dictionary<NodeId,Node>
// 兑现本不变量，且防 GC 回收（订阅委托目标强引用节点，节点再被业务持有很常见）。
//
// 生命周期：GetOrCreate 造 + 缓存；Dispose 时主动 Remove（防悬挂引用访问已删 Rust 节点）。
// 不做 LRU / 弱引用——围栏闭合场景节点数有界，强引用简单且对。

using System.Collections.Generic;

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：NodeId(u32) → typed Node 的强引用身份缓存。
    /// UIContext 持有单实例；NodeFactory 造节点后入缓存；Node.Dispose 时 evict。
    /// </summary>
    internal sealed class NodeRegistry
    {
        readonly UIContext _ctx;
        readonly Dictionary<uint, Node> _nodes = new();

        internal NodeRegistry(UIContext ctx) => _ctx = ctx;

        /// <summary>
        /// 命中缓存则返同一实例；未命中则经 NodeFactory 造 typed 子类 + 入缓存。
        /// 调用方拿到的对象保证：同一 id 多次返 ReferenceEqual 同一实例。
        /// </summary>
        internal Node GetOrCreate(uint id)
        {
            if (_nodes.TryGetValue(id, out var n))
                return n;
            var node = NodeFactory.CreateTyped(_ctx, id);
            _nodes[id] = node;
            return node;
        }

        /// <summary>
        /// 命中返 true + out 实例；未命中返 false + null。不造新实例。
        /// Dispose / 测试用：判断缓存状态而不触发构造。
        /// </summary>
        internal bool TryGet(uint id, out Node node) => _nodes.TryGetValue(id, out node);

        /// <summary>
        /// 从缓存移除（不调 FFI、不 Dispose 节点）。
        /// Node.Dispose 完成时 evict 自己；外部直接调用破坏身份稳定，仅 Dispose 路径用。
        /// </summary>
        internal void Remove(uint id) => _nodes.Remove(id);
    }
}
