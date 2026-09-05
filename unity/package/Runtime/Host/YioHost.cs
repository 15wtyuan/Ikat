// YioHost：引擎无关 stage 宿主 + 每帧驱动核心。
//
// 设计契约（严格时序）：
// - 持 stage handle（StageHandle*）+ UIContext（业务表面）+ YioBackend（Unity/Godot 实现）。
// - 构造：yio_stage_new → UIContext(stage)（复用 internal UIContext(IntPtr)）。
//   不重新建 EventDemuxer——UIContext 构造时已建并接到自身 _eventBus（单一实例，单一事件入口）。
// - 每帧 Step(dt)：backend.CollectInput → 逻辑泵（OnUpdate/CallLater/CallNextFrame）→ flush → tick → borrow_frame → backend.SyncFrame → borrow_events → demuxer.Pump。
//   borrow_frame FFI 在此（backend 只消费 blob，避免二次 borrow）；set_input FFI 由 backend 调（引擎中立）。
// - 资源 FFI（register_font/set_fallback_families/set_image_sizes）引擎中立，byte[]/描述过桥，放此。
// - Dispose：yio_stage_free。
//
// 零 UnityEngine（放 Runtime/Host/）。Unity + Godot-C# 共享——Godot 写 GodotYioBackend : YioBackend 注入。

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using Yio.Bindings;

namespace Yio
{
    /// <summary>
    /// 引擎无关 stage 宿主 + 每帧驱动核心。持 stage handle + <see cref="UIContext"/> +
    /// <see cref="YioBackend"/>。Driver 持本类，每帧调 <see cref="Step"/>。
    /// 零 UnityEngine（放 Runtime/Host/，Unity+Godot-C# 共享）。
    /// </summary>
    public sealed unsafe class YioHost : IDisposable
    {
        StageHandle* _stage;
        readonly UIContext _ctx;
        readonly YioBackend _backend;

        /// <summary>stage 失败标记（构造后 stage=null 也算 disposed，Step/资源 FFI 全 no-op）。</summary>
        public bool IsDisposed { get; private set; }

        /// <summary>
        /// 建 Stage 句柄 + <see cref="UIContext"/> + 接 backend（自建独占资源宿主——单 Stage 行为）。
        /// 不在此注入 Unity 特定资源（SpriteResolver 等）——交给 <paramref name="backend"/> 内部持有，
        /// 由 Driver 调 <see cref="UnityYioBackend"/>.InitSprites/SetRuntimeRoot 等引擎特定初始化。
        /// </summary>
        /// <param name="designW">设计宽（design px，与 HTML/CSS 像素 1:1）。</param>
        /// <param name="designH">设计高。</param>
        /// <param name="backend">引擎后端实现（Unity: <see cref="UnityYioBackend"/>；未来 Godot: GodotYioBackend）。</param>
        /// <exception cref="InvalidOperationException">yio_stage_new 返 null（核心侧 stage 分配失败）。</exception>
        public YioHost(float designW, float designH, YioBackend backend)
            : this(designW, designH, backend, IntPtr.Zero) { }

        /// <summary>
        /// 挂共享资源宿主建 Stage（多 Stage 共享字体驻留 / glyph atlas / 包池）。
        /// 资源 FFI（RegisterFont/SetFallbackFamilies/SetImageSizes/LoadPackage）在挂接后
        /// 仍走本类 stage 级入口——native 侧等价落同一宿主。
        /// </summary>
        /// <param name="resourceHostHandle">共享宿主句柄（Unity 侧 <c>YioResourceHost.Handle</c>；
        /// 引擎中立层只收裸句柄不引资源类型）。Zero = 自建独占宿主。</param>
        /// <exception cref="InvalidOperationException">yio_stage_new/bound 返 null。</exception>
        public YioHost(float designW, float designH, YioBackend backend, IntPtr resourceHostHandle)
        {
            _stage = resourceHostHandle != IntPtr.Zero
                ? Native.yio_stage_new_bound((HostHandle*)resourceHostHandle, designW, designH)
                : Native.yio_stage_new(designW, designH);
            if (_stage == null)
                throw new InvalidOperationException(
                    $"yio_stage_new({designW},{designH}, host={resourceHostHandle != IntPtr.Zero}) returned null");
            _ctx = new UIContext((IntPtr)_stage);
            _backend = backend ?? throw new ArgumentNullException(nameof(backend));
        }

