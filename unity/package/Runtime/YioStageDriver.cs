using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using Yio.Bindings;
using UnityEngine;

namespace Yio
{
    /// <summary>
    /// YioHost 的 Unity 生命周期宿主。持引擎无关 <see cref="YioHost"/>（stage 句柄 +
    /// <see cref="UIContext"/> + <see cref="YioBackend"/>）+ Unity 特定 <see cref="UnityYioBackend"/>
    /// （MirrorPool / MaterialManager / NativeHostManager / SpriteResolver / InputCollector）。
    /// Awake 构造两者 + 注入字体/根 transform + 配 UI 相机/根变换；LateUpdate 每帧驱动
    /// <see cref="YioHost.Step"/>（内含 CollectInput→tick→borrow_frame→SyncFrame→borrow_events→Pump）。
    ///
    /// 启动流程：读 yio.runtime.json → 加载包 → 加载 atlas.json → set_image_sizes →
    /// SpriteResolver.Init → 注册字体 → 正常 tick。不再依赖 ScriptableObject 配置（改读 yio.runtime.json）。
    ///
    /// 三个 public virtual 加载钩子（LoadTextFile/LoadBytes/LoadTexture）默认直读文件系统，
    /// 以 <see cref="_productRoot"/> 为基目录。项目继承覆写以换 AssetBundle/Addressables 加载。
    ///
    /// 设计坐标系：origin 左上、y-down（design px，<see cref="_designSize"/>）。根 transform 一次性
    /// 做适配缩放 + y-flip（localScale=(sf,-sf,sf)）+ 平移到屏幕左上原点。
    /// 此变换由 <see cref="ConfigureTransforms"/> 配置；UI 相机独立于根（不被根的负 scale 影响）。
    /// shader Cull Off 吸收翻转的 winding。
    ///
    /// 分辨率适配（三模式，策略数学在 Rust——<c>yio_compute_adaptation</c>，Driver 只消费）：
    /// Letterbox = contain 黑边（root 锁设计分辨率）；FitWidth/FitHeight = 拆黑边重排
    /// （root 一维锁设计稿、另一维随屏幕，vw/vh/% 跟随画布流动）。设计分辨率与模式的正主是
    /// workspace（yio.runtime.json 的 design/match_mode 透传）；Inspector 字段是 manifest
    /// 缺项时的 fallback。渲染/输入共用「contain-of-canvas」投影公式——喂画布尺寸
    /// （<see cref="_canvas"/>）而非设计分辨率，三模式统一。
    /// </summary>
    [ExecuteAlways]
    public class YioStageDriver : MonoBehaviour
    {
        /// <summary>适配模式（Rust AdaptMode 的 C# 投影，Inspector 可选）。</summary>
        public enum AdaptMode
        {
            /// <summary>contain：完整可见，safe 区内居中，留 letterbox 黑边（默认）。</summary>
            Letterbox = 0,
            /// <summary>宽锚：宽 = 设计宽，高重排（竖屏异形高常用）。</summary>
            FitWidth = 1,
            /// <summary>高锚：高 = 设计高，宽重排（横屏带鱼屏常用）。</summary>
            FitHeight = 2,
        }

        [Tooltip("设计分辨率（design px）。1080x1920 竖屏 / 1920x1080 横屏。runtime.json 带 design 时以 manifest 为准（本字段是 fallback）。")]
        [SerializeField] UnityEngine.Vector2 _designSize = new(1080, 1920);

        [Tooltip("适配模式。Letterbox=contain 黑边；FitWidth/FitHeight=拆黑边重排。runtime.json 带 match_mode 时以 manifest 为准（本字段是 fallback）。")]
        [SerializeField] AdaptMode _adaptMode = AdaptMode.Letterbox;

        [Tooltip("UI 相机（独立 GO，渲染内置 UI layer(5)）。留空时 Awake 自建（自建物不进场景序列化，见 #108）。")]
        [SerializeField] Camera _uiCamera;

        /// <summary>
        /// Awake 自建的相机（#108）。与用户指派的 <see cref="_uiCamera"/> 分开存：序列化字段
        /// 只承载用户意图，自建的走 NonSerialized——编辑态保存场景烤不进引用，杜绝「保存时机
        /// 决定场景里留不留幽灵」（旧值缩放/跨场景悬空引用）。自管生命周期同
        /// NativeHostManager 的 DontSaveInEditor 先例：Unity 不回收，<see cref="OnDestroy"/> 主动销毁。
        /// </summary>
        [NonSerialized] Camera _selfCamera;

        /// <summary>生效 UI 相机：用户指派优先，否则自建。可能为 null（Awake 未跑/失败）。</summary>
        Camera UiCamera => _uiCamera != null ? _uiCamera : _selfCamera;

        [Tooltip("显示 on-screen FPS 读数（调试用）。")]
        [SerializeField] bool _showFps;

        [Tooltip("输入采集器（通常与本 Driver 同 GO）。留空时 Awake GetComponent 兜底。")]
        [SerializeField] YioInputCollector _inputCollector;

        [Tooltip("挂共享资源宿主（多 Stage 共享字体驻留/glyph atlas/包池——per-Stage 固定成本降回一份）。" +
                 "on 时本 Driver 挂 YioResourceHost.Shared（首个开启者懒建）；同宿主的 Driver 须用同一份字体清单（首个注册生效）。")]
        // 缺省 true：多 Stage 就绪默认（单 Driver 自建 Shared 自用，语义不变）；场景缺序列化
// 值时反序列化吃此初始化值（P5 后加字段的老场景正是此态）。
        [SerializeField] internal bool _useSharedHost = true;

        [Tooltip("Stage 层序（小 = 底层，大 = 顶层；决定跨 Stage 渲染序与输入路由优先级）。同序按启用先后。")]
        [SerializeField] internal int _stageOrder;

        [Tooltip("参与输入路由（off = 本 Stage 不收任何指针/键盘事件——world-space 舞台等纯展示 Stage 用）。")]
        [SerializeField] bool _inputEnabled = true;

        /// <summary>hub 输入路由面：本 Driver 是否参与输入（inspector _inputEnabled）。</summary>
        internal bool InputEnabled => _inputEnabled && isActiveAndEnabled;

        /// <summary>
        /// hub 输入路由探测：当前屏幕指针落点映射进本 stage design 系后是否命中可交互
        /// 内容（Context.Pick）。画布外（letterbox 黑边）不算命中。读上帧变换（与 tick
        /// 内 hit 同源、同 1 帧延迟语义）。
        /// 注意：Pick 按 CSS 语义命中可命中盒（含铺满画布的普通容器）——覆盖全画布的
        /// 高层 Stage 页面根须声明 pointer-events:none（交互面板再 auto），否则本探测
        /// 在全画布命中、低层 Stage 的输入被整体饿死（showcase mini-hud 实锤）。
        /// </summary>
        internal bool PointerHitProbe()
        {
            if (_host == null || _inputCollector == null) return false;
            Vector2? screen = CurrentPointerScreen();
            if (screen == null) return false;
            var design = YioInputCollector.ScreenToDesign(
                screen.Value, _adaptScale, _adaptOffX, _adaptOffYTopDown, Screen.height);
            if (design.x < 0f || design.y < 0f || design.x > _canvas.x || design.y > _canvas.y)
                return false;
            return _host.Context.Pick(new YioVector2(design.x, design.y)) != null;
        }

        Vector2? CurrentPointerScreen()
        {
#if ENABLE_INPUT_SYSTEM
            var m = UnityEngine.InputSystem.Mouse.current;
            if (m != null) return m.position.ReadValue();
            var t = UnityEngine.InputSystem.Touchscreen.current;
            if (t != null) return t.primaryTouch.position.ReadValue();
            return null;
#else
            return new Vector2?(UnityEngine.Input.mousePosition);
#endif
        }

        /// <summary>sortingOrder 基址（hub 按层序分配；MirrorPool/NativeHost 消费）。
        /// 注册集变化（DriverCount 变）时 LateUpdate 重取——中途注册低序 Driver 会挤档，
        /// 缓存旧值会与新高序 Stage 撞档穿插。</summary>
        int _sortBase;
        int _lastDriverCount = -1;

        /// <summary>自建相机是否来自 hub 共享池（OnDestroy 走 Release 而非直接销毁）。</summary>
        bool _cameraFromHub;

        /// <summary>URP cameraStack 归属的宿主 Base 相机（ConfigureTransforms 挂上，
        /// OnDestroy 摘除——UI 相机销毁后残留 stack 条目会让宿主相机每帧渲染报错）。</summary>
        Camera _urpBaseCamera;

