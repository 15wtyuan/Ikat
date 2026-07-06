using System.Collections.Generic;
using UnityEngine;

namespace LoomGUI
{
    // showcase driver：layer 骨架 + web 式多页导航 + 按页 listener 清 + tips 叠加。
    //
    // 模型（spec §7.3-7.5）：
    //   Start 建 root + ui_layer（主界面层）+ tips_layer（tips 层，在上）；
    //   LoadPackage("showcase", bytes) 进资源池；Instantiate("showcase","home") 挂 ui_layer。
    //   home nav 按钮 → OpenPage(目标页)：清当前页 listener + RemoveNode(当前页) + Instantiate(目标) + 挂 ui_layer + SubscribePage。
    //   各页 back-home → OpenPage("home")。tips_toast 演示 → Instantiate 挂 tips_layer + 定时 RemoveNode。
    //   dyn-load-mail → Instantiate("showcase","mail") 挂 ui_layer（叠加，非切包）；dyn-load-showcase → RemoveNode(mail)。
    //
    // 按页 listener 清（§7.5）：driver 维护当前页 listener 注册表，切页前批量 RemoveListener（不用 EventHandler.Clear 粗清）。
    //
    // 注：本 driver 依赖 LoomStageDriver 提供 LoomStage 实例 + 生命周期管理（Awake/OnDestroy/Tick）。
    // 字体/UI相机/设计分辨率/根变换等基础设施由 LoomStageDriver 统一管理，showcase 专用字段留在本类。
    public unsafe class LoomShowcaseDriver : MonoBehaviour
    {
        // LoomStageDriver 宿主（Inspector 拖同 GO 或场景内任意 GO 上的 LoomStageDriver）。
        // Awake 时从此取 LoomStage 引用；driver 负责 stage 的构造/销毁/Tick。
        [SerializeField] LoomStageDriver _driver;
        // LoomStage 纯类实例（从 _driver.Stage 缓存，非 SerializeField——Unity 不序列化非 Object 引用）。
        LoomStage _stage;
        // page_controls §1.6 NativeHost 演示：Inspector 拖 prefab（driver Instantiate 缓存实例后绑，
        // 不直接绑 prefab asset——Unity 禁改 prefab transform，Bind 也不该越界实例化）。
        [SerializeField] GameObject _nativeModel;
        // Cube 1m³ 在 UI design 空间天然小，设 scale 放大填 slot（NativeHost Sync 不动用户 GO scale）。
        [SerializeField] Vector3 _nativeScale = new Vector3(120, 120, 120);

        // _nativeModel 的缓存实例（Instantiate 一次，跨页复用，Unbind 只 SetActive(false)）。
        GameObject _nativeModelInstance;

        // === page_nativehost：3D 角色 + 粒子（NativeHost 压测）===
        [SerializeField] GameObject _characterPrefab;       // animatedman 角色 prefab（Inspector 拖）
        [SerializeField] GameObject _effectPrefab;          // Kenney Magic/Fire prefab（Inspector 拖)
        // 角色缩放（仅 xy——z 由 driver 固定 1：正交相机 z 缩放不影响视觉大小，只放大 depth 厚度，
        // z 大了会超相机 near/far 被 near clip 切掉。故 z 不暴露给用户填）。
        [SerializeField] Vector2 _characterScale = new Vector2(70, 70);
        // 角色在 slot 的本地偏移（wrapper origin = slot 左上角；通常用 _characterAnchor 自动对齐，
        // 此字段留作微调）。
        [SerializeField] Vector2 _characterLocalOffset = new Vector2(0, 0);
        // 粒子缩放（仅 xy，独立于角色）：ParticleSystem startSize 是世界单位，不完全随父 GO scale 放大，
        // 故角色 scale 不等比放大粒子——粒子需独立 _effectScale。z 同角色由 driver 固定 1。
        [SerializeField] Vector2 _effectScale = new Vector2(120, 120);
        [SerializeField] Vector2 _effectLocalOffset = new Vector2(0, 0);
        [SerializeField] Animator _characterAnimator;       // 角色 Animator（留空则自动从实例 GetComponent 取；勿拖 prefab 的 Animator——那是模板非运行实例）
        [SerializeField] string[] _animStates = { "Idle", "Walk", "Run" };   // Animator state 名（按实际 clip 填；可空）

        // 角色 + 粒子 child 缓存实例：跨页存活只 Instantiate 一次。
        // 离开页 Unbind 只 SetActive(false) 不销毁 → 复用同一 GO，避免反复进出页堆积。
        GameObject _characterInstance;
        int _animIdx;
        bool _effectOn = true;
        // 角色 anchor（0-1，相对 nh-stage layout rect）：(0.5,1)=底中（角色 fbx origin 通常脚底 →
        // 脚底自动落 slot 底中）。driver 在 Update 查 nh-stage rect 自动算 localPosition，不用手填 offset。
        [SerializeField] Vector2 _characterAnchor = new Vector2(0.5f, 1f);
        uint _nhStageId = uint.MaxValue;
        bool _anchorApplied;

        // layer 骨架 NodeId
        uint _root = uint.MaxValue;
        uint _uiLayer = uint.MaxValue;
        uint _tipsLayer = uint.MaxValue;

        // 当前页根 NodeId（home 初始）；uint.MaxValue = 未建。
        uint _currentPage = uint.MaxValue;
        // mail 叠加层 NodeId（dyn-load-mail instantiate 出的 mail 组件根；uint.MaxValue = 未挂）。
        uint _mailOverlay = uint.MaxValue;

        // NativeHost 绑定的 UI 节点 NodeId（page_controls 的 model-slot）。
        // 离开 page_controls 时须 UnbindNativeHost 摘 wrapper GO，否则 _nhm._bindings 仍持被删 NodeId
        // （已 RemoveNode、gen++ 失效）→ Sync 查 blob 找不到 → wrapper 停在末位、GO 视觉残留。
        // uint.MaxValue = 当前页未绑 NativeHost。
        uint _nativeBoundNode = uint.MaxValue;

        // showcase 包名（LoadPackage 用）。
        const string ShowcasePkg = "showcase";

        // === 按页 listener 注册表（§7.5）===
        // 当前页注册的 listener：nodeId → [(eventType, callback)]。切页前遍历逐个 RemoveListener。
        readonly Dictionary<uint, List<(EventType type, EventCallback cb)>> _pageListeners = new();
        // 当前页注册的 Controller 切页 listener：(mountNode, callback) 列表。切页前遍历逐个 RemoveControllerChangedListener。
        readonly List<(uint mount, ControllerChangedCallback cb)> _controllerListeners = new();

        // === 灯阵计数（page_interact）===
        int _clickCount, _hoverCount, _dragCount, _longCount, _keyCount;
        int _routeOuterCount, _routeInnerCount, _routePeCount;
        int _completeCount;

        // lamp 组当前已亮盏数（0..N 循环重置）。key=灯组 name。
        readonly Dictionary<string, int> _lampLit = new();
        static int LampCount(string name) => name switch {
            "click" => 6, "hover" => 4, "drag" => 4, "key" => 8,
            "complete" => 3, "longpress" => 3,
            "outer" => 3, "inner" => 3, "pe" => 3,
            _ => 3
        };

        // === tween 演示（page_tween）===
        // Ease 0..9 与 Rust tween::Ease 对齐（OnEasePlay 取子集对比）。六 prop 在 OnTweenPlay 逐个硬编码 PlayProp。
        static readonly Ease[] _allEase = { Ease.Linear, Ease.QuadIn, Ease.QuadOut, Ease.QuadInOut, Ease.CubicIn, Ease.CubicOut, Ease.CubicInOut, Ease.BackIn, Ease.BackOut, Ease.BackInOut };
        const uint TagComplete = 7;   // complete 回调用 tag
        const uint TagKillSpin = 8;   // kill-target 持续旋转 loop 标识