        /// <summary>业务 API 表面（typed Node 树 + 事件 + LoadPackage）。</summary>
        public UIContext Context => _ctx;

        /// <summary>
        /// 光标意图（#93 桌面指针 affordance）：每帧 tick 后查询 core 决策——
        /// 0 = 系统箭头 / 1 = 手型 pointer（pressable 控件或 &lt;a&gt; 悬停，作者 cursor:pointer）/
        /// 2 = 隐藏（作者 cursor:none，游戏自绘光标让位）。引擎无关层只出数值；
        /// 引擎侧（Driver 订阅 CursorIntentChanged 或轮询本属性）驱动 SetCursor。
        /// </summary>
        public uint CursorIntent { get; private set; }

        /// <summary>
        /// 光标意图变化时 fire（tick 后比较去抖；首帧构造哨兵保证必 fire 一次回放当前值）。
        /// 宿主若在 dispose 前需还原系统光标，订阅者自负责（Unity Driver OnDestroy 处理）。
        /// </summary>
        public event Action<uint> CursorIntentChanged;
        uint _lastCursorIntent = uint.MaxValue; // 哨兵 ≠ 任何合法值 → 首帧 Step 必 fire


        /// <summary>
        /// 运行时改画布尺寸（分辨率适配 / 窗口 resize）。core 下帧 solve 按新 root_size
        /// 重排（vw/vh/% 跟随）。返回 false = 拒绝（非有限/≤0，core 保持原值）。
        /// 引擎无关适配策略在 Rust（yio_compute_adaptation）——本方法只搬运结果。
        /// </summary>
        public bool SetRootSize(float w, float h)
        {
            if (_stage == null) return false;
            return Native.yio_stage_set_root_size(_stage, w, h) == 0;
        }

        /// <summary>
        /// 注入屏幕 safe 矩形 + 适配映射（scale/offset，top-down 屏幕 px）——core 算
        /// root 伸进 unsafe 区的深度折 design px 存 Stage，作 env(safe-area-inset-*)
        /// 的取值源。三模式同公式：fit 贴物理边 → 真实 inset；letterbox root 全在
        /// safe 内 → 恒 0（黑边已让位）。每次 RecomputeAdaptation 后跟调一次。
        /// 换算单源在 Rust——各引擎宿主只转发数字。
        /// </summary>
        public bool SetSafeArea(float scale, float offX, float offY, float safeX, float safeY, float safeW, float safeH)
        {
            if (_stage == null) return false;
            return Native.yio_stage_set_safe_area(_stage, scale, offX, offY, safeX, safeY, safeW, safeH) == 0;
        }

        /// <summary>Stage 原始句柄（internal——同程序集 backend / Driver / InputCollector 可见）。</summary>
        internal IntPtr StagePtr => (IntPtr)_stage;

        /// <summary>
        /// 运行时渲染隐藏开关（世界锚点出屏/相机背后自动隐藏）。与 display:none 正交——
        /// 不动布局/命中/子树；visible=false 后端保留镜像对象仅隐藏（MirrorPool SetActive(false)，
        /// 不销毁）。返回 0=成功；-1=stage 已释放或节点不 live（调用方可据此清理锚点登记）。
        /// </summary>
        internal int SetNodeRenderVisible(ulong nodeId, bool visible)
        {
            if (_stage == null) return -1;
            return Native.yio_stage_set_node_visible(_stage, nodeId, visible ? (byte)1 : (byte)0);
        }

        /// <summary>
        /// world-space 挂载登记（#109 C8）：slot 非 0 = node 子树挂到业务 3D 容器（渲染行
        /// 顶点 re-base 到挂载根局部系 + blob mount_id 标注）；slot 0 = 解除回屏幕空间。
        /// 返回 0=成功；-1=stage 已释放或节点不 live。
        /// </summary>
        internal int SetNodeMount(ulong nodeId, uint slot)
        {
            if (_stage == null) return -1;
            return Native.yio_stage_set_node_mount(_stage, nodeId, slot);
        }

        /// <summary>注入的引擎后端（Driver 可拿回去调引擎特定方法，如 UnityYioBackend.NativeHost）。</summary>
        public YioBackend Backend => _backend;

