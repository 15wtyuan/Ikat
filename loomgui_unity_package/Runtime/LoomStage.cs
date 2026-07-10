using System;
using System.Buffers;   // ArrayPool<byte> for _frameBuf
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using LoomGUI.Bindings;
using UnityEngine;

namespace LoomGUI
{
    /// 与 Rust tween::TweenProp (u8) 对齐。
    public enum TweenProp : byte { Opacity = 0, Translate = 1, Scale = 2, Rotation = 3, BgColor = 4, TextColor = 5 }
    /// 与 Rust tween::Ease (u8) 对齐。
    public enum Ease : byte { Linear = 0, QuadIn = 1, QuadOut = 2, QuadInOut = 3, CubicIn = 4, CubicOut = 5, CubicInOut = 6, BackIn = 7, BackOut = 8, BackInOut = 9 }

    /// <summary>
    /// 纯 C# Stage 门面：把 Rust Stage（tick→borrow_frame→blob）接到 Unity MirrorPool 渲染。
    /// 本类不是 MonoBehaviour——它由 LoomStageDriver 持有并驱动生命周期：
    ///   driver.Awake → new LoomStage(designSize) + RegisterFont + InitSprites + SetNativeHostRoot
    ///   driver.LateUpdate → stage.Tick(dt)
    ///   driver.OnDestroy → stage.Dispose()
    ///
    /// 所有引擎无关的 FFI 透传 API（CreateRoot/Tween/Controller/...）保持不变——它们只调
    /// Native.loomgui_stage_*，与 Unity 生命周期解耦。Unity 相机/transform/输入采集由 Driver 管辖。
    ///
    /// 设计坐标系：origin 左上、y-down，单位 design px（DesignSize）。根 transform 一次做
    /// MatchWidthOrHeight 缩放 + y-flip（localScale=(sf,-sf,sf)）+ 平移到屏幕左上原点——此变换
    /// 由 Driver 配置（Driver 拥有 transform + 相机）。shader Cull Off 吸收翻转的 winding。
    /// </summary>
    public sealed unsafe class LoomStage : IDisposable
    {
        StageHandle* _stage;
        readonly Vector2 _designSize;
        MaterialManager _mm;
        MirrorPool _pool;
        NativeHostManager _nhm;
        SpriteResolver _sprites;
        // v10：字体光栅化已迁入核心——核心用 ttf-parser 自绘字形产 atlas + mesh，Unity 后端不再持 Unity Font asset。
        // RegisterFont 只喂 Rust 字节（核心端测量），后端不存 Unity Font 表。
        // ArrayPool 租用（非 new）。Rent 返回 ≥len，只 copy/解析 len 字节。
        // Dispose 归还防泄漏。冷帧零 GC（ReadMesh per-node alloc 留观察，撞墙再上 List 复用）。
        byte[] _frameBuf;
        readonly LoomEventHandler _eventHandler = new();

        // Driver 在 Awake 后注入：渲染根 transform（MirrorPool 挂此 root 下）+ safe-area 开关。
        // 未注入（null）时 Tick 跳过渲染——测试构造后直接 Tick(null) 不崩。
        Transform _renderRoot;
        bool _safeArea = true;

        /// <summary>
        /// 建 Stage 句柄 + 基础设施（MaterialManager/MirrorPool/NativeHostManager/SpriteResolver）。
        /// 不在此注册字体——Driver.Awake 后调 RegisterFont 注入字体（bytes 喂 Rust 核心端测量+自绘）。
        /// v10：不再绑 Font.textureRebuilt——核心自产 atlas，无异步 Unity font atlas rebuild。
        /// designSize 默认 (1080,1920)；零向量退回默认（避免除零）。
        /// </summary>
        public LoomStage(Vector2 designSize = default)
        {
            _designSize = designSize == default ? new Vector2(1080, 1920) : designSize;
            _stage = Native.loomgui_stage_new(_designSize.x, _designSize.y);
            if (_stage == null) { Debug.LogError("[LoomStage] loomgui_stage_new failed"); return; }
            _eventHandler.SetHandle((System.IntPtr)_stage);
            var shader = Shader.Find("LoomGUI/Unlit");
            if (shader == null) { Debug.LogError("[LoomStage] Shader LoomGUI/Unlit not found"); FreeStage(); return; }
            _mm = new MaterialManager(shader);
            _pool = new MirrorPool();
            _nhm = new NativeHostManager();
            // NativeHost 的 container transform 由 SetNativeHostRoot 注入（Driver.Awake 调）。
            _sprites = new SpriteResolver();
        }

