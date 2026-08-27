using UnityEngine;
using LoomGUI;

/// PlayMode showcase 查看器：导航完全走框架自己的事件系统，无 IMGUI。
///
/// 挂在与 LoomStageDriver 同一 GameObject 上。Play 后：
///   - 首页 home：点 7 张 nav-card（nav-settings / nav-mail / ...）跳对应页
///   - 各页：点顶栏 / 侧栏的「← 首页」(button#back-home) 回 home
/// 这些导航元素的 id 已在 showcase HTML 里就位（home.html 的 nav-card、各页的
/// back-home），runner 只用 Button.Clicked 订阅，不画任何 Unity GUI。
/// 订阅随切页 Dispose 自动清理（public-api §5.4），切页即换树即换订阅。
///
/// 验收 7-20/7-21 改动时各页看什么：
///   - 圆角 border (P2-A)：lab(16处)/shop(7)/home(4) 等页的边框/阴影圆角，边角不突出
///   - 真 CSS block (P1)：裸 div 子元素垂直堆叠、不被 flex-grow 拉伸
///   - Image bg-color：带 background-color 的 img 有底色
///   - TextField/Password/Search 投影：form 页三种输入框类型正确
public class ShowcaseRunner : MonoBehaviour
{
    // home 页 nav-card id → showcase 组件 stem（Instantiate 第二参）。
    // 与 showcase/showcase/home.html 的 nav-card id 一一对应。
    static readonly (string cardId, string page)[] NAV_CARDS =
    {
        ("nav-settings", "settings"),
        ("nav-inventory", "inventory"),
        ("nav-mail", "mail"),
        ("nav-shop", "shop"),
        ("nav-character", "character"),
        ("nav-form", "form"),
        ("nav-lab", "lab"),
        ("nav-anim", "m2-animation"),
        ("nav-layout", "layout-anim"),
        ("nav-infra", "api-infra"),
    };

    // settings 页 tab → panel 配对（HTML 标准 role=tab/tabpanel 模式）。
    // 浏览器里 loom-preview.js 的 JS 切 panel display；LoomGUI 运行时无 JS，这里复刻该逻辑。
    // panel-audio 默认可见，其余 HTML 里 style="display:none" 冻结进 pkg。
    static readonly (string tabId, string panelId)[] SETTINGS_TABS =
    {
        ("tab-audio", "panel-audio"),
        ("tab-graphics", "panel-graphics"),
        ("tab-controls", "panel-controls"),
        ("tab-account", "panel-account"),
        ("tab-search", "panel-search"),
    };

    LoomStageDriver _driver;
    Container _current;
    string _shown;

    // ── character 页 3D 展位（NativeHost 同屏渲染验证） ──
    Container _nativeSlot;         // 绑定目标（native-slot div；Unbind 需同节点）
    GameObject _characterModel;    // NativeHost 持位根（挂 wrapper 下）
    Transform _figureSpin;         // 旋转体（模型本体）
    const float FigureSpinDegPerSec = 40f;

    void Update()
    {
        if (_figureSpin != null)
            _figureSpin.Rotate(Vector3.up, FigureSpinDegPerSec * Time.deltaTime, Space.Self);
    }

    void Start()
    {
        // 编辑器验收防冻：编辑器窗口失焦（看 Console/切窗）时播放器循环会被挂起，
        // 表现为「游戏只剩一两帧」。Run In Background 让循环失焦持续跑（真机默认行为）。
        Application.runInBackground = true;
        _driver = GetComponent<LoomStageDriver>();
        if (_driver == null)
        {
            Debug.LogError("[Showcase] LoomStageDriver not found on same GameObject — runner wired wrong");
            return;
        }
        // 让 driver Awake 完成（同帧 Awake 先于 Start，理论已就绪）+ 给 LateUpdate 几帧余量。
        Invoke(nameof(Boot), 0.1f);
    }

    void Boot()
    {
        if (_current == null) Show("home");
    }

    void Show(string page)
    {
        if (_shown == page) return;
        TeardownCharacterStage();   // 上一页若是 character：解绑 NativeHost + 销毁模型
        if (_current != null)
        {
            _current.Dispose();   // 递归销毁旧页 + 清旧页事件订阅（Rust remove_node + 后端镜像下帧清）
            _current = null;
        }
        _current = _driver.Instantiate("showcase", page);
        _shown = _current != null ? page : null;
        if (_current == null)
        {
            Debug.Log($"[Showcase] Instantiate showcase/{page} = FAIL (pkg not loaded? comp not found?)");
            return;
        }
        WireNav(_current, page);
        WireControls(_current, page);
        WireSettingsTabs(_current, page);
        WireListViews(_current, page);
        WireCharacterStage(_current, page);
        Debug.Log($"[Showcase] Instantiate showcase/{page} = OK");
    }

    /// 用框架事件系统接导航：nav-card 与 back-home 都是 `<button>`（Button.Clicked）。
    /// （nav-card 原为 `<a>`/Link.Activated，围栏紧缩 a→button 后统一走 Button.Clicked。）
    /// TryGet 找不到（本页没该元素）就跳过——home 页无 back-home，其他页无 nav-card，各取所需。
    /// 闭包捕获的 page/target 是 per-iteration 局部，每次 Show 重新订阅当前页实例。
    void WireNav(Container page, string pageName)
    {
        // back-home 两处形态：settings 侧栏（页面域直 Get）；其余 6 页在 <page-top> 组件内
        //（打包期展开 + 硬墙作用域——组件内 id 须经 host 两跳，L3 查找边界）。
        if (!page.TryGet<Button>("back-home", out var back)
            && page.TryGet<CustomElement>("page-top", out var top))
        {
            top.TryGet<Button>("back-home", out back);
        }
        if (back != null)
            back.Clicked += () => Show("home");
        if (pageName == "m2-animation" && page.TryGet<Button>("btn-replay", out var replay))
            replay.Clicked += ReplayCurrentPage;
        if (pageName == "m2-animation")
            WireM2AnimationDrivers(page);
        if (pageName == "layout-anim")
            WireLayoutAnimDrivers(page);
        if (pageName == "api-infra")
            WireInfraDrivers(page);
        if (pageName == "home")
        {
            foreach (var (cardId, target) in NAV_CARDS)
            {
                string p = target;   // 防御性局部拷贝，确保每个闭包绑各自的页名
                if (page.TryGet<Button>(cardId, out var card))
                    card.Clicked += () => Show(p);
            }
        }
    }