        /// <summary>生效共享宿主（Awake 解析）。null = 自建独占宿主（单 Stage 行为）。</summary>
        YioResourceHost _sharedHost;

        [Tooltip("产物根目录（含 yio.runtime.json + ui/ + atlas/ + fonts/）。空 = Assets/Bundles（打包器输出，editor 用）；built player 该路径不存在，须显式设此字段（如指向 StreamingAssets 拷贝）。")]
        [SerializeField] string _productRoot = "";

        YioHost _host;
        UnityYioBackend _backend;
        MaterialManager _mm;
        int _lastScreenW = -1, _lastScreenH = -1;
        UnityEngine.Rect _lastSafeArea = new(-1, -1, -1, -1);

        /// <summary>
        /// 当前画布（stage root_size，设计单位）。Letterbox = 设计分辨率；Fit 模式一维锁设计稿、
        /// 另一维随屏幕（Rust 适配数学算出）。渲染根变换与输入映射都吃它（contain-of-canvas 统一投影）。
        /// </summary>
        UnityEngine.Vector2 _canvas = new(1080, 1920);

        /// <summary>生效模式（Awake 从 manifest/Inspector 解析后的 u32 形态）。</summary>
        uint _modeU32 = Yio.Bindings.YioAdaptMode.Letterbox;

        /// <summary>生效设计分辨率（manifest 优先，Inspector fallback；Awake 解析一次）。</summary>
        UnityEngine.Vector2 _designEff = new(1080, 1920);

        /// 适配映射三元组（Rust yio_compute_adaptation 结果；渲染根变换与输入映射共用）。
        float _adaptScale = 1f;
        float _adaptOffX;
        float _adaptOffYTopDown;

        // UI 节点 + 相机 + NativeHost wrapper 都用此 layer；cullingMask = 1<<layer 让 UI 相机只渲 UI。
        // 必须是内置锁定 layer（Unity 内置名 0–5 用户改不了名）："UI"(5)。用户可命名层是
        // 6–31——占任何一个都会与宿主工程撞名（#105：layer 6 被宿主命名成 FloatingText 双影）。
        // 内置层从结构上消灭冲突类，与 FairyGUI 同选（其 StageCamera 用 LayerName "UI"）。
        // 宿主 3D 相机按 Unity 惯例排除本层（cullingMask 抠掉 1<<5），否则 UI 四边形被画两遍。
        const int YioUILayer = 5;

        /// <summary>
        /// 持有的 <see cref="YioHost"/>（Awake 构造）。引擎无关 stage 宿主——
        /// 持 stage 句柄 + <see cref="UIContext"/> + <see cref="UnityYioBackend"/>。
        /// Awake 失败时为 null。
        /// </summary>
        public YioHost Host => _host;

        /// <summary>
        /// 业务 API 表面（typed Node 树 + 事件 + LoadPackage）。游戏侧通过此 property 拿
        /// <see cref="UIContext"/> 调 typed API（Create&lt;T&gt;/LoadPackage/Events）。
        /// Awake 失败时为 null。
        /// </summary>
        public UIContext Context => _host?.Context;

        /// <summary>暴露给输入采集等同程序集内部消费者。</summary>
        internal UnityEngine.Vector2 DesignSize => _designSize;

        /// <summary>
        /// Load a text file relative to the product root.
        /// Default: File.ReadAllText from {productRoot}/{relPath}. Override for AB/Addressables.
        /// Returns null on failure.
        /// </summary>
        public virtual string LoadTextFile(string relPath)
        {
            string root = GetProductRoot();
            if (string.IsNullOrEmpty(root)) return null;
            string path = Path.Combine(root, relPath);
            return File.Exists(path) ? File.ReadAllText(path) : null;
        }

        /// <summary>
        /// Load binary data relative to the product root.
        /// Default: File.ReadAllBytes from {productRoot}/{relPath}. Override for AB/Addressables.
        /// Returns null on failure.
        /// </summary>
        public virtual byte[] LoadBytes(string relPath)
        {
            string root = GetProductRoot();
            if (string.IsNullOrEmpty(root)) return null;
            string path = Path.Combine(root, relPath);
            return File.Exists(path) ? File.ReadAllBytes(path) : null;
        }

        /// <summary>
        /// Load a texture (PNG) relative to the product root.
        /// Default: File.ReadAllBytes + Texture2D.LoadImage from {productRoot}/{relPath}.
        /// Override for AB/Addressables. Returns null on failure.
        /// </summary>
        public virtual Texture2D LoadTexture(string relPath)
        {
            string root = GetProductRoot();
            if (string.IsNullOrEmpty(root)) return null;
            string path = Path.Combine(root, relPath);
            if (!File.Exists(path)) return null;
            try
            {
                var tex = new Texture2D(2, 2);
                tex.LoadImage(File.ReadAllBytes(path));
                return tex;
            }
            catch (Exception e)
            {
                Debug.LogWarning($"[YioStageDriver] Failed to load texture {path}: {e.Message}");
                return null;
            }
        }

        string GetProductRoot()
        {
            if (!string.IsNullOrEmpty(_productRoot))
                return _productRoot;
#if UNITY_EDITOR
            // editor：直读打包器输出（Assets/Bundles），showcase 演示零配置。
            return Path.Combine(Application.dataPath, "Bundles");
#else
            // player：Unity 标准资源位置（发行前把 Bundles 内容拷进 StreamingAssets）。
            // dataPath/Bundles 在 built player 不存在，走 streamingAssetsPath 避免 silent 空白屏。
            return Application.streamingAssetsPath;
#endif
        }

        /// <summary>
        /// Merge all atlas manifests' sprite entries into a deduplicated list of (key, width, height).
        /// Pure function — testable without Unity runtime.
        /// Deduplication: first occurrence wins (atlases ordered by manifest list order).
        /// </summary>
        public static List<(string key, uint w, uint h)> MergeSpriteSizes(List<AtlasManifest> atlases)
        {
            var result = new List<(string, uint, uint)>();
            if (atlases == null) return result;
            var seen = new HashSet<string>();
            foreach (var atlas in atlases)
            {
                if (atlas?.sprites == null) continue;
                foreach (var kv in atlas.sprites)
                {
                    if (!seen.Add(kv.Key)) continue;
                    var orig = kv.Value.orig;
                    if (orig == null || orig.Length < 2) continue;
                    result.Add((kv.Key, (uint)orig[0], (uint)orig[1]));
                }
            }
            return result;
        }

