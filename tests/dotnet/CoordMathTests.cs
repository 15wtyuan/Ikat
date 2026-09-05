using Xunit;

namespace Yio.Tests.Core
{
    public class CoordMathTests
    {
        [Fact]
        public void ScreenToDesign_CenteredFullScreen_ReturnsIdentity()
        {
            // 屏幕=1920x1080，设计=1920x1080，全屏 no safeArea
            var (dx, dy) = CoordMath.ScreenToDesign(
                screenX: 960f, screenY: 540f,       // 屏幕中心
                screenW: 1920f, screenH: 1080f,
                rootW: 1920f, rootH: 1080f,
                areaX: 0f, areaY: 0f, areaW: 1920f, areaH: 1080f,
                useSafeArea: false);

            Assert.Equal(960f, dx, 4);
            Assert.Equal(540f, dy, 4);  // y: offYTop(1080) - screenY(540) / sf(1) = 540
        }

        [Fact]
        public void ScreenToDesign_TopLeft_ReturnsDesignOrigin()
        {
            // 屏幕 1920x1080，设计 1080x1920 竖屏 shrink-to-fit
            // sf = min(1920/1080, 1080/1920) = min(1.777, 0.5625) = 0.5625
            // rendered span = 1080*0.5625=607.5 × 1920*0.5625=1080
            // offX = 0 + (1920-607.5)*0.5 = 656.25
            // offYTop = 0 + 1080 = 1080
            // screen(656.25, 1080) → dx=(656.25-656.25)/0.5625=0, dy=(1080-1080)/0.5625=0
            var (dx, dy) = CoordMath.ScreenToDesign(
                screenX: 656.25f, screenY: 1080f,
                screenW: 1920f, screenH: 1080f,
                rootW: 1080f, rootH: 1920f,
                areaX: 0f, areaY: 0f, areaW: 1920f, areaH: 1080f,
                useSafeArea: false);

            Assert.Equal(0f, dx, 2);
            Assert.Equal(0f, dy, 2);
        }

        [Fact]
        public void ScreenToDesign_SafeArea_OffsetsCorrectly()
        {
            // 刘海屏：safeArea = (100, 50, 1720, 980)，设计=1080x1920
            // sf = min(1720/1080, 980/1920) = min(1.59, 0.5104) = 0.5104
            // rendered span = 1080*0.5104=551.23 × 1920*0.5104=980
            // offX = 100 + (1720-551.23)*0.5 = 684.38
            // offYTop = 50 + 980 = 1030
            // screen(684.38, 1030) → dx=0, dy=0
            var (dx, dy) = CoordMath.ScreenToDesign(
                screenX: 684.38f, screenY: 1030f,
                screenW: 1920f, screenH: 1080f,
                rootW: 1080f, rootH: 1920f,
                areaX: 100f, areaY: 50f, areaW: 1720f, areaH: 980f,
                useSafeArea: true);

            Assert.Equal(0f, dx, 1);
            Assert.Equal(0f, dy, 1);
        }

        [Fact]
        public void ScreenToDesign_ZeroSizes_ReturnsSafeDefault()
        {
            var (dx, dy) = CoordMath.ScreenToDesign(
                screenX: 100f, screenY: 200f,
                screenW: 0f, screenH: 0f,    // 防御
                rootW: 0f, rootH: 0f,
                areaX: 0f, areaY: 0f, areaW: 0f, areaH: 0f,
                useSafeArea: true);

            // 全 0 输入退回 sf=1，屏幕左上→design(100, 方向翻转后 dy≈-199)
            Assert.True(float.IsFinite(dx));
            Assert.True(float.IsFinite(dy));
        }

        [Fact]
        public void ComputeClipBox_UnitRect_ReturnsCorrectBox()
        {
            // tl=(0,0) br=(100,200) → center(50,100) halfW=50 halfH=100
            var (x, y, z, w) = CoordMath.ComputeClipBox(0f, 0f, 100f, 200f);
            Assert.Equal(-1f, x, 4);    // -50/50
            Assert.Equal(-1f, y, 4);    // -100/100
            Assert.Equal(0.02f, z, 4);  // 1/50
            Assert.Equal(0.01f, w, 4);  // 1/100
        }

        [Fact]
        public void ComputeClipBox_ZeroArea_ReturnsSafeBlank()
        {
            var (x, y, z, w) = CoordMath.ComputeClipBox(10f, 10f, 10f, 10f);
            Assert.Equal(-2f, x);
            Assert.Equal(-2f, y);
            Assert.Equal(0f, z);
            Assert.Equal(0f, w);
        }

        [Fact]
        public void ScreenToDesign_RoundTrip_DesignCenter()
        {
            // 设计中心(540,960) 应映射到屏幕中心(960,540)
            // 全屏，1920x1080 屏，1080x1920 设计
            // sf=0.5625, offX=656.25, offYTop=1080
            // design(540,960) → screen.x=656.25+540*0.5625=960, screen.y=1080-960*0.5625=540
            var (dx, dy) = CoordMath.ScreenToDesign(
                screenX: 960f, screenY: 540f,
                screenW: 1920f, screenH: 1080f,
                rootW: 1080f, rootH: 1920f,
                areaX: 0f, areaY: 0f, areaW: 1920f, areaH: 1080f,
                useSafeArea: false);

            Assert.Equal(540f, dx, 1);
            Assert.Equal(960f, dy, 1);
        }
    }
}
