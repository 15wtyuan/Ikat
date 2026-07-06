using System;
using System.Buffers;   // ArrayPool<byte> for _frameBuf
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using LoomGUI.Bindings;
using UnityEngine;
using UnityEngine.U2D;   // SpriteAtlas（path→Sprite 查询）

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
        // family → Unity 动态字体（光栅用）。与 Rust 端 FontTable 对称：Rust 用 ttf 字节测量，
        // Unity 用同一份 Font asset 光栅。RegisterFont 双写：bytes 喂 Rust，unityFont 进此表。
        readonly Dictionary<string, Font> _unityFonts = new();
        Font _defaultUnityFont;
        // per-stage atlas rebuild 版本号。Driver.Awake 绑 Font.textureRebuilt += stage.OnFontRebuilt，
        // rebuild 时自增 → MirrorPool.Sync 检测到版本变 → 强制 text 节点重光栅取新 UV。
        int _fontVersion;
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
        /// 不在此注册字体——Driver.Awake 后调 RegisterFont 注入字体（bytes 喂 Rust 测量，unityFont 进光栅表）。
        /// 不在此绑 Font.textureRebuilt——Driver.Awake 绑 stage.OnFontRebuilt（全局静态事件，Driver.OnDestroy 解绑）。
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

        /// per-stage atlas rebuild 版本。MirrorPool.Sync 据此判断是否需强制重光栅。
        public int FontVersion => _fontVersion;

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
        /// 注册字体进 Stage。bytes 喂 Rust（ttf-parser 测量），unityFont 存本实例表（Unity 光栅）。
        /// family = 字体族名（CSS font-family 匹配键）；isDefault=true 设为 Rust FontTable + Unity 表的默认 fallback。
        /// Driver.Awake 后调此方法注入项目字体（可多次调注册多字体）。
        /// </summary>
        public void RegisterFont(string family, byte[] bytes, Font unityFont, bool isDefault)
        {
            if (_stage == null) return;
            byte[] fb = Encoding.UTF8.GetBytes(family ?? "");
            fixed (byte* fp = fb, bp = bytes)
            {
                Native.loomgui_stage_register_font(
                    _stage, fp, (nuint)fb.Length, bp, (nuint)(bytes?.Length ?? 0),
                    isDefault ? (byte)1 : (byte)0);
            }
            if (!string.IsNullOrEmpty(family)) _unityFonts[family] = unityFont;
            if (isDefault) _defaultUnityFont = unityFont;
        }

        /// <summary>
        /// Font.textureRebuilt 回调（Driver.Awake 绑：Font.textureRebuilt += stage.OnFontRebuilt）。
        /// 动态字体 atlas 异步 rebuild 时 glyph UV 变——自增版本号，MirrorPool.Sync 下帧检测到版本
        /// 变 → 强制所有 text 节点重 RequestCharactersInTexture + 重取 UV。
        /// Driver.OnDestroy 解绑（全局静态事件，泄漏会跨场景/实例）。
        /// </summary>
        public void OnFontRebuilt(Font font) => _fontVersion++;

        // ===== Sprite 解析器初始化（Driver 调）=====

        /// <summary>
        /// SpriteResolver 建名字映射 + 注入 atlas 懒加载委托。
        /// Driver.Awake 后调（settings 来自 LoomSettings.GetOrCreateDefault，loadAtlas 由 Driver 提供）：
        /// settings.atlasEntries → folder→atlasName 映射；GetSprite 命中时按需回调 loadAtlas(atlasName)
        /// 拿 SpriteAtlas（Driver 决定走 Resources/AB/Addressables）。loadAtlas=null 则全 miss（调用方 fallback）。
        /// </summary>
        public void InitSprites(LoomSettings settings, System.Func<string, SpriteAtlas> loadAtlas)
        {
            _sprites?.Init(settings, loadAtlas);
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
                // MirrorPool.Sync 读本实例 FontVersion（atlas rebuild 检测）+ 字体表（text 光栅）。
                _pool.Sync(blob, _renderRoot, _mm, _sprites, Texture2D.whiteTexture,
                           _unityFonts, _defaultUnityFont, _fontVersion);
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
        /// Font.textureRebuilt 解绑由 Driver 负责（Driver.Awake 绑的，Driver.OnDestroy 解）。
        /// </summary>
        public void Dispose()
        {
            _pool?.Clear();
            _nhm?.Clear();
            _mm?.Clear();
            // SpriteResolver 持 folder→atlasName 名字映射 + 运行时懒加载的 SpriteAtlas 缓存（Driver 钩子加载，
            // 非序列化字段）。Clear 清两套缓存；LoomStage 不主动 Dispose SpriteAtlas（Driver/构建后端拥有其生命周期）。
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
