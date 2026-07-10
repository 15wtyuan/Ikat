using System.Collections.Generic;
using System.IO;
using LoomGUI.Bindings;
using UnityEngine;

namespace LoomGUI
{
    /// <summary>
    /// LoomStage 的 Unity 生命周期宿主。持纯 C# <see cref="LoomStage"/> 实例，在 Awake 构造 + 注入
    /// 字体/根 transform + 配 UI 相机/根变换；LateUpdate 每帧驱动 stage.Tick(dt) + 输入采集。
    ///
    /// 三个 public virtual 加载钩子（LoadFont/LoadPackageBytes）默认直读
    /// Assets/LoomGUI/Bundles/ 目录——仅 editor 可用。项目继承覆写以换 AssetBundle/Addressables 加载。
    /// 加载钩子是 public（非 protected）以便跨程序集的 demo/项目 driver 直接调用。
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
        [SerializeField] Vector2 _designSize = new(1080, 1920);

        [Tooltip("UI 相机（独立 GO，渲染 LoomUILayer）。留空时 Awake 自建。")]
        [SerializeField] Camera _uiCamera;

        [Tooltip("显示 on-screen FPS 读数（调试用）。")]
        [SerializeField] bool _showFps;

        [Tooltip("根 shrink-to-fit 到 Screen.safeArea（on=避刘海，off=全屏）。")]
        [SerializeField] bool _safeArea = true;

        [Tooltip("输入采集器（通常与本 Driver 同 GO）。留空时 Awake GetComponent 兜底。")]
        [SerializeField] LoomInputCollector _inputCollector;

        LoomStage _stage;
        int _lastScreenW = -1, _lastScreenH = -1;

        // UI 节点 + 相机 + NativeHost wrapper 都用此 layer；cullingMask = 1<<6 让 UI 相机只渲 UI。
        const int LoomUILayer = 6;

        /// <summary>
        /// 持有的 LoomStage 实例（Awake 构造）。游戏侧通过此属性拿 stage 调 FFI 透传 API
        /// （CreateRoot/LoadPackage/Instantiate/Tween/...）。Awake 失败时为 null。
        /// </summary>
        public LoomStage Stage => _stage;

        /// <summary>暴露给输入采集等同程序集内部消费者。</summary>
        internal Vector2 DesignSize => _designSize;
        internal bool UseSafeArea => _safeArea;

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

            _stage = new LoomStage(_designSize);
            // CollectWheel 是静态方法读 stage.UseSafeArea（须推进 stage）；Collect 直接收 _safeArea 作参。
            _stage.UseSafeArea = _safeArea;

            // 注入渲染根（NativeHostManager 建 container GO 挂此 root；MirrorPool 镜像 GO 也挂此 root）。
            // 必须在 Tick 前调——Tick 读 _renderRoot，未注入则跳过渲染（空帧）。
            _stage.SetNativeHostRoot(transform);

            var settings = LoomSettings.GetOrCreateDefault();
            // Build atlas manifests from loom.runtime.json + *.atlas.json (standalone packer output).
            // loadPage reads page PNGs from {pkgDir}/atlas/. T15 will wire AB/Addressables override.
            _stage.InitSprites(BuildAtlasManifests(settings), LoadAtlasPage);
            RegisterFontsFromSettings();

            EnsureCamera();
            ConfigureTransforms();

            gameObject.layer = LoomUILayer;