        /// <summary>
        /// 缺字诊断（tofu 取证）：tick 后有新记录时 fire，参数为多行报告文本
        /// （每行：字体族 + 字符 + 码位 + 修法）。会话级去重（同字体族+字符只报一次）。
        /// 引擎无关层不直接打日志——Unity 侧由 Driver 订阅转 Debug.LogWarning。
        /// </summary>
        public event Action<string> MissingGlyphReport;

        /// <summary>
        /// 运行时警告（每条一 fire）：core 侧 warn-once 诊断（如数据驱动 ListView 无滚动
        /// 容器退化全量渲染、ul 被父 flex 纵向拉伸不能滚）。引擎无关层不直接打日志——
        /// Unity 侧由 Driver 订阅转 Debug.LogWarning。
        /// </summary>
        public event Action<string> RuntimeWarning;

        /// <summary>
        /// 每帧驱动序（严格时序）：
        /// 1. <see cref="YioBackend.CollectInput"/>（backend 采集引擎输入 → set_input 系 FFI，引擎中立）
        /// 2. flush seam：攒批回写——一次性把帧内标脏的 StyleMirror / NodeTransform flush 到 core
        ///    （在 tick 前，保证下帧 solve/compute_world_transforms 拿到最新 inline/transform）
        /// 3. yio_stage_tick（核心 process/rematch/solve/refresh/compute_world/build）
        /// 4. borrow_frame → <see cref="YioBackend.SyncFrame"/>（backend 只消费 blob，不再调 borrow FFI）
        /// 5. borrow_events → <see cref="EventDemuxer.Pump"/>（typed On&lt;T&gt; 路由，UIContext._eventDemuxer）
        /// </summary>
        /// <param name="dt">帧时长（秒，建议 Time.unscaledDeltaTime——暂停不受影响）。</param>
        public void Step(float dt)
        {
            if (_stage == null) return;

            // 1. 输入采集 → set_input 系 FFI（backend 调引擎中立 FFI，不破坏 YioHost 引擎无关性）。
            _backend.CollectInput((IntPtr)_stage);

            // 1.4 死亡泵：取走 core 节点死亡通知队列（上帧 list 槽位换绑淘汰克隆 /
            //     外部 remove_node / 内部剪枝）→ evict wrapper + 组件 OnDisconnected。
            //     先于逻辑泵——已断开的组件本帧不再跑 OnUpdate。C# Dispose 走同步回调，
            //     此处只见 Rust 侧死亡；无 wrapper 的死亡静默跳过（去重）。
            _ctx.PumpRemovedNodes();

            // 1.5 逻辑泵：OnUpdate / 到期 CallLater / CallNextFrame（UIContext 投影层内建调度器）。
            //     帧头 fire——回调内改 Style 走下述 flush seam 过桥，本帧 solve 生效。
            _ctx.PumpLogic(dt);

            // 2. 帧末 flush seam：攒批回写。把帧内标脏的 StyleMirror（set_inline_override）
            //    + NodeTransform（set_transform）一次性过桥，在 tick 前保证 core 拿到最新 inline/transform。
            //    旧即时版：setter 每次立即过桥（本处空）；攒批版：集中过桥（N setter = 1 次 flush 遍历）。
            _ctx.FlushPendingWrites();

            // 2.5 ListView tick-drain：拉 core pending_binds 队列、按 slot 反查所属 ListView
            //    调 BindItem。须在 tick 前——同帧克隆的 slot 本 tick 即完成绑定 + 布局，
            //    不首帧显示模板原样。core tick 内 plan/execute 产 pending_binds 是在 solve 前，
            //    故本帧 drain 的 bind 数据下帧 solve 时可见（文本/图片等业务内容）。
            _ctx.DrainPendingBinds();

            // 3. tick：核心一帧编排（process hit 用上帧 world → rematch → solve → refresh_content →
            //    compute_world_transforms → build RenderNode blob）。
            Native.yio_stage_tick(_stage, dt);

            // 3.5 tick 后泵：CallAfterLayout 回调（新挂载子树本帧 solve 完成后 fire，
            //     Geometry 已可读——对 Instantiate 后的摆位是同帧精确值，无需自旋等待）。
            //     回调内改 Style 落 mirror dirty、下帧 flush seam 过桥 + solve 生效。
            _ctx.PumpAfterLayout();

            // 3.55 桌面指针 affordance（#93）：tick 后指针状态机/hover 已刷新，查询光标
            //     意图；与上帧不同才 fire（值不变零开销）。宿主不订阅时仅多一次 FFI 查询。
            uint ci = Native.yio_stage_cursor_query(_stage);
            if (ci != _lastCursorIntent)
            {
                _lastCursorIntent = ci;
                CursorIntent = ci;
                CursorIntentChanged?.Invoke(ci);
            }

            // 3.6 缺字诊断（tofu 取证）：取走本帧新记录 → 事件（引擎无关层不直接打日志）。
            nuint mgLen = 0;
            byte* mgPtr = Native.yio_stage_take_missing_glyphs(_stage, &mgLen);
            if (mgPtr != null && mgLen > 0)
            {
                int n = (int)mgLen;
                if (n > 0 && mgPtr[n - 1] == 0) n--; // 剥尾部 NUL
                MissingGlyphReport?.Invoke(Encoding.UTF8.GetString(mgPtr, n));
                Native.yio_bytes_free(mgPtr, mgLen);
            }

            // 3.7 运行时警告 drain（drain 语义，取走即清）：core warn-once 诊断（无滚动容器
            //     退化全量渲染 / flex 拉伸不能滚等）。多条以 \n 连接，逐条 fire；ptr 由
            //     StageHandle 拥有（下次 take 覆盖），读完即弃无需 free。
            nuint warnLen = 0;
            byte* warnPtr = Native.yio_stage_take_warnings(_stage, &warnLen);
            if (warnPtr != null && warnLen > 0)
            {
                int n = (int)warnLen;
                if (n > 0 && warnPtr[n - 1] == 0) n--; // 剥尾部 NUL
                string joined = Encoding.UTF8.GetString(warnPtr, n);
                foreach (string line in joined.Split('\n'))
                {
                    if (line.Length > 0) RuntimeWarning?.Invoke(line);
                }
            }

            // 4. borrow_frame → backend.SyncFrame（backend 不调 borrow FFI，只消费 blob 做镜像渲染）。
            //    ptr 在下帧 tick 前都有效（核心 reset 借出 buffer）；len=0 时 backend.SyncFrame 自检跳过。
            nuint lenRaw = 0;
            byte* ptr = Native.yio_stage_borrow_frame(_stage, &lenRaw);
            _backend.SyncFrame((IntPtr)_stage, (IntPtr)ptr, (int)lenRaw);

            // 5. borrow_events → demuxer.Pump（typed 路由：raw EventRecord → typed struct → EventBus.Dispatch）。
            //    即使 borrow_frame 空（无渲染节点），事件仍须派发（hover/点击不依赖渲染）。
            //    UIContext._eventDemuxer 在 ctx 构造时建并接到 ctx._eventBus——YioHost 复用单一实例。
            nuint evLen = 0;
            byte* evPtr = Native.yio_stage_borrow_events(_stage, &evLen);
            _ctx._eventDemuxer.Pump((IntPtr)evPtr, (int)evLen);
        }