        // === 动态树演示（page_dyntree §3.10）===
        // dyn-anchor 是 pkg 里的空容器；点击 dyn-add 运行时 create_node 建 panel+title+icon 挂到 anchor。
        // _dynPanels 记已建 panel NodeId 栈，dyn-del remove 最后一个。
        uint _dynAnchor = uint.MaxValue;
        readonly List<uint> _dynPanels = new();
        int _dynSeq;
        bool _dynStyleToggled;   // toggle 末个 panel 样式状态

        // === tips 叠加演示 ===
        // Coroutine 计时器句柄（防重复触发叠多个 toast）。
        Coroutine _tipsRoutine;

        // === Controller 演示（page_controller）===
        // tab-root 的 Controller 挂载 NodeId（GetController 返回）。driver 缓存后用 handle 调 SetSelectedIndex。
        uint _tabMount = uint.MaxValue;
        uint _dialogMount = uint.MaxValue;
        uint _biconMount = uint.MaxValue;
        uint _otabMount = uint.MaxValue;
        uint _itabMount = uint.MaxValue;
        // bicon 当前页（路 B 演示：点"下一个"循环切 0→1→2→0）
        int _biconIdx;

        // === 虚拟列表演示===
        bool _isListPage;
        VirtualListDriver _listDriverEqual;
        VirtualListDriver _listDriverVar;

        void Awake()
        {
            // 从 LoomStageDriver 取 stage 引用（driver 负责构造/初始化/Tick/销毁）。
            if (_driver == null) _driver = GetComponent<LoomStageDriver>();
            if (_driver == null)
            {
                Debug.LogError("[Showcase] 无 LoomStageDriver——请在 Inspector 指定或挂同 GO");
                return;
            }
            _stage = _driver.Stage;
            if (_stage == null)
            {
                Debug.LogError("[Showcase] Driver.Stage 未建——LoomStageDriver.Awake 可能未跑");
                return;
            }
        }

        // #1a1d2e = .root 背景色（showcase 深蓝底）。主相机配同色，letterbox 与 root 无缝。
        static readonly Color RootBg = new Color(26f / 255f, 29f / 255f, 46f / 255f, 1f);

        void Start()
        {
            if (_stage == null) return;
            ConfigureCameraBackground();

            // layer 骨架：root + ui_layer（主界面层）+ tips_layer（tips 层，在上）。
            _root = _stage.CreateRoot("div", "width:1080px;height:1920px;background-color:#1a1d2e;flex-direction:column");
            _uiLayer = _stage.CreateNode("div", "flex-grow:1");
            _tipsLayer = _stage.CreateNode("div", "position:absolute;top:0;left:0;width:100%;height:100%;flex-direction:column;align-items:center;justify-content:flex-end;padding:40px;pointer-events:none");
            _stage.AppendChild(_root, _uiLayer);
            _stage.AppendChild(_root, _tipsLayer);

            // load showcase 包进资源池（不建 scene）。通过 LoomStageDriver 的统一加载钩子读 pkg 字节。
            byte[] pkgBytes = _driver.LoadPackageBytes(ShowcasePkg);
            if (pkgBytes == null)
            {
                Debug.LogError($"[Showcase] 无法加载 {ShowcasePkg}——showcase 不显示");
                return;
            }
            int r = _stage.LoadPackage(ShowcasePkg, pkgBytes);
            if (r != 0)
            {
                Debug.LogError($"[Showcase] LoadPackage({ShowcasePkg}) 失败 rc={r}");
                return;
            }

            OpenPage("home");
        }

        // 主相机默认 Skybox；root shrink-to-fit + safeArea letterbox 后透出 → 整体灰蒙蒙。
        // LoomUICamera clearFlags=Depth 不清色、叠在主相机上。改主相机纯色 = root bg，letterbox 统一深色。
        void ConfigureCameraBackground()
        {
            var cam = Camera.main;
            if (cam != null)
            {
                cam.clearFlags = CameraClearFlags.SolidColor;
                cam.backgroundColor = RootBg;
            }
        }

        // === 导航跳页（模型 2 web 式，§7.3）===
        // OpenPage: 清当前页 listener + RemoveNode(当前页) + Instantiate(目标页) + 挂 ui_layer + SubscribePage。
        // home 也是页（ui_layer 整层换）。各页 back-home → OpenPage("home")。
        void OpenPage(string page)
        {
            if (_currentPage != uint.MaxValue)
            {
                // 若离开的页绑了 NativeHost，先 Unbind 摘 wrapper GO（RemoveNode 后被删 NodeId gen++ 失效，
                // _nhm._bindings 残留 → Sync 查不到 → wrapper 卡末位、GO 视觉残留）。
                if (_nativeBoundNode != uint.MaxValue)
                {
                    _stage.UnbindNativeHost(_nativeBoundNode);
                    _nativeBoundNode = uint.MaxValue;
                }
                ClearPageListeners();              // 按页清 listener（§7.5）
                _stage.RemoveNode(_currentPage);   // 摘当前页（联动清 anim/scroll/tween/focused_node）
                _currentPage = uint.MaxValue;
            }
            // 切页时若 mail 叠加层还挂着，一并摘（mail 属于 page_dyntree 的演示，切走 dyntree 页时清理）。
            if (_mailOverlay != uint.MaxValue)
            {
                _stage.RemoveNode(_mailOverlay);
                _mailOverlay = uint.MaxValue;
            }
            // 离开列表页时清 driver（RemoveNode 递归清所有 slot，driver 下帧 Update 不跑）。
            _isListPage = false;
            _listDriverEqual = null;
            _listDriverVar = null;
            uint node = _stage.Instantiate(ShowcasePkg, page);
            if (node == uint.MaxValue)
            {
                Debug.LogError($"[Showcase] Instantiate({ShowcasePkg}, {page}) 失败");
                return;
            }
            _currentPage = node;
            _stage.AppendChild(_uiLayer, node);
            SubscribePage(page);
            Debug.Log($"[Showcase] OpenPage({page}) → node={node}");
        }

        // === 按页 listener 注册表（§7.5）===
        // AddPageListener: AddListener + 记进 _pageListeners（切页前 ClearPageListeners 批量 RemoveListener）。
        void AddPageListener(uint node, EventType type, EventCallback cb)
        {
            if (node == uint.MaxValue) return;
            _stage.EventHandler.AddListener(node, type, cb);
            if (!_pageListeners.TryGetValue(node, out var list))
                _pageListeners[node] = list = new List<(EventType, EventCallback)>();
            list.Add((type, cb));
        }

        // ClearPageListeners: 遍历当前页注册的 listener 逐个 RemoveListener（不用 EventHandler.Clear 粗清）。
        // 切页前调（OpenPage 里 RemoveNode 之前）。RemoveNode 后被删 NodeId 失效，listener 成悬空条目——故须先清。
        void ClearPageListeners()
        {
            foreach (var kv in _pageListeners)
            {
                uint node = kv.Key;
                foreach (var (type, cb) in kv.Value)
                    _stage.EventHandler.RemoveListener(node, type, cb);
            }
            _pageListeners.Clear();
            // Controller 切页 listener 同理：切页前逐个摘（mount_node 即将被 RemoveNode gen++ 失效）。
            foreach (var (mount, cb) in _controllerListeners)
                _stage.EventHandler.RemoveControllerChangedListener(mount, cb);
            _controllerListeners.Clear();
            // 重置 Controller handle（下页 SubscribeController 重填）。
            _tabMount = _dialogMount = _biconMount = _otabMount = _itabMount = uint.MaxValue;
        }

