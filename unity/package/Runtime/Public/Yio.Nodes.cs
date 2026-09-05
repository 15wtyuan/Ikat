// Yio Frozen Public API: Node hierarchy & controls

using System;
using System.Collections.Generic;
using System.Text;
using Yio.Bindings;

#pragma warning disable CS0169, CS0067, CS0649

namespace Yio
{
    // 三分模型：Style（可写/布局层，下帧 solve）/ Transform（可写/渲染层，不触发 solve）/
    //           Geometry（只读/布局产物，读最近一次 solve 结果，滞后一帧）。
    // Style/Transform 是 class + 内部 owner 引用（投影层：写回经 owner 标脏到 NodeId）；
    // Geometry 是 readonly struct 快照（从每帧 blob 填充）。
    public abstract unsafe class Node
    {
        // _id = Rust NodeId 的 u32 投影（slotmap key）；所有 FFI 调用经此转回 Rust 节点。
        // _ctx = 持有 stage handle + NodeRegistry 的 UIContext；本 Node 入 _ctx._registry 缓存。
        // _disposed = Dispose 后置 true；后续公共读操作抛 ObjectDisposedException。
        internal readonly ulong _id;
        internal readonly UIContext _ctx;
        internal bool _disposed;

        // 投影层内部 ctor：经 NodeFactory 调（同 assembly 子类 base 链调）。公共 API 无构造路径
        // （业务从 Create<T> / Instantiate 拿现成 Node）。
        internal Node(UIContext ctx, ulong id)
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