        void Awake()
        {
            // ExecuteAlways：EditMode/Play 反复 Awake + domain reload 会让上一轮的 yio_node 镜像 GO
            // （root 的子）成孤儿残留——上一轮 _pool 引用已丢、Clear 不到。开局先清 root 下所有 yio_node
            // 子 GO，防累积泄漏。UI 相机是独立 GO（SetParent(null)），非 root 子，不受影响。
            for (int c = transform.childCount - 1; c >= 0; c--)
            {
                var child = transform.GetChild(c);
                if (child.name == "yio_node") DestroyImmediate(child.gameObject);
            }

            // manifest 先读（纯文本 IO，无 Unity 资源依赖）——设计分辨率/适配模式的正主在
            // workspace（design/match_mode 透传），Inspector 字段是 manifest 缺项时的 fallback。
            RuntimeManifest runtime = null;
            string runtimeJson = LoadTextFile("yio.runtime.json");
            if (!string.IsNullOrEmpty(runtimeJson))
            {
                try { runtime = RuntimeManifest.ParseRuntime(runtimeJson); }
                catch (Exception e)
                {
                    // 解析失败 = 产物契约破裂，阻断启动（_host 留 null，后续静默跳过）。
                    // manifest 整体作废意味着 packages 列表随之丢失——warning + 继续
                    // 只会让下游（Instantiate 等）报出离根因数层的笼统错误，误导排查。
                    Debug.LogError($"[YioStageDriver] Failed to parse yio.runtime.json — UI install aborted. Re-run `yio build` to regenerate: {e.Message}");
                    return;
                }
            }
            ResolveAdaptation(runtime, applyToStage: false);

            // InputCollector 提前 GetComponent：backend.SetRuntimeRoot 需要它（CollectInput 内读）。
            // 同步注入 DesignSize（= 画布尺寸，三模式统一投影）——
            // YioInputCollector 自带该属性，backend.CollectInput 走同路径（不依赖 stage 字段）。
            if (_inputCollector == null) _inputCollector = GetComponent<YioInputCollector>();
            if (_inputCollector != null)
            {
                _inputCollector.DesignSize = _canvas;
            }

            // Unity 特定资源：Shader + MaterialManager。
            var shader = Shader.Find("Yio/Unlit");
            if (shader == null)
            {
                Debug.LogError("[YioStageDriver] Shader Yio/Unlit not found");
                return;
            }
            _mm = new MaterialManager(shader);

            // A4 多 Stage 合成：注册层序（排序基址）+ 共享相机（EnsureCamera 经 hub 认领/新建）。
            _sortBase = YioStageHub.Register(this, _stageOrder);

            // 引擎分层：backend（Unity 特定）+ host（引擎无关驱动序）。
            // YioHost 构造 yio_stage_new（共享宿主版走 yio_stage_new_bound）→ 建 UIContext → 接 backend。
            // 构造失败时 YioHost 抛 InvalidOperationException——_host 留 null，LateUpdate/OnDestroy 静默跳过。
            // stage 建在画布尺寸上（Letterbox = 设计分辨率；Fit 模式一维已随屏幕适配——
            // ResolveAdaptation 已算好 _canvas）。零向量防御在 Rust 侧（1080×1920 兜底）。
            // 共享宿主：atlas 拉取随 backend 路由到 YioResourceHost.SyncAtlas 单点。
            if (_useSharedHost && _sharedHost == null)
                _sharedHost = YioResourceHost.Shared ??= new YioResourceHost();
            _backend = new UnityYioBackend(_mm, _sharedHost);
            try
            {
                _host = new YioHost(_canvas.x, _canvas.y, _backend, _sharedHost?.Handle ?? IntPtr.Zero);
            }
            catch (Exception e)
            {
                Debug.LogError($"[YioStageDriver] YioHost construct failed: {e.Message}");
                return;
            }
            // 缺字诊断（tofu 取证）+ 运行时告警面（core warn-once）：Editor / Development
            // build 才订阅（发布 build 零日志成本；core 侧 drain 照常跑、缓冲有界）。
            // tofu 框是开发期故意暴露的信号；运行时警告（数据驱动 ListView 配置问题 /
            // 滚轮打进无可滚余量的容器）静默错渲染不如 Console 一行点名。
#if UNITY_EDITOR || DEVELOPMENT_BUILD
            _host.MissingGlyphReport += msg => Debug.LogWarning($"[Yio] missing glyphs (tofu):\n{msg}");
            _host.RuntimeWarning += msg => Debug.LogWarning($"[Yio] {msg}");
#endif

            // 桌面指针 affordance（#93）：core 每帧决策 → 值变化时应用 Unity 软件光标。
            // 纹理程序化生成（无包内资源依赖、零 .meta 交接）；Destroy 时还原系统箭头。
            _host.CursorIntentChanged += ApplyCursorIntent;

            // 引擎根注入：MirrorPool/NativeHost 镜像 GO 挂此 root（transform）。
            // backend.SetRuntimeRoot 设 backend._renderRoot + _inputCollector；
            // backend.NativeHost.Init 建 _container GO 挂此 root（抵消 root y-flip）。
            // 必须在第一次 Step 前——Step 读 _renderRoot，未注入则跳过镜像（空帧）。
            _backend.SetRuntimeRoot(transform, _inputCollector);
            _backend.NativeHost.Init(transform);

            // 1. runtime manifest 已在 Awake 头部读取（design/match_mode 解析早于 stage 构造）。
            if (runtime != null)
            {
                // 2. Load packages（UIContext.LoadPackage typed path）
                foreach (var pkgName in runtime.packages)
                {
                    byte[] bytes = LoadPackageBytes(pkgName);
                    if (bytes != null && bytes.Length > 0)
                        _host.Context.LoadPackage(pkgName, bytes);
                    else
                        Debug.LogWarning($"[YioStageDriver] Package not found: ui/{pkgName}.pkg.bin");
                }

                // 3. Load atlas manifests
                var atlasManifests = new List<AtlasManifest>();
                foreach (var atlasName in runtime.atlases)
                {
                    string atlasJson = LoadTextFile($"atlas/{atlasName}.atlas.json");
                    if (string.IsNullOrEmpty(atlasJson))
                    {
                        Debug.LogWarning($"[YioStageDriver] atlas.json not found: atlas/{atlasName}.atlas.json");
                        continue;
                    }
                    try { atlasManifests.Add(AtlasManifest.ParseAtlas(atlasJson)); }
                    catch (Exception e) { Debug.LogWarning($"[YioStageDriver] Failed to parse atlas/{atlasName}.atlas.json: {e.Message}"); }
                }

                // 4. Push image sizes to Rust core (one FFI call, before first tick)
                if (atlasManifests.Count > 0)
                {
                    var sizes = MergeSpriteSizes(atlasManifests);
                    if (sizes.Count > 0)
                    {
                        int n = sizes.Count;
                        var paths = new string[n];
                        var ws = new uint[n];
                        var hs = new uint[n];
                        for (int i = 0; i < n; i++)
                        {
                            paths[i] = sizes[i].key;
                            ws[i] = sizes[i].w;
                            hs[i] = sizes[i].h;
                        }
                        _host.SetImageSizes(paths, ws, hs);
                    }
                }

                // 5. Init SpriteResolver with atlas manifests + lazy page loader
                //    Unity 特定资源 IO（Texture2D）——归 UnityYioBackend，不进引擎无关 YioHost。
                _backend.InitSprites(atlasManifests, pageName => LoadTexture($"atlas/{pageName}"));

                // 6. Register fonts from runtime manifest
                RegisterFontsFromManifest(runtime);
            }

            // 7. Create scene root（必须 step——instantiate/cascade/solve 都依赖 scene 存在）。
            //    围栏闭合下根节点 kind=Container（div）。设 ctx._rootId 让 ctx.Root 公共入口可用——
            //    业务代码（runner）通过 ctx.Root 拿 typed Container 做挂子 / 查询。
            EnsureSceneRoot();

            EnsureCamera();
            ConfigureTransforms();

            gameObject.layer = YioUILayer;
        }

        /// <summary>
        /// 建场景根（kind=div Container）并写 ctx._rootId。Awake 末尾调一次；多次调幂等（_rootId
        /// 已设则跳过）。create_root 失败（Rust 侧 stage 异常）只 LogError——后续 Instantiate 会
        /// 因 scene 缺失返 sentinel，runner 自行处理 null。
        /// </summary>
        unsafe void EnsureSceneRoot()
        {
            if (_host == null) return;
            var ctx = _host.Context;
            if (ctx._rootId != Node.RootSentinel) return;

            StageHandle* h = (StageHandle*)_host.StagePtr.ToPointer();
            byte[] kind = Encoding.UTF8.GetBytes("div");
            ulong rootId;
            fixed (byte* kp = kind)
                rootId = Native.yio_stage_create_root(h, kp, (nuint)kind.Length, null, 0);
            if (rootId == Node.RootSentinel)
            {
                Debug.LogError("[YioStageDriver] create_root failed (stage null / kind non-UTF-8)");
                return;
            }
            ctx._rootId = rootId;
        }

        /// <summary>
        /// 实例化模板组件到当前 scene 根下。封装 FFI instantiate + typed 包装 + append 到 ctx.Root，
        /// 让业务 runner 不必直接持 UIPackage 句柄（package 已在 Awake 经 runtime.json 自动 load）。
        /// 返回模板根的 typed Container；package 未加载 / 组件路径错 / scene 未建 → null + LogError。
        ///
        /// pkgName 必须已在 yio.runtime.json packages 段列出（Awake 时已 load_package）。
        /// compPath 是 HTML 文件主干名（去 .html），如 workspace 下 foo.html → "foo"。
        /// </summary>
        public unsafe Container Instantiate(string pkgName, string compPath)
        {
            if (_host == null)
            {
                Debug.LogError("[YioStageDriver] Instantiate called but host is null (Awake failed?)");
                return null;
            }
            EnsureSceneRoot();
            var ctx = _host.Context;
            if (ctx._rootId == Node.RootSentinel)
            {
                Debug.LogError($"[YioStageDriver] Instantiate({pkgName},{compPath}) aborted: scene root not created");
                return null;
            }

            StageHandle* h = (StageHandle*)_host.StagePtr.ToPointer();
            byte[] pb = Encoding.UTF8.GetBytes(pkgName ?? "");
            byte[] cb = Encoding.UTF8.GetBytes(compPath ?? "");
            ulong instId;
            fixed (byte* pp = pb)
            fixed (byte* cp = cb)
                instId = Native.yio_stage_instantiate(h, pp, (nuint)pb.Length, cp, (nuint)cb.Length);
            if (instId == Node.RootSentinel)
            {
                Debug.LogError($"[YioStageDriver] instantiate failed: pkg={pkgName} comp={compPath} (pkg not loaded / comp not found / scene missing)");
                return null;
            }

            Container inst = (Container)ctx._registry.GetOrCreate(instId);
            // eager 物化子树内注册组件（RegisterComponent：OnConnected 实例化时跑；
            // 根自身已由上行 GetOrCreate 路由）。同 UITemplate.DoInstantiate。
            ctx.MaterializeCustomElements(instId);
            int rc = Native.yio_stage_append_child(h, ctx._rootId, instId);
            if (rc != 0)
                Debug.LogWarning($"[YioStageDriver] append_child(sceneRoot, {pkgName}/{compPath}) failed rc={rc} (child may have existing parent)");
            return inst;
        }