        /// 游戏侧通过此属性注册 listener（AddListener/RemoveListener），例如
        /// stage.EventHandler.AddListener(nodeId, EventType.Click, OnBtnClick)。
        public LoomEventHandler EventHandler => _eventHandler;

        /// 暴露给 LoomInputCollector.CollectWheel + demo 等内部消费者。
        internal System.IntPtr StagePtr => (System.IntPtr)_stage;
        public Vector2 DesignSize => _designSize;

        /// safe-area letterbox 开关（Driver 配置）。LoomInputCollector.CollectWheel 读此做坐标映射。
        /// 默认 true；Driver 可在 Awake 设 false（全屏无 letterbox）。
        internal bool UseSafeArea
        {
            get => _safeArea;
            set => _safeArea = value;
        }

        // ===== 字体注册（A4 multi-font FFI）=====

        /// <summary>
        /// 注册字体进 Stage。bytes 喂 Rust（核心端 ttf-parser 测量 + 自绘字形产 atlas）。
        /// family = 字体族名（CSS font-family 匹配键）；isDefault=true 设为 Rust FontTable 默认 fallback。
        /// v10：不再双写 Unity Font asset——核心自产 atlas，后端不再光栅化文本。
        /// Driver.Awake 后调此方法注入项目字体（可多次调注册多字体）。
        /// </summary>
        public void RegisterFont(string family, byte[] bytes, bool isDefault)
        {
            if (_stage == null) return;
            byte[] fb = Encoding.UTF8.GetBytes(family ?? "");
            fixed (byte* fp = fb, bp = bytes)
            {
                Native.loomgui_stage_register_font(
                    _stage, fp, (nuint)fb.Length, bp, (nuint)(bytes?.Length ?? 0),
                    isDefault ? (byte)1 : (byte)0);
            }
        }

        /// 设全局字体回退链。families 中主字体缺字时按序 probe，首个含该字的补上（RmlUi fallback 模型）。
        /// 空列表/null 清空回退。须在所有 RegisterFont 之后调（family 须已注册，未注册的 Rust 端静默跳过）。
        /// source-agnostic：后端把系统字体 RegisterFont 进来后，其 family 名同样填这里即可。
        /// </summary>
        public void SetFallbackFamilies(System.Collections.Generic.IEnumerable<string> families)
        {
            if (_stage == null) return;
            string text = families == null ? "" : string.Join("\n", families);
            byte[] tb = Encoding.UTF8.GetBytes(text);
            fixed (byte* tp = tb)
            {
                Native.loomgui_stage_set_fallback_families(_stage, tp, (nuint)tb.Length);
            }
        }

        // ===== Sprite 解析器初始化（Driver 调）=====

        /// <summary>
        /// SpriteResolver 初始化：传入所有 atlas manifest + 页纹理懒加载委托。
        /// Driver.Awake 后调：ParseAtlas 解析每个 atlas.json → AtlasManifest，传入 Init。
        /// loadPage(pageFileName) 按需加载页 PNG（Driver 决定走 Resources/AB/Addressables）。
        /// loadPage=null 则 GetSprite 全 miss（调用方 fallback）。
        /// </summary>
        public void InitSprites(List<AtlasManifest> atlases, System.Func<string, Texture2D> loadPage)
        {
            _sprites?.Init(atlases, loadPage);
        }

        /// <summary>
        /// Set image sizes for all known sprites before first tick.
        /// Merged atlas sprites → (key, width, height) arrays → one FFI call.
        /// Call after loading all atlas.json manifests and before first Tick().
        /// </summary>
        public void SetImageSizes(string[] paths, uint[] ws, uint[] hs)
        {
            if (_stage == null || paths == null || paths.Length == 0) return;
            int n = paths.Length;
            var pathPtrs = new IntPtr[n];
            for (int i = 0; i < n; i++)
                pathPtrs[i] = Marshal.StringToHGlobalAnsi(paths[i] ?? "");
            try
            {
                unsafe
                {
                    fixed (IntPtr* pp = pathPtrs)
                    fixed (uint* wp = ws)
                    fixed (uint* hp = hs)
                    {
                        Native.loomgui_stage_set_image_sizes(_stage, (byte**)pp, wp, hp, (nuint)n);
                    }
                }
            }
            finally
            {
                for (int i = 0; i < n; i++)
                    Marshal.FreeHGlobal(pathPtrs[i]);
            }
        }