    /// layout-anim 页 driver（#10 布局动画验收）：
    /// #1/#3/#4 折叠/侧栏/vw 按钮 add_class 切换（CSS transition 起效）；
    /// #6 C# TweenBuilder.Height 运行时 API 摆台（60↔220px）。
    void WireLayoutAnimDrivers(Container page)
    {
        bool foldOpen = false;
        if (page.TryGet<Button>("btn-fold", out var bFold) && page.TryGet<Container>("fold-body", out var foldBody))
        {
            bFold.Clicked += () =>
            {
                foldOpen = !foldOpen;
                if (foldOpen) { foldBody.Classes.Add("open"); bFold.TextContent = "收起"; }
                else { foldBody.Classes.Remove("open"); bFold.TextContent = "展开"; }
            };
        }
        bool sideCollapsed = false;
        if (page.TryGet<Button>("btn-sidebar", out var bSide) && page.TryGet<Container>("sidebar-pair", out var pair))
        {
            bSide.Clicked += () =>
            {
                sideCollapsed = !sideCollapsed;
                if (sideCollapsed) { pair.Classes.Add("collapsed"); bSide.TextContent = "展开侧栏"; }
                else { pair.Classes.Remove("collapsed"); bSide.TextContent = "收起侧栏"; }
            };
        }
        bool vwWide = false;
        if (page.TryGet<Button>("btn-vw", out var bVw) && page.TryGet<Container>("vw-panel", out var vwPanel))
        {
            bVw.Clicked += () =>
            {
                vwWide = !vwWide;
                if (vwWide) { vwPanel.Classes.Add("wide"); bVw.TextContent = "缩回"; }
                else { vwPanel.Classes.Remove("wide"); bVw.TextContent = "拉宽"; }
            };
        }
        if (!page.TryGet<Button>("btn-tween", out var bTween) || !page.TryGet<Container>("tween-panel", out var tweenPanel))
            return;
        bool tweenTall = false;
        bTween.Clicked += () =>
        {
            // TweenBuilder.Height（#10 新通道）：值+域码载荷（[v, (float)LenDomain.Px]）。
            tweenTall = !tweenTall;
            float from = tweenTall ? 60f : 220f;
            float to = tweenTall ? 220f : 60f;
            tweenPanel.Tween(TweenChannel.Height)
                .FromPx(from)
                .ToPx(to)
                .Duration(0.6f)
                .Ease(LoomGUI.EaseKind.CubicOut)
                .Start();
        };
    }

    /// m2-animation #11/#12：程序化动画（node.Play + 句柄 L3）的 driver 接线。
    /// #11 点盒子 Play（OnKey/OnHook 回调进 Console）；#12 按钮排控制同一句柄的
    /// Pause/Resume/Stop/Time seek。Play 每次新建 programmatic player（句柄换新）。
    void WireM2AnimationDrivers(Container page)
    {
        if (page.TryGet<Container>("b11-target", out var playTarget))
        {
            playTarget.On<ClickEvent>(_ =>
                playTarget.Play("m2-play-fade")
                    .OnEnd(() => Debug.Log("[Showcase] m2 #11 Play(m2-play-fade) end")));
        }
        if (page.TryGet<Container>("b11-hook", out var hookTarget))
        {
            hookTarget.On<ClickEvent>(_ =>
                hookTarget.Play("m2-hookanim")
                    .OnKey(0.5f, () => Debug.Log("[Showcase] m2 #11 OnKey(0.5) fired"))
                    .OnHook("half", () => Debug.Log("[Showcase] m2 #11 OnHook(half) fired")));
        }
        if (!page.TryGet<Container>("b12-target", out var handleTarget))
            return;
        LoomGUI.AnimationHandle handle = null;
        if (page.TryGet<Button>("btn-h-play", out var bPlay))
            bPlay.Clicked += () =>
            {
                handle = handleTarget.Play("m2-play-fade");
                Debug.Log("[Showcase] m2 #12 Play -> new handle");
            };
        if (page.TryGet<Button>("btn-h-pause", out var bPause))
            bPause.Clicked += () =>
            {
                handle?.Pause();
                Debug.Log($"[Showcase] m2 #12 Pause @ t={(handle?.Time ?? -1f):F2}s");
            };
        if (page.TryGet<Button>("btn-h-resume", out var bResume))
            bResume.Clicked += () =>
            {
                handle?.Resume();
                Debug.Log("[Showcase] m2 #12 Resume");
            };
        if (page.TryGet<Button>("btn-h-stop", out var bStop))
            bStop.Clicked += () =>
            {
                handle?.Stop();
                Debug.Log("[Showcase] m2 #12 Stop（句柄失效）");
            };
        if (page.TryGet<Button>("btn-h-seek", out var bSeek))
            bSeek.Clicked += () =>
            {
                if (handle == null) return;
                handle.Time = 0.5f;
                Debug.Log("[Showcase] m2 #12 seek Time=0.5s");
            };
    }