        /// <summary>
        /// 把外部 GameObject 绑定到 UI 节点（NativeHost）：GO 挂 per-node wrapper，每帧 Sync
        /// 跟随节点的 world transform / 显隐（display:none→SetActive(false)）/ 排序
        /// （sortingOrder = 节点 sort_key，与 UI mesh 同队列 interleaved 渲染）。典型用途：
        /// 3D 模型 / 粒子嵌进 UI 卡面（如角色展示位）。GO 自身 transform（含 scale）不被
        /// Sync 覆盖。材质自动 clone 转 URP Transparent（renderQueue=3000 与 UI 一致）。
        /// 节点 Dispose 后 visible=0 → GO 自动隐藏；重绑同节点先解旧绑。
        /// </summary>
        public void BindNativeHost(Node node, GameObject go)
        {
            if (node == null || go == null) return;
            _backend?.NativeHost.Bind(node._id, go);
            NativeHostManager.ConfigureTransparentMaterials(go);
        }

        /// <summary>解绑 NativeHost（<see cref="BindNativeHost"/> 的逆操作）。GO 不销毁，归还调用方管理。</summary>
        public void UnbindNativeHost(Node node)
        {
            if (node == null) return;
            _backend?.NativeHost.Unbind(node._id);
        }

        // ── world-space 子树挂载（#109 C8）─────────────────────────────────────
        // 与世界锚点（投影路，纯 C# 组合）互补的第三路：整棵 UI 子树挂到业务 3D 变换——
        // 行顶点 re-base 到挂载根局部系（core），镜像 GO SetParent 到容器（MirrorPool 按
        // blob mount_id 路由）。容器层由业务定（场景层 → 3D 相机渲染，ZTest LEqual 吃
        // 深度遮挡）。内层 y-flip 容器镜像 UI 根 (sf,-sf,sf) 的 y 翻转；挂载尺寸不含屏幕
        // 适配缩放——世界大小由 worldParent 的 scale 决定。
        readonly Dictionary<ulong, uint> _mountSlots = new();      // node id → slot
        readonly Dictionary<uint, GameObject> _mountInners = new(); // slot → y-flip 容器 GO
        uint _nextMountSlot = 1;

        /// <summary>
        /// 把 node 子树挂到 3D 变换 worldParent 下：子树在挂载根设计位置处的视觉整体
        /// （布局/命中仍在屏幕系——挂载只改渲染归属）重现为 worldParent 的子物体。
        /// v1 约束：挂载根须成 stacking context（声明 z-index）；挂载内禁 dropdown /
        /// 滚动容器 / 外阴影根 / overflow clip（clip 平面定义在屏幕系，挂载后无意义）。
        /// 重复绑定同节点 = 换绑（旧容器销毁重建）。node null / host 未就绪 = no-op。
        /// </summary>
        public void BindWorldMount(Node node, Transform worldParent)
        {
            if (node == null || worldParent == null || _host == null) return;
            UnbindWorldMount(node);   // 换绑：先清旧容器与登记
            uint slot = _nextMountSlot++;
            var inner = new GameObject("YioMountInner")
            {
                hideFlags = HideFlags.DontSaveInEditor,
            };
            inner.transform.SetParent(worldParent, false);
            inner.transform.localScale = new Vector3(1f, -1f, 1f); // y-flip（同 UI 根约定）
            if (_host.SetNodeMount(node._id, slot) != 0)
            {
                // core 拒绝（v1 约束：挂载子树内禁 dropdown / overflow clip）——回滚容器与登记。
                _mountInners.Remove(slot);
                if (Application.isPlaying) Destroy(inner); else DestroyImmediate(inner);
                Debug.LogError(
                    $"[Yio] BindWorldMount rejected (v1: no dropdowns / overflow clip inside mount): {node._id}");
                return;
            }
            _mountSlots[node._id] = slot;
            _backend.SetMountContainer(slot, inner.transform);
        }

        /// <summary>
        /// 解除 world-space 挂载：子树回屏幕空间渲染（镜像 GO 先行挂回渲染根，容器销毁）。
        /// 未挂载节点 = no-op。
        /// </summary>
        public void UnbindWorldMount(Node node)
        {
            if (node == null) return;
            if (_mountSlots.TryGetValue(node._id, out uint slot) && _mountSlots.Remove(node._id))
            {
                _host?.SetNodeMount(node._id, 0);
                _backend?.ClearMountContainer(slot);
                if (_mountInners.TryGetValue(slot, out var inner) && _mountInners.Remove(slot) && inner != null)
                {
                    if (Application.isPlaying) Destroy(inner);
                    else DestroyImmediate(inner);
                }
            }
        }

        /// <summary>
        /// Register fonts from the runtime manifest's font list.
        /// Overridable for custom loading strategies.
        /// </summary>
        protected virtual void RegisterFontsFromManifest(RuntimeManifest runtime)
        {
            // 共享宿主守卫：同名重注册会换 font_id（native 代数失效钩触发全文本重测）
            // 且 atlas 按新 GlyphKey 重光栅整套字形（N driver × N 套字形副本）。
            // 首个挂接 Driver 的字体清单即宿主清单；差异化字体须直接操作 YioResourceHost。
            if (_sharedHost != null && _sharedHost.FontsRegistered) return;
            if (runtime?.fonts == null) return;
            var fallbacks = new List<string>();
            foreach (var rf in runtime.fonts)
            {
                if (string.IsNullOrEmpty(rf.family) || string.IsNullOrEmpty(rf.file)) continue;
                byte[] bytes = LoadFontBytes(rf.file);
                if (bytes != null)
                    _host.RegisterFont(rf.family, bytes, rf.@default);
                // Collect fallback families (registered or not — Rust side skips unregistered).
                if (rf.fallback)
                    fallbacks.Add(rf.family);
            }
            if (fallbacks.Count > 0)
                _host.SetFallbackFamilies(fallbacks);
            if (_sharedHost != null) _sharedHost.FontsRegistered = true;
        }

        /// <summary>
        /// Load font file bytes from {productRoot}/fonts/{fontFile}.
        /// `fontFile` is the runtime.json `file` value (already includes `.bytes`, e.g. "NotoSansSC.ttc.bytes").
        /// Override for AB/Addressables (builds). Returns null on failure.
        /// </summary>
        public virtual byte[] LoadFontBytes(string fontFile)
        {
            string bytesPath = Path.Combine(GetProductRoot(), "fonts", fontFile);
            return File.Exists(bytesPath) ? File.ReadAllBytes(bytesPath) : null;
        }

        /// <summary>
        /// Default: load .pkg.bin from {productRoot}/ui/{name}.pkg.bin.
        /// Override for AB/Addressables. Returns null on failure.
        /// </summary>
        public virtual byte[] LoadPackageBytes(string name)
        {
            return LoadBytes($"ui/{name}.pkg.bin");
        }

        void Update()
        {
            // 诊断：按 F8 dump 当前 blob（core 视角）+ MirrorPool（Unity 视角）到 console + 文件。
            // 用法：进 play 导航到出问题的页面，按 F8。在「好」「坏」两种布局各按一次，对比两份 dump。
            // 轮询按 Active Input Handling 分流：InputSystem-only 项目里旧版 Input.GetKeyDown
            // 每帧抛 InvalidOperationException。
#if ENABLE_INPUT_SYSTEM
            bool f8 = UnityEngine.InputSystem.Keyboard.current != null
                && UnityEngine.InputSystem.Keyboard.current.f8Key.wasPressedThisFrame;
#elif ENABLE_LEGACY_INPUT_MANAGER
            bool f8 = Input.GetKeyDown(UnityEngine.KeyCode.F8);
#else
            bool f8 = false;
#endif
            if (!f8) return;
            DumpDiagnostic();
        }