        // ===== NativeHost 根注入（Driver 调）=====

        /// <summary>
        /// 注入 NativeHost 的 container 挂载根（Driver 的 transform）。
        /// NativeHostManager.Init 建 _container GO 挂此 root（继承 design→world position + y-flip 抵消）。
        /// Driver.Awake 调；未调则 BindNativeHost 后 Sync 无 container 挂载点（wrapper 无父）。
        /// </summary>
        public void SetNativeHostRoot(Transform root)
        {
            _renderRoot = root;
            _nhm?.Init(root);
        }

        // ===== 每帧 tick（Driver.LateUpdate 调）=====

        /// <summary>
        /// 每帧驱动：tick → borrow_frame → MirrorPool.Sync → NativeHost.Sync → 事件派发。
        /// 替代旧 LateUpdate。渲染根从 _renderRoot 读（SetNativeHostRoot 注入，Driver.Awake 调）。
        /// _renderRoot=null 时跳过渲染（测试构造后无 root 也能 tick 不崩）。
        /// dt 用 unscaledDeltaTime（暂停不受影响）。
        /// </summary>
        public void Tick(float dt)
        {
            if (_stage == null) return;
            Native.loomgui_stage_tick(_stage, dt);

            nuint lenRaw = 0;
            byte* ptr = Native.loomgui_stage_borrow_frame(_stage, &lenRaw);
            int len = (int)lenRaw;
            if (ptr != null && len > 0 && _renderRoot != null)
            {
                if (_frameBuf == null || _frameBuf.Length < len)
                {
                    if (_frameBuf != null) ArrayPool<byte>.Shared.Return(_frameBuf);
                    _frameBuf = ArrayPool<byte>.Shared.Rent(len);
                }
                Marshal.Copy((IntPtr)ptr, _frameBuf, 0, len);
                var blob = new FrameBlob(_frameBuf);
                SyncFontAtlas();
                // v10：不再传字体表（核心自产 atlas，后端不再光栅化文本）。
                _pool.Sync(blob, _renderRoot, _mm, _sprites, Texture2D.whiteTexture);
                _nhm.Sync(_stage);
            }

            // 事件派发（tick 后——borrow_events 读本帧 last_events，下 tick 失效）。
            // 即使 borrow_frame 为空（无渲染节点），事件仍须派发（hover/点击不依赖渲染）。
            nuint evLen = 0;
            byte* evPtr = Native.loomgui_stage_borrow_events(_stage, &evLen);
            _eventHandler.DispatchPending((System.IntPtr)evPtr, (int)evLen);

            // Controller 切页事件（同窗口：tick 后、下 tick 前。out_len=COUNT 非字节）。
            nuint ccLen = 0;
            byte* ccPtr = Native.loomgui_stage_borrow_controller_changed_events(_stage, &ccLen);
            _eventHandler.DispatchControllerChanged((System.IntPtr)ccPtr, (int)ccLen);
        }

