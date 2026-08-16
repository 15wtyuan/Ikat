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

    void Start()
    {
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

    /// m2-animation #11/#12：程序化动画（node.Play + 句柄 L3）的 driver 接线。
    /// #11 点盒子 Play（OnKey/OnHook 回调进 Console）；#12 按钮排控制同一句柄的
    /// Pause/Resume/Stop/Time seek。Play 每次新建 programmatic player（句柄换新）。
    void WireM2AnimationDrivers(Container page)
    {
        if (page.TryGet<Container>("b11-target", out var playTarget))
        {
            playTarget.On<ClickEvent>(_ =>
                playTarget.Play("m2-play-fade")
                    .OnEnd(() => Debug.Log("[Showcase] m2 #11 Play("m2-play-fade") end")));
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
        Animation handle = null;
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
                Debug.Log($"[Showcase] m2 #12 Pause @ t={handle?.Time:F2}s");
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

    /// m2-animation 页「↻ 重播」：原地重启声明式动画（Container.RestartAnimations）——
    /// player 重建、delay 重计，节点/滚动/控件值/订阅全保留。
    void ReplayCurrentPage()
    {
        // 原地重启声明式动画（Container.RestartAnimations）：player 重建、delay 重计，
        // 节点/滚动/控件值/订阅全保留——不再走销毁重实例化。
        _current?.RestartAnimations();
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
            // char-pass：<input type="password"> 现折叠为 TextField（web-only 控件，游戏自实现掩码）。
            if (page.TryGet<TextField>("char-pass", out var pass))
                pass.ValueChanged += e => Debug.Log($"[Showcase] char-pass changed (len={(e.NewValue?.Length ?? 0)})");
            // char-search：<input type="search"> 同样折叠为 TextField。
            if (page.TryGet<TextField>("char-search", out var search))
                search.ValueChanged += e => Debug.Log($"[Showcase] char-search: \"{e.NewValue}\"");
            // Dropdown.SelectionChanged（P3：select 弹出列表，typed 事件链）。
            if (page.TryGet<Dropdown>("char-class", out var cls))
                cls.SelectionChanged += e => Debug.Log($"[Showcase] char-class selected index = {e.NewIndex}");
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
