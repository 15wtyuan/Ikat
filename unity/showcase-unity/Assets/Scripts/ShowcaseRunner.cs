using UnityEngine;
using LoomGUI;

// OnGUI 用 UnityEngine.Rect / UnityEngine.Color;LoomGUI 命名空间也有同名类型,
// using LoomGUI 会让 Rect/Color 歧义(CS0104)。alias 显式锁定到 UnityEngine 侧。
using Rect = UnityEngine.Rect;
using Color = UnityEngine.Color;

/// <summary>
/// PlayMode showcase 逐页查看器：OnGUI 画页签按钮，点击切换 showcase 8 页。
/// 挂在与 LoomStageDriver 同一 GameObject 上（仿 Spec4bAcceptanceRunner）。
///
/// 用法（家里机）：
///   1. 选中 LoomGUI GameObject → Inspector → Add Component → ShowcaseRunner
///   2. 把同 GameObject 上其他 runner（Spec4bAcceptanceRunner / VisualAcceptanceRunner）
///      的 enabled 关掉——多个 runner 同帧各 Instantiate 会叠在一起看不清。
///   3. Play → 左上角点页签切页；Game 视图看该页渲染；当前页按钮高亮黄。
///
/// 切页靠 Node.Dispose() 销毁旧实例（递归清子 + Rust remove_node + 后端镜像 GO 下帧清），
/// 再 Instantiate 新页 append 到 ctx.Root。driver 已在 Awake 加载 showcase 包，所以
/// Instantiate("showcase", stem) 直接可用。
///
/// 验收 7-20/7-21 改动时各页看什么：
///   - 圆角 border (P2-A)：lab(16处)/shop(7)/home(4) 等页的边框/阴影圆角，边角不突出
///   - 真 CSS block (P1)：裸 div 子元素垂直堆叠、不被 flex-grow 拉伸
///   - Image bg-color：带 background-color 的 img 有底色
///   - TextField/Password/Search 投影：form 页三种输入框类型正确
/// </summary>
public class ShowcaseRunner : MonoBehaviour
{
    static readonly string[] PAGES =
    { "home", "settings", "mail", "inventory", "shop", "character", "form", "lab" };

    LoomStageDriver _driver;
    Container _current;
    string _shown;   // 当前显示页名（OnGUI 高亮 + 防重复点击）

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
            _current.Dispose();   // 递归销毁旧页（Rust remove_node + 后端镜像下帧清）
            _current = null;
        }
        _current = _driver.Instantiate("showcase", page);
        _shown = _current != null ? page : null;
        Debug.Log($"[Showcase] Instantiate showcase/{page} = {(_current != null ? "OK" : "FAIL (pkg not loaded? comp not found?)")}");
    }

    void OnGUI()
    {
        GUI.skin.button.fontSize = 18;
        GUILayout.BeginArea(new Rect(8f, 40f, 1180f, 44f));   // y=40 避开 driver FPS 读数
        GUILayout.BeginHorizontal();
        foreach (var p in PAGES)
        {
            var prev = GUI.color;
            if (_shown == p) GUI.color = Color.yellow;   // 当前页高亮
            if (GUILayout.Button(p, GUILayout.Width(130f), GUILayout.Height(36f)))
                Show(p);
            GUI.color = prev;
        }
        GUILayout.EndHorizontal();
        GUILayout.EndArea();
    }
}