        /// <summary>
        /// 拉取核心字体 atlas 脏页 → 上传 R8 Texture2D → Sprite 包装 → 注册进 SpriteResolver。
        /// tick 后 borrow_frame 已更新（本帧渲染节点包含 text Mesh image_path），
        /// _pool.Sync 前注册 atlas Sprite 使 text 节点的 image_path 命中 GetSprite 缓存。
        ///
        /// 双调法取页数据：先探 buf_len=0 返所需字节数 → 分配 buf → 再调填 w/h/bytes。
        /// Atlas 页面通常是 512×512=256KB，每页用独立 ArrayPool 缓冲区（不挤 _frameBuf）。
        /// v1.6 单字体路径固定 f0（默认字体 font_id=0）；多字体 T8 再扩 key。
        /// </summary>
        unsafe void SyncFontAtlas()
        {
            // 探脏页（通常 ≤8 页；v1.6 单字体极少超 16）。
            const int MAX_DIRTY = 16;
            uint* dirtyPtr = stackalloc uint[MAX_DIRTY];
            int n = (int)Native.loomgui_stage_font_atlas_dirty_pages(_stage, dirtyPtr, (nuint)MAX_DIRTY);
            if (n <= 0) return;
            if (n > MAX_DIRTY)
            {
                Debug.LogWarning($"[LoomStage] font atlas dirty pages ({n}) exceed MAX_DIRTY ({MAX_DIRTY}); skipping extras");
                n = MAX_DIRTY;
            }

            for (int i = 0; i < n; i++)
            {
                uint page = dirtyPtr[i];
                uint w = 0, h = 0;
                // 探所需字节数（buf_len=0, out_buf=null → 返 needed 不写 w/h/pixels）。
                int needed = (int)Native.loomgui_stage_font_atlas_page(_stage, page, &w, &h, null, (nuint)0);
                if (needed <= 0) continue;

                byte[] buf = ArrayPool<byte>.Shared.Rent(needed);
                try
                {
                    fixed (byte* pBuf = buf)
                    {
                        int got = (int)Native.loomgui_stage_font_atlas_page(_stage, page, &w, &h, pBuf, (nuint)needed);
                        if (got != needed) continue;
                    }
                    var tex = new Texture2D((int)w, (int)h, TextureFormat.R8, false);
                    fixed (byte* p = buf) { tex.LoadRawTextureData((IntPtr)p, needed); }
                    tex.Apply(false, true);
                    // atlas 是 Stage 级单一共享实例（所有字体字形混在同一 page），路径只以 page 为键——
                    // 不含 font_id（font_id 只作 GlyphKey 区分字形槽位，不进 path）。与 render 侧
                    // build_text_mesh 合成的 loomgui://font-atlas/p{n} 对齐。
                    string path = $"loomgui://font-atlas/p{page}";
                    _sprites.RegisterFontAtlasPage(path, tex);
                }
                finally { ArrayPool<byte>.Shared.Return(buf); }
            }
            Native.loomgui_stage_font_atlas_clear_dirty(_stage);
        }

        // ===== 引擎无关 FFI 透传 API（保持不变）=====

        /// UI 挡住时游戏不响应点击。= 任一活跃槽（鼠标 + 触摸）命中非根节点。
        /// 游戏侧每帧/点击时查此 bool 决定是否消费输入（true → 游戏不响应）。
        public bool IsPointerOnUI()
        {
            if (_stage == null) return false;
            return Native.loomgui_stage_is_pointer_on_ui(_stage);
        }

        /// 按 CSS id 属性查节点（硬编码 build 序 id 不可靠——auto Text 子会偏移序）。
        /// 返 node_id；无匹配 / stage 未建 → uint.MaxValue（0xFFFF_FFFF）。
        public uint FindNodeById(string id)
        {
            if (_stage == null) return uint.MaxValue;
            byte[] bytes = Encoding.UTF8.GetBytes(id);
            fixed (byte* p = bytes)
                return Native.loomgui_stage_find_node_by_id(_stage, p, (nuint)bytes.Length);
        }

        /// 业务设节点 disabled（伪类源 + active/click 抑制）。NodeId 越界 native 侧静默跳过。
        public void SetNodeDisabled(uint nodeId, bool disabled)
        {
            if (_stage == null) return;
            Native.loomgui_stage_set_node_disabled(_stage, nodeId, disabled);
        }

        /// 在子树内找 data-controller="name" 的挂载点，返其 NodeId。
        /// 无匹配 / stage 未建 → uint.MaxValue（0xFFFF_FFFF）。
        public uint GetController(uint subtreeRoot, string name)
        {
            if (_stage == null) return uint.MaxValue;
            byte[] nb = Encoding.UTF8.GetBytes(name ?? "");
            fixed (byte* np = nb)
                return Native.loomgui_stage_get_controller(_stage, subtreeRoot, np, (nuint)nb.Length);
        }

