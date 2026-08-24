using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using LoomGUI.Bindings;
using UnityEngine;

namespace LoomGUI
{
    /// <summary>
    /// LoomHost 的 Unity 生命周期宿主。持引擎无关 <see cref="LoomHost"/>（stage 句柄 +
    /// <see cref="UIContext"/> + <see cref="LoomBackend"/>）+ Unity 特定 <see cref="UnityLoomBackend"/>
    /// （MirrorPool / MaterialManager / NativeHostManager / SpriteResolver / InputCollector）。
    /// Awake 构造两者 + 注入字体/根 transform + 配 UI 相机/根变换；LateUpdate 每帧驱动
    /// <see cref="LoomHost.Step"/>（内含 CollectInput→tick→borrow_frame→SyncFrame→borrow_events→Pump）。
    ///
    /// 启动流程：读 loom.runtime.json → 加载包 → 加载 atlas.json → set_image_sizes →
    /// SpriteResolver.Init → 注册字体 → 正常 tick。不再依赖 ScriptableObject 配置（改读 loom.runtime.json）。
    ///
    /// 三个 public virtual 加载钩子（LoadTextFile/LoadBytes/LoadTexture）默认直读文件系统，
    /// 以 <see cref="_productRoot"/> 为基目录。项目继承覆写以换 AssetBundle/Addressables 加载。
    ///
    /// 设计坐标系：origin 左上、y-down（design px，<see cref="_designSize"/>）。根 transform 一次性
    /// 做适配缩放 + y-flip（localScale=(sf,-sf,sf)）+ 平移到屏幕左上原点。
    /// 此变换由 <see cref="ConfigureTransforms"/> 配置；UI 相机独立于根（不被根的负 scale 影响）。
    /// shader Cull Off 吸收翻转的 winding。
    ///
    /// 分辨率适配（三模式，策略数学在 Rust——<c>loomgui_compute_adaptation</c>，Driver 只消费）：
    /// Letterbox = contain 黑边（root 锁设计分辨率）；FitWidth/FitHeight = 拆黑边重排
    /// （root 一维锁设计稿、另一维随屏幕，vw/vh/% 跟随画布流动）。设计分辨率与模式的正主是
    /// workspace（loom.runtime.json 的 design/match_mode 透传）；Inspector 字段是 manifest
    /// 缺项时的 fallback。渲染/输入共用「contain-of-canvas」投影公式——喂画布尺寸
    /// （<see cref="_canvas"/>）而非设计分辨率，三模式统一。
    /// </summary>
    [ExecuteAlways]
    public class LoomStageDriver : MonoBehaviour
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

        [Tooltip("UI 相机（独立 GO，渲染 LoomUILayer）。留空时 Awake 自建。")]
        [SerializeField] Camera _uiCamera;

        [Tooltip("显示 on-screen FPS 读数（调试用）。")]
        [SerializeField] bool _showFps;

        [Tooltip("根 shrink-to-fit 到 Screen.safeArea（on=避刘海，off=全屏）。")]
        [SerializeField] bool _safeArea = true;

        [Tooltip("输入采集器（通常与本 Driver 同 GO）。留空时 Awake GetComponent 兜底。")]
        [SerializeField] LoomInputCollector _inputCollector;

        [Tooltip("产物根目录（含 loom.runtime.json + ui/ + atlas/ + fonts/）。空 = Assets/Bundles（打包器输出，editor 用）；built player 该路径不存在，须显式设此字段（如指向 StreamingAssets 拷贝）。")]
        [SerializeField] string _productRoot = "";

        LoomHost _host;
        UnityLoomBackend _backend;
        MaterialManager _mm;
        int _lastScreenW = -1, _lastScreenH = -1;
        UnityEngine.Rect _lastSafeArea = new(-1, -1, -1, -1);

        /// <summary>
        /// 当前画布（stage root_size，设计单位）。Letterbox = 设计分辨率；Fit 模式一维锁设计稿、
        /// 另一维随屏幕（Rust 适配数学算出）。渲染根变换与输入映射都吃它（contain-of-canvas 统一投影）。
        /// </summary>
        UnityEngine.Vector2 _canvas = new(1080, 1920);

        /// <summary>生效模式（Awake 从 manifest/Inspector 解析后的 u32 形态）。</summary>
        uint _modeU32 = LoomGUI.Bindings.LoomAdaptMode.Letterbox;

        /// <summary>生效设计分辨率（manifest 优先，Inspector fallback；Awake 解析一次）。</summary>
        UnityEngine.Vector2 _designEff = new(1080, 1920);

