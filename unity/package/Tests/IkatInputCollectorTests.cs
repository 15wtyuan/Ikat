using NUnit.Framework;
using UnityEngine;

namespace Ikat.Tests
{
    public class IkatInputCollectorTests
    {
        // 全屏零回归验：sf=1、offX=0、offYTopDown=0 → 纯 y-flip 恒等映射
        // （screen 左下原点 y-up ↔ design 左上原点 y-down，仅 y 翻转）。
        // 映射三元组由 Driver 从 Rust ikat_compute_adaptation 注入——适配数学
        // 的单源在 Rust（core adapt.rs 单测覆盖三模式），这里只验线性映射本体。
        [Test]
        public void ScreenToDesign_MapsCorrectly()
        {
            // screen (100,50) in 200x100 → design (100, 100-50-0=50)
            var design = IkatInputCollector.ScreenToDesign(
                new UnityEngine.Vector2(100f, 50f), 1f, 0f, 0f, 100f);
            Assert.AreEqual(100f, design.x, 0.01f, "sf=1 → design_x = screen_x");
            Assert.AreEqual(50f, design.y, 0.01f, "design_y = screenH - screen_y（y-flip，sf=1）");
        }

        // screen (0, 100) 左上（Unity 左下原点，y=100=顶部）→ design (0, 0)
        //   验 y-flip：Unity 顶部（y=screen_h）↦ Ikat 左上（design_y=0）
        [Test]
        public void ScreenToDesign_TopLeftScreen_IsTopLeftDesign()
        {
            var design = IkatInputCollector.ScreenToDesign(
                new UnityEngine.Vector2(0f, 100f), 1f, 0f, 0f, 100f);
            Assert.AreEqual(0f, design.x, 0.01f);
            Assert.AreEqual(0f, design.y, 0.01f, "screen 顶部 → design y=0（左上原点）");
        }

        // screen 底部（y=0）↦ design 底部（design_y=canvas 高）—— y-flip 对称验。
        [Test]
        public void ScreenToDesign_BottomScreen_IsBottomDesign()
        {
            var design = IkatInputCollector.ScreenToDesign(
                new UnityEngine.Vector2(0f, 0f), 1f, 0f, 0f, 100f);
            Assert.AreEqual(0f, design.x, 0.01f);
            Assert.AreEqual(100f, design.y, 0.01f, "screen 底部 → design y=canvas 高");
        }

        // letterbox 偏移 + 缩放 round-trip：三元组含偏移（Letterbox 居中 / Fit 铺满 safe 区
        // 都编码在 offX/offYTopDown 里）。场景：sf=1.6、offX=40（左侧 40px 刘海）、offYTopDown=80
        // （垂直 letterbox 上黑边 80）。
        //   前向（top-down）：screen.x = 40 + dx*1.6；screenTD.y = 80 + dy*1.6
        //   逆：dx = (screen.x - 40)/1.6；dy = (screenTD.y - 80)/1.6 → 恒等回原 dx,dy ✓
        [Test]
        public void ScreenToDesign_NotchedSafeArea_RoundTrip()
        {
            const float sf = 1.6f, offX = 40f, offYTd = 80f, screenH = 800f;

            UnityEngine.Vector2[] designPoints = new[]
            {
                new UnityEngine.Vector2(0f, 0f),       // 左上（span 左上，恰在刘海右沿）
                new UnityEngine.Vector2(200f, 0f),     // 右上
                new UnityEngine.Vector2(0f, 400f),     // 左下
                new UnityEngine.Vector2(200f, 400f),   // 右下
                new UnityEngine.Vector2(100f, 200f),   // 中心
                new UnityEngine.Vector2(50f, 350f),    // 刘海右沿附近
            };
            foreach (var d in designPoints)
            {
                // 前向：design → screen（top-down 公式；Input y-up → top-down = screenH - y）
                var screenTd = new UnityEngine.Vector2(offX + d.x * sf, offYTd + d.y * sf);
                var screen = new UnityEngine.Vector2(screenTd.x, screenH - screenTd.y);
                // 逆：screen → design
                var back = IkatInputCollector.ScreenToDesign(screen, sf, offX, offYTd, screenH);
                Assert.AreEqual(d.x, back.x, 0.001f, $"round-trip dx 失败（design={d}, screen={screen}）");
                Assert.AreEqual(d.y, back.y, 0.001f, $"round-trip dy 失败（design={d}, screen={screen}）");
            }
        }
    }
}
