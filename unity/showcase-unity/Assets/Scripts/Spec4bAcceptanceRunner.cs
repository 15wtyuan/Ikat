using UnityEngine;
using LoomGUI;

/// <summary>
/// Spec-4b P3 验收 runner：挂在与 LoomStageDriver 同一 GameObject 上。
/// 启动后实例化 spec4b-acceptance 模板 + 跑 §5 四门（typed Get / typed event / class hit / 跨层 rect）
/// 并把每门结果 Debug.Log 出来——PlayMode 人眼对 Console 输出验收（家里机执行）。
///
/// 时序：Driver.Awake 先跑（建 stage + 加载包 + create_root）→ Runner.Start 取 Context +
/// Instantiate 模板 → Invoke 延迟读 LayoutRect（等若干 tick 跑 solve/compute）。
/// 不订阅 driver ready 事件（P2 未暴露）；用 Invoke 延迟兜底，简单可靠。
///
/// API 用法严格对照 LoomGUI 公共 API 终态契约（docs/design/public-api.md）：
///   - driver.Context 拿 UIContext（driver P2.6 已暴露）
///   - driver.Instantiate(pkg, comp) 跑 FFI instantiate + append 到 ctx.Root（P3 Task 3 加）
///   - instRoot.Get&lt;T&gt;(id) 作用域内 typed 查找
///   - Button.Clicked 是 event Action（D3 semantic sugar，无参）
///   - node.Classes.Contains(name) 查 class（C5 直 FFI has_class）
///   - node.Geometry.LayoutRect 返 Rect（X/Y/Width/Height，滞后一帧）
/// </summary>
public class Spec4bAcceptanceRunner : MonoBehaviour
{
    LoomStageDriver _driver;
    Container _instRoot;
    bool _verified;

    void Start()
    {
        _driver = GetComponent<LoomStageDriver>();
        if (_driver == null)
        {
            Debug.LogError("[Spec4b] LoomStageDriver not found on same GameObject — runner wired wrong");
            return;
        }

        // 让 driver Awake 完成（同帧 Awake 先于 Start，理论已就绪）+ Instantiate 模板。
        // Invoke 0.1s 给若干 LateUpdate 跑过的余量——主要防 ExecuteAlways EditMode 下时序差异。
        Invoke(nameof(Boot), 0.1f);
    }

    void Boot()
    {
        if (_instRoot != null) return;

        _instRoot = _driver.Instantiate("spec4b-acceptance", "spec4b-acceptance");
        if (_instRoot == null)
        {
            Debug.LogError("[Spec4b] Instantiate spec4b-acceptance failed (pkg not loaded? scene root missing?)");
            return;
        }
        Debug.Log($"[Spec4b] Instantiate spec4b-acceptance root = {_instRoot.GetType().Name} (id={_instRoot.Id})");

        // ── 门 2：作用域查找 Get<T>(id) ────────────────────────────────
        // btn-back 是 <button id="btn-back">Back</button>，模板根子树内。
        Button backBtn = null;
        try { backBtn = _instRoot.Get<Button>("btn-back"); }
        catch (System.Exception e) { Debug.LogException(e); }
        Debug.Log($"[Spec4b] Gate2 Get<Button>(\"btn-back\") = {(backBtn != null ? "OK" : "FAIL")}");

        // ── 门 4：typed 事件订阅（Clicked += Action） ───────────────────
        // D3 semantic sugar：Button.Clicked 是 event Action，+= 经 On<ClickEvent> 冒泡订阅。
        // 实际触发由 PlayMode 点击驱动——本 runner 只挂订阅 + 在 handler 里 log，验收人点按钮看 log。
        if (backBtn != null)
        {
            backBtn.Clicked += () => Debug.Log("[Spec4b] Gate4 btn-back Clicked fired ✓");
            Debug.Log("[Spec4b] Gate4 subscribed btn-back.Clicked (click it in PlayMode to fire)");
        }

        // ── 门 3：class 命中（cascade highlight → card-text 颜色应改变） ──
        // 公共 typed API 暂无 computed style 读回（Style 只反映 inline override；computed 走 FFI
        // get_node_computed_style 未在 typed 层暴露）。降级为：
        //   (a) 验 card-2 上 .highlight class 真实存在（has_class FFI 返真）
        //   (b) Query(".card-text") 能找到 span 子节点（CSS selector 子树查找路径通）
        // 完整 computed color 验收靠 PlayMode 人眼门 1（card-2 文字应为红 #e94560，card-1 文字默认浅灰）。
        try
        {
            Container card2 = _instRoot.Get<Container>("card-2");
            bool hasHighlight = card2 != null && card2.Classes.Contains("highlight");
            Debug.Log($"[Spec4b] Gate3a card-2 has class \"highlight\" = {hasHighlight} (expect true)");

            // Query(".card-text") 子树查找（CSS-like selector，验证 cascade target 节点可定位）
            if (card2 != null)
            {
                var cardTexts = card2.Query(".card-text");
                Debug.Log($"[Spec4b] Gate3b Query(\".card-text\") under card-2 = {cardTexts.Count} match(es) (expect 1)");
            }
        }
        catch (System.Exception e)
        {
            Debug.LogException(e);
        }

        // ── 门 1 + rect：跨层（HTML/CSS width:300px → solve → typed Geometry.LayoutRect.Width）──
        // 等 1-2 tick solve 完成（Instantiated 同帧 solve 还没跑；driver.LateUpdate 每 frame 一 tick）。
        Invoke(nameof(VerifyRect), 0.2f);
    }

    void VerifyRect()
    {
        if (_verified) return;
        _verified = true;
        if (_instRoot == null) return;

        try
        {
            Container card1 = _instRoot.Get<Container>("card-1");
            if (card1 == null)
            {
                Debug.LogError("[Spec4b] Gate1 Get<Container>(\"card-1\") = FAIL (not found)");
                return;
            }
            LoomGUI.Rect r = card1.Geometry.LayoutRect;
            // CSS .card { width:300px } → solve 后 LayoutRect.Width 应 ≈ 300（允许亚像素误差）
            bool widthOk = r.Width >= 295f && r.Width <= 305f;
            Debug.Log($"[Spec4b] Gate1 card-1 LayoutRect = {{X={r.X:F1}, Y={r.Y:F1}, W={r.Width:F1}, H={r.Height:F1}}} " +
                      $"(expect W≈300; {(widthOk ? "PASS" : "FAIL")})");
        }
        catch (System.Exception e)
        {
            Debug.LogException(e);
        }
    }
}