        /// 适配映射三元组（Rust loomgui_compute_adaptation 结果；渲染根变换与输入映射共用）。
        float _adaptScale = 1f;
        float _adaptOffX;
        float _adaptOffYTopDown;

        // UI 节点 + 相机 + NativeHost wrapper 都用此 layer；cullingMask = 1<<6 让 UI 相机只渲 UI。
        const int LoomUILayer = 6;

        /// <summary>
        /// 持有的 <see cref="LoomHost"/>（Awake 构造）。引擎无关 stage 宿主——
        /// 持 stage 句柄 + <see cref="UIContext"/> + <see cref="UnityLoomBackend"/>。
        /// Awake 失败时为 null。
        /// </summary>
        public LoomHost Host => _host;

        /// <summary>
        /// 业务 API 表面（typed Node 树 + 事件 + LoadPackage）。游戏侧通过此 property 拿
        /// <see cref="UIContext"/> 调 typed API（Create&lt;T&gt;/LoadPackage/Events）。
        /// Awake 失败时为 null。
        /// </summary>
        public UIContext Context => _host?.Context;

        /// <summary>暴露给输入采集等同程序集内部消费者。</summary>
        internal UnityEngine.Vector2 DesignSize => _designSize;
        internal bool UseSafeArea => _safeArea;

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
                Debug.LogWarning($"[LoomStageDriver] Failed to load texture {path}: {e.Message}");
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
            // ExecuteAlways：EditMode/Play 反复 Awake + domain reload 会让上一轮的 loom_node 镜像 GO
            // （root 的子）成孤儿残留——上一轮 _pool 引用已丢、Clear 不到。开局先清 root 下所有 loom_node
            // 子 GO，防累积泄漏。UI 相机是独立 GO（SetParent(null)），非 root 子，不受影响。
            for (int c = transform.childCount - 1; c >= 0; c--)
            {
                var child = transform.GetChild(c);
                if (child.name == "loom_node") DestroyImmediate(child.gameObject);
            }

            // manifest 先读（纯文本 IO，无 Unity 资源依赖）——设计分辨率/适配模式的正主在
            // workspace（design/match_mode 透传），Inspector 字段是 manifest 缺项时的 fallback。
            // 解析失败不阻断启动（warning + Inspector 值兜底，行为同旧版）。
            RuntimeManifest runtime = null;
            string runtimeJson = LoadTextFile("loom.runtime.json");
            if (!string.IsNullOrEmpty(runtimeJson))
            {
                try { runtime = RuntimeManifest.ParseRuntime(runtimeJson); }
                catch (Exception e) { Debug.LogWarning($"[LoomStageDriver] Failed to parse loom.runtime.json: {e.Message}"); }
            }
            ResolveAdaptation(runtime, applyToStage: false);

            // InputCollector 提前 GetComponent：backend.SetRuntimeRoot 需要它（CollectInput 内读）。
            // 同步注入 DesignSize（= 画布尺寸，三模式统一投影）/UseSafeArea——
            // LoomInputCollector 自带这两个属性，backend.CollectInput 走同路径（不依赖 stage 字段）。
            if (_inputCollector == null) _inputCollector = GetComponent<LoomInputCollector>();
            if (_inputCollector != null)
            {
                _inputCollector.DesignSize = _canvas;
                _inputCollector.UseSafeArea = _safeArea;
            }

            // Unity 特定资源：Shader + MaterialManager。
            var shader = Shader.Find("LoomGUI/Unlit");
            if (shader == null)
            {
                Debug.LogError("[LoomStageDriver] Shader LoomGUI/Unlit not found");
                return;
            }
            _mm = new MaterialManager(shader);

