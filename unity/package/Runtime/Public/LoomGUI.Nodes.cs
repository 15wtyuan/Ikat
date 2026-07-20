// LoomGUI Frozen Public API: Node hierarchy & controls
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)

using System;
using System.Collections.Generic;
using System.Text;
using LoomGUI.Bindings;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    // ── Node 基础层 ──────────────────────────────────────────────────
    // 三分模型：Style（可写/布局层，下帧 solve）/ Transform（可写/渲染层，不触发 solve）/
    //           Geometry（只读/布局产物，读最近一次 solve 结果，滞后一帧）。
    // Style/Transform 是 class + 内部 owner 引用（投影层：写回经 owner 标脏到 NodeId）；
    // Geometry 是 readonly struct 快照（从每帧 blob 填充）。
    public abstract unsafe class Node
    {
        // ── 投影层字段（internal）：owner 身份 + 生命周期标志 ─────────────
        // _id = Rust NodeId 的 u32 投影（slotmap key）；所有 FFI 调用经此转回 Rust 节点。
        // _ctx = 持有 stage handle + NodeRegistry 的 UIContext；本 Node 入 _ctx._registry 缓存。
        // _disposed = Dispose 后置 true；后续公共读操作抛 ObjectDisposedException。
        internal readonly uint _id;
        internal readonly UIContext _ctx;
        internal bool _disposed;

        // 投影层内部 ctor：经 NodeFactory 调（同 assembly 子类 base 链调）。公共 API 无构造路径
        // （业务从 Create<T> / Instantiate 拿现成 Node）。
        internal Node(UIContext ctx, uint id)
        {
            _ctx = ctx;
            _id = id;
        }

        public UIContext Context
        {
            get
            {
                ThrowIfDisposed();
                return _ctx;
            }
        }

        // Ponytail：暂无 FFI 读节点 id 属性（find_node_by_id 是反向：id 字符串 → NodeId）。
        // 返 numeric NodeId 作调试可读占位；待 get_id_attr FFI 加上后替换为真 id 属性读取。
        public string Id
        {
            get
            {
                ThrowIfDisposed();
                return _id.ToString();
            }
        }

        // Root.Parent == null（FFI node_parent 返 sentinel 0xFFFF_FFFF）。
        // 非根：registry.GetOrCreate(parent_id) → Container（围栏限定只容器型节点可为父）。
        public Container Parent
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                uint parentId = Native.loomgui_node_parent(h, _id);
                if (parentId == RootSentinel) return null;
                return (Container)_ctx._registry.GetOrCreate(parentId);
            }
        }

        // 投影层（C3）：lazy 造 NodeStyle 挂本 Node。同一 Node 多次访问 Style 返同一实例——
        // projection §2.5：node.Style.Width=X 与 node.Style.Height=Y 必须改同一 StyleMirror。
        // 未访问过 = null（不预造，避免给从未读写的节点带镜像开销）。
        internal NodeStyle _style;

        /// <summary>
        /// Style = inline override 层（最高优先级，> 动态规则 > base_style）。lazy 造稳定单一实例：
        /// 首次访问构造 + 挂本 Node；后续访问返同一引用。Dispose 后访问抛 ObjectDisposedException。
        /// </summary>
        public NodeStyle Style
        {
            get
            {
                ThrowIfDisposed();
                _style ??= new NodeStyle(this);
                return _style;
            }
        }

        // 投影层（C4）：lazy 造 NodeTransform 挂本 Node。同 Style 模式：同一 Node 多次访问 Transform
        // 返同一实例——node.Transform.Position=X 与 .Scale=Y 必须改同一 NodeTransform（projection §2.5）。
        // 未访问过 = null（不预造，避免给从未读写的节点带镜像开销）。
        internal NodeTransform _transform;

        /// <summary>
        /// Transform = 渲染层（不触发 solve）。lazy 造稳定单一实例：首次访问构造 + 挂本 Node；
        /// 后续访问返同一引用。setter 只存镜像、不 flush（set_transform FFI 推后，ponytail 注释见
        /// <see cref="NodeTransform"/>）。Dispose 后访问抛 ObjectDisposedException。
        /// </summary>
        public NodeTransform Transform
        {
            get
            {
                ThrowIfDisposed();
                _transform ??= new NodeTransform(this);
                return _transform;
            }
        }

        /// <summary>
        /// Geometry = 只读 layout/world 产物快照。每次访问返 fresh readonly struct（直读 FFI，不缓存）。
        /// 滞后一帧（web-reflow 语义）：读最近一次 solve/compute_world_transforms 结果，本帧写入下帧才反映。
        /// Dispose 后访问抛 ObjectDisposedException。
        /// </summary>
        public NodeGeometry Geometry
        {
            get
            {
                ThrowIfDisposed();
                return new NodeGeometry(_ctx, _id);
            }
        }

        public bool Touchable { get { throw NE(); } set { throw NE(); } }
        public bool Focusable { get { throw NE(); } set { throw NE(); } }   // 运行时改可获焦性（对齐 fgui focusable）

        // 投影层（C5）：lazy 造 ClassList 挂本 Node。同 Style/Transform 模式：同一 Node 多次访问
        // Classes 返同一实例——node.Classes.Add("a") 与 .Contains("a") 必须作用同一 ClassList
        // （projection §2.5 稳定单一实例）。未访问过 = null（不预造，避免给从未读写 class 的节点带开销）。
        internal ClassList _classes;

        /// <summary>
        /// Classes = class 集合投影（Add/Remove/Contains/Toggle/Set/Replace）。lazy 造稳定单一实例：
        /// 首次访问构造 + 挂本 Node；后续访问返同一引用。每次操作直 FFI（class 低频，无镜像——
        /// Contains 直查 has_class 反映 core 真相）。Dispose 后访问抛 ObjectDisposedException。
        /// </summary>
        public ClassList Classes
        {
            get
            {
                ThrowIfDisposed();
                _classes ??= new ClassList(this);
                return _classes;
            }
        }

        public bool IsDisposed => _disposed;

        /// <summary>
        /// 摘父：从父节点 children 移除，自身仍 live 可重挂。不清订阅、不 Dispose。
        /// 根节点（无父）no-op。
        /// </summary>
        public void RemoveFromParent()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            uint parentId = Native.loomgui_node_parent(h, _id);
            if (parentId == RootSentinel) return;   // 根：无父可摘
            Native.loomgui_stage_remove_child(h, parentId, _id);
        }

        /// <summary>
        /// 永久销毁：递归清子 + 调 Rust remove_node + evict 自身 + 标 _disposed。
        /// 后续公共读操作抛 ObjectDisposedException。幂等（二次调 no-op）。
        /// </summary>
        public void Dispose()
        {
            if (_disposed) return;

            // 先递归 evict C# 缓存中的后代（标 _disposed + 移出 registry），避免悬挂引用。
            // 走 Rust FFI 遍历子树——后代的 C# wrapper 可能尚未 GetOrCreate 过，跳过即可。
            DisposeDescendantsInRegistry(_id);

            // Rust 侧递归清子 + slotmap remove + anim/scroll/tween 联动（lib.rs:1230）。
            // 调用后 NodeId 失效（gen++）；后续该 id 的 FFI 调用是 no-op。
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_remove_node(h, _id);

            _ctx._registry.Remove(_id);
            _disposed = true;
        }

        /// <summary>
        /// 按 id 在本节点子树内查找 typed T（DFS 候选取 find_node_by_id 全局首匹配 + 父链 scope-check）。
        /// 不含 self（与 <see cref="Query{T}"/> 一致）：仅查 _id 的后代，自身 id_attr 不被命中——
        /// 即使本节点声明了 id 等于查询值也返 miss。scope-check（IsInSubtree）严格判后代。
        /// 未命中（无 id / 不在子树 / 类型不符）抛 <see cref="UIContractException"/>。null/empty id 直接抛
        /// （DOM getElementById 习惯：空 id 是调用方写错）。
        ///
        /// 作用域契约（public-api §3.1）：组件作用域内查找，不穿透嵌套组件边界。4a 简化：仅校验候选在
        /// 本节点子树内（parent chain 命中 _id）；完整 IsScopeRoot 边界（不穿透嵌套组件/List item）推 4b。
        ///
        /// find_node_by_id 是全局首匹配（core stage.find_node_by_id 遍历整 scene 的 id_attr）——若多节点
        /// 共用同一 id（本身违反"id 在作用域内唯一"约定），first-match 可能落在本子树外导致 Get 误报未命中。
        /// 这是已知 gap，等 4b 加 scope-rooted lookup FFI 时一并修（roadmap §3.1）。
        /// </summary>
        public T Get<T>(string id) where T : Node
        {
            if (!TryGet<T>(id, out var node))
                throw new UIContractException(
                    $"node with id '{id ?? "<null>"}' not found in scope of ({GetType().Name} id={_id})" +
                    " (missing / outside subtree / wrong type)");
            return node;
        }

        /// <summary>
        /// TryGet 是 Get 的 bool-out 版：找到且类型符 → true + out；否则 false（不抛）。
        /// 找到但类型不符（found is not T）也算 miss（false），与 Get 共享一致命中判定。
        /// null/empty id 直接返 false（与 Get 的「抛」互补——TryGet 是宽松查询路径）。
        /// </summary>
        public bool TryGet<T>(string id, out T node) where T : Node
        {
            node = default;
            ThrowIfDisposed();
            if (string.IsNullOrEmpty(id)) return false;

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] idb = Encoding.UTF8.GetBytes(id);
            uint candidate;
            fixed (byte* p = idb)
                candidate = Native.loomgui_stage_find_node_by_id(h, p, (nuint)idb.Length);

            // 无匹配（含 null stage / 非 UTF-8，后两者 ThrowIfDisposed + UTF-8 编码已拦）。
            if (candidate == RootSentinel) return false;
            // 命中但不在本子树：scope-check 走父链，确认候选的祖先链中有 _id。
            if (!IsInSubtree(h, candidate)) return false;

            // registry.GetOrCreate 兑现身份稳定（同 NodeId → 同实例）。若已 Dispose 后 slot 复用，
            // candidate 指向新节点——find_node_by_id 返 live NodeId，不会是已 Dispose 的 stale id。
            Node found = _ctx._registry.GetOrCreate(candidate);
            if (found is T typed) { node = typed; return true; }
            return false;   // 找到但类型不符：算 miss（TryGet false / Get 抛）。
        }

        /// <summary>
        /// 按类型 DFS 子树（文档序 pre-order）。visit 顺序：先子后孙——先访问当前节点所有直系子，
        /// 再递归各子的子树。<see cref="Query{T}"/> 不含 self（与 DOM querySelectorAll 一致：
        /// 在 element 上调 query 只查后代，不含 element 自身）。
        ///
        /// T 是 C# typed 子类（Button/Container/Image/...）；is T 同时匹配子类（Query&lt;Container&gt;
        /// 含 Button/Link/TextBlock 等 Container 派生）。
        /// </summary>
        public IReadOnlyList<T> Query<T>() where T : Node
        {
            ThrowIfDisposed();
            var result = new List<T>();
            DfsPreOrder(n => { if (n is T t) result.Add(t); });
            return result;
        }

        /// <summary>
        /// 按 CSS-like selector DFS 子树（文档序 pre-order）。selector 支持 ".cls"（class）/ "tag"
        /// （tag 名）/ "tag.cls"（both），空 selector 返空 list（不抛）。tag 匹配经 get_node_kind →
        /// NodeKind → 围栏 tag 名（fence schema 子集，详见 <see cref="TagToNodeKind"/>）。
        ///
        /// 不支持：复合 selector（"div &gt; .foo" / ".a.b" 多 class）、伪类（":hover"）、
        /// 属性（"[type=text]"）。围栏闭合下 runtime 节点的 type 已固化为 NodeKind，"input" selector
        /// 只匹配 TextField（默认 type=text），不匹配 Slider/Toggle 等 type 派生——这是 4a 简化，
        /// type-aware selector 推后续（YAGNI：尚无场景驱动）。
        /// </summary>
        public IReadOnlyList<Node> Query(string selector)
        {
            ThrowIfDisposed();
            var (tag, cls) = ParseSelector(selector);
            // 空 selector（null/empty/whitespace）→ 空结果（不是「匹配全部」）。
            // DOM querySelectorAll("") 抛 SyntaxError；LoomGUI 容错返空（不抛——宽松查询路径）。
            // "*" 走下面的 path：TagToNodeKind("*")=null → 所有节点 tagOk=false → 空结果（4a 不支持通用选择器）。
            if (tag == null && cls == null) return Array.Empty<Node>();
            var result = new List<Node>();
            DfsPreOrder(n =>
            {
                bool tagOk = tag == null || MatchesTag(n, tag);
                bool clsOk = cls == null || n.Classes.Contains(cls);
                if (tagOk && clsOk) result.Add(n);
            });
            return result;
        }

        public Animation Play(string name) { throw NE(); }

        public void Focus() { throw NE(); }
        public void Blur() { throw NE(); }

        public IDisposable OnUpdate(Action<float> cb) { throw NE(); }   // 逻辑驱动每帧更新钩子（返回句柄，Dispose 撤销）
        /// <summary>
        /// 订阅 typed 路由事件（DOM addEventListener 等价）。
        ///
        /// <paramref name="useCapture"/>：true → capture 阶段触发（root→target 路径上）；false → bubble
        /// 阶段触发（target→root 路径上，默认）。target 节点上 capture/bubble listener 都触发
        /// （DOM target 阶段等价）。<paramref name="once"/>：true → 触发一次后自动退订（防"等一个结束事件"
        /// 泄漏，如等 <see cref="AnimationEndEvent"/> 后 Dispose）。
        ///
        /// 返 <see cref="EventRegistration"/>——Dispose 退订。订阅随 <see cref="Dispose"/> 自动清理
        /// （public-api §5.4）。详细路由模型见 public-api §5.2。
        /// </summary>
        public EventRegistration On<T>(Action<T> handler, bool useCapture = false, bool once = false) where T : IRouteEvent
        {
            ThrowIfDisposed();
            if (handler == null) throw new ArgumentNullException(nameof(handler));
            return _ctx._eventBus.Subscribe<T>(_id, handler, useCapture, once);
        }

        // ── helpers ─────────────────────────────────────────────────────

        // node_parent 哨兵：根 / 越界 / 无 scene 均返 0xFFFF_FFFF（lib.rs:429）。
        internal const uint RootSentinel = 0xFFFF_FFFFu;

        /// <summary>
        /// Dispose 后访问抛 ObjectDisposedException。所有公共读操作入口都先调本方法。
        /// 异常消息用具体子类名（GetType().Name），帮助定位是哪种节点被误用。
        /// </summary>
        internal void ThrowIfDisposed()
        {
            if (_disposed) throw new ObjectDisposedException(GetType().Name);
        }

        // ── scope lookup helpers（Get/TryGet/Query 内部）──────────────────

        /// <summary>
        /// 走父链判断 candidateId 是否在 _id 子树内（含直接子 + 任意深度后代；不含 _id 自身）。
        /// 用 loomgui_node_parent 逐层向上，直到撞 _id（在子树）或 RootSentinel（走出根）。
        /// 单线程同步内树结构稳定；防御性 cycle check（parent == current）防 ABI 异常死循环。
        /// </summary>
        private bool IsInSubtree(StageHandle* h, uint candidateId)
        {
            uint current = candidateId;
            for (int i = 0; i < 10_000; i++)   // 上限防御：scene 树深度受围栏闭合有界，10k 兜底
            {
                uint parent = Native.loomgui_node_parent(h, current);
                if (parent == RootSentinel) return false;   // 走出根，candidate 在别棵子树
                if (parent == _id) return true;             // 命中本节点——candidate 是其后代
                if (parent == current) return false;        // 防御：自循环（理论不达）
                current = parent;
            }
            return false;   // 深度超 10k：当作不在子树（理论不达，scene 不会有如此深树）
        }

        /// <summary>
        /// 文档序 pre-order DFS：从本节点的直系子开始，依次 visit 每个子 + 递归子的子树。
        /// 不 visit self（与 DOM querySelectorAll 语义一致——element.query 不含 element 自身）。
        /// 非 Container 节点无 Children —— no-op（Query 在叶子节点上返空 list）。
        /// </summary>
        private void DfsPreOrder(Action<Node> visit)
        {
            if (this is Container c)
            {
                // c.Children lazy materialize 每次 re-fetch 最新直系子（树可变，不缓存 list）。
                // registry.GetOrCreate 兑现身份稳定——同 NodeId 多次 DFS 入参返同实例。
                foreach (Node child in c.Children)
                {
                    visit(child);
                    child.DfsPreOrder(visit);
                }
            }
        }

        /// <summary>
        /// 解析 CSS-like selector（fence 子集）。支持 ".cls" / "tag" / "tag.cls" 三种形式；
        /// 其它形式（".a.b" / "a &gt; b"）按容错解析：取首个 '.' 切 tag|cls，多 class 取末段为 cls
        /// （粗糙 4a 简化——复合 selector 不在 4a 范围）。null/空/whitespace → (null,null) 即匹配空集。
        /// </summary>
        private static (string tag, string cls) ParseSelector(string selector)
        {
            if (string.IsNullOrWhiteSpace(selector)) return (null, null);
            string s = selector.Trim();
            int dot = s.IndexOf('.');
            if (dot < 0) return (s, null);                      // "tag"
            string tagPart = dot > 0 ? s.Substring(0, dot) : null;
            string clsPart = dot < s.Length - 1 ? s.Substring(dot + 1) : null;
            // cls 含 '.' 或 tag 时不再细切——4a 把 "tag.a.b" 当作 (tag, "a.b") 永远 miss。
            // 后续升级到真 CSS selector parser 时替换本方法。
            return (tagPart, clsPart);
        }

        /// <summary>
        /// tag 匹配：取节点 NodeKind + 查 <see cref="TagToNodeKind"/> 映射。
        /// 多个 tag 共映同一 NodeKind（div/header/nav → Container；ul/ol → ListView；span/strong/em
        /// → TextElement）—— selector 用任一别名都命中。未知 tag（含围栏外的）TagToNodeKind 返 null
        /// → 永远 false（容错：不抛、selector 静默空集）。
        /// </summary>
        private static bool MatchesTag(Node n, string tag)
        {
            NodeKind? expected = TagToNodeKind(tag);
            if (expected == null) return false;
            StageHandle* h = (StageHandle*)n._ctx._stage.ToPointer();
            byte kind = 0xFF;
            int rc = Native.loomgui_stage_get_node_kind(h, n._id, &kind);
            if (rc != 0) return false;   // 节点不 live / stage 失效——保守 false
            return (NodeKind)kind == expected.Value;
        }

        /// <summary>
        /// 围栏 tag 名 → C# NodeKind 映射（crates/fence/src/schema/tag.rs::resolve_semantic 子集）。
        /// input 无 type 默认 TextField；type=range/checkbox/... 派生 kind 在 parse 期已固化，selector
        /// 用 "input" 只匹配 TextField（不匹配派生——4a 简化，type-aware selector 推后续）。
        /// template 不在映射表——parse 期消费、不进 runtime 树，selector "template" 永远空集。
        ///
        /// 已知 core 不一致（span）：本表对齐 parse/pkg 路径（resolve_semantic("span") → TextElement，
        /// 覆盖 pkg 加载的绝大多数 span）。但 core 的动态建树 API 走另一张表——
        /// crates/core/src/scene/dynamic.rs::kind_from_tag("span") → NodeKind::TextNode（byte=1）。因此
        /// 运行时通过 Container.TextContent setter / create_node("span") 产出的 span 携带 kind=TextNode，
        /// `Query("span")` 对该子树会落空（不命中 TextElement）。pkg-loaded 节点不受影响。core 表拓宽
        /// 到完整映射（或动态 API 改走 resolve_semantic）留作 roadmap 项，本表不改（4a：取 pkg 主路径）。
        /// </summary>
        private static NodeKind? TagToNodeKind(string tag) => tag switch
        {
            "div" or "header" or "nav" => NodeKind.Container,
            "p" => NodeKind.TextBlock,
            "span" or "strong" or "em" => NodeKind.TextElement,
            "br" => NodeKind.LineBreak,
            "label" => NodeKind.Label,
            "button" => NodeKind.Button,
            "a" => NodeKind.Link,
            "img" => NodeKind.Image,
            "canvas" => NodeKind.Canvas,
            "input" => NodeKind.TextField,       // 默认 type=text；派生 kind 不命中（4a 简化）
            "textarea" => NodeKind.TextArea,
            "select" => NodeKind.Dropdown,
            "option" => NodeKind.OptionItem,
            "progress" => NodeKind.ProgressBar,
            "ul" or "ol" => NodeKind.ListView,
            "li" => NodeKind.ListItem,
            "slot" => NodeKind.Slot,
            _ => null,                            // 含围栏外 tag + 自定义标签（带连字符）
        };

        /// <summary>
        /// 经 FFI 递归遍历 Rust 子树，对每个后代 NodeId：若 C# 侧有缓存 wrapper 则标 _disposed
        /// + 从 registry 移除。单次 remove_node（调用方 Dispose 末尾调）清 Rust 侧整棵子树；
        /// 本方法只维护 C# 缓存一致性，不调 remove_node per node。
        /// </summary>
        private void DisposeDescendantsInRegistry(uint subtreeRootId)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();

            // Snapshot 直系子（Rust 侧查询；下面的递归会改 Rust 树结构，不能边遍历边改）。
            int count = Native.loomgui_stage_get_child_count(h, subtreeRootId);
            if (count <= 0) return;
            uint[] buf = new uint[count];
            int written;
            fixed (uint* bp = buf)
            {
                written = Native.loomgui_stage_get_children(h, subtreeRootId, bp, (nuint)buf.Length);
            }
            // written < 0 = 节点刚被并发移除（理论单线程不达）；防御性早退防读越界。
            if (written < 0) return;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                uint childId = buf[i];
                // 深度优先：先清孙，再清子（清子后该 childId 在 Rust 侧可能已失效，但仍可读 cache）。
                DisposeDescendantsInRegistry(childId);
                if (_ctx._registry.TryGet(childId, out var cached))
                {
                    cached._disposed = true;
                    _ctx._registry.Remove(childId);
                }
            }
        }

        static NotImplementedException NE() => new NotImplementedException();
    }

    // Style = inline override 层（最高优先级），不是 cascade 读取窗口。
    // getter 只反映 C# setter 写过的属性；未写过返回 Unset（要 computed 走 Geometry）。
    // setter 写 Unset = 撤销该属性 inline override，回落 CSS。
    //
    // C3：每个 typed 属性的 setter/getter 走 _mirror（StyleMirror）。CSS prop 名严格对照 core
    // inline_bit 表（crates/core/src/style/dynamic.rs）+ apply_decl（mapping.rs）——表外的 prop
    // 经 set_inline_override 会被 bit 检查前置静默丢弃（ghost-state 防护），故本类只接 24 个
    // inline_bit 表内 prop；ZIndex / Visibility / SetVar / RemoveVar 暂 ponytail defer
    // （core apply_decl 未实现，throw NE + 注释）。
    public sealed class NodeStyle
    {
        // 投影层内部：owner Node + mirror。Node.Style lazy 造时传入 this；StyleMirror 持 owner
        // 转调 FFI（set/unset_inline_override）需 owner._ctx._stage + owner._id。
        internal readonly Node _owner;
        internal readonly StyleMirror _mirror;
        internal NodeStyle(Node owner) { _owner = owner; _mirror = new StyleMirror(owner); }

        // ── 盒模型（Length：宽高 + min/max + inset 四边）──────────────────
        // Length getter：mirror 无 → Length.Unset()（frozen 约定"未写过返 Unset"）。
        public Length Width
        {
            get => _mirror.IsSet("width") ? _mirror.Get<Length>("width")!.Value : Length.Unset();
            set => _mirror.Set("width", value);
        }
        public Length Height
        {
            get => _mirror.IsSet("height") ? _mirror.Get<Length>("height")!.Value : Length.Unset();
            set => _mirror.Set("height", value);
        }
        public Length MinWidth
        {
            get => _mirror.IsSet("min-width") ? _mirror.Get<Length>("min-width")!.Value : Length.Unset();
            set => _mirror.Set("min-width", value);
        }
        public Length MaxWidth
        {
            get => _mirror.IsSet("max-width") ? _mirror.Get<Length>("max-width")!.Value : Length.Unset();
            set => _mirror.Set("max-width", value);
        }
        public Length MinHeight
        {
            get => _mirror.IsSet("min-height") ? _mirror.Get<Length>("min-height")!.Value : Length.Unset();
            set => _mirror.Set("min-height", value);
        }
        public Length MaxHeight
        {
            get => _mirror.IsSet("max-height") ? _mirror.Get<Length>("max-height")!.Value : Length.Unset();
            set => _mirror.Set("max-height", value);
        }
        public Length Left
        {
            get => _mirror.IsSet("left") ? _mirror.Get<Length>("left")!.Value : Length.Unset();
            set => _mirror.Set("left", value);
        }
        public Length Top
        {
            get => _mirror.IsSet("top") ? _mirror.Get<Length>("top")!.Value : Length.Unset();
            set => _mirror.Set("top", value);
        }
        public Length Right
        {
            get => _mirror.IsSet("right") ? _mirror.Get<Length>("right")!.Value : Length.Unset();
            set => _mirror.Set("right", value);
        }
        public Length Bottom
        {
            get => _mirror.IsSet("bottom") ? _mirror.Get<Length>("bottom")!.Value : Length.Unset();
            set => _mirror.Set("bottom", value);
        }

        // ── Thickness 盒模型（padding/margin/border-width）──────────────
        // Thickness 无 Unset 哨兵（裸四值 struct）；getter 未写过返 default（全 0）+ 不代表
        // "显式 0"，仅表示"未写过"。如需判"是否写过"走 Geometry（C4）或自带 IsSet 查询。
        public Thickness Padding
        {
            get => _mirror.Get<Thickness>("padding") ?? default;
            set => _mirror.Set("padding", value);
        }
        public Thickness Margin
        {
            get => _mirror.Get<Thickness>("margin") ?? default;
            set => _mirror.Set("margin", value);
        }
        public Thickness BorderWidth
        {
            get => _mirror.Get<Thickness>("border-width") ?? default;
            set => _mirror.Set("border-width", value);
        }
        public Length Gap
        {
            get => _mirror.IsSet("gap") ? _mirror.Get<Length>("gap")!.Value : Length.Unset();
            set => _mirror.Set("gap", value);
        }

        // ── flex（enum：getter 未写过返 Unset=0 变体）─────────────────────
        public DisplayMode Display
        {
            get => _mirror.Get<DisplayMode>("display") ?? DisplayMode.Unset;
            set => _mirror.Set("display", value);
        }
        public FlexDirection FlexDirection
        {
            get => _mirror.Get<FlexDirection>("flex-direction") ?? FlexDirection.Unset;
            set => _mirror.Set("flex-direction", value);
        }
        public FlexWrap FlexWrap
        {
            get => _mirror.Get<FlexWrap>("flex-wrap") ?? FlexWrap.Unset;
            set => _mirror.Set("flex-wrap", value);
        }
        public JustifyContent JustifyContent
        {
            get => _mirror.Get<JustifyContent>("justify-content") ?? JustifyContent.Unset;
            set => _mirror.Set("justify-content", value);
        }
        public AlignItems AlignItems
        {
            get => _mirror.Get<AlignItems>("align-items") ?? AlignItems.Unset;
            set => _mirror.Set("align-items", value);
        }

        // ── 溢出 / 定位（enum）──────────────────────────────────────────
        public Overflow OverflowX
        {
            get => _mirror.Get<Overflow>("overflow-x") ?? Overflow.Unset;
            set => _mirror.Set("overflow-x", value);
        }
        public Overflow OverflowY
        {
            get => _mirror.Get<Overflow>("overflow-y") ?? Overflow.Unset;
            set => _mirror.Set("overflow-y", value);
        }
        public PositionMode Position
        {
            get => _mirror.Get<PositionMode>("position") ?? PositionMode.Unset;
            set => _mirror.Set("position", value);
        }

        // ── 视觉（Color/float）──────────────────────────────────────────
        public Color BackgroundColor
        {
            get => _mirror.Get<Color>("background-color") ?? Color.Unset;
            set => _mirror.Set("background-color", value);
        }
        public Color Color
        {
            get => _mirror.Get<Color>("color") ?? Color.Unset;
            set => _mirror.Set("color", value);
        }
        // Opacity 无 Unset 哨兵（裸 float）；getter 未写过返 default（0f）+ 不代表"显式 0"。
        // 业务语义：CSS opacity 默认 1f，但本 getter 只反映 setter 写过的值（projection §2.3 严格语义）。
        public float Opacity
        {
            get => _mirror.Get<float>("opacity") ?? default;
            set => _mirror.Set("opacity", value);
        }

        // ── ponytail defer：core apply_decl / inline_bit 表未实现的 prop ──
        // ZIndex（z-index）：core apply_decl 处理 "order"（mapping.rs:829）但未给 inline_bit —
        // set_inline_override 会被 bit 检查前置跳过（打包期 CSS order:N 仍生效）。
        // Visibility（visibility）：core apply_decl 无 "visibility" 分支（display:none 是围栏闭合的隐藏语义）。
        // SetVar/RemoveVar（--xxx）：core apply_decl 不处理 CSS 自定义属性；custom-property 通道待加。
        // 保留 throw NE 防止静默丢：调用方期望 round-trip，prop-name 不在 inline_bit 表经 set_inline_override
        // 会被 bit 检查前置静默忽略（ghost-state 防护）。补 core 支持后把这些 setter 接 _mirror 即可。
        public int ZIndex { get { throw NE(); } set { throw NE(); } }
        public Visibility Visibility { get { throw NE(); } set { throw NE(); } }
        public void SetVar(string n, Length v) { throw NE(); }
        public void SetVar(string n, Color v) { throw NE(); }
        public void SetVar(string n, float v) { throw NE(); }
        public void SetVar(string n, string v) { throw NE(); }
        public void RemoveVar(string n) { throw NE(); }   // 撤销 inline var，回落 CSS
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Transform = 渲染层，不触发 solve。回写走独立数值 FFI（set_transform，纯 f32）。
    //
    // C4（标脏不 flush）：setter 只存镜像值、不调任何 FFI——set_transform FFI 推后到第一个逐帧
    // transform 控件落地（roadmap §3.5 / spec §5）。升级路径：FFI 加上后，NodeTransform setter
    // 标脏 + 帧末（攒批 seam 同 Style.FlushInline）一次性 flush 全属性到 core，core 在
    // compute_world_transforms 时并入 local_transform。本类签名零改动——只把"存镜像"扩到"标脏 + flush"。
    //
    // ponytail: 4a 不实现 set_transform FFI 是有意——无控件消费 transform 写入时，flush 也只是落
    // 空 core bit（无视觉变化）；等首个真实控件（如绝对定位动画 / 触摸抖动）落地再接通，避免 ghost state。
    public sealed class NodeTransform
    {
        // 投影层内部：owner Node。lazy 造时由 Node.Transform 传 this；getter/setter 经它走 FFI
        // （升级后：owner._ctx._stage + owner._id 转调 set_transform）。
        internal readonly Node _owner;

        // 镜像值：setter 写、getter 读。default 按业务语义初始化（Scale=One 不缩放，其它 Zero）。
        // 未 flush 到 core——读到的是 C# 侧最近一次写入的快照。
        internal Vector2 _position = Vector2.Zero;
        internal Vector2 _scale = Vector2.One;
        internal float _rotation;
        internal Vector2 _origin = Vector2.Zero;
        // dirty 在升级路径用（攒批 flush 时帧末扫所有 dirty 的 NodeTransform）。4a flush 未接通时
        // 只标脏不消费，留作 future seam 接入点；现保留写以让"setter 改状态"语义可观察。
        internal bool _dirty;

        internal NodeTransform(Node owner) { _owner = owner; }

        /// <summary>位移（local 坐标，px）。setter 存镜像、不 flush（set_transform 推后）。</summary>
        public Vector2 Position { get => _position; set => Store(ref _position, value); }
        /// <summary>缩放（local 基）。default = One（不缩放）；setter 存镜像、不 flush。</summary>
        public Vector2 Scale { get => _scale; set => Store(ref _scale, value); }
        /// <summary>旋转（弧度，绕 Origin）。setter 存镜像、不 flush。</summary>
        public float Rotation { get => _rotation; set => Store(ref _rotation, value); }
        /// <summary>旋转/缩放原点（local 坐标，px）。setter 存镜像、不 flush。</summary>
        public Vector2 Origin { get => _origin; set => Store(ref _origin, value); }

        // 统一 setter 路径：写镜像 + 标脏。ponytail: 升级时在此调帧末 flush seam（攒批同 Style）。
        void Store<T>(ref T field, T value)
        {
            field = value;
            _dirty = true;
        }
    }

    // Geometry = 只读快照，直读 FFI layout/world 产物（滞后一帧，同 web reflow）。
    //
    // C4（直读 FFI）：readonly struct 持 owner 身份（uint _id + UIContext _ctx）；node.Geometry 每次
    // 返 fresh struct snapshot。projection §2.5 / §2.6 读时序——LayoutRect/WorldRect 反映最近一次
    // solve/compute_world_transforms 结果，本帧写 Style/Transform 下帧才反映（滞后一帧）。
    //
    // ponytail: blob 缓存推后（projection §5 升级路径给 FrameBlob 加 rect/world 列）。4a 直读 FFI
    // 简单且正确——单次 layout_rect/world_matrix 读是 6 f32 + 1 dict 查找，热路径（每帧 N 节点读）
    // 暂未达需缓存的规模，YAGNI。
    public readonly unsafe struct NodeGeometry
    {
        // struct 不持 disposed 状态——disposed 检在 node.Geometry getter 入口（Node.ThrowIfDisposed）。
        // 调用方拿到 struct 后假设 owner 活；FFI 在 owner 失效时返 identity/0（h.is_null/无效节点兜底）。
        internal readonly UIContext _ctx;
        internal readonly uint _id;
        internal NodeGeometry(UIContext ctx, uint id) { _ctx = ctx; _id = id; }

        /// <summary>
        /// 节点 layout 产物（solve 输出，左上 + w/h）。直读 get_node_layout_rect FFI（x/y/w/h → Rect）。
        /// 滞后一帧：本帧 Style 写入下帧才反映。
        /// </summary>
        public Rect LayoutRect
        {
            get
            {
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                float x = 0, y = 0, w = 0, hh = 0;
                Native.loomgui_stage_get_node_layout_rect(h, _id, &x, &y, &w, &hh);
                return new Rect(x, y, w, hh);
            }
        }

        /// <summary>
        /// 节点 world AABB（layout_rect 经 world_matrix 变换后的轴对齐外接盒）。
        /// 直读 get_node_world_matrix FFI（Affine2 = [a,b,c,d,tx,ty]）+ 对 LayoutRect 四角 apply_point + 取 AABB。
        /// 滞后一帧：本帧 layout/transform 写入下帧才反映。
        /// </summary>
        public Rect WorldRect => LocalToGlobal(LayoutRect);

        /// <summary>
        /// 本地点 → 世界点（经 world_matrix）。Affine2 列主序：x' = a·x + c·y + tx，y' = b·x + d·y + ty
        /// （crates/core/src/transform.rs:46 apply_point 公式）。
        /// </summary>
        public Vector2 LocalToGlobal(Vector2 p)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            return new Vector2(a * p.X + c * p.Y + tx, b * p.X + d * p.Y + ty);
        }

        /// <summary>
        /// 世界点 → 本地点（world_matrix 的逆变换）。退化情形（det≈0，如 scale(0)）Rust 侧 inverse
        /// 返 IDENTITY（transform.rs:55），此处逆变换即原 world_matrix 逆——与 hit_test 一致的兜底。
        /// </summary>
        public Vector2 GlobalToLocal(Vector2 p)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            InverseAffine(a, b, c, d, tx, ty,
                          out float ia, out float ib, out float ic, out float id, out float itx, out float ity);
            return new Vector2(ia * p.X + ic * p.Y + itx, ib * p.X + id * p.Y + ity);
        }

        /// <summary>本地 rect → world AABB：四角 LocalToGlobal + 轴对齐外接盒。</summary>
        public Rect LocalToGlobal(Rect r)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            return TransformAABB(a, b, c, d, tx, ty, r);
        }

        /// <summary>world rect → local AABB：四角 GlobalToLocal + 轴对齐外接盒。</summary>
        public Rect GlobalToLocal(Rect r)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            InverseAffine(a, b, c, d, tx, ty,
                          out float ia, out float ib, out float ic, out float id, out float itx, out float ity);
            return TransformAABB(ia, ib, ic, id, itx, ity, r);
        }

        // ── FFI + 矩阵 helpers ─────────────────────────────────────────
        // 与 transform.rs 的 apply_point / inverse 公式一一对应；保留为 private 静态以便 JIT 内联。
        // Rust FFI 的 null/无效节点兜底写 identity（[1,0,0,1,0,0]）——调用方 owner Dispose 后理论
        // 不达（node.Geometry getter 抛 ODE），但兜底保证 struct 不持活节点也能安全读。

        void GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            // locals 而非直接 &out：C# 禁止对 out 参数取地址（GC 可能移动托管引用）。
            float la = 1f, lb = 0f, lc = 0f, ld = 1f, ltx = 0f, lty = 0f;   // identity default（null/失效兜底）
            Native.loomgui_stage_get_node_world_matrix(h, _id, &la, &lb, &lc, &ld, &ltx, &lty);
            a = la; b = lb; c = lc; d = ld; tx = ltx; ty = lty;
        }

        // Affine2 逆：与 transform.rs:52 inverse 同算法（det≈0 退化返 identity）。
        static void InverseAffine(float a, float b, float c, float d, float tx, float ty,
                                  out float ia, out float ib, out float ic, out float id,
                                  out float itx, out float ity)
        {
            float det = a * d - b * c;
            if (Math.Abs(det) < 1e-12f)   // 退化：返 identity（与 Rust inverse 兜底一致）
            {
                ia = 1f; ib = 0f; ic = 0f; id = 1f; itx = 0f; ity = 0f;
                return;
            }
            float invDet = 1f / det;
            ia = d * invDet;
            ib = -b * invDet;
            ic = -c * invDet;
            id = a * invDet;
            itx = -(ia * tx + ic * ty);
            ity = -(ib * tx + id * ty);
        }

        // 仿射变换 AABB：取四角变换后的外接盒（轴对齐）。旋转/缩放时 world box > local box。
        static Rect TransformAABB(float a, float b, float c, float d, float tx, float ty, Rect r)
        {
            // 四角：min/max × min/max（避免重复算 0 尺寸退化点）。
            float x0 = r.X, y0 = r.Y;
            float x1 = r.X + r.Width, y1 = r.Y + r.Height;
            ApplyPoint(a, b, c, d, tx, ty, x0, y0, out float p0x, out float p0y);
            ApplyPoint(a, b, c, d, tx, ty, x1, y0, out float p1x, out float p1y);
            ApplyPoint(a, b, c, d, tx, ty, x0, y1, out float p2x, out float p2y);
            ApplyPoint(a, b, c, d, tx, ty, x1, y1, out float p3x, out float p3y);
            float minX = Math.Min(Math.Min(p0x, p1x), Math.Min(p2x, p3x));
            float minY = Math.Min(Math.Min(p0y, p1y), Math.Min(p2y, p3y));
            float maxX = Math.Max(Math.Max(p0x, p1x), Math.Max(p2x, p3x));
            float maxY = Math.Max(Math.Max(p0y, p1y), Math.Max(p2y, p3y));
            return new Rect(minX, minY, maxX - minX, maxY - minY);
        }

        static void ApplyPoint(float a, float b, float c, float d, float tx, float ty,
                               float x, float y, out float ox, out float oy)
        {
            ox = a * x + c * y + tx;
            oy = b * x + d * y + ty;
        }
    }

    // ── Container 与树操作 ──────────────────────────────────────────
    public unsafe class Container : Node
    {
        internal Container(UIContext ctx, uint id) : base(ctx, id) { }

        /// <summary>
        /// 直系子节点数。每次访问直读 Rust（get_child_count），不缓存——树可变
        /// （C6 写操作 AddChild/InsertChild/RemoveChild 会改子数），缓存会 stale。
        /// </summary>
        public int ChildCount
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int c = Native.loomgui_stage_get_child_count(h, _id);
                // -1 = 节点不 live（post-ThrowIfDisposed 理论不达，FFI 防御性兜底）。
                return c < 0 ? 0 : c;
            }
        }

        /// <summary>
        /// 直系子节点列表（typed）。每次访问 lazy 物化：调 get_children 拿 NodeId 数组 +
        /// 逐个 registry.GetOrCreate 包成 typed Node。不缓存 list 本身——树可变，缓存的 list
        /// 会 stale。但 list 内的 Node 引用稳定：GetOrCreate 走 registry 强引用缓存，同一 NodeId
        /// 永远返同一实例（订阅 / 镜像挂对象上不丢——projection §2.4）。
        /// </summary>
        public IReadOnlyList<Node> Children
        {
            get
            {
                ThrowIfDisposed();
                return MaterializeChildren();
            }
        }

        /// <summary>
        /// DOM textContent：读=递归拼接后代 TextNode.Text（文档序），写=清所有子 + 挂单个 TextNode。
        /// 读侧递归 Container 子树（含 TextBlock/TextElement/Button 等 Container 子类）累加 TextNode._text；
        /// 非 TextNode 叶子（Image / 控件）贡献 0 字符。
        ///
        /// 写侧 DOM 语义：先清所有当前子（remove_child 各子——DOM 不 Dispose，子可重挂），再建一个
        /// TextNode（create_node "span" + set_text）+ append_child。多次写值=替换当前 TextNode 文本
        /// 不重建——但本实现简化为每次写都重建（与 DOM textContent setter 一致：每次写都重建内容树）。
        /// </summary>
        public string TextContent
        {
            get
            {
                ThrowIfDisposed();
                var sb = new StringBuilder();
                AppendDescendantText(this, sb);
                return sb.ToString();
            }
            set
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                string text = value ?? "";

                // 1) 清当前直系子（DOM：移除但不 Dispose——子可重挂）。直 FFI 逐个 remove_child，
                //    跳过 RemoveChild 的 GetChildIndex 校验（snapshot 已确保是直系子，校验纯开销）。
                ClearDirectChildrenFFI(h);

                // 2) 建 TextNode + 写文本 + append。三步 FFI 顺序——建后才有 NodeId，setText 后再挂，
                //    避免 append 后挂前核心状态不一致窗口（无父 TextNode 也合法，标 dirty_text 即可）。
                byte[] tag = Encoding.UTF8.GetBytes("span");   // 围栏 kind_from_tag: "span" → TextNode
                uint textId;
                fixed (byte* tp = tag)
                    textId = Native.loomgui_stage_create_node(h, tp, (nuint)tag.Length, null, 0);
                if (textId == RootSentinel)
                    throw new InvalidOperationException("create_node(\"span\") failed (stage null / non-UTF-8)");

                byte[] tb = Encoding.UTF8.GetBytes(text);
                fixed (byte* tp = tb)
                {
                    int src = Native.loomgui_stage_set_text(h, textId, tp, (nuint)tb.Length);
                    if (src != 0)
                        throw new InvalidOperationException("set_text on fresh TextNode failed (non-TextNode kind)");
                }

                int arc = Native.loomgui_stage_append_child(h, _id, textId);
                if (arc != 0)
                    throw new InvalidOperationException("append_child(textNode) failed (child has existing parent)");

                // 3) Cache 镜像到 C# TextNode wrapper——registry.GetOrCreate 据 textId 派发到 TextNode
                //    （NodeFactory: kind=TextNode → TextNode ctor）。写镜像避免下次读触发 FFI。
                var tn = (TextNode)_ctx._registry.GetOrCreate(textId);
                tn._text = text;
            }
        }

        /// <summary>
        /// 挂子到末尾（DOM AppendChild 语义）。返回 c（身份稳定——registry 同一 NodeId → 同一实例）。
        /// c 必须当前无父（Rust append_child 前置：child.parent=None）；若 c 已挂别处，FFI 返错抛。
        /// 不 Dispose / 不复制 c——c 仍是调用方传入的同一对象。
        /// </summary>
        public T AddChild<T>(T c) where T : Node
        {
            ThrowIfDisposed();
            if (c is null) throw new ArgumentNullException(nameof(c));
            c.ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_append_child(h, _id, c._id);
            // rc!=0：child 已挂父 / 节点不 live / null stage（前两者 ThrowIfDisposed 拦）。
            if (rc != 0)
                throw new InvalidOperationException(
                    $"append_child(parent={_id}, child={c._id}) failed (child has existing parent?)");
            return c;
        }

        /// <summary>
        /// 在 index i 处插子（DOM insertBefore 语义，但按 index）。i=0 头插，i=ChildCount 末尾追加，
        /// 中间值插到「当前第 i 子之前」。越界（负数 / &gt; ChildCount）抛 ArgumentOutOfRangeException。
        /// 返回 c（身份稳定）。
        /// </summary>
        public T InsertChild<T>(T c, int i) where T : Node
        {
            ThrowIfDisposed();
            if (c is null) throw new ArgumentNullException(nameof(c));
            c.ThrowIfDisposed();

            // MaterializeChildren 拿当前直系子——既要算 ChildCount 也要拿 refChild NodeId。
            // 一次性 materialize 复用，避免 GetChildAt 二次 FFI。
            var kids = MaterializeChildren();
            // uint 强转一次校验 i ∈ [0, kids.Count]：负数 → 大正数越界，省一条分支。
            // i == kids.Count 允许（append 语义，对应 insert_before ref_id=INVALID）。
            if ((uint)i > (uint)kids.Count)
                throw new ArgumentOutOfRangeException(nameof(i), i,
                    $"index {i} out of range [0, {kids.Count}]");

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            // i == Count：ref_id = INVALID（Rust insert_before：INVALID → 末尾追加）。
            uint refId = (i == kids.Count) ? RootSentinel : kids[i]._id;
            int rc = Native.loomgui_stage_insert_before(h, _id, c._id, refId);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"insert_before(parent={_id}, child={c._id}, ref={refId}) failed (child has existing parent?)");
            return c;
        }

        /// <summary>
        /// 摘子（DOM RemoveChild 语义）。不 Dispose 子——子仍 live，可重挂到别处。区别于
        /// <see cref="Node.Dispose"/>（递归永久销毁）。c 必须是 this 的当前直系子（GetChildIndex != -1），
        /// 否则抛 ArgumentException（DOM NotFoundError 等价）。
        /// </summary>
        public void RemoveChild(Node c)
        {
            ThrowIfDisposed();
            if (c is null) throw new ArgumentNullException(nameof(c));
            c.ThrowIfDisposed();
            // 必须先校验 c 是直系子：core remove_child 不校验，会对「别 parent 的子」误设 parent=None
            // （dynamic.rs:231 retain no-op 但 parent=None 仍执行——bug 兜底在投影层拦）。
            if (GetChildIndex(c) < 0)
                throw new ArgumentException(
                    $"node (id={c._id}) is not a child of container (id={_id})", nameof(c));

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_remove_child(h, _id, c._id);
            if (rc != 0)
                throw new InvalidOperationException(
                    $"remove_child(parent={_id}, child={c._id}) failed (post-check race?)");
            // 不 Dispose c：DOM 语义——c 可重挂。c._disposed 保持 false。
        }

        /// <summary>
        /// 取第 i 个直系子节点（按 append 顺序）。越界（负数 / ≥ ChildCount）抛
        /// ArgumentOutOfRangeException。物化路径同 <see cref="Children"/>：lazy + registry 缓存。
        /// </summary>
        public Node GetChildAt(int i)
        {
            ThrowIfDisposed();
            var kids = MaterializeChildren();
            // uint 强转一次校验 i ∈ [0, kids.Count)：负数变大正数越界，省一条分支。
            if ((uint)i >= (uint)kids.Count)
                throw new ArgumentOutOfRangeException(nameof(i), i,
                    $"index {i} out of range [0, {kids.Count})");
            return kids[i];
        }

        /// <summary>
        /// 查 c 在直系子中的索引（按 append 顺序）。未找到返 -1（.NET IndexOf 习惯，不抛）。
        /// null → ArgumentNullException；c 已 Dispose → ObjectDisposedException（stale 句柄不该再传 API）。
        /// 用 _id 比较：registry 保证同一 NodeId → 同一实例，_id 相等 ⇔ ReferenceEquals。
        /// </summary>
        public int GetChildIndex(Node c)
        {
            ThrowIfDisposed();
            if (c is null) throw new ArgumentNullException(nameof(c));
            if (c._disposed) throw new ObjectDisposedException(c.GetType().Name);

            var kids = MaterializeChildren();
            for (int i = 0; i < kids.Count; i++)
            {
                if (kids[i]._id == c._id) return i;
            }
            return -1;
        }

        /// <summary>
        /// 改 c 在直系子中的位置到 i（DOM 无直接等价，但 fgui/setChildIndex 习惯）。
        /// i ∈ [0, ChildCount-1]：c 的最终位置（在最终排列里）。范围外抛 ArgumentOutOfRangeException。
        /// c 必须是直系子。组合实现：RemoveChild + InsertChild。Remove 后 count=N-1，InsertChild
        /// 在该减表上 i ∈ [0, N-1] 合法——恰好对应原表 i ∈ [0, N-1] 的目标位（见 swap/insert 算法）。
        /// </summary>
        public void SetChildIndex(Node c, int i)
        {
            ThrowIfDisposed();
            if (c is null) throw new ArgumentNullException(nameof(c));
            c.ThrowIfDisposed();
            // 入口校验 i ∈ [0, ChildCount-1]：c 当前占一槽，最终排列总位数 ChildCount，
            // 故 c 的目标位最大为 ChildCount-1。i == ChildCount 拒（语义「排到末尾新位」无意义）。
            int n = ChildCount;
            if ((uint)i >= (uint)n)
                throw new ArgumentOutOfRangeException(nameof(i), i,
                    $"target index {i} out of range [0, {n - 1}]");

            // RemoveChild 内再校验 GetChildIndex != -1（直系子约束）。
            RemoveChild(c);
            // InsertChild 在减表上校验 i ∈ [0, newCount]——这里 i ≤ 原 N-1 = newCount，必通过。
            InsertChild(c, i);
        }

        /// <summary>
        /// 交换两直系子 a/b 的位置。a/b 必须都是直系子（否则 ArgumentException）。
        /// 同一节点（ia == ib）no-op。索引偏移处理：先移高位再移低位（移高位不影低位索引），
        /// 再分别按原索引插回（upper→lower 位、lower→upper 位）。首末 / 相邻 / 含中位 情形
        /// 经 ContainerTreeWriteOpsTests.SwapChildrenSwapsPositions Theory 覆盖（4 case）。
        /// </summary>
        public void SwapChildren(Node a, Node b)
        {
            ThrowIfDisposed();
            if (a is null) throw new ArgumentNullException(nameof(a));
            if (b is null) throw new ArgumentNullException(nameof(b));
            a.ThrowIfDisposed();
            b.ThrowIfDisposed();

            int ia = GetChildIndex(a), ib = GetChildIndex(b);
            if (ia < 0)
                throw new ArgumentException($"node (id={a._id}) is not a child of container (id={_id})", nameof(a));
            if (ib < 0)
                throw new ArgumentException($"node (id={b._id}) is not a child of container (id={_id})", nameof(b));
            if (ia == ib) return;   // 同节点（或同位不同实例，理论不达——registry 身份保证）

            // upper/lower：先移高位（不影低位索引），再移低位。最后按原索引插回：
            //   upperChild → lower 位（占据 a/b 中较前者的原位）
            //   lowerChild → upper 位（占据 a/b 中较后者的原位）
            // 验：首末 / 相邻 / 含中位 情形均经 ContainerTreeWriteOpsTests.SwapChildrenSwapsPositions Theory 覆盖。
            int lower = Math.Min(ia, ib), upper = Math.Max(ia, ib);
            Node lowerChild = (ia < ib) ? a : b;   // 占 lower 位的原始节点
            Node upperChild = (ia < ib) ? b : a;   // 占 upper 位的原始节点

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            // 顺序敏感：先移 upper（不影 lower 索引），再移 lower。
            Native.loomgui_stage_remove_child(h, _id, upperChild._id);
            Native.loomgui_stage_remove_child(h, _id, lowerChild._id);
            // 插 upperChild 到 lower 位：当前 count = N-2，lower ∈ [0, N-2] 合法。
            InsertChild(upperChild, lower);
            // 插 lowerChild 到 upper 位：当前 count = N-1，upper ≤ N-1 合法（== 时 append）。
            InsertChild(lowerChild, upper);
        }

        /// <summary>
        /// 按索引交换两直系子。a/b ∈ [0, ChildCount)——越界抛 ArgumentOutOfRangeException。
        /// 委托给 <see cref="SwapChildren(Node, Node)"/>（GetChildAt 已做范围校验 + 物化）。
        /// </summary>
        public void SwapChildrenAt(int a, int b)
        {
            ThrowIfDisposed();
            // GetChildAt 校验 (uint)i >= (uint)Count 并抛 ArgumentOutOfRangeException——复用其校验路径。
            Node ca = GetChildAt(a);
            Node cb = GetChildAt(b);
            SwapChildren(ca, cb);
        }
        public void ScrollTo(Vector2 p, ScrollBehavior b = ScrollBehavior.Smooth) { throw NE(); }
        // ScrollChanged source 待补：ScrollPane 物理自维护 tween，无 borrow_scroll_events FFI。
        // D3 defer——event 签名冻结（PublicApi 编译门已含此字段），add/remove 推后到 source 补齐。
        public event Action<ScrollChangedEvent> Scrolled;
        public UITemplate GetTemplate(string name) { throw NE(); }   // 取内联 template（原 Panel.GetTemplate 上移）

        // ── helpers ─────────────────────────────────────────────────────

        /// <summary>
        /// 调 get_children 拿当前直系子 NodeId 数组 + 逐个 registry.GetOrCreate 包成 typed Node。
        /// FFI 调用模式复用 C1 <see cref="Node.DisposeDescendantsInRegistry"/>：先 get_child_count
        /// 定 cap，再 get_children 写入 fixed 钉住的 buffer（return-code + out-param，A6 cap 编码）。
        /// 单线程同步内 count 不会 stale；written 防御性 clamp 兜底 ABI 异常。
        /// </summary>
        private List<Node> MaterializeChildren()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();

            int count = Native.loomgui_stage_get_child_count(h, _id);
            var list = new List<Node>(count > 0 ? count : 0);
            if (count <= 0) return list;   // 0 子 / FFI err：返空 list（err post-ThrowIfDisposed 理论不达）

            uint[] buf = new uint[count];
            int written;
            fixed (uint* bp = buf)
            {
                written = Native.loomgui_stage_get_children(h, _id, bp, (nuint)buf.Length);
            }
            // written < 0 = 节点刚被并发移除（单线程理论不达）；防御性早退防读越界。
            if (written < 0) return list;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                // registry 缓存命中返同一实例；未命中走 NodeFactory 造 typed 子类（C1）+ 入缓存。
                list.Add(_ctx._registry.GetOrCreate(buf[i]));
            }
            return list;
        }

        /// <summary>
        /// 清当前直系子（DOM semantics：移除不 Dispose，子可重挂）。snapshot NodeId 列表 + 逐个
        /// remove_child FFI——跳过 RemoveChild 的 GetChildIndex 校验（snapshot 保证是直系子，校验纯开销）。
        /// TextContent setter 用——清后立建新 TextNode。
        /// </summary>
        private void ClearDirectChildrenFFI(StageHandle* h)
        {
            int count = Native.loomgui_stage_get_child_count(h, _id);
            if (count <= 0) return;

            uint[] buf = new uint[count];
            int written;
            fixed (uint* bp = buf)
                written = Native.loomgui_stage_get_children(h, _id, bp, (nuint)buf.Length);
            if (written < 0) return;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
                Native.loomgui_stage_remove_child(h, _id, buf[i]);
        }

        /// <summary>
        /// 递归子树累加 TextNode._text 到 sb（文档序）。Container 子递归；TextNode 叶子累加 _text；
        /// 其它叶子（Image / 控件）贡献 0 字符。TextContent getter 用。
        /// 递归终止：围栏闭合保证无循环引用（parent 指针单向）；深度受场景树规模有界。
        /// </summary>
        static void AppendDescendantText(Container root, StringBuilder sb)
        {
            // Children getter lazy materialize + registry cache：每次访问重新拿最新直系子列表。
            // 递归路径稳——同一 Node 多次入参不会（无环）。
            foreach (Node child in root.Children)
            {
                if (child is TextNode tn) sb.Append(tn._text);
                else if (child is Container c) AppendDescendantText(c, sb);
                // 其它（Image / 控件 / 未知叶子）：跳过。
            }
        }

        static NotImplementedException NE() => new NotImplementedException();
    }

    // AbsolutePanel：自身 relative，AddChild 自动施加 absolute 到子节点。API 与 Container 一致。
    public sealed class AbsolutePanel : Container
    {
        internal AbsolutePanel(UIContext ctx, uint id) : base(ctx, id) { }
    }

    // 注：无 Panel 类型。作用域是运行时标记（IsScopeRoot），非类型；Instantiate 返回模板根真实类型。

    // ── 叶子：内容/绘制 ──
    //
    // TextNode.Text 的读侧是 C# 镜像（_text），不是 core 直读——lib.rs 无 get_text FFI
    // （grep crates/ffi/src/lib.rs 只有 set_text）。setter 同步写穿到 core（set_text 标 dirty_text
    // → 下帧 rematch）+ 缓存到 _text；getter 读 _text。Container.TextContent 读递归子树累加
    // 各 TextNode._text。
    //
    // ponytail: 真值在 core（text_contents HashMap<NodeId, String>），Instantiate 路径把
    // pkg 内文本写入 core 但不通知 C# → 这类 TextNode 的 _text 保持 ""。读镜像返 ""
    // 与 core 实际渲染不一致是已知 ghost state；待首个 Instantiate 文本读回场景落地时
    // 加 get_text FFI（同 C4 set_transform 推后的模式——等真实读回消费者出现再接通，
    // 避免空 flush / 无用 FFI）。业务侧文本交互（Create<TextNode>() + 写 Text）当前路径正确。
    public unsafe class TextNode : Node
    {
        // C# 镜像：默认空串（围栏 create_node("span") 建 TextNode 时 core text_contents 也填 "")。
        // setter 写穿 core（set_text FFI）后更新本字段；getter 直接读，不走 FFI。
        internal string _text = "";

        internal TextNode(UIContext ctx, uint id) : base(ctx, id) { }

        /// <summary>
        /// 文本内容（对应 DOM Text.data / CharacterData.data）。setter 写穿 core（set_text FFI：
        /// UTF-8 编码 + ptr+len，标 dirty_text → 下帧重排文本 runs）+ 缓存镜像；getter 读镜像。
        /// null 当空串处理（与 DOM textContent=null 一致）。Dispose 后访问抛 ObjectDisposedException。
        /// </summary>
        public string Text
        {
            get
            {
                ThrowIfDisposed();
                return _text;
            }
            set
            {
                ThrowIfDisposed();
                string v = value ?? "";
                _text = v;
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                byte[] b = Encoding.UTF8.GetBytes(v);
                fixed (byte* p = b)
                {
                    // rc!=0 仅发生于 null stage / 节点不 live / 非 TextNode——本类 ctor 经 NodeFactory
                    // 据 get_node_kind 派发（kind=TextNode=1 → TextNode ctor），kind 不可变；ThrowIfDisposed
                    // 拦 stale；UTF-8 编码不会产非 UTF-8。故 rc 理论必 0，与 ClassList add/remove 一致不抛。
                    _ = Native.loomgui_stage_set_text(h, _id, p, (nuint)b.Length);
                }
            }
        }
    }
    public class Image : Node
    {
        internal Image(UIContext ctx, uint id) : base(ctx, id) { }

        public string Src { get { throw NE(); } set { throw NE(); } }   // 字符串 key（包内 or 运行时注册）；动态纹理注册归引擎后端
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 容器类文本/标签（TextContent 走 Container 继承）──
    public class TextBlock : Container
    {
        internal TextBlock(UIContext ctx, uint id) : base(ctx, id) { }
    }      // p
    public class TextElement : Container
    {
        internal TextElement(UIContext ctx, uint id) : base(ctx, id) { }
    }    // span/strong/em
    public class Label : Container
    {
        internal Label(UIContext ctx, uint id) : base(ctx, id) { }
    }          // 退化为语义容器：不加 For、不自动聚焦（点标签聚焦用 On<ClickEvent>+Focus() 积木）
    public class Canvas : Container
    {
        internal Canvas(UIContext ctx, uint id) : base(ctx, id) { }
    }         // 引擎渲染挂载点，无绘图 API；集成层 Query<Canvas>() + 读 Geometry.WorldRect 摆摄像机
    public class ListItem : Container
    {
        internal ListItem(UIContext ctx, uint id) : base(ctx, id) { }
        public int Index { get { throw NE(); } } static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 控件（叶子：私有内部结构）──
    public class Button : Container
    {
        internal Button(UIContext ctx, uint id) : base(ctx, id) { }

        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        // 文本走 Container.TextContent（删原 TextContent 特例）

        // D3 semantic sugar：Action 参数无类型——handler 形参与 ClickEvent 解耦，对齐 UGUI Button.onClick。
        // add = On<ClickEvent>(e => value()) 冒泡到自身（button 是 target，bubble 阶段自触）。
        // remove 经 EventRegistration backing 退订（Dictionary<Action,EventRegistration>）。
        [NonSerialized] System.Collections.Generic.Dictionary<Action, EventRegistration> _clickedBacking;
        public event Action Clicked
        {
            add
            {
                if (value == null) return;
                if (_clickedBacking == null)
                    _clickedBacking = new System.Collections.Generic.Dictionary<Action, EventRegistration>();
                if (_clickedBacking.ContainsKey(value)) return;
                var reg = On<ClickEvent>(e => value(), useCapture: false);
                _clickedBacking[value] = reg;
            }
            remove
            {
                if (_clickedBacking != null && _clickedBacking.TryGetValue(value, out var reg))
                {
                    _clickedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Link : Container
    {
        internal Link(UIContext ctx, uint id) : base(ctx, id) { }

        public string Href { get { throw NE(); } set { throw NE(); } }   // 仅存字符串，框架不自动导航

        // D3 semantic sugar：同 Button.Clicked——ClickEvent 冒泡到自身后调 handler。
        [NonSerialized] System.Collections.Generic.Dictionary<Action, EventRegistration> _activatedBacking;
        public event Action Activated
        {
            add
            {
                if (value == null) return;
                if (_activatedBacking == null)
                    _activatedBacking = new System.Collections.Generic.Dictionary<Action, EventRegistration>();
                if (_activatedBacking.ContainsKey(value)) return;
                var reg = On<ClickEvent>(e => value(), useCapture: false);
                _activatedBacking[value] = reg;
            }
            remove
            {
                if (_activatedBacking != null && _activatedBacking.TryGetValue(value, out var reg))
                {
                    _activatedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextField : Node
    {
        internal TextField(UIContext ctx, uint id) : base(ctx, id) { }

        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public TextSelection Selection { get { throw NE(); } set { throw NE(); } }   // 单行也支持选区/光标控制
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        public event Action<string> Submitted;   // 单行回车=提交；多行（TextArea）不提交
        static NotImplementedException NE() => new NotImplementedException();
    }

    // PasswordField / SearchField：<input type="password"> / <input type="search"> 的 typed 投影。
    // 与 TextField 同语义表面（Rust 侧 Task 1 拆分仅服务于 attribute-selector [type=...] 精确匹配，
    // 运行时 API 与 TextField 一致——public-api.md 把三者合并在 input[text/password/search] | TextField
    // 一行；此处分作 sibling 类是投影层为 Rust kind 留 arm 的对齐，待 Task 7 同步 public-api.md）。
    public class PasswordField : Node
    {
        internal PasswordField(UIContext ctx, uint id) : base(ctx, id) { }

        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public TextSelection Selection { get { throw NE(); } set { throw NE(); } }
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        public event Action<string> Submitted;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class SearchField : Node
    {
        internal SearchField(UIContext ctx, uint id) : base(ctx, id) { }

        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public TextSelection Selection { get { throw NE(); } set { throw NE(); } }
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        public event Action<string> Submitted;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class NumberField : Node
    {
        internal NumberField(UIContext ctx, uint id) : base(ctx, id) { }

        public float Value { get { throw NE(); } set { throw NE(); } }
        public float? Min { get { throw NE(); } set { throw NE(); } }
        public float? Max { get { throw NE(); } set { throw NE(); } }
        public float Step { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<float>> ValueChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Slider : Node
    {
        internal Slider(UIContext ctx, uint id) : base(ctx, id) { }

        public float Value { get { throw NE(); } set { throw NE(); } }
        public float Min { get { throw NE(); } set { throw NE(); } }
        public float Max { get { throw NE(); } set { throw NE(); } }
        public float Step { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<float>> ValueChanged;
        public event Action<float> ChangeCommitted;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Toggle : Node
    {
        internal Toggle(UIContext ctx, uint id) : base(ctx, id) { }

        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class RadioButton : Node
    {
        internal RadioButton(UIContext ctx, uint id) : base(ctx, id) { }

        public bool IsChecked { get { throw NE(); } set { throw NE(); } }
        public string Name { get { throw NE(); } }   // 只读：结构性，决定分组语义
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<bool>> CheckedChanged;   // 同组互斥框架自动做；只新选中项触发（对齐 web）
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class TextArea : Node
    {
        internal TextArea(UIContext ctx, uint id) : base(ctx, id) { }

        public string Value { get { throw NE(); } set { throw NE(); } }
        public string Placeholder { get { throw NE(); } set { throw NE(); } }
        public TextSelection Selection { get { throw NE(); } set { throw NE(); } }
        public bool ReadOnly { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<ValueChangedEvent<string>> ValueChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class Dropdown : Node
    {
        internal Dropdown(UIContext ctx, uint id) : base(ctx, id) { }

        public int SelectedIndex { get { throw NE(); } set { throw NE(); } }
        public string SelectedValue { get { throw NE(); } set { throw NE(); } }
        public bool Disabled { get { throw NE(); } set { throw NE(); } }
        public event Action<SelectionChangedEvent> SelectionChanged;
        static NotImplementedException NE() => new NotImplementedException();
    }

    public class ProgressBar : Node
    {
        internal ProgressBar(UIContext ctx, uint id) : base(ctx, id) { }

        public float Value { get { throw NE(); } set { throw NE(); } }
        public float Max { get { throw NE(); } set { throw NE(); } }   // 0 基底，照 <progress> 标准，无 Min
        public bool IsIndeterminate { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── ListView ────────────────────────────────────────────────────
    // 虚拟化是运行时实现决策，不进 HTML。首次设 ItemCount/ItemTemplate/BindItem 即数据驱动+清空设计期 li；
    // 静态/数据驱动强制互斥（越界抛 UIContractException）。
    public class ListView : Container
    {
        internal ListView(UIContext ctx, uint id) : base(ctx, id) { }

        public int ItemCount { get { throw NE(); } set { throw NE(); } }
        public UITemplate ItemTemplate { get { throw NE(); } set { throw NE(); } }
        public Func<int, UITemplate> TemplateSelector { get { throw NE(); } set { throw NE(); } }
        public Action<ListItem, int> BindItem { get { throw NE(); } set { throw NE(); } }
        public int SelectedIndex { get { throw NE(); } set { throw NE(); } }
        public event Action<SelectionChangedEvent> SelectionChanged;
        public void ScrollToItem(int i, ScrollBehavior b = ScrollBehavior.Smooth) { throw NE(); }
        public void RefreshItem(int i) { throw NE(); }
        public void RefreshItems() { throw NE(); }
        public void NotifyInserted(int i, int c = 1) { throw NE(); }
        public void NotifyRemoved(int i, int c = 1) { throw NE(); }
        public void NotifyMoved(int f, int t) { throw NE(); }
        public string ItemExitClass { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 动画 ────────────────────────────────────────────────────────
    // Animation 句柄非长期对象，生命周期 = 那次播放；播放结束句柄失效、hook 自动释放。
    public sealed class Animation
    {
        public string Name { get { throw NE(); } }
        public bool IsPlaying { get { throw NE(); } }
        public float Time { get { throw NE(); } set { throw NE(); } }
        public void Pause() { throw NE(); }
        public void Resume() { throw NE(); }
        public void Stop() { throw NE(); }
        public Animation OnStart(Action cb) { throw NE(); }
        public Animation OnEnd(Action cb) { throw NE(); }
        public Animation OnKey(float pct, Action cb) { throw NE(); }
        public Animation OnHook(string n, Action cb) { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 样式辅助 ────────────────────────────────────────────────────
    // ClassList = Node 的 class 集合投影（Add/Remove/Contains/Toggle/Set/Replace）。
    //
    // 投影层契约（projection §3.2 即时过桥）：class 是低频 UI 事件路径（非每帧热路径），每次操作
    // 直 FFI；无镜像需求——class 状态真相在 core，Contains 直查 has_class FFI（不缓存）。Add/Remove
    // 在 core 标 dirty_mesh（lib.rs:1428/1452）触发下帧 rematch，命中 .foo 规则的节点下帧 cascade
    // 重算 computed_style——本类不参与 tick 时序，调用方自然推进帧。
    public sealed unsafe class ClassList
    {
        // 投影层内部：owner Node。lazy 造时由 Node.Classes 传 this；方法体经它取 stage + NodeId 转调 FFI。
        internal readonly Node _owner;
        internal ClassList(Node owner) { _owner = owner; }

        /// <summary>加 class（重复名 core 侧去重）。直 FFI add_class。</summary>
        public void Add(string name) { _owner.ThrowIfDisposed(); CallAdd(name); }
        /// <summary>移除 class（core 全部匹配；不存在 no-op，对齐 DOM classList.remove）。直 FFI remove_class。</summary>
        public void Remove(string name) { _owner.ThrowIfDisposed(); CallRemove(name); }
        /// <summary>
        /// 查询 class 是否存在。直 FFI has_class tri-state：1=true / 0=false / -1=err。
        /// err 抛 InvalidOperationException——stale/recycled NodeId 是 use-after-dispose 信号，
        /// 不能伪装成"无此 class"掩盖 bug。null stage / 非 UTF-8 也走 err 分支，理论被前置检查拦截。
        /// </summary>
        public bool Contains(string name)
        {
            _owner.ThrowIfDisposed();
            int rc = CallHas(name);
            if (rc < 0) throw new InvalidOperationException("has_class FFI returned error (stale node / null stage / non-UTF-8).");
            return rc == 1;
        }
        /// <summary>翻转：在 → 移除；不在 → 添加。C# 组合（Contains + Add/Remove）。</summary>
        public void Toggle(string name) { _owner.ThrowIfDisposed(); if (Contains(name)) Remove(name); else Add(name); }
        /// <summary>条件加/移除（on=true 加、on=false 移）。C# 组合。</summary>
        public void Set(string name, bool on) { _owner.ThrowIfDisposed(); if (on) Add(name); else Remove(name); }
        /// <summary>原子语义替换：移除 oldName + 添加 newName。C# 组合（两次 FFI，非真原子）。</summary>
        public void Replace(string oldName, string newName) { _owner.ThrowIfDisposed(); Remove(oldName); Add(newName); }

        // ── FFI 转调（ptr+len，A6 编码）─────────────────────────────────
        // 同 StyleMirror：UTF-8 编码 + fixed 钉住 + ptr+len。
        //
        // disposed 防御：每个公共方法入口调 _owner.ThrowIfDisposed()——覆盖"业务 var cl = node.Classes;
        // node.Dispose(); cl.Add(...)"这条跨 Dispose 持引用路径（Node.Classes getter 的 ThrowIfDisposed
        // 只拦 getter 入口，不拦后捕获的 cl）。ClassList 是低频 UI 事件路径，多一次 _disposed 读可忽略。
        //
        // add_class/remove_class 失败静默（rc!=0 仅发生于 null stage / 节点不 live / 非 UTF-8——
        // 前两者 ThrowIfDisposed 已拦，UTF-8 编码不会产非 UTF-8；防御性不抛，与同 assembly 其他
        // FFI 转调一致）。
        // has_class 返 i32 三态（lib.rs:1481）：1=true / 0=false / -1=err——Contains 把 -1 升级为
        // InvalidOperationException（不静默吞：stale NodeId 是 use-after-dispose 信号，不能当"无此 class"）。

        void CallAdd(string name)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(name);
            fixed (byte* p = b)
                Native.loomgui_stage_add_class(h, _owner._id, p, (nuint)b.Length);
        }

        void CallRemove(string name)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(name);
            fixed (byte* p = b)
                Native.loomgui_stage_remove_class(h, _owner._id, p, (nuint)b.Length);
        }

        int CallHas(string name)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(name);
            fixed (byte* p = b)
                return Native.loomgui_stage_has_class(h, _owner._id, p, (nuint)b.Length);
        }
    }

    // StyleSheet 逃生舱：Add 返回 IDisposable 句柄，撤销靠 Dispose（不靠原文匹配）。
    public class StyleSheet
    {
        public IDisposable Add(string css) { throw NE(); }
        public void Clear() { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 模板 ────────────────────────────────────────────────────────
    public sealed unsafe class UITemplate
    {
        // 投影层内部字段：持有上下文 + 包名 + 模板路径。
        // Name 返 _path（模板路径即名称）；Instantiate 经 _ctx FFI instantiate(_pkg, _path)。
        internal readonly UIContext _ctx;
        internal readonly string _pkg;
        internal readonly string _path;

        internal UITemplate(UIContext ctx, string pkg, string path)
        {
            _ctx = ctx; _pkg = pkg; _path = path;
        }

        public string Name => _path;
        public Container Instantiate()
        {
            if (_ctx._stage == IntPtr.Zero)
                throw new ObjectDisposedException(nameof(UIContext));
            return DoInstantiate(_ctx, _pkg, _path);
        }

        /// <summary>
        /// 调 instantiate FFI → 根 NodeId → registry.GetOrCreate → typed Container。
        /// UIPackage.Instantiate 和 UITemplate.Instantiate 共享同一实现。
        /// </summary>
        internal static Container DoInstantiate(UIContext ctx, string pkg, string path)
        {
            StageHandle* h = (StageHandle*)ctx._stage.ToPointer();
            byte[] pb = Encoding.UTF8.GetBytes(pkg);
            byte[] cb = Encoding.UTF8.GetBytes(path);
            uint rootId;
            fixed (byte* pp = pb)
            fixed (byte* cp = cb)
                rootId = Native.loomgui_stage_instantiate(h, pp, (nuint)pb.Length, cp, (nuint)cb.Length);
            if (rootId == Node.RootSentinel)
                throw new UIPackageException(
                    $"instantiate failed: pkg='{pkg}' comp='{path}' " +
                    "(package not loaded / component not found / no scene created)");
            return (Container)ctx._registry.GetOrCreate(rootId);
        }
    }

    // ── 顶层上下文 ──────────────────────────────────────────────────
    // UIContext 是「获取而非创建」：无公共构造，由引擎集成层创建/驱动。业务程序员从集成层获取。
    public sealed unsafe class UIContext
    {
        // B3：headless harness / 引擎集成层建 UIContext 时持有的 Stage 句柄（raw FFI handle）。
        // 投影层（C1+）通过它转调 loomgui_stage_* FFI；公共 API 表面看不到本字段。
        internal IntPtr _stage;

        // C1：NodeId → typed Node 的强引用身份缓存（投影层 §2.4）。
        // NodeFactory 造节点入缓存；Node.Dispose 时 evict。公共 API 不见本字段。
        internal readonly NodeRegistry _registry;

        // D2：typed 事件订阅表 + capture/bubble/once 路由。Node.On<T> 经此转调 Subscribe<T>；
        // D3 demux 翻译 raw LoomEvent → typed struct 后调 Dispatch<T>。公共 API 不见本字段。
        internal readonly EventBus _eventBus;

        // D3：raw LoomEvent stream → typed event struct demux。LoomHost.Step 调 Pump 每帧
        // 翻译 borrow_events buffer → EventBus.Dispatch。公共 API 不见本字段。
        internal readonly EventDemuxer _eventDemuxer;

        // E1：create_root FFI 返回的根 NodeId。由 harness/集成层调 create_root 后写入本字段；
        // Root getter 据此返回 typed Container。无公共 FFI 直接读 roots[0]——Rust 侧 roots Vec
        // 未暴露 getter，故投影层需自己跟踪。
        internal uint _rootId = Node.RootSentinel;

        // E1：已加载包名集合（load_package 时加入，unload_package 时移除）。
        // 用于同名重复检测（公共契约：LoadPackage 同名重复抛 UIContractException）。
        internal readonly HashSet<string> _loadedPackages = new HashSet<string>();

        // E1：lazy 创建的 StyleSheet 实例。同 Node.Style/Node.Transform 模式——未访问过 = null，
        // 首次访问构造并挂本 context。StyleSheet.Add/Clear 方法体本身仍 throw NE（core 未接通）。
        StyleSheet _styleSheet;

        // B3：headless harness 工厂构造。public API 无构造（业务从集成层拿现成 instance）。
        // 建 NodeRegistry 持有自身反向引用（registry 转调 FFI 时需 stage handle）。
        // 建 EventBus + EventDemuxer 同持自身反向引用。
        internal UIContext(IntPtr stage)
        {
            _stage = stage;
            _registry = new NodeRegistry(this);
            _eventBus = new EventBus(this);
            _eventDemuxer = new EventDemuxer(this);
        }

        /// <summary>
        /// 场景根节点（Container）。create_root FFI 建根后由 harness/集成层写入 _rootId；
        /// 若 _rootId 尚未设置（根未建），getter 读不到合法值——_rootId 仍是 RootSentinel
        /// （0xFFFF_FFFF），registry.GetOrCreate 会产无意义的 wrapper。调用方需确保 create_root
        /// 先于 Root 访问（集成层保证此顺序）。
        /// </summary>
        public Container Root
        {
            get
            {
                if (_rootId == Node.RootSentinel)
                    throw new InvalidOperationException(
                        "Root accessed before create_root (harness/integration layer must create_root first)");
                return (Container)_registry.GetOrCreate(_rootId);
            }
        }

        /// <summary>
        /// 当前焦点节点。FFI loomgui_stage_focused_node 返 NodeId（无焦点 → sentinel）。
        /// 返 null 当无焦点（DOM document.activeElement 为 body 的习惯：LoomGUI 返 null 而非抛异常）。
        /// </summary>
        public Node FocusedNode
        {
            get
            {
                if (_stage == IntPtr.Zero) return null;
                StageHandle* h = (StageHandle*)_stage.ToPointer();
                uint id = Native.loomgui_stage_focused_node(h);
                if (id == Node.RootSentinel) return null;
                return _registry.GetOrCreate(id);
            }
        }

        /// <summary>
        /// 样式逃生舱（动态 CSS 规则注入）。lazy 造单一实例：同一 UIContext 多次访问返同一 StyleSheet。
        /// StyleSheet.Add(string css) 返回 IDisposable 句柄，撤销靠 Dispose（不靠原文匹配）。
        /// StyleSheet.Add/Clear 方法体当前 throw NE——core 未接通动态 CSS 注入通道（ponytail defer）。
        /// </summary>
        public StyleSheet StyleSheet
        {
            get
            {
                _styleSheet ??= new StyleSheet();
                return _styleSheet;
            }
        }

        /// <summary>
        /// 装载包（pkg.bin bytes → 注册模板到 Rust stage）。同名重复抛 UIContractException；
        /// 内部失败（格式错 / 重复 pkg id / 资源缺失）抛 UIPackageException。
        /// null/空 name 直接抛（与 DOM getElementById 习惯一致：空 id 是调用方写错）。
        /// </summary>
        public UIPackage LoadPackage(string name, byte[] bytes)
        {
            if (string.IsNullOrEmpty(name))
                throw new ArgumentNullException(nameof(name));
            if (bytes == null || bytes.Length == 0)
                throw new ArgumentNullException(nameof(bytes));
            if (_loadedPackages.Contains(name))
                throw new UIContractException(
                    $"package '{name}' is already loaded. Unload it first or use a different name.");

            StageHandle* h = (StageHandle*)_stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name);
            int rc;
            fixed (byte* np = nb)
            fixed (byte* bp = bytes)
                rc = Native.loomgui_stage_load_package(h, np, (nuint)nb.Length, bp, (nuint)bytes.Length);
            if (rc != 0)
                throw new UIPackageException(
                    $"load_package '{name}' failed (malformed pkg.bin / duplicate pkg id / missing resources)");

            _loadedPackages.Add(name);
            return new UIPackage(this, name);
        }

        /// <summary>
        /// 卸载包：从 Rust stage 移除模板注册。已实例化的活节点独立副本不受影响
        /// （同 Unity prefab：删 prefab 不删已实例化的 GO）。
        /// ponytail: lib.rs 无 unload_package FFI——待 Rust 侧加 Stage::unload_package 后接通。
        /// 届时 C# 侧只需加一句 Native.loomgui_stage_unload_package(h, name) + _loadedPackages.Remove。
        /// </summary>
        public void UnloadPackage(string name)
        {
            // ponytail: no loomgui_stage_unload_package FFI yet.
            // When Rust side adds Stage::unload_package(name), wire here:
            //   StageHandle* h = (StageHandle*)_stage.ToPointer();
            //   byte[] nb = Encoding.UTF8.GetBytes(name ?? "");
            //   fixed (byte* np = nb)
            //       Native.loomgui_stage_unload_package(h, np, (nuint)nb.Length);
            //   _loadedPackages.Remove(name);
            throw new NotImplementedException(
                "UnloadPackage: no loomgui_stage_unload_package FFI yet (ponytail defer). " +
                "Will wire when Rust side adds Stage::unload_package.");
        }

        /// <summary>
        /// 建类型化节点（不挂父）。白名单：Container, AbsolutePanel, TextNode, Image。
        /// 非法 T（Button / Slider / Toggle / ListView 等控件或作用域根）
        /// 抛 UIContractException——控件只能 Instantiate（含内建子树），不能裸建。
        ///
        /// tag 映射（对齐 core dynamic.rs::kind_from_tag）：
        /// Container/AbsolutePanel → "div", TextNode → "span", Image → "img"。
        /// Button 虽在 kind_from_tag 白名单但 E1 不列入 Create<T>——Button 带内建子树，
        /// 裸建 produce 无 label 的残缺按钮，Instantiate 是唯一路径。
        /// </summary>
        public T Create<T>() where T : Node
        {
            Type t = typeof(T);
            string tag;
            if (t == typeof(Container) || t == typeof(AbsolutePanel))
                tag = "div";
            else if (t == typeof(TextNode))
                tag = "span";
            else if (t == typeof(Image))
                tag = "img";
            else
                throw new UIContractException(
                    $"Create<{t.Name}> is not allowed. Create<T> whitelist: " +
                    "Container, AbsolutePanel, TextNode, Image. " +
                    "Controls (Button, Slider, Toggle, ...) must be Instantiate'd, not Create'd.");

            StageHandle* h = (StageHandle*)_stage.ToPointer();
            byte[] tb = Encoding.UTF8.GetBytes(tag);
            uint id;
            fixed (byte* tp = tb)
                id = Native.loomgui_stage_create_node(h, tp, (nuint)tb.Length, null, 0);
            if (id == Node.RootSentinel)
                throw new InvalidOperationException(
                    $"create_node(\"{tag}\") failed (stage null / non-UTF-8 / unknown kind)");

            // AbsolutePanel：Rust 侧 kind 是 Container，但 C# 需 AbsolutePanel 子类实例。
            // NodeFactory 据 kind 派发到 Container ctor——此处绕过 registry.GetOrCreate，
            // 手动造 AbsolutePanel + 注册到 registry（id 是刚 create_node 的新鲜 id，不会覆盖已有缓存）。
            if (t == typeof(AbsolutePanel))
            {
                var panel = new AbsolutePanel(this, id);
                _registry.Register(id, panel);
                return (T)(Node)panel;
            }
            return (T)_registry.GetOrCreate(id);
        }

        /// <summary>
        /// 命中测试：返回 globalPoint 处最上层可 Touchable 节点。
        /// ponytail: lib.rs 无 hit_test / pick FFI。core hit_test 走 Node::hit_test 递归，
        /// 依赖上帧 world_matrix，但未暴露为 FFI。待 Rust 侧加 loomgui_stage_hit_test(h, x, y) → NodeId
        /// 后接通。现阶段 C# 业务可走 Geometry.WorldRect + 手工 Contains 做近似命中（简陋但有）。
        /// </summary>
        public Node Pick(Vector2 globalPoint)
        {
            // ponytail: no loomgui_stage_hit_test FFI.
            // When Rust adds Stage::hit_test(x,y) → Option<NodeId>, wire:
            //   StageHandle* h = (StageHandle*)_stage.ToPointer();
            //   uint id = Native.loomgui_stage_hit_test(h, globalPoint.X, globalPoint.Y);
            //   if (id == Node.RootSentinel) return null;
            //   return _registry.GetOrCreate(id);
            throw new NotImplementedException(
                "Pick: no loomgui_stage_hit_test FFI yet (ponytail defer). " +
                "Will wire when Rust side adds Stage::hit_test.");
        }

        /// <summary>
        /// 延迟回调（秒）。同 DOM setTimeout——d 秒后调 cb（不精确，帧级粒度）。
        /// ponytail: core 无 call_later / timer queue。动画时钟仅 TweenManager::update(dt)，
        /// 无通用延迟回调基础设施。待 Rust 侧加 timer 队列后接通。
        /// </summary>
        public void CallLater(float d, Action cb)
        {
            // ponytail: no timer queue in Rust yet.
            throw new NotImplementedException(
                "CallLater: no timer queue FFI yet (ponytail defer). " +
                "Will wire when Rust side adds Stage::call_later(dt, cb).");
        }

        /// <summary>
        /// 下帧回调（帧末 fire，先于 render）。ponytail defer——理由同 CallLater。
        /// </summary>
        public void CallNextFrame(Action cb)
        {
            // ponytail: no per-frame deferred callback queue in Rust yet.
            throw new NotImplementedException(
                "CallNextFrame: no next-frame queue FFI yet (ponytail defer). " +
                "Will wire when Rust side adds Stage::call_next_frame.");
        }

        /// <summary>
        /// 当前是否有指针在 UI 上（命中任意 Touchable 节点）。
        /// 直透传 loomgui_stage_is_pointer_on_ui FFI（lib.rs:399）。
        /// null stage → false（防御性——_stage 不应为 null，但容错不抛）。
        /// </summary>
        public bool IsPointerOnUI
        {
            get
            {
                if (_stage == IntPtr.Zero) return false;
                StageHandle* h = (StageHandle*)_stage.ToPointer();
                return Native.loomgui_stage_is_pointer_on_ui(h);
            }
        }

        static NotImplementedException NE() => new NotImplementedException();
    }

    public sealed class UIPackage
    {
        // 投影层内部：持有上下文 + 包名。Instantiate/GetTemplate 经 _ctx 转调 FFI。
        internal readonly UIContext _ctx;
        internal readonly string _name;

        internal UIPackage(UIContext ctx, string name)
        {
            _ctx = ctx; _name = name;
        }

        /// <summary>
        /// 包名（load_package 时指定）。只读——包名是 load 期确定的身份标识，不可变。
        /// </summary>
        public string Name => _name;

        /// <summary>
        /// 从包内克隆组件到当前 scene，返回模板根（typed Container）。
        /// path = 组件路径（HTML 文件名，如 "pages/main.html"）。包必须已 load_package 过
        /// （否则 FFI 返 sentinel → UIPackageException）。
        /// </summary>
        public Container Instantiate(string path)
        {
            if (string.IsNullOrEmpty(path))
                throw new ArgumentNullException(nameof(path));
            return UITemplate.DoInstantiate(_ctx, _name, path);
        }

        /// <summary>
        /// 取模板句柄（不实例化）。返回持有 pkg+path 的 UITemplate，
        /// 供 ListView.ItemTemplate 等延迟实例化场景用。
        /// null/空 path 直接抛。
        /// </summary>
        public UITemplate GetTemplate(string path)
        {
            if (string.IsNullOrEmpty(path))
                throw new ArgumentNullException(nameof(path));
            return new UITemplate(_ctx, _name, path);
        }
    }
}
