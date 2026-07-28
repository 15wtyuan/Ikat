// NodeRegistry：投影层对象身份缓存 + 攒批 dirty 集合。
//
// 投影层契约（docs/design/projection-layer.md §2.4）：对同一 Rust NodeId，C# 侧必须返同一
// Node 实例——订阅 / 镜像状态挂在对象上，若每次返不同实例则丢失。强引用 Dictionary<NodeId,Node>
// 兑现本不变量，且防 GC 回收（订阅委托目标强引用节点，节点再被业务持有很常见）。
//
// 攒批 flush（Task 9）：StyleMirror setter / NodeTransform.Store 标脏后把自己注册进本 registry
// 的 dirty 集合；帧末（LoomHost.Step 的 flush seam，或显式 UIContext.FlushPendingWrites）
// 一次性遍历 dirty 集合调 FlushInline / FlushTransform，清脏 + 清集合。避免每帧扫全部节点找脏。
//
// 生命周期：GetOrCreate 造 + 缓存；Dispose 时主动 Remove（含 dirty 集合 evict，防悬挂引用）。
// 不做 LRU / 弱引用——围栏闭合场景节点数有界，强引用简单且对。

using System.Collections.Generic;

namespace LoomGUI
{
    /// <summary>
    /// 投影层内部：NodeId(u32) → typed Node 的强引用身份缓存 + 攒批 dirty 集合。
    /// UIContext 持有单实例；NodeFactory 造节点后入缓存；Node.Dispose 时 evict。
    /// </summary>
    internal sealed class NodeRegistry
    {
        readonly UIContext _ctx;
        readonly Dictionary<uint, Node> _nodes = new();

        // 攒批 dirty 集合：存 Node（不是 StyleMirror/NodeTransform）——Remove(id) 统一清理。
        // HashSet 去重：同一节点多次 Set/Store 只占一条目，帧末只 flush 一次。
        readonly HashSet<Node> _dirtyStyles = new();
        readonly HashSet<Node> _dirtyTransforms = new();

        internal NodeRegistry(UIContext ctx) => _ctx = ctx;

        /// <summary>当前脏 StyleMirror 条目数（测试可观察，验证攒批去重）。</summary>
        internal int DirtyStyleCount => _dirtyStyles.Count;
        /// <summary>当前脏 NodeTransform 条目数（测试可观察，验证攒批去重）。</summary>
        internal int DirtyTransformCount => _dirtyTransforms.Count;

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

        // ── 攒批 dirty 注册（StyleMirror / NodeTransform 标脏时调）──────────
        // HashSet.Add 自带去重：同节点多次标脏只占一条目。帧末 FlushDirty* 遍历后清集合。

        /// <summary>StyleMirror.Set/Unset 标脏时注册——帧末 flush 一次性 FlushInline。</summary>
        internal void MarkStyleDirty(Node n) => _dirtyStyles.Add(n);
        /// <summary>NodeTransform.Store 标脏时注册——帧末 flush 一次性 FlushTransform。</summary>
        internal void MarkTransformDirty(Node n) => _dirtyTransforms.Add(n);

        /// <summary>
        /// 帧末 flush 所有脏 StyleMirror：遍历 dirty 集合调 FlushInline（重建 CSS 串送 set_inline_override），
        /// 清 StyleMirror._dirty + 清集合。LoomHost.Step flush seam 调，或显式 UIContext.FlushPendingWrites。
        /// </summary>
        internal void FlushDirtyStyles()
        {
            if (_dirtyStyles.Count == 0) return;
            foreach (var n in _dirtyStyles)
            {
                // _style 可能为 null（理论上标脏过就不为 null，防御性跳）；disposed 节点已从集合移除。
                // NodeStyle._mirror 持真 StyleMirror，FlushInline 在那。
                n._style?._mirror.FlushInline();
            }
            _dirtyStyles.Clear();
        }

        /// <summary>
        /// 帧末 flush 所有脏 NodeTransform：遍历调 FlushTransform（set_transform FFI 9-arg），
        /// 清 _dirty + 清集合。set_transform 是整值替换（非累加），每次 flush 送全 4 字段。
        /// </summary>
        internal void FlushDirtyTransforms()
        {
            if (_dirtyTransforms.Count == 0) return;
            foreach (var n in _dirtyTransforms)
            {
                n._transform?.FlushTransform();
            }
            _dirtyTransforms.Clear();
        }

        /// <summary>
        /// 从缓存移除（不调 FFI、不 Dispose 节点）+ 清 dirty 集合条目。
        /// Node.Dispose 完成时 evict 自己；外部直接调用破坏身份稳定，仅 Dispose 路径用。
        /// dirty 集合同步清，防悬挂引用在帧末 flush 已删节点（FFI 对 dead NodeId 静默返 -1，但清掉更干净）。
        /// </summary>
        internal void Remove(uint id)
        {
            if (_nodes.TryGetValue(id, out var n))
            {
                _dirtyStyles.Remove(n);
                _dirtyTransforms.Remove(n);
            }
            _nodes.Remove(id);
        }

        /// <summary>
        /// 手动注册节点（绕过 NodeFactory）。Create&lt;AbsolutePanel&gt; 等场景需要：
        /// Rust 侧 kind 是 Container，但 C# 需要 AbsolutePanel 子类实例。
        /// 调用方负责确保 id 尚未注册（否则覆盖现有缓存，破坏身份稳定）。
        /// </summary>
        internal void Register(uint id, Node node) => _nodes[id] = node;
    }
}