    /// api-infra 页：公共 API 基础设施验收 driver（调度三件套 / option-tab 派生 getter /
    /// 多模板列表 / 包生命周期）。页面只到 HTML 结构，行为全部在此接线——真机上看的就是这些
    /// （确定性断言在 headless SchedulerAndLifecycleTests，本页是行为/视觉面）。
    /// 切页防御：CallLater/CallNextFrame 挂在 UIContext 上跨页存活，回调里对目标节点
    /// IsDisposed 短路；OnUpdate 订阅随页 Dispose 自动清理（契约），无需手动拆。
    void WireInfraDrivers(Container page)
    {
        var ui = _driver.Context;

        // ── #1 OnUpdate 逻辑时钟：dt 累积逐帧刷新 + 帧计数；按钮 Dispose / 重订阅句柄。 ──
        // span 打包后是 TextElement（SemanticKind::TextElement；运行时 create_node("span")
        // 才产 TextNode——两路径不同型，TryGet 按 C# 类型精确匹配，写错型整块静默跳过）。
        if (page.TryGet<TextElement>("infra-clock", out var clock) &&
            page.TryGet<TextElement>("infra-frames", out var frames))
        {
            float elapsed = 0f;
            long pumps = 0;
            void Tick(float dt)
            {
                elapsed += dt;
                pumps++;
                clock.TextContent = elapsed.ToString("F1") + " s";
                frames.TextContent = pumps + " 帧";
            }
            var sub = page.OnUpdate(Tick);
            if (page.TryGet<Button>("btn-clock-toggle", out var toggle))
                toggle.Clicked += () =>
                {
                    if (sub == null) sub = page.OnUpdate(Tick);
                    else { sub.Dispose(); sub = null; }
                };
        }

        // ── #2 CallLater 倒计时链：每步 1s 延迟，one-shot 链式调度。 ──
        if (page.TryGet<Button>("btn-later", out var laterBtn) &&
            page.TryGet<TextElement>("infra-later", out var later))
        {
            laterBtn.Clicked += () =>
            {
                later.Classes.Remove("done");
                InfraCountdown(later, ui, 3);
            };
        }

        // ── #3 CallNextFrame：点击当帧「已受理」，下一帧帧头改文本。 ──
        if (page.TryGet<Button>("btn-nf", out var nfBtn) &&
            page.TryGet<TextElement>("infra-nf", out var nf) &&
            page.TryGet<TextElement>("infra-nf-count", out var nfCount))
        {
            int fired = 0;
            nfBtn.Clicked += () =>
            {
                nf.TextContent = "已点击（本帧受理）→ 等待下一帧…";
                ui.CallNextFrame(() =>
                {
                    if (nf.IsDisposed) return;
                    fired++;
                    nf.TextContent = "下一帧回调已触发 ✓（帧头 fire）";
                    nfCount.TextContent = fired + " 次";
                });
            };
        }

        // ── #4 Dropdown value 链读数：SelectedValue / option.Value / option.Selected。 ──
        if (page.TryGet<Dropdown>("infra-dd", out var dd) &&
            page.TryGet<TextElement>("dd-sel", out var ddSel) &&
            page.TryGet<TextElement>("dd-va", out var va) && page.TryGet<TextElement>("dd-sa", out var sa) &&
            page.TryGet<TextElement>("dd-vb", out var vb) && page.TryGet<TextElement>("dd-sb", out var sb) &&
            page.TryGet<TextElement>("dd-vc", out var vc) && page.TryGet<TextElement>("dd-sc", out var sc) &&
            page.TryGet<OptionItem>("opt-lang-a", out var oa) &&
            page.TryGet<OptionItem>("opt-lang-b", out var ob) &&
            page.TryGet<OptionItem>("opt-lang-c", out var oc))
        {
            void RefreshDd()
            {
                ddSel.TextContent = dd.SelectedValue ?? "(null)";
                va.TextContent = oa.Value; sa.TextContent = oa.Selected ? "true" : "false";
                vb.TextContent = ob.Value; sb.TextContent = ob.Selected ? "true" : "false";
                vc.TextContent = oc.Value; sc.TextContent = oc.Selected ? "true" : "false";
            }
            RefreshDd();
            dd.SelectionChanged += _ => RefreshDd();
        }

        // ── #5 Tab.Selected 合成读数：切选即跟随（父 TabList 状态派生）。 ──
        if (page.TryGet<TabList>("infra-tabs", out var tabs) &&
            page.TryGet<Tab>("itab-1", out var t1) && page.TryGet<Tab>("itab-2", out var t2) &&
            page.TryGet<Tab>("itab-3", out var t3) &&
            page.TryGet<TextElement>("tab-r1", out var r1) &&
            page.TryGet<TextElement>("tab-r2", out var r2) &&
            page.TryGet<TextElement>("tab-r3", out var r3))
        {
            void RefreshTabs()
            {
                r1.TextContent = t1.Selected ? "true" : "false";
                r2.TextContent = t2.Selected ? "true" : "false";
                r3.TextContent = t3.Selected ? "true" : "false";
            }
            RefreshTabs();
            tabs.SelectionChanged += _ => RefreshTabs();
        }

        // ── #6 GetTemplate 具名模板：经 API 取蓝图喂 ItemTemplate + BindItem 行样式切换。
        //    按 index 自动选多模板（TemplateSelector 逐项切换）机制未交付（core
        //    enter_data_driven 要求恰好一个 template），本节不演示。
        if (page.TryGet<ListView>("infra-mt-list", out var mt))
        {
            mt.ItemTemplate = mt.GetTemplate("row-tpl");
            mt.BindItem = (item, i) =>
            {
                var spans = item.Query<TextElement>();
                if (spans.Count >= 2)
                {
                    spans[0].TextContent = string.Format("#{0:00}", i);
                    spans[1].TextContent = (i % 3 == 2) ? "强调行（class 切换）" : "普通行";
                }
                if (i % 3 == 2) item.Classes.Add("mt-row-accent");
                else item.Classes.Remove("mt-row-accent");
            };
            mt.ItemCount = 30;
        }

        // ── #7 UnloadPackage：别名重载 showcase 字节 → 实例化本页微缩窗 → 卸载见存活。
        //    载荷用 api-infra 组件本身（pkg 可寻址组件粒度 = html 文件，无需另造载荷组件）：
        //    实例后 Style 覆写宽高 + overflow hidden 裁成小窗。
        if (page.TryGet<Container>("infra-ul-stage", out var ulStage) &&
            page.TryGet<TextElement>("infra-ul-status", out var ulStatus))
        {
            UIPackage copyPkg = null;
            const string CopyName = "infra-copy";
            Container InstantiateMiniWindow()
            {
                var win = copyPkg.Instantiate("api-infra");
                win.Style.Width = Length.Px(420);
                win.Style.Height = Length.Px(88);
                win.Style.OverflowX = Overflow.Clip;
                win.Style.OverflowY = Overflow.Clip;
                // 微缩窗 88px 高只露出顶栏，正文整棵被 clip 不可见——却照付全额 solve 成本
                //（solve 每帧全量重建 taffy 树，每 mini ≈ +7.7ms/帧，几个窗口就把帧率拖垮）。
                // display:none 让 solve 跳过该子树（taffy 语义），视觉零变化。
                foreach (var c in win.Query<Container>())
                {
                    if (c.Classes.Contains("body")) { c.Style.Display = DisplayMode.None; break; }
                }
                ulStage.AddChild(win);
                return win;
            }
            if (page.TryGet<Button>("btn-ul-load", out var ulLoad))
                ulLoad.Clicked += () =>
                {
                    try
                    {
                        if (copyPkg == null)
                            copyPkg = ui.LoadPackage(CopyName, _driver.LoadPackageBytes("showcase"));
                        InstantiateMiniWindow();
                        ulStatus.TextContent = "已实例化（api-infra 微缩窗 · 舞台上 " + ulStage.ChildCount + " 个存活）";
                    }
                    catch (System.Exception ex)
                    {
                        ulStatus.TextContent = "Load/Instantiate 异常：" + ex.GetType().Name;
                    }
                };
            if (page.TryGet<Button>("btn-ul-unload", out var ulUnload))
                ulUnload.Clicked += () =>
                {
                    if (copyPkg == null) { ulStatus.TextContent = "副本未加载"; return; }
                    try
                    {
                        ui.UnloadPackage(CopyName);
                        bool staleThrew = false;
                        try { copyPkg.Instantiate("api-infra"); }
                        catch (UIPackageException) { staleThrew = true; }
                        ulStatus.TextContent = "模板已卸载 · 旧句柄 Instantiate 抛 = " + (staleThrew ? "✓" : "✗")
                            + " · 微缩窗独立存活 = " + (ulStage.ChildCount > 0 ? "✓" : "✗");
                        copyPkg = null;   // 下次 Load 重建句柄（重载同名包）
                    }
                    catch (System.Exception ex)
                    {
                        ulStatus.TextContent = "Unload 异常：" + ex.GetType().Name;
                    }
                };
        }
    }

