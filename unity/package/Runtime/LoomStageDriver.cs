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
    /// 启动流程（v1.8）：读 loom.runtime.json → 加载包 → 加载 atlas.json → set_image_sizes →
    /// SpriteResolver.Init → 注册字体 → 正常 tick。不再依赖 ScriptableObject 配置（改读 loom.runtime.json）。
    ///
    /// 三个 public virtual 加载钩子（LoadTextFile/LoadBytes/LoadTexture）默认直读文件系统，
    /// 以 <see cref="_productRoot"/> 为基目录。项目继承覆写以换 AssetBundle/Addressables 加载。
    ///
    /// 设计坐标系：origin 左上、y-down（design px，<see cref="_designSize"/>）。根 transform 一次性
    /// 做 MatchWidthOrHeight shrink-to-fit 缩放 + y-flip（localScale=(sf,-sf,sf)）+ 平移到屏幕左上原点。
    /// 此变换由 <see cref="ConfigureTransforms"/> 配置；UI 相机独立于根（不被根的负 scale 影响）。
    /// shader Cull Off 吸收翻转的 winding。
    /// </summary>
    [ExecuteAlways]
    public class LoomStageDriver : MonoBehaviour
    {
        [Tooltip("设计分辨率（design px）。1080x1920 竖屏 / 1920x1080 横屏。")]
        [SerializeField] UnityEngine.Vector2 _designSize = new(1080, 1920);

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

        // UI 节点 + 相机 + NativeHost wrapper 都用此 layer；cullingMask = 1<<6 让 UI 相机只渲 UI。
        const int LoomUILayer = 6;

        /// <summary>
        /// 持有的 <see cref="LoomHost"/>（Awake 构造）。引擎无关 stage 宿主——
        /// 持 stage 句柄 + <see cref="UIContext"/> + <see cref="UnityLoomBackend"/>。
        /// Awake 失败时为 null。
        /// </summary>
        public LoomHost Host => _host;

        /// <summary>
        /// 业务 API 表面（4a typed Node 树 + 事件 + LoadPackage）。游戏侧通过此 property 拿
        /// <see cref="UIContext"/> 调 typed API（Create&lt;T&gt;/LoadPackage/Events）。
        /// 替代旧 <c>LoomStage.Stage</c> 透传 FFI 表面。Awake 失败时为 null。
        /// </summary>
        public UIContext Context => _host?.Context;

        /// <summary>暴露给输入采集等同程序集内部消费者。</summary>
        internal UnityEngine.Vector2 DesignSize => _designSize;
        internal bool UseSafeArea => _safeArea;

        // ===== Virtual loading hooks (override for AB/Addressables) =====

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

        // ===== Pure logic: merge atlas sprites into (key, width, height) list =====

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

            // InputCollector 提前 GetComponent：backend.SetRuntimeRoot 需要它（CollectInput 内读）。
            // 同步注入 DesignSize/UseSafeArea——LoomInputCollector 自 P2.2 起自带这两个属性，
            // backend.CollectInput 走 _inputCollector.DesignSize/UseSafeArea 路径（不再依赖 stage 字段）。
            if (_inputCollector == null) _inputCollector = GetComponent<LoomInputCollector>();
            if (_inputCollector != null)
            {
                _inputCollector.DesignSize = _designSize;
                _inputCollector.UseSafeArea = _safeArea;
            }

            // Unity 特定资源：Shader + MaterialManager（搬自 LoomStage ctor，LoomStage.cs:61-63）。
            var shader = Shader.Find("LoomGUI/Unlit");
            if (shader == null)
            {
                Debug.LogError("[LoomStageDriver] Shader LoomGUI/Unlit not found");
                return;
            }
            _mm = new MaterialManager(shader);

            // 引擎分层：backend（Unity 特定）+ host（引擎无关驱动序）。
            // LoomHost 构造 loomgui_stage_new → 建 UIContext → 接 backend。
            // loomgui_stage_new 失败时 LoomHost 抛 InvalidOperationException——保留旧 LoomStage
            // 「LogError + return」语义（_host 留 null，LateUpdate/OnDestroy 静默跳过）。
            // 零向量 designSize 退回 (1080,1920)——避免 loomgui_stage_new(0,0) 返 null（搬自旧
            // LoomStage ctor 的 fallback 语义）。
            float dw = _designSize.x > 0f ? _designSize.x : 1080f;
            float dh = _designSize.y > 0f ? _designSize.y : 1920f;
            _backend = new UnityLoomBackend(_mm);
            try
            {
                _host = new LoomHost(dw, dh, _backend);
            }
            catch (Exception e)
            {
                Debug.LogError($"[LoomStageDriver] LoomHost construct failed: {e.Message}");
                return;
            }

            // 引擎根注入：MirrorPool/NativeHost 镜像 GO 挂此 root（transform）。
            // backend.SetRuntimeRoot 设 backend._renderRoot + _inputCollector；
            // backend.NativeHost.Init 建 _container GO 挂此 root（抵消 root y-flip）。
            // 必须在第一次 Step 前——Step 读 _renderRoot，未注入则跳过镜像（空帧）。
            _backend.SetRuntimeRoot(transform, _inputCollector);
            _backend.NativeHost.Init(transform);

            // ── Bootstrap from loom.runtime.json ──
            // 1. Load runtime manifest
            RuntimeManifest runtime = null;
            string runtimeJson = LoadTextFile("loom.runtime.json");
            if (!string.IsNullOrEmpty(runtimeJson))
            {
                try { runtime = RuntimeManifest.ParseRuntime(runtimeJson); }
                catch (Exception e) { Debug.LogWarning($"[LoomStageDriver] Failed to parse loom.runtime.json: {e.Message}"); }
            }

            if (runtime != null)
            {
                // 2. Load packages（UIContext.LoadPackage 4a typed path，替代旧 stage.LoadPackage FFI 透传）
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

        // ===== Font registration (from runtime.json) =====

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
            if (!Input.GetKeyDown(UnityEngine.KeyCode.F8)) return;
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

        void LateUpdate()
        {
            if (_host == null) return;

            // 屏幕 resize 检测（editor 改 Game 视图尺寸 / player 改窗口）→ 重配根变换 + 相机 orthoSize。
            if (Screen.width != _lastScreenW || Screen.height != _lastScreenH)
            {
                _lastScreenW = Screen.width;
                _lastScreenH = Screen.height;
                ConfigureTransforms();
            }

            // host.Step 内含：backend.CollectInput → tick → borrow_frame → backend.SyncFrame
            // → borrow_events → demuxer.Pump。输入采集不再 Driver 直调 InputCollector——
            // backend.CollectInput 走 UnityLoomBackend._inputCollector 路径（与 host 引擎无关性兼容）。
            // unscaledDeltaTime：暂停不受影响（与 UI 时间语义一致）。
            _host.Step(Time.unscaledDeltaTime);
        }

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

        // ===== 以下三个方法为 Unity 相机/transform 配置（design→screen shrink-to-fit + y-flip），逻辑等价于原 LoomStage 同名方法 =====

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
        /// design→screen 根变换（sf + rootPos）。_safeArea=true 时 shrink-to-fit 到 Screen.safeArea
        /// 并把设计 span 居中进 safe 区（safe 区外 letterbox，避刘海）；false 时全屏。
        /// 相机 orthoSize 不变（仍=sh/2 覆盖全屏），root transform 把 design 映射进 safe 区。
        /// <see cref="LoomInputCollector.ScreenToDesign"/> 用同一公式逐项逆映射，保触摸↔渲染对齐。
        /// </summary>
        void ConfigureTransforms()
        {
            float sw = Screen.width, sh = Screen.height;
            var (sf, rootPos) = ComputeRootTransform();

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
        /// 算 shrink-to-fit 缩放 + 屏幕居中偏移。
        /// 前向映射（design→screen，组合 root transform + 正交相机）：
        ///   screen.x = rootPos.x + dx*sf + sw/2     （world.x = rootPos.x + dx*sf；screen.x = world.x + sw/2）
        ///   screen.y = rootPos.y - dy*sf + sh/2     （world.y = rootPos.y - dy*sf，y-flip；screen.y = world.y + sh/2）
        /// 令 offX = 设计 span 在屏幕的左边距（screen.x of design(0)），offYTop = span 顶边（screen.y of design(0)）：
        ///   offX   = area.x + (area.width  - dw*sf) * 0.5   （safe 区水平居中 rendered span dw*sf）
        ///   offYTop= area.y + area.height                  （Unity screen y 下原点，设计 y 上原点 → span 顶 = safe 区顶）
        ///   rootPos.x = offX   - sw/2     （令 screen.x of design(0) = offX = rootPos.x + sw/2）
        ///   rootPos.y = offYTop - sh/2     （令 screen.y of design(0) = offYTop = rootPos.y + sh/2）
        /// </summary>
        (float sf, Vector3 rootPos) ComputeRootTransform()
        {
            float sw = Screen.width, sh = Screen.height;
            UnityEngine.Rect area = _safeArea ? Screen.safeArea : new UnityEngine.Rect(0, 0, sw, sh);
            // 防御：safeArea 可能零宽高（编辑器未配屏）→ 退回全屏
            if (area.width <= 0f || area.height <= 0f) area = new UnityEngine.Rect(0, 0, sw, sh);
            float dw = _designSize.x, dh = _designSize.y;
            // shrink-to-fit：取较小缩放比，保证完整可见 + 留白 letterbox。
            float sf = Mathf.Min(area.width / dw, area.height / dh);
            // 把设计的 rendered span（dw*sf × dh*sf）在 safe 区内居中。
            float offX = area.x + (area.width - dw * sf) * 0.5f;
            float offYTop = area.y + area.height;
            // world-root 位置：令 design(0,0) 渲染到 screen(offX, offYTop) [span 左上角，y 已 flip]。
            Vector3 rootPos = new Vector3(offX - sw * 0.5f, offYTop - sh * 0.5f, 0f);
            return (sf, rootPos);
        }
    }
}
