using UnityEngine;
using LoomGUI;

// OnGUI 用 UnityEngine.Rect / UnityEngine.Color;LoomGUI 命名空间也有同名类型,
// using LoomGUI 会让 Rect/Color 歧义(CS0104)。alias 显式锁定到 UnityEngine 侧。
using Rect = UnityEngine.Rect;
using Color = UnityEngine.Color;

/// <summary>
/// PlayMode P1/P2 验收页查看器：OnGUI 画按钮切换 p1-block / p2-visual 验收页。
/// 挂在与 LoomStageDriver 同一 GameObject 上（仿 Spec4bAcceptanceRunner）。
///
/// 这两页来自 showcase 工作区的 spec4b-acceptance package（showcase/spec4b/*.html 经
/// loom-pkg build 打进 spec4b-acceptance.pkg.bin）。所以 Instantiate 走
/// ("spec4b-acceptance", stem)。
///
/// headless 已断几何/computed（BlockLayoutTests 验 flex-grow、VisualDecorationTests 验
/// border 进 computed style），本 runner 只在 PlayMode 做视觉确认：
///   - p2-visual：#rb 红色圆角边框（边角跟随半径，不再直角突出）、#rs 圆角绿阴影
///   - p1-block：两个 .item 垂直堆叠、保持 height:40（真 block 忽略 flex-grow，不被拉到 ~140）
///
/// 用法：选中 LoomGUI GameObject → Add Component → VisualAcceptanceRunner，关掉其他 runner，
/// Play → 左上角点按钮切页（默认 p2-visual，最新改动）。
/// </summary>
public class VisualAcceptanceRunner : MonoBehaviour
{
    static readonly string[] PAGES = { "p2-visual-acceptance", "p1-block-acceptance" };

    LoomStageDriver _driver;
    Container _current;
    string _shown;

    void Start()
    {
        _driver = GetComponent<LoomStageDriver>();
        if (_driver == null)
        {
            Debug.LogError("[VisualAcceptance] LoomStageDriver not found on same GameObject — runner wired wrong");
            return;
        }
        Invoke(nameof(Boot), 0.1f);
    }

    void Boot()
    {
        if (_current == null) Show("p2-visual-acceptance");   // 默认最新改动
    }

    void Show(string page)
    {
        if (_shown == page) return;
        if (_current != null)
        {
            _current.Dispose();
            _current = null;
        }
        _current = _driver.Instantiate("spec4b-acceptance", page);
        _shown = _current != null ? page : null;
        Debug.Log($"[VisualAcceptance] Instantiate spec4b-acceptance/{page} = {(_current != null ? "OK" : "FAIL (pkg not loaded? comp not found? did you rebuild showcase.pkg.bin?)")}");
    }

    void OnGUI()
    {
        GUI.skin.button.fontSize = 18;
        GUILayout.BeginArea(new Rect(8f, 40f, 700f, 44f));   // y=40 避开 driver FPS 读数
        GUILayout.BeginHorizontal();
        foreach (var p in PAGES)
        {
            var prev = GUI.color;
            if (_shown == p) GUI.color = Color.yellow;
            if (GUILayout.Button(p, GUILayout.Width(220f), GUILayout.Height(36f)))
                Show(p);
            GUI.color = prev;
        }
        GUILayout.EndHorizontal();
        GUILayout.EndArea();
    }
}