    /// #2 的倒计时链：n→…→1→完成，每步 CallLater(1s)（递归延迟调度）。切页后目标节点
    /// 已 Dispose 即短路（timer 挂在 UIContext 上跨页存活，不随页清理）。
    void InfraCountdown(TextElement label, UIContext ui, int n)
    {
        if (label.IsDisposed) return;
        if (n == 0)
        {
            label.TextContent = "完成 ✓";
            label.Classes.Add("done");
            return;
        }
        label.TextContent = n.ToString();
        ui.CallLater(1f, () => InfraCountdown(label, ui, n - 1));
    }

    /// m2-animation 页「↻ 重播」：原地重启声明式动画（Container.RestartAnimations）——
    /// player 重建、delay 重计，节点/滚动/控件值/订阅全保留。
    void ReplayCurrentPage()
    {
        // 原地重启声明式动画（Container.RestartAnimations）：player 重建、delay 重计，
        // 节点/滚动/控件值/订阅全保留——不再走销毁重实例化。
        _current?.RestartAnimations();
    }

    // ── character 页 3D 展位：NativeHost 把引擎 GO 嵌进 UI 层级 ──
    //
    // 验证目标：UI（自绘 mesh）与引擎原生渲染（3D 模型 + 光照）同屏 interleaved——
    // 模型 sortingOrder = native-slot 节点 sort_key（NativeHostManager.Sync 每帧写），
    // 与 UI 同 Transparent 队列按 UI 绘制序穿插；模型跟随节点 world transform。
    // 模型用基元拼装（无外部资产依赖）：机甲 + 剑 + 自发光基座 + 点光（ shading 证明
    // 走的是引擎光照而非 UI 自绘）。尺寸按 design px：holder scale 100 → 1 unit = 100px。

    void WireCharacterStage(Container page, string pageName)
    {
        if (pageName != "character") return;
        if (!page.TryGet<Container>("native-slot", out var slot)) return;
        _nativeSlot = slot;
        _characterModel = BuildCharacterModel(out _figureSpin);
        _driver.BindNativeHost(slot, _characterModel);
        // 帧延迟对齐：build 时（Animator 未评估）的 bounds 与真实播放 pose 有偏差（曾整体高出
        // 展位数百 px）——等动画跑 2 帧后按世界包围盒重新归一：高 520、中心对齐展位中心。
        StartCoroutine(AlignModelAfterAnimEval(_characterModel.transform));
        Debug.Log("[Showcase] character native-slot bound to 3D model (NativeHost)");
    }

    /// 帧延迟对齐：build 时（Animator 未评估）的 bounds 与真实播放 pose 有偏差——模型
    /// 曾整体高出展位数百万至更多 px（脚底钉在展位中心、身高向上溢出展位顶）。
    /// 等 2 帧动画真实评估后按世界包围盒重新归一：高 520、中心对齐展位中心（持位原点）。
    System.Collections.IEnumerator AlignModelAfterAnimEval(Transform modelRoot)
    {
        yield return null;
        yield return null;
        var rends = modelRoot.GetComponentsInChildren<Renderer>();
        if (rends.Length == 0) yield break;
        var b = rends[0].bounds;
        foreach (var r in rends) b.Encapsulate(r.bounds);
        if (b.size.y < 0.001f || b.size.y > 10000f) yield break;
        float s = 520f / b.size.y;
        modelRoot.localScale *= s;
        // 缩放后包围盒随 localScale 变化——重测一次再对齐（两步收敛）。
        b = rends[0].bounds;
        foreach (var r in rends) b.Encapsulate(r.bounds);
        // 中心对齐到 modelRoot（holder）自身位置 = 展位中心（不是 wrapper 原点 = slot 左上）。
        Vector3 worldOffset = modelRoot.position - b.center;
        modelRoot.position += worldOffset;
        // 观察向（z）压扁 + 抬到 UI 平面前：模型原生 z 深 ~±135px，超出 UI 相机视景
        // （near z=-9.9 / far z=90）会被远近裁剪面各切一刀（视觉"被 UI 平面切成两半，
        // 只剩后半"）。holder（不随自转）压 z 至 ~1/4 并整体 z+=20 → z∈[20..87]，
        // 全程在裁剪区间内、位于 UI 平面（z=0）之前。
        Vector3 ls = modelRoot.localScale;
        modelRoot.localScale = new Vector3(ls.x, ls.y, ls.z * 0.25f);
        Vector3 pos = modelRoot.position;
        modelRoot.position = new Vector3(pos.x, pos.y, pos.z + 20f);
        Debug.Log($"[Showcase] model aligned: size={b.size} center={b.center} rootPos={modelRoot.position}");
    }

    void TeardownCharacterStage()
    {
        if (_nativeSlot != null)
        {
            _driver.UnbindNativeHost(_nativeSlot);   // 销毁 wrapper（GO 先 reparent 出来）
            _nativeSlot = null;
        }
        if (_characterModel != null)
        {
            Destroy(_characterModel);
            _characterModel = null;
        }
        _figureSpin = null;
    }