        /// 切 Controller 页。无效 mount（未挂 data-controller）→ 静默返 -1。
        /// 返 prev（切前 selected_index）；首次 set（无条目）返 -1。
        public int SetSelectedIndex(uint mount, int idx)
        {
            if (_stage == null) return -1;
            return Native.loomgui_stage_set_selected_index(_stage, mount, idx);
        }

        /// 读 Controller 当前选中页。无条目 / 无效 mount → -1。
        public int GetSelectedIndex(uint mount)
        {
            if (_stage == null) return -1;
            return Native.loomgui_stage_get_selected_index(_stage, mount);
        }

        /// 编程滚动到指定位置。非 scroll 容器 / 越界 node → no-op（不 panic）。
        /// animated: true → cubic-out 缓动；false → 瞬移。
        public void SetScrollPos(uint node, float x, float y, bool animated = true)
        {
            if (_stage == null) return;
            Native.loomgui_stage_set_scroll_pos(_stage, node, x, y, animated ? (byte)1 : (byte)0);
        }

        // 虚拟列表 driver API（转调 FFI）。
        public void SetContentSize(uint node, float w, float h)
        {
            if (_stage == null) return;
            Native.loomgui_stage_set_content_size(_stage, node, w, h);
        }

        public void ClearContentSizeOverride(uint node)
        {
            if (_stage == null) return;
            Native.loomgui_stage_clear_content_size_override(_stage, node);
        }

        public (float x, float y) GetScrollPos(uint node)
        {
            if (_stage == null) return default;
            float x = 0f, y = 0f;
            // 栈局部 unmanaged 直接 & 取址（CS0213：栈上已固定无需 fixed；C# unsafe 允许栈局部取址）。
            unsafe { Native.loomgui_stage_get_scroll_pos(_stage, node, &x, &y); }
            return (x, y);
        }

        public (float x, float y, float w, float h) GetNodeLayoutRect(uint node)
        {
            if (_stage == null) return default;
            float x = 0f, y = 0f, w = 0f, h = 0f;
            // 栈局部 unmanaged 直接 & 取址（CS0213：栈上已固定无需 fixed；C# unsafe 允许栈局部取址）。
            unsafe { Native.loomgui_stage_get_node_layout_rect(_stage, node, &x, &y, &w, &h); }
            return (x, y, w, h);
        }

        public void SetReuseKey(uint node, uint key)
        {
            if (_stage == null) return;
            Native.loomgui_stage_set_reuse_key(_stage, node, key);
        }

        /// 绑定外部 GO 到 UI 节点（NativeHost-lite spec）。
        /// 每帧 Sync 时自动同步 TRS + visible + sortingOrder。
        public void BindNativeHost(uint nodeId, GameObject go) => _nhm.Bind(nodeId, go);

        /// 按 CSS id 查 nodeId 后绑定外部 GO。
        public void BindNativeHost(string id, GameObject go)
        {
            uint nodeId = FindNodeById(id);
            if (nodeId == uint.MaxValue) { Debug.LogError($"[LoomGUI] NativeHost bind: id '{id}' not found"); return; }
            _nhm.Bind(nodeId, go);
        }

        public void UnbindNativeHost(uint nodeId) => _nhm.Unbind(nodeId);

        /// dump 当前 scene 为 JSON（Rust 拥有，下 tick 失效）。
        public string DumpScene()
        {
            if (_stage == null) return "[]";
            unsafe
            {
                nuint len;
                byte* p = Native.loomgui_stage_dump_scene(_stage, &len);
                if (p == null) return "[]";
                return Encoding.UTF8.GetString(p, (int)len);
            }
        }

        /// 注册 tween。start/end 取前 value_size 个分量（prop 决定）。
        /// 例：fade-in → Tween(id, TweenProp.Opacity, new[]{0f,0,0,0}, new[]{1f,0,0,0}, 0.3f, Ease.Linear, 0f, tag)。
        public void Tween(uint nodeId, TweenProp prop, float[] start, float[] end, float duration, Ease ease, float delay, uint tag)
        {
            if (_stage == null) return;
            unsafe
            {
                fixed (float* sp = start, ep = end)
                    Native.loomgui_stage_tween(_stage, nodeId, (uint)prop, sp, ep, duration, (uint)ease, delay, tag);
            }
        }