        // === 按页订阅（SubscribePage）===
        // switch page → 调对应 SubscribeXxx（每页一组，lamp/tween/dyntree 各自的订阅逻辑）。
        // 每订阅走 AddPageListener（记进注册表），切页时批量清。
        void SubscribePage(string page)
        {
            // 页面切换时清灯组状态 + 计数（页面 fresh instantiate，旧 _lampLit/计数会与视觉脱节）。
            _lampLit.Clear();
            _clickCount = _hoverCount = _dragCount = _longCount = _keyCount = 0;
            _routeOuterCount = _routeInnerCount = _routePeCount = 0;
            _completeCount = 0;
            switch (page)
            {
                case "home": SubscribeHome(); break;
                case "page_controls": SubscribeControls(); break;
                case "page_text": SubscribeText(); break;
                case "page_image": SubscribeImage(); break;
                case "page_scroll": SubscribeScroll(); break;
                case "page_tween": SubscribeTween(); break;
                case "page_interact": SubscribeInteract(); break;
                case "page_dyntree": SubscribeDynTree(); break;
                case "page_list": SubscribeList(); break;
                case "page_nativehost": SubscribeNativeHost(); break;
                case "page_controller": SubscribeController(); break;
            }
        }

        // home：订阅各 nav 按钮 → OpenPage(目标页) + nav-tips-demo → ShowTips。
        // nav-* id 与 home.html 一致（nav-controls/nav-text/nav-image/nav-scroll/nav-tween/nav-interact/nav-dyntree/nav-tips-demo）。
        void SubscribeHome()
        {
            AddNavListener("nav-controls", "page_controls");
            AddNavListener("nav-text", "page_text");
            AddNavListener("nav-image", "page_image");
            AddNavListener("nav-scroll", "page_scroll");
            AddNavListener("nav-tween", "page_tween");
            AddNavListener("nav-interact", "page_interact");
            AddNavListener("nav-dyntree", "page_dyntree");
            AddNavListener("nav-list", "page_list");
            AddNavListener("nav-nativehost", "page_nativehost");
            AddNavListener("nav-controller", "page_controller");
            // nav-tips-demo → 弹 tips_toast 演示（tips_layer 叠加）。
            uint tipsBtn = _stage.FindNodeById("nav-tips-demo");
            AddPageListener(tipsBtn, EventType.Click, _ => ShowTips());
            Debug.Log("[Showcase] home 订阅完成（9 nav + tips-demo）");
        }

        void AddNavListener(string navId, string targetPage)
        {
            uint n = _stage.FindNodeById(navId);
            AddPageListener(n, EventType.Click, _ => OpenPage(targetPage));
        }

        // 各页通用：back-home 按钮 → OpenPage("home")。
        // 所有 page_*.html 都有 id="back-home"（语义 id 复用）。
        void SubscribeBackHome()
        {
            uint back = _stage.FindNodeById("back-home");
            AddPageListener(back, EventType.Click, _ => OpenPage("home"));
        }

        // page_controls：back-home + btn-demo-disabled 禁用 + model-slot NativeHost 绑定。
        void SubscribeControls()
        {
            SubscribeBackHome();
            uint dbd = _stage.FindNodeById("btn-demo-disabled");
            if (dbd != uint.MaxValue) _stage.SetNodeDisabled(dbd, true);
            // NativeHost：Instantiate _nativeModel 缓存实例后绑 model-slot（每帧 Sync 自动同步 wrapper TRS）。
            // 不直接绑 prefab asset——Bind 契约是场景实例（fgui GoWrapper 同款：caller 管实例化，框架只显示）。
            EnsureNativeModelInstance();
            if (_nativeModelInstance != null)
            {
                uint slot = _stage.FindNodeById("model-slot");
                if (slot != uint.MaxValue)
                {
                    _stage.BindNativeHost(slot, _nativeModelInstance);
                    _nativeBoundNode = slot;   // 记下，离开页时 OpenPage 的 Unbind 摘 wrapper GO
                }
                else
                {
                    Debug.LogError("[Showcase] page_controls: id 'model-slot' 未找到，跳过 NativeHost 绑定");
                }
            }
            Debug.Log("[Showcase] page_controls 订阅完成（back + disabled + NativeHost）");
        }

        // _nativeModel 缓存实例（只 Instantiate 一次，跨页复用）。scale 在实例上设（非 prefab asset）。
        void EnsureNativeModelInstance()
        {
            if (_nativeModelInstance != null) return;
            if (_nativeModel == null)
            {
                Debug.LogError("[Showcase] _nativeModel 未配，page_controls NativeHost 不显示");
                return;
            }
            _nativeModelInstance = Instantiate(_nativeModel);
            _nativeModelInstance.transform.localScale = _nativeScale;
            _nativeModelInstance.SetActive(false);
        }

        // page_nativehost：back-home + 角色/粒子 NativeHost 绑定 + 放光效/切动画按钮。
        void SubscribeNativeHost()
        {
            SubscribeBackHome();
            EnsureCharacterInstance();
            uint stage = _stage.FindNodeById("nh-stage");
            _nhStageId = stage;
            _anchorApplied = false;   // 每次进页 re-anchor（nh-stage 新 instantiate，rect 新算）
            if (_characterInstance != null && stage != uint.MaxValue)
            {
                _stage.BindNativeHost(stage, _characterInstance);
                _nativeBoundNode = stage;   // 记下，离开页时 OpenPage 的 Unbind 摘 wrapper GO
            }
            else if (stage == uint.MaxValue) Debug.LogError("[Showcase] page_nativehost: id 'nh-stage' 未找到（pkg 未含 page_nativehost.html？），跳过 NativeHost 绑定");
            SubscribeLamp("nh-effect", EventType.Click, OnNhEffect);
            SubscribeLamp("nh-anim", EventType.Click, OnNhAnim);
            Debug.Log("[Showcase] page_nativehost 订阅完成（角色+粒子 NativeHost + 按钮）");
        }

        // 角色 + 粒子 child 缓存实例（只建一次）。Instantiate 后立即 SetActive(false)：
        // BindNativeHost 前角色默认 active 会显示在场景原点；藏起来等 wrapper Sync 重新 SetActive(true)。
        void EnsureCharacterInstance()
        {
            if (_characterInstance != null) return;
            if (_characterPrefab == null)
            {
                Debug.LogError("[Showcase] _characterPrefab 未配，page_nativehost 角色不显示");
                return;
            }
            _characterInstance = Instantiate(_characterPrefab);
            // z 固定 1（正交相机 z 缩放只放 depth 厚度，放大了 near clip 切——z 不暴露给用户）
            _characterInstance.transform.localScale = new Vector3(_characterScale.x, _characterScale.y, 1);
            // localPosition 由 ApplyCharacterAnchor 算（anchor + _characterLocalOffset 微调），不在这里设。
            _characterInstance.SetActive(false);
            // Animator 从实例取（prefab 上的 Animator 是模板，对它 Play 无效/报错）。
            // Inspector 留空自动取；若 caller 拖了场景实例的 Animator 则覆盖。
            if (_characterAnimator == null)
                _characterAnimator = _characterInstance.GetComponent<Animator>();
            if (_effectPrefab != null)
            {
                // 粒子挂角色 child；独立 scale（ParticleSystem startSize 不随父 scale 等比放大）+ offset。
                var fx = Instantiate(_effectPrefab, _characterInstance.transform, false);
                fx.transform.localScale = new Vector3(_effectScale.x, _effectScale.y, 1);
                fx.transform.localPosition = new Vector3(_effectLocalOffset.x, _effectLocalOffset.y, 0);
            }
            else Debug.LogWarning("[Showcase] _effectPrefab 未配，page_nativehost 无粒子");
        }