        /// <summary>
        /// HTML id 属性（authoring id：&lt;div id="btn-ok"&gt; → "btn-ok"；未声明 → 空串）。
        /// 直读 get_node_id_attr FFI（双调法，同 control text 通道）。数值 NodeId 不在公共面
        /// （作者契约是 authoring id）。
        /// </summary>
        public string Id
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadText(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_node_id_attr(hp, _id, buf, cap, len));
            }
        }

        // Root.Parent == null（FFI node_parent 返 sentinel ulong.MaxValue）。
        // 非根：registry.GetOrCreate(parent_id) → Container（围栏限定只容器型节点可为父）。
        public Container Parent
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                ulong parentId = Native.yio_node_parent(h, _id);
                if (parentId == RootSentinel) return null;
                return (Container)_ctx._registry.GetOrCreate(parentId);
            }
        }

        // 投影层：lazy 造 NodeStyle 挂本 Node。同一 Node 多次访问 Style 返同一实例——
        // node.Style.Width=X 与 node.Style.Height=Y 必须改同一 StyleMirror。
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

        // 投影层：lazy 造 NodeTransform 挂本 Node。同 Style 模式：同一 Node 多次访问 Transform
        // 返同一实例——node.Transform.Position=X 与 .Scale=Y 必须改同一 NodeTransform。
        // 未访问过 = null（不预造，避免给从未读写的节点带镜像开销）。
        internal NodeTransform _transform;

        /// <summary>
        /// Transform = 渲染层（不触发 solve）。lazy 造稳定单一实例：首次访问构造 + 挂本 Node；
        /// 后续访问返同一引用。setter 只存镜像、不 flush（set_transform FFI 推后，注释见
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

        /// <summary>
        /// Computed = 只读 computed style 查询（cascade 解析终值，非 Style 的 inline override
        /// 写层）。每次访问直读 FFI 不缓存；时效：rematch 后有效、本帧 tick 后反映最新 cascade。
        /// 背景/边框色缺席时（bg_present=0）对应属性返 null。
        /// </summary>
        public NodeComputedStyle Computed
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                ComputedNodeStyleRepr repr;
                int rc = Native.yio_stage_get_node_computed_style(h, _id, &repr);
                if (rc != 0)
                    throw new InvalidOperationException($"get_node_computed_style failed (node {_id})");
                return new NodeComputedStyle(ref repr);
            }
        }

        // Touchable（CSS `pointer-events` 的运行时面）：false = 本节点不参与命中（子节点
        // 照常——透传语义）。setter 直 FFI（写 interaction + base_style 双处，rematch 存活）；
        // getter 读 interaction.touchable（hit_test 同源）。
        public bool Touchable
        {
            get { ThrowIfDisposed(); return GetNodeTouchable(); }
            set { ThrowIfDisposed(); SetNodeTouchable(value); }
        }
        // 运行时改可获焦性（对齐 fgui focusable；tabindex>=0 的布尔投影）。set false =
        // Tab 链/点击聚焦排除，编程 Focus() 仍可用（DOM tabindex=-1 语义）。
        public bool Focusable
        {
            get { ThrowIfDisposed(); return GetNodeFocusable(); }
            set { ThrowIfDisposed(); SetNodeFocusable(value); }
        }
        // 运行时改拖拽使能（HTML `draggable` 属性的运行时面）。true = 本节点成为
        // drag_target 候选，pointer-down 后按位移阈值启动 DragStart/DragMove/DragEnd 事件链。
        public bool Draggable
        {
            get { ThrowIfDisposed(); return GetNodeDraggable(); }
            set { ThrowIfDisposed(); SetNodeDraggable(value); }
        }

        // 投影层：lazy 造 ClassList 挂本 Node。同 Style/Transform 模式：同一 Node 多次访问
        // Classes 返同一实例——node.Classes.Add("a") 与 .Contains("a") 必须作用同一 ClassList
        // （稳定单一实例）。未访问过 = null（不预造，避免给从未读写 class 的节点带开销）。
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
            ulong parentId = Native.yio_node_parent(h, _id);
            if (parentId == RootSentinel) return;   // 根：无父可摘
            Native.yio_stage_remove_child(h, parentId, _id);
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

            // Rust 侧递归清子 + slotmap remove + anim/scroll/tween 联动。
            // 调用后 NodeId 失效（gen++）；后续该 id 的 FFI 调用是 no-op。
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_remove_node(h, _id);

            _ctx.RemoveUpdateHooks(_id);   // 契约：OnUpdate 订阅随 Dispose 自动清理
            _ctx._registry.Remove(_id);
            _disposed = true;
        }

        /// <summary>
        /// 按 id 在本节点子树内查找 typed T（经 find_node_by_id_in_subtree FFI，DFS 从 _id 的
        /// 直接子开始——self-exclusive，与 <see cref="Query{T}"/> / DOM querySelectorAll 一致：
        /// 仅查后代，自身 id_attr 不被命中。即使本节点声明了 id 等于查询值也返 miss）。
        /// 未命中（无 id / 不在子树 / 类型不符）抛 <see cref="UIContractException"/>。null/empty id 直接抛
        /// （DOM getElementById 习惯：空 id 是调用方写错）。
        ///
        /// 作用域契约：组件作用域内查找，不穿透嵌套组件边界。
        /// 作用域内不穿透组件边界已完整实现：core DFS 遇 LOOKUP_SCOPE 子节点（组件展开域 host /
        /// List slot 根）
        /// 检查其自身 id 后不再下钻——组件级 Get 不再穿透 list item / 嵌套组件。
        /// 要访问嵌套作用域内部：先 Get 作用域根（host/slot），再在其上 Get。
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
        /// 查找经 find_node_by_id_in_subtree FFI（_id 子树 DFS，self-exclusive：
        /// 从 _id 的直接子开始，自身 id_attr 不被命中），不再走全局首匹配 + 父链后过滤。
        /// </summary>
        public bool TryGet<T>(string id, out T node) where T : Node
        {
            node = default;
            ThrowIfDisposed();
            if (string.IsNullOrEmpty(id)) return false;

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] idb = Encoding.UTF8.GetBytes(id);
            ulong candidate;
            fixed (byte* p = idb)
                candidate = Native.yio_stage_find_node_by_id_in_subtree(h, _id, p, (nuint)idb.Length);

            // 无匹配（含 null stage / 非 UTF-8，后两者 ThrowIfDisposed + UTF-8 编码已拦）。
            if (candidate == RootSentinel) return false;
            // IsInSubtree 后过滤已冗余（FFI 直接在 _id 子树内 DFS），
            // 保留 Debug.Assert 作结构不变量体检。
            System.Diagnostics.Debug.Assert(IsInSubtree(h, candidate),
                $"find_node_by_id_in_subtree returned node {candidate} outside subtree of {_id}");

            // registry.GetOrCreate 兑现身份稳定（同 NodeId → 同实例）。若已 Dispose 后 slot 复用，
            // candidate 指向新节点——find_node_by_id_in_subtree 返 live NodeId，不会是已 Dispose 的 stale id。
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
        /// 只匹配 TextField（默认 type=text），不匹配 Slider/Toggle 等 type 派生——这是简化取舍，
        /// type-aware selector 推后续（YAGNI：尚无场景驱动）。
        /// </summary>
        public IReadOnlyList<Node> Query(string selector)
        {
            ThrowIfDisposed();
            var (tag, cls) = ParseSelector(selector);
            // 空 selector（null/empty/whitespace）→ 空结果（不是「匹配全部」）。
            // DOM querySelectorAll("") 抛 SyntaxError；Yio 容错返空（不抛——宽松查询路径）。
            // "*" 走下面的 path：TagToNodeKind("*")=null → 所有节点 tagOk=false → 空结果（不支持通用选择器）。
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
        /// 程序化播放 @keyframes 动画。返 AnimationHandle 句柄。
        ///
        /// 建 programmatic player（core <c>play_programmatic</c>，不受 class 声明管）：
        /// 默认 1s / 无 delay / 单次迭代 / normal / fill both / cubic-out，立即写首帧。
        /// 结束用句柄 <see cref="AnimationHandle.OnEnd"/> 或 <c>On&lt;AnimationEndEvent&gt;()</c>；
        /// class 触发的动画无句柄（声明式，只需知结束走 EventBus 订阅）。
        ///
        /// 未知动画名（keyframes 表无此 name）抛 <see cref="UIContractException"/>（调用方
        /// 写错——同 Get&lt;T&gt; 未命中语义）；null name 抛 ArgumentNullException。
        /// </summary>
        public AnimationHandle Play(string name)
        {
            ThrowIfDisposed();
            if (name == null) throw new ArgumentNullException(nameof(name));
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name);
            fixed (byte* np = nb)
            {
                ulong key = Native.yio_stage_play_animation(h, _id, np, (nuint)nb.Length);
                if (key == 0)
                    throw new UIContractException(
                        $"Play(\"{name}\"): no @keyframes with this name (keyframes table lookup failed)");
                var anim = new AnimationHandle(this, key, name);
                _ctx.RegisterAnimation(anim);
                return anim;
            }
        }

        /// <summary>
        /// 同 <see cref="Play(string)"/>，显式指定时长（秒）。无 `animation:` 声明绑定的
        /// keyframes 没有声明层时长可读——无参重载固定按 1s 播（无 delay / 单次 / normal /
        /// fill both / cubic-out）；本重载让程序化演出的节奏由调用方精确控制。
        /// durationSeconds ≤ 0 或 NaN 按 1s 处理。
        /// </summary>
        public AnimationHandle Play(string name, float durationSeconds)
        {
            ThrowIfDisposed();
            if (name == null) throw new ArgumentNullException(nameof(name));
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name);
            fixed (byte* np = nb)
            {
                ulong key = Native.yio_stage_play_animation_dur(h, _id, np, (nuint)nb.Length, durationSeconds);
                if (key == 0)
                    throw new UIContractException(
                        $"Play(\"{name}\"): no @keyframes with this name (keyframes table lookup failed)");
                var anim = new AnimationHandle(this, key, name);
                _ctx.RegisterAnimation(anim);
                return anim;
            }
        }

        /// <summary>
        /// 链式 tween builder 入口（#9 契约；CSS transition/keyframes 之外的程序化演出通道）：
        /// <code>
        /// node.Tween(TweenChannel.Opacity).From(0).To(1)
        ///     .Duration(0.3f).Delay(0.1f)
        ///     .Ease(EaseKind.CubicOut)
        ///     .Repeat(2, yoyo: true)
        ///     .OnComplete(n => Debug.Log("done"))
        ///     .Start();
        /// </code>
        /// From/To 各通道分量数：Opacity/Rotation 1、Translate/Scale 2、Bg/TextColor 4（RGBA）、
        /// Transform 5（[tx,ty,sx,sy,rotRad]——运行时侧恒 px/弧度，百分比形只在 CSS @keyframes）。
        /// </summary>
        public TweenBuilder Tween(TweenChannel channel)
        {
            ThrowIfDisposed();
            return new TweenBuilder(this, channel);
        }

        // 编程聚焦节点（照 fgui RequestFocus）。直转 FFI request_focus（记 pending_focus_request，
        // 下 tick 最前消费写 scene.focused_node + 产 FocusIn/FocusOut）。文本框聚焦后才能接收
        // set_key_input / set_text_input 的输入（core input 只插焦点控件）。
        public void Focus()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_request_focus(h, _id);
        }
        // Blur 清除当前焦点（stage::blur）：记 pending_focus_request = Some(None)，下 tick 消费清焦点
        // （与 request_focus 对称的 stage 级操作）。FFI yio_stage_blur 不带 node_id——它清的是
        // 「当前获焦节点」而非「本节点」，故无焦点时调为 no-op（业务侧通常对聚焦控件调 Blur）。
        public void Blur()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_blur(h);
        }

        /// <summary>
        /// 捕获指针（DOM element.setPointerCapture 对齐）：本节点加入该指针的 monitor 表，
        /// 后续 PointerMove 即使移出命中域也直派给本节点，直到指针 Up 自动释放（无显式
        /// release API——core 在 Up 清 monitor 表）。须在指针 Down 之后调（Down 前槽未分配，
        /// no-op）——典型用法在 On&lt;PointerDownEvent&gt; handler 里以 evt.TouchId 调。
        /// </summary>
        public void SetPointerCapture(int touchId)
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_add_touch_monitor(h, touchId, _id);
        }

        /// <summary>
        /// 取消该指针待决的 Click（如长按后松手不要 Click、拖拽开始取消点击）。
        /// touchId 从对应事件（PointerDown/LongPress/DragStart）的 TouchId 取；鼠标 = -1。
        /// </summary>
        public void CancelClick(int touchId)
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_cancel_click(h, touchId);
        }

        public IDisposable OnUpdate(Action<float> cb)
        {
            ThrowIfDisposed();
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            // 逻辑驱动的每帧更新钩子（数据插值、跟随、状态响应），非命令式动画系统——
            // 预定义视觉动画走 CSS/Play。回调由 UIContext.PumpLogic 帧头泵（本帧 solve 生效），
            // 订阅随本节点 Dispose 自动清理（RemoveFromParent 不清理）。dt 即 Step 收到的帧时长。
            return _ctx.RegisterUpdateHook(_id, cb);
        }
        /// <summary>
        /// 订阅 typed 路由事件（DOM addEventListener 等价）。
        ///
        /// <paramref name="useCapture"/>：true → capture 阶段触发（root→target 路径上）；false → bubble
        /// 阶段触发（target→root 路径上，默认）。target 节点上 capture/bubble listener 都触发
        /// （DOM target 阶段等价）。<paramref name="once"/>：true → 触发一次后自动退订（防"等一个结束事件"
        /// 泄漏，如等 <see cref="AnimationEndEvent"/> 后 Dispose）。
        ///
        /// 返 <see cref="EventRegistration"/>——Dispose 退订。订阅随 <see cref="Dispose"/> 自动清理。
        /// </summary>
        public EventRegistration On<T>(Action<T> handler, bool useCapture = false, bool once = false) where T : IRouteEvent
        {
            ThrowIfDisposed();
            if (handler == null) throw new ArgumentNullException(nameof(handler));
            return _ctx._eventBus.Subscribe<T>(_id, handler, useCapture, once);
        }


        // node_parent 哨兵：根 / 越界 / 无 scene 均返 ulong.MaxValue（#26 u64 INVALID）。
        internal const ulong RootSentinel = ulong.MaxValue;   // #26 u64 INVALID

        /// <summary>
        /// Dispose 后访问抛 ObjectDisposedException。所有公共读操作入口都先调本方法。
        /// 异常消息用具体子类名（GetType().Name），帮助定位是哪种节点被误用。
        /// </summary>
        internal void ThrowIfDisposed()
        {
            if (_disposed) throw new ObjectDisposedException(GetType().Name);
        }


        /// <summary>
        /// 走父链判断 candidateId 是否在 _id 子树内（含直接子 + 任意深度后代；不含 _id 自身）。
        /// 用 yio_node_parent 逐层向上，直到撞 _id（在子树）或 RootSentinel（走出根）。
        /// 单线程同步内树结构稳定；防御性 cycle check（parent == current）防 ABI 异常死循环。
        /// </summary>
        private bool IsInSubtree(StageHandle* h, ulong candidateId)
        {
            ulong current = candidateId;
            for (int i = 0; i < 10_000; i++)   // 上限防御：scene 树深度受围栏闭合有界，10k 兜底
            {
                ulong parent = Native.yio_node_parent(h, current);
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
        ///
        /// 查找边界：遇 LOOKUP_SCOPE 子节点（组件展开域 host / ListView slot 根）visit 后
        /// 不再下钻——Query 与 Get/TryGet 同口径，嵌套作用域内部节点不进结果。
        /// 作用域根自身照常入结果（同 Shadow DOM：host 在 light tree）。
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
                    if (!child.IsLookupScopeBoundary())
                        child.DfsPreOrder(visit);
                }
            }
        }

        /// <summary>
        /// 节点是否为查找作用域边界（core NodeFlags::LOOKUP_SCOPE：实例根 / 组件展开域
        /// host / ListView slot 根）。Query 剪枝内部用；FFI 读失败（-1）按非边界处理（防御）。
        /// </summary>
        internal bool IsLookupScopeBoundary()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            return Native.yio_node_is_lookup_scope(h, _id) == 1;
        }

        /// <summary>
        /// 解析 CSS-like selector（fence 子集）。支持 ".cls" / "tag" / "tag.cls" 三种形式；
        /// 其它形式（".a.b" / "a &gt; b"）按容错解析：取首个 '.' 切 tag|cls，多 class 取末段为 cls
        /// （粗糙简化——复合 selector 不在当前范围）。null/空/whitespace → (null,null) 即匹配空集。
        /// </summary>
        private static (string tag, string cls) ParseSelector(string selector)
        {
            if (string.IsNullOrWhiteSpace(selector)) return (null, null);
            string s = selector.Trim();
            int dot = s.IndexOf('.');
            if (dot < 0) return (s, null);                      // "tag"
            string tagPart = dot > 0 ? s.Substring(0, dot) : null;
            string clsPart = dot < s.Length - 1 ? s.Substring(dot + 1) : null;
            // cls 含 '.' 或 tag 时不再细切——本实现把 "tag.a.b" 当作 (tag, "a.b") 永远 miss。
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
            int rc = Native.yio_stage_get_node_kind(h, n._id, &kind);
            if (rc != 0) return false;   // 节点不 live / stage 失效——保守 false
            return (NodeKind)kind == expected.Value;
        }

        /// <summary>
        /// 围栏 tag 名 → C# NodeKind 映射（crates/fence/src/schema/tag.rs::resolve_semantic 子集）。
        /// input 无 type 默认 TextField；type=range/checkbox/... 派生 kind 在 parse 期已固化，selector
        /// 用 "input" 只匹配 TextField（不匹配派生——简化取舍，type-aware selector 推后续）。
        /// template 不在映射表——parse 期消费、不进 runtime 树，selector "template" 永远空集。
        /// a 不在映射表——Link 仅 rich-text-block 上下文合法（围栏保证），运行时 create_node 不产
        /// 该 kind，selector "a" 空集（pkg-loaded 的 a 节点经 NodeFactory 正常投影为 Link，
        /// 业务用 Get&lt;Link&gt;/Id 定位）。
        ///
        /// 已知 core 不一致（span）：本表对齐 parse/pkg 路径（resolve_semantic("span") → TextElement，
        /// 覆盖 pkg 加载的绝大多数 span）。但 core 的动态建树 API 走另一张表——
        /// crates/core/src/scene/dynamic.rs::kind_from_tag("span") → NodeKind::TextNode（byte=1）。因此
        /// 运行时通过 Container.TextContent setter / create_node("span") 产出的 span 携带 kind=TextNode，
        /// `Query("span")` 对该子树会落空（不命中 TextElement）。pkg-loaded 节点不受影响。core 表拓宽
        /// 到完整映射（或动态 API 改走 resolve_semantic）留作后续改进项，本表不改（取 pkg 主路径）。
        /// </summary>
        private static NodeKind? TagToNodeKind(string tag) => tag switch
        {
            "div" => NodeKind.Container,
            "span" => NodeKind.TextElement,
            "button" => NodeKind.Button,
            "img" => NodeKind.Image,
            "input" => NodeKind.TextField,       // 默认 type=text；派生 kind 不命中
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
        /// internal：Container.TextContent 清子路径（ClearDirectChildrenFFI）同样需要先 evict。
        /// </summary>
        internal void DisposeDescendantsInRegistry(ulong subtreeRootId)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();

            // Snapshot 直系子（Rust 侧查询；下面的递归会改 Rust 树结构，不能边遍历边改）。
            int count = Native.yio_stage_get_child_count(h, subtreeRootId);
            if (count <= 0) return;
            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
            {
                written = Native.yio_stage_get_children(h, subtreeRootId, bp, (nuint)buf.Length);
            }
            // written < 0 = 节点刚被并发移除（理论单线程不达）；防御性早退防读越界。
            if (written < 0) return;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                ulong childId = buf[i];
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

        void SetNodeTouchable(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_touchable(h, _id, v);
        }
        bool GetNodeTouchable()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            int rc = Native.yio_stage_get_node_touchable(h, _id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_node_touchable failed (node {_id})");
            return b != 0;
        }
        void SetNodeFocusable(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_focusable(h, _id, v);
        }
        bool GetNodeFocusable()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            int rc = Native.yio_stage_get_node_focusable(h, _id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_node_focusable failed (node {_id})");
            return b != 0;
        }
        void SetNodeDraggable(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_draggable(h, _id, v);
        }
        bool GetNodeDraggable()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            int rc = Native.yio_stage_get_node_draggable(h, _id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_node_draggable failed (node {_id})");
            return b != 0;
        }
    }

    // Style = inline override 层（最高优先级），不是 cascade 读取窗口。
    // getter 只反映 C# setter 写过的属性；未写过返回 Unset（要 computed 走 Geometry）。
    // setter 写 Unset = 撤销该属性 inline override，回落 CSS。
    //
    // 每个 typed 属性的 setter/getter 走 _mirror（StyleMirror）。CSS prop 名严格对照 core
    // inline_bit 表（crates/core/src/style/dynamic.rs）+ apply_decl（mapping.rs）——表外的 prop
    // 经 set_inline_override 会被 bit 检查前置静默丢弃（ghost-state 防护），故本类只接
    // inline_bit 表内 prop（25 个，z-index 在 u64 位图 bit 32）；SetVar / RemoveVar 暂缓
    // （core apply_decl 未实现，throw NE + 注释）。
    /// <summary>
    /// <summary>
    /// UIContext.MeasureText 的输出：布局前纯文本预估（无节点、不进树）。
    /// W/H 像素；LineCount 断行后的行数（不换行测量恒 1，空文本 0）。
    /// </summary>
    public readonly struct TextMetrics
    {
        public readonly float W, H;
        public readonly uint LineCount;

        internal TextMetrics(float w, float h, uint lines) { W = w; H = h; LineCount = lines; }
    }

    /// <summary>
    /// 只读 computed style 快照（Node.Computed 每次访问新建）。cascade 解析终值——含
    /// tag 默认 / class 规则 / inline override 全层叠后的结果；背景/边框色缺席（cascade
    /// 无该声明）时对应属性返 null。时效：rematch 后有效、本帧 tick 后反映最新 cascade
    /// （同 web getComputedStyle 的回流后语义）。
    /// </summary>
    public readonly unsafe struct NodeComputedStyle
    {
        readonly DisplayMode _display;
        readonly FlexDirection _flexDirection;
        readonly Overflow _overflowX, _overflowY;
        readonly YioColor _color;
        readonly YioColor? _background;
        readonly float _opacity;
        readonly YioColor? _border;
        readonly float _fontSize;
        readonly int _fontWeight;
        readonly TextAlign _textAlign;
        readonly float _lineHeight;
        readonly float _letterSpacing;

        internal NodeComputedStyle(ref ComputedNodeStyleRepr r)
        {
            // FFI 判别值（lib.rs from_computed 显式映射，不依赖 Rust enum repr）：
            // display Flex=0/Block=1/None=2；flex_dir Row=0/Column=1/RowReverse=2/ColumnReverse=3；
            // overflow Visible=0/Hidden=1/Scroll=2/Auto=3（C# 侧 Hidden 叫 Clip）；align L/C/R=0/1/2。
            _display = r.display_mode switch
            {
                0 => DisplayMode.Flex, 1 => DisplayMode.Block, _ => DisplayMode.None,
            };
            _flexDirection = r.flex_direction switch
            {
                0 => FlexDirection.Row, 1 => FlexDirection.Column, 2 => FlexDirection.RowReverse,
                _ => FlexDirection.ColumnReverse,
            };
            _overflowX = MapOverflow(r.overflow_x);
            _overflowY = MapOverflow(r.overflow_y);
            _color = new YioColor(r.color[0], r.color[1], r.color[2], r.color[3]);
            _background = r.bg_present != 0
                ? (YioColor?)new YioColor(r.background_color[0], r.background_color[1], r.background_color[2], r.background_color[3])
                : null;
            _opacity = r.opacity;
            _border = r.border_present != 0
                ? (YioColor?)new YioColor(r.border_color[0], r.border_color[1], r.border_color[2], r.border_color[3])
                : null;
            _fontSize = r.font_size;
            _fontWeight = r.font_weight;
            _textAlign = (TextAlign)r.text_align;
            _lineHeight = r.line_height;
            _letterSpacing = r.letter_spacing;
        }

        static Overflow MapOverflow(byte v) => v switch
        {
            0 => Overflow.Visible, 1 => Overflow.Clip, 2 => Overflow.Scroll, _ => Overflow.Auto,
        };

        public DisplayMode Display => _display;
        public FlexDirection FlexDirection => _flexDirection;
        public Overflow OverflowX => _overflowX;
        public Overflow OverflowY => _overflowY;
        /// <summary>文字色（CSS color 通道；总有值——继承链兜底黑色）。</summary>
        public YioColor Color => _color;
        /// <summary>背景色；cascade 无 background-color 声明时 null。</summary>
        public YioColor? Background => _background;
        public float Opacity => _opacity;
        /// <summary>边框色；无 border 声明时 null。</summary>
        public YioColor? Border => _border;
        public float FontSize => _fontSize;
        public int FontWeight => _fontWeight;
        public TextAlign TextAlign => _textAlign;
        public float LineHeight => _lineHeight;
        public float LetterSpacing => _letterSpacing;
    }

    public sealed class NodeStyle
    {
        // 投影层内部：owner Node + mirror。Node.Style lazy 造时传入 this；StyleMirror 持 owner
        // 转调 FFI（set/unset_inline_override）需 owner._ctx._stage + owner._id。
        internal readonly Node _owner;
        internal readonly StyleMirror _mirror;
        internal NodeStyle(Node owner) { _owner = owner; _mirror = new StyleMirror(owner); }

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

        // Thickness 无 Unset 哨兵（裸四值 struct）；getter 未写过返 default（全 0）+ 不代表
        // "显式 0"，仅表示"未写过"。如需判"是否写过"走 Geometry 或自带 IsSet 查询。
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

        public YioColor BackgroundColor
        {
            get => _mirror.Get<YioColor>("background-color") ?? YioColor.Unset;
            set => _mirror.Set("background-color", value);
        }
        /// <summary>文字颜色（CSS color 通道，内联最高优先级；继承语义同 CSS——子树未显式设色时继承）。与 BackgroundColor 对称。</summary>
        public YioColor TextColor
        {
            get => _mirror.Get<YioColor>("color") ?? YioColor.Unset;
            set => _mirror.Set("color", value);
        }
        /// <summary>旧名（文字颜色）——N10 防 UnityEngine.Color 撞名时误把类型名当属性名，语义不直观。用 <see cref="TextColor"/>。</summary>
        [System.Obsolete("Use TextColor (same channel, clearer name).")]
        public YioColor YioColor
        {
            get => TextColor;
            set => TextColor = value;
        }
        // Opacity 无 Unset 哨兵（裸 float）；getter 未写过返 default（0f）+ 不代表"显式 0"。
        // 业务语义：CSS opacity 默认 1f，但本 getter 只反映 setter 写过的值（严格语义）。
        public float Opacity
        {
            get => _mirror.Get<float>("opacity") ?? default;
            set => _mirror.Set("opacity", value);
        }

        // CSS `<integer>`（负数合法）。getter 只反映 setter 写过的值（mirror 稀疏语义，
        // 同 Opacity）；未写过返 0 = CSS 初始值。CSS 侧 class 规则的 z-index 经打包期
        // base_style 进核心，与本便签层独立。
        public int ZIndex
        {
            get => _mirror.Get<int>("z-index") ?? 0;
            set => _mirror.Set("z-index", value);
        }

        // SetVar/RemoveVar（--xxx，#11 已接线）：CSS 自定义属性最高优先级层（高于行内 style
        // 与样式表规则声明），供运行时主题切换。值格式化为 CSS 值字符串过 FFI（core 存
        // node_vars）；rematch 每帧全量 → 下一帧 var() 消费面生效。RemoveVar 撤销本层条目、
        // 回落 CSS 声明值。名字须 `--` 前缀（否则 UIContractException）。不进 _mirror
        //（mirror 是 inline_bit 表内 prop 的便签层，--* 不在其域）。
        public void SetVar(string n, Length v)
        {
            _owner.ThrowIfDisposed();
            CallSetVar(n, FormatLength(v));
        }
        public void SetVar(string n, YioColor v)
        {
            _owner.ThrowIfDisposed();
            CallSetVar(n, FormatColor(v));
        }
        public void SetVar(string n, float v)
        {
            _owner.ThrowIfDisposed();
            CallSetVar(n, v.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }
        public void SetVar(string n, string v)
        {
            _owner.ThrowIfDisposed();
            CallSetVar(n, v ?? string.Empty);
        }
        /// <summary>撤销 SetVar 条目（回落 CSS 声明值：行内 style / 样式表规则）。</summary>
        public void RemoveVar(string n)
        {
            _owner.ThrowIfDisposed();
            FfiRemoveVar(_owner, n);
        }

        void CallSetVar(string name, string value)
        {
            if (string.IsNullOrEmpty(name) || !name.StartsWith("--", StringComparison.Ordinal) || name.Length <= 2)
                throw new UIContractException($"SetVar name \"{name}\" is not a custom property — names start with `--`");
            int rc = FfiSetVar(_owner, name, value);
            if (rc != 0)
                throw new InvalidOperationException("set_var FFI returned error (stale node).");
        }

        // 指针封送在独立 unsafe 静态方法（NodeStyle 类本体非 unsafe，公共签名保持不动）。
        static unsafe int FfiSetVar(Node owner, string name, string value)
        {
            StageHandle* h = (StageHandle*)owner._ctx._stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name);
            byte[] vb = Encoding.UTF8.GetBytes(value);
            fixed (byte* np = nb)
            fixed (byte* vp = vb)
                return Native.yio_stage_node_set_var(h, owner._id, np, (nuint)nb.Length, vp, (nuint)vb.Length);
        }

        static unsafe void FfiRemoveVar(Node owner, string name)
        {
            StageHandle* h = (StageHandle*)owner._ctx._stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name ?? string.Empty);
            int rc;
            fixed (byte* p = nb)
                rc = Native.yio_stage_node_remove_var(h, owner._id, p, (nuint)nb.Length);
            if (rc != 0)
                throw new InvalidOperationException("remove_var FFI returned error (stale node).");
        }

        // Length → CSS 值字符串（Px/Percent 直映；Auto/Unset 不是合法 var 值——
        // 写它们当契约错抛，静默吞会让主题变量悄悄变空串）。
        static string FormatLength(Length v)
        {
            return v.Unit switch
            {
                LengthUnit.Px => v.Value.ToString(System.Globalization.CultureInfo.InvariantCulture) + "px",
                LengthUnit.Percent => v.Value.ToString(System.Globalization.CultureInfo.InvariantCulture) + "%",
                _ => throw new UIContractException("SetVar(Length): Auto/Unset are not var() values — use Px/Pct"),
            };
        }

        // YioColor → #RRGGBBAA（fence/core parse_color 认的 hex 形）。
        static string FormatColor(YioColor c)
        {
            if (c.IsUnset) throw new UIContractException("SetVar(YioColor): Unset is not a var() value");
            static byte F2B(float f) => (byte)(Math.Clamp(f, 0f, 1f) * 255f + 0.5f);
            return $"#{F2B(c.R):X2}{F2B(c.G):X2}{F2B(c.B):X2}{F2B(c.A):X2}";
        }
    }

    // Transform = 渲染层，不触发 solve。回写走独立数值 FFI（set_transform，纯 f32）。
    //
    // 攒批 flush：setter 存镜像 + 标脏 + 注册到 NodeRegistry dirty 集；帧末
    // （YioHost.Step flush seam / UIContext.FlushPendingWrites）调 FlushTransform 一次性送
    // set_transform FFI（9-arg：tx,ty,sx,sy,rot,ox,oy）。core compute_world_transforms 并入 local_transform。
    // 整值替换语义（非累加）：每次 flush 送全 4 字段，不需要增量。本类签名零改动——只加帧末 flush。
    public sealed unsafe class NodeTransform
    {
        // 投影层内部：owner Node。lazy 造时由 Node.Transform 传 this；getter/setter 经它走 FFI
        // （owner._ctx._stage + owner._id 转调 set_transform）。
        internal readonly Node _owner;

        // 镜像值：setter 写、getter 读。default 按业务语义初始化（Scale=One 不缩放，其它 Zero）。
        // 帧末 flush 前读到的是 C# 侧最近一次写入的快照（getter 不依赖 core 状态）。
        internal YioVector2 _position = YioVector2.Zero;
        internal YioVector2 _scale = YioVector2.One;
        internal float _rotation;
        internal YioVector2 _origin = YioVector2.Zero;
        // dirty 标志：Store 置 true；FlushTransform 末尾置 false。配合 NodeRegistry dirty 集。
        internal bool _dirty;

        internal NodeTransform(Node owner) { _owner = owner; }

        /// <summary>位移（local 坐标，px）。setter 存镜像 + 标脏（帧末 flush 到 core）。</summary>
        public YioVector2 Position { get => _position; set => Store(ref _position, value); }
        /// <summary>缩放（local 基）。default = One（不缩放）；setter 存镜像 + 标脏。</summary>
        public YioVector2 Scale { get => _scale; set => Store(ref _scale, value); }
        /// <summary>旋转（弧度，绕 Origin）。setter 存镜像 + 标脏。</summary>
        public float Rotation { get => _rotation; set => Store(ref _rotation, value); }
        /// <summary>旋转/缩放原点（local 坐标，px）。setter 存镜像 + 标脏。</summary>
        public YioVector2 Origin { get => _origin; set => Store(ref _origin, value); }

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
            Native.yio_stage_set_transform(
                h, _owner._id,
                _position.X, _position.Y,
                _scale.X, _scale.Y,
                _rotation,
                _origin.X, _origin.Y);
        }
    }

    // Geometry = 只读快照，直读 FFI layout/world 产物（滞后一帧，同 web reflow）。
    //
    // 直读 FFI：readonly struct 持 owner 身份（ulong _id + UIContext _ctx）；node.Geometry 每次
    // 返 fresh struct snapshot。读时序——LayoutRect/WorldRect 反映最近一次
    // solve/compute_world_transforms 结果，本帧写 Style/Transform 下帧才反映（滞后一帧）。
    //
    // blob 缓存推后（升级路径：给 FrameBlob 加 rect/world 列）。直读 FFI
    // 简单且正确——单次 layout_rect/world_matrix 读是 6 f32 + 1 dict 查找，热路径（每帧 N 节点读）
    // 暂未达需缓存的规模，YAGNI。
    public readonly unsafe struct NodeGeometry
    {
        // struct 不持 disposed 状态——disposed 检在 node.Geometry getter 入口（Node.ThrowIfDisposed）。
        // 调用方拿到 struct 后假设 owner 活；FFI 在 owner 失效时返 identity/0（h.is_null/无效节点兜底）。
        internal readonly UIContext _ctx;
        internal readonly ulong _id;
        internal NodeGeometry(UIContext ctx, ulong id) { _ctx = ctx; _id = id; }

        /// <summary>
        /// 节点 layout 产物（solve 输出，左上 + w/h）。直读 get_node_layout_rect FFI（x/y/w/h → YioRect）。
        /// 滞后一帧：本帧 Style 写入下帧才反映。
        /// </summary>
        public YioRect LayoutRect
        {
            get
            {
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                float x = 0, y = 0, w = 0, hh = 0;
                Native.yio_stage_get_node_layout_rect(h, _id, &x, &y, &w, &hh);
                return new YioRect(x, y, w, hh);
            }
        }

        /// <summary>
        /// 节点 world AABB。world_matrix 已含节点自身 layout 偏移（tx,ty = 节点盒世界原点），
        /// 故对 (0,0)-(w,h) 盒做变换——再喂 LayoutRect 的 x/y 会把偏移算两次（滚动页上
        /// 世界位 = 视口位 + 内容位，翻倍错位）。滞后一帧：本帧 layout/transform 写入下帧才反映。
        /// </summary>
        public YioRect WorldRect
        {
            get
            {
                YioRect lr = LayoutRect;
                return LocalToGlobal(new YioRect(0, 0, lr.Width, lr.Height));
            }
        }

        /// <summary>
        /// 本地点 → 世界点（经 world_matrix）。Affine2 列主序：x' = a·x + c·y + tx，y' = b·x + d·y + ty
        /// （core transform.rs apply_point 公式）。
        /// </summary>
        public YioVector2 LocalToGlobal(YioVector2 p)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            return new YioVector2(a * p.X + c * p.Y + tx, b * p.X + d * p.Y + ty);
        }

        /// <summary>
        /// 世界点 → 本地点（world_matrix 的逆变换）。退化情形（det≈0，如 scale(0)）Rust 侧 inverse
        /// 返 IDENTITY（core transform.rs inverse），此处逆变换即原 world_matrix 逆——与 hit_test 一致的兜底。
        /// </summary>
        public YioVector2 GlobalToLocal(YioVector2 p)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            InverseAffine(a, b, c, d, tx, ty,
                          out float ia, out float ib, out float ic, out float id, out float itx, out float ity);
            return new YioVector2(ia * p.X + ic * p.Y + itx, ib * p.X + id * p.Y + ity);
        }

        /// <summary>本地 rect → world AABB：四角 LocalToGlobal + 轴对齐外接盒。</summary>
        public YioRect LocalToGlobal(YioRect r)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            return TransformAABB(a, b, c, d, tx, ty, r);
        }

        /// <summary>world rect → local AABB：四角 GlobalToLocal + 轴对齐外接盒。</summary>
        public YioRect GlobalToLocal(YioRect r)
        {
            GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty);
            InverseAffine(a, b, c, d, tx, ty,
                          out float ia, out float ib, out float ic, out float id, out float itx, out float ity);
            return TransformAABB(ia, ib, ic, id, itx, ity, r);
        }

        // 与 transform.rs 的 apply_point / inverse 公式一一对应；保留为 private 静态以便 JIT 内联。
        // Rust FFI 的 null/无效节点兜底写 identity（[1,0,0,1,0,0]）——调用方 owner Dispose 后理论
        // 不达（node.Geometry getter 抛 ODE），但兜底保证 struct 不持活节点也能安全读。

        void GetWorldMatrix(out float a, out float b, out float c, out float d, out float tx, out float ty)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            // locals 而非直接 &out：C# 禁止对 out 参数取地址（GC 可能移动托管引用）。
            float la = 1f, lb = 0f, lc = 0f, ld = 1f, ltx = 0f, lty = 0f;   // identity default（null/失效兜底）
            Native.yio_stage_get_node_world_matrix(h, _id, &la, &lb, &lc, &ld, &ltx, &lty);
            a = la; b = lb; c = lc; d = ld; tx = ltx; ty = lty;
        }

        // Affine2 逆：与 core transform.rs inverse 同算法（det≈0 退化返 identity）。
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
        static YioRect TransformAABB(float a, float b, float c, float d, float tx, float ty, YioRect r)
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
            return new YioRect(minX, minY, maxX - minX, maxY - minY);
        }

        static void ApplyPoint(float a, float b, float c, float d, float tx, float ty,
                               float x, float y, out float ox, out float oy)
        {
            ox = a * x + c * y + tx;
            oy = b * x + d * y + ty;
        }
    }

    public unsafe class Container : Node
    {
        internal Container(UIContext ctx, ulong id) : base(ctx, id) { }

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
                int c = Native.yio_stage_get_child_count(h, _id);
                // -1 = 节点不 live（post-ThrowIfDisposed 理论不达，FFI 防御性兜底）。
                return c < 0 ? 0 : c;
            }
        }

        /// <summary>
        /// 直系子节点列表（typed）。每次访问 lazy 物化：调 get_children 拿 NodeId 数组 +
        /// 逐个 registry.GetOrCreate 包成 typed Node。不缓存 list 本身——树可变，缓存的 list
        /// 会 stale。但 list 内的 Node 引用稳定：GetOrCreate 走 registry 强引用缓存，同一 NodeId
        /// 永远返同一实例（订阅 / 镜像挂对象上不丢）。
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
        /// 读侧递归 Container 子树（含 TextBlock/TextElement/Button 等 Container 子类）累加 TextNode.Text；
        /// 非 TextNode 叶子（Image / 控件）贡献 0 字符。
        ///
        /// 写侧 DOM 语义：先清所有当前子（**真释放**——remove_node 递归回收，非 detach；见
        /// ClearDirectChildrenFFI 注释），再建一个 TextNode（create_node "span" + set_text）
        /// + append_child。多次写值=替换当前 TextNode 文本不重建——但本实现简化为每次写都重建
        /// （与 DOM textContent setter 一致：每次写都重建内容树）。
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

                // 0) 快路径：现有直系子恰为单个 TextNode → 就地 set_text（写穿 core，getter
                //    读 core 真值天然同步）。必须走快路径：本 setter 是每帧高频改写路径
                //    （OnUpdate 读数刷新），清子重建会每帧烧一次 slotmap generation——
                //    单槽复用 ~4096 次后 NodeId 的 12-bit gen 截断回卷，产生活着的
                //    「幽灵死节点」（core from_key 版本截断，get(id) 永久 miss）。
                //    语义偏差（有意的）：复用现有 TextNode 而非替换——若作者是手动 append
                //    的带样式 TextNode，其 class/style 会保留（DOM 会替换成裸文本节点）。
                //    本 setter 建的 TextNode 均为裸节点，快路径只在自己产物上命中时无偏差。
                if (ChildCount == 1 && GetChildAt(0) is TextNode existing)
                {
                    existing.Text = text;
                    return;
                }

                // 1) 清当前直系子（真释放 remove_node：递归清子 + slotmap 回收 + registry evict）。
                //    释放而非 detach 是必须的：detach 会无限累积死节点。子树销毁 = DOM
                //    textContent 替换语义（句柄随之失效）。
                ClearDirectChildrenFFI(h);

                // 2) 建 TextNode + 写文本 + append。三步 FFI 顺序——建后才有 NodeId，setText 后再挂，
                //    避免 append 后挂前核心状态不一致窗口（无父 TextNode 也合法，标 dirty_text 即可）。
                byte[] tag = Encoding.UTF8.GetBytes("span");   // 围栏 kind_from_tag: "span" → TextNode
                ulong textId;
                fixed (byte* tp = tag)
                    textId = Native.yio_stage_create_node(h, tp, (nuint)tag.Length, null, 0);
                if (textId == RootSentinel)
                    throw new InvalidOperationException("create_node(\"span\") failed (stage null / non-UTF-8)");

                byte[] tb = Encoding.UTF8.GetBytes(text);
                fixed (byte* tp = tb)
                {
                    int src = Native.yio_stage_set_text(h, textId, tp, (nuint)tb.Length);
                    if (src != 0)
                        throw new InvalidOperationException("set_text on fresh TextNode failed (non-TextNode kind)");
                }

                int arc = Native.yio_stage_append_child(h, _id, textId);
                if (arc != 0)
                    throw new InvalidOperationException("append_child(textNode) failed (child has existing parent)");

                // 3) registry.GetOrCreate 据 textId 派发到 TextNode（NodeFactory: kind=TextNode →
                //    TextNode ctor）。无需回填 C# 侧镜像——Text getter 直读 core 真值。
                _ = _ctx._registry.GetOrCreate(textId);
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
            int rc = Native.yio_stage_append_child(h, _id, c._id);
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
            ulong refId = (i == kids.Count) ? RootSentinel : kids[i]._id;
            int rc = Native.yio_stage_insert_before(h, _id, c._id, refId);
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
            // （dynamic.rs 的 retain no-op 但 parent=None 仍执行——bug 兜底在投影层拦）。
            if (GetChildIndex(c) < 0)
                throw new ArgumentException(
                    $"node (id={c._id}) is not a child of container (id={_id})", nameof(c));

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_remove_child(h, _id, c._id);
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
            int lower = Math.Min(ia, ib), upper = Math.Max(ia, ib);
            Node lowerChild = (ia < ib) ? a : b;   // 占 lower 位的原始节点
            Node upperChild = (ia < ib) ? b : a;   // 占 upper 位的原始节点

            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            // 顺序敏感：先移 upper（不影 lower 索引），再移 lower。
            Native.yio_stage_remove_child(h, _id, upperChild._id);
            Native.yio_stage_remove_child(h, _id, lowerChild._id);
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
        /// 重启本子树内全部声明式动画（class 触发的 `animation` keyframes）。
        /// player 原地重建：delay 重新计时、backwards/both 立即写首帧；
        /// `node.Play` 的程序化 player（句柄持有）不受影响。
        /// 与销毁重实例化的差别：节点身份、滚动位置、控件值、事件订阅全部保留。
        public void RestartAnimations()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_restart_animations(h, _id);
        }

        /// 当前滚动位置（本节点为滚动容器时；非滚动容器返 (0,0)）。
        /// 与 <see cref="ScrollTo"/> 成对：读经 get_scroll_pos FFI（ScrollPane 物理量，
        /// 含未 settle 的惯性位移）。重实例化/换页前读出、solve 就绪后 ScrollTo(Instant) 回填。
        public YioVector2 ScrollPos
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                float x = 0f, y = 0f;
                Native.yio_stage_get_scroll_pos(h, _id, &x, &y);
                return new YioVector2(x, y);
            }
        }

        public void ScrollTo(YioVector2 p, ScrollBehavior b = ScrollBehavior.Smooth)
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_scroll_pos(h, _id, p.X, p.Y, (byte)(b == ScrollBehavior.Smooth ? 1 : 0));
        }
        // ScrollChanged source 待补：ScrollPane 物理自维护 tween，无 borrow_scroll_events FFI。
        // defer——event 签名冻结（PublicApi 编译门已含此字段），add/remove 推后到 source 补齐。
        public event Action<ScrollChangedEvent> Scrolled;
        public UITemplate GetTemplate(string name)
        {
            ThrowIfDisposed();
            if (string.IsNullOrEmpty(name)) throw new ArgumentNullException(nameof(name));
            // 取设计期 <template id="name">（ListView 多模板故事的配套：取出后塞进
            // TemplateSelector lambda 按 index 选）。作用域内按 id 找 Template 节点（与
            // Get<T> 同一作用域边界 DFS），蓝图 = template 的单个元素子（围栏校验 template
            // 根恰一个 role=listitem），克隆目标取该子（SceneSubtree 变体 UITemplate）。
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] idb = Encoding.UTF8.GetBytes(name);
            ulong candidate;
            fixed (byte* p = idb)
                candidate = Native.yio_stage_find_node_by_id_in_subtree(h, _id, p, (nuint)idb.Length);
            const byte NodeKindTemplate = 18; // Rust NodeKind::Template（ListView item 蓝图，不进 typed 投影）
            const byte NodeKindTextNode = 1;  // 模板源 HTML 的缩进空白是 TextNode 子，蓝图须取首个元素子
            if (candidate == RootSentinel)
                throw new UIContractException(
                    $"node with id '{name}' not found in scope of ({GetType().Name} id={_id}) (missing template)");
            byte kind = 0;
            Native.yio_stage_get_node_kind(h, candidate, &kind);
            if (kind != NodeKindTemplate)
                throw new UIContractException(
                    $"node with id '{name}' in scope of ({GetType().Name} id={_id}) is not a <template> " +
                    "(GetTemplate takes <template id> only)");
            int count = Native.yio_stage_get_child_count(h, candidate);
            ulong[] children = new ulong[Math.Max(count, 0)];
            ulong child = RootSentinel;
            fixed (ulong* cp = children)
                Native.yio_stage_get_children(h, candidate, cp, (nuint)children.Length);
            foreach (ulong c in children)
            {
                byte k = 0;
                Native.yio_stage_get_node_kind(h, c, &k);
                if (k == NodeKindTextNode) continue;
                child = c;
                break;
            }
            if (child == RootSentinel)
                throw new UIContractException(
                    $"<template id='{name}'> has no element child (fence requires exactly one role=listitem)");
            return new UITemplate(_ctx, child);
        }


        /// <summary>
        /// 调 get_children 拿当前直系子 NodeId 数组 + 逐个 registry.GetOrCreate 包成 typed Node。
        /// FFI 调用模式复用 <see cref="Node.DisposeDescendantsInRegistry"/>：先 get_child_count
        /// 定 cap，再 get_children 写入 fixed 钉住的 buffer（return-code + out-param + cap 编码）。
        /// 单线程同步内 count 不会 stale；written 防御性 clamp 兜底 ABI 异常。
        /// </summary>
        private List<Node> MaterializeChildren()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();

            int count = Native.yio_stage_get_child_count(h, _id);
            var list = new List<Node>(count > 0 ? count : 0);
            if (count <= 0) return list;   // 0 子 / FFI err：返空 list（err post-ThrowIfDisposed 理论不达）

            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
            {
                written = Native.yio_stage_get_children(h, _id, bp, (nuint)buf.Length);
            }
            // written < 0 = 节点刚被并发移除（单线程理论不达）；防御性早退防读越界。
            if (written < 0) return list;
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                // registry 缓存命中返同一实例；未命中走 NodeFactory 造 typed 子类 + 入缓存。
                list.Add(_ctx._registry.GetOrCreate(buf[i]));
            }
            return list;
        }

        /// <summary>
        /// 真释放当前直系子（remove_node 递归清子 + slotmap 回收，非 detach）。snapshot NodeId
        /// 列表后逐个走 Node.Dispose 同款路径：evict 后代 wrapper + remove_node + 清 update
        /// hooks + evict 自身 wrapper（标 _disposed——调用方持有的子句柄随之失效，公共读抛
        /// ObjectDisposedException，与 Dispose 契约一致）。跳过 RemoveChild 的 GetChildIndex
        /// 校验（snapshot 保证是直系子，校验纯开销）。TextContent setter 用——清后立建新 TextNode。
        /// </summary>
        private void ClearDirectChildrenFFI(StageHandle* h)
        {
            int count = Native.yio_stage_get_child_count(h, _id);
            if (count <= 0) return;

            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
                written = Native.yio_stage_get_children(h, _id, bp, (nuint)buf.Length);
            if (written < 0) return;
            if (written > buf.Length) written = buf.Length;

            // 真释放（remove_node 递归清子 + slotmap 回收），非 detach。textContent 是高频
            // 改写路径（OnUpdate 每帧刷新读数）：detach 语义下被清的 TextNode 永远留在
            // scene.nodes 里，每帧漏节点——长会话把 slotmap index 推过 4096，撞破 render
            // 合成 text 子页 id 方案的硬上限（真实节点被误判为子页、不进渲染）。
            // 被清子树对作者语义 = DOM textContent 替换（子树销毁，句柄失效）。
            for (int i = 0; i < written; i++)
            {
                ulong cid = buf[i];
                DisposeDescendantsInRegistry(cid);
                // 直系子自身的 wrapper 也要标 _disposed（DisposeDescendantsInRegistry 只管
                // 后代）：不标则调用方手里的子句柄 _disposed=false 但 Rust 节点已死，
                // 公共读不抛 ObjectDisposedException 而是静默 no-op，违背 Dispose 契约。
                if (_ctx._registry.TryGet(cid, out var cachedChild))
                    cachedChild._disposed = true;
                // remove_node 递归清子 + 脱挂 + slotmap 回收（同 Node.Dispose 路径）。
                Native.yio_stage_remove_node(h, cid);
                _ctx.RemoveUpdateHooks(cid);
                _ctx._registry.Remove(cid);
            }
        }

        /// <summary>
        /// 递归子树累加 TextNode.Text 到 sb（文档序）。Container 子递归；TextNode 叶子累加 Text
        /// （core 真值——pkg 烙入文本 C# 镜像曾读空）；
        /// 其它叶子（Image / 控件）贡献 0 字符。TextContent getter 用。
        /// 递归终止：围栏闭合保证无循环引用（parent 指针单向）；深度受场景树规模有界。
        /// </summary>
        static void AppendDescendantText(Container root, StringBuilder sb)
        {
            // Children getter lazy materialize + registry cache：每次访问重新拿最新直系子列表。
            // 递归路径稳——同一 Node 多次入参不会（无环）。
            foreach (Node child in root.Children)
            {
                if (child is TextNode tn) sb.Append(tn.Text);
                else if (child is Container c) AppendDescendantText(c, sb);
                // 其它（Image / 控件 / 未知叶子）：跳过。
            }
        }

        static NotImplementedException NE() => new NotImplementedException();
    }

    // AbsolutePanel：自身 relative，AddChild 自动施加 absolute 到子节点。API 与 Container 一致。
    public sealed class AbsolutePanel : Container
    {
        internal AbsolutePanel(UIContext ctx, ulong id) : base(ctx, id) { }
    }

    // 注：无 Panel 类型。作用域是运行时标记（IsScopeRoot），非类型；Instantiate 返回模板根真实类型。

    //
    // TextNode.Text 读侧直读 core 真值（get_node_text FFI 双调法，同 Node.Id 的
    // get_node_id_attr 通道）。曾用 C# 镜像（_text）：真值在 core（text_contents
    // HashMap<NodeId, String>），Instantiate 路径把 pkg 内文本写入 core 但不通知 C#
    // → 镜像保持 ""，读镜像与 core 实际渲染不一致（已知 ghost state，tree 页读数
    // 是首个真实读回消费者，按当年预案接通 FFI 后撤销镜像）。
    public unsafe class TextNode : Node
    {
        internal TextNode(UIContext ctx, ulong id) : base(ctx, id) { }

        /// <summary>
        /// 文本内容（对应 DOM Text.data / CharacterData.data）。getter 直读 core 真值
        /// （get_node_text FFI 双调法）——pkg 烙入的 HTML 文本从不过 C# setter，纯 C# 镜像
        /// 读会把合法初值读成空串（showcase tree 页读数实锤），读路径必须走 core。
        /// setter 写穿 core（set_text FFI：UTF-8 编码 + ptr+len，标 dirty_text → 下帧重排
        /// 文本 runs）。null 当空串处理（与 DOM textContent=null 一致）。Dispose 后访问抛
        /// ObjectDisposedException。
        /// </summary>
        public string Text
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadText(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_node_text(hp, _id, buf, cap, len));
            }
            set
            {
                ThrowIfDisposed();
                string v = value ?? "";
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                byte[] b = Encoding.UTF8.GetBytes(v);
                fixed (byte* p = b)
                {
                    // rc!=0 仅发生于 null stage / 节点不 live / 非 TextNode——本类 ctor 经 NodeFactory
                    // 据 get_node_kind 派发（kind=TextNode=1 → TextNode ctor），kind 不可变；ThrowIfDisposed
                    // 拦 stale；UTF-8 编码不会产非 UTF-8。故 rc 理论必 0，与 ClassList add/remove 一致不抛。
                    _ = Native.yio_stage_set_text(h, _id, p, (nuint)b.Length);
                }
            }
        }
    }
    public unsafe class Image : Node
    {
        internal Image(UIContext ctx, ulong id) : base(ctx, id) { }

        /// <summary>
        /// 图片资源 key（atlas sprite_key，如 "res/icons/item-potion.png"）。getter 直读 core
        /// 真值（get_src FFI 双调法）——pkg 烙入的 HTML src 从不过 C# setter，纯 C# 镜像读会
        /// 把合法初值读成空串（同 TextNode.Text 的 ghost state 预案）。
        /// setter 写穿 core：set_src 标 dirty_mesh → 下帧 render 重读 image_srcs 出 mesh
        /// image_path → Unity MirrorPool 查 atlas manifest 重映射 UV——故运行时换图有效
        /// （前提：新 key 是已打包进 atlas 的 sprite）。null 当空串。Dispose 后访问抛
        /// ObjectDisposedException。
        /// </summary>
        public string Src
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadText(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_src(hp, _id, buf, cap, len));
            }
            set
            {
                ThrowIfDisposed();
                string v = value ?? "";
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                byte[] b = Encoding.UTF8.GetBytes(v);
                fixed (byte* p = b)
                {
                    _ = Native.yio_stage_set_src(h, _id, p, (nuint)b.Length);
                }
            }
        }
    }

    public class TextElement : Container
    {
        internal TextElement(UIContext ctx, ulong id) : base(ctx, id) { }
    }    // span

    /// <summary>
    /// <a>（富文本内链接，#74）。仅 rich-text-block 上下文合法（围栏打包期拦截非法用法），
    /// 子只许文本/嵌 span。href 是 opaque 标识符：框架不解析、不 OpenURL，原样回传给游戏
    /// 自行解释。点击走 <see cref="Clicked"/>（指针命中细化到 a 节点，含嵌套 span 内文字）；
    /// UA 默认蓝 #0000EE + 下划线（作者 CSS 可覆盖）；键盘聚焦/Enter 激活属键盘导航项
    /// （deferred）。Href 只读——打包期从 href 属性烙印，运行时不可改。
    /// </summary>
    public unsafe class Link : Container
    {
        internal Link(UIContext ctx, ulong id) : base(ctx, id) { }

        /// <summary>
        /// 链接目标（opaque 标识符；游戏侧自解释，框架不解析）。只读：打包期烙印。
        /// 双调法读 get_link_href（同 <see cref="Node.Id"/> getter 通道）；rc=1 = 非 Link 节点
        /// （类型错配的调用方错误）→ 抛 UIContractException。
        /// </summary>
        public string Href
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                string v = TextControlFFI.ReadTextOrNull(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_link_href(hp, _id, buf, cap, len));
                if (v == null)
                    throw new UIContractException(
                        $"Href 仅 Link 节点可读（node {_id} 非 Link 或无 href 条目）");
                return v;
            }
        }

        // 语义 sugar：与 Button.Clicked 同款 backing-dict 模式（On<ClickEvent> 订阅翻译成 Action）。
        // Link 的命中细化到 a 节点本身（含嵌套 span 内文字），点击即冒泡至此。
        [NonSerialized] Dictionary<Action, EventRegistration> _clickedBacking;
        public event Action Clicked
        {
            add
            {
                if (value == null) return;
                if (_clickedBacking == null)
                    _clickedBacking = new Dictionary<Action, EventRegistration>();
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
    }

    public class ListItem : Container
    {
        internal ListItem(UIContext ctx, ulong id) : base(ctx, id) { }
        // 业务逻辑项序号（tick-drain BindItem 时由 UIContext 回填，不走 FFI）。
        // core 不存该值；item_index 进 pending_binds 队列，C# 取后传给本属性。
        internal int _index;
        public int Index => _index;
    }

    public unsafe class Button : Container
    {
        internal Button(UIContext ctx, ulong id) : base(ctx, id) { }

        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }
        void SetNodeDisabled(bool v) { StageHandle* h = (StageHandle*)_ctx._stage.ToPointer(); Native.yio_stage_set_node_disabled(h, _id, v); }
        bool GetNodeDisabled() { StageHandle* h = (StageHandle*)_ctx._stage.ToPointer(); byte b = 0; Native.yio_stage_get_node_disabled(h, _id, &b); return b != 0; }
        // 文本走 Container.TextContent

        // semantic sugar：Action 参数无类型——handler 形参与 ClickEvent 解耦，对齐 UGUI Button.onClick。
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
        internal static string GetControlText(StageHandle* h, ulong id) =>
            ReadText(h, id, (hp, buf, cap, len) => Native.yio_stage_get_control_text(hp, id, buf, cap, len));

        internal static void SetControlText(StageHandle* h, ulong id, string v)
        {
            byte[] b = Encoding.UTF8.GetBytes(v ?? "");
            fixed (byte* bp = b)
            {
                int rc = Native.yio_stage_set_control_text(h, id, bp, (nuint)b.Length);
                if (rc != 0) throw new InvalidOperationException($"set_control_text failed (node {id})");
            }
        }

        internal static string GetControlPlaceholder(StageHandle* h, ulong id) =>
            ReadText(h, id, (hp, buf, cap, len) => Native.yio_stage_get_control_placeholder(hp, id, buf, cap, len));

        internal static void SetControlPlaceholder(StageHandle* h, ulong id, string v)
        {
            byte[] b = Encoding.UTF8.GetBytes(v ?? "");
            fixed (byte* bp = b)
            {
                int rc = Native.yio_stage_set_control_placeholder(h, id, bp, (nuint)b.Length);
                if (rc != 0) throw new InvalidOperationException($"set_control_placeholder failed (node {id})");
            }
        }

        internal static TextSelection GetSelection(StageHandle* h, ulong id)
        {
            nuint start = 0, end = 0;
            int rc = Native.yio_stage_get_selection(h, id, &start, &end);
            if (rc != 0) throw new InvalidOperationException($"get_selection failed (node {id})");
            return new TextSelection((int)start, (int)end);
        }

        internal static void SetSelection(StageHandle* h, ulong id, int anchor, int cursor)
        {
            int rc = Native.yio_stage_set_selection(h, id, (nuint)anchor, (nuint)cursor);
            if (rc != 0) throw new InvalidOperationException($"set_selection failed (node {id})");
        }

        internal static void SetControlReadonly(StageHandle* h, ulong id, bool v)
        {
            int rc = Native.yio_stage_set_control_readonly(h, id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_readonly failed (node {id})");
        }

        // get_control_readonly：return-code + byte* out（与 set 对称的读出口）。TextField / TextArea /
        // NumberField 共享 EditState，故三者皆读。非文本控件 / 节点缺失 / null out → rc=-1；命中 → rc=0
        // 且 *out 已填（0/1）。rc<0 升异常不吞（post-ThrowIfDisposed 理论不达）。
        internal static bool GetControlReadonly(StageHandle* h, ulong id)
        {
            byte b = 0;
            int rc = Native.yio_stage_get_control_readonly(h, id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_control_readonly failed (node {id}, non-text?)");
            return b != 0;
        }

        internal static void SetNodeDisabled(StageHandle* h, ulong id, bool v)
        {
            Native.yio_stage_set_node_disabled(h, id, v);
        }

        // maxlength：UTF-8 字符上限（0 = 无限）。setter 不追溯裁剪现有 value（HTML maxlength
        // 语义——只限后续输入/粘贴）；getter 与 set 对称（TextField/TextArea 双变体口径）。
        internal static void SetControlMaxLength(StageHandle* h, ulong id, int v)
        {
            // 负值拒绝（HTML maxlength 同语义）：FFI 参数是 nuint，直接 cast 会让 -1 回绕成
            // 0xFFFFFFFF（≈无限）静默通过，getter 再 (int) 回来变 -1——往返对称但语义是错的。
            if (v < 0)
                throw new ArgumentOutOfRangeException(nameof(v), v, "MaxLength must be >= 0 (0 = unlimited)");
            int rc = Native.yio_stage_set_control_maxlength(h, id, (nuint)v);
            if (rc != 0) throw new InvalidOperationException($"set_control_maxlength failed (node {id})");
        }

        internal static int GetControlMaxLength(StageHandle* h, ulong id)
        {
            nuint v = 0;
            int rc = Native.yio_stage_get_control_maxlength(h, id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_maxlength failed (node {id}, non-text?)");
            return (int)v;
        }

        // get_node_disabled：void + byte* out（与 set 对称的读出口）。null 句柄 / 节点缺失 → 写 0（false），
        // 不报错（与 set 的「悬空 NodeId 静默跳过」语义一致）。所有 Node 子类的 Disabled getter 经此。
        internal static bool GetNodeDisabled(StageHandle* h, ulong id)
        {
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, id, &b);
            return b != 0;
        }

        // get_control_text/get_control_placeholder 共用的双调法：fn(h, buf, cap, out_len) → rc。
        // 先 stackalloc 256 探；rc=-2 时 *out_len = 所需 → 堆分配按所需重调一次（必合）。非文本/-1 升异常。
        // FFI 写恰好 out_len 字节（copy_nonoverlapping，无 NUL 填充）——不做 TrimEnd('\0')，信任契约
        // （用户合法设 Value 含 '\0' 也不被静默截断；之前防御性 trim 是死代码 + 值腐化风险）。
        internal static string ReadText(StageHandle* h, ulong id, ReadTextFn fn)
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

        // ReadText 的可空变体：rc=1 是「语义空值」（如 Dropdown 无选项的 SelectedValue），
        // 返 null 而非抛。其余 rc 语义同 ReadText（0=有值 / -2 扩容重调 / 其他=抛）。
        internal static string ReadTextOrNull(StageHandle* h, ulong id, ReadTextFn fn)
        {
            nuint needed = 0;
            Span<byte> stackBuf = stackalloc byte[256];
            fixed (byte* sbp = stackBuf)
            {
                int rc = fn(h, sbp, (nuint)stackBuf.Length, &needed);
                if (rc == 1) return null;
                if (rc == 0) return Encoding.UTF8.GetString(stackBuf.Slice(0, (int)needed));
                if (rc != -2) throw new InvalidOperationException($"read text failed rc={rc} (node {id})");
            }
            byte[] heapBuf = new byte[(int)needed];
            fixed (byte* hbp = heapBuf)
            {
                nuint written = 0;
                int rc = fn(h, hbp, (nuint)heapBuf.Length, &written);
                if (rc == 1) return null;
                if (rc != 0) throw new InvalidOperationException($"read text retry failed rc={rc} (node {id})");
                return Encoding.UTF8.GetString(heapBuf, 0, (int)written);
            }
        }

        internal delegate int ReadTextFn(StageHandle* h, byte* buf, nuint cap, nuint* outLen);
    }

    public unsafe class TextField : Node
    {
        internal TextField(UIContext ctx, ulong id) : base(ctx, id) { }

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
        // MaxLength：UTF-8 字符上限（0 = 无限）。setter 不追溯裁剪现有 value（HTML maxlength
        // 语义——只限后续输入/粘贴）。
        public int MaxLength
        {
            get { ThrowIfDisposed(); return TextControlFFI.GetControlMaxLength(Handle(), _id); }
            set { ThrowIfDisposed(); TextControlFFI.SetControlMaxLength(Handle(), _id, value); }
        }

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
        internal NumberField(UIContext ctx, ulong id) : base(ctx, id) { }

        // Value：直转 NumberField 专用 FFI（get/set_number_value）。setter 在 core 侧做 clamp[min,max]
        // + step 量化后写回 EditState.value 文本（与 Slider set_control_value 同口径，只是 NumberField
        // 存文本、Slider 存 f32）。getter 解析文本→f32。故 C# 侧只透传，不做 clamp/量化。
        public float Value
        {
            get { ThrowIfDisposed(); return GetNumberValue(); }
            set { ThrowIfDisposed(); SetNumberValue(value); }
        }
        // Min/Max/Step：core ControlState::NumberField 存了 min/max/step（打包期 ControlInit 烘焙，
        // set_number_value 据此 clamp+量化）。getter 复用 get_control_min/max/step；setter 直转
        // set_control_min/max/step（FFI arm 已扩到 NumberField）：改界后 core 侧把 value 文本
        // parse→clamp→量化→re-format，C# 只透传，不做 clamp/量化。
        public float Min { get { ThrowIfDisposed(); return GetControlMin(); } set { ThrowIfDisposed(); SetControlMin(value); } }
        public float Max { get { ThrowIfDisposed(); return GetControlMax(); } set { ThrowIfDisposed(); SetControlMax(value); } }
        public float Step { get { ThrowIfDisposed(); return GetControlStep(); } set { ThrowIfDisposed(); SetControlStep(value); } }
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

        // value：NumberField 专用通道（clamp+量化在 core）。float out 经 local + &local（同 GetControlValue）。
        float GetNumberValue()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.yio_stage_get_number_value(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_number_value failed (node {_id})");
            return v;
        }
        void SetNumberValue(float v)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_number_value(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_number_value failed (node {_id})");
        }
        // readonly/disabled 经 TextControlFFI（readonly 共享 EditState 通道，disabled 经 node flag）。
        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        void SetControlReadonly(bool v) => TextControlFFI.SetControlReadonly(Handle(), _id, v);
        bool GetControlReadonly() => TextControlFFI.GetControlReadonly(Handle(), _id);
        void SetNodeDisabled(bool v) => TextControlFFI.SetNodeDisabled(Handle(), _id, v);
        bool GetNodeDisabled() => TextControlFFI.GetNodeDisabled(Handle(), _id);
        // min/max/step：复用 Slider 同名 FFI（get_control_min/max/step 已扩到 NumberField）。
        // float out 经 local + &local（同 GetControlValue 局部取址模式）。rc!=0 升异常不吞。
        float GetControlMin()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.yio_stage_get_control_min(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_min failed (node {_id})");
            return v;
        }
        float GetControlMax()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.yio_stage_get_control_max(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_max failed (node {_id})");
            return v;
        }
        float GetControlStep()
        {
            StageHandle* h = Handle();
            float v = 0f; int rc = Native.yio_stage_get_control_step(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_step failed (node {_id})");
            return v;
        }
        // setter：core 侧改界后把 value 文本重约束（parse→clamp→量化→re-format）。
        void SetControlMin(float v)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_control_min(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_min failed (node {_id})");
        }
        void SetControlMax(float v)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_control_max(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_max failed (node {_id})");
        }
        void SetControlStep(float v)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_control_step(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_step failed (node {_id})");
        }
    }

    public unsafe class Slider : Node
    {
        internal Slider(UIContext ctx, ulong id) : base(ctx, id) { }

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

        float GetControlValue()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.yio_stage_get_control_value(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_value failed (node {_id})");
            return v;
        }
        void SetControlValue(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_value(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_value failed (node {_id})");
        }
        float GetControlMin()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.yio_stage_get_control_min(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_min failed (node {_id})");
            return v;
        }
        void SetControlMin(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_min(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_min failed (node {_id})");
        }
        float GetControlMax()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.yio_stage_get_control_max(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_max failed (node {_id})");
            return v;
        }
        void SetControlMax(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_max(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_max failed (node {_id})");
        }
        float GetControlStep()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f; int rc = Native.yio_stage_get_control_step(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_step failed (node {_id})");
            return v;
        }
        void SetControlStep(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_step(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_step failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    public unsafe class Toggle : Node
    {
        internal Toggle(UIContext ctx, ulong id) : base(ctx, id) { }

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

        bool GetControlChecked()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            bool v = false; int rc = Native.yio_stage_get_control_checked(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_checked failed (node {_id})");
            return v;
        }
        void SetControlChecked(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_checked(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_checked failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    public unsafe class RadioButton : Node
    {
        internal RadioButton(UIContext ctx, ulong id) : base(ctx, id) { }

        // IsChecked 直转 FFI set/get_control_checked（与 Toggle 同语义；同组互斥框架自动做）。
        public bool IsChecked
        {
            get { ThrowIfDisposed(); return GetControlChecked(); }
            set { ThrowIfDisposed(); SetControlChecked(value); }
        }
        // Name = radio 分组名（HTML name 语义，决定互斥分组；打包期 data-name bake 进
        // ControlState::Radio）。只读——分组是结构性属性，运行时改名会破坏互斥不变量。
        public string Name
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadText(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_radio_name(hp, _id, buf, cap, len));
            }
        }
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

        bool GetControlChecked()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            bool v = false; int rc = Native.yio_stage_get_control_checked(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_checked failed (node {_id})");
            return v;
        }
        void SetControlChecked(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_checked(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_checked failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    public unsafe class TextArea : Node
    {
        internal TextArea(UIContext ctx, ulong id) : base(ctx, id) { }

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
        // MaxLength：UTF-8 字符上限（0 = 无限），不追溯裁剪现有 value（同 TextField）。
        public int MaxLength
        {
            get { ThrowIfDisposed(); return TextControlFFI.GetControlMaxLength(Handle(), _id); }
            set { ThrowIfDisposed(); TextControlFFI.SetControlMaxLength(Handle(), _id, value); }
        }

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
        internal Dropdown(UIContext ctx, ulong id) : base(ctx, id) { }

        // SelectedIndex：直转 FFI get/set_dropdown_selected_index。core ControlState::Dropdown
        // 的 selected_index（打包期 ControlInit::Dropdown.options 由 <option selected> 烘焙初值；运行时
        // 交互 / 本 setter 改写）。FFI 以 uint* 出参，公共签名用 int（index 不会超 int 正区）——边界 cast。
        public int SelectedIndex
        {
            get { ThrowIfDisposed(); return (int)GetDropdownSelectedIndex(); }
            set { ThrowIfDisposed(); SetDropdownSelectedIndex((uint)value); }
        }
        // SelectedValue：选中 option 的 value（`value` 内容属性，HTML 语义——缺席回落该项文本；
        // 无选项返 null）。只读——value 由选中项派生，业务经 SelectedIndex 改选。rc=1 = 无选项。
        public string SelectedValue
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadTextOrNull(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_dropdown_selected_value(hp, _id, buf, cap, len));
            }
        }
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
                // NewValue 派发时实取当前选中值（事件在 core 已应用新 index 后泵出，
                // get_dropdown_selected_value 读到的就是新选中项）；OldValue 无数据源
                // （core 事件流只携新 index）→ null，同 ValueChangedEvent.OldValue=default 家族语义。
                var reg = On<ControlSelectionChangedEvent>(e =>
                {
                    string nv = null;
                    try { nv = SelectedValue; } catch (InvalidOperationException) { }
                    value(new SelectionChangedEvent { _oldIndex = -1, _newIndex = e.NewIndex, _newValue = nv });
                });
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

        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        uint GetDropdownSelectedIndex()
        {
            StageHandle* h = Handle();
            uint v = 0; int rc = Native.yio_stage_get_dropdown_selected_index(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_dropdown_selected_index failed (node {_id})");
            return v;
        }
        void SetDropdownSelectedIndex(uint v)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_dropdown_selected_index(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_dropdown_selected_index failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = Handle();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = Handle();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    // OptionItem = <option> 的 typed 投影（Dropdown 的子项）。结构上是容器型节点（围栏 content=text，
    // 可被渲染当文本块），故继承 Container（同 ListItem 模式）。NodeFactory 据 NodeKind.OptionItem
    // 派发到本类（替代之前的 Container 回落）。
    //
    // Value：option 的 `value` 内容属性（打包期静态配置，缺席回落自身文本——HTML 语义）。
    // Selected：父 Dropdown 当前选中项的合成判定（上溯 + 声明序对位，非字面存储）。
    // Disabled 读 NodeFlags::DISABLED（通用 node flag）。
    public unsafe class OptionItem : Container
    {
        internal OptionItem(UIContext ctx, ulong id) : base(ctx, id) { }

        // value 内容属性优先；缺席回落 option 文本（与 Dropdown.SelectedValue 同源同口径）。
        public string Value
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadText(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_option_value(hp, _id, buf, cap, len));
            }
        }
        // 序号 == 父 Dropdown.selected_index 的合成值（core 侧上溯派生，改选即跟随）。
        public bool Selected
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int rc = Native.yio_stage_is_option_selected(h, _id);
                if (rc < 0)
                    throw new InvalidOperationException($"is_option_selected failed (node {_id}: not an option / no dropdown ancestor)");
                return rc == 1;
            }
        }
        /// <summary>本 option 在所属 Dropdown 里的声明序（0 基，与 SelectedIndex / 键盘 seek
        /// 同口径）。读失败（非 option / 上溯无 Dropdown）抛 InvalidOperationException。</summary>
        public int Index
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int idx = Native.yio_stage_get_option_index(h, _id);
                if (idx < 0)
                    throw new InvalidOperationException($"get_option_index failed (node {_id}: not an option / no dropdown ancestor)");
                return idx;
            }
        }
        // Disabled：伪类源（NodeFlags::DISABLED）。setter 直 FFI；getter 读 node flag（与 Slider 等一致）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Slot = <slot> 的 typed 投影。**打包期投影后产物中不再有 Slot 节点**（slot 在拼接位被
    // 消费：light 子替换或 fallback 原位拼接，见 CustomElement 注释）——本类仅为 kind 派发
    // 保留的 typed shell（动态建树路径的完备性），正常 pkg 实例化不产生本类实例。
    public class Slot : Container
    {
        internal Slot(UIContext ctx, ulong id) : base(ctx, id) { }
    }

    // CustomElement = 带连字符的自定义标签（<my-widget>）的 typed 投影。打包期由组件系统展开：
    // host 节点 kind=CustomElement（保留原始 tag 字面量），组件模板子树挂 host 下，<slot> 投影
    // 在拼接位消费（产物无 Slot 节点）。host 是硬墙作用域——投影内容归组件域（Get/Query 不穿透），
    // host 自身归页面域。组件注册 = 打包器 components/ 目录（Package 注册表承担
    // customElements.define() 角色）。
    //
    // 类绑定（RegisterComponent）：用户经 ctx.RegisterComponent("my-widget", factory) 注册
    // 派生子类后，NodeFactory 对该 tag 构造用户类型（行为接线进 typed 子类，替代 wrapper div +
    // TryGet 绕法）。生命周期回调 OnConnected/OnDisconnected 见下。
    public unsafe class CustomElement : Container
    {
        // protected internal：NodeFactory（本程序集）与用户子类（RegisterComponent 工厂
        // 委托里 new）都可链本构造。
        protected internal CustomElement(UIContext ctx, ulong id) : base(ctx, id) { }

        /// <summary>
        /// 原始 hyphen 标签名（`<game-item-card>` → "game-item-card"；pkg v35 展开保留字面量，
        /// 打包期烘入）。非 CustomElement 节点不会构成本类实例。读失败（理论不可达）抛
        /// InvalidOperationException（双调法 FFI，同 RadioButton.Name 口径）。
        /// </summary>
        public string Tag
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                return TextControlFFI.ReadText(h, _id,
                    (hp, buf, cap, len) => Native.yio_stage_get_custom_tag(hp, _id, buf, cap, len));
            }
        }

        /// <summary>
        /// wrapper 构造完成（派生类 ctor 已跑完）后由投影层回调——组件接线点（订阅事件 /
        /// 取内部节点引用 / 初始化状态）。所有构造路径都触发：instantiate（eager 物化）、
        /// 懒物化（Parent/Children/Get 访问）、事件预物化。由 NodeFactory 在工厂委托
        /// 返回后调用——不在 ctor 链内调虚方法（派生字段未初始化）。
        /// </summary>
        protected virtual void OnConnected() { }

        /// <summary>
        /// 节点死亡时回调。两条路径汇入：用户调 Dispose（同步，回调时 core 节点已删）；
        /// Rust 侧删除（list 槽位换绑淘汰克隆 / 外部 remove_node / 内部剪枝——经
        /// UIContext.PumpRemovedNodes 帧泵，宿主每帧驱动）。回调后 wrapper 标 _disposed，
        /// 后续公共读抛 ObjectDisposedException。重挂（再 instantiate 同 tag）= 新实例
        /// + 新 OnConnected——身份缓存不复活旧对象。
        /// </summary>
        protected virtual void OnDisconnected() { }

        // 投影层桥：protected virtual 不便跨可见性直调，fire 入口收口在这两桥。
        internal void FireConnected() => OnConnected();
        internal void FireDisconnected() => OnDisconnected();
    }

    // TabList = <div role="tablist"> 的 typed 投影（WAI-ARIA tablist 容器，持若干 <button role=tab> 子）。
    // 继承 Container（同 ListView，因持有 tab 子节点——非 Dropdown 那样的叶子控件）。
    //
    // ControlState::TabList{selected_index}：selected_index 由打包期 aria-selected="true" 烘焙初值，
    // 运行时交互（click / 方向键）与本 setter 改写（core 合成 aria-selected 到各 tab，
    // 并触发 SelectionChanged）。SelectionChanged 复用 Dropdown 同源 ControlSelectionChangedEvent +
    // 公共 SelectionChangedEvent（core 侧同一 EVT_SELECTION_CHANGED=26，touch_id=新 index）——零新增
    // event struct / demux arm。
    public unsafe class TabList : Container
    {
        internal TabList(UIContext ctx, ulong id) : base(ctx, id) { }

        // SelectedIndex：直转 FFI get/set_tablist_selected_index。uint* 出参，公共签名用 int
        // （index 不会超 int 正区）——边界 cast。rc!=0（节点非 TabList / 不 live）升 InvalidOperationException
        // 不吞（ThrowIfDisposed 后正常路径不该达）。
        public int SelectedIndex
        {
            get { ThrowIfDisposed(); return (int)GetTabListSelectedIndex(); }
            set { ThrowIfDisposed(); SetTabListSelectedIndex((uint)value); }
        }
        // Activation：激活模型（HTML data-activation 属性的运行时面）。Manual = 方向键只移
        // 焦点、Enter/Space 提交选中；Automatic（缺省）= 方向键即时选中且焦点跟随。
        public TabActivation Activation
        {
            get { ThrowIfDisposed(); return GetTabActivation() ? TabActivation.Manual : TabActivation.Automatic; }
            set { ThrowIfDisposed(); SetTabActivation(value == TabActivation.Manual); }
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

        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        uint GetTabListSelectedIndex()
        {
            StageHandle* h = Handle();
            uint v = 0; int rc = Native.yio_stage_get_tablist_selected_index(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_tablist_selected_index failed (node {_id})");
            return v;
        }
        void SetTabListSelectedIndex(uint v)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_tablist_selected_index(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_tablist_selected_index failed (node {_id})");
        }
        bool GetTabActivation()
        {
            StageHandle* h = Handle();
            byte b = 0; int rc = Native.yio_stage_get_tab_activation(h, _id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_tab_activation failed (node {_id})");
            return b != 0;
        }
        void SetTabActivation(bool manual)
        {
            StageHandle* h = Handle();
            int rc = Native.yio_stage_set_tab_activation(h, _id, manual);
            if (rc != 0) throw new InvalidOperationException($"set_tab_activation failed (node {_id})");
        }
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = Handle();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = Handle();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
    }

    // Tab = <button role="tab"> 的 typed 投影（TabList 的子项）。结构上是容器型节点（围栏 content=text，
    // 可持 label / 图标子），继承 Container（同 OptionItem 模式）。
    //
    // Selected：父 TabList.SelectedIndex + 自身 DOM 序的合成判定（core 侧上溯派生，与
    // aria-selected synth 同源——非字面存储）。或订阅 TabList.SelectionChanged（payload=新 index）。
    // Disabled 读 NodeFlags::DISABLED（通用 node flag，与 OptionItem 一致）。
    public unsafe class Tab : Container
    {
        internal Tab(UIContext ctx, ulong id) : base(ctx, id) { }

        // 序号 == 父 TabList.selected_index 的合成值（core 侧上溯派生，切换即跟随）。
        public bool Selected
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int rc = Native.yio_stage_is_tab_selected(h, _id);
                if (rc < 0)
                    throw new InvalidOperationException($"is_tab_selected failed (node {_id}: not a tab / no tablist ancestor)");
                return rc == 1;
            }
        }
        // Disabled：伪类源（NodeFlags::DISABLED）。setter 直 FFI；getter 读 node flag（与 OptionItem 等一致）。
        public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }

        StageHandle* Handle() => (StageHandle*)_ctx._stage.ToPointer();
        void SetNodeDisabled(bool v)
        {
            StageHandle* h = Handle();
            Native.yio_stage_set_node_disabled(h, _id, v);
        }
        bool GetNodeDisabled()
        {
            StageHandle* h = Handle();
            byte b = 0;
            Native.yio_stage_get_node_disabled(h, _id, &b);
            return b != 0;
        }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Tree = <div role="tree"> 的 typed 投影（#8，WAI-ARIA 层级列表容器，单选树）。
    // 继承 Container（条目是子树内任意深度的 role=treeitem，直接嵌套声明无 group 包装层）。
    //
    // ControlState::Tree{selected}：选中项 NodeId（打包期 aria-selected="true" 烘焙初值，
    // 无则首项）。交互（click / APG 核心档键盘）与本 setter 改写；aria-selected 由 core synth
    // 到各条目。事件：交互路径发 EVT_SELECTION_CHANGED@tree（payload 无身份——NodeId 64 位
    // 不进 i32，事件只作「变了」信号，SelectedItem 读 FFI 取当前值）。
    public unsafe class Tree : Container
    {
        internal Tree(UIContext ctx, ulong id) : base(ctx, id) { }

        /// <summary>当前选中条目；空树 / 无选中 → null。rc=1（无选中）映射 null，非异常。</summary>
        public TreeItem SelectedItem
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                ulong item = 0;
                int rc = Native.yio_stage_get_tree_selected(h, _id, &item);
                if (rc < 0)
                    throw new InvalidOperationException($"get_tree_selected failed (node {_id}: not a tree)");
                if (rc == 1 || item == 0) return null;
                return (TreeItem)_ctx._registry.GetOrCreate(item);
            }
            set
            {
                ThrowIfDisposed();
                if (value == null) throw new ArgumentNullException("value");
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int rc = Native.yio_stage_set_tree_selected(h, _id, value._id);
                if (rc != 0)
                    throw new InvalidOperationException($"set_tree_selected failed (node {_id}: item {value._id} not in this tree)");
            }
        }

        /// <summary>全部 branch 条目统一展开（程序化批量，不发 ExpandChanged）。</summary>
        public void ExpandAll() { SetAllExpanded(true); }
        /// <summary>全部 branch 条目统一折叠（程序化批量，不发 ExpandChanged）。</summary>
        public void CollapseAll() { SetAllExpanded(false); }
        void SetAllExpanded(bool expanded)
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_tree_set_all_expanded(h, _id, expanded ? (byte)1 : (byte)0);
            if (rc != 0)
                throw new InvalidOperationException($"tree_set_all_expanded failed (node {_id})");
        }

        // SelectionChanged：core EVT_SELECTION_CHANGED@tree（交互路径：点击条目 / 键盘移动）。
        // 翻译为 TreeSelectionChangedEvent——SelectedItem 在事件时点读 FFI 取当前选中（core 事件
        // 不携身份）。程序化 SelectedItem=value 不发事件（同 TabList.SelectedIndex 语义）。
        [NonSerialized] Dictionary<Action<TreeSelectionChangedEvent>, EventRegistration> _selectionChangedBacking;
        public event Action<TreeSelectionChangedEvent> SelectionChanged
        {
            add
            {
                if (value == null) return;
                if (_selectionChangedBacking == null)
                    _selectionChangedBacking = new Dictionary<Action<TreeSelectionChangedEvent>, EventRegistration>();
                if (_selectionChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlSelectionChangedEvent>(e => value(new TreeSelectionChangedEvent { _tree = this }));
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
    }

    /// <summary>Tree 选中变更事件（#8）。SelectedItem 在事件时点读 core 现值。</summary>
    public sealed class TreeSelectionChangedEvent
    {
        internal Tree _tree;
        /// <summary>发出事件的 Tree。</summary>
        public Tree Tree { get { return _tree; } }
        /// <summary>事件时点的选中条目（core 现值；空树为 null）。</summary>
        public TreeItem SelectedItem { get { return _tree.SelectedItem; } }
    }

    // TreeItem = <div role="treeitem"> 的 typed 投影（#8）。容器型（label 内容 + 可选嵌套
    // treeitem——branch 有展开/折叠态，leaf 无）。结构上镜像 Tab（条目持 label 子）。
    //
    // Selected 从所属 Tree.selected 跨节点派生（aria-selected synth 同源）；Expanded 是
    // branch 自身 ControlState（leaf 读/写抛 InvalidOperationException——先用 IsBranch 判）。
    public unsafe class TreeItem : Container
    {
        internal TreeItem(UIContext ctx, ulong id) : base(ctx, id) { }

        /// <summary>是否 branch（有嵌套 treeitem，可展开/折叠）。leaf 无展开态。</summary>
        public bool IsBranch
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                byte b = 0;
                int rc = Native.yio_stage_get_treeitem_expanded(h, _id, &b);
                return rc == 0;
            }
        }

        /// <summary>branch 的展开/折叠态。leaf 读写均抛 InvalidOperationException（先用 IsBranch 判）。
        /// setter 是程序化改态（不发 ExpandChanged——交互路径才发，展开剪枝由 core visuals 同步）。</summary>
        public bool Expanded
        {
            get
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                byte b = 0;
                int rc = Native.yio_stage_get_treeitem_expanded(h, _id, &b);
                if (rc != 0)
                    throw new InvalidOperationException($"get_treeitem_expanded failed (node {_id}: leaf has no expansion)");
                return b != 0;
            }
            set
            {
                ThrowIfDisposed();
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int rc = Native.yio_stage_set_treeitem_expanded(h, _id, value ? (byte)1 : (byte)0);
                if (rc != 0)
                    throw new InvalidOperationException($"set_treeitem_expanded failed (node {_id}: leaf has no expansion)");
            }
        }

        /// <summary>是否当前选中（所属 Tree.selected 恒等派生）。无 Tree 祖先 → false。</summary>
        public bool Selected
        {
            get
            {
                ThrowIfDisposed();
                Tree owner = OwnerTree();
                return owner != null && owner.SelectedItem != null && owner.SelectedItem._id == _id;
            }
        }

        /// <summary>层级（ARIA aria-level：顶层=1）。数到 Tree 根的 TreeItem 祖先数 +1；无 Tree 祖先 → 0。</summary>
        public int Level
        {
            get
            {
                ThrowIfDisposed();
                int level = 1;
                Node cur = Parent;
                for (int i = 0; i < 100000 && cur != null; i++)
                {
                    if (cur is Tree) return level;
                    if (cur is TreeItem) level++;
                    cur = cur.Parent;
                }
                return 0;
            }
        }

        /// <summary>程序化选中本条目（所属 Tree.SelectedItem = this；无 Tree 祖先 → InvalidOperationException）。</summary>
        public void Select()
        {
            ThrowIfDisposed();
            Tree owner = OwnerTree() ?? throw new InvalidOperationException($"treeitem {_id} has no tree ancestor");
            owner.SelectedItem = this;
        }

        Tree OwnerTree()
        {
            Node cur = Parent;
            for (int i = 0; i < 100000 && cur != null; i++)
            {
                if (cur is Tree t) return t;
                cur = cur.Parent;
            }
            return null;
        }

        // ExpandedChanged：core EVT_EXPAND_CHANGED@treeitem（交互路径：点击 branch /
        // Enter/Space/Right/Left 键）。程序化 Expanded=value 不发。
        [NonSerialized] Dictionary<Action<ExpandChangedEvent>, EventRegistration> _expandedChangedBacking;
        public event Action<ExpandChangedEvent> ExpandedChanged
        {
            add
            {
                if (value == null) return;
                if (_expandedChangedBacking == null)
                    _expandedChangedBacking = new Dictionary<Action<ExpandChangedEvent>, EventRegistration>();
                if (_expandedChangedBacking.ContainsKey(value)) return;
                var reg = On<ControlExpandChangedEvent>(e => value(new ExpandChangedEvent { _item = this, _expanded = e.Expanded }));
                _expandedChangedBacking[value] = reg;
            }
            remove
            {
                if (_expandedChangedBacking != null && _expandedChangedBacking.TryGetValue(value, out var reg))
                {
                    _expandedChangedBacking.Remove(value);
                    reg.Dispose();
                }
            }
        }
    }

    /// <summary>Tree branch 条目展开/折叠事件（#8）。交互路径（点击/键盘）才发；程序化 setter 不发。</summary>
    public sealed class ExpandChangedEvent
    {
        internal TreeItem _item;
        internal bool _expanded;
        /// <summary>展开/折叠的条目。</summary>
        public TreeItem Item { get { return _item; } }
        /// <summary>新态（true=展开 / false=折叠）。</summary>
        public bool Expanded { get { return _expanded; } }
    }

    public unsafe class ProgressBar : Node
    {
        internal ProgressBar(UIContext ctx, ulong id) : base(ctx, id) { }

        // 投影层填实：直转 FFI set/get_control_value·set/get_control_max（value clamp [0,max]）。
        // rc<0（非值控件 / 节点缺失）经 ThrowIfDisposed 后不该达——升 InvalidOperationException 不吞。
        // 动画期间（AnimateValue）get 返缓存目标（数据值），set 显式获胜：取消动画后直写。
        public float Value
        {
            get { ThrowIfDisposed(); return _animating ? _animTarget : GetControlValue(); }
            set { ThrowIfDisposed(); if (_animating) FinishAnim(writeFinal: false); SetControlValue(value); }
        }
        public float Max
        {
            get { ThrowIfDisposed(); return GetControlMax(); }
            set { ThrowIfDisposed(); SetControlMax(value); }
        }
        // min 参与填充数学（ARIA：(value-min)/(max-min)，#97），运行时可改；FFI 已对 Progress 开放。
        public float Min
        {
            get { ThrowIfDisposed(); return GetControlMin(); }
            set { ThrowIfDisposed(); SetControlMin(value); }
        }
        // indeterminate（不确定进度态）：FFI 读写 Progress 状态位（get/set_control_indeterminate）。
        // 纯状态——视觉切换走作者 CSS 选择器（core 不做 marquee 渲染）。value/max 不受扰动。
        public bool IsIndeterminate
        {
            get { ThrowIfDisposed(); return GetControlIndeterminate(); }
            set { ThrowIfDisposed(); SetControlIndeterminate(value); }
        }

        // Value 走 taffy 布局通道每帧离散重算 fill 宽（CSS transition 只覆盖背景/文字/透明
        // 三通道，布局属性无过渡），演出缓动归 C# 投影层。动画期间 _animTarget 缓存目标：
        // Value 读回数据值，插值中间值经 FFI 只喂渲染；直接赋 Value 显式获胜（取消动画）。
        IDisposable _animSub;
        float _animTarget, _animFrom, _animElapsed, _animDuration;
        bool _animating;

        /// <summary>
        /// 演出缓动：在 <paramref name="durationSec"/> 秒内把填充从当前显示值 easeOut
        /// 趋近 <paramref name="target"/>（clamp 到 [0, Max]）。进行中重复调用重锚——
        /// 从当前插值位置平滑转向新目标。订阅随节点 Dispose 自动清理（OnUpdate 契约）。
        /// </summary>
        public void AnimateValue(float target, float durationSec = 0.4f)
        {
            ThrowIfDisposed();
            if (durationSec <= 0f) { Value = target; return; }
            float max = GetControlMax();
            if (max <= 0f) max = 1f;
            _animFrom = GetControlValue();
            _animTarget = Math.Max(0f, Math.Min(target, max));
            _animElapsed = 0f;
            _animDuration = durationSec;
            if (!_animating)
            {
                _animating = true;
                _animSub = OnUpdate(AnimateStep);
            }
        }

        void AnimateStep(float dt)
        {
            if (!_animating) return;
            _animElapsed += dt;
            float t = _animElapsed / _animDuration;
            if (t >= 1f)
            {
                FinishAnim(writeFinal: true);
                return;
            }
            float e = 1f - (1f - t) * (1f - t) * (1f - t); // easeOutCubic
            SetControlValue(_animFrom + (_animTarget - _animFrom) * e);
        }

        void FinishAnim(bool writeFinal)
        {
            _animating = false;
            _animSub?.Dispose();
            _animSub = null;
            if (writeFinal) SetControlValue(_animTarget);
        }

        static NotImplementedException NE() => new NotImplementedException();

        bool GetControlIndeterminate()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte b = 0;
            int rc = Native.yio_stage_get_control_indeterminate(h, _id, &b);
            if (rc != 0) throw new InvalidOperationException($"get_control_indeterminate failed (node {_id})");
            return b != 0;
        }
        void SetControlIndeterminate(bool v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_indeterminate(h, _id, v ? (byte)1 : (byte)0);
            if (rc != 0) throw new InvalidOperationException($"set_control_indeterminate failed (node {_id})");
        }

        // float out 经 local + &local（同 GetWorldMatrix 局部取址模式，不用 fixed）。rc<0 升异常不吞。
        // internal：headless 测试观测动画期间的 FFI 显示值（Value 公共读回是缓存目标）。
        internal float GetControlValue()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f;
            int rc = Native.yio_stage_get_control_value(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_value failed (node {_id})");
            return v;
        }
        void SetControlValue(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_value(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_value failed (node {_id})");
        }
        float GetControlMax()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f;
            int rc = Native.yio_stage_get_control_max(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_max failed (node {_id})");
            return v;
        }
        void SetControlMax(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_max(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_max failed (node {_id})");
        }
        float GetControlMin()
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            float v = 0f;
            int rc = Native.yio_stage_get_control_min(h, _id, &v);
            if (rc != 0) throw new InvalidOperationException($"get_control_min failed (node {_id})");
            return v;
        }
        void SetControlMin(float v)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_stage_set_control_min(h, _id, v);
            if (rc != 0) throw new InvalidOperationException($"set_control_min failed (node {_id})");
        }
    }

    // 虚拟化是运行时实现决策，不进 HTML。首次设 ItemCount/ItemTemplate/BindItem 即数据驱动+清空设计期 li；
    // 静态/数据驱动强制互斥（越界抛 UIContractException）。
    public unsafe class ListView : Container
    {
        internal ListView(UIContext ctx, ulong id) : base(ctx, id) { }

        // C# 侧缓存（core 无 item-count getter FFI）。setter 过桥后回填本字段，getter 直读。
        // set 0 时回填 0，保证 getter 与 core item_count 同步。
        int _itemCount;
        // 首次设 ItemCount 标记：首次过桥后调 drain_now 同帧克隆初始 slot + binds 入队，
        // 再 DrainPendingBinds 绑定（同帧 bind，避免首帧模板原样）。后续 set 靠
        // tick-drain 自然推进，无需重复 drain（hot-path 避免 FFI 开销）。
        bool _firstItemCountSet;
        // BindItem 委托（core 不感知，纯 C# 业务回调）。
        // ItemTemplate/TemplateSelector：模板选择结果经 FFI 批量推送进 core（selector
        // 在本类求值——core 零回调；见 EvaluateAndPushTemplates）。委托本体仍只存 C# 侧。
        // internal：UIContext.DrainPendingBinds 同程序集直读调本委托。
        internal Action<ListItem, int> _bindItem;
        UITemplate _itemTemplate;
        Func<int, UITemplate> _templateSelector;

        /// <summary>
        /// 项数（数据驱动）。setter 调 yio_list_set_item_count：首次调用若该 ul 尚未进入
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
                // selector 先求值推送（enter 前 = core 侧 pending 缓冲，enter 收养蓝图后
                // 解析；已 enter 则即时重映射）。null/包组件模板在此抛（严格派）。
                EvaluateAndPushTemplates(0, value);
                StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                int rc = Native.yio_list_set_item_count(h, _id, value);
                if (rc == -2)
                    throw new UIContractException(
                        $"list (node {_id}) has multiple <template> children but neither " +
                        "ItemTemplate nor TemplateSelector was set (multiple templates need a choice)");
                if (rc != 0)
                    throw new InvalidOperationException(
                        $"list_set_item_count failed (node {_id}): not a ListView / no template source");
                _itemCount = value;
                _ctx.RegisterListView(this);
                // 首次进入数据驱动：同帧推进虚拟化管线（plan+execute 克隆初始 slot + binds 入队），
                // 再 DrainPendingBinds 绑定——避免首帧模板原样。后续 set 靠 tick-drain。
                if (!_firstItemCountSet)
                {
                    _firstItemCountSet = true;
                    Native.yio_list_drain_now(h, _id);
                    _ctx.DrainPendingBinds();
                }
            }
        }

        /// <summary>
        /// 项模板（默认蓝图）。SceneSubtree 变体：yio_list_set_template 收养源子树为新蓝图
        /// 并设为 default——enter 前调用会被 core 缓冲到 enter 时消费（不丢）；enter 后调用
        /// 换 default，未显式指定模板的项跟随。PackageComponent 变体需先 Instantiate 再传
        /// （本 setter 只接 SceneSubtree，包组件路径走业务侧 Instantiate + 转传）。
        /// 与 TemplateSelector 同设时 selector 赢（per-item 显式映射优先于默认蓝图）。
        /// 源子树已死（节点被删）→ UIContractException。
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
                    int rc = Native.yio_list_set_template(h, _id, value._srcNodeId);
                    if (rc != 0)
                        throw new UIContractException(
                            $"ItemTemplate rejected (node {_id}): source subtree is stale " +
                            "(the source node was removed — GetTemplate taken before an enter that cleared it?)");
                }
            }
        }

        /// <summary>
        /// 多模板选择器（按 item index 选模板，参与克隆）。严格派语义：设了即全权——
        /// 每个 index 必须返回 UITemplate（返 null 抛 UIContractException；包组件变体需先
        /// Instantiate）。求值在本侧完成后批量推给 core（core 侧零回调），enter 前推送会被
        /// 缓冲。与 ItemTemplate 同设时 selector 赢。已数据驱动时换 selector 立即重推：
        /// 模板变了的项由 core park 旧蓝图 slot、下帧以正确蓝图重新物化。
        /// </summary>
        public Func<int, UITemplate> TemplateSelector
        {
            get { ThrowIfDisposed(); return _templateSelector; }
            set
            {
                ThrowIfDisposed();
                _templateSelector = value;
                if (_firstItemCountSet && _itemCount > 0)
                    EvaluateAndPushTemplates(0, _itemCount);
            }
        }

        /// <summary>
        /// 求值 TemplateSelector 并把 [start, start+count) 的模板源批量推给 core
        /// （yio_list_set_item_templates）。selector 未设时 no-op。count=0 仍过 FFI
        /// 标记「选择已给出」（防多模板预检误判 -2）。模板源须为场景内子树
        /// （GetTemplate 模板 li / Instantiate 游离克隆）。
        /// </summary>
        void EvaluateAndPushTemplates(int start, int count)
        {
            var sel = _templateSelector;
            if (sel == null)
                return;
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            if (count <= 0)
            {
                // 空推送：只标记选择意图（多模板 + ItemCount=0 的首 set）。
                Native.yio_list_set_item_templates(h, _id, start, null, 0);
                return;
            }
            ulong[] ids = new ulong[count];
            for (int k = 0; k < count; k++)
            {
                UITemplate t = sel(start + k);
                if (t == null)
                    throw new UIContractException(
                        $"TemplateSelector returned null at index {start + k} (node {_id}): " +
                        "a set selector must answer every index (return the default UITemplate explicitly if intended)");
                if (!t.IsSceneSubtree)
                    throw new UIContractException(
                        $"TemplateSelector returned a package-component UITemplate at index {start + k} " +
                        $"(node {_id}): call Instantiate() first — cloning needs an in-scene subtree");
                ids[k] = t._srcNodeId;
            }
            fixed (ulong* p = ids)
            {
                int rc = Native.yio_list_set_item_templates(h, _id, start, p, count);
                if (rc != 0)
                    throw new UIContractException(
                        $"TemplateSelector push failed (node {_id}, range [{start}, {start + count})): " +
                        "stale template source (node was removed)");
            }
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
        /// ListView 虚拟化：Children 不可枚举（拿到的是随滚动变的可见 slot 子集，语义混乱易误用）。
        /// 操作项用 BindItem/ItemTemplate/ScrollToItem。对齐公共 API 契约（Container.Children 非虚，用 new 隐藏）。
        /// </summary>
        public new IReadOnlyList<Node> Children => throw new UIContractException("ListView 是虚拟化列表，Children 不可枚举——用 BindItem/ItemTemplate/ScrollToItem 操作项。");

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
        /// 滚动到指定 item。core 先设祖先 ScrollPane.scroll_pos 到目标偏移，再
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
            int rc = Native.yio_list_scroll_to(h, _id, i, behavior);
            if (rc != 0)
                throw new UIContractException(
                    $"ScrollToItem failed (node {_id}, index {i}): not a data-driven ListView");
            // core 已 drain_now（slot 克隆 + binds 入队）；此处取出 binds 绑定，同帧完成。
            _ctx.DrainPendingBinds();
        }

        /// <summary>刷新单个当前可见的 item（重新 BindItem）。不在可见区的静默跳过。</summary>
        public void RefreshItem(int i)
        {
            ThrowIfDisposed();
            if (i < 0 || i >= _itemCount)
                throw new UIContractException(
                    $"RefreshItem index {i} out of range [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_list_refresh(h, _id, i, 1);
            if (rc != 0)
                throw new UIContractException(
                    $"RefreshItem failed (node {_id}, index {i}): not a data-driven ListView");
            _ctx.DrainPendingBinds();
        }

        /// <summary>刷新全部当前可见 item（重新 BindItem）。count=ItemCount 覆盖全部可见 slot。</summary>
        public void RefreshItems()
        {
            ThrowIfDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_list_refresh(h, _id, 0, _itemCount);
            if (rc != 0)
                throw new UIContractException(
                    $"RefreshItems failed (node {_id}): not a data-driven ListView");
            _ctx.DrainPendingBinds();
        }

        /// <summary>
        /// 插入通知：在 <paramref name="i"/> 处插入 <paramref name="c"/> 项。
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
            int rc = Native.yio_list_notify(h, _id, (byte)0, i, c);
            if (rc != 0)
                throw new UIContractException(
                    $"NotifyInserted failed (node {_id}): not a data-driven ListView");
            _itemCount += c;
            // selector 按 index 选模板：插入移位后受影响区间重求值重推。
            EvaluateAndPushTemplates(i, _itemCount - i);
        }

        /// <summary>
        /// 删除通知：删 [i, i+c)。区间内已物化 slot 就地休眠（parked，留挂列表待复用）；区间后的 slot.item_index 前移。
        /// i/c 越界 → UIContractException。同步更新 _itemCount 缓存。
        /// </summary>
        public void NotifyRemoved(int i, int c = 1)
        {
            ThrowIfDisposed();
            if (i < 0 || c < 0 || i + c > _itemCount)
                throw new UIContractException(
                    $"NotifyRemoved range [{i}, {i + c}) out of bounds [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_list_notify(h, _id, (byte)1, i, c);
            if (rc != 0)
                throw new UIContractException(
                    $"NotifyRemoved failed (node {_id}): not a data-driven ListView");
            _itemCount -= c;
            // 删除移位后受影响区间重求值重推。
            EvaluateAndPushTemplates(i, _itemCount - i);
        }

        /// <summary>
        /// 移动通知：把 from 项搬到 to 位置。heights 同步搬；slot.item_index 重映射。
        /// from/to 越界 → UIContractException。
        /// </summary>
        public void NotifyMoved(int f, int t)
        {
            ThrowIfDisposed();
            if (f < 0 || f >= _itemCount || t < 0 || t >= _itemCount)
                throw new UIContractException(
                    $"NotifyMoved from {f} / to {t} out of range [0, {_itemCount})");
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            int rc = Native.yio_list_notify(h, _id, (byte)2, f, t);
            if (rc != 0)
                throw new UIContractException(
                    $"NotifyMoved failed (node {_id}): not a data-driven ListView");
            // 移动重排 [min(f,t), max(f,t)] 区间的 index→模板配对，从区间头重推后缀
            // （selector 任意函数，保守多推无损）。
            int lo = f < t ? f : t;
            EvaluateAndPushTemplates(lo, _itemCount - lo);
        }

        public string ItemExitClass { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // AnimationHandle 句柄非长期对象，生命周期 = 那次播放；播放结束句柄失效、hook 自动释放。
    //
    // 生命周期不变量：
    // - END 事件（demux 触发 onEnd 后）/ Stop()（scene 层终态）→ _disposed=true +
    //   UIContext 注销注册表条目；此后成员调用全部 no-op（不抛——契约为「调用 no-op」）。
    // - 循环动画（infinite）句柄存活到 Stop()。
    // - class 触发的动画无句柄，只走 EventBus 广播（On<AnimationEndEvent> 等）。
    //
    // 回调路由：OnStart/OnEnd/OnHook 纯 C#（core 本就 emit 事件，demux 按
    // playerKey 查本实例触发）；OnKey 半 FFI（cb 留本类，pct 经 animation_on_key 注册到 core
    // ——core 才知道检测哪些百分比跨越，注册须在 Play 之后、key 有效时）。
    //
    // 事件载荷解码（core event.rs payload 编码）：demux 把 EventRecord 的 touch_id(低 32)/x(高 32)
    // 拼回 PlayerKey u64，按 key 查 UIContext._animations 命中本实例。
    public sealed unsafe class AnimationHandle
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
        internal AnimationHandle(Node node, ulong playerKey, string name)
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
                    // 强引用悬挂（UIContext→AnimationHandle→Node→用户回调全链）。
                    Invalidate();
                    return false;
                }
                StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
                byte state = Native.yio_stage_get_animation_state(h, _playerKey);
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
        /// 时间轴位置（elapsed——含 delay 计时的唯一时间源头）。setter = seek：
        /// 下一帧按新位置采样。句柄失效后 get 返 0 / set no-op。
        /// </summary>
        public float Time
        {
            get
            {
                if (_disposed || _node._disposed) return 0f;
                StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
                return Native.yio_stage_get_animation_time(h, _playerKey);
            }
            set
            {
                if (_disposed || _node._disposed) return;
                StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
                Native.yio_stage_set_animation_time(h, _playerKey, value);
            }
        }

        /// <summary>暂停（Playing → Paused，位置冻结；可 Resume）。句柄失效后 no-op。</summary>
        public void Pause()
        {
            if (_disposed || _node._disposed) return;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.yio_stage_pause_animation(h, _playerKey);
        }

        /// <summary>恢复（Paused → Playing；Completed/Stopped 是终态不可恢复）。失效后 no-op。</summary>
        public void Resume()
        {
            if (_disposed || _node._disposed) return;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.yio_stage_resume_animation(h, _playerKey);
        }

        /// <summary>
        /// 停止（scene 层终态，不可恢复，勿当暂停）。core 下帧
        /// 回收 player（不发 END 事件），故本方法同步失效句柄 + 注销注册表。
        /// </summary>
        public void Stop()
        {
            if (_disposed || _node._disposed) return;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.yio_stage_stop_animation(h, _playerKey);
            Invalidate();
        }

        /// <summary>链式注册播放启动回调（START 事件按 playerKey 命中时触发）。</summary>
        public AnimationHandle OnStart(Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            (_onStart ??= new List<Action>()).Add(cb);
            return this;
        }

        /// <summary>链式注册播放完成回调（完成后句柄失效，onEnd 先触发再失效）。</summary>
        public AnimationHandle OnEnd(Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            (_onEnd ??= new List<Action>()).Add(cb);
            return this;
        }

        /// <summary>
        /// 链式注册百分比跨越回调（半 FFI：cb 留 C#，pct 注册进 core 检测阈值）。
        /// 须在 key 有效时调（Play 之后；链式 <c>Play(name).OnKey(.5, cb)</c> 是标准用法）。
        /// 同 pct 重复注册去重（core register_on_key 去重，cb 仍各存各发）。
        /// </summary>
        public AnimationHandle OnKey(float pct, Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            Native.yio_stage_animation_on_key(h, _playerKey, pct);
            var list = _onKeys ??= new List<(float, Action)>();
            // cb 不去重（同 pct 多 cb 各自触发）；pct 去重由 core 保证。
            list.Add((pct, cb));
            return this;
        }

        /// <summary>
        /// 链式注册 @yio-hook 锚点回调（纯 C#：core emit HOOK 带 hook_name，
        /// demux 按 name 匹配触发；无需 FFI 注册）。
        /// </summary>
        public AnimationHandle OnHook(string name, Action cb)
        {
            if (name == null) throw new ArgumentNullException(nameof(name));
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (_disposed || _node._disposed) return this;
            (_onHooks ??= new List<(string, Action)>()).Add((name, cb));
            return this;
        }

        // 回调是 Action（无事件参数），触发时只传载荷（pct / hook_name）。

        /// <summary>START 事件 → onStart 回调。</summary>
        internal void FireStart()
        {
            if (_onStart == null) return;
            var cbs = _onStart.ToArray();   // snapshot：回调内再注册不影响本次遍历
            for (int i = 0; i < cbs.Length; i++) cbs[i]();
        }

        /// <summary>END 事件 → onEnd 回调 + 句柄失效（播放结束句柄失效）。</summary>
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
        /// 幂等。此后成员调用 no-op（player 回收 → 句柄失效 → 调用 no-op）。
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

    // ClassList = Node 的 class 集合投影（Add/Remove/Contains/Toggle/Set/Replace）。
    //
    // 投影层契约（即时过桥）：class 是低频 UI 事件路径（非每帧热路径），每次操作
    // 直 FFI；无镜像需求——class 状态真相在 core，Contains 直查 has_class FFI（不缓存）。Add/Remove
    // 在 core 标 dirty_mesh 触发下帧 rematch，命中 .foo 规则的节点下帧 cascade
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

        // 同 StyleMirror：UTF-8 编码 + fixed 钉住 + ptr+len。
        //
        // disposed 防御：每个公共方法入口调 _owner.ThrowIfDisposed()——覆盖"业务 var cl = node.Classes;
        // node.Dispose(); cl.Add(...)"这条跨 Dispose 持引用路径（Node.Classes getter 的 ThrowIfDisposed
        // 只拦 getter 入口，不拦后捕获的 cl）。ClassList 是低频 UI 事件路径，多一次 _disposed 读可忽略。
        //
        // add_class/remove_class 失败静默（rc!=0 仅发生于 null stage / 节点不 live / 非 UTF-8——
        // 前两者 ThrowIfDisposed 已拦，UTF-8 编码不会产非 UTF-8；防御性不抛，与同 assembly 其他
        // FFI 转调一致）。
        // has_class 返 i32 三态：1=true / 0=false / -1=err——Contains 把 -1 升级为
        // InvalidOperationException（不静默吞：stale NodeId 是 use-after-dispose 信号，不能当"无此 class"）。

        void CallAdd(string name)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(name);
            fixed (byte* p = b)
                Native.yio_stage_add_class(h, _owner._id, p, (nuint)b.Length);
        }

        void CallRemove(string name)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(name);
            fixed (byte* p = b)
                Native.yio_stage_remove_class(h, _owner._id, p, (nuint)b.Length);
        }

        int CallHas(string name)
        {
            StageHandle* h = (StageHandle*)_owner._ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(name);
            fixed (byte* p = b)
                return Native.yio_stage_has_class(h, _owner._id, p, (nuint)b.Length);
        }
    }

    // StyleSheet 逃生舱（#11 运行时 CSS）：Add 返回 IDisposable 句柄，撤销靠 Dispose（不靠原文匹配）。
    // Add 的 CSS 文本由 FFI 层用 fence 解析（选择器/声明子集 + --* 自定义属性 + var()；at-rule 一律
    // 拒——@keyframes 走包内 CSS），解析失败抛 UIStyleException 带行列。注入规则是**全局规则**：
    // 与模板 CSS 同 cascade 优先级（同 specificity 后 Add 赢）、跨作用域命中——打包期组件内容墙
    // 不约束运行时注入（public-api §10.2）；下一帧全量 rematch 生效。值类型按 Length/YioColor/
    // float/string 四重载的 SetVar 见 Node（custom props 最高优先级层）。
    public unsafe class StyleSheet
    {
        // 投影层内部：持有上下文。lazy 造时由 UIContext.StyleSheet 传 this；方法体经它取 stage 转调 FFI。
        internal readonly UIContext _ctx;
        internal StyleSheet(UIContext ctx) { _ctx = ctx; }

        void ThrowIfCtxDisposed()
        {
            if (_ctx._stage == IntPtr.Zero)
                throw new ObjectDisposedException(nameof(UIContext));
        }

        /// <summary>
        /// 注入一段运行时 CSS 规则集（选择器 + 声明）。返回句柄，Dispose 撤销该批规则。
        /// 解析失败（at-rule / 越界选择器 / 未知属性 / var() 坏形状）抛 UIStyleException 带行列。
        /// </summary>
        public IDisposable Add(string css) { ThrowIfCtxDisposed(); return DoAdd(css); }

        /// <summary>清空全部运行时注入规则（包内模板 CSS 不受影响）。</summary>
        public void Clear()
        {
            ThrowIfCtxDisposed();
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            Native.yio_stage_style_sheet_clear(h);
        }

        RuleSetRegistration DoAdd(string css)
        {
            StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
            byte[] b = Encoding.UTF8.GetBytes(css ?? string.Empty);
            ulong setId = 0;
            int rc;
            fixed (byte* p = b)
                rc = Native.yio_stage_style_sheet_add(h, p, (nuint)b.Length, &setId);
            if (rc == 1)
            {
                uint line = 0, col = 0;
                byte* msgPtr = Native.yio_stage_style_sheet_last_error(h, &line, &col);
                string msg = ReadUtf8Nul(msgPtr);
                throw new UIStyleException(msg, line, col);
            }
            if (rc != 0)
                throw new InvalidOperationException("style_sheet_add FFI returned error (null stage / non-UTF-8).");
            return new RuleSetRegistration(_ctx, setId);
        }

        /// <summary>NUL 结尾 UTF-8 C 串 → string（Rust 拥有，立即消费）。</summary>
        static string ReadUtf8Nul(byte* p)
        {
            if (p == null) return string.Empty;
            int n = 0;
            while (p[n] != 0) n++;
            return n == 0 ? string.Empty : Encoding.UTF8.GetString(p, n);
        }

        /// <summary>Add 返回的撤销句柄：Dispose 调 style_sheet_remove。重复 Dispose no-op。</summary>
        sealed class RuleSetRegistration : IDisposable
        {
            UIContext _ctx;
            ulong _setId;
            internal RuleSetRegistration(UIContext ctx, ulong setId) { _ctx = ctx; _setId = setId; }
            public void Dispose()
            {
                if (_ctx == null) return;
                if (_ctx._stage != IntPtr.Zero)
                {
                    StageHandle* h = (StageHandle*)_ctx._stage.ToPointer();
                    Native.yio_stage_style_sheet_remove(h, _setId);
                }
                _ctx = null;
            }
        }
    }

    public sealed unsafe class UITemplate
    {
        // 投影层内部字段：持有上下文 + 包名 + 模板路径。
        // Name 返 _path（模板路径即名称）；Instantiate 经 _ctx FFI instantiate(_pkg, _path)。
        internal readonly UIContext _ctx;
        internal readonly string _pkg;
        internal readonly string _path;

        // SceneSubtree 变体标识：非 RootSentinel 时本模板表示「克隆场景内某个子树」
        // （非包组件）。供虚拟列表 slot 克隆路径用——
        // ListView ItemTemplate 可指向场景内已建子树，Instantiate 走 clone_subtree FFI
        // 而非包组件 instantiate FFI。两种变体共用同一个公共 API 表面（Name/Instantiate）。
        internal readonly ulong _srcNodeId = Node.RootSentinel;
        internal bool IsSceneSubtree => _srcNodeId != Node.RootSentinel;

        internal UITemplate(UIContext ctx, string pkg, string path)
        {
            _ctx = ctx; _pkg = pkg; _path = path;
        }

        // SceneSubtree 变体构造：克隆场景内 srcNodeId 子树。path/pkg 留空（不供人读，
        // Name 返空串——调用方按 IsSceneSubtree 区分两种变体）。
        internal UITemplate(UIContext ctx, ulong srcNodeId)
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
        internal static Container DoInstantiateSubtree(UIContext ctx, ulong srcNodeId)
        {
            StageHandle* h = (StageHandle*)ctx._stage.ToPointer();
            ulong rootId = Native.yio_stage_clone_subtree(h, srcNodeId);
            if (rootId == Node.RootSentinel)
                throw new UIPackageException(
                    "clone_subtree failed: invalid source node / no scene created");
            Container root = (Container)ctx._registry.GetOrCreate(rootId);
            // 同 DoInstantiate：eager 物化子树内注册组件（模板根自身已路由）。
            ctx.MaterializeCustomElements(rootId);
            return root;
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
            ulong rootId;
            fixed (byte* pp = pb)
            fixed (byte* cp = cb)
                rootId = Native.yio_stage_instantiate(h, pp, (nuint)pb.Length, cp, (nuint)cb.Length);
            if (rootId == Node.RootSentinel)
                throw new UIPackageException(
                    $"instantiate failed: pkg='{pkg}' comp='{path}' " +
                    "(package not loaded / component not found / no scene created)");
            Container root = (Container)ctx._registry.GetOrCreate(rootId);
            // eager 物化注册组件（RegisterComponent 契约：OnConnected 在实例化时跑，
            // 不等首次访问）——根自身已由上行 GetOrCreate 路由。
            ctx.MaterializeCustomElements(rootId);
            return root;
        }
    }

    // UIContext 是「获取而非创建」：无公共构造，由引擎集成层创建/驱动。业务程序员从集成层获取。
    /// OnUpdate 订阅句柄：Dispose 撤销单个订阅（不触其他订阅）。节点 Dispose 时其全部
    /// 订阅由 UIContext 联动清（公共契约：订阅随 Dispose 自动清理，RemoveFromParent 不清理）。
    internal sealed class UpdateSubscription : IDisposable
    {
        UIContext _ctx;
        ulong _nodeId;
        Action<float> _cb;
        bool _disposed;
        internal UpdateSubscription(UIContext ctx, ulong nodeId, Action<float> cb)
        {
            _ctx = ctx; _nodeId = nodeId; _cb = cb;
        }
        internal Action<float> Callback => _cb;
        internal bool IsDisposed => _disposed;
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _ctx?.RemoveUpdateHook(_nodeId, this);
        }
    }

    /// <summary>
    /// 链式 tween builder（<see cref="Node.Tween"/> 的返回形态）。消费型：每方法返自身，
    /// <see cref="Start"/> 提交（FFI spec-struct 单次调用）；OnComplete 走 TweenComplete
    /// 事件按 tag 路由（tag 未显式给时自动分配），完成即注销（重复播放需重挂）。
    /// 值语义与 CSS transition 相同通道互踩（replace-override：新 tween 覆写同通道）。
    /// </summary>
    public sealed unsafe class TweenBuilder
    {
        private readonly Node _node;
        private readonly float[] _start = new float[8];
        private readonly float[] _end = new float[8];
        private YioTweenSpec _spec;
        private Action<Node> _onComplete;
        // BoxShadow 通道列表载荷（其余通道 null）。
        private TweenShadow[] _shadowStart;
        private TweenShadow[] _shadowEnd;

        internal TweenBuilder(Node node, TweenChannel channel)
        {
            _node = node;
            _spec = new YioTweenSpec
            {
                prop = (uint)channel,
                // CSS 缺省 timing = ease（精确 bezier(.25,.1,.25,1)，与 CSS/fence 侧同一真值）
                ease_kind = (uint)EaseKind.CubicBezier,
                duration = 0.3f,
                yoyo = 0,
            };
            unsafe
            {
                _spec.ease_params[0] = 0.25f;
                _spec.ease_params[1] = 0.1f;
                _spec.ease_params[2] = 0.25f;
                _spec.ease_params[3] = 1.0f;
            }
        }

        /// <summary>起始值（分量数按通道，见 <see cref="Node.Tween"/>）。Width/Height 载荷
        /// = [value, domainCode]（<see cref="LenDomain"/>）——便捷形态用 <see cref="FromPx(float)"/>
        /// 等单参方法。BoxShadow 通道不用本方法（走 <see cref="FromShadow"/>）。</summary>
        public TweenBuilder From(params float[] values)
        {
            CopyValues(_start, values);
            return this;
        }

        /// <summary>目标值（分量数按通道）。同 <see cref="From"/> 约定。</summary>
        public TweenBuilder To(params float[] values)
        {
            CopyValues(_end, values);
            return this;
        }

        // Width/Height 便捷形态：单值 + 域（默认 px）。双端域必须一致——FFI 侧拒收
        // 异域提交（C# 这里不重复校验， Start 静默 no-op 由 core 防）。
        public TweenBuilder FromPx(float v) => From(v, (float)LenDomain.Px);
        public TweenBuilder ToPx(float v) => To(v, (float)LenDomain.Px);
        public TweenBuilder FromPct(float v) => From(v, (float)LenDomain.Pct);
        public TweenBuilder ToPct(float v) => To(v, (float)LenDomain.Pct);
        public TweenBuilder FromVw(float v) => From(v, (float)LenDomain.Vw);
        public TweenBuilder ToVw(float v) => To(v, (float)LenDomain.Vw);

        /// <summary>box-shadow 起始列表（空数组 = box-shadow:none 端点）。</summary>
        public TweenBuilder FromShadow(params TweenShadow[] shadows)
        {
            _shadowStart = shadows ?? Array.Empty<TweenShadow>();
            return this;
        }

        /// <summary>box-shadow 目标列表（空数组 = 动画到无阴影）。</summary>
        public TweenBuilder ToShadow(params TweenShadow[] shadows)
        {
            _shadowEnd = shadows ?? Array.Empty<TweenShadow>();
            return this;
        }

        public TweenBuilder Duration(float seconds)
        {
            _spec.duration = seconds;
            return this;
        }

        public TweenBuilder Delay(float seconds)
        {
            _spec.delay = seconds;
            return this;
        }

        /// <summary>keyword 缓动（CubicBezier kind 之外用；精确 CSS ease 曲线用
        /// <see cref="EaseBezier"/>）。</summary>
        public TweenBuilder Ease(EaseKind kind)
        {
            _spec.ease_kind = (uint)kind;
            return this;
        }

        /// <summary>cubic-bezier(x1,y1,x2,y2)：x∈[0,1]（越界按缺省拒——Start 抛契约异常）。
        /// CSS 标准 keyword 的精确曲线：ease=(.25,.1,.25,1) / ease-in=(.42,0,1,1) /
        /// ease-out=(0,0,.58,1) / ease-in-out=(.42,0,.58,1)。</summary>
        public TweenBuilder EaseBezier(float x1, float y1, float x2, float y2)
        {
            _spec.ease_kind = (uint)EaseKind.CubicBezier;
            unsafe
            {
                _spec.ease_params[0] = x1;
                _spec.ease_params[1] = y1;
                _spec.ease_params[2] = x2;
                _spec.ease_params[3] = y2;
            }
            return this;
        }

        /// <summary>extraRepeats = 额外重播次数（0=单次）；yoyo = 奇数轮反向（CSS alternate）。</summary>
        public TweenBuilder Repeat(uint extraRepeats, bool yoyo)
        {
            _spec.repeat = extraRepeats;
            _spec.yoyo = yoyo ? (byte)1 : (byte)0;
            return this;
        }

        /// <summary>complete 事件载荷（OnComplete 路由键；同 tag 后注册者胜）。</summary>
        public TweenBuilder Tag(uint tag)
        {
            _spec.tag = tag;
            return this;
        }

        /// <summary>完成回调（TweenComplete 事件驱动，帧头泵触发）。tag 未显式给时自动分配。
        /// 一次性：完成即注销——Repeat 只在全部轮次跑满后触发一次。</summary>
        public TweenBuilder OnComplete(Action<Node> onComplete)
        {
            _onComplete = onComplete;
            return this;
        }

        /// <summary>提交（经 FFI spec-struct 注册进 TweenManager，本帧起生效）。
        /// bezier x 越界抛 <see cref="UIContractException"/>（FFI 侧静默 no-op，这里前置拦）；
        /// BoxShadow 通道缺 FromShadow/ToShadow 同抛契约异常；Width/Height 异域提交
        /// 同抛（FFI 防御拒收在 core）。</summary>
        public void Start()
        {
            _node.ThrowIfDisposed();
            if (_onComplete != null)
            {
                if (_spec.tag == 0)
                    _spec.tag = _node._ctx.AllocTweenTag();
                _node._ctx.RegisterTweenComplete(_spec.tag, _onComplete);
            }
            if (_spec.ease_kind == (uint)EaseKind.CubicBezier)
            {
                float x1, x2;
                unsafe { x1 = _spec.ease_params[0]; x2 = _spec.ease_params[2]; }
                if (x1 < 0f || x1 > 1f || x2 < 0f || x2 > 1f)
                    throw new UIContractException(
                        $"EaseBezier x1/x2 must be in [0,1] (got x1={x1}, x2={x2})");
            }
            StageHandle* h = (StageHandle*)_node._ctx._stage.ToPointer();
            if (_spec.prop == (uint)TweenChannel.BoxShadow)
            {
                if (_shadowStart == null || _shadowEnd == null)
                    throw new UIContractException(
                        "BoxShadow tween requires FromShadow(...) and ToShadow(...) list endpoints");
                if (_shadowStart.Length > 12 || _shadowEnd.Length > 12)
                    throw new UIContractException(
                        $"BoxShadow tween supports at most 12 layers per endpoint (got {_shadowStart.Length}/{_shadowEnd.Length})");
                float[] sp = PackShadows(_shadowStart);
                float[] ep = PackShadows(_shadowEnd);
                YioTweenSpec spec = _spec;
                fixed (float* sPtr = sp)
                fixed (float* ePtr = ep)
                {
                    Native.yio_stage_tween_shadow(h, _node._id, &spec,
                        sPtr, (uint)_shadowStart.Length, ePtr, (uint)_shadowEnd.Length);
                }
                return;
            }
            if ((_spec.prop == (uint)TweenChannel.Width || _spec.prop == (uint)TweenChannel.Height)
                && _start[1] != _end[1])
            {
                throw new UIContractException(
                    $"Width/Height tween endpoints must share one length domain (start domain code {_start[1]}, end {_end[1]}) — px↔px / %↔% / vw↔vw");
            }
            fixed (float* sp = _start)
            fixed (float* ep = _end)
            {
                YioTweenSpec spec = _spec;
                Native.yio_stage_tween_spec(h, _node._id, &spec, sp, ep);
            }
        }

        /// 每层 9 float：[ox, oy, spread, blur, r, g, b, a, inset?]（core FFI 载荷契约）。
        private static float[] PackShadows(TweenShadow[] shadows)
        {
            float[] buf = new float[shadows.Length * 9];
            for (int i = 0; i < shadows.Length; i++)
            {
                var s = shadows[i];
                int b = i * 9;
                buf[b] = s.OffsetX; buf[b + 1] = s.OffsetY;
                buf[b + 2] = s.Spread; buf[b + 3] = s.Blur;
                buf[b + 4] = s.R; buf[b + 5] = s.G; buf[b + 6] = s.B; buf[b + 7] = s.A;
                buf[b + 8] = s.Inset ? 1f : 0f;
            }
            return buf;
        }

        private static void CopyValues(float[] dst, float[] src)
        {
            if (src == null) throw new ArgumentNullException(nameof(src));
            int n = Math.Min(src.Length, 8);
            for (int i = 0; i < n; i++) dst[i] = src[i];
            for (int i = n; i < 8; i++) dst[i] = 0f;
        }
    }

    public sealed unsafe class UIContext
    {
        // headless harness / 引擎集成层建 UIContext 时持有的 Stage 句柄（raw FFI handle）。
        // 投影层通过它转调 yio_stage_* FFI；公共 API 表面看不到本字段。
        internal IntPtr _stage;

        // NodeId → typed Node 的强引用身份缓存。
        // NodeFactory 造节点入缓存；Node.Dispose 时 evict。公共 API 不见本字段。
        internal readonly NodeRegistry _registry;

        // typed 事件订阅表 + capture/bubble/once 路由。Node.On<T> 经此转调 Subscribe<T>；
        // EventDemuxer 翻译 raw YioEvent → typed struct 后调 Dispatch<T>。公共 API 不见本字段。
        internal readonly EventBus _eventBus;

        // raw YioEvent stream → typed event struct demux。YioHost.Step 调 Pump 每帧
        // 翻译 borrow_events buffer → EventBus.Dispatch。公共 API 不见本字段。
        internal readonly EventDemuxer _eventDemuxer;

        // create_root FFI 返回的根 NodeId。由 harness/集成层调 create_root 后写入本字段；
        // Root getter 据此返回 typed Container。无公共 FFI 直接读 roots[0]——Rust 侧 roots Vec
        // 未暴露 getter，故投影层需自己跟踪。
        internal ulong _rootId = Node.RootSentinel;

        // 已加载包名集合（load_package 时加入，unload_package 时移除）。
        // 用于同名重复检测（公共契约：LoadPackage 同名重复抛 UIContractException）。
        internal readonly HashSet<string> _loadedPackages = new HashSet<string>();

        // ListView NodeId → C# 实例表。ListView 设 ItemCount/BindItem 时 RegisterListView 进本表；
        // tick-drain 取 pending_binds 后按 slot 的 NodeId 向上走 node_parent，命中本表即找到
        // 所属 ListView 实例、调其 BindItem。公共 API 不见本字段。
        internal readonly Dictionary<ulong, ListView> _listViews = new Dictionary<ulong, ListView>();

        // #9 tween builder OnComplete 路由表：tag → 回调（TweenComplete 事件 touch_id 槽
        // 装 tag；完成即注销——一次性语义）。tag 0 保留 = 无回调（旧 transition 路径）。
        internal readonly Dictionary<uint, Action<Node>> _tweenCompleteCallbacks = new Dictionary<uint, Action<Node>>();
        internal uint _nextTweenTag = 1;

        // PlayerKey → AnimationHandle 实例注册表（demux 句柄路由查用）。
        // 强引用：句柄生命周期 = 那次播放（END/Stop 时 AnimationHandle.Invalidate 注销）。
        // 循环动画存活到 Stop——用户持有句柄期间注册表保留引用，结束自动释放。
        // player 被 core 静默回收（节点销毁）的悬挂条目由 IsPlaying 惰性失效清理。
        internal readonly Dictionary<ulong, AnimationHandle> _animations = new Dictionary<ulong, AnimationHandle>();

        // lazy 创建的 StyleSheet 实例。同 Node.Style/Node.Transform 模式——未访问过 = null，
        // 首次访问构造并挂本 context（#11 已接线 FFI）。
        StyleSheet _styleSheet;

        // 组件类绑定注册表（RegisterComponent）：custom tag → wrapper 工厂。显式委托
        // 构造零反射（IL2CPP/AOT 安全——反射构造泛型实例是经典 AOT 雷）。注册只影响
        // 未来构造（NodeFactory 路由查表）；已构造 wrapper 不追改（身份缓存不可破坏）
        // ——注册时序约定 setup 期（instantiate 前）。公共 API 不见本字段。
        internal readonly Dictionary<string, Func<UIContext, ulong, CustomElement>> _componentFactories = new();

        // 回调是 C# 闭包，core 的 C ABI 存不了——调度器整体住在投影层，PumpLogic 由
        // YioHost.Step 帧头泵（CollectInput 后、FlushPendingWrites 前）：回调内改
        // Style/数据经既有 flush seam 过桥，本帧 solve 生效（零延迟语义）。
        // 计时与 Step 同一 dt 累积（同源不双钟；TweenManager 单一动画时钟不受影响）。
        // headless 测试在 tick 前手动调 PumpLogic（同 FlushPendingWrites 模式）。
        internal readonly Dictionary<ulong, List<UpdateSubscription>> _updateHooks = new();
        internal readonly List<(float Due, Action Cb)> _timers = new();
        internal readonly Queue<Action> _nextFrame = new();
        // tick 后队列（CallAfterLayout）：YioHost.Step 在 stage tick 之后泵——新挂载
        // 子树当帧 solve 已完成，回调里读 Geometry 拿到实测值（CallNextFrame 帧头 fire
        // 先于 solve，新子树首读必全零）。
        internal readonly Queue<Action> _afterLayout = new();

        // headless harness 工厂构造。public API 无构造（业务从集成层拿现成 instance）。
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
        /// YioHost.Step 在 tick 前调（flush→solve 序）；headless 测试在 raw tick 前调。
        /// 攒批契约：setter 只标脏不立即过桥，本方法集中过桥，避免每 setter 一次 FFI。
        /// </summary>
        internal void FlushPendingWrites()
        {
            _registry.FlushDirtyStyles();
            _registry.FlushDirtyTransforms();
        }

        /// <summary>
        /// 注册组件类绑定：custom tag（hyphen 标签，如 "my-widget"）→ wrapper 工厂。
        /// 此后实例化/物化到该 tag 时，工厂构造用户派生的 CustomElement 子类（派生
        /// ctor 完整跑完后回调 <see cref="CustomElement.OnConnected"/>——组件行为接线
        /// 进 typed 子类，替代 wrapper div + TryGet 绕法）。fgui extensionCreator 等价。
        ///
        /// 工厂是显式委托（AOT 零反射）：<c>(c, id) => new MyWidget(c, id)</c>；
        /// MyWidget 链 protected internal 基类构造。重复注册同 tag / null/空 tag /
        /// null 工厂 → <see cref="UIContractException"/>（fail loud——静默覆盖藏接线错）。
        /// 注册只影响未来构造的 wrapper：已构造实例不追改（身份缓存不可破坏），约定
        /// 在 setup 期（instantiate 前）注册。
        /// </summary>
        public void RegisterComponent(string tag, Func<UIContext, ulong, CustomElement> factory)
        {
            if (string.IsNullOrEmpty(tag))
                throw new UIContractException("RegisterComponent: tag must be non-empty");
            if (factory == null)
                throw new UIContractException($"RegisterComponent('{tag}'): factory must be non-null");
            if (_componentFactories.ContainsKey(tag))
                throw new UIContractException(
                    $"RegisterComponent: tag '{tag}' already registered (re-register hides wiring bugs; fail loud)");
            _componentFactories[tag] = factory;
        }

        /// <summary>
        /// 节点死亡帧泵：取走 core 死亡通知队列（任何删除路径——外部 remove_node /
        /// list 槽位换绑淘汰克隆 / 内部剪枝），evict 对应 C# wrapper 并对组件派生类
        /// 回调 <see cref="CustomElement.OnDisconnected"/>。顺序 = 释放顺序（叶先于
        /// 祖先）。YioHost.Step 帧头调（CollectInput 后、PumpLogic 前——已断开组件
        /// 本帧不再跑 OnUpdate）；headless 测试手动调（同 PumpLogic 模式）。
        ///
        /// 无缓存 wrapper 的死亡静默跳过：用户 C# Dispose 已同步回调过（双重通知的
        /// 天然去重），list 换绑 churn 的大批无 wrapper 克隆 id 同理。非组件 wrapper
        /// 顺带 evict + 标 _disposed——死亡变显式（后续读抛 ObjectDisposedException，
        /// 不再是死 id 静默打 FFI）。
        /// </summary>
        public void PumpRemovedNodes()
        {
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            nuint len;
            ulong* p = Native.yio_stage_drain_removed_nodes(h, &len);
            if (p == null) return;
            for (nuint i = 0; i < len; i++)
                _registry.Remove(p[i]);
        }

        /// <summary>
        /// DFS 子树物化 CustomElement wrapper（eager 构造，RegisterComponent 契约的
        /// 「实例化时」半边）：非组件节点不物化（保持懒物化内存画像——每节点一次
        /// get_node_kind 查询是 instantiate 期一次性成本，非每帧）。instantiate 根
        /// 自身由调用方 GetOrCreate 路由。嵌套组件一并物化（递归全深）。
        /// </summary>
        internal void MaterializeCustomElements(ulong rootId)
        {
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            int count = Native.yio_stage_get_child_count(h, rootId);
            if (count <= 0) return;
            ulong[] buf = new ulong[count];
            int written;
            fixed (ulong* bp = buf)
                written = Native.yio_stage_get_children(h, rootId, bp, (nuint)buf.Length);
            if (written < 0) return; // 节点刚被并发移除（理论单线程不达），防御早退
            if (written > buf.Length) written = buf.Length;

            for (int i = 0; i < written; i++)
            {
                ulong childId = buf[i];
                byte kind = 0xFF;
                if (Native.yio_stage_get_node_kind(h, childId, &kind) == 0
                    && (NodeKind)kind == NodeKind.CustomElement)
                {
                    // GetOrCreate → NodeFactory → 注册表路由 → 派生类构造 + OnConnected
                    _registry.GetOrCreate(childId);
                }
                MaterializeCustomElements(childId);
            }
        }


        /// <summary>
        /// 帧头逻辑泵：排空上帧入队的 next-frame 回调 → 逐节点 OnUpdate(dt) → 到期 timer。
        /// YioHost.Step 在 CollectInput 后、FlushPendingWrites 前调——回调内改 Style 走
        /// 既有 flush seam，本帧 solve 生效。单回调抛异常被 catch + Debug 诊断，不阻断
        /// 其他回调与后续帧（同 DrainPendingBinds 对 BindItem 的隔离策略）。回调内再注册
        /// 的 hook/timer/next-frame 下次泵起效（快照遍历，不炸正在进行的遍历）。
        /// </summary>
        internal void PumpLogic(float dt)
        {
            if (_nextFrame.Count > 0)
            {
                Action[] due = _nextFrame.ToArray();
                _nextFrame.Clear();
                foreach (var cb in due) InvokeLogicGuarded(cb);
            }
            if (_updateHooks.Count > 0)
            {
                // 快照容量 = 订阅总数（节点数 × 各自 list 长度之和），非节点数。
                var snapshot = new List<UpdateSubscription>();
                foreach (var list in _updateHooks.Values)
                    snapshot.AddRange(list);
                for (int i = 0; i < snapshot.Count; i++)
                {
                    var sub = snapshot[i];
                    // 先前回调里 Dispose 掉的订阅跳过（快照定格早于本行执行）。
                    if (sub.IsDisposed) continue;
                    InvokeLogicGuarded(() => sub.Callback(dt));
                }
            }
            if (_timers.Count > 0)
            {
                // 就地递减；到期的收集后在注册序 fire（倒序移除保序）。本轮执行中新增的
                // timer 不参与本轮（fire 列表在递减阶段定格）。
                List<Action> fired = null;
                for (int i = _timers.Count - 1; i >= 0; i--)
                {
                    var t = _timers[i];
                    if (t.Due - dt <= 0f)
                    {
                        _timers.RemoveAt(i);
                        (fired ??= new List<Action>()).Add(t.Cb);
                    }
                    else
                    {
                        _timers[i] = (t.Due - dt, t.Cb);
                    }
                }
                if (fired != null)
                    for (int i = fired.Count - 1; i >= 0; i--)
                        InvokeLogicGuarded(fired[i]);
            }
        }

        void InvokeLogicGuarded(Action cb)
        {
            try { cb(); }
            catch (Exception ex)
            {
                // 业务回调抛不阻断调度器（上层应自己捕获）；静默吞错会让坏回调不可见，
                // 留诊断痕迹到 Debug 输出 / Unity player log。
                System.Diagnostics.Debug.WriteLine($"[Yio] logic callback threw: {ex}");
            }
        }

        /// <summary>
        /// tick 后泵：排空 after-layout 回调。YioHost.Step 在 stage tick 之后调——
        /// 本帧（含刚 Instantiate 的新子树）的 solve/world 已完成，回调里读 Geometry
        /// 是实测值。headless 测试在 raw tick 后手动调（同 PumpLogic 模式）。
        /// 回调内改 Style 落 mirror dirty，下帧 flush seam 过桥 + solve 生效。
        /// </summary>
        internal void PumpAfterLayout()
        {
            if (_afterLayout.Count == 0) return;
            Action[] due = _afterLayout.ToArray();
            _afterLayout.Clear();
            foreach (var cb in due) InvokeLogicGuarded(cb);
        }

        /// <summary>注册 per-node 每帧回调（Node.OnUpdate 调）。异常隔离见 PumpLogic。</summary>
        internal UpdateSubscription RegisterUpdateHook(ulong nodeId, Action<float> cb)
        {
            if (!_updateHooks.TryGetValue(nodeId, out var list))
            {
                list = new List<UpdateSubscription>();
                _updateHooks[nodeId] = list;
            }
            var sub = new UpdateSubscription(this, nodeId, cb);
            list.Add(sub);
            return sub;
        }

        /// <summary>撤销单个订阅（UpdateSubscription.Dispose 调）。</summary>
        internal void RemoveUpdateHook(ulong nodeId, UpdateSubscription sub)
        {
            if (_updateHooks.TryGetValue(nodeId, out var list))
            {
                list.Remove(sub);
                if (list.Count == 0) _updateHooks.Remove(nodeId);
            }
        }

        /// <summary>清节点的全部订阅（Node.Dispose 联动调——契约：订阅随 Dispose 自动清理）。</summary>
        internal void RemoveUpdateHooks(ulong nodeId) => _updateHooks.Remove(nodeId);

        // ListView.ItemCount/BindItem setter 调 RegisterListView 进本表；DrainPendingBinds
        // 在 tick 前（raw tick 前或集成层 Step 开头）调一次：拉 core pending_binds 队列、
        // 按 slot NodeId 反查所属 ListView、构 ListItem 调 BindItem。core 不存业务回调——
        // 本路径是 C# 业务状态与 core 虚拟化内核的唯一结合点。

        /// <summary>注册 ListView 实例（ItemCount/BindItem setter 调）。幂等。</summary>
        internal void RegisterListView(ListView lv) => _listViews[lv._id] = lv;
        /// <summary>该 NodeId 是否已注册为 ListView（数据驱动模式已激活）。</summary>
        internal bool IsListViewRegistered(ulong id) => _listViews.ContainsKey(id);

        /// <summary>注册 AnimationHandle 句柄（Node.Play 成功后调；demux 按 playerKey 路由）。</summary>
        internal void RegisterAnimation(AnimationHandle a) => _animations[a._playerKey] = a;
        /// <summary>按 playerKey 查 AnimationHandle 实例（demux 句柄路由；未命中 = class 触发/已失效 → null）。</summary>
        internal AnimationHandle ResolveAnimation(ulong playerKey) =>
            _animations.TryGetValue(playerKey, out var a) ? a : null;
        /// <summary>注销 AnimationHandle（END / Stop / 惰性失效时调）。幂等。</summary>
        internal void UnregisterAnimation(ulong playerKey) => _animations.Remove(playerKey);

        /// <summary>分配 tween tag（OnComplete 未显式 Tag 时自动取；单调递增，0 保留）。</summary>
        internal uint AllocTweenTag() => _nextTweenTag++;
        /// <summary>注册 tween 完成回调（builder OnComplete → Start 时调；同 tag 后注册者胜）。</summary>
        internal void RegisterTweenComplete(uint tag, Action<Node> cb) => _tweenCompleteCallbacks[tag] = cb;
        /// <summary>Demuxer TweenComplete 分支调：命中即触发并注销（一次性）。未注册 no-op。</summary>
        internal void FireTweenComplete(uint tag, ulong nodeId)
        {
            if (tag != 0 && _tweenCompleteCallbacks.TryGetValue(tag, out var cb))
            {
                _tweenCompleteCallbacks.Remove(tag);
                cb(_registry.GetOrCreate(nodeId));
            }
        }

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
            ulong* nodes = stackalloc ulong[Cap];
            int* indices = stackalloc int[Cap];
            uint len = 0;
            int rc = Native.yio_list_take_pending_binds(h, nodes, indices, Cap, &len);
            if (rc != 0) return;
            for (int i = 0; i < len; i++)
            {
                ulong slotNode = nodes[i];
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
                        $"[Yio] ListView BindItem threw for item {itemIndex} (slot node {slotNode}): {ex}");
                }
            }
        }

        /// <summary>
        /// 从 slotNode 向上走 node_parent，找到首个命中 _listViews 的祖先 ListView。
        /// 未找到（slot 已脱离树 / ListView 未注册）返 null。防环：限 10 万层（远超任何合法树深）。
        /// </summary>
        ListView FindListViewAncestor(StageHandle* h, ulong slotNode)
        {
            ulong cur = slotNode;
            for (int i = 0; i < 100_000; i++)
            {
                if (cur == Node.RootSentinel) return null;
                if (_listViews.TryGetValue(cur, out var lv)) return lv;
                cur = Native.yio_node_parent(h, cur);
            }
            return null;
        }

        /// <summary>
        /// 场景根节点（Container）。create_root FFI 建根后由 harness/集成层写入 _rootId；
        /// 若 _rootId 尚未设置（根未建），getter 读不到合法值——_rootId 仍是 RootSentinel
        /// （RootSentinel = ulong.MaxValue），registry.GetOrCreate 会产无意义的 wrapper。调用方需确保 create_root
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
        /// 当前焦点节点。FFI yio_stage_focused_node 返 NodeId（无焦点 → sentinel）。
        /// 返 null 当无焦点（DOM document.activeElement 为 body 的习惯：Yio 返 null 而非抛异常）。
        /// </summary>
        public Node FocusedNode
        {
            get
            {
                if (_stage == IntPtr.Zero) return null;
                StageHandle* h = (StageHandle*)_stage.ToPointer();
                ulong id = Native.yio_stage_focused_node(h);
                if (id == Node.RootSentinel) return null;
                return _registry.GetOrCreate(id);
            }
        }

        /// <summary>
        /// 样式逃生舱（动态 CSS 规则注入，#11）。lazy 造单一实例：同一 UIContext 多次访问返同一 StyleSheet。
        /// Add(string css) 返回 IDisposable 句柄，撤销靠 Dispose（不靠原文匹配）；解析失败抛 UIStyleException 带行列。
        /// </summary>
        public StyleSheet StyleSheet
        {
            get
            {
                _styleSheet ??= new StyleSheet(this);
                return _styleSheet;
            }
        }

        /// <summary>
        /// 无节点纯文本测量：字符串 + 字体 + 字号 → 宽高 + 行数（布局前预估——tips
        /// 预分行 / 飘字宽估 / 按钮自适应宽，消灭业务侧手数字数）。断行与 solve 内
        /// 文本测量同一条代码，预估即所见。maxWidth <= 0 不换行；&gt; 0 按该宽断行。
        /// family 未注册抛 UIContractException：测量必须用将渲染的同款字体，静默
        /// fallback 到默认字体会给出误导性的宽度。
        /// </summary>
        public TextMetrics MeasureText(string text, string fontFamily, float sizePx, float maxWidth = 0f)
        {
            if (string.IsNullOrEmpty(fontFamily))
                throw new ArgumentNullException(nameof(fontFamily));
            text ??= "";
            if (float.IsNaN(sizePx) || sizePx <= 0f)
                throw new UIContractException($"MeasureText: invalid font size {sizePx}");
            if (float.IsNaN(maxWidth))
                throw new UIContractException($"MeasureText: invalid maxWidth {maxWidth}");

            StageHandle* h = (StageHandle*)_stage.ToPointer();
            byte[] tb = Encoding.UTF8.GetBytes(text);
            byte[] fb = Encoding.UTF8.GetBytes(fontFamily);
            float w = 0f, ht = 0f;
            uint lines = 0;
            int rc;
            fixed (byte* tp = tb)
            fixed (byte* fp = fb)
                rc = Native.yio_stage_measure_text(
                    h, tp, (nuint)tb.Length, fp, (nuint)fb.Length, sizePx, maxWidth, &w, &ht, &lines);
            switch (rc)
            {
                case 0:
                    return new TextMetrics(w, ht, lines);
                case -2:
                    throw new UIContractException(
                        $"MeasureText: family '{fontFamily}' is not registered. Register it with " +
                        "YioHost.RegisterFont (or the runtime manifest) first — measure must use " +
                        "the same font that will render the text.");
                default:
                    throw new UIContractException("MeasureText failed (invalid arguments)");
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
                rc = Native.yio_stage_load_package(h, np, (nuint)nb.Length, bp, (nuint)bytes.Length);
            if (rc != 0)
            {
                // 版本错配（rc 1/2）专属文案：Unity 包与 yio.exe CLI 必须同版本同刷——
                // 只升一侧时旧 CLI 打的 pkg 被新运行时拒载（或反之），报错须点破修法。
                if (rc == 1 || rc == 2)
                {
                    uint pkgVer = Native.yio_stage_last_pkg_load_version(h);
                    uint runtimeVer = Native.yio_pkg_format_version();
                    string dir = rc == 1 ? "older" : "newer";
                    throw new UIPackageException(
                        $"load_package '{name}' failed: pkg format v{pkgVer} is {dir} than this " +
                        $"runtime's v{runtimeVer}. The Unity package and the yio.exe CLI must be " +
                        $"upgraded together — re-run `yio build` with the yio.exe shipped with this " +
                        $"package version (check `.yio/yio.exe --version`).");
                }
                throw new UIPackageException(
                    $"load_package '{name}' failed (malformed pkg.bin / duplicate pkg id / missing resources)");
            }

            _loadedPackages.Add(name);
            return new UIPackage(this, name);
        }

        /// <summary>
        /// 卸载包：从 Rust stage 移除模板注册（Unity prefab 删除语义——已实例化的活节点是
        /// 独立副本不受影响；持有旧 UITemplate 再 Instantiate 抛 UIPackageException）。
        /// 卸载后可重新 LoadPackage 同名包。
        ///
        /// 只动模板注册表，不触碰 atlas 纹理与字体：atlas 是 workspace 级共享资源
        /// （runtime.json 的 atlases 列表跨包并行、SpriteResolver 全局懒缓存，与包注册表
        /// 解耦——重载同名包不重载纹理），字体是 driver 级 RegisterFont 注册、不隶属任何包。
        /// 未加载的包名抛 UIContractException（与 LoadPackage 同名重复抛异常对称，不静默）。
        /// </summary>
        public void UnloadPackage(string name)
        {
            if (string.IsNullOrEmpty(name))
                throw new ArgumentNullException(nameof(name));
            if (!_loadedPackages.Contains(name))
                throw new UIContractException($"package '{name}' is not loaded. Load it first.");

            StageHandle* h = (StageHandle*)_stage.ToPointer();
            byte[] nb = Encoding.UTF8.GetBytes(name);
            int rc;
            fixed (byte* np = nb)
                rc = Native.yio_stage_unload_package(h, np, (nuint)nb.Length);
            if (rc != 0)
                throw new UIPackageException($"unload_package '{name}' failed (stage null / non-UTF-8)");
            _loadedPackages.Remove(name);
        }

        /// <summary>
        /// 建类型化节点（不挂父）。白名单：Container, AbsolutePanel, TextNode, Image。
        /// 非法 T（Button / Slider / Toggle / ListView 等控件或作用域根）
        /// 抛 UIContractException——控件只能 Instantiate（含内建子树），不能裸建。
        ///
        /// tag 映射（对齐 core dynamic.rs::kind_from_tag）：
        /// Container/AbsolutePanel → "div", TextNode → "span", Image → "img"。
        /// Button 虽在 kind_from_tag 白名单但不列入 Create<T>——Button 带内建子树，
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
            ulong id;
            fixed (byte* tp = tb)
                id = Native.yio_stage_create_node(h, tp, (nuint)tb.Length, null, 0);
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
        /// 命中测试：返回 globalPoint 处最上层可 Touchable 节点；未命中返回 null。
        /// 直转 yio_stage_hit_test（core hit::hit_test，上帧 world_transforms；结构
        /// 变更帧的新节点本帧未命中，1 帧延迟语义）。scrollbar thumb 命中 → 容器节点
        /// （FFI 侧 decode sentinel flag；公共树无 thumb 节点）。拖放 drop target 查找靠它。
        /// </summary>
        public Node Pick(YioVector2 globalPoint)
        {
            StageHandle* h = (StageHandle*)_stage.ToPointer();
            ulong id = 0;
            int rc = Native.yio_stage_hit_test(h, globalPoint.X, globalPoint.Y, &id);
            if (rc == 1) return null;
            if (rc != 0) throw new InvalidOperationException($"hit_test failed rc={rc} at ({globalPoint.X},{globalPoint.Y})");
            return _registry.GetOrCreate(id);
        }

        /// <summary>
        /// 延迟回调（秒）。同 DOM setTimeout——d 秒后调 cb（不精确，帧级粒度）。
        /// d≤0 视为下一帧（同 setTimeout(0) 宏任务语义，CallNextFrame 队列）。
        /// 计时由 PumpLogic 用 Step 同一 dt 累积。one-shot；cb 抛异常被隔离（Debug 诊断）。
        /// </summary>
        public void CallLater(float d, Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            if (d <= 0f) { _nextFrame.Enqueue(cb); return; }
            _timers.Add((d, cb));
        }

        /// <summary>
        /// 下帧回调（one-shot）。下一次 PumpLogic 开头 fire（帧头语义）——回调内改 Style 走
        /// 当帧 flush seam，当帧 solve 生效。cb 抛异常被隔离（Debug 诊断）。
        /// 注意：帧头 fire 先于 solve——刚 Instantiate 的子树在本回调里读 Geometry 仍是
        /// 全零（首帧尚未解算）；要「挂载后拿实测几何」用 <see cref="CallAfterLayout"/>。
        /// </summary>
        public void CallNextFrame(Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            _nextFrame.Enqueue(cb);
        }

        /// <summary>
        /// 布局后回调（one-shot）。下一次 stage tick 之后 fire（本帧 solve/world 已完成）——
        /// 刚 Instantiate/Append 的子树在本回调里读 <c>Geometry</c> 已是实测值，无需逐帧
        /// 自旋等待。回调内改 Style 落 mirror dirty，下帧 flush + solve 生效。
        /// 时序：Instantiate(任意时点) → 当帧或下帧 tick → 本回调 fire。cb 抛异常被隔离。
        /// </summary>
        public void CallAfterLayout(Action cb)
        {
            if (cb == null) throw new ArgumentNullException(nameof(cb));
            _afterLayout.Enqueue(cb);
        }

        /// <summary>
        /// 当前是否有指针在 UI 上（命中任意 Touchable 节点）。
        /// 直透传 yio_stage_is_pointer_on_ui FFI。
        /// null stage → false（防御性——_stage 不应为 null，但容错不抛）。
        /// </summary>
        public bool IsPointerOnUI
        {
            get
            {
                if (_stage == IntPtr.Zero) return false;
                StageHandle* h = (StageHandle*)_stage.ToPointer();
                return Native.yio_stage_is_pointer_on_ui(h);
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