        /// <summary>F8 诊断：dump 当前帧 blob（core 给 Unity 的）+ MirrorPool GO 状态（Unity 渲染的）
        /// + scene 可读树（布局归因：rect/文本行高行数/滚动几何每节点一行，#85）到 console + 文件。</summary>
        void DumpDiagnostic()
        {
            if (_backend == null) { Debug.LogWarning("[DumpF8] backend null"); return; }
            string blobDump = _backend.DumpBlobState();
            string poolDump = _backend.DumpMirrorState();
            string sceneTree = _host != null ? _host.DumpSceneTree() : "(no host)";
            string stamp = System.DateTime.Now.ToString("HHmmss");
            string combined = $"===== F8 DIAGNOSTIC {stamp} =====\nstage={(int)_designSize.x}x{(int)_designSize.y} screen={Screen.width}x{Screen.height}\n\n[Scene tree]\n{sceneTree}\n\n{blobDump}\n{poolDump}\n";
            Debug.Log(combined);
            try
            {
                string dir = Path.Combine(Application.dataPath, "..");
                string path = Path.Combine(dir, $"yio-dump-{stamp}.txt");
                File.WriteAllText(path, combined);
                Debug.Log($"[DumpF8] written to {path}");
            }
            catch (Exception e) { Debug.LogWarning($"[DumpF8] file write failed: {e.Message}"); }
        }

        /// <summary>
        /// dev 调试桥用：返回 MirrorPool 状态文本（转发 <see cref="UnityYioBackend.DumpMirrorState"/>，
        /// 同 F8 诊断源）。PlayMode 下有活跃 backend；无 driver/backend 时返提示串。被
        /// Showcase.YioBridge.DumpMirrorPool 经 unity-cli-loop execute-dynamic-code 调。
        /// </summary>
        public string DumpMirrorPoolState() => _backend != null ? _backend.DumpMirrorState() : "backend null";

        // ── 世界锚点（投影路世界 UI）────────────────────────────────────────────
        // 业务持 3D 世界点 + 观察相机，Driver 每帧 Step 前把 worldPos 投影到屏幕 →
        // ScreenToDesign 换算设计坐标写 node.Transform.Position（Step 前写当帧生效——
        // flush seam 在 Step 内）。出屏/相机背后自动隐藏（渲染层开关，与 display:none
        // 正交：布局/命中不动）。锚点按 NodeId 登记：重复 Set 同一节点 = 原位更新
        // （跟随移动实体：每帧重设 worldPos 即可）。
        struct WorldAnchor
        {
            public Node Node;
            public Camera Cam;       // null = 每帧取 Camera.main
            public Vector3 WorldPos;
            public Vector2 OffsetPx; // 设计 px，叠加在投影点右上（y-down 坐标系 → 负 y = 上移）
        }

        readonly Dictionary<ulong, WorldAnchor> _worldAnchors = new();

        /// <summary>
        /// 把节点锚到一个 3D 世界点：每帧（Step 前）经 camera 把 worldPos 投到屏幕 →
        /// 换算设计坐标写入 node.Transform.Position + 叠加 offsetPx。节点出屏或位于
        /// 相机背后时自动隐藏（纯渲染，不动布局/命中），回屏自动恢复显示。
        /// Position 是局部坐标——节点须直挂 stage 根（或业务自行补偿父链偏移）。
        /// camera null = Camera.main。节点已销毁时锚点自动除名。
        /// </summary>
        public void SetWorldAnchor(Node node, Camera camera, Vector3 worldPos, Vector2 offsetPx)
        {
            if (node == null) throw new ArgumentNullException(nameof(node));
            _worldAnchors[node._id] = new WorldAnchor
            {
                Node = node, Cam = camera, WorldPos = worldPos, OffsetPx = offsetPx,
            };
        }

        /// <summary>
        /// 双 Stage 摆台入口：设层序（hub 排序/输入路由用）与共享宿主开关。**须在 Awake
        /// 前调**——运行时建 Driver 的流程 = inactive GO 上 AddComponent → 本调用 →
        /// SetActive(true)（Awake 读这两个字段完成 hub 注册与宿主解析）。
        /// </summary>
        public void ConfigureStage(int stageOrder, bool useSharedHost)
        {
            _stageOrder = stageOrder;
            _useSharedHost = useSharedHost;
        }

        /// <summary>
        /// 运行时渲染隐藏开关（世界锚点出屏同款通道）：visible=false 只控本节点及子树
        /// 全部渲染行隐藏（visibility 继承语义；后端保留镜像对象），与 display:none
        /// 正交——布局与命中不受影响。压测/批量显隐的公共入口（showcase 等包外代码用；
        /// 包内 FFI 直连 YioHost）。
        /// </summary>
        public void SetNodeRenderVisible(Node node, bool visible)
        {
            if (node == null || _host == null) return;
            _host.SetNodeRenderVisible(node._id, visible);
        }

        /// <summary>
        /// 解除世界锚定（节点保持当前 transform 与显示态——隐藏态不回卷，销毁/回收路径
        /// 自理；需要恢复显示走再次 SetWorldAnchor 或留在屏内即自动恢复）。
        /// 未锚定节点为 no-op。
        /// </summary>
        public void ClearWorldAnchor(Node node)
        {
            if (node != null) _worldAnchors.Remove(node._id);
        }

        /// <summary>世界锚点登记数（压测/诊断读数）。</summary>
        public int WorldAnchorCount => _worldAnchors.Count;

        /// <summary>
        /// 锚点投影遍历（LateUpdate 调，Step 前）。屏内：写 Position + 确保显示；
        /// 屏外/背后：只切渲染隐藏（Position 冻结在最后位置，不动 transform 省 churn）。
        /// core 节点已死（rc≠0）→ 锚点自动除名，防死登记逐帧空转。
        /// </summary>
        void UpdateWorldAnchors()
        {
            List<ulong> dead = null;
            foreach (var kv in _worldAnchors)
            {
                var a = kv.Value;
                // 节点已 Dispose（跳字到期/切页）：显式出列——Transform 写在已死节点上会抛
                // ObjectDisposedException 且中断本帧其余锚点跟随；rc≠0 自清兜底不到这里。
                if (a.Node == null || a.Node.IsDisposed)
                {
                    (dead ??= new List<ulong>()).Add(kv.Key);
                    continue;
                }
                var cam = a.Cam != null ? a.Cam : Camera.main;
                if (cam == null) continue; // 无相机可投影：保持现状（不闪隐）
                // WorldToViewportPoint：y-up 0..1；z>0 = 在相机前方（背后 z<0）。
                Vector3 vp = cam.WorldToViewportPoint(a.WorldPos);
                bool onScreen = vp.z > 0f
                    && vp.x >= 0f && vp.x <= 1f && vp.y >= 0f && vp.y <= 1f;
                int rc;
                if (onScreen)
                {
                    // 底左原点屏幕系（ScreenToDesign 输入约定），y 不翻——vp.y 本就 y-up。
                    var screen = new Vector2(vp.x * Screen.width, vp.y * Screen.height);
                    var design = YioInputCollector.ScreenToDesign(
                        screen, _adaptScale, _adaptOffX, _adaptOffYTopDown, Screen.height);
                    a.Node.Transform.Position = new YioVector2(
                        design.x + a.OffsetPx.x, design.y + a.OffsetPx.y);
                    rc = _host.SetNodeRenderVisible(kv.Key, true);
                }
                else
                {
                    rc = _host.SetNodeRenderVisible(kv.Key, false);
                }
                if (rc != 0) (dead ??= new List<ulong>()).Add(kv.Key);
            }
            if (dead != null)
                foreach (ulong id in dead) _worldAnchors.Remove(id);
        }