    /// 展位模型：优先 FBX 资产（Animated Human prefab，含 Animator controller 自动播
    /// 骨骼动画——验证 NativeHost 带真实 SkinnedMeshRenderer + 动画同屏渲染）；资产缺失
    /// （built player / 路径变动）回落程序化基元机甲。两者都做归一化：骨架/渲染包围盒
    /// 缩放到 ~520 design px、脚底对齐持位点、水平居中，模型细节与资产原始尺寸解耦。
    static GameObject BuildCharacterModel(out Transform spin)
    {
#if UNITY_EDITOR
        var prefab = UnityEditor.AssetDatabase.LoadAssetAtPath<GameObject>(
            "Assets/Models/quaternius_animatedman/Animated Human.prefab");
        if (prefab != null)
        {
            // 归一化期间 holder 必须留在原点：bounds 是世界系读数，holder 若已带 slot 偏移
            // （360,-340），偏移会被当几何中心反向"归位"——模型被甩出数万单位（曾现）。
            // 量完再挪到展位中心。
            var holder = new GameObject("NativeCharacter");

            var inst = Instantiate(prefab, holder.transform);
            inst.transform.localPosition = Vector3.zero;
            inst.transform.localRotation = Quaternion.identity;
            inst.transform.localScale = Vector3.one;
            // 骨骼动画 pose 决定 skinned bounds——先评估首帧再量。骨架 AABB 一并封装
            //（蒙皮渲染的真值；SMR.bounds 在 skinning 首评估前可能是陈旧的小盒）。
            var animator = inst.GetComponentInChildren<Animator>();
            if (animator != null)
            {
                animator.applyRootMotion = false;
                animator.Rebind();
                animator.Update(0f);
            }
            var rends = inst.GetComponentsInChildren<Renderer>();
            bool have = false;
            var b = new Bounds();
            foreach (var r in rends)
            {
                if (!have) { b = r.bounds; have = true; }
                else b.Encapsulate(r.bounds);
            }
            foreach (var smr in inst.GetComponentsInChildren<SkinnedMeshRenderer>())
            {
                smr.updateWhenOffscreen = true;   // 骨架驱动世界 bounds，杜绝误剔除
                foreach (var bone in smr.bones)
                    if (bone != null)
                    {
                        if (!have) { b = new Bounds(bone.position, Vector3.zero); have = true; }
                        else b.Encapsulate(bone.position);
                    }
            }
            if (have && b.size.y > 0.001f && b.size.y < 10000f)
            {
                float s = 520f / b.size.y;
                inst.transform.localScale = Vector3.one * s;
                // 脚底对齐 + 水平/纵深居中（旋转 pivot = 脚底中心）。
                inst.transform.localPosition = new Vector3(
                    -b.center.x * s, -b.min.y * s, -b.center.z * s);
                // z 微前：与 slot 自身底色同 sort_key 时以距离赢 tiebreak（近者后画）。
                inst.transform.localPosition += new Vector3(0f, 0f, 0.5f);
            }
            // wrapper 原点 = native-slot 左上角（design 坐标 y 下 → container y-up 空间取负）。
            // slot 720x680 → 持位居中、脚底落在中心点。
            holder.transform.localPosition = new Vector3(360f, -340f, 0f);


            var lightGo = new GameObject("rimLight");
            lightGo.transform.SetParent(holder.transform, false);
            lightGo.transform.localPosition = Vector3.zero;
            // 平行光（无距离衰减；design px 尺度的模型下点光衰减到近黑）+ 暖色斜照。
            var pl = lightGo.AddComponent<Light>();
            pl.type = LightType.Directional;
            pl.transform.localRotation = Quaternion.Euler(50f, -30f, 0f);
            pl.color = new UnityEngine.Color(1f, 0.94f, 0.85f);
            pl.intensity = 2.2f;
            // 正面补光（贴图深色系，纯侧逆光太暗）：从相机方向低强度补。
            var fillGo = new GameObject("fillLight");
            fillGo.transform.SetParent(holder.transform, false);
            var fl = fillGo.AddComponent<Light>();
            fl.type = LightType.Directional;
            fl.transform.localRotation = Quaternion.Euler(10f, 190f, 0f);
            fl.color = new UnityEngine.Color(0.85f, 0.92f, 1f);
            fl.intensity = 0.9f;
            Debug.Log($"[Showcase] native-slot model = FBX prefab（Animator={animator != null}）");
            spin = inst.transform;
            return holder;
        }
        Debug.LogWarning("[Showcase] Animated Human.prefab not found — fallback to primitive mech");
#endif
        return BuildPrimitiveMech(out spin);
    }

    /// 程序化机甲（FBX 缺失时的 fallback）：躯干/头/肩/臂 capsule+cube，右手发光剑，
    /// 脚下发光基座环，一点光。
    static GameObject BuildPrimitiveMech(out Transform spin)
    {
        var holder = new GameObject("NativeCharacter");
        // wrapper 原点 = native-slot 左上角（design 坐标 y 下 → container y-up 空间取负）。
        // slot 720x680 → 持位居中。figure z +0.01：与 slot 自身底色同 sort_key 时以 z
        // 近者后画赢 tiebreak，保证模型画在底色之上。
        holder.transform.localPosition = new Vector3(360f, -340f, 0f);
        holder.transform.localScale = Vector3.one * 100f;

        var figure = new GameObject("figure");
        figure.transform.SetParent(holder.transform, false);
        figure.transform.localPosition = new Vector3(0f, 0f, 0.01f);

        var steel = new UnityEngine.Color(0.55f, 0.62f, 0.70f);
        var armor = new UnityEngine.Color(0.16f, 0.30f, 0.42f);

        Prim(figure.transform, PrimitiveType.Capsule, "torso",
            new Vector3(0f, 1.05f, 0f), new Vector3(0.55f, 0.50f, 0.42f), armor);
        Prim(figure.transform, PrimitiveType.Sphere, "head",
            new Vector3(0f, 1.74f, 0f), new Vector3(0.34f, 0.32f, 0.34f), steel);
        // 面甲：自发光青条（朝相机面 z+）。
        Prim(figure.transform, PrimitiveType.Cube, "visor",
            new Vector3(0f, 1.78f, 0.30f), new Vector3(0.26f, 0.07f, 0.05f),
            new UnityEngine.Color(0f, 0f, 0f), new UnityEngine.Color(0.37f, 0.71f, 0.83f) * 3f);
        Prim(figure.transform, PrimitiveType.Cube, "shoulderL",
            new Vector3(-0.44f, 1.42f, 0f), new Vector3(0.26f, 0.18f, 0.30f), armor);
        Prim(figure.transform, PrimitiveType.Cube, "shoulderR",
            new Vector3(0.44f, 1.42f, 0f), new Vector3(0.26f, 0.18f, 0.30f), armor);
        Prim(figure.transform, PrimitiveType.Capsule, "armL",
            new Vector3(-0.46f, 1.02f, 0f), new Vector3(0.13f, 0.32f, 0.13f), steel);
        Prim(figure.transform, PrimitiveType.Capsule, "armR",
            new Vector3(0.46f, 1.02f, 0f), new Vector3(0.13f, 0.32f, 0.13f), steel);
        // 剑：右手竖持，自发光金刃 + 小幅倾斜。
        var sword = Prim(figure.transform, PrimitiveType.Cube, "sword",
            new Vector3(0.62f, 1.25f, 0.08f), new Vector3(0.07f, 1.15f, 0.13f),
            new UnityEngine.Color(0f, 0f, 0f), new UnityEngine.Color(0.83f, 0.64f, 0.31f) * 2.5f);
        sword.transform.localRotation = Quaternion.Euler(0f, 0f, 10f);
        // 基座环：自发光青（对称体，随 figure 旋转不可见）。
        Prim(figure.transform, PrimitiveType.Cylinder, "baseRing",
            new Vector3(0f, 0.02f, 0f), new Vector3(1.0f, 0.015f, 1.0f),
            new UnityEngine.Color(0f, 0f, 0f), new UnityEngine.Color(0.37f, 0.71f, 0.83f) * 1.6f);

        var lightGo = new GameObject("rimLight");
        lightGo.transform.SetParent(figure.transform, false);
        lightGo.transform.localPosition = new Vector3(0.8f, 2.3f, 1.4f);
        var pl = lightGo.AddComponent<Light>();
        pl.type = LightType.Point;
        pl.color = new UnityEngine.Color(1f, 0.9f, 0.75f);
        pl.intensity = 1.6f;
        pl.range = 4f;

        spin = figure.transform;
        return holder;
    }