        // 查 nh-stage layout_rect + 按 _characterAnchor 算角色 localPosition（脚底对齐 slot 锚点）。
        // wrapper origin = slot 左上（design）；character 在 wrapper 下，其 local y 反向 design y
        // （_container y-flip——见 NativeHostManager.Sync 注释：wrapper.localPos.y = -Mty）。
        // 故 offset = (anchor.x·w, -anchor.y·h) + _characterLocalOffset（Inspector 微调，默认 0）。
        // 首帧 nh-stage layout 未算（rect.h=0）→ Update 下帧重试（首帧角色在 origin，1 帧闪可接受）。
        void ApplyCharacterAnchor()
        {
            if (_nhStageId == uint.MaxValue) return;
            var (_rx, _ry, w, h) = _stage.GetNodeLayoutRect(_nhStageId);
            if (h <= 0) return;   // layout 未完成
            _characterInstance.transform.localPosition = new Vector3(
                _characterAnchor.x * w + _characterLocalOffset.x,
                -_characterAnchor.y * h + _characterLocalOffset.y,
                0);
            _anchorApplied = true;
        }

        // toggle 角色 child 下的粒子（SetActive + Play/Stop）。
        void OnNhEffect(EventContext ctx)
        {
            if (_characterInstance == null) return;
            // includeInactive=true：粒子可能挂在被 SetActive(false) 的角色子树下。
            var ps = _characterInstance.GetComponentInChildren<ParticleSystem>(true);
            if (ps == null) { Debug.LogWarning("[Showcase] 角色下无 ParticleSystem（_effectPrefab 未配或 prefab 无 ParticleSystem 组件）"); return; }
            _effectOn = !_effectOn;
            if (_effectOn) { ps.gameObject.SetActive(true); ps.Play(); }
            else { ps.Stop(); ps.gameObject.SetActive(false); }
        }

        // 循环切 Animator state（按 Inspector _animStates 填的 clip 名）。
        void OnNhAnim(EventContext ctx)
        {
            if (_characterAnimator == null || _animStates == null || _animStates.Length == 0) return;
            _animIdx = (_animIdx + 1) % _animStates.Length;
            _characterAnimator.Play(_animStates[_animIdx]);
            // 注：若 Animator controller 没建对应 state，会报 "State could not be found"——
            // 在 Unity Animator 窗口把 clip 拖进去建 state（state 名 = clip 名）。
        }

        // page_text：back-home（无其他交互元素，纯展示文本样式）。
        void SubscribeText()
        {
            SubscribeBackHome();
            Debug.Log("[Showcase] page_text 订阅完成（back）");
        }

        // page_controller（v1.5 Controller 演示）：
        // - tab Controller（§1）：点 tab 头 → SetSelectedIndex(tabMount, n)；CSS [data-page] 切 panel 显隐 + opacity transition。
        // - dialog Controller（§2）：开关按钮 → SetSelectedIndex 控弹窗 display。
        // - 路 A src/text（§3）：订阅 tab on_changed → SetSrc/SetText 按 new_index 切图标 + 文案。
        // - 路 B 多元素兄弟（§4）：bicon Controller 点"下一个"循环切 page，CSS [data-page] 切三个 img 兄弟显隐。
        // - 嵌套 Controller（§5）：外层 otab + 内层 itab，driver 各自 SetSelectedIndex，CSS 就近原则互不干扰。
        // - :hover/:active + transition（§6）：纯 CSS，driver 不订阅（回归测试）。
        void SubscribeController()
        {
            SubscribeBackHome();

            // === §1 标签页 ===
            // GetController 在子树内找 data-controller="tab" 的挂载点 → 返 NodeId（handle）。
            // driver 缓存 handle，后续 SetSelectedIndex/订阅 on_changed 都用 handle（不再用全局 name）。
            _tabMount = _stage.GetController(_currentPage, "tab");
            if (_tabMount != uint.MaxValue)
            {
                // 订阅 on_changed（pull 模式：core 派事件 → LoomStage tick 后 DispatchControllerChanged → 回调）。
                // 用 AddControllerChangedListener（mount_node 粒度，无 bubble）。
                // 记进 _controllerListeners（切页前 ClearPageListeners 批量 RemoveControllerChangedListener）。
                _stage.EventHandler.AddControllerChangedListener(_tabMount, OnTabChanged);
                _controllerListeners.Add((_tabMount, OnTabChanged));
            }
            else
            {
                Debug.LogError("[Showcase] page_controller: GetController(tab) 未找到（pkg 未含 data-controller=\"tab\"？）");
            }
            // 三个 tab 头按钮 → SetSelectedIndex(handle, n)。
            AddPageListener(_stage.FindNodeById("tab-btn-0"), EventType.Click, _ => SetTab(0));
            AddPageListener(_stage.FindNodeById("tab-btn-1"), EventType.Click, _ => SetTab(1));
            AddPageListener(_stage.FindNodeById("tab-btn-2"), EventType.Click, _ => SetTab(2));

            // === §2 弹窗显隐 ===
            // dialog Controller：page 0=隐藏, 1=显示。
            _dialogMount = _stage.GetController(_currentPage, "dialog");
            AddPageListener(_stage.FindNodeById("dlg-open"), EventType.Click, _ =>
            {
                if (_dialogMount != uint.MaxValue) _stage.SetSelectedIndex(_dialogMount, 1);
            });
            AddPageListener(_stage.FindNodeById("dlg-close"), EventType.Click, _ =>
            {
                if (_dialogMount != uint.MaxValue) _stage.SetSelectedIndex(_dialogMount, 0);
            });

            // === §3 路 A src/text ===
            // 路径 A 按钮直接调 SetSelectedIndex(tabMount, n)（复用 §1 的 tab Controller）。
            // on_changed 回调 OnTabChanged 里按 new_index 调 SetSrc/SetText。
            AddPageListener(_stage.FindNodeById("route-a-0"), EventType.Click, _ => SetTab(0));
            AddPageListener(_stage.FindNodeById("route-a-1"), EventType.Click, _ => SetTab(1));
            AddPageListener(_stage.FindNodeById("route-a-2"), EventType.Click, _ => SetTab(2));

            // === §4 路 B 多元素兄弟 ===
            // bicon Controller：点"下一个"循环切 0→1→2→0。显隐全由 CSS [data-page] 处理，driver 只切 page。
            _biconMount = _stage.GetController(_currentPage, "bicon");
            _biconIdx = 0;
            AddPageListener(_stage.FindNodeById("bicon-next"), EventType.Click, _ =>
            {
                if (_biconMount == uint.MaxValue) return;
                _biconIdx = (_biconIdx + 1) % 3;
                _stage.SetSelectedIndex(_biconMount, _biconIdx);
                // bicon-label 不受 Controller 管（不在 [data-page] 选择器目标里），driver 手动更新计数。
                uint lbl = _stage.FindNodeById("bicon-label");
                if (lbl != uint.MaxValue) _stage.SetText(lbl, $"图标 {_biconIdx + 1}/3");
            });

            // === §5 嵌套 Controller ===
            // 外层 otab + 内层 itab：各自 GetController + SetSelectedIndex。
            // CSS [data-page] 就近原则：内层 [data-page] 回溯找最近 data-controller="itab" 祖先，
            // 不查 otab → 内层切 page 不影响外层 panel 显隐。
            _otabMount = _stage.GetController(_currentPage, "otab");
            AddPageListener(_stage.FindNodeById("otab-btn-0"), EventType.Click, _ =>
            {
                if (_otabMount != uint.MaxValue) _stage.SetSelectedIndex(_otabMount, 0);
            });
            AddPageListener(_stage.FindNodeById("otab-btn-1"), EventType.Click, _ =>
            {
                if (_otabMount != uint.MaxValue) _stage.SetSelectedIndex(_otabMount, 1);
            });
            _itabMount = _stage.GetController(_currentPage, "itab");
            AddPageListener(_stage.FindNodeById("itab-btn-0"), EventType.Click, _ =>
            {
                if (_itabMount != uint.MaxValue) _stage.SetSelectedIndex(_itabMount, 0);
            });
            AddPageListener(_stage.FindNodeById("itab-btn-1"), EventType.Click, _ =>
            {
                if (_itabMount != uint.MaxValue) _stage.SetSelectedIndex(_itabMount, 1);
            });

            Debug.Log($"[Showcase] page_controller 订阅完成（tab={_tabMount} dialog={_dialogMount} bicon={_biconMount} otab={_otabMount} itab={_itabMount}）");
        }