        void LateUpdate()
        {
            if (_host == null) return;

            // 屏幕 resize / safeArea 变化（editor 改 Game 视图 / player 改窗口 / 旋转）→
            // 重算适配（Fit 模式画布随屏幕变 → set_root_size 触发下帧重排）+ 重配根变换。
            if (Screen.width != _lastScreenW || Screen.height != _lastScreenH || Screen.safeArea != _lastSafeArea)
            {
                _lastScreenW = Screen.width;
                _lastScreenH = Screen.height;
                _lastSafeArea = Screen.safeArea;
                RecomputeAdaptation();
            }

            // A4 多 Stage 输入隔离：多 Driver 并存时按层序独占路由（首个 Pick 命中者得
            // 本帧全部输入——渲染次序即输入次序）；单 Driver 零开销直通。
            _backend.InputEnabled = YioStageHub.DriverCount <= 1
                || YioStageHub.RouteInput(this) == this;
            if (YioStageHub.DriverCount != _lastDriverCount)
            {
                _lastDriverCount = YioStageHub.DriverCount;
                _sortBase = YioStageHub.SortBaseOf(this);
            }
            _backend.SetSortBase(_sortBase);

            // 世界锚点投影（Step 前：Transform 写在 flush seam 之前才当帧生效）。
            if (_worldAnchors.Count > 0) UpdateWorldAnchors();

            // host.Step 内含：backend.CollectInput → tick → borrow_frame → backend.SyncFrame
            // → borrow_events → demuxer.Pump。输入采集不再 Driver 直调 InputCollector——
            // backend.CollectInput 走 UnityYioBackend._inputCollector 路径（与 host 引擎无关性兼容）。
            // unscaledDeltaTime：暂停不受影响（与 UI 时间语义一致）。
            _host.Step(Time.unscaledDeltaTime);

#if UNITY_EDITOR || DEVELOPMENT_BUILD
            UpdatePickProbe();
#endif
        }

#if UNITY_EDITOR || DEVELOPMENT_BUILD
        // F9 pick 命中链探针（编辑器/开发构建；YioDebugProbe.DescribePickChain 本体
        // 常驻可用——正式构建自定义热键绑它即可）。顶层命中变化才打日志，不逐帧刷屏。
        bool _pickProbeOn;
        ulong _probeLastHit = ulong.MaxValue;

        void UpdatePickProbe()
        {
#if ENABLE_INPUT_SYSTEM
            var kb = UnityEngine.InputSystem.Keyboard.current;
            if (kb != null && kb.f9Key.wasPressedThisFrame)
            {
                _pickProbeOn = !_pickProbeOn;
                _probeLastHit = ulong.MaxValue;
                Debug.LogWarning($"[Yio] pick probe {(_pickProbeOn ? "ON" : "OFF")} (F9)");
            }
            if (!_pickProbeOn || _host == null) return;
            var mouse = UnityEngine.InputSystem.Mouse.current;
            if (mouse == null) return;
            var screen = mouse.position.ReadValue();
            var design = YioInputCollector.ScreenToDesign(
                screen, _adaptScale, _adaptOffX, _adaptOffYTopDown, Screen.height);
            Node hit = _host.Context.Pick(new YioVector2(design.x, design.y));
            ulong hitId = hit?._id ?? ulong.MaxValue;
            if (hitId == _probeLastHit) return;
            _probeLastHit = hitId;
            Debug.LogWarning(YioDebugProbe.DescribePickChain(_host.Context, design.x, design.y));
#endif
        }
#endif

        /// <summary>on-screen FPS 读数（_showFps=true 时显示）。1/Time.smoothDeltaTime 平滑帧率。</summary>
        void OnGUI()
        {
            if (!_showFps) return;
            float fps = Time.smoothDeltaTime > 0f ? 1f / Time.smoothDeltaTime : 0f;
            GUI.Label(new UnityEngine.Rect(8f, 8f, 240f, 24f), $"FPS {fps:F1}");
        }

        void OnDestroy()
        {
            // host.Dispose 释放 stage 句柄（引擎中立），backend.Dispose 清理 Unity 资源
            // （MirrorPool GO + NativeHostManager wrapper + MaterialManager 材质 + ArrayPool buffer）。
            // 两者顺序无关（host 不持 backend 引用计数）；先 host 再 backend 与「先 core drop scene
            // 再清引擎镜像」的语义一致。
            if (_host != null)
            {
                _host.Dispose();
                _host = null;
            }
            _backend?.Dispose();
            _backend = null;
            // 自建相机 DontSaveInEditor = Unity 不接管回收：不主动销毁会跨场景泄漏（#108）。
            // hub 共享相机走引用计数 Release（最后一个引用者释放）；IsPlaying/Destroy 分流在 hub。
            // URP overlay 先摘：UI 相机（无论销毁与否）不能再留在宿主 Base 的 cameraStack 里。
            // 但 hub 共享相机可能还有别的 Driver 在用（多 Stage 同场景引用计数）——只有最后
            // 持有者才摘：先行销毁者摘掉会把幸存 Stage 的相机踢出 stack，全部屏幕 UI 整体消失
            // （Overlay 不在任何 stack = 不渲染），直到下一次 resize 触发 ConfigureTransforms
            // 重挂才恢复（showcase「关 stage2 后界面全没了」实锤）。
            if (_urpBaseCamera != null)
            {
                if (!(_cameraFromHub && YioStageHub.CameraHeldByOthers(this)))
                {
                    var data = UrpData(_urpBaseCamera);
                    var stackProp = data != null ? data.GetType().GetProperty("cameraStack") : null;
                    if (stackProp?.GetValue(data) is System.Collections.Generic.List<Camera> stack)
                        stack.Remove(UiCamera);
                }
                _urpBaseCamera = null;
            }
            if (_selfCamera != null)
            {
                if (_cameraFromHub) YioStageHub.ReleaseCamera(this);
                else
                {
                    var cgo = _selfCamera.gameObject;
                    if (Application.isPlaying) UnityEngine.Object.Destroy(cgo);
                    else UnityEngine.Object.DestroyImmediate(cgo);
                }
                _selfCamera = null;
            }
            YioStageHub.Unregister(this);
            // world-space 挂载清账：stage 已释放（上方 host.Dispose）、镜像 GO 已随
            // backend.Dispose 清——此处只剩 y-flip 容器本体须销毁（DontSaveInEditor 不归
            // 编辑器回收）。挂载子树本身随场景/树销毁，core 登记随 stage 释放。
            foreach (var kv in _mountInners)
            {
                if (kv.Value != null)
                {
                    if (Application.isPlaying) Destroy(kv.Value);
                    else DestroyImmediate(kv.Value);
                }
            }
            _mountInners.Clear();
            _mountSlots.Clear();
            // 软件光标还原系统箭头（#93）：SetCursor 的纹理是进程级状态，Play 结束/对象销毁
            // 后残留会把箭头替换带出 UI 会话。先还原再销毁纹理——顺序反了会有一帧
            // SetCursor 指向已销毁纹理。
            UnityEngine.Cursor.SetCursor(null, Vector2.zero, CursorMode.Auto);
            // 注册贴图所有权归消费者，driver 不销毁；只清自建的 hidden 载体。
            if (_hiddenCursor != null) { UnityEngine.Object.Destroy(_hiddenCursor); _hiddenCursor = null; }
        }

        // ---- 桌面指针 affordance（#93）----

        Texture2D _hiddenCursor; // cursor:none 内置默认载体（32×32 全透明，自建自管）
        uint _currentIntent;     // 当前激活意图（0=箭头），注册/清除时判断是否立即重放
        readonly Dictionary<uint, (Texture2D texture, Vector2 hotspot)> _cursorTextures
            = new Dictionary<uint, (Texture2D, Vector2)>();

        /// <summary>
        /// 注册消费者光标贴图（键 = core 光标意图：0=箭头 / 1=手型 pointer / 2=隐藏）。
        /// 未注册的意图走默认：0/1 = 系统箭头（SetCursor(null)），2 = 内置全透明载体。
        /// texture 传 null（或已销毁）= 清除该意图注册。注册/清除时若该意图正激活则立即生效，
        /// 无需等下一次悬停。所有权归消费者——driver 只在 Dispose 还原系统光标并销毁自建
        /// 载体，不销毁注册贴图（激活时贴图已销毁则静默回落默认）。UI 线程调用。
        /// </summary>
        public void SetCursorTexture(uint intent, Texture2D texture, Vector2 hotspot)
        {
            if (texture == null) _cursorTextures.Remove(intent);
            else _cursorTextures[intent] = (texture, hotspot);
            if (intent == _currentIntent) ApplyCursor();
        }

        /// <summary>
        /// core 光标意图 → Unity 软件光标。仅在意图变化帧调用（host 已去抖）；消费侧注册 /
        /// 清除当前意图时经 <see cref="SetCursorTexture"/> 重放。cursor:none 的完整「游戏
        /// 自绘光标」方案需业务自供跟随纹理——本层只负责把指针从元素上藏掉。
        /// </summary>
        void ApplyCursorIntent(uint intent)
        {
            _currentIntent = intent;
            ApplyCursor();
        }

        /// 注册贴图优先（存活才用），未注册回落各意图默认。
        void ApplyCursor()
        {
            if (_cursorTextures.TryGetValue(_currentIntent, out var reg) && reg.texture != null)
            {
                UnityEngine.Cursor.SetCursor(reg.texture, reg.hotspot, CursorMode.Auto);
                return;
            }
            switch (_currentIntent)
            {
                case 2: // cursor:none 是语义（藏指针）而非皮肤——全透明载体即默认
                    if (_hiddenCursor == null) _hiddenCursor = BuildHiddenCursorTexture();
                    UnityEngine.Cursor.SetCursor(_hiddenCursor, Vector2.zero, CursorMode.Auto);
                    break;
                default: // 0=箭头 / 1=手型：默认系统光标，手型皮肤由消费侧 SetCursorTexture 注册
                    UnityEngine.Cursor.SetCursor(null, Vector2.zero, CursorMode.Auto);
                    break;
            }
        }