    /// 基元快捷构造：挂父、定位、缩放、赋 lit 材质（可选自发光），剥 Collider（UI 层无物理）。
    static GameObject Prim(Transform parent, PrimitiveType type, string name,
        Vector3 localPos, Vector3 localScale, UnityEngine.Color color, UnityEngine.Color? emission = null)
    {
        var go = GameObject.CreatePrimitive(type);
        go.name = name;
        var col = go.GetComponent<Collider>();
        if (col != null) Destroy(col);
        go.transform.SetParent(parent, false);
        go.transform.localPosition = localPos;
        go.transform.localScale = localScale;
        var shader = Shader.Find("Universal Render Pipeline/Lit");
        if (shader == null) shader = Shader.Find("Standard");
        var m = new Material(shader);
        m.color = color;
        if (emission.HasValue)
        {
            m.EnableKeyword("_EMISSION");
            m.SetColor("_EmissionColor", emission.Value);
        }
        go.GetComponent<Renderer>().sharedMaterial = m;
        return go;
    }

    /// settings 页 tab 切换：HTML 的 role=tab/tabpanel 模式依赖运行时 JS 改 panel display，
    /// LoomGUI 运行时无 JS，这里订阅 tab 按钮 Clicked → 隐藏当前 panel + 显示目标 panel。
    /// panel 是裸 <div>（.panel CSS 无 display 声明）→ 默认 display:block（子元素 page-title/
    /// page-desc/field 垂直堆叠）。显示用 DisplayMode.Block，**不能用 Flex**——Flex 默认
    /// flex-direction:row 会让 panel 的子元素水平排列，布局错乱。隐藏用 DisplayMode.None。
    /// 改 Style.Display 攒批下帧 flush 到 core 触发 solve 重排（display 变是低频 UI 操作）。
    void WireSettingsTabs(Container page, string pageName)
    {
        if (pageName != "settings") return;
        // 预取 tab 按钮与 panel，过滤掉本页不存在的（宽松查询，同 WireNav）。
        var tabs = new System.Collections.Generic.List<(Button tab, Container panel)>();
        foreach (var (tabId, panelId) in SETTINGS_TABS)
        {
            if (page.TryGet<Button>(tabId, out var tab) && page.TryGet<Container>(panelId, out var panel))
                tabs.Add((tab, panel));
        }
        if (tabs.Count == 0) return;
        // 找当前可见的 panel 作初始 active（HTML 里 panel-audio 默认可见）。
        Container initial = null;
        foreach (var (_, panel) in tabs)
        {
            if (panel.Style.Display != DisplayMode.None) { initial = panel; break; }
        }
        // active 用单元素数组承载：C# 闭包捕获数组引用，所有 tab 闭包共享 arr[0]，
        // 任一 tab 点击后更新它，其余 tab 下次点击读到最新 active（避免 per-iteration 快照失同步）。
        var active = new Container[] { initial };
        foreach (var (tab, panel) in tabs)
        {
            Container target = panel;        // 防御性局部拷贝
            tab.Clicked += () =>
            {
                if (active[0] == target) return;   // 已是当前页，no-op
                if (active[0] != null) active[0].Style.Display = DisplayMode.None;
                target.Style.Display = DisplayMode.Block;
                active[0] = target;                // 后续点击以新 active 为基准
            };
        }
    }
    /// 控件事件流演示：settings 滑块拖动更新旁边数值、character 训练按钮给 EXP 进度条加经验。
    /// 只验证 ValueChanged / Clicked → ProgressBar.Value 的端到端事件链，不构建完整逻辑。
    /// 元素缺失（本页没该控件）TryGet 返 false 跳过——和 WireNav 同样的宽松查询模式。
    void WireControls(Container page, string pageName)
    {
        if (pageName == "lab")
        {
            // lab #14 运行时 ZIndex：按钮把 B 片在 4（置顶）/ 0（回落 DOM 序）间切换——
            // 便签层 inline override，下帧绘制序生效（不触发 flex solve）。
            if (page.TryGet<Button>("zi-btn", out var ziBtn)
                && page.TryGet<Container>("zi-b", out var ziB))
            {
                ziBtn.Clicked += () =>
                {
                    bool raised = ziB.Style.ZIndex > 0;
                    ziB.Style.ZIndex = raised ? 0 : 4;
                    Debug.Log($"[Showcase] lab #14 B z-index -> {ziB.Style.ZIndex}");
                };
            }
            // lab #15 长按 + CancelClick：长按触发后砍掉本次 click（松开读数不得变「click」）；
            // 短按走 ClickEvent。验证 LongPressEvent 路由 + CancelClick FFI 链。
            if (page.TryGet<Container>("lp-target", out var lpTarget)
                && page.TryGet<TextElement>("lp-read", out var lpRead))
            {
                lpTarget.On<LongPressEvent>(e =>
                {
                    lpTarget.CancelClick(e.TouchId);
                    lpRead.TextContent = "长按触发 @" + (int)e.Position.X + "," + (int)e.Position.Y + "（click 已取消）";
                });
                lpTarget.On<ClickEvent>(e => { lpRead.TextContent = "click 触发（短按）"; });
            }
            // lab #15 pointer capture：Down 时 SetPointerCapture(0)（鼠标槽），拖出元素外
            // Move 仍路由到本节点（读数持续跟随 = capture 生效）；Up 自动释放（契约）。
            if (page.TryGet<Container>("cap-target", out var capTarget)
                && page.TryGet<TextElement>("cap-read", out var capRead))
            {
                capTarget.On<PointerDownEvent>(e =>
                {
                    // touchId 从事件取（鼠标 = -1，触摸 = fingerId——手写 0 会注册到空槽）。
                    capTarget.SetPointerCapture(e.TouchId);
                    capRead.TextContent = "down（已 capture，touchId=" + e.TouchId + "）";
                });
                capTarget.On<PointerMoveEvent>(e =>
                {
                    capRead.TextContent = "move " + (int)e.Position.X + "," + (int)e.Position.Y;
                });
                capTarget.On<PointerUpEvent>(e => { capRead.TextContent = "up（capture 已自动释放）"; });
            }
            // lab #16 runtime TweenBuilder：按钮触发 C# 链式 tween（Transform 五元组，
            // EaseBezier 精确 CSS ease + Repeat+yoyo 两轮 + OnComplete tag 路由收尾）。
            // HTML 只摆台（块/按钮/读数），动画全走运行时 API——CSS 面 + 运行时面的分工演示。
            if (page.TryGet<Button>("tw-btn", out var twBtn)
                && page.TryGet<Container>("tw-target", out var twTarget)
                && page.TryGet<TextElement>("tw-read", out var twRead))
            {
                twBtn.Clicked += () =>
                {
                    twRead.TextContent = "tween 播放中（2 轮 yoyo）…";
                    twTarget.Tween(TweenChannel.Transform)
                        .From(0f, 0f, 1f, 1f, 0f)
                        .To(176f, 0f, 1f, 1f, 0f)
                        .Duration(0.5f)
                        .EaseBezier(0.25f, 0.1f, 0.25f, 1f)
                        .Repeat(1, yoyo: true)
                        .OnComplete(_ => twRead.TextContent = "tween 完成（OnComplete 触发）")
                        .Start();
                };
            }
            // lab #17 动态内容范式（#88）：模板实例化 + Query 注入 + 运行时切类。
            // dyn-* 类声明在 lab.dynamic.css（<link> 引入 = 动态样式声明位——围栏可校验、
            // 随 pkg 打包、预览可见），C# 侧只做类切换、不拼 CSS 串。伪类（:hover /
            // :nth-child）对实例化节点照常生效，是本节同时验证的点。
            if (page.TryGet<Button>("dyn-btn", out var dynBtn)
                && page.TryGet<Button>("dyn-sel-btn", out var dynSelBtn)
                && page.TryGet<Container>("dyn-list", out var dynList)
                && page.TryGet<TextElement>("dyn-read", out var dynRead))
            {
                var cards = new System.Collections.Generic.List<Container>();
                string[] names = { "哨塔", "兵营", "金矿" };
                int[] levels = { 3, 7, 12 };
                dynBtn.Clicked += () =>
                {
                    foreach (var c in cards) c.Dispose(); // 重复点击 = 重建
                    cards.Clear();
                    var tpl = page.GetTemplate("dyn-card");
                    for (int i = 0; i < names.Length; i++)
                    {
                        var card = tpl.Instantiate();
                        card.Get<TextElement>("dyn-name").TextContent = names[i];
                        card.Get<TextElement>("dyn-count").TextContent = "LV." + levels[i];
                        dynList.AddChild(card);
                        cards.Add(card);
                    }
                    dynRead.TextContent = "已实例化 " + cards.Count + " 节点（Query 注入完成）";
                };
                dynSelBtn.Clicked += () =>
                {
                    if (cards.Count < 2) { dynRead.TextContent = "先点「实例化 3 节点」"; return; }
                    var card = cards[1];
                    card.Classes.Toggle("dyn-selected");
                    var bg = card.Computed.Background;
                    // computed 背景色进读数：选中翻转即级联证据（#17331f ↔ 奇偶行底色）。
                    string bgHex = bg.HasValue
                        ? string.Format("#{0:X2}{1:X2}{2:X2}",
                            (int)(bg.Value.R * 255f), (int)(bg.Value.G * 255f), (int)(bg.Value.B * 255f))
                        : "null";
                    dynRead.TextContent = "dyn-selected=" + card.Classes.Contains("dyn-selected")
                        + " computed bg=" + bgHex;
                };
            }
            // lab #19 链接（#74）：Get<Link> + Clicked 把 href 写进读数 span——href 原样
            // 回传（opaque 标识符，游戏自解释路由），点击命中细化到 a 节点（含嵌 span 文字）；
            // 点击链接外普通文字不触发（读数不变即判据）。四个链接共用一个读数；第四个
            // （link-custom）作者 color/text-decoration 声明覆盖 UA 默认——视觉判据在页内 desc。
            if (page.TryGet<Link>("link-shop", out var linkShop)
                && page.TryGet<Link>("link-bag", out var linkBag)
                && page.TryGet<Link>("link-quest", out var linkQuest)
                && page.TryGet<Link>("link-custom", out var linkCustom)
                && page.TryGet<TextElement>("link-readout", out var linkRead))
            {
                linkShop.Clicked += () => { linkRead.TextContent = linkShop.Href; Debug.Log("[Showcase] link -> " + linkShop.Href); };
                linkBag.Clicked += () => { linkRead.TextContent = linkBag.Href; };
                linkQuest.Clicked += () => { linkRead.TextContent = linkQuest.Href; };
                linkCustom.Clicked += () => { linkRead.TextContent = linkCustom.Href; };
            }
        }
        if (pageName == "settings")
        {
            // Slider.ValueChanged 逐帧拖拽值 → 同步刷新旁边的数值标签。
            if (page.TryGet<Slider>("vol-master", out var vol)
                && page.TryGet<TextElement>("vol-master-val", out var volVal))
            {
                vol.ValueChanged += e => volVal.TextContent = Mathf.RoundToInt(e.NewValue).ToString();
            }
            // Toggle.CheckedChanged → 控制台输出（演示 checkbox 事件链）。
            if (page.TryGet<Toggle>("gfx-fullscreen", out var fs))
                fs.CheckedChanged += e => Debug.Log($"[Showcase] fullscreen = {e.NewValue}");
            // Dropdown.SelectionChanged（控件束 P3 typed 事件链：select 弹出列表选中）。
            if (page.TryGet<Dropdown>("gfx-res", out var res))
                res.SelectionChanged += e => Debug.Log($"[Showcase] gfx-res selected index = {e.NewIndex}");
            // NumberField.ValueChanged（控件束 P3：数值框，float 值经 min/max clamp + step 量化）。
            if (page.TryGet<NumberField>("snd-voices", out var voices))
                voices.ValueChanged += e => Debug.Log($"[Showcase] snd-voices = {e.NewValue}");
            // TextField.Submitted（控件束 P2：单行框回车提交）。
            if (page.TryGet<TextField>("key-custom", out var keyCustom))
                keyCustom.Submitted += v => Debug.Log($"[Showcase] key-custom submitted: \"{v}\"");
        }

        if (pageName == "character")
        {
            // Button.Clicked → ProgressBar.Value += 10（clamp 由 core 做），并刷新百分比标签。
            // EXP 条在 <stat-bar id="exp-bar"> 组件展开域内（投影内容归组件域）——两跳获取。
            ProgressBar exp = null;
            TextElement expVal = null;
            if (page.TryGet<ProgressBar>("stat-exp", out var expDirect))
            {
                exp = expDirect;   // 兼容未组件化的直排形态
                page.TryGet<TextElement>("stat-exp-val", out expVal);
            }
            else if (page.TryGet<CustomElement>("exp-bar", out var expBar))
            {
                expBar.TryGet<ProgressBar>("stat-exp", out exp);
                expBar.TryGet<TextElement>("stat-exp-val", out expVal);
            }
            if (page.TryGet<Button>("btn-train", out var train) && exp != null && expVal != null)
            {
                train.Clicked += () =>
                {
                    exp.Value = Mathf.Min(exp.Value + 10f, exp.Max);
                    expVal.TextContent = $"{Mathf.RoundToInt(exp.Value)}%";
                };
            }
        }

        // form 页（角色创建表单）= 控件束 P2/P3 typed 事件主力验收页：文本框全家 + Dropdown。
        // 每个变体类型各接一条事件 → Console，证明 C# 投影类的 typed 事件链全通。
        // 文本框 ValueChanged 逐字符触发（验收时输几个字符看 Console 几条 log）；Submitted 回车触发。
        if (pageName == "form")
        {
            // TextField：ValueChanged（逐字符）+ Submitted（回车提交）。
            if (page.TryGet<TextField>("char-name", out var name))
            {
                name.ValueChanged += e => Debug.Log($"[Showcase] char-name: \"{e.NewValue}\"");
                name.Submitted += v => Debug.Log($"[Showcase] char-name submitted: \"{v}\"");
            }
            // char-pass：password 掩码由 CSS -webkit-text-security:disc 声明（core 显示层
            // 变换，value 原文不变）；这里只 log 长度证明 value 未被掩码污染。
            if (page.TryGet<TextField>("char-pass", out var pass))
                pass.ValueChanged += e => Debug.Log($"[Showcase] char-pass changed (len={(e.NewValue?.Length ?? 0)})");
            // char-search：<input type="search"> 同样折叠为 TextField。
            if (page.TryGet<TextField>("char-search", out var search))
                search.ValueChanged += e => Debug.Log($"[Showcase] char-search: \"{e.NewValue}\"");
            // Dropdown.SelectionChanged（P3：select 弹出列表，typed 事件链）。
            if (page.TryGet<Dropdown>("char-class", out var cls))
                cls.SelectionChanged += e => Debug.Log($"[Showcase] char-class selected index = {e.NewIndex}");
            // 初始属性分配 slider：ValueChanged → 旁边数字标签（同 settings vol-master 模式）。
            // label 的 id 在 form.html 里（attr-str-val / attr-agi-val / attr-int-val）。
            string[] attrSliders = { "attr-str", "attr-agi", "attr-int" };
            foreach (string sid in attrSliders)
            {
                if (page.TryGet<Slider>(sid, out var attr)
                    && page.TryGet<TextElement>(sid + "-val", out var attrVal))
                {
                    Slider s = attr;
                    TextElement v = attrVal;
                    s.ValueChanged += e => v.TextContent = Mathf.RoundToInt(e.NewValue).ToString();
                }
            }
            // TextArea.ValueChanged（P2 多行变体类型对）。
            if (page.TryGet<TextArea>("char-bio", out var bio))
                bio.ValueChanged += e => Debug.Log($"[Showcase] char-bio changed (len={(e.NewValue?.Length ?? 0)})");
        }
    }