        // SetTab: 切 tab Controller 选中页。SetSelectedIndex 返 prev（不用），core 派 on_changed 下 tick 回调。
        void SetTab(int n)
        {
            if (_tabMount != uint.MaxValue) _stage.SetSelectedIndex(_tabMount, n);
        }

        // OnTabChanged: §3 路 A src/text 切换。
        // core 派 ControllerChangedEvent(mount_node, prev, new_index) → LoomStage tick 后 Dispatch → 此回调。
        // 按 new_index 调 SetSrc/SetText 切图标 + 文案（CSS 表达不了 src/text，故走 driver 代码路径 A）。
        void OnTabChanged(uint mountNode, int prev, int newIndex)
        {
            // 只响应 tab Controller（dialog/bicon/otab/itab 不走此回调——它们没 AddControllerChangedListener）。
            if (mountNode != _tabMount) return;
            // per-page 图标（路 A src 切换）。
            string[] icons = { "icons/home.png", "icons/eye.png", "icons/zap.png" };
            string[] texts = { "当前页：1 · 概览", "当前页：2 · 详情", "当前页：3 · 设置" };
            int idx = newIndex;
            if (idx < 0 || idx >= icons.Length) idx = 0;
            uint iconNode = _stage.FindNodeById("route-a-icon");
            if (iconNode != uint.MaxValue) _stage.SetSrc(iconNode, icons[idx]);
            uint textNode = _stage.FindNodeById("route-a-text");
            if (textNode != uint.MaxValue) _stage.SetText(textNode, texts[idx]);
        }

        // page_image：back-home（无其他交互元素，纯展示视觉样式）。
        void SubscribeImage()
        {
            SubscribeBackHome();
            Debug.Log("[Showcase] page_image 订阅完成（back）");
        }

        // page_scroll：back-home（外层 page-scroll 自带滚动行为，无需 driver 订阅）。
        void SubscribeScroll()
        {
            SubscribeBackHome();
            uint pageScroll = _stage.FindNodeById("page-scroll");
            AddPageListener(_stage.FindNodeById("scroll-top"), EventType.Click, _ => _stage.SetScrollPos(pageScroll, 0f, 0f));
            AddPageListener(_stage.FindNodeById("scroll-mid"), EventType.Click, _ => _stage.SetScrollPos(pageScroll, 0f, 600f));
            AddPageListener(_stage.FindNodeById("scroll-bottom"), EventType.Click, _ => _stage.SetScrollPos(pageScroll, 0f, 99999f));
            Debug.Log("[Showcase] page_scroll 订阅完成（back + 3 SetScrollPos 按钮）");
        }

        // page_interact（§4 灯阵）：back-home + 各交互元素事件 + disabled + 路由。
        // 走 AddPageListener 记进注册表（切页时批量清）。
        void SubscribeInteract()
        {
            SubscribeBackHome();
            SubscribeLamp("hit-click", EventType.Click, OnClickHit);
            SubscribeLamp("hit-hover", EventType.RollOver, OnHoverHit);
            SubscribeLamp("hit-hover", EventType.RollOut, OnHoverLeave);
            SubscribeLamp("hit-drag", EventType.DragMove, OnDragHit);
            SubscribeLamp("hit-longpress", EventType.LongPress, OnLongHit);
            SubscribeLamp("hit-key", EventType.KeyDown, OnKeyHit);
            uint dn = _stage.FindNodeById("hit-disabled");
            if (dn != uint.MaxValue) _stage.SetNodeDisabled(dn, true);
            // 路由：outer/inner 均订阅 Click；inner 调 StopPropagation 止冒泡（outer 不触发）。
            SubscribeLamp("route-outer", EventType.Click, OnRouteOuter);
            SubscribeLamp("route-inner", EventType.Click, OnRouteInner);
            SubscribeLamp("route-pe-under", EventType.Click, OnRoutePeUnder);
            Debug.Log("[Showcase] page_interact 灯阵订阅完成（click/hover/drag/longpress/key + route + disabled）");
        }

        void SubscribeLamp(string id, EventType t, EventCallback cb)
        {
            uint n = _stage.FindNodeById(id);
            AddPageListener(n, t, cb);
        }

        // 点亮 lamp-{name}-{lit} 第 lit 盏（id-addressable）+ count-{name} 计数。
        // 全亮后下一次触发先灭所有再重新从 0 点亮（循环）。count 显示累计触发次数。
        void LightLamp(string name, int totalCount)
        {
            int n = LampCount(name);
            if (!_lampLit.TryGetValue(name, out int lit)) lit = 0;
            if (lit >= n)
            {
                for (int i = 0; i < n; i++)
                {
                    uint lamp = _stage.FindNodeById($"lamp-{name}-{i}");
                    if (lamp != uint.MaxValue) _stage.SetStyle(lamp, "background-color:#3a3f55");
                }
                lit = 0;
            }
            uint node = _stage.FindNodeById($"lamp-{name}-{lit}");
            if (node != uint.MaxValue) _stage.SetStyle(node, "background-color:#5fb2c4");
            _lampLit[name] = lit + 1;
            uint countNode = _stage.FindNodeById("count-" + name);
            if (countNode != uint.MaxValue) _stage.SetText(countNode, totalCount.ToString());
        }

        // click + dblclick：双击额外多亮一盏（用 acc 色标记）。
        void OnClickHit(EventContext ctx)
        {
            LightLamp("click", ++_clickCount);
            if (ctx.isDoubleClick) LightLamp("click", ++_clickCount);
        }
        void OnHoverHit(EventContext ctx) { LightLamp("hover", ++_hoverCount); }
        void OnHoverLeave(EventContext ctx) { LightLamp("hover", ++_hoverCount); }
        void OnDragHit(EventContext ctx) { LightLamp("drag", ++_dragCount); }
        void OnLongHit(EventContext ctx) { LightLamp("longpress", ++_longCount); }
        void OnKeyHit(EventContext ctx) { LightLamp("key", ++_keyCount); }

        // 路由演示：inner StopPropagation → outer 不收。独立 lamp-route 反馈。
        void OnRouteOuter(EventContext ctx) { LightLamp("outer", ++_routeOuterCount); }
        void OnRouteInner(EventContext ctx)
        {
            ctx.StopPropagation();
            LightLamp("inner", ++_routeInnerCount);
        }
        void OnRoutePeUnder(EventContext ctx) { LightLamp("pe", ++_routePeCount); }

