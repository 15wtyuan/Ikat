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
        Debug.Log($"[Showcase] Instantiate showcase/{page} = OK");
    }

    /// 用框架事件系统接导航：nav-card 与 back-home 都是 `<button>`（Button.Clicked）。
    /// （nav-card 原为 `<a>`/Link.Activated，围栏紧缩 a→button 后统一走 Button.Clicked。）
    /// TryGet 找不到（本页没该元素）就跳过——home 页无 back-home，其他页无 nav-card，各取所需。
    /// 闭包捕获的 page/target 是 per-iteration 局部，每次 Show 重新订阅当前页实例。
    void WireNav(Container page, string pageName)
    {
        if (page.TryGet<Button>("back-home", out var back))
            back.Clicked += () => Show("home");
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
            if (page.TryGet<Button>("btn-train", out var train)
                && page.TryGet<ProgressBar>("stat-exp", out var exp)
                && page.TryGet<TextElement>("stat-exp-val", out var expVal))
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
            // PasswordField.ValueChanged（P2 变体：掩码显示，typed 值仍为明文；证投影类对）。
            if (page.TryGet<PasswordField>("char-pass", out var pass))
                pass.ValueChanged += e => Debug.Log($"[Showcase] char-pass changed (len={(e.NewValue?.Length ?? 0)})");
            // SearchField.ValueChanged（P2 变体类型对）。
            if (page.TryGet<SearchField>("char-search", out var search))
                search.ValueChanged += e => Debug.Log($"[Showcase] char-search: \"{e.NewValue}\"");
            // Dropdown.SelectionChanged（P3：select 弹出列表，typed 事件链）。
            if (page.TryGet<Dropdown>("char-class", out var cls))
                cls.SelectionChanged += e => Debug.Log($"[Showcase] char-class selected index = {e.NewIndex}");
            // TextArea.ValueChanged（P2 多行变体类型对）。
            if (page.TryGet<TextArea>("char-bio", out var bio))
                bio.ValueChanged += e => Debug.Log($"[Showcase] char-bio changed (len={(e.NewValue?.Length ?? 0)})");
        }
    }
}