    /// ListView 虚拟化驱动：背包 / 邮件左侧列表。
    /// runtime ListView 是数据驱动的——data-fill 只供浏览器 preview 克隆（loom-preview.js），
    /// runtime 必须业务侧设 ItemCount + BindItem 才克隆 slot 渲染 item（见 LoomGUI.Nodes ListView）。
    /// 按 index 区分图标（Image.Src 轮换）+ badge 数量 + 耐久（背包）/ 发件人 + 主题（邮件）。子节点用
    /// Query<T> 按类型取：template 蓝图克隆后 N 个 slot 子节点 id 重复，Get<T> 全局首匹配只命中
    /// 首个 slot（Nodes.cs Get gap），故不用 id。
    /// BindItem 须先于 ItemCount 设：ItemCount setter 首次会 drain_now + DrainPendingBinds 触发 BindItem。
    void WireListViews(Container page, string pageName)
    {
        if (pageName == "inventory" && page.TryGet<ListView>("inv-list", out var invList))
        {
            string[] icons = { "item-potion", "item-chest", "item-gem", "item-scroll", "item-staff", "item-wand" };
            invList.BindItem = (item, i) =>
            {
                var dur = item.Query<ProgressBar>();
                if (dur.Count > 0) dur[0].Value = (i * 7) % 100;
                var spans = item.Query<TextElement>();
                if (spans.Count > 0) spans[0].TextContent = "x" + ((i * 13) % 99 + 1);
                var img = item.Query<Image>();
                if (img.Count > 0) img[0].Src = "res/icons/" + icons[i % icons.Length] + ".png";
            };
            invList.ItemCount = 120;
        }

        if (pageName == "mail" && page.TryGet<ListView>("mail-list", out var mailList))
        {
            string[] senders = { "系统奖励", "竞技场", "公会战报", "好友留言", "商会通知", "赛季手册" };
            string[] subjects =
            {
                "每日登录奖励已发放", "本赛季排名结算完毕", "公会贡献度更新",
                "你的基地被探访了", "本周交易汇总已生成", "新赛季手册已解锁",
                "限时活动即将开启", "背包已满请及时清理"
            };
            mailList.BindItem = (item, i) =>
            {
                var spans = item.Query<TextElement>();
                if (spans.Count >= 2)
                {
                    spans[0].TextContent = senders[i % senders.Length];
                    spans[1].TextContent = subjects[i % subjects.Length];
                }
            };
            mailList.ItemCount = 100;
        }
    }
}