        // page_tween（§7 动效）：back-home + tween 播放/kill/clear + complete 回调 + kill-target 旋转。
        // 走 AddPageListener 记进注册表。
        void SubscribeTween()
        {
            SubscribeBackHome();
            SubscribeLamp("tween-play", EventType.Click, OnTweenPlay);
            SubscribeLamp("ease-play", EventType.Click, OnEasePlay);
            SubscribeLamp("delay-play", EventType.Click, OnDelayPlay);
            SubscribeLamp("complete-play", EventType.Click, OnCompletePlay);
            SubscribeLamp("kill-btn", EventType.Click, OnKill);
            SubscribeLamp("clear-btn", EventType.Click, OnClear);
            // t-opacity 的 TweenComplete（core 完成时直派，ctx.clickCount=prop、ctx.touchId=tag）。
            SubscribeLamp("t-opacity", EventType.TweenComplete, OnTweenCompleteTag);
            // kill-target：点"播放"才开始持续旋转（loop 靠 TweenComplete 重启）。
            SubscribeLamp("play-kill-target", EventType.Click, OnPlayKillTarget);
            SubscribeLamp("kill-target", EventType.TweenComplete, OnKillTargetLoop);
            Debug.Log("[Showcase] page_tween 订阅完成（play/ease/delay/complete/kill/clear + play-kill-target loop）");
        }

        void OnPlayKillTarget(EventContext ctx)
        {
            PlayProp("kill-target", TweenProp.Rotation, new float[] { 0f, 0, 0, 0 }, new float[] { 360f, 0, 0, 0 }, 4f, Ease.Linear, 0f, TagKillSpin);
        }
        void OnKillTargetLoop(EventContext ctx)
        {
            if (ctx.touchId == TagKillSpin)
                PlayProp("kill-target", TweenProp.Rotation, new float[] { 0f, 0, 0, 0 }, new float[] { 360f, 0, 0, 0 }, 4f, Ease.Linear, 0f, TagKillSpin);
        }

        void PlayProp(string id, TweenProp prop, float[] s, float[] e, float dur, Ease ease, float delay, uint tag)
        {
            uint n = _stage.FindNodeById(id);
            if (n != uint.MaxValue) _stage.Tween(n, prop, s, e, dur, ease, delay, tag);
        }

        // 六属性同放：opacity / translate / scale / rotation / bg-color / text-color。
        void OnTweenPlay(EventContext ctx)
        {
            PlayProp("t-opacity", TweenProp.Opacity, new float[] { 0f, 0, 0, 0 }, new float[] { 1f, 0, 0, 0 }, 0.8f, Ease.Linear, 0f, 0);
            PlayProp("t-translate", TweenProp.Translate, new float[] { -40f, 0, 0, 0 }, new float[] { 40f, 0, 0, 0 }, 0.8f, Ease.CubicInOut, 0f, 0);
            PlayProp("t-scale", TweenProp.Scale, new float[] { 0.5f, 0.5f, 0, 0 }, new float[] { 1.4f, 1.4f, 0, 0 }, 0.8f, Ease.BackOut, 0f, 0);
            PlayProp("t-rotate", TweenProp.Rotation, new float[] { 0f, 0, 0, 0 }, new float[] { 360f, 0, 0, 0 }, 0.8f, Ease.QuadInOut, 0f, 0);
            // 颜色 tween：Rust anim 通道是归一化 [0,1] RGBA（style/mapping.rs /255.0），故 float[] 也须归一化。
            PlayProp("t-bgcolor", TweenProp.BgColor, Rgba(0x5f, 0xb2, 0xc4), Rgba(0x6f, 0xa6, 0x6c), 0.8f, Ease.Linear, 0f, 0);
            PlayProp("t-textcolor", TweenProp.TextColor, Rgba(0xe6, 0xe6, 0xe0), Rgba(0xc2, 0x60, 0x5a), 0.8f, Ease.Linear, 0f, 0);
        }

        // 三条 ease 对比（QuadIn / CubicOut / BackInOut），同 translate 200px。
        void OnEasePlay(EventContext ctx)
        {
            int[] pick = { 1, 5, 9 };
            for (int i = 0; i < pick.Length; i++)
                PlayProp("ease-" + i, TweenProp.Translate, new float[] { 0f, 0, 0, 0 }, new float[] { 200f, 0, 0, 0 }, 1.0f, _allEase[pick[i]], 0f, 0);
        }

        // delay 错峰：三块依次起。
        void OnDelayPlay(EventContext ctx)
        {
            PlayProp("d-0", TweenProp.Opacity, new float[] { 0f, 0, 0, 0 }, new float[] { 1f, 0, 0, 0 }, 0.5f, Ease.CubicOut, 0f, 0);
            PlayProp("d-1", TweenProp.Opacity, new float[] { 0f, 0, 0, 0 }, new float[] { 1f, 0, 0, 0 }, 0.5f, Ease.CubicOut, 0.2f, 0);
            PlayProp("d-2", TweenProp.Opacity, new float[] { 0f, 0, 0, 0 }, new float[] { 1f, 0, 0, 0 }, 0.5f, Ease.CubicOut, 0.4f, 0);
        }

        // complete：t-opacity 跑完后 core 派 TweenComplete（tag=TagComplete），C# 识别 tag 亮灯。
        void OnCompletePlay(EventContext ctx)
        {
            PlayProp("t-opacity", TweenProp.Opacity, new float[] { 1f, 0, 0, 0 }, new float[] { 0.2f, 0, 0, 0 }, 0.6f, Ease.QuadIn, 0f, TagComplete);
        }
        void OnTweenCompleteTag(EventContext ctx)
        {
            if (ctx.touchId == TagComplete) LightLamp("complete", ++_completeCount);
        }

        // kill 冻结当前角（停在末值）；clear 清所有 anim 回 CSS 初始。
        void OnKill(EventContext ctx) { _stage.KillTween(_stage.FindNodeById("kill-target"), TweenProp.Rotation); }
        void OnClear(EventContext ctx) { _stage.ClearAnim(_stage.FindNodeById("kill-target")); }

        // page_dyntree（§3.10）：back-home + 建/删/批量/set_style + dyn-load-mail/showcase。
        // 走 AddPageListener 记进注册表；dyn-load-* 用 instantiate/remove（非切包）。
        void SubscribeDynTree()
        {
            SubscribeBackHome();
            _dynAnchor = _stage.FindNodeById("dyn-anchor");
            SubscribeLamp("dyn-add", EventType.Click, OnDynAdd);
            SubscribeLamp("dyn-add20", EventType.Click, OnDynAdd20);
            SubscribeLamp("dyn-del", EventType.Click, OnDynDel);
            SubscribeLamp("dyn-clear", EventType.Click, OnDynClear);
            SubscribeLamp("dyn-style", EventType.Click, OnDynStyle);
            // dyn-load-mail → instantiate("showcase","mail") 挂 ui_layer（叠加，非切包）。
            // dyn-load-showcase → remove mail（摘邮件叠加）。
            SubscribeLamp("dyn-load-mail", EventType.Click, OnDynLoadMail);
            SubscribeLamp("dyn-load-showcase", EventType.Click, OnDynLoadShowcase);
            Debug.Log($"[Showcase] page_dyntree 订阅完成（anchor={_dynAnchor}）");
        }

