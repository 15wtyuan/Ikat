using NUnit.Framework;
using UnityEngine;

namespace Yio.Tests
{
    public class YioInputCollectorTests
    {
        // 全屏零回归验：sf=1、offX=0、offYTopDown=0 → 纯 y-flip 恒等映射
        // （screen 左下原点 y-up ↔ design 左上原点 y-down，仅 y 翻转）。
        // 映射三元组由 Driver 从 Rust yio_compute_adaptation 注入——适配数学
        // 的单源在 Rust（core adapt.rs 单测覆盖三模式），这里只验线性映射本体。
        [Test]
        public void ScreenToDesign_MapsCorrectly()
        {
            // screen (100,50) in 200x100 → design (100, 100-50-0=50)
            var design = YioInputCollector.ScreenToDesign(
                new UnityEngine.Vector2(100f, 50f), 1f, 0f, 0f, 100f);
            Assert.AreEqual(100f, design.x, 0.01f, "sf=1 → design_x = screen_x");
            Assert.AreEqual(50f, design.y, 0.01f, "design_y = screenH - screen_y（y-flip，sf=1）");
        }

        // screen (0, 100) 左上（Unity 左下原点，y=100=顶部）→ design (0, 0)
        //   验 y-flip：Unity 顶部（y=screen_h）↦ Yio 左上（design_y=0）
        [Test]
        public void ScreenToDesign_TopLeftScreen_IsTopLeftDesign()
        {
            var design = YioInputCollector.ScreenToDesign(
                new UnityEngine.Vector2(0f, 100f), 1f, 0f, 0f, 100f);
            Assert.AreEqual(0f, design.x, 0.01f);
            Assert.AreEqual(0f, design.y, 0.01f, "screen 顶部 → design y=0（左上原点）");
        }

        // screen 底部（y=0）↦ design 底部（design_y=canvas 高）—— y-flip 对称验。
        [Test]
        public void ScreenToDesign_BottomScreen_IsBottomDesign()
        {
            var design = YioInputCollector.ScreenToDesign(
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
                var back = YioInputCollector.ScreenToDesign(screen, sf, offX, offYTd, screenH);
                Assert.AreEqual(d.x, back.x, 0.001f, $"round-trip dx 失败（design={d}, screen={screen}）");
                Assert.AreEqual(d.y, back.y, 0.001f, $"round-trip dy 失败（design={d}, screen={screen}）");
            }
        }

        // —— key repeat 状态机（#76）：OS 节律合成重复 keydown ——
        // Unity 两代输入系统都不发 OS 键盘重复；collector 用 KeyRepeatState 合成
        // （0.5s 初始延迟 + 0.03s 间隔，最后按下优先，keyup 即停）。纯逻辑直测。

        [Test]
        public void KeyRepeat_FiresAfterInitialDelayThenInterval()
        {
            var st = new KeyRepeatState();
            const uint backspace = 8;   // KeyCode.Backspace
            st.OnKeyDown(backspace);

            // 延迟期内不重发（9 帧 × 0.05s = 0.45s < 0.5s）。
            for (int i = 0; i < 9; i++)
                Assert.AreEqual(0u, st.Advance(0.05f), "初始延迟期内不重发");

            // 延迟耗尽（累计 0.5s）→ 首次重发；此后每 0.03s 一次。
            Assert.AreEqual(backspace, st.Advance(0.05f), "初始延迟耗尽 → 首次重发");
            Assert.AreEqual(0u, st.Advance(0.02f), "间隔未满不发");
            Assert.AreEqual(backspace, st.Advance(0.01f), "间隔满 → 再重发");
        }

        [Test]
        public void KeyRepeat_KeyUpStopsAndClearStops()
        {
            var st = new KeyRepeatState();
            const uint left = 276;      // KeyCode.LeftArrow
            st.OnKeyDown(left);
            Assert.AreEqual(left, st.Advance(0.5f), "延迟耗尽首发");

            st.OnKeyUp(left);
            Assert.AreEqual(0u, st.Key, "keyup 清目标");
            Assert.AreEqual(0u, st.Advance(0.5f), "keyup 后不再重发");

            // Clear（失焦路径）同理。
            st.OnKeyDown(left);
            st.Clear();
            Assert.AreEqual(0u, st.Advance(1f), "Clear 后不再重发");
        }

        [Test]
        public void KeyRepeat_LastPressedKeyWinsAndOlderKeyUpDoesNotStop()
        {
            var st = new KeyRepeatState();
            const uint left = 276, right = 275;
            st.OnKeyDown(left);
            st.Advance(0.2f);
            // 按下第二键：重复目标切换 + 计时重置（最后按下优先，OS 同感）。
            st.OnKeyDown(right);
            Assert.AreEqual(0u, st.Advance(0.4f), "换键后重新走初始延迟");
            Assert.AreEqual(right, st.Advance(0.1f), "重发的是最新键");

            // 释放更早的键（left）不打断最新键的重复。
            st.OnKeyUp(left);
            Assert.AreEqual(right, st.Advance(KeyRepeatState.Interval), "旧键 keyup 不打断新键重复");
            // 释放当前目标键才停。
            st.OnKeyUp(right);
            Assert.AreEqual(0u, st.Advance(KeyRepeatState.Interval), "目标键 keyup 停止");
        }

        [Test]
        public void KeyRepeat_LongFrameDoesNotBurst()
        {
            // 超长帧（断点/卡顿 5s）：只发一次 + 计时重置整周期，不连发补帧。
            var st = new KeyRepeatState();
            const uint del = 323;       // KeyCode.Delete
            st.OnKeyDown(del);
            Assert.AreEqual(del, st.Advance(5f), "超长帧只发一次");
            Assert.AreEqual(0u, st.Advance(0.01f), "计时重置整周期（不补帧）");
            Assert.AreEqual(del, st.Advance(KeyRepeatState.Interval), "下个整周期再发");
        }
    }
}
