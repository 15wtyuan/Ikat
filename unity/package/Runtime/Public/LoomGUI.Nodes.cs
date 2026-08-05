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

        /// <summary>
        /// 程序化播放 @keyframes 动画（spec §7.3 / public-api §9.1 触发 2）。返 Animation 句柄。
        ///
        /// 建 programmatic player（core <c>play_programmatic</c>，不受 class 声明管）：
        /// 默认 1s / 无 delay / 单次迭代 / normal / fill both / cubic-out，立即写首帧。
        /// 结束用句柄 <see cref="Animation.OnEnd"/> 或 <c>On&lt;AnimationEndEvent&gt;()</c>；
        /// class 触发的动画无句柄（声明式，只需知结束走 EventBus 订阅）。
        ///
        /// 未知动画名（keyframes 表无此 name）抛 <see cref="UIContractException"/>（调用方
        /// 写错——同 Get&lt;T&gt; 未命中语义）；null name 抛 ArgumentNullException。
        /// </summary>
        public Animation Play(string name)
        {
            ThrowIfDisposed();
            if (name == null) throw new ArgumentNullException(nameof(name));
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name);
            fixed (byte* np = nb)
            {
                ulong key = Native.loomgui_stage_play_animation(h, _id, np, (nuint)nb.Length);
                if (key == 0)
                    throw new UIContractException(
                        $"Play(\"{name}\"): no @keyframes with this name (keyframes table lookup failed)");
                var anim = new Animation(this, key, name);
                _ctx.RegisterAnimation(anim);
                return anim;
            }
        }

        // 编程聚焦节点（照 fgui RequestFocus）。直转 FFI request_focus（记 pending_focus_request，
        // 下 tick 最前消费写 scene.focused_node + 产 FocusIn/FocusOut）。文本框聚焦后才能接收
        // set_key_input / set_text_input 的输入（core input 只插焦点控件）。
        public void Focus()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_request_focus(h, _id);
        }
        // Blur 清除当前焦点（stage::blur）：记 pending_focus_request = Some(None)，下 tick 消费清焦点
        // （与 request_focus 对称的 stage 级操作）。FFI loomgui_stage_blur 不带 node_id——它清的是
        // 「当前获焦节点」而非「本节点」，故无焦点时调为 no-op（业务侧通常对聚焦控件调 Blur）。
        public void Blur()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_blur(h);
        }

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
            "div" => NodeKind.Container,
            "span" => NodeKind.TextElement,
            "button" => NodeKind.Button,
            "img" => NodeKind.Image,
            "input" => NodeKind.TextField,       // 默认 type=text；派生 kind 不命中（4a 简化）
            "textarea" => NodeKind.TextArea,
            "select" => NodeKind.Dropdown,
            "option" => NodeKind.OptionItem,
            "progress" => NodeKind.ProgressBar,
            "ul" => NodeKind.ListView,
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
    // 攒批 flush（Task 9）：setter 存镜像 + 标脏 + 注册到 NodeRegistry dirty 集；帧末
    // （LoomHost.Step flush seam / UIContext.FlushPendingWrites）调 FlushTransform 一次性送
    // set_transform FFI（9-arg：tx,ty,sx,sy,rot,ox,oy）。core compute_world_transforms 并入 local_transform。
    // 整值替换语义（非累加）：每次 flush 送全 4 字段，不需要增量。本类签名零改动——只加帧末 flush。
    public sealed unsafe class NodeTransform
    {
        // 投影层内部：owner Node。lazy 造时由 Node.Transform 传 this；getter/setter 经它走 FFI
        // （owner._ctx._stage + owner._id 转调 set_transform）。
        internal readonly Node _owner;

        // 镜像值：setter 写、getter 读。default 按业务语义初始化（Scale=One 不缩放，其它 Zero）。
        // 帧末 flush 前读到的是 C# 侧最近一次写入的快照（getter 不依赖 core 状态）。
        internal Vector2 _position = Vector2.Zero;
        internal Vector2 _scale = Vector2.One;
        internal float _rotation;
        internal Vector2 _origin = Vector2.Zero;
        // dirty 标志：Store 置 true；FlushTransform 末尾置 false。配合 NodeRegistry dirty 集。
        internal bool _dirty;

        internal NodeTransform(Node owner) { _owner = owner; }

        /// <summary>位移（local 坐标，px）。setter 存镜像 + 标脏（帧末 flush 到 core）。</summary>
        public Vector2 Position { get => _position; set => Store(ref _position, value); }
        /// <summary>缩放（local 基）。default = One（不缩放）；setter 存镜像 + 标脏。</summary>
        public Vector2 Scale { get => _scale; set => Store(ref _scale, value); }
        /// <summary>旋转（弧度，绕 Origin）。setter 存镜像 + 标脏。</summary>
        public float Rotation { get => _rotation; set => Store(ref _rotation, value); }
        /// <summary>旋转/缩放原点（local 坐标，px）。setter 存镜像 + 标脏。</summary>
        public Vector2 Origin { get => _origin; set => Store(ref _origin, value); }

        // 统一 setter 路径：写镜像 + 标脏 + 注册 dirty 集（帧末集中 flush）。
        void Store<T>(ref T field, T value)
        {
            field = value;
            _dirty = true;
            _owner._ctx._registry.MarkTransformDirty(_owner);
        }

        /// <summary>
        /// 帧末 flush：送全 4 字段到 set_transform FFI（整值替换，非累加）+ 清 _dirty。
        /// 由 NodeRegistry.FlushDirtyTransforms 遍历 dirty 集调。null stage / dead NodeId 静默返 -1（防御）。
        /// </summary>
        internal void FlushTransform()
        {
            _dirty = false;
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            Native.loomgui_stage_set_transform(
                h, _owner._id,
                _position.X, _position.Y,
                _scale.X, _scale.Y,
                _rotation,
                _origin.X, _origin.Y);
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
    public class TextElement : Container
    {
        internal TextElement(UIContext ctx, uint id) : base(ctx, id) { }
    }    // span
    public class ListItem : Container
    {
        internal ListItem(UIContext ctx, uint id) : base(ctx, id) { }
        // 业务逻辑项序号（tick-drain BindItem 时由 UIContext 回填，不走 FFI）。
        // core 不存该值；item_index 进 pending_binds 队列，C# 取后传给本属性。
        internal int _index;
        public int Index => _index;
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

    // 文本控件 FFI 通道共享层。TextField/TextArea 两类投影原先各拷一份
    // 9 个 FFI helper（get/set text/placeholder、get/set selection、set readonly、set disabled、
    // 双调法 ReadText）+ ReadTextFn 委托——收口到本单一真相源防漂移。投影类仅保留薄转调。
    internal static unsafe class TextControlFFI
    {
        // get_control_text/get_control_placeholder 是 return-code + out-param 双调法：
        // buf_cap 足够 → rc=0；不够 → rc=-2 + *out_len=所需（扩容重调）；非文本/null → -1。
        internal static string GetControlText(StageHandle* h, uint id) =>
            ReadText(h, id, (hp, buf, cap, len) => Native.loomgui_stage_get_control_text(hp, id, buf, cap, len));

        internal static void SetControlText(StageHandle* h, uint id, string v)
        {
            byte[] b = Encoding.UTF8.GetBytes(v ?? "");
            fixed (byte* bp = b)
            {
                int rc = Native.loomgui_stage_set_control_text(h, id, bp, (nuint)b.Length);
                if (rc != 0) throw new InvalidOperationException($"set_control_text failed (node {id})");
            }
        }

        internal static string GetControlPlaceholder(StageHandle* h, uint id) =>
            ReadText(h, id, (hp, buf, cap, len) => Native.loomgui_stage_get_control_placeholder(hp, id, buf, cap, len));

        internal static void SetControlPlaceholder(StageHandle* h, uint id, string v)
        {
            byte[] b = Encoding.UTF8.GetBytes(v ?? "");
            fixed (byte* bp = b)
            {
                int rc = Native.loomgui_stage_set_control_placeholder(h, id, bp, (nuint)b.Length);
                if (rc != 0) throw new InvalidOperationException($"set_control_placeholder failed (node {id})");
            }
        }

        internal static TextSelection GetSelection(StageHandle* h, uint id)
        {
            nuint start = 0, end = 0;
            int rc = Native.loomgui_stage_get_selection(h, id, &start, &end);
            if (rc != 0) throw new InvalidOperationException($"get_selection failed (node {id})");
            return new TextSelection((int)start, (int)end);
        }

        internal static void SetSelection(StageHandle* h, uint id, int anchor, int cursor)
        {
            int rc = Native.loomgui_stage_set_selection(h, id, (nuint)anchor, (nuint)cursor);
            if (rc != 0) throw new InvalidOperationException($"set_selection failed (node {id})");
        }

        internal static void SetControlReadonly(StageHandle* h, uint id, bool v)
        {
            int rc = Native.loomgui_stage_set_control_readonly(h, id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_readonly failed (node {id})");
        }

        // get_control_readonly：return-code + byte* out（与 set 对称的读出口）。TextField / TextArea /
        // NumberField 共享 EditState，故三者皆读。非文本控件 / 节点缺失 / null out → rc=-1；命中 → rc=0
        // 且 *out 已填（0/1）。rc<0 升异常不吞（post-ThrowIfDisposed 理论不达）。
        internal static bool GetControlReadonly(StageHandle* h, uint id)
        {
            byte b = 0;
            int rc = Native.loomgui_stage_get_control_readonly(h, id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_control_readonly failed (node {id}, non-text?)");
            return b != 0;
        }

        internal static void SetNodeDisabled(StageHandle* h, uint id, bool v)
        {
            Native.loomgui_stage_set_node_disabled(h, id, v);
        }

        // get_node_disabled：void + byte* out（与 set 对称的读出口）。null 句柄 / 节点缺失 → 写 0（false），
        // 不报错（与 set 的「悬空 NodeId 静默跳过」语义一致）。所有 Node 子类的 Disabled getter 经此。
        internal static bool GetNodeDisabled(StageHandle* h, uint id)
        {
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, id, &b);
            return b != 0;
        }

        // get_control_text/get_control_placeholder 共用的双调法：fn(h, buf, cap, out_len) → rc。
        // 先 stackalloc 256 探；rc=-2 时 *out_len = 所需 → 堆分配按所需重调一次（必合）。非文本/-1 升异常。
        // FFI 写恰好 out_len 字节（copy_nonoverlapping，无 NUL 填充）——不做 TrimEnd('\0')，信任契约
        // （用户合法设 Value 含 '\0' 也不被静默截断；之前防御性 trim 是死代码 + 值腐化风险）。
        internal static string ReadText(StageHandle* h, uint id, ReadTextFn fn)
        {
            nuint needed = 0;
            // stack 探（256 字节够绝大多数 placeholder / 短 value）。
            Span<byte> stackBuf = stackalloc byte[256];
            fixed (byte* sbp = stackBuf)
            {
                int rc = fn(h, sbp, (nuint)stackBuf.Length, &needed);
                if (rc == 0) return Encoding.UTF8.GetString(stackBuf.Slice(0, (int)needed));
                if (rc != -2) throw new InvalidOperationException($"read text failed rc={rc} (node {id})");
            }
            // 不够：按 needed 堆分配重调。
            byte[] heapBuf = new byte[(int)needed];
            fixed (byte* hbp = heapBuf)
            {
                nuint written = 0;
                int rc = fn(h, hbp, (nuint)heapBuf.Length, &written);
                if (rc != 0) throw new InvalidOperationException($"read text retry failed rc={rc} (node {id})");
                return Encoding.UTF8.GetString(heapBuf, 0, (int)written);
            }
        }

        internal delegate int ReadTextFn(StageHandle* h, byte* buf, nuint cap, nuint* outLen);
    }

    public unsafe class TextField : Node
    {
        internal TextField(UIContext ctx, uint id) : base(ctx, id) { }

        // Value：编程 setter（照 JS `.value =`）直替换 EditState.value + 光标移末尾；getter 双调法读 UTF-8。
        public string Value
        {
            get { ThrowIfDisposed(); return GetControlText(); }
            set { ThrowIfDisposed(); SetControlText(value); }
        }
        // Placeholder：value 为空时渲染它（渲染侧逻辑，core 仅存串）。同 Value 的双调法。
        public string Placeholder
        {
            get { ThrowIfDisposed(); return GetControlPlaceholder(); }
            set { ThrowIfDisposed(); SetControlPlaceholder(value); }
        }
        // Selection：选区 [Start,End)（字节偏移）。setter 直转 set_selection(anchor,cursor)；
        // getter get_selection 归一为 [start,end]（start≤end）。单行框也支持选区/光标控制。
        public TextSelection Selection
        {
            get { ThrowIfDisposed(); return GetSelection(); }
            set { ThrowIfDisposed(); SetSelection(value.Start, value.End); }
        }
        // readonly：true = 用户不可编辑（拦输入 / 退格 / 粘贴），但编程 setter Value 仍可写
        // （HTML JS 语义）。getter 读 core EditState.readonly（get_control_readonly，与 set 对称）。
        public bool ReadOnly
        {
            set { ThrowIfDisposed(); SetControlReadonly(value); }
            get { ThrowIfDisposed(); return GetControlReadonly(); }
        }
        // disabled：伪类源 + active/click 抑制（set_node_disabled）。getter 读 NodeFlags::DISABLED
        // （get_node_disabled，与 set 对称）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // ValueChanged：文本框值变更（core EVT_VALUE_CHANGED=22）。文本框的 EventRecord 不携值
        // （x=0，与 Slider 的 x=新值 不同）——订阅 ControlValueChangedEvent，在触发时回读当前
        // value（get_control_text）填 ValueChangedEvent<string>。backing-dict 模式同 Button.Clicked。
        [NonSerialized] Dictionary<Action<ValueChangedEvent<string>>, EventRegistration> _valueChangedBacking;
        public event Action<ValueChangedEvent<string>> ValueChanged
        {
            add
            {
                if (value == null) return;
                if (_valueChangedBacking == null)
                    _valueChangedBacking = new Dictionary<Action<ValueChangedEvent<string>>, EventRegistration>();
                if (_valueChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlValueChangedEvent>(e =>
                    value(new ValueChangedEvent<string> { _newValue = GetControlText() }));
                _valueChangedBacking[value] = reg;
            }
            remove
            {
                if (_valueChangedBacking != null && _valueChangedBacking.TryGetValue(value, out var reg))
                {
                    _valueChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }

        // Submitted：单行框回车提交（core EVT_SUBMITTED=25）。订阅 ControlSubmittedEvent，
        // 在触发时回读当前 value 填 Action<string>。backing-dict 同 ValueChanged。
        [NonSerialized] Dictionary<Action<string>, EventRegistration> _submittedBacking;
        public event Action<string> Submitted
        {
            add
            {
                if (value == null) return;
                if (_submittedBacking == null)
                    _submittedBacking = new Dictionary<Action<string>, EventRegistration>();
                if (_submittedBacking.ContainsKey(value)) return;
                var reg = On<ControlSubmittedEvent>(e => value(GetControlText()));
                _submittedBacking[value] = reg;
            }
            remove
            {
                if (_submittedBacking != null && _submittedBacking.TryGetValue(value, out var reg))
                {
                    _submittedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // ── FFI 转调 ────────────────────────────────────────────────────────
        // FFI 通道收口在 TextControlFFI（四类文本框共享单一真相源：双调法 ReadText + set/get 直转 +
        // rc 升异常）。本类仅薄转调：Handle() 取 stage 句柄，TextControlFFI.X(h, _id, ...) 直转。
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        string GetControlText() => TextControlFFI.GetControlText(Handle(), _id);
        void SetControlText(string v) => TextControlFFI.SetControlText(Handle(), _id, v);
        string GetControlPlaceholder() => TextControlFFI.GetControlPlaceholder(Handle(), _id);
        void SetControlPlaceholder(string v) => TextControlFFI.SetControlPlaceholder(Handle(), _id, v);
        TextSelection GetSelection() => TextControlFFI.GetSelection(Handle(), _id);
        void SetSelection(int anchor, int cursor) => TextControlFFI.SetSelection(Handle(), _id, anchor, cursor);
        void SetControlReadonly(bool v) => TextControlFFI.SetControlReadonly(Handle(), _id, v);
        bool GetControlReadonly() => TextControlFFI.GetControlReadonly(Handle(), _id);
        void SetNodeDisabled(bool v) => TextControlFFI.SetNodeDisabled(Handle(), _id, v);
        bool GetNodeDisabled() => TextControlFFI.GetNodeDisabled(Handle(), _id);
    }

    public unsafe class NumberField : Node
    {
        internal NumberField(UIContext ctx, uint id) : base(ctx, id) { }

        // Value：直转 NumberField 专用 FFI（get/set_number_value）。setter 在 core 侧做 clamp[min,max]
        // + step 量化后写回 EditState.value 文本（与 Slider set_control_value 同口径，只是 NumberField
        // 存文本、Slider 存 f32）。getter 解析文本→f32。故 C# 侧只透传，不做 clamp/量化。
        public float Value
        {
            get { ThrowIfDisposed(); return GetNumberValue(); }
            set { ThrowIfDisposed(); SetNumberValue(value); }
        }
        // Min/Max/Step：core ControlState::NumberField 存了 min/max/step（打包期 ControlInit 烘焙，
        // set_number_value 据此 clamp+量化）。getter 复用 Slider 的 get_control_min/max/step FFI——FFI 侧
        // pattern-match 已扩到 NumberField（见 c55389d）。三者打包期冻结、运行时不可变：core 无 NumberField
        // 专用 setter（value 存 EditState 文本，setter 须 parse→clamp→quantize→re-format，留待后续），
        // 故 getter 通、setter throw NE，与 Slider 同 get+set 形状但 setter 锁住只读语义（同 RadioButton.Name）。
        public float Min { get { ThrowIfDisposed(); return GetControlMin(); } set { throw NE(); } }
        public float Max { get { ThrowIfDisposed(); return GetControlMax(); } set { throw NE(); } }
        public float Step { get { ThrowIfDisposed(); return GetControlStep(); } set { throw NE(); } }
        // ReadOnly：NumberField 与 TextField/TextArea 共享 EditState（get_control_readonly 按 node 派发）。
        // setter 直转 FFI；getter 读 EditState.readonly（与 set 对称）。
        public bool ReadOnly
        {
            set { ThrowIfDisposed(); SetControlReadonly(value); }
            get { ThrowIfDisposed(); return GetControlReadonly(); }
        }
        // Disabled：伪类源 + active/click 抑制（set_node_disabled）。getter 读 NodeFlags::DISABLED。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // ValueChanged：值变更事件（core EVT_VALUE_CHANGED = 22，与 Slider 同事件，x=新值）。backing-dict
        // 模式同 Slider/Slider.ValueChanged——订阅 internal ControlValueChangedEvent，翻译为公共
        // ValueChangedEvent<float>（NewValue 取 demux 解出的 float）。
        [NonSerialized] Dictionary<Action<ValueChangedEvent<float>>, EventRegistration> _valueChangedBacking;
        public event Action<ValueChangedEvent<float>> ValueChanged
        {
            add
            {
                if (value == null) return;
                if (_valueChangedBacking == null)
                    _valueChangedBacking = new Dictionary<Action<ValueChangedEvent<float>>, EventRegistration>();
                if (_valueChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlValueChangedEvent>(e => value(new ValueChangedEvent<float> { _newValue = e.Value }));
                _valueChangedBacking[value] = reg;
            }
            remove
            {
                if (_valueChangedBacking != null && _valueChangedBacking.TryGetValue(value, out var reg))
                {
                    _valueChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // ── FFI 转调 ───────────────────────────────────────────────────────────
        // value：NumberField 专用通道（clamp+量化在 core）。float out 经 local + &local（同 GetControlValue）。
        float GetNumberValue()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.loomgui_stage_get_number_value(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_number_value failed (node {_id})");
            return v;
        }
        void SetNumberValue(float v)
        {
            StageHandle* h = Handle();
            int rc = Native.loomgui_stage_set_number_value(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_number_value failed (node {_id})");
        }
        // readonly/disabled 经 TextControlFFI（readonly 共享 EditState 通道，disabled 经 node flag）。
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        void SetControlReadonly(bool v) => TextControlFFI.SetControlReadonly(Handle(), _id, v);
        bool GetControlReadonly() => TextControlFFI.GetControlReadonly(Handle(), _id);
        void SetNodeDisabled(bool v) => TextControlFFI.SetNodeDisabled(Handle(), _id, v);
        bool GetNodeDisabled() => TextControlFFI.GetNodeDisabled(Handle(), _id);
        // min/max/step：复用 Slider 同名 FFI（get_control_min/max/step 已扩到 NumberField，见 c55389d）。
        // float out 经 local + &local（同 GetControlValue 局部取址模式）。rc!=0 升异常不吞。
        float GetControlMin()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.loomgui_stage_get_control_min(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_min failed (node {_id})");
            return v;
        }
        float GetControlMax()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.loomgui_stage_get_control_max(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_max failed (node {_id})");
            return v;
        }
        float GetControlStep()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.loomgui_stage_get_control_step(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_step failed (node {_id})");
            return v;
        }
    }

    public unsafe class Slider : Node
    {
        internal Slider(UIContext ctx, uint id) : base(ctx, id) { }

        // 投影层填实：value/min/max/step 直转 FFI（value clamp [min,max] + step 量化）。
        public float Value
        {
            get { ThrowIfDisposed(); return GetControlValue(); }
            set { ThrowIfDisposed(); SetControlValue(value); }
        }
        public float Min
        {
            get { ThrowIfDisposed(); return GetControlMin(); }
            set { ThrowIfDisposed(); SetControlMin(value); }
        }
        public float Max
        {
            get { ThrowIfDisposed(); return GetControlMax(); }
            set { ThrowIfDisposed(); SetControlMax(value); }
        }
        public float Step
        {
            get { ThrowIfDisposed(); return GetControlStep(); }
            set { ThrowIfDisposed(); SetControlStep(value); }
        }
        // disabled 是伪类源 + active/click 抑制（set_node_disabled）。getter 读 NodeFlags::DISABLED
        // （get_node_disabled，与 set 对称）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // ValueChanged：逐帧拖拽值变更（core EVT_VALUE_CHANGED，x=新值）。backing-dict 模式同
        // Button.Clicked——订阅 internal ControlValueChangedEvent，翻译为公共 ValueChangedEvent<float>。
        [NonSerialized] Dictionary<Action<ValueChangedEvent<float>>, EventRegistration> _valueChangedBacking;
        public event Action<ValueChangedEvent<float>> ValueChanged
        {
            add
            {
                if (value == null) return;
                if (_valueChangedBacking == null)
                    _valueChangedBacking = new Dictionary<Action<ValueChangedEvent<float>>, EventRegistration>();
                if (_valueChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlValueChangedEvent>(e => value(new ValueChangedEvent<float> { _newValue = e.Value }));
                _valueChangedBacking[value] = reg;
            }
            remove
            {
                if (_valueChangedBacking != null && _valueChangedBacking.TryGetValue(value, out var reg))
                {
                    _valueChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }

        // ChangeCommitted：拖拽松手提交终值（core EVT_CHANGE_COMMITTED，x=终值）。Action<float> 直给终值。
        [NonSerialized] Dictionary<Action<float>, EventRegistration> _changeCommittedBacking;
        public event Action<float> ChangeCommitted
        {
            add
            {
                if (value == null) return;
                if (_changeCommittedBacking == null)
                    _changeCommittedBacking = new Dictionary<Action<float>, EventRegistration>();
                if (_changeCommittedBacking.ContainsKey(value)) return;
                var reg = On<ControlChangeCommittedEvent>(e => value(e.Value));
                _changeCommittedBacking[value] = reg;
            }
            remove
            {
                if (_changeCommittedBacking != null && _changeCommittedBacking.TryGetValue(value, out var reg))
                {
                    _changeCommittedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // ── FFI 转调（float out 经 local + &local，同 GetWorldMatrix 模式）──────────
        float GetControlValue()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.loomgui_stage_get_control_value(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_value failed (node {_id})");
            return v;
        }
        void SetControlValue(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_value(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_value failed (node {_id})");
        }
        float GetControlMin()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.loomgui_stage_get_control_min(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_min failed (node {_id})");
            return v;
        }
        void SetControlMin(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_min(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_min failed (node {_id})");
        }
        float GetControlMax()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.loomgui_stage_get_control_max(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_max failed (node {_id})");
            return v;
        }
        void SetControlMax(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_max(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_max failed (node {_id})");
        }
        float GetControlStep()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.loomgui_stage_get_control_step(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_step failed (node {_id})");
            return v;
        }
        void SetControlStep(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_step(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_step failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    public unsafe class Toggle : Node
    {
        internal Toggle(UIContext ctx, uint id) : base(ctx, id) { }

        // IsChecked 直转 FFI set/get_control_checked（bool* out 经 local + &local）。
        public bool IsChecked
        {
            get { ThrowIfDisposed(); return GetControlChecked(); }
            set { ThrowIfDisposed(); SetControlChecked(value); }
        }
        // disabled setter 直 FFI（set_node_disabled）；getter 读 NodeFlags::DISABLED（get_node_disabled）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // CheckedChanged：翻转事件（core EVT_CHECKED_CHANGED，pad[0]=0/1）。订阅 internal
        // ControlCheckedChangedEvent，翻译为公共 ValueChangedEvent<bool>。backing-dict 同 Button.Clicked。
        [NonSerialized] Dictionary<Action<ValueChangedEvent<bool>>, EventRegistration> _checkedChangedBacking;
        public event Action<ValueChangedEvent<bool>> CheckedChanged
        {
            add
            {
                if (value == null) return;
                if (_checkedChangedBacking == null)
                    _checkedChangedBacking = new Dictionary<Action<ValueChangedEvent<bool>>, EventRegistration>();
                if (_checkedChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlCheckedChangedEvent>(e => value(new ValueChangedEvent<bool> { _newValue = e.Checked }));
                _checkedChangedBacking[value] = reg;
            }
            remove
            {
                if (_checkedChangedBacking != null && _checkedChangedBacking.TryGetValue(value, out var reg))
                {
                    _checkedChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // ── FFI 转调 ────────────────────────────────────────────────────────
        bool GetControlChecked()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            bool v = false; int rc = Native.loomgui_stage_get_control_checked(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_checked failed (node {_id})");
            return v;
        }
        void SetControlChecked(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_checked(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_checked failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    public unsafe class RadioButton : Node
    {
        internal RadioButton(UIContext ctx, uint id) : base(ctx, id) { }

        // IsChecked 直转 FFI set/get_control_checked（与 Toggle 同语义；同组互斥框架自动做）。
        public bool IsChecked
        {
            get { ThrowIfDisposed(); return GetControlChecked(); }
            set { ThrowIfDisposed(); SetControlChecked(value); }
        }
        // Name = radio 分组名（HTML name 属性，结构性，决定互斥语义）。core 无 node-attribute getter FFI
        // ——暂留 throw，待打包期属性镜像或 side query 暴露后填。
        public string Name { get { throw NE(); } }
        // disabled setter 直 FFI；getter 读 NodeFlags::DISABLED（get_node_disabled）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // CheckedChanged：新选中事件（core EVT_CHECKED_CHANGED，pad[0]=1）。与 Toggle 同 payload 结构——
        // 语义差别在 core（同组互斥只新选中项触发），C# 投影同一套 demux。backing-dict 同 Button.Clicked。
        [NonSerialized] Dictionary<Action<ValueChangedEvent<bool>>, EventRegistration> _checkedChangedBacking;
        public event Action<ValueChangedEvent<bool>> CheckedChanged
        {
            add
            {
                if (value == null) return;
                if (_checkedChangedBacking == null)
                    _checkedChangedBacking = new Dictionary<Action<ValueChangedEvent<bool>>, EventRegistration>();
                if (_checkedChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlCheckedChangedEvent>(e => value(new ValueChangedEvent<bool> { _newValue = e.Checked }));
                _checkedChangedBacking[value] = reg;
            }
            remove
            {
                if (_checkedChangedBacking != null && _checkedChangedBacking.TryGetValue(value, out var reg))
                {
                    _checkedChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // ── FFI 转调 ────────────────────────────────────────────────────────
        bool GetControlChecked()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            bool v = false; int rc = Native.loomgui_stage_get_control_checked(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_checked failed (node {_id})");
            return v;
        }
        void SetControlChecked(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_checked(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_checked failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    public unsafe class TextArea : Node
    {
        internal TextArea(UIContext ctx, uint id) : base(ctx, id) { }

        // TextArea：多行文本框。FFI 通道与 TextField 共用（get/set_control_text 按 node 派发）。
        // 与单行框的差别在 core：sanitize_str 保留换行 / Enter 插换行而非提交（故无 Submitted 事件）。
        public string Value
        {
            get { ThrowIfDisposed(); return GetControlText(); }
            set { ThrowIfDisposed(); SetControlText(value); }
        }
        public string Placeholder
        {
            get { ThrowIfDisposed(); return GetControlPlaceholder(); }
            set { ThrowIfDisposed(); SetControlPlaceholder(value); }
        }
        public TextSelection Selection
        {
            get { ThrowIfDisposed(); return GetSelection(); }
            set { ThrowIfDisposed(); SetSelection(value.Start, value.End); }
        }
        public bool ReadOnly
        {
            set { ThrowIfDisposed(); SetControlReadonly(value); }
            get { ThrowIfDisposed(); return GetControlReadonly(); }
        }
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // ValueChanged：值变更（core EVT_VALUE_CHANGED=22，含 Enter 插换行）。订阅
        // ControlValueChangedEvent，在触发时回读当前 value 填 ValueChangedEvent<string>。
        // 无 Submitted 事件（多行框 Enter 插换行，不提交）。
        [NonSerialized] Dictionary<Action<ValueChangedEvent<string>>, EventRegistration> _valueChangedBacking;
        public event Action<ValueChangedEvent<string>> ValueChanged
        {
            add
            {
                if (value == null) return;
                if (_valueChangedBacking == null)
                    _valueChangedBacking = new Dictionary<Action<ValueChangedEvent<string>>, EventRegistration>();
                if (_valueChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlValueChangedEvent>(e =>
                    value(new ValueChangedEvent<string> { _newValue = GetControlText() }));
                _valueChangedBacking[value] = reg;
            }
            remove
            {
                if (_valueChangedBacking != null && _valueChangedBacking.TryGetValue(value, out var reg))
                {
                    _valueChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // FFI 通道收口在 TextControlFFI（四类文本框共享单一真相源）。本类仅薄转调：Handle() 取
        // stage 句柄，TextControlFFI.X(h, _id, ...) 直转。详参 TextControlFFI。
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        string GetControlText() => TextControlFFI.GetControlText(Handle(), _id);
        void SetControlText(string v) => TextControlFFI.SetControlText(Handle(), _id, v);
        string GetControlPlaceholder() => TextControlFFI.GetControlPlaceholder(Handle(), _id);
        void SetControlPlaceholder(string v) => TextControlFFI.SetControlPlaceholder(Handle(), _id, v);
        TextSelection GetSelection() => TextControlFFI.GetSelection(Handle(), _id);
        void SetSelection(int anchor, int cursor) => TextControlFFI.SetSelection(Handle(), _id, anchor, cursor);
        void SetControlReadonly(bool v) => TextControlFFI.SetControlReadonly(Handle(), _id, v);
        bool GetControlReadonly() => TextControlFFI.GetControlReadonly(Handle(), _id);
        void SetNodeDisabled(bool v) => TextControlFFI.SetNodeDisabled(Handle(), _id, v);
        bool GetNodeDisabled() => TextControlFFI.GetNodeDisabled(Handle(), _id);
    }

    public unsafe class Dropdown : Node
    {
        internal Dropdown(UIContext ctx, uint id) : base(ctx, id) { }

        // SelectedIndex：直转 FFI get/set_dropdown_selected_index（Task 6）。core ControlState::Dropdown
        // 的 selected_index（打包期 ControlInit::Dropdown.options 由 <option selected> 烘焙初值；运行时
        // 交互 / 本 setter 改写）。FFI 以 uint* 出参，公共签名用 int（index 不会超 int 正区）——边界 cast。
        public int SelectedIndex
        {
            get { ThrowIfDisposed(); return (int)GetDropdownSelectedIndex(); }
            set { ThrowIfDisposed(); SetDropdownSelectedIndex((uint)value); }
        }
        // SelectedValue：选中 option 的 value 属性。core 无 per-option value getter FFI（option value 在
        // 打包期进 Dropdown.options side table，运行时未暴露——见 OptionItem.Value 同源 gap）。只读——
        // HTML 语义上 value 由选中项派生，业务经 SelectedIndex 改选即可。待 option-value FFI 补后填。
        public string SelectedValue { get { throw NE(); } }
        // Disabled：伪类源 + active/click 抑制（set_node_disabled）。getter 读 NodeFlags::DISABLED（通用
        // node flag 通道，与 Slider/Toggle 一致）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // SelectionChanged：选中项变更事件（core EVT_SELECTION_CHANGED = 26，touch_id=新 index）。
        // backing-dict 模式同 Slider.ValueChanged——订阅 internal ControlSelectionChangedEvent，翻译为公共
        // SelectionChangedEvent（NewIndex 取 demux 解出的 index；OldIndex=-1 sentinel，core 不携旧值，
        // 同 ValueChangedEvent.OldValue=default 语义但用 -1 避免与合法 index 0 混淆）。
        [NonSerialized] Dictionary<Action<SelectionChangedEvent>, EventRegistration> _selectionChangedBacking;
        public event Action<SelectionChangedEvent> SelectionChanged
        {
            add
            {
                if (value == null) return;
                if (_selectionChangedBacking == null)
                    _selectionChangedBacking = new Dictionary<Action<SelectionChangedEvent>, EventRegistration>();
                if (_selectionChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlSelectionChangedEvent>(e => value(new SelectionChangedEvent { _oldIndex = -1, _newIndex = e.NewIndex }));
                _selectionChangedBacking[value] = reg;
            }
            remove
            {
                if (_selectionChangedBacking != null && _selectionChangedBacking.TryGetValue(value, out var reg))
                {
                    _selectionChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
        static NotImplementedException NE() => new NotImplementedException();

        // ── FFI 转调 ────────────────────────────────────────────────────────
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        uint GetDropdownSelectedIndex()
        {
            StageHandle* h = Handle();
            uint v = 0; int rc = Native.loomgui_stage_get_dropdown_selected_index(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_dropdown_selected_index failed (node {_id})");
            return v;
        }
        void SetDropdownSelectedIndex(uint v)
        {
            StageHandle* h = Handle();
            int rc = Native.loomgui_stage_set_dropdown_selected_index(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_dropdown_selected_index failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = Handle();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = Handle();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    // OptionItem = <option> 的 typed 投影（Dropdown 的子项）。结构上是容器型节点（围栏 content=text，
    // 可被渲染当文本块），故继承 Container（同 ListItem 模式）。NodeFactory 据 NodeKind.OptionItem
    // 派发到本类（替代之前的 Container 回落）。
    //
    // Value/Selected：core 尚无 option-value / option-selected 的 side query FFI（option 的 value 属性
    // 在打包期进 ControlInit::Dropdown.options，运行时无 per-option getter）——暂留 throw，待
    // Dropdown 完整投影（composite bundle）落地后填。Disabled 读 NodeFlags::DISABLED（通用 node flag）。
    public unsafe class OptionItem : Container
    {
        internal OptionItem(UIContext ctx, uint id) : base(ctx, id) { }

        // TODO(option-ffi): core 无 per-option value getter（option value 在打包期进 Dropdown.options
        // side table，运行时未暴露）。待 Dropdown 完整投影补 get_option_value FFI 后填。
        public string Value { get { throw NE(); } }
        // TODO(option-ffi): selected 由父 Dropdown.selected_index 派生，无 per-option selected getter。
        // 待 Dropdown 投影补齐后，OptionItem 可回查父 Dropdown 的 selected_index == self.Index 判定。
        public bool Selected { get { throw NE(); } }
        // Disabled：伪类源（NodeFlags::DISABLED）。setter 直 FFI；getter 读 node flag（与 Slider 等一致）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // ── FFI 转调（disabled 经通用 node flag 通道；Value/Selected 待 option FFI）──────────
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Slot = <slot> 的 typed 投影（模板插槽占位）。结构上是容器型节点，继承 Container。
    // 完整插槽投影机制（按 name 填充 / fallback content）是 composite bundle 工作，本类先落 class
    // shell 让 NodeFactory 派发到正确类型（替代之前的 Container 回落）。
    public class Slot : Container
    {
        internal Slot(UIContext ctx, uint id) : base(ctx, id) { }
    }

    // CustomElement = 带连字符的自定义标签（<my-widget>）的 typed 投影。围栏把未知 tag（含连字符）
    // 归为 CustomElement。结构上是容器型节点，继承 Container。投影机制（自定义元素注册 / 生命周期
    // 钩子）是 composite bundle 工作，本类先落 class shell 让 NodeFactory 派发到正确类型。
    public class CustomElement : Container
    {
        internal CustomElement(UIContext ctx, uint id) : base(ctx, id) { }
    }

    // TabList = <div role="tablist"> 的 typed 投影（WAI-ARIA tablist 容器，持若干 <button role=tab> 子）。
    // 继承 Container（同 ListView，因持有 tab 子节点——非 Dropdown 那样的叶子控件）。
    //
    // ControlState::TabList{selected_index}：selected_index 由打包期 aria-selected="true" 烘焙初值
    // （core T2-T3），运行时交互（click / 方向键）与本 setter 改写（core T5-T6 合成 aria-selected 到各 tab，
    // T7 触发 SelectionChanged）。SelectionChanged 复用 Dropdown 同源 ControlSelectionChangedEvent +
    // 公共 SelectionChangedEvent（core 侧同一 EVT_SELECTION_CHANGED=26，touch_id=新 index）——零新增
    // event struct / demux arm。
    public unsafe class TabList : Container
    {
        internal TabList(UIContext ctx, uint id) : base(ctx, id) { }

        // SelectedIndex：直转 FFI get/set_tablist_selected_index（Task 8）。uint* 出参，公共签名用 int
        // （index 不会超 int 正区）——边界 cast。rc!=0（节点非 TabList / 不 live）升 InvalidOperationException
        // 不吞（ThrowIfDisposed 后正常路径不该达）。
        public int SelectedIndex
        {
            get { ThrowIfDisposed(); return (int)GetTabListSelectedIndex(); }
            set { ThrowIfDisposed(); SetTabListSelectedIndex((uint)value); }
        }
        // Disabled：伪类源 + active/click 抑制（set_node_disabled）。getter 读 NodeFlags::DISABLED
        // （通用 node flag 通道，与 Dropdown / Slider 一致）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // SelectionChanged：选中 tab 变更事件。与 Dropdown.SelectionChanged 同源 backing-dict 模式——
        // 订阅 internal ControlSelectionChangedEvent（demux 解 EVT_SELECTION_CHANGED=26 后派），翻译为公共
        // SelectionChangedEvent（NewIndex 取 demux 解出的 index；OldIndex=-1 sentinel，core 不携旧值）。
        [NonSerialized] Dictionary<Action<SelectionChangedEvent>, EventRegistration> _selectionChangedBacking;
        public event Action<SelectionChangedEvent> SelectionChanged
        {
            add
            {
                if (value == null) return;
                if (_selectionChangedBacking == null)
                    _selectionChangedBacking = new Dictionary<Action<SelectionChangedEvent>, EventRegistration>();
                if (_selectionChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlSelectionChangedEvent>(e => value(new SelectionChangedEvent { _oldIndex = -1, _newIndex = e.NewIndex }));
                _selectionChangedBacking[value] = reg;
            }
            remove
            {
                if (_selectionChangedBacking != null && _selectionChangedBacking.TryGetValue(value, out var reg))
                {
                    _selectionChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }

        // ── FFI 转调 ────────────────────────────────────────────────────────
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        uint GetTabListSelectedIndex()
        {
            StageHandle* h = Handle();
            uint v = 0; int rc = Native.loomgui_stage_get_tablist_selected_index(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_tablist_selected_index failed (node {_id})");
            return v;
        }
        void SetTabListSelectedIndex(uint v)
        {
            StageHandle* h = Handle();
            int rc = Native.loomgui_stage_set_tablist_selected_index(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_tablist_selected_index failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = Handle();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = Handle();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    // Tab = <button role="tab"> 的 typed 投影（TabList 的子项）。结构上是容器型节点（围栏 content=text，
    // 可持 label / 图标子），继承 Container（同 OptionItem 模式）。
    //
    // Selected：本类不提供（throw，by design）。Tab 选中态是**派生量**——从父 TabList.SelectedIndex +
    // core synth_aria_value 合成 aria-selected 派生，不存在也不需要 per-tab 的 selected 存储 / getter FFI。
    // 业务经父 TabList.SelectedIndex 比对 self.Index 判定，或订阅 TabList.SelectionChanged（payload=新 index）。
    // Disabled 读 NodeFlags::DISABLED（通用 node flag，与 OptionItem 一致）。
    public unsafe class Tab : Container
    {
        internal Tab(UIContext ctx, uint id) : base(ctx, id) { }

        // Selected：by design 不提供（throw）。Tab 选中态是派生量（父 TabList.SelectedIndex +
        // core aria-selected synth），无 per-tab getter FFI 也不会加。经父 TabList.SelectedIndex
        // 比对 self.Index 判定，或 demux TabList.SelectionChanged。
        public bool Selected { get { throw NE(); } }
        // Disabled：伪类源（NodeFlags::DISABLED）。setter 直 FFI；getter 读 node flag（与 OptionItem 等一致）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        // ── FFI 转调（disabled 经通用 node flag 通道；Selected 派生无 FFI）──────────
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = Handle();
            Native.loomgui_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = Handle();
            byte b = 0;
            Native.loomgui_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public unsafe class ProgressBar : Node
    {
        internal ProgressBar(UIContext ctx, uint id) : base(ctx, id) { }

        // 投影层填实：直转 FFI set/get_control_value·set/get_control_max（value clamp [0,max]）。
        // rc<0（非值控件 / 节点缺失）经 ThrowIfDisposed 后不该达——升 InvalidOperationException 不吞。
        public float Value
        {
            get { ThrowIfDisposed(); return GetControlValue(); }
            set { ThrowIfDisposed(); SetControlValue(value); }
        }
        public float Max
        {
            get { ThrowIfDisposed(); return GetControlMax(); }
            set { ThrowIfDisposed(); SetControlMax(value); }
        }
        // indeterminate 是打包期 control_init 字段（core 无 runtime setter / 无 getter FFI）——
        // 设计期产物，运行时不可变。getter 暂留 throw：待 core 暴露 side query 或打包期镜像后再填。
        public bool IsIndeterminate { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();

        // float out 经 local + &local（同 GetWorldMatrix 局部取址模式，不用 fixed）。rc<0 升异常不吞。
        float GetControlValue()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f;
            int rc = Native.loomgui_stage_get_control_value(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_value failed (node {_id})");
            return v;
        }
        void SetControlValue(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_value(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_value failed (node {_id})");
        }
        float GetControlMax()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f;
            int rc = Native.loomgui_stage_get_control_max(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_max failed (node {_id})");
            return v;
        }
        void SetControlMax(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_stage_set_control_max(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_max failed (node {_id})");
        }
    }

    // ── ListView ────────────────────────────────────────────────────
    // 虚拟化是运行时实现决策，不进 HTML。首次设 ItemCount/ItemTemplate/BindItem 即数据驱动+清空设计期 li；
    // 静态/数据驱动强制互斥（越界抛 UIContractException）。
    public unsafe class ListView : Container
    {
        internal ListView(UIContext ctx, uint id) : base(ctx, id) { }

        // C# 侧缓存（core 无 item-count getter FFI）。setter 过桥后回填本字段，getter 直读。
        // set 0 时回填 0，保证 getter 与 core item_count 同步。
        int _itemCount;
        // 首次设 ItemCount 标记：首次过桥后调 drain_now 同帧克隆初始 slot + binds 入队，
        // 再 DrainPendingBinds 绑定（spec §7 同帧 bind，避免首帧模板原样）。后续 set 靠
        // tick-drain 自然推进，无需重复 drain（hot-path 避免 FFI 开销）。
        bool _firstItemCountSet;
        // BindItem 委托 + ItemTemplate/TemplateSelector（core 不存这二者，纯 C# 业务回调）。
        // internal：UIContext.DrainPendingBinds 同程序集直读调本委托。
        internal Action<ListItem, int> _bindItem;
        UITemplate _itemTemplate;
        Func<int, UITemplate> _templateSelector;

        /// <summary>
        /// 项数（数据驱动）。setter 调 loomgui_list_set_item_count：首次调用若该 ul 尚未进入
        /// 数据驱动模式，FFI 内部自动 enter_data_driven（备用模板 = 第一个设计期 li、分配全局
        /// list_ordinal）。注册本实例到 ctx._listViews（tick-drain 分发 BindItem 时反查祖先用）。
        /// 负值拋（与 DOM 语义一致：负 item 数无意义）。
        /// </summary>
        public int ItemCount
        {
            get { ThrowIfDisposed(); return _itemCount; }
            set
            {
                ThrowIfDisposed();
                if (value < 0)
                    throw new ArgumentOutOfRangeException(nameof(value), "ItemCount must be non-negative");
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int rc = Native.loomgui_list_set_item_count(h, _id, value);
                if (rc != 0)
                    throw new InvalidOperationException(
                        $"list_set_item_count failed (node {_id}): not a ListView / no template source");
                _itemCount = value;
                _ctx.RegisterListView(this);
                // 首次进入数据驱动：同帧推进虚拟化管线（plan+execute 克隆初始 slot + binds 入队），
                // 再 DrainPendingBinds 绑定——避免首帧模板原样（spec §7）。后续 set 靠 tick-drain。
                if (!_firstItemCountSet)
                {
                    _firstItemCountSet = true;
                    Native.loomgui_list_drain_now(h, _id);
                    _ctx.DrainPendingBinds();
                }
            }
        }

        /// <summary>
        /// 项模板。SceneSubtree 变体：调 loomgui_list_set_template 覆盖 enter_data_driven 备份的
        /// 备用 li，指向场景内克隆出的模板子树根。PackageComponent 变体需先 Instantiate 再传
        /// （本 setter 只接 SceneSubtree，包组件路径走业务侧 Instantiate + 转传）。
        /// </summary>
        public UITemplate ItemTemplate
        {
            get { ThrowIfDisposed(); return _itemTemplate; }
            set
            {
                ThrowIfDisposed();
                _itemTemplate = value;
                if (value != null && value.IsSceneSubtree)
                {
                    StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                    Native.loomgui_list_set_template(h, _id, value._srcNodeId);
                }
            }
        }

        /// <summary>
        /// 多模板选择器（按 item index 选模板）。纯 C# 业务回调，core 不存。
        /// setter 只缓存委托；实际选模板在 BindItem 回调里由业务据 index 调本选择器（若需要）。
        /// </summary>
        public Func<int, UITemplate> TemplateSelector
        {
            get { ThrowIfDisposed(); return _templateSelector; }
            set { ThrowIfDisposed(); _templateSelector = value; }
        }

        /// <summary>
        /// 绑定回调（每新克隆 slot 触发一次）。core tick 产 pending_binds，C# tick-drain
        /// 取队列后按 slot 的 ListView 祖先反查本实例调本委托。委托存 C# 侧（core 不感知业务回调）。
        /// </summary>
        public Action<ListItem, int> BindItem
        {
            get { ThrowIfDisposed(); return _bindItem; }
            set { ThrowIfDisposed(); _bindItem = value; _ctx.RegisterListView(this); }
        }

        /// <summary>
        /// 数据驱动模式下返 ItemCount（不直走 get_child_count——core ul 的真子是
        /// 2 spacer + N slot，与业务「逻辑项数」语义不符）。非数据驱动（未设过 ItemCount）
        /// 回落 get_child_count。用 new 而非 virtual override：Container.ChildCount 非虚，
        // 且 ListView 总经 NodeFactory 造为具体子类，调用方按 ListView 类型访问即命中本隐藏属性。
        /// </summary>
        public new int ChildCount
        {
            get
            {
                ThrowIfDisposed();
                // _itemCount 默认 0；设过 ItemCount 后 >0（或特意设 0）。用 _ctx._listViews
                // 注册态判数据驱动（设过 ItemCount/BindItem 即注册过）。
                if (_ctx.IsListViewRegistered(_id))
                    return _itemCount;
                return base.ChildCount;
            }
        }

        /// <summary>
        /// 滚动到指定 item（spec §7）。core 先设祖先 ScrollPane.scroll_pos 到目标偏移，再
        /// drain_now 同帧克隆新可见区 slot + binds 入队；随后本方法调 DrainPendingBinds 绑定
        /// ——同帧完成克隆 + bind，避免首帧模板原样。越界 index（负 / ≥ ItemCount）→
        /// UIContractException（调用方写错，非投影层内部错）。Smooth 走 ScrollPane 自维护
        /// cubic-out tween（TweenProp 无 Scroll 变体）。
        /// </summary>
        public void ScrollToItem(int i, ScrollBehavior b = ScrollBehavior.Smooth)
        {
            ThrowIfDisposed();
            if (i < 0 || i >= _itemCount)
                throw new UIContractException(
                    $"ScrollToItem index {i} out of range [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte behavior = (byte)(b == ScrollBehavior.Smooth ? 1 : 0);
            int rc = Native.loomgui_list_scroll_to(h, _id, i, behavior);
            if (rc != 0)
                throw new UIContractException(
                    $"ScrollToItem failed (node {_id}, index {i}): not a data-driven ListView");
            // core 已 drain_now（slot 克隆 + binds 入队）；此处取出 binds 绑定，同帧完成。
            _ctx.DrainPendingBinds();
        }

        /// <summary>刷新单个已物化 item（重新 BindItem）。未物化的静默跳过。</summary>
        public void RefreshItem(int i)
        {
            ThrowIfDisposed();
            if (i < 0 || i >= _itemCount)
                throw new UIContractException(
                    $"RefreshItem index {i} out of range [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_list_refresh(h, _id, i, 1);
            if (rc != 0)
                throw new UIContractException(
                    $"RefreshItem failed (node {_id}, index {i}): not a data-driven ListView");
            _ctx.DrainPendingBinds();
        }

        /// <summary>刷新全部已物化 item（重新 BindItem）。count=ItemCount 覆盖全部可见 slot。</summary>
        public void RefreshItems()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_list_refresh(h, _id, 0, _itemCount);
            if (rc != 0)
                throw new UIContractException(
                    $"RefreshItems failed (node {_id}): not a data-driven ListView");
            _ctx.DrainPendingBinds();
        }

        /// <summary>
        /// 插入通知（spec §10）：在 <paramref name="i"/> 处插入 <paramref name="c"/> 项。
        /// heights 插入 c 个未知项；已物化 slot 的 item_index 后移。i 越界 → UIContractException。
        /// </summary>
        public void NotifyInserted(int i, int c = 1)
        {
            ThrowIfDisposed();
            if (i < 0 || i > _itemCount)
                throw new UIContractException(
                    $"NotifyInserted at {i} out of range [0, {_itemCount}]");
            if (c < 0)
                throw new ArgumentOutOfRangeException(nameof(c), c, "count must be non-negative");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_list_notify(h, _id, (byte)0, i, c);
            if (rc != 0)
                throw new UIContractException(
                    $"NotifyInserted failed (node {_id}): not a data-driven ListView");
            _itemCount += c;
        }

        /// <summary>
        /// 删除通知（spec §10）：删 [i, i+c)。区间内已物化 slot 就地休眠（parked，留挂列表待复用）；区间后的 slot.item_index 前移。
        /// i/c 越界 → UIContractException。同步更新 _itemCount 缓存。
        /// </summary>
        public void NotifyRemoved(int i, int c = 1)
        {
            ThrowIfDisposed();
            if (i < 0 || c < 0 || i + c > _itemCount)
                throw new UIContractException(
                    $"NotifyRemoved range [{i}, {i + c}) out of bounds [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_list_notify(h, _id, (byte)1, i, c);
            if (rc != 0)
                throw new UIContractException(
                    $"NotifyRemoved failed (node {_id}): not a data-driven ListView");
            _itemCount -= c;
        }

        /// <summary>
        /// 移动通知（spec §10）：把 from 项搬到 to 位置。heights 同步搬；slot.item_index 重映射。
        /// from/to 越界 → UIContractException。
        /// </summary>
        public void NotifyMoved(int f, int t)
        {
            ThrowIfDisposed();
            if (f < 0 || f >= _itemCount || t < 0 || t >= _itemCount)
                throw new UIContractException(
                    $"NotifyMoved from {f} / to {t} out of range [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.loomgui_list_notify(h, _id, (byte)2, f, t);
            if (rc != 0)
                throw new UIContractException(
                    $"NotifyMoved failed (node {_id}): not a data-driven ListView");
        }

        public string ItemExitClass { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // ── 动画 ────────────────────────────────────────────────────────
    // Animation 句柄非长期对象，生命周期 = 那次播放；播放结束句柄失效、hook 自动释放。
    //
    // 生命周期不变量（spec §7.6 / public-api §9.2）：
    // - END 事件（demux 触发 onEnd 后）/ Stop()（scene 层终态）→ _disposed=true +
    //   UIContext 注销注册表条目；此后成员调用全部 no-op（不抛——§7.6「调用 no-op」）。
    // - 循环动画（infinite）句柄存活到 Stop()。
    // - class 触发的动画无句柄，只走 EventBus 广播（On<AnimationEndEvent> 等）。
    //
    // 回调路由（spec §7.4）：OnStart/OnEnd/OnHook 纯 C#（core 本就 emit 事件，demux 按
    // playerKey 查本实例触发）；OnKey 半 FFI（cb 留本类，pct 经 animation_on_key 注册到 core
    // ——core 才知道检测哪些百分比跨越，注册须在 Play 之后、key 有效时）。
    //
    // 事件载荷解码（T9 event.rs payload 编码）：demux 把 EventRecord 的 touch_id(低 32)/x(高 32)
    // 拼回 PlayerKey u64，按 key 查 UIContext._animations 命中本实例。
    public sealed unsafe class Animation
    {
        /// <summary>core PlayerKey（slotmap 稳定句柄，u64；0 = 无效哨兵）。</summary>
        internal readonly ulong _playerKey;
        /// <summary>播放目标节点（经其 _ctx 转调 FFI + 注销注册表）。</summary>
        internal readonly Node _node;
        /// <summary>动画名（Play 时传入；事件流同源，字符串表读回值一致）。</summary>
        internal readonly string _name;
        /// <summary>句柄失效标记（END / Stop 后置 true，成员调用 no-op）。</summary>
        internal bool _disposed;

        internal List<Action> _onStart;
        internal List<Action> _onEnd;
        internal List<(float pct, Action cb)> _onKeys;
        internal List<(string name, Action cb)> _onHooks;

        /// <summary>投影层内部：Node.Play 经 FFI play_animation 成功后构造 + 注册。</summary>
        internal Animation(Node node, ulong playerKey, string name)
        {
            _node = node;
            _playerKey = playerKey;
            _name = name;
        }

        /// <summary>动画名（Play(name) 参数）。</summary>
        public string Name => _name;

        /// <summary>
        /// 是否播放中（core PlayerPlayState::Playing）。句柄失效（END/Stop/player 回收）
        /// 返 false；顺带把已失效但未收到 END 的句柄（如节点销毁回收）注销出注册表。
        /// </summary>
        public bool IsPlaying
        {
            get
            {
                if (_disposed) return false;
                if (_node._disposed)
                {
                    // 节点已销毁：core 静默回收悬空 player 且不发 END（remove_node 不清
                    // scene.players，update_all 直接回收）——此处惰性失效，防注册表
                    // 强引用悬挂（UIContext→Animation→Node→用户回调全链）。
                    Invalidate();
                    return false;
                }
                StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
                byte state = Native.loomgui_stage_get_animation_state(h, _playerKey);
                if (state == 255)
                {
                    // player 已被 core 回收（Stop / 悬空节点 / Completed+fill none）——
                    // 惰性失效：句柄标记 disposed + 注销，防注册表悬挂条目。
                    Invalidate();
                    return false;
                }
                return state == 0;
            }
        }

        /// <summary>
        /// 时间轴位置（elapsed——含 delay 计时的唯一时间源头，spec §5.3）。setter = seek：
        /// 下一帧按新位置采样。句柄失效后 get 返 0 / set no-op。
        /// </summary>
        public float Time
        {
            get
            {
                if (_disposed || _node._disposed) return 0f;
                StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
                return Native.loomgui_stage_get_animation_time(h, _playerKey);
            }
            set
            {
                if (_disposed || _node._disposed) return;
                StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
                Native.loomgui_stage_set_animation_time(h, _playerKey, value);
            }
        }

        /// <summary>暂停（Playing → Paused，位置冻结；可 Resume）。句柄失效后 no-op。</summary>
        public void Pause()
        {
            if (_disposed || _node._disposed) return;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.loomgui_stage_pause_animation(h, _playerKey);
        }

        /// <summary>恢复（Paused → Playing；Completed/Stopped 是终态不可恢复）。失效后 no-op。</summary>
        public void Resume()
        {
            if (_disposed || _node._disposed) return;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.loomgui_stage_resume_animation(h, _playerKey);
        }

        /// <summary>
        /// 停止（scene 层终态，不可恢复，勿当暂停——T6 review Minor 1 钉死）。core 下帧
        /// 回收 player（不发 END 事件），故本方法同步失效句柄 + 注销注册表。
        /// </summary>
        public void Stop()
        {
            if (_disposed || _node._disposed) return;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.loomgui_stage_stop_animation(h, _playerKey);
            Invalidate();
        }

        /// <summary>链式注册播放启动回调（START 事件按 playerKey 命中时触发）。</summary>
        public Animation OnStart(Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            (_onStart ??= new List<Action>()).Add(cb);
            return this;
        }

        /// <summary>链式注册播放完成回调（完成后句柄失效，onEnd 先触发再失效）。</summary>
        public Animation OnEnd(Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            (_onEnd ??= new List<Action>()).Add(cb);
            return this;
        }

        /// <summary>
        /// 链式注册百分比跨越回调（spec §7.4 半 FFI：cb 留 C#，pct 注册进 core 检测阈值）。
        /// 须在 key 有效时调（Play 之后；链式 <c>Play(name).OnKey(.5, cb)</c> 是标准用法）。
        /// 同 pct 重复注册去重（core register_on_key 去重，cb 仍各存各发）。
        /// </summary>
        public Animation OnKey(float pct, Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.loomgui_stage_animation_on_key(h, _playerKey, pct);
            var list = _onKeys ??= new List<(float, Action)>();
            // cb 不去重（同 pct 多 cb 各自触发）；pct 去重由 core 保证。
            list.Add((pct, cb));
            return this;
        }

        /// <summary>
        /// 链式注册 @loom-hook 锚点回调（spec §7.4 纯 C#：core emit HOOK 带 hook_name，
        /// demux 按 name 匹配触发；无需 FFI 注册）。
        /// </summary>
        public Animation OnHook(string name, Action cb)
        {
            if (name == null) throw new ArgumentNullException(nameof(name));
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            (_onHooks ??= new List<(string, Action)>()).Add((name, cb));
            return this;
        }

        // ── 投影层内部：demux 句柄路由入口（spec §7.1）────────────────────
        // 回调是 Action（无事件参数），触发时只传载荷（pct / hook_name）。

        /// <summary>START 事件 → onStart 回调。</summary>
        internal void FireStart()
        {
            if (_onStart == null) return;
            var cbs = _onStart.ToArray();   // snapshot：回调内再注册不影响本次遍历
            for (int i = 0; i < cbs.Length; i++) cbs[i]();
        }

        /// <summary>END 事件 → onEnd 回调 + 句柄失效（§7.6：播放结束句柄失效）。</summary>
        internal void FireEnd()
        {
            try
            {
                if (_onEnd != null)
                {
                    var cbs = _onEnd.ToArray();
                    for (int i = 0; i < cbs.Length; i++) cbs[i]();
                }
            }
            finally
            {
                // 回调抛异常也须失效（异常向上传播，不吞——finally 只保证失效执行）。
                Invalidate();
            }
        }

        /// <summary>KEY 事件 → 匹配 pct 的 onKey 回调（core 按阈值逐个发，pct 是同一 f32 值，精确相等）。</summary>
        internal void FireKey(float percent)
        {
            if (_onKeys == null) return;
            var keys = _onKeys.ToArray();
            for (int i = 0; i < keys.Length; i++)
            {
                if (keys[i].pct == percent) keys[i].cb();
            }
        }

        /// <summary>HOOK 事件 → 匹配 name 的 onHook 回调。</summary>
        internal void FireHook(string hookName)
        {
            if (_onHooks == null) return;
            var hooks = _onHooks.ToArray();
            for (int i = 0; i < hooks.Length; i++)
            {
                if (hooks[i].name == hookName) hooks[i].cb();
            }
        }

        /// <summary>
        /// 标记失效 + 从 UIContext 注册表注销（END / Stop / IsPlaying 检出回收）。
        /// 幂等。此后成员调用 no-op（§7.6「player 回收 → 句柄失效 → 调用 no-op」）。
        ///
        /// 节点已 dispose 时调用也安全：只碰 <c>_node._ctx</c>（readonly，Node ctor 赋，
        /// Dispose 不清）做纯 C# 字典注销，无 FFI 调用——死节点不阻塞清理。
        /// </summary>
        internal void Invalidate()
        {
            if (_disposed) return;
            _disposed = true;
            _node._ctx.UnregisterAnimation(_playerKey);
        }
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

        // SceneSubtree 变体标识：非 RootSentinel 时本模板表示「克隆场景内某个子树」
        // （非包组件）。Task 2 加，供虚拟列表 slot 克隆路径用——
        // ListView ItemTemplate 可指向场景内已建子树，Instantiate 走 clone_subtree FFI
        // 而非包组件 instantiate FFI。两种变体共用同一个公共 API 表面（Name/Instantiate）。
        internal readonly uint _srcNodeId = Node.RootSentinel;
        internal bool IsSceneSubtree => _srcNodeId != Node.RootSentinel;

        internal UITemplate(UIContext ctx, string pkg, string path)
        {
            _ctx = ctx; _pkg = pkg; _path = path;
        }

        // SceneSubtree 变体构造：克隆场景内 srcNodeId 子树。path/pkg 留空（不供人读，
        // Name 返空串——调用方按 IsSceneSubtree 区分两种变体）。
        internal UITemplate(UIContext ctx, uint srcNodeId)
        {
            _ctx = ctx; _pkg = string.Empty; _path = string.Empty;
            _srcNodeId = srcNodeId;
        }

        public string Name => _path;
        public Container Instantiate()
        {
            if (_ctx._stage == IntPtr.Zero)
                throw new ObjectDisposedException(nameof(UIContext));
            // SceneSubtree 变体：clone_subtree FFI（游离根，不挂树）。
            // PackageComponent 变体：原包组件 instantiate FFI。
            return IsSceneSubtree
                ? DoInstantiateSubtree(_ctx, _srcNodeId)
                : DoInstantiate(_ctx, _pkg, _path);
        }

        /// <summary>
        /// SceneSubtree 变体实例化：调 clone_subtree FFI → 根 NodeId → registry.GetOrCreate。
        /// 返回游离 Container（调用方负责 append_child 挂到 slot）。
        /// </summary>
        internal static Container DoInstantiateSubtree(UIContext ctx, uint srcNodeId)
        {
            StageHandle* h = (StageHandle*)ctx._stage.ToPointer();
            uint rootId = Native.loomgui_stage_clone_subtree(h, srcNodeId);
            if (rootId == Node.RootSentinel)
                throw new UIPackageException(
                    "clone_subtree failed: invalid source node / no scene created");
            return (Container)ctx._registry.GetOrCreate(rootId);
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

        // ListView NodeId → C# 实例表。ListView 设 ItemCount/BindItem 时 RegisterListView 进本表；
        // tick-drain 取 pending_binds 后按 slot 的 NodeId 向上走 node_parent，命中本表即找到
        // 所属 ListView 实例、调其 BindItem。公共 API 不见本字段。
        internal readonly Dictionary<uint, ListView> _listViews = new Dictionary<uint, ListView>();

        // M2（T11）：PlayerKey → Animation 实例注册表（demux 句柄路由查用，spec §7.1/§7.6）。
        // 强引用：句柄生命周期 = 那次播放（END/Stop 时 Animation.Invalidate 注销）。
        // 循环动画存活到 Stop（§7.6）——用户持有句柄期间注册表保留引用，结束自动释放。
        // player 被 core 静默回收（节点销毁）的悬挂条目由 IsPlaying 惰性失效清理。
        internal readonly Dictionary<ulong, Animation> _animations = new Dictionary<ulong, Animation>();

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
        /// 帧末 flush seam：一次性把所有标脏的 StyleMirror / NodeTransform 回写到 core。
        /// LoomHost.Step 在 tick 前调（main-design §16 flush→solve 序）；headless 测试在 raw tick 前调。
        /// 攒批契约（Task 9）：setter 只标脏不立即过桥，本方法集中过桥，避免每 setter 一次 FFI。
        /// </summary>
        internal void FlushPendingWrites()
        {
            _registry.FlushDirtyStyles();
            _registry.FlushDirtyTransforms();
        }

        // ── ListView 虚拟化 tick-drain（Task 5）───────────────────────
        // ListView.ItemCount/BindItem setter 调 RegisterListView 进本表；DrainPendingBinds
        // 在 tick 前（raw tick 前或集成层 Step 开头）调一次：拉 core pending_binds 队列、
        // 按 slot NodeId 反查所属 ListView、构 ListItem 调 BindItem。core 不存业务回调——
        // 本路径是 C# 业务状态与 core 虚拟化内核的唯一结合点。

        /// <summary>注册 ListView 实例（ItemCount/BindItem setter 调）。幂等。</summary>
        internal void RegisterListView(ListView lv) => _listViews[lv._id] = lv;
        /// <summary>该 NodeId 是否已注册为 ListView（数据驱动模式已激活）。</summary>
        internal bool IsListViewRegistered(uint id) => _listViews.ContainsKey(id);

        /// <summary>注册 Animation 句柄（Node.Play 成功后调；demux 按 playerKey 路由）。</summary>
        internal void RegisterAnimation(Animation a) => _animations[a._playerKey] = a;
        /// <summary>按 playerKey 查 Animation 实例（demux 句柄路由；未命中 = class 触发/已失效 → null）。</summary>
        internal Animation ResolveAnimation(ulong playerKey) =>
            _animations.TryGetValue(playerKey, out var a) ? a : null;
        /// <summary>注销 Animation（END / Stop / 惰性失效时调）。幂等。</summary>
        internal void UnregisterAnimation(ulong playerKey) => _animations.Remove(playerKey);

        /// <summary>
        /// 排空 core pending_binds 并分发到对应 ListView 的 BindItem。集成层 Step 开头 /
        /// headless 测试 raw tick 前调一次。同帧克隆的 slot 在本 tick 完成绑定，避免首帧
        /// 显示模板原样。调法：take_pending_binds(out nodes[], out indices[], cap, out len)，
        /// 逐条按 node_id 向上走 node_parent 找命中 _listViews 的祖先，调 BindItem。
        /// cap 足够大（core 虚拟化保证常量 slot 数，不会溢出）。
        /// </summary>
        internal void DrainPendingBinds()
        {
            if (_listViews.Count == 0) return;
            if (_stage == IntPtr.Zero) return;
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            // 缓冲区：虚拟化保证可见 slot 常量级（INITIAL_SLOTS + 2*BUFFER 起），取 1024 冗余上限。
            const int Cap = 1024;
            uint* nodes = stackalloc uint[Cap];
            int* indices = stackalloc int[Cap];
            uint len = 0;
            int rc = Native.loomgui_list_take_pending_binds(h, nodes, indices, Cap, &len);
            if (rc != 0) return;
            for (int i = 0; i < len; i++)
            {
                uint slotNode = nodes[i];
                int itemIndex = indices[i];
                ListView lv = FindListViewAncestor(h, slotNode);
                if (lv == null || lv._bindItem == null) continue;
                // 构 ListItem 包装（registry 走身份缓存——同 slot 跨帧返同一实例）。
                ListItem item = (ListItem)_registry.GetOrCreate(slotNode);
                item._index = itemIndex;
                try { lv._bindItem(item, itemIndex); }
                catch (Exception ex)
                {
                    // 业务回调抛不阻断其他 slot 绑定（上层应自己捕获）；但静默吞错会让坏的
                    // BindItem 不可见，这里至少留一条诊断痕迹到 Debug 输出 / Unity player log。
                    System.Diagnostics.Debug.WriteLine(
                        $"[LoomGUI] ListView BindItem threw for item {itemIndex} (slot node {slotNode}): {ex}");
                }
            }
        }

        /// <summary>
        /// 从 slotNode 向上走 node_parent，找到首个命中 _listViews 的祖先 ListView。
        /// 未找到（slot 已脱离树 / ListView 未注册）返 null。防环：限 10 万层（远超任何合法树深）。
        /// </summary>
        ListView FindListViewAncestor(StageHandle* h, uint slotNode)
        {
            uint cur = slotNode;
            for (int i = 0; i < 100_000; i++)
            {
                if (cur == Node.RootSentinel) return null;
                if (_listViews.TryGetValue(cur, out var lv)) return lv;
                cur = Native.loomgui_node_parent(h, cur);
            }
            return null;
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