        /// <summary>
        /// 注册字体进 Stage 字体表。bytes 喂 Rust（核心端 ttf-parser 测量 + 自绘字形产 atlas）。
        /// family = 字体族名（CSS font-family 匹配键）；isDefault=true 设为 Rust FontTable 默认 fallback。
        /// 多次调可注册多字体（Driver.Awake 后注入项目字体）。
        /// </summary>
        public void RegisterFont(string family, byte[] bytes, bool isDefault)
        {
            if (_stage == null) return;
            byte[] fb = Encoding.UTF8.GetBytes(family ?? "");
            fixed (byte* fp = fb, bp = bytes)
            {
                Native.yio_stage_register_font(
                    _stage, fp, (nuint)fb.Length, bp, (nuint)(bytes?.Length ?? 0),
                    isDefault ? (byte)1 : (byte)0);
            }
        }

        /// <summary>
        /// 设全局字体回退链。families 中主字体缺字时按序 probe，首个含该字的补上（RmlUi fallback 模型）。
        /// 空/null 清空回退。须在所有 RegisterFont 之后调（family 须已注册，未注册的 Rust 端静默跳过）。
        /// </summary>
        public void SetFallbackFamilies(IEnumerable<string> families)
        {
            if (_stage == null) return;
            string text = families == null ? "" : string.Join("\n", families);
            byte[] tb = Encoding.UTF8.GetBytes(text);
            fixed (byte* tp = tb)
            {
                Native.yio_stage_set_fallback_families(_stage, tp, (nuint)tb.Length);
            }
        }