        // page_list（虚拟列表）：back-home + 左右双列表（等高 + 不等高）。
        void SubscribeList()
        {
            SubscribeBackHome();
            uint eq = _stage.FindNodeById("list-equal");
            uint vr = _stage.FindNodeById("list-variable");
            if (eq == uint.MaxValue || vr == uint.MaxValue)
            {
                Debug.LogError("[Showcase] list containers not found (id 'list-equal'/'list-variable')");
                return;
            }
            _listDriverEqual = new VirtualListDriver(_stage, eq, 1000);
            // 不等高尺寸：正弦波 60~140px
            // 本 demo 不等高用预定义 size（sin 波），size 已知无需 spec §2.8 实测补偿回路。
            // 真实数据源（异步图片加载致 item 高度变）场景需补 MeasureAndUpdateSlot + SetScrollPos 补偿防跳动。
            // reuseBase=100000：双列表 reuse_key 独占段（等高用默认 0 段 1..N，不等高用 100000 段），
            // 防 MirrorPool _poolByReuse 跨列表撞车（见 VirtualListDriver 注释）。
            float[] sizes = new float[200];
            for (int i = 0; i < 200; i++)
                sizes[i] = 100f + 40f * Mathf.Sin(i * 0.3f);
            _listDriverVar = new VirtualListDriver(_stage, vr, 200, variableHeight: true, sizes: sizes, reuseBase: 100000u);
            _isListPage = true;
            Debug.Log("[Showcase] page_list 订阅完成（等高1000 + 不等高200 sin-height）");
        }

        // 建 1 个 panel（panel+title+icon 子树）。返回 panel NodeId。
        uint CreateDynPanel()
        {
            if (_dynAnchor == uint.MaxValue) return uint.MaxValue;
            _dynSeq++;
            uint panel = _stage.CreateNode("div", "width:120px;height:90px;background:#2a2f45;border-radius:8px;flex-direction:column;gap:4px;padding:6px");
            if (panel == uint.MaxValue) return uint.MaxValue;
            _stage.AppendChild(_dynAnchor, panel);
            uint title = _stage.CreateNode("span", "font-size:14px;color:#e6e6e0");
            _stage.AppendChild(panel, title);
            _stage.SetText(title, "item-" + _dynSeq);
            uint icon = _stage.CreateNode("img", "width:40px;height:40px");
            _stage.AppendChild(panel, icon);
            _stage.SetSrc(icon, "icons/skin.png");
            return panel;
        }

        void OnDynAdd(EventContext ctx)
        {
            uint panel = CreateDynPanel();
            if (panel != uint.MaxValue) _dynPanels.Add(panel);
        }

        // 批量建 20 个（测动态建树性能 + 大量子树）。
        void OnDynAdd20(EventContext ctx)
        {
            for (int i = 0; i < 20; i++)
            {
                uint panel = CreateDynPanel();
                if (panel != uint.MaxValue) _dynPanels.Add(panel);
            }
            Debug.Log($"[Showcase] 批量建 20，anchor 下共 {_dynPanels.Count} 个");
        }

        // 删最后（remove_node 联动清子 + anim/scroll/tween）。
        void OnDynDel(EventContext ctx)
        {
            if (_dynPanels.Count == 0) return;
            uint last = _dynPanels[_dynPanels.Count - 1];
            _dynPanels.RemoveAt(_dynPanels.Count - 1);
            _stage.RemoveNode(last);
        }

        // 清空所有动态建的 panel。
        void OnDynClear(EventContext ctx)
        {
            foreach (uint p in _dynPanels) _stage.RemoveNode(p);
            _dynPanels.Clear();
        }

        // toggle 末个 panel 样式（set_style 增量改 base_style + 下帧 rematch）。
        void OnDynStyle(EventContext ctx)
        {
            if (_dynPanels.Count == 0) return;
            uint last = _dynPanels[_dynPanels.Count - 1];
            _dynStyleToggled = !_dynStyleToggled;
            _stage.SetStyle(last, _dynStyleToggled
                ? "background:#c2605a;width:160px;height:70px;border-radius:16px"
                : "background:#2a2f45;width:120px;height:90px;border-radius:8px");
        }

        // dyn-load-mail：instantiate("showcase","mail") 挂 ui_layer（叠加，非切包）。
        // mail 是 showcase 包内的组件，不需 LoadPackage 切包。
        void OnDynLoadMail(EventContext ctx)
        {
            if (_mailOverlay != uint.MaxValue)
            {
                Debug.Log("[Showcase] mail 已挂载，忽略重复 instantiate");
                return;
            }
            uint mail = _stage.Instantiate(ShowcasePkg, "mail");
            if (mail == uint.MaxValue)
            {
                Debug.LogError("[Showcase] Instantiate mail 失败");
                return;
            }
            _mailOverlay = mail;
            _stage.AppendChild(_uiLayer, mail);
            UpdateDynLoadStatus("mail");
            Debug.Log("[Showcase] mail 叠加挂载（instantiate，非切包）");
        }

        // dyn-load-showcase：remove mail（摘邮件叠加）。
        void OnDynLoadShowcase(EventContext ctx)
        {
            if (_mailOverlay == uint.MaxValue)
            {
                Debug.Log("[Showcase] mail 未挂载，无需 remove");
                return;
            }
            _stage.RemoveNode(_mailOverlay);
            _mailOverlay = uint.MaxValue;
            UpdateDynLoadStatus("showcase");
            Debug.Log("[Showcase] mail 摘除（remove）");
        }

        void UpdateDynLoadStatus(string current)
        {
            uint status = _stage.FindNodeById("dyn-load-status");
            if (status != uint.MaxValue) _stage.SetText(status, "当前：" + current);
        }

        // === tips 叠加演示（§7.3）===
        // ShowTips: instantiate("showcase","tips_toast") → append tips_layer → Coroutine 定时 RemoveNode。
        void ShowTips()
        {
            if (_tipsRoutine != null) return;   // 防重复触发叠多个 toast
            uint toast = _stage.Instantiate(ShowcasePkg, "tips_toast");
            if (toast == uint.MaxValue)
            {
                Debug.LogError("[Showcase] Instantiate tips_toast 失败");
                return;
            }
            _stage.AppendChild(_tipsLayer, toast);
            _tipsRoutine = StartCoroutine(RemoveAfter(toast, 2.0f));
            Debug.Log("[Showcase] tips 叠加显示（2s 后摘除）");
        }

        System.Collections.IEnumerator RemoveAfter(uint node, float seconds)
        {
            yield return new WaitForSeconds(seconds);
            _stage.RemoveNode(node);
            _tipsRoutine = null;
        }

        // 虚拟列表每帧 SyncSlots。Update 在 LoomStageDriver.LateUpdate(tick) 前跑，
        // 本帧内 slot 增删吃进同帧 tick → solve → 渲染。
        void Update()
        {
            if (_isListPage)
            {
                _listDriverEqual?.SyncSlots();
                _listDriverVar?.SyncSlots();
            }
            // 角色 anchor：nh-stage layout 完成后（rect.h>0）算 localPosition（首帧 layout 未算，下帧补）。
            if (_characterInstance != null && !_anchorApplied) ApplyCharacterAnchor();
        }

        // 0-255 RGB → 归一化 [0,1] RGBA float[4]（alpha=1）。Rust tween 直接写 anim 通道，须与 style 归一化一致。
        static float[] Rgba(int r, int g, int b) => new float[] { r / 255f, g / 255f, b / 255f, 1f };
    }

    // 虚拟列表 driver。
    // 模型：slot = CreateNode div(position:absolute) + img + span title。
    // 每帧 get_scroll_pos 算可见区间 → diff slot 绑定 → create/remove/set_text。
    // 等高：_itemSize 单值 O(1)；不等高：_itemSizes[] 累加搜索。
    // slot top = absolute 定位；SetReuseKey 让 MirrorPool 回收 GO。
    sealed class VirtualListDriver
    {
        readonly LoomStage _stage;
        readonly uint _listContainer;
        readonly uint _itemCount;
        readonly bool _variableHeight;
        readonly uint _reuseBase;   // reuse_key 段基址（每列表独占，防多列表同屏撞车）