            if (_inputCollector == null) _inputCollector = GetComponent<LoomInputCollector>();
        }

        /// <summary>
        /// 默认实现：遍历 <see cref="LoomSettings.fonts"/> → <see cref="LoadFontBytes"/> → stage.RegisterFont。
        /// 项目子类可覆写以改加载策略（如先批量预加载、异步加载、错误兜底）。
        /// protected：内部编排钩子，外部不应直接调用（用 <see cref="LoadFontBytes"/> 单个加载）。
        /// </summary>
        protected virtual void RegisterFontsFromSettings()
        {
            var settings = LoomSettings.GetOrCreateDefault();
            var fallbacks = new System.Collections.Generic.List<string>();
            foreach (var entry in settings.fonts)
            {
                byte[] bytes = LoadFontBytes(entry);
                if (bytes != null)
                    _stage.RegisterFont(entry.familyName, bytes, entry.isDefault);
                // 收集 isFallback 的 family（即使 bytes 加载失败也登记——Rust 端跳过未注册的）。
                if (entry.isFallback && !string.IsNullOrEmpty(entry.familyName))
                    fallbacks.Add(entry.familyName);
            }
            // 所有字体注册完再设回退链（family 须已 register）。
            if (fallbacks.Count > 0)
                _stage.SetFallbackFamilies(fallbacks);
        }

        /// <summary>
        /// 默认直读 {pkgOutputDir}/fonts/{sourceFileName}.bytes。
        /// v10：不再加载 Unity Font asset——核心自产 atlas，后端只喂 Rust 字节。
        /// 项目覆写换 AssetBundle/Addressables（build 后 Font asset 不在文件系统）。
        /// public 以便跨程序集（如 LoomGUI.Demo）直接调用。
        /// 返 null 表示加载失败，调用方（RegisterFontsFromSettings）跳过此 entry。
        /// </summary>
        public virtual byte[] LoadFontBytes(FontEntry entry)
        {
            string bytesPath = Path.Combine(BundlesSubDir("fonts"), entry.sourceFileName + ".bytes");
            return File.Exists(bytesPath) ? File.ReadAllBytes(bytesPath) : null;
        }

        /// <summary>
        /// 默认直读 {pkgOutputDir}/ui/{name}.pkg.bin。项目覆写换 AB/Addressables。
        /// public 以便跨程序集调用。返 null = 文件不存在/读取失败。
        /// </summary>
        public virtual byte[] LoadPackageBytes(string name)
        {
            string path = Path.Combine(BundlesSubDir("ui"), name + ".pkg.bin");
            return File.Exists(path) ? File.ReadAllBytes(path) : null;
        }

        // ===== Self-drawn atlas bridge (T14: T15 will override for AB/Addressables) =====

        /// <summary>
        /// Build AtlasManifest list from loom.runtime.json's atlas list.
        /// Reads {pkgDir}/loom.runtime.json → parse → for each atlas name read {pkgDir}/atlas/{name}.atlas.json.
        /// Returns empty list if loom.runtime.json is missing or parsing fails.
        /// </summary>
        static List<AtlasManifest> BuildAtlasManifests(LoomSettings settings)
        {
            var result = new List<AtlasManifest>();
            if (settings == null) return result;

            string pkgDir = settings.pkgOutputDir;
            string runtimePath = Path.Combine(pkgDir, "loom.runtime.json");
            if (!File.Exists(runtimePath)) return result;

            RuntimeManifest runtime;
            try { runtime = RuntimeManifest.ParseRuntime(File.ReadAllText(runtimePath)); }
            catch (System.Exception e)
            {
                Debug.LogWarning($"[LoomStageDriver] Failed to parse loom.runtime.json: {e.Message}");
                return result;
            }

            foreach (var atlasName in runtime.atlases)
            {
                string jsonPath = Path.Combine(pkgDir, "atlas", atlasName + ".atlas.json");
                if (!File.Exists(jsonPath))
                {
                    Debug.LogWarning($"[LoomStageDriver] atlas.json not found: {jsonPath}");
                    continue;
                }
                try { result.Add(AtlasManifest.ParseAtlas(File.ReadAllText(jsonPath))); }
                catch (System.Exception e)
                {
                    Debug.LogWarning($"[LoomStageDriver] Failed to parse {jsonPath}: {e.Message}");
                }
            }
            return result;
        }

        /// <summary>
        /// Load an atlas page PNG from {pkgDir}/atlas/{pageFileName}.
        /// Editor only — reads from file system. T15 will override for AB/Addressables.
        /// Returns null if the file doesn't exist or load fails.
        /// </summary>
        static Texture2D LoadAtlasPage(string pageFileName)
        {
            string pkgDir = LoomSettings.GetOrCreateDefault().pkgOutputDir;
            string path = Path.Combine(pkgDir, "atlas", pageFileName);
            if (!File.Exists(path)) return null;
            try
            {
                var tex = new Texture2D(2, 2);
                tex.LoadImage(File.ReadAllBytes(path));
                return tex;
            }
            catch (System.Exception e)
            {
                Debug.LogWarning($"[LoomStageDriver] Failed to load atlas page {path}: {e.Message}");
                return null;
            }
        }

        // deprecated — kept for source compatibility with subclasses that override it.
        // T15 will remove this and replace with a BuildAtlasManifests / LoadAtlasPage override pattern.
        /// <summary>
        /// Deprecated. With standalone packer, loading happens via BuildAtlasManifests + LoadAtlasPage.
        /// Kept virtual for backward source compatibility; T15 will remove.
        /// </summary>
        public virtual object LoadSpriteAtlas(string atlasName)
        {
            Debug.LogWarning("[LoomStageDriver] LoadSpriteAtlas is deprecated — atlas loading is now via BuildAtlasManifests + LoadAtlasPage. T15 will remove.");
            return null;
        }

        // 拼 Bundles 子目录绝对路径。pkgOutputDir 是相对工程根的 "Assets/Bundles" 形式；
        // 去 "Assets/" 前缀后相对 Assets/，与 Application.dataPath（已含 .../Assets）拼成绝对路径。
        static string BundlesSubDir(string sub)
        {
            string pkgDir = LoomSettings.GetOrCreateDefault().pkgOutputDir;
            if (pkgDir.StartsWith("Assets/")) pkgDir = pkgDir.Substring("Assets/".Length);
            return Path.Combine(Application.dataPath, pkgDir, sub);
        }

        void LateUpdate()
        {
            if (_stage == null) return;

            // 屏幕 resize 检测（editor 改 Game 视图尺寸 / player 改窗口）→ 重配根变换 + 相机 orthoSize。
            if (Screen.width != _lastScreenW || Screen.height != _lastScreenH)
            {
                _lastScreenW = Screen.width;
                _lastScreenH = Screen.height;
                ConfigureTransforms();
            }

            // 输入采集 → set_input/set_key_input/set_wheel_input（tick 前——input 管线消费本帧输入产事件）。
            if (_inputCollector != null)
            {
                _inputCollector.Collect(_stage.StagePtr, _designSize, _safeArea);
                _inputCollector.CollectKeys(_stage.StagePtr);
                LoomInputCollector.CollectWheel(_stage);
            }

            // tick → borrow_frame → MirrorPool.Sync → NativeHost.Sync → 事件派发（全在 stage.Tick 内）。
            // unscaledDeltaTime：暂停不受影响（与 UI 时间语义一致）。
            _stage.Tick(Time.unscaledDeltaTime);
        }

        /// <summary>on-screen FPS 读数（_showFps=true 时显示）。1/Time.smoothDeltaTime 平滑帧率。</summary>
        void OnGUI()
        {
            if (!_showFps) return;
            float fps = Time.smoothDeltaTime > 0f ? 1f / Time.smoothDeltaTime : 0f;
            GUI.Label(new Rect(8f, 8f, 240f, 24f), $"FPS {fps:F1}");
        }

        void OnDestroy()
        {
            if (_stage != null)
            {
                _stage.Dispose();
                _stage = null;
            }
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
                    var t = System.Type.GetType("UnityEngine.Rendering.Universal.UniversalAdditionalCameraData, Unity.RenderPipelines.Universal.Runtime");
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
            Rect area = _safeArea ? Screen.safeArea : new Rect(0, 0, sw, sh);
            // 防御：safeArea 可能零宽高（编辑器未配屏）→ 退回全屏
            if (area.width <= 0f || area.height <= 0f) area = new Rect(0, 0, sw, sh);
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