        public void KillTween(uint nodeId, TweenProp prop)
        {
            if (_stage == null) return;
            Native.loomgui_stage_kill_tween(_stage, nodeId, (uint)prop);
        }

        public void ClearAnim(uint nodeId)
        {
            if (_stage == null) return;
            Native.loomgui_stage_clear_anim(_stage, nodeId);
        }

        public void ClearAnimProp(uint nodeId, TweenProp prop)
        {
            if (_stage == null) return;
            Native.loomgui_stage_clear_anim_prop(_stage, nodeId, (uint)prop);
        }

        // ===== 包加载 API（§4 load_package/instantiate）：转调 FFI（csbindgen 生成）。
        // 包 = 资源池里的组件模板库；load_package 只进资源池不建 scene；instantiate 克隆子树进 scene。
        // 调用流程（业务 driver）：CreateRoot 建 scene → LoadPackage(name,bytes) 进资源池 →
        // Instantiate(pkg,comp) 建内容 → AppendChild 挂 layer。

        /// 加载包进 Stage 资源池（不建 scene）。多包共存（多次调，name 区分）。
        /// name = 包名（UTF-8，对齐 Stage::load_package(name, bytes)）；bytes = .pkg.bin 二进制。
        /// 返 0=ok，-1=err（stage 未建 / native 解析失败）。包是 Rust-internal，C# 只透传 bytes（不解析）。
        public int LoadPackage(string name, byte[] bytes)
        {
            if (_stage == null) return -1;
            byte[] nb = Encoding.UTF8.GetBytes(name ?? "");
            fixed (byte* np = nb, bp = bytes)
            {
                int r = Native.loomgui_stage_load_package(
                    _stage, np, (nuint)nb.Length, bp, (nuint)(bytes?.Length ?? 0));
                return r;
            }
        }

        /// 从包克隆组件子树进当前 scene，返组件根 NodeId（孤立，调用方 AppendChild 挂 layer）。
        /// pkg = 包名；comp = 组件名（HTML 文件名去 .html）。返 0xFFFF_FFFF = 失败（无 scene / 包/组件不存在）。
        public uint Instantiate(string pkg, string comp)
        {
            if (_stage == null) return uint.MaxValue;
            byte[] pb = Encoding.UTF8.GetBytes(pkg ?? "");
            byte[] cb = Encoding.UTF8.GetBytes(comp ?? "");
            fixed (byte* pp = pb, cp = cb)
                return Native.loomgui_stage_instantiate(
                    _stage, pp, (nuint)pb.Length, cp, (nuint)cb.Length);
        }

        // ===== 动态树 API 封装（§7.2）：转调 FFI（csbindgen 生成 Native.loomgui_stage_*）。
        // kind/css/text/src = UTF-8 字节（fixed 钉住 + 指针+len，同 FindNodeById 风格）。
        // create_root/create_node 返 uint NodeId（0xFFFF_FFFF = 失败）；其余返 int（0=ok，-1=err）。
        // 调用方：用返回的 NodeId 句柄，勿硬编码 0（slotmap idx 从 1 起 → 首节点 NodeId 非 0）。
        // 前置：须先 CreateRoot 建 scene（create_node 等需 self.scene Some）。

        /// 建根节点并设为 roots[0]。kind ∈ {div/button/img/span}；css = "w:100px;..."。
        /// 返 NodeId；0xFFFF_FFFF = 失败（无 scene / 未知 kind）。
        public uint CreateRoot(string kind, string css)
        {
            if (_stage == null) return uint.MaxValue;
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            byte[] c = Encoding.UTF8.GetBytes(css ?? "");
            fixed (byte* kp = k, cp = c)
                return Native.loomgui_stage_create_root(_stage, kp, (nuint)k.Length, cp, (nuint)c.Length);
        }

        /// 建游离节点（不挂父）。需配合 AppendChild/InsertBefore 挂到树。
        /// 返 NodeId；0xFFFF_FFFF = 失败。
        public uint CreateNode(string kind, string css)
        {
            if (_stage == null) return uint.MaxValue;
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            byte[] c = Encoding.UTF8.GetBytes(css ?? "");
            fixed (byte* kp = k, cp = c)
                return Native.loomgui_stage_create_node(_stage, kp, (nuint)k.Length, cp, (nuint)c.Length);
        }