        /// <summary>
        /// 注入合并图集 sprite 像素尺寸（atlas.json 解析后）。须在第一次 <see cref="Step"/> 前调——
        /// 核心用此尺寸算 Image 节点的 aspect-ratio + 闭包 known.w/h。
        /// 路径与 Unity Texture2D 上传无关（只传元数据），引擎中立放此；
        /// 实际 Texture2D 上传由 <see cref="UnityYioBackend.InitSprites"/> 走 SpriteResolver 注册。
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
                fixed (IntPtr* pp = pathPtrs)
                fixed (uint* wp = ws)
                fixed (uint* hp = hs)
                {
                    Native.yio_stage_set_image_sizes(_stage, (byte**)pp, wp, hp, (nuint)n);
                }
            }
            finally
            {
                for (int i = 0; i < n; i++)
                    Marshal.FreeHGlobal(pathPtrs[i]);
            }
        }

        /// <summary>
        /// dump 整树 JSON（调 <see cref="Native.yio_stage_dump_scene"/>，UTF-8 marshal）。
        /// Rust 侧拥有 C 串、下 tick 失效——立即消费。未 instantiate（scene=None）/ 已 Dispose → "[]"。
        /// dev 调试桥用（unity-cli-loop execute-dynamic-code 经 Showcase.YioBridge 调），非冻结公共签名。
        /// </summary>
        public string DumpSceneJson()
        {
            if (_stage == null) return "[]";
            nuint len = 0;
            byte* ptr = Native.yio_stage_dump_scene(_stage, &len);
            if (ptr == null || len == 0) return "[]";
            int n = (int)len;
            var buf = new byte[n];
            Marshal.Copy((IntPtr)ptr, buf, 0, n);
            // FFI out_len 含尾部 NUL（as_bytes_with_nul）——剥掉，避免 JSON 末尾多 \0。
            if (n > 0 && buf[n - 1] == 0) n--;
            return Encoding.UTF8.GetString(buf, 0, n);
        }

        /// <summary>
        /// dump 可读树视图（#85）：每节点一行 tag#id.class (rect) + 文本（font/行高/行数/
        /// 内容摘要）与滚动（viewport/content/overlap/pos）关键 resolved 值，ASCII 树缩进。
        /// filter = id/class 子串（null/空 = 全量），只出命中子树——大 UI 不再全量肉眼扫。
        /// Rust 侧拥有 C 串、下次调用覆盖——立即消费。未 instantiate / 已 Dispose → "(no scene)"。
        /// </summary>
        public string DumpSceneTree(string filter = null)
        {
            if (_stage == null) return "(no scene)";
            byte[] fb = string.IsNullOrEmpty(filter) ? null : Encoding.UTF8.GetBytes(filter);
            nuint len = 0;
            byte* ptr;
            fixed (byte* fp = fb)
                ptr = Native.yio_stage_dump_tree(_stage, fp, (nuint)(fb?.Length ?? 0), &len);
            if (ptr == null || len == 0) return "(no scene)";
            int n = (int)len;
            var buf = new byte[n];
            Marshal.Copy((IntPtr)ptr, buf, 0, n);
            if (n > 0 && buf[n - 1] == 0) n--;
            return Encoding.UTF8.GetString(buf, 0, n);
        }

        /// <summary>
        /// 释放 Stage 句柄（Rust 侧 drop Stage + 拥有的所有内存：scene/atlas/tween table）。
        /// 引擎资源（MirrorPool GO/MaterialManager 等）归 backend 自管，本方法不递归——
        /// Driver.OnDestroy 调本方法后，backend 资源由 Driver 额外清理（或 backend 自己 Dispose）。
        /// </summary>
        public void Dispose()
        {
            if (_stage != null)
            {
                Native.yio_stage_free(_stage);
                _stage = null;
            }
            IsDisposed = true;
        }
    }
}