        /// <summary>cursor:none 载体：全透明纹理贴住热点位（元素级藏指针；整窗自绘光标
        /// 是业务侧 Cursor.visible=false + 自绘 sprite 的既有方案，不经此处）。
        /// 尺寸用 32×32 标准光标尺寸（4×4 非法硬件光标尺寸，Windows 下 SetCursor 拒收）。</summary>
        static Texture2D BuildHiddenCursorTexture()
        {
            const int S = 32;
            var tex = new Texture2D(S, S, TextureFormat.RGBA32, false);
            var clear = new Color32(0, 0, 0, 0);
            var px = new Color32[S * S];
            for (int i = 0; i < px.Length; i++) px[i] = clear;
            tex.SetPixels32(px);
            tex.Apply(false, false);
            return tex;
        }

        // Domain reload 保护。SubsystemRegistration 在 Domain reload 时跑（关闭 Domain Reload 仍跑——
        // 这正是本 hook 存在的根因：关 reload 时 C# 静态活过 Play，但 native 全局态可能悬空）。
        // native 全局态当前为空（Stage per-handle，stage_free drop），但 hook 必须接——引入
        // global texture/font registry 时此处自动清，无需再改接线。
        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.SubsystemRegistration)]
        static void ResetStatics()
        {
            Native.yio_shutdown();
            YioResourceHost.Shared = null;
            YioStageHub.ResetStatics();
        }

        /// <summary>
        /// 建/取 UI 相机。独立 GO（非根的子节点）——避免被根的 (sf,-sf,sf) scale 影响。
        /// 用户在 Inspector 指定优先；否则现场建一个（DontSaveInEditor + NonSerialized 引用，
        /// 不进场景文件——#108）。配 URP UniversalAdditionalCameraData（若类型可寻，
        /// 反射避免硬引用 URP 程序集；缺失则跳过，用户可手挂）。
        /// </summary>
        void EnsureCamera()
        {
            // A4：无用户相机时经 hub 取共享相机（per-Scene 引用计数 + 按名认领存量——
            // 编辑器重编译不跑 OnDestroy、DontSave 相机幸存，不认领会积累重复相机）。
            // 相机配置全由屏幕推导（ConfigureTransforms），多 Driver 共享一台 = 消灭
            // layer 5 互画（每台相机的 cullingMask 都含 UI 层）。
            if (_uiCamera == null && _selfCamera == null)
            {
                _selfCamera = YioStageHub.AcquireCamera(this);
                _cameraFromHub = true;
            }
            var cam = UiCamera;
            if (cam != null)
                cam.gameObject.layer = YioUILayer;
        }

        /// <summary>
        /// design→screen 根变换，消费 <see cref="RecomputeAdaptation"/> 算好的适配结果
        /// （sf + top-down 偏移；Letterbox 在 safe 区居中留黑边，Fit 铺满 safe 区——
        /// 三模式差异全在 Rust 算的偏移里，本函数模式无关）。
        /// 相机 orthoSize 不变（仍=sh/2 覆盖全屏），root transform 把画布映射进 safe 区。
        /// <see cref="YioInputCollector.ScreenToDesign"/> 用同一组 sf/偏移逐项逆映射，保触摸↔渲染对齐。
        /// </summary>
        void ConfigureTransforms()
        {
            float sw = Screen.width, sh = Screen.height;
            float sf = _adaptScale;
            // world-root 位置：令画布原点 (0,0) 渲染到 top-down screen(_adaptOffX, _adaptOffYTopDown)。
            // screen.y 用左下原点（Unity）：y-up 顶边 = sh - offYTopDown → rootPos.y = 顶边 - sh/2。
            Vector3 rootPos = new Vector3(_adaptOffX - sw * 0.5f, (sh - _adaptOffYTopDown) - sh * 0.5f, 0f);

            transform.localScale = new Vector3(sf, -sf, sf);
            transform.localPosition = rootPos;

            var cam = UiCamera;
            if (cam != null)
            {
                cam.orthographic = true;
                cam.orthographicSize = sh / 2f;   // 不变（覆盖全屏，root 映射进 safe 区）
                cam.cullingMask = 1 << YioUILayer;
                // 正交相机允许负 near：裁剪窗口须以 UI 平面（z=0）为中心前后对称——
                // NativeHost 3D 内容按 design px 归一化（~520px 高 × root scale 即数千
                // 世界单位），居中摆位时深度会越过 UI 平面向后延伸到相机（z=-10）之后；
                // near>0 会把 z<-9.9 的整段后方内容切掉。前后各给 10000 深度余量。
                // UI mesh 全在 z≈0 平面、排序走 sortingOrder，均不依赖此窗口。
                cam.nearClipPlane = -9990f;
                cam.farClipPlane = 10000f;
                // 相机独立于根（不 SetParent）：放世界 (0,0,-10) 看向 +z，content 在 z=0。
                cam.transform.SetParent(null, false);
                cam.transform.localPosition = new Vector3(0f, 0f, -10f);
                cam.transform.localRotation = Quaternion.identity;

                // 「UI 叠在宿主 3D 相机之上」按管线分派（见 TryAttachUrpOverlay 注释）。
                _urpBaseCamera = null;
                Camera underlay = FindUnderlayCamera(cam);
                if (underlay != null && TryAttachUrpOverlay(cam, underlay))
                {
                    _urpBaseCamera = underlay;   // OnDestroy 从 stack 摘除
                }
                else if (underlay != null)
                {
                    // Built-in / 反射失败回退：经典保色叠加（只清深度）。
                    ResetToBaseRenderType(cam);
                    cam.clearFlags = CameraClearFlags.Depth;
                }
                else
                {
                    // 无打底相机（纯 UI 游戏、本相机是最底层）：清自己的底色——
                    // 不清色的首相机会读到未初始化缓冲（残影/垃圾）。
                    ResetToBaseRenderType(cam);
                    cam.clearFlags = CameraClearFlags.SolidColor;
                }
            }
        }

        /// <summary>最深的打底相机（depth &lt; 本相机、enabled、URP 下 renderType=Base）。
        /// 无则 null。Overlay 型相机不作打底候选（它自己依附别的 Base）。</summary>
        Camera FindUnderlayCamera(Camera self)
        {
            Camera best = null;
#if UNITY_2023_1_OR_NEWER
            foreach (var c in UnityEngine.Object.FindObjectsByType<Camera>(UnityEngine.FindObjectsInactive.Exclude))
#else
            foreach (var c in FindObjectsOfType<Camera>())
#endif
            {
                if (c == self || !c.isActiveAndEnabled || c.depth >= self.depth) continue;
                if (!IsUrpBaseCamera(c)) continue;
                if (best == null || c.depth > best.depth) best = c;
            }
            return best;
        }

        static readonly string UrpCameraDataType =
            "UnityEngine.Rendering.Universal.UniversalAdditionalCameraData, Unity.RenderPipelines.Universal.Runtime";

        /// <summary>URP 相机数据组件（无 URP / 组件缺席返 null）。反射避免硬引用管线程序集
        /// （Built-in 工程里 URP 程序集不存在，编译期引用直接炸）。</summary>
        static UnityEngine.Component UrpData(Camera c)
        {
            var t = System.Type.GetType(UrpCameraDataType);
            return t != null ? c.GetComponent(t) : null;
        }

        /// <summary>URP 下该相机是否 Base 型（可作打底）。非 URP 工程恒 true。</summary>
        static bool IsUrpBaseCamera(Camera c)
        {
            var data = UrpData(c);
            if (data == null) return true;   // Built-in：无 URP 数据，全算候选
            var rtProp = data.GetType().GetProperty("renderType");
            if (rtProp == null) return true;
            // CameraRenderType.Base = 0（Overlay = 1）。枚举直接比 int 值，不依赖类型名。
            return Convert.ToInt32(rtProp.GetValue(data)) == 0;
        }

        /// <summary>
        /// URP 叠加路：UI 相机配成 Overlay 挂进宿主 Base 相机的 cameraStack。URP 的
        /// Base 相机没有「保色叠加」语义——CameraClearFlags.Depth 实测把颜色也清成
        /// backgroundColor（宿主 3D 整帧抹掉、场景不可见），Nothing 更糟（读到未初始化
        /// 缓冲整屏垃圾色）。cameraStack 是 URP 唯一的跨相机合成通道：Base 先画（天幕/
        /// 3D），Overlay 后画且不碰颜色/深度初值——正是「UI 叠 3D」的管线原生形态。
        /// 成功返 true；非 URP / 宿主缺 URP 数据返 false（调用方走 Built-in 回退）。
        /// 共享 hub 相机多 Driver 重复调用安全（Contains 幂等）。
        /// </summary>
        static bool TryAttachUrpOverlay(Camera ui, Camera baseCam)
        {
            var t = System.Type.GetType(UrpCameraDataType);
            if (t == null) return false;
            var uiData = UrpData(ui);
            if (uiData == null) uiData = ui.gameObject.AddComponent(t);
            var baseData = baseCam.GetComponent(t);
            if (baseData == null) return false;   // 宿主相机无 URP 数据（异常形态），走回退
            try
            {
                var rtProp = t.GetProperty("renderType");
                var overlay = System.Enum.Parse(rtProp.PropertyType, "Overlay");
                rtProp.SetValue(uiData, overlay);
                var stackProp = t.GetProperty("cameraStack");
                if (stackProp.GetValue(baseData) is System.Collections.Generic.List<Camera> stack)
                {
                    // 先清悬挂条目再挂：编辑态（ExecuteAlways）挂上的 DontSave 相机在进播放的
                    // 场景副本里变成 null 引用（DontSave 不随场景序列化），URP 首帧渲染 Base
                    // 相机时报「overlay no longer exists」警告并自清。本方法在场景加载的 Awake
                    // 期跑（先于首帧渲染），把死引用提前清掉——警告不外溢，stack 也不带尸体。
                    stack.RemoveAll(c => c == null);
                    if (!stack.Contains(ui)) stack.Add(ui);
                    return true;
                }
            }
            catch (Exception e)
            {
                Debug.LogWarning($"[YioStageDriver] URP overlay attach failed, fallback to Depth clear: {e.Message}");
            }
            return false;
        }

        /// <summary>Overlay 配置失败/无打底时把 UI 相机还原成独立 Base 相机
        /// （悬挂的 Overlay 型不在任何 stack 里 = 整相机不渲染）。</summary>
        static void ResetToBaseRenderType(Camera ui)
        {
            var data = UrpData(ui);
            var rtProp = data != null ? data.GetType().GetProperty("renderType") : null;
            if (rtProp == null) return;
            try
            {
                var baseType = System.Enum.Parse(rtProp.PropertyType, "Base");
                if (Convert.ToInt32(rtProp.GetValue(data)) != Convert.ToInt32(baseType))
                    rtProp.SetValue(data, baseType);
            }
            catch { /* 非 URP / 枚举缺失：Base 是缺省，无需还原 */ }
        }

        /// <summary>
        /// 解析生效设计分辨率/模式（manifest 优先，Inspector fallback）并调 Rust 适配数学
        /// （yio_compute_adaptation，策略单源——未来 Godot 后端复用同一份）算画布 + 映射三元组。
        /// Awake 首次（stage 未建，只填字段）与运行中 resize 都走这里；画布变化时
        /// set_root_size（core 下帧按新 root_size 重排，vw/vh/% 跟随）+ 刷新 collector 注入 + 重配根变换。
        /// </summary>
        void ResolveAdaptation(RuntimeManifest runtime, bool applyToStage)
        {
            float dw = _designSize.x, dh = _designSize.y;
            if (runtime != null && runtime.design != null && runtime.design.w > 0f && runtime.design.h > 0f)
            {
                dw = runtime.design.w;
                dh = runtime.design.h;
            }
            else if (dw <= 0f || dh <= 0f)
            {
                dw = 1080f; dh = 1920f;   // 零向量防御（与 Rust 侧兜底同值）
            }

            _modeU32 = (uint)_adaptMode;   // Inspector fallback
            if (runtime != null && !string.IsNullOrEmpty(runtime.match_mode))
            {
                var m = Yio.Bindings.YioAdaptMode.FromString(runtime.match_mode);
                if (m.HasValue) _modeU32 = m.Value;
                else Debug.LogWarning($"[YioStageDriver] unknown match_mode '{runtime.match_mode}' in yio.runtime.json -> Inspector fallback ({_adaptMode})");
            }
            RecomputeAdaptation(dw, dh, applyToStage);
        }

        /// <summary>用已解析的设计分辨率/模式重算适配（Awake 后的 resize 路径）。</summary>
        void RecomputeAdaptation()
        {
            RecomputeAdaptation(_designEff.x, _designEff.y, true);
        }

        /// <summary>
        /// 运行时切换适配模式（letterbox / fit-width / fit-height）——立即重算适配并
        /// 喂 Stage 重排（fit 模式画布随屏幕变，vw/vh/% 声明当帧跟随）。演示/设置页
        /// 常用；manifest 的 match_mode 仍是启动正主，本调用只在运行期改写当前值。
        /// 未知字符串返 false 不动现状。
        /// </summary>
        public bool SetAdaptMode(string mode)
        {
            var m = Yio.Bindings.YioAdaptMode.FromString(mode);
            if (!m.HasValue) return false;
            _modeU32 = m.Value;
            _adaptMode = (AdaptMode)m.Value;
            RecomputeAdaptation();
            return true;
        }

        unsafe void RecomputeAdaptation(float dw, float dh, bool applyToStage)
        {
            _designEff = new UnityEngine.Vector2(dw, dh);
            float sw = Screen.width, sh = Screen.height;
            // safe 矩形恒传：letterbox 以它为 contain 框；fit 模式在 core 忽略（贴物理边，
            // unsafe 深度走 env() 通道——SetSafeArea 注入，见下）。
            UnityEngine.Rect a = Screen.safeArea;
            if (a.width <= 0f || a.height <= 0f) a = new UnityEngine.Rect(0, 0, sw, sh);   // 编辑器未配屏防御
            // Rust 侧 safe y 是 top-down（Unity safeArea 是左下原点 y-up）：top-down y = sh - (y+h)。
            Bindings.AdaptResult r;
            if (Native.yio_compute_adaptation(
                    dw, dh, sw, sh, a.x, sh - (a.y + a.height), a.width, a.height, _modeU32, &r) != 0)
            {
                Debug.LogError("[YioStageDriver] yio_compute_adaptation failed -> fallback letterbox @design");
                // 与 Rust adapt::compute 的 Letterbox 同式：top-down safe 原点 + rendered span
                // 双轴居中。a.y 是 Unity 左下原点，须先转 top-down 且补垂直居中项——直接
                // 用 a.y 是错坐标系 + 漏 sy + (sah - dh*scale)*0.5（高屏时贴错边）。
                float syTopDown = sh - (a.y + a.height);
                float fScale = Mathf.Min(a.width / dw, a.height / dh);
                r = new Bindings.AdaptResult {
                    scale = fScale,
                    root_w = dw,
                    root_h = dh,
                    offset_x = a.x + (a.width - dw * fScale) * 0.5f,
                    offset_y = syTopDown + (a.height - dh * fScale) * 0.5f,
                };
            }
            _adaptScale = r.scale;
            _adaptOffX = r.offset_x;
            _adaptOffYTopDown = r.offset_y;

            bool canvasChanged = !Mathf.Approximately(_canvas.x, r.root_w) || !Mathf.Approximately(_canvas.y, r.root_h);
            _canvas = new UnityEngine.Vector2(r.root_w, r.root_h);

            if (_inputCollector != null)
            {
                _inputCollector.DesignSize = _canvas;
                _inputCollector.MapScale = _adaptScale;
                _inputCollector.MapOffX = _adaptOffX;
                _inputCollector.MapOffYTopDown = _adaptOffYTopDown;
            }

            if (applyToStage && canvasChanged && _host != null && !_host.SetRootSize(r.root_w, r.root_h))
                Debug.LogWarning($"[YioStageDriver] set_root_size({r.root_w},{r.root_h}) rejected (invalid size?)");

            // env(safe-area-inset-*) 取值源：适配映射 + 屏幕 safe 矩形（top-down y）。
            // core 算 root 伸进 unsafe 区的深度折 design px（letterbox → 0，fit → 真实值）。
            if (applyToStage && _host != null
                && !_host.SetSafeArea(r.scale, r.offset_x, r.offset_y, a.x, sh - (a.y + a.height), a.width, a.height))
                Debug.LogWarning("[YioStageDriver] set_safe_area rejected");

            if (applyToStage) ConfigureTransforms();
        }
    }
}
