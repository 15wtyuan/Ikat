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
    }
}