            // 引擎分层：backend（Unity 特定）+ host（引擎无关驱动序）。
            // LoomHost 构造 loomgui_stage_new → 建 UIContext → 接 backend。
            // loomgui_stage_new 失败时 LoomHost 抛 InvalidOperationException——
            // _host 留 null，LateUpdate/OnDestroy 静默跳过。
            // stage 建在画布尺寸上（Letterbox = 设计分辨率；Fit 模式一维已随屏幕适配——
            // ResolveAdaptation 已算好 _canvas）。零向量防御在 Rust 侧（1080×1920 兜底）。
            _backend = new UnityLoomBackend(_mm);
            try
            {
                _host = new LoomHost(_canvas.x, _canvas.y, _backend);
            }
            catch (Exception e)
            {
                Debug.LogError($"[LoomStageDriver] LoomHost construct failed: {e.Message}");
                return;
            }
            // 缺字诊断（tofu 取证）：core 每帧报全链缺字（family + 字符 + 码位 + 修法），
            // Console 一行点名——tofu 框是开发期故意暴露的信号，本日志把它变成可查的。
            _host.MissingGlyphReport += msg => Debug.LogWarning($"[LoomGUI] missing glyphs (tofu):\n{msg}");
            // 运行时警告（core warn-once）：数据驱动 ListView 配置类问题（无滚动容器退化
            // 全量渲染 / ul 被父 flex 拉伸不能滚），静默错渲染不如 Console 一行点名。
            _host.RuntimeWarning += msg => Debug.LogWarning($"[LoomGUI] {msg}");

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
                        Debug.LogWarning($"[LoomStageDriver] Package not found: ui/{pkgName}.pkg.bin");
                }

                // 3. Load atlas manifests
                var atlasManifests = new List<AtlasManifest>();
                foreach (var atlasName in runtime.atlases)
                {
                    string atlasJson = LoadTextFile($"atlas/{atlasName}.atlas.json");
                    if (string.IsNullOrEmpty(atlasJson))
                    {
                        Debug.LogWarning($"[LoomStageDriver] atlas.json not found: atlas/{atlasName}.atlas.json");
                        continue;
                    }
                    try { atlasManifests.Add(AtlasManifest.ParseAtlas(atlasJson)); }
                    catch (Exception e) { Debug.LogWarning($"[LoomStageDriver] Failed to parse atlas/{atlasName}.atlas.json: {e.Message}"); }
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
                //    Unity 特定资源 IO（Texture2D）——归 UnityLoomBackend，不进引擎无关 LoomHost。
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

            gameObject.layer = LoomUILayer;
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
            uint rootId;
            fixed (byte* kp = kind)
                rootId = Native.loomgui_stage_create_root(h, kp, (nuint)kind.Length, null, 0);
            if (rootId == Node.RootSentinel)
            {
                Debug.LogError("[LoomStageDriver] create_root failed (stage null / kind non-UTF-8)");
                return;
            }
            ctx._rootId = rootId;
        }

        /// <summary>
        /// 实例化模板组件到当前 scene 根下。封装 FFI instantiate + typed 包装 + append 到 ctx.Root，
        /// 让业务 runner 不必直接持 UIPackage 句柄（package 已在 Awake 经 runtime.json 自动 load）。
        /// 返回模板根的 typed Container；package 未加载 / 组件路径错 / scene 未建 → null + LogError。
        ///
        /// pkgName 必须已在 loom.runtime.json packages 段列出（Awake 时已 load_package）。
        /// compPath 是 HTML 文件主干名（去 .html），如 workspace 下 foo.html → "foo"。
        /// </summary>
        public unsafe Container Instantiate(string pkgName, string compPath)
        {
            if (_host == null)
            {
                Debug.LogError("[LoomStageDriver] Instantiate called but host is null (Awake failed?)");
                return null;
            }
            EnsureSceneRoot();
            var ctx = _host.Context;
            if (ctx._rootId == Node.RootSentinel)
            {
                Debug.LogError($"[LoomStageDriver] Instantiate({pkgName},{compPath}) aborted: scene root not created");
                return null;
            }

            StageHandle* h = (StageHandle*)_host.StagePtr.ToPointer();
            byte[] pb = Encoding.UTF8.GetBytes(pkgName ?? "");
            byte[] cb = Encoding.UTF8.GetBytes(compPath ?? "");
            uint instId;
            fixed (byte* pp = pb)
            fixed (byte* cp = cb)
                instId = Native.loomgui_stage_instantiate(h, pp, (nuint)pb.Length, cp, (nuint)cb.Length);
            if (instId == Node.RootSentinel)
            {
                Debug.LogError($"[LoomStageDriver] instantiate failed: pkg={pkgName} comp={compPath} (pkg not loaded / comp not found / scene missing)");
                return null;
            }

            Container inst = (Container)ctx._registry.GetOrCreate(instId);
            int rc = Native.loomgui_stage_append_child(h, ctx._rootId, instId);
            if (rc != 0)
                Debug.LogWarning($"[LoomStageDriver] append_child(sceneRoot, {pkgName}/{compPath}) failed rc={rc} (child may have existing parent)");
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

        /// <summary>
        /// Register fonts from the runtime manifest's font list.
        /// Overridable for custom loading strategies.
        /// </summary>
        protected virtual void RegisterFontsFromManifest(RuntimeManifest runtime)
        {
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

        /// <summary>F8 诊断：dump 当前帧 blob（core 给 Unity 的）+ MirrorPool GO 状态（Unity 渲染的）。</summary>
        void DumpDiagnostic()
        {
            if (_backend == null) { Debug.LogWarning("[DumpF8] backend null"); return; }
            string blobDump = _backend.DumpBlobState();
            string poolDump = _backend.DumpMirrorState();
            string stamp = System.DateTime.Now.ToString("HHmmss");
            string combined = $"===== F8 DIAGNOSTIC {stamp} =====\nstage={(int)_designSize.x}x{(int)_designSize.y} screen={Screen.width}x{Screen.height}\n\n{blobDump}\n{poolDump}\n";
            Debug.Log(combined);
            try
            {
                string dir = Path.Combine(Application.dataPath, "..");
                string path = Path.Combine(dir, $"loom-dump-{stamp}.txt");
                File.WriteAllText(path, combined);
                Debug.Log($"[DumpF8] written to {path}");
            }
            catch (Exception e) { Debug.LogWarning($"[DumpF8] file write failed: {e.Message}"); }
        }

        /// <summary>
        /// dev 调试桥用：返回 MirrorPool 状态文本（转发 <see cref="UnityLoomBackend.DumpMirrorState"/>，
        /// 同 F8 诊断源）。PlayMode 下有活跃 backend；无 driver/backend 时返提示串。被
        /// Showcase.LoomBridge.DumpMirrorPool 经 unity-cli-loop execute-dynamic-code 调。
        /// </summary>
        public string DumpMirrorPoolState() => _backend != null ? _backend.DumpMirrorState() : "backend null";

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

            // host.Step 内含：backend.CollectInput → tick → borrow_frame → backend.SyncFrame
            // → borrow_events → demuxer.Pump。输入采集不再 Driver 直调 InputCollector——
            // backend.CollectInput 走 UnityLoomBackend._inputCollector 路径（与 host 引擎无关性兼容）。
            // unscaledDeltaTime：暂停不受影响（与 UI 时间语义一致）。
            _host.Step(Time.unscaledDeltaTime);

#if UNITY_EDITOR || DEVELOPMENT_BUILD
            UpdatePickProbe();
#endif
        }

#if UNITY_EDITOR || DEVELOPMENT_BUILD
        // F9 pick 命中链探针（编辑器/开发构建；LoomDebugProbe.DescribePickChain 本体
        // 常驻可用——正式构建自定义热键绑它即可）。顶层命中变化才打日志，不逐帧刷屏。
        bool _pickProbeOn;
        uint _probeLastHit = 0xFFFF_FFFF;

        void UpdatePickProbe()
        {
#if ENABLE_INPUT_SYSTEM
            var kb = UnityEngine.InputSystem.Keyboard.current;
            if (kb != null && kb.f9Key.wasPressedThisFrame)
            {
                _pickProbeOn = !_pickProbeOn;
                _probeLastHit = 0xFFFF_FFFF;
                Debug.LogWarning($"[LoomGUI] pick probe {(_pickProbeOn ? "ON" : "OFF")} (F9)");
            }
            if (!_pickProbeOn || _host == null) return;
            var mouse = UnityEngine.InputSystem.Mouse.current;
            if (mouse == null) return;
            var screen = mouse.position.ReadValue();
            var design = LoomInputCollector.ScreenToDesign(
                screen, _adaptScale, _adaptOffX, _adaptOffYTopDown, Screen.height);
            Node hit = _host.Context.Pick(new LoomVector2(design.x, design.y));
            uint hitId = hit?._id ?? 0xFFFF_FFFF;
            if (hitId == _probeLastHit) return;
            _probeLastHit = hitId;
            Debug.LogWarning(LoomDebugProbe.DescribePickChain(_host.Context, design.x, design.y));
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
        }

        // Domain reload 保护。SubsystemRegistration 在 Domain reload 时跑（关闭 Domain Reload 仍跑——
        // 这正是本 hook 存在的根因：关 reload 时 C# 静态活过 Play，但 native 全局态可能悬空）。
        // native 全局态当前为空（Stage per-handle，stage_free drop），但 hook 必须接——引入
        // global texture/font registry 时此处自动清，无需再改接线。
        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.SubsystemRegistration)]
        static void ResetStatics() { Native.loomgui_shutdown(); }

        /// <summary>
        /// 建/取 UI 相机。独立 GO（非根的子节点）——避免被根的 (sf,-sf,sf) scale 影响。
        /// 用户在 Inspector 指定优先；否则现场建一个。配 URP UniversalAdditionalCameraData（若类型可寻，
        /// 反射避免硬引用 URP 程序集；缺失则跳过，用户可手挂）。
        /// </summary>
        void EnsureCamera()
        {
            if (_uiCamera == null)
            {
                var cgo = new GameObject("LoomUICamera");
                _uiCamera = cgo.AddComponent<Camera>();
                // URP：附加 UniversalAdditionalCameraData（若有该类型）。反射避免硬引用 URP 程序集；
                // 缺失则跳过（用户可手挂）。
                try
                {
                    var t = Type.GetType("UnityEngine.Rendering.Universal.UniversalAdditionalCameraData, Unity.RenderPipelines.Universal.Runtime");
                    if (t != null && _uiCamera.GetComponent(t) == null)
                        _uiCamera.gameObject.AddComponent(t);
                }
                catch { /* URP 缺失：忽略 */ }
            }
            _uiCamera.gameObject.layer = LoomUILayer;
        }

        /// <summary>
        /// design→screen 根变换，消费 <see cref="RecomputeAdaptation"/> 算好的适配结果
        /// （sf + top-down 偏移；Letterbox 在 safe 区居中留黑边，Fit 铺满 safe 区——
        /// 三模式差异全在 Rust 算的偏移里，本函数模式无关）。
        /// 相机 orthoSize 不变（仍=sh/2 覆盖全屏），root transform 把画布映射进 safe 区。
        /// <see cref="LoomInputCollector.ScreenToDesign"/> 用同一组 sf/偏移逐项逆映射，保触摸↔渲染对齐。
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

            if (_uiCamera != null)
            {
                _uiCamera.orthographic = true;
                _uiCamera.orthographicSize = sh / 2f;   // 不变（覆盖全屏，root 映射进 safe 区）
                _uiCamera.cullingMask = 1 << LoomUILayer;
                _uiCamera.clearFlags = CameraClearFlags.Depth;
                _uiCamera.nearClipPlane = 0.1f;   // Unity 要求 near>0；相机 z=-10 看向 z=0 内容
                _uiCamera.farClipPlane = 100f;
                // 相机独立于根（不 SetParent）：放世界 (0,0,-10) 看向 +z，content 在 z=0。
                _uiCamera.transform.SetParent(null, false);
                _uiCamera.transform.localPosition = new Vector3(0f, 0f, -10f);
                _uiCamera.transform.localRotation = Quaternion.identity;
            }
        }

        /// <summary>
        /// 解析生效设计分辨率/模式（manifest 优先，Inspector fallback）并调 Rust 适配数学
        /// （loomgui_compute_adaptation，策略单源——未来 Godot 后端复用同一份）算画布 + 映射三元组。
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
                var m = LoomGUI.Bindings.LoomAdaptMode.FromString(runtime.match_mode);
                if (m.HasValue) _modeU32 = m.Value;
                else Debug.LogWarning($"[LoomStageDriver] unknown match_mode '{runtime.match_mode}' in loom.runtime.json -> Inspector fallback ({_adaptMode})");
            }
            RecomputeAdaptation(dw, dh, applyToStage);
        }

        /// <summary>用已解析的设计分辨率/模式重算适配（Awake 后的 resize 路径）。</summary>
        void RecomputeAdaptation()
        {
            RecomputeAdaptation(_designEff.x, _designEff.y, true);
        }

        void RecomputeAdaptation(float dw, float dh, bool applyToStage)
        {
            _designEff = new UnityEngine.Vector2(dw, dh);
            float sw = Screen.width, sh = Screen.height;
            UnityEngine.Rect a = _safeArea ? Screen.safeArea : new UnityEngine.Rect(0, 0, sw, sh);
            if (a.width <= 0f || a.height <= 0f) a = new UnityEngine.Rect(0, 0, sw, sh);   // 编辑器未配屏防御
            // Rust 侧 safe y 是 top-down（Unity safeArea 是左下原点 y-up）：top-down y = sh - (y+h)。
            Bindings.AdaptResult r;
            if (Native.loomgui_compute_adaptation(
                    dw, dh, sw, sh, a.x, sh - (a.y + a.height), a.width, a.height, _modeU32, &r) != 0)
            {
                Debug.LogError("[LoomStageDriver] loomgui_compute_adaptation failed -> fallback letterbox @design");
                r = new Bindings.AdaptResult { scale = Mathf.Min(a.width / dw, a.height / dh), root_w = dw, root_h = dh,
                    offset_x = a.x + (a.width - dw * Mathf.Min(a.width / dw, a.height / dh)) * 0.5f, offset_y = a.y };
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
                Debug.LogWarning($"[LoomStageDriver] set_root_size({r.root_w},{r.root_h}) rejected (invalid size?)");

            if (applyToStage) ConfigureTransforms();
        }
    }
}