        /// 挂子到 parent 末尾。child 必须当前无父。返 0=ok，-1=err。
        public int AppendChild(uint parent, uint child)
        {
            if (_stage == null) return -1;
            return Native.loomgui_stage_append_child(_stage, parent, child);
        }

        /// 在 parent.children 中 refId 之前插 child。refId=0xFFFF_FFFF → 末尾追加。
        /// 返 0=ok，-1=err。
        public int InsertBefore(uint parent, uint child, uint refId)
        {
            if (_stage == null) return -1;
            return Native.loomgui_stage_insert_before(_stage, parent, child, refId);
        }

        /// 摘子（不删节点）：从 parent.children 移除，节点仍 live 可重挂。返 0=ok，-1=err。
        public int RemoveChild(uint parent, uint child)
        {
            if (_stage == null) return -1;
            return Native.loomgui_stage_remove_child(_stage, parent, child);
        }

        /// 删节点（递归删子 + 联动清 anim/scroll/tween + slotmap remove）。
        /// 该 NodeId 此后失效（gen++）。返 0（恒成功，no-op 语义）。
        public int RemoveNode(uint node)
        {
            if (_stage == null) return 0;
            return Native.loomgui_stage_remove_node(_stage, node);
        }

        /// 改 Text 节点 content + 标 dirty_text。非 Text 节点 → -1。返 0=ok，-1=err。
        public int SetText(uint node, string text)
        {
            if (_stage == null) return -1;
            byte[] t = Encoding.UTF8.GetBytes(text ?? "");
            fixed (byte* tp = t)
                return Native.loomgui_stage_set_text(_stage, node, tp, (nuint)t.Length);
        }

        /// 改 RichText 节点 markup（runtime 解析 → runs + 标 dirty_text）。非 RichText / 解析失败 → -1。0=ok，-1=err。
        public int SetRichText(uint node, string markup)
        {
            if (_stage == null) return -1;
            byte[] m = Encoding.UTF8.GetBytes(markup ?? "");
            fixed (byte* mp = m)
                return Native.loomgui_stage_set_rich_text(_stage, node, mp, (nuint)m.Length);
        }

        /// 改 Image 节点 src + 标 dirty_mesh。非 Image 节点 → -1。返 0=ok，-1=err。
        public int SetSrc(uint node, string src)
        {
            if (_stage == null) return -1;
            byte[] s = Encoding.UTF8.GetBytes(src ?? "");
            fixed (byte* sp = s)
                return Native.loomgui_stage_set_src(_stage, node, sp, (nuint)s.Length);
        }

        /// 改 base_style（apply_css）+ 标 dirty_mesh。下帧 rematch 从 base 重算 style。
        /// 返 0=ok，-1=err。
        public int SetStyle(uint node, string css)
        {
            if (_stage == null) return -1;
            byte[] c = Encoding.UTF8.GetBytes(css ?? "");
            fixed (byte* cp = c)
                return Native.loomgui_stage_set_style(_stage, node, cp, (nuint)c.Length);
        }

        // ===== 释放（Driver.OnDestroy 调；或 using 语法）=====

        /// <summary>
        /// 释放 Stage 持有的 Unity 资源（MirrorPool GO/Mesh + NativeHost wrapper + MaterialManager +
        /// ArrayPool buffer）+ Rust Stage 句柄。Driver.OnDestroy 调；本类非 MonoBehaviour 故无 OnDestroy。
        /// </summary>
        public void Dispose()
        {
            _pool?.Clear();
            _nhm?.Clear();
            _mm?.Clear();
            // SpriteResolver 持 merged sprite 表 + 页纹理缓存（lazy-loaded via loadPage 委托）。
            // Clear 清所有缓存；LoomStage 不主动 Dispose 页纹理（Driver/构建后端拥有其生命周期）。
            _sprites?.Clear();
            if (_frameBuf != null)
            {
                ArrayPool<byte>.Shared.Return(_frameBuf);
                _frameBuf = null;
            }
            FreeStage();
        }

        void FreeStage()
        {
            if (_stage != null)
            {
                Native.loomgui_stage_free(_stage); // null-safe（native 侧检查）
                _stage = null;
            }
        }
    }
}