        // Equal-height
        float _itemSize;
        uint _measureRoot;

        // Variable-height
        readonly float[] _itemSizes;

        // Init: 0=need_measure, 1=measuring, 2=ready
        int _initStep;

        // slotIdx（0..visibleCount-1，视口内固定槽位序号）→ (root, title, boundItemIndex)。
        // reuse_key 绑 slotIdx（非 itemIndex）——滚动时槽位稳定，只换绑的 itemIndex，
        // reuse_key 不变 → MirrorPool _poolByReuse 命中 → GO 复用只重建 mesh（不销毁重建）。
        readonly Dictionary<int, (uint root, uint title, int boundItemIndex)> _slots = new();

        const uint MeasureReuseKey = 0;

        public VirtualListDriver(LoomStage stage, uint container, uint itemCount,
            bool variableHeight = false, float[] sizes = null, float defaultItemSize = 80f,
            uint reuseBase = 0u)
        {
            _stage = stage;
            _listContainer = container;
            _itemCount = itemCount;
            _variableHeight = variableHeight;
            _reuseBase = reuseBase;

            if (variableHeight && sizes != null && sizes.Length == itemCount)
            {
                _itemSizes = sizes;
                float total = 0f;
                for (int i = 0; i < sizes.Length; i++) total += sizes[i];
                _stage.SetContentSize(container, 0, total);
                _initStep = 2;
            }
            else
            {
                _itemSize = defaultItemSize;
                _initStep = 0;
            }
        }

        (uint root, uint title) CreateItem(float height, float topY)
        {
            uint item = _stage.CreateNode("div",
                $"width:100%;height:{height}px;flex-direction:row;align-items:center;gap:12px;padding:0 16px;background-color:#252839;position:absolute;left:0;top:{topY}px");
            uint icon = _stage.CreateNode("img", "width:48px;height:48px");
            _stage.AppendChild(item, icon);
            _stage.SetSrc(icon, "icons/skin.png");
            uint title = _stage.CreateNode("span", "color:#e0e0e0;font-size:20px");
            _stage.AppendChild(item, title);
            return (item, title);
        }

        string GetItemTitle(uint idx)
        {
            return _variableHeight
                ? $"Item {idx}  ({_itemSizes[idx]:F0}px)"
                : $"Item {idx}";
        }

        // 每帧调（driver.Update，LoomStage tick 前）：
        // init 阶段测量 itemSize (两帧)；就绪后 diff slot 绑定。
        public void SyncSlots()
        {
            if (_initStep == 0) { InitMeasure(); return; }
            if (_initStep == 1) { FinishMeasure(); return; }
            SyncSlotsReady();
        }

        void InitMeasure()
        {
            var (item, title) = CreateItem(80f, 0);
            _measureRoot = item;
            _stage.AppendChild(_listContainer, item);
            _stage.SetReuseKey(item, MeasureReuseKey);
            _stage.SetText(title, "Measuring...");
            _initStep = 1;
        }

        void FinishMeasure()
        {
            var r = _stage.GetNodeLayoutRect(_measureRoot);
            if (r.h > 0)
            {
                _itemSize = r.h;
                _stage.SetContentSize(_listContainer, 0, _itemCount * _itemSize);
                _stage.RemoveNode(_measureRoot);
                _measureRoot = 0;
                _initStep = 2;
            }
        }

        void SyncSlotsReady()
        {
            var (sx, sy) = _stage.GetScrollPos(_listContainer);
            var vp = _stage.GetNodeLayoutRect(_listContainer);
            if (vp.h <= 0) return;

            int first, last;
            if (_variableHeight)
            {
                first = FindFirstVisible(sy);
                last = FindLastVisible(sy + vp.h);
            }
            else
            {
                first = Mathf.FloorToInt(sy / _itemSize);
                last = Mathf.FloorToInt((sy + vp.h) / _itemSize);
            }
            first = Mathf.Max(0, first);
            last = Mathf.Min((int)_itemCount - 1, last);
            if (first > last) { ClearAllSlots(); return; }

            int visibleCount = last - first + 1;

            // 移除超出可见槽位数的多余 slot（视口缩小时）。
            var removeKeys = new List<int>();
            foreach (var kv in _slots)
                if (kv.Key >= visibleCount) removeKeys.Add(kv.Key);
            foreach (var key in removeKeys)
            {
                _stage.RemoveNode(_slots[key].root);
                _slots.Remove(key);
            }

            // 每个可见槽位 slotIdx 0..visibleCount-1 绑定 itemIndex = first + slotIdx。
            // slotIdx 稳定 → reuse_key 不变 → GO 复用。换绑时只改 top/height/text。
            for (int slotIdx = 0; slotIdx < visibleCount; slotIdx++)
            {
                int itemIndex = first + slotIdx;
                float itemH = _variableHeight ? _itemSizes[itemIndex] : _itemSize;
                float top = _variableHeight ? SumSizesUpTo(itemIndex) : itemIndex * _itemSize;

                if (!_slots.ContainsKey(slotIdx))
                {
                    // 新建 slot（视口扩大或首次）。
                    var (root, title) = CreateItem(itemH, top);
                    _stage.AppendChild(_listContainer, root);
                    // reuse_key = _reuseBase + slotIdx + 1。_reuseBase 每列表独占段：
                    // page_list 双列表同屏，若两列表都用 slotIdx+1（都从 1 起），MirrorPool 的
                    // _poolByReuse 是场景级单字典 → 两列表同 slotIdx 的 slot 抢同一个 GO（右列表覆盖左）。
                    // 因仅 slot 容器背景带 reuse_key>0（icon/span 为 0 按 node_id 走），撞车精确发生在
                    // 灰底背景：被顶掉的 slot 没了灰底（只剩 icon+文字）→ 灰底按槽位闪/缺 → 缝隙 + 一闪一闪。
                    _stage.SetReuseKey(root, _reuseBase + (uint)slotIdx + 1);
                    _stage.SetText(title, GetItemTitle((uint)itemIndex));
                    _slots[slotIdx] = (root, title, itemIndex);
                }
                else
                {
                    // 复用 slot：boundItemIndex 变 → 改 top/height（位置）+ text（内容）。
                    // slotIdx 不变 → reuse_key 不变 → MirrorPool 命中现有 GO，只重建 mesh。
                    var slot = _slots[slotIdx];
                    if (slot.boundItemIndex != itemIndex)
                    {
                        _stage.SetStyle(slot.root,
                            $"width:100%;height:{itemH}px;flex-direction:row;align-items:center;gap:12px;padding:0 16px;background-color:#252839;position:absolute;left:0;top:{top}px");
                        _stage.SetText(slot.title, GetItemTitle((uint)itemIndex));
                        _slots[slotIdx] = (slot.root, slot.title, itemIndex);
                    }
                }
            }
        }

        void ClearAllSlots()
        {
            foreach (var kv in _slots) _stage.RemoveNode(kv.Value.root);
            _slots.Clear();
        }

        int FindFirstVisible(float sy)
        {
            float acc = 0f;
            for (int i = 0; i < _itemCount; i++)
            {
                if (acc + _itemSizes[i] > sy) return i;
                acc += _itemSizes[i];
            }
            return (int)_itemCount - 1;
        }

        int FindLastVisible(float bottom)
        {
            float acc = 0f;
            for (int i = 0; i < _itemCount; i++)
            {
                acc += _itemSizes[i];
                if (acc > bottom) return i;
            }
            return (int)_itemCount - 1;
        }

        float SumSizesUpTo(int idx)
        {
            float sum = 0f;
            for (int i = 0; i < idx; i++) sum += _itemSizes[i];
            return sum;
        }
    }
}
