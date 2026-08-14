using System;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    public class CssValueConvertTests
    {
        // ── Length ───────────────────────────────────────────────────
        [Fact]
        public void LengthPx() => Assert.Equal("100px", CssValueConvert.ToCss(Length.Px(100)));

        [Fact]
        public void LengthPct() => Assert.Equal("50%", CssValueConvert.ToCss(Length.Pct(50)));

        [Fact]
        public void LengthPxFractional() => Assert.Equal("12.5px", CssValueConvert.ToCss(Length.Px(12.5f)));

        [Fact]
        public void LengthAuto() => Assert.Equal("auto", CssValueConvert.ToCss(Length.Auto()));

        [Fact]
        public void LengthUnsetIsNull() => Assert.Null(CssValueConvert.ToCss(Length.Unset()));

        // ── Color ────────────────────────────────────────────────────
        [Fact]
        public void ColorHexRedOpaque() => Assert.Equal("#ff0000ff", CssValueConvert.ToCss(new Color(1f, 0f, 0f, 1f)));

        [Fact]
        public void ColorHexWhiteHalfAlpha() => Assert.Equal("#ffffff80", CssValueConvert.ToCss(new Color(1f, 1f, 1f, 0.5f)));

        [Fact]
        public void ColorHexBlackTransparent() => Assert.Equal("#00000000", CssValueConvert.ToCss(new Color(0f, 0f, 0f, 0f)));

        [Fact]
        public void ColorUnsetIsNull() => Assert.Null(CssValueConvert.ToCss(Color.Unset));

        // ── Thickness ────────────────────────────────────────────────
        // 输出顺序 TRBL（top right bottom left）匹配 core parse_four 解析序。
        // ctor 参数序（left, top, right, bottom）—— toCss 再按 TRBL 重排输出。
        [Fact]
        public void ThicknessTrbl() => Assert.Equal("10 20 30 40",
            CssValueConvert.ToCss(new Thickness(left: 40, top: 10, right: 20, bottom: 30)));

        // ── Float ────────────────────────────────────────────────────
        [Fact]
        public void FloatInvariant() => Assert.Equal("1.5", CssValueConvert.ToCss(1.5f));

        [Fact]
        public void FloatInvariantInteger() => Assert.Equal("100", CssValueConvert.ToCss(100f));

        [Fact]
        public void FloatInvariantLarge() => Assert.Equal("1234.5", CssValueConvert.ToCss(1234.5f));

        // ── DisplayMode ──────────────────────────────────────────────
        [Fact]
        public void DisplayBlock() => Assert.Equal("block", CssValueConvert.ToCss(DisplayMode.Block));

        [Fact]
        public void DisplayFlex() => Assert.Equal("flex", CssValueConvert.ToCss(DisplayMode.Flex));

        [Fact]
        public void DisplayNone() => Assert.Equal("none", CssValueConvert.ToCss(DisplayMode.None));

        [Fact]
        public void DisplayUnsetIsNull() => Assert.Null(CssValueConvert.ToCss(DisplayMode.Unset));

        // ── FlexDirection ────────────────────────────────────────────
        [Fact]
        public void FlexDirectionRow() => Assert.Equal("row", CssValueConvert.ToCss(FlexDirection.Row));

        [Fact]
        public void FlexDirectionColumn() => Assert.Equal("column", CssValueConvert.ToCss(FlexDirection.Column));

        [Fact]
        public void FlexDirectionRowReverse() => Assert.Equal("row-reverse", CssValueConvert.ToCss(FlexDirection.RowReverse));

        [Fact]
        public void FlexDirectionColumnReverse() => Assert.Equal("column-reverse", CssValueConvert.ToCss(FlexDirection.ColumnReverse));

        [Fact]
        public void FlexDirectionUnsetIsNull() => Assert.Null(CssValueConvert.ToCss(FlexDirection.Unset));

        // ── FlexWrap ─────────────────────────────────────────────────
        [Fact]
        public void FlexWrapNoWrap() => Assert.Equal("nowrap", CssValueConvert.ToCss(FlexWrap.NoWrap));

        [Fact]
        public void FlexWrapWrap() => Assert.Equal("wrap", CssValueConvert.ToCss(FlexWrap.Wrap));

        [Fact]
        public void FlexWrapWrapReverse() => Assert.Equal("wrap-reverse", CssValueConvert.ToCss(FlexWrap.WrapReverse));

        // ── JustifyContent ───────────────────────────────────────────
        [Fact]
        public void JustifyContentSpaceBetween() => Assert.Equal("space-between", CssValueConvert.ToCss(JustifyContent.SpaceBetween));

        [Fact]
        public void JustifyContentFlexStart() => Assert.Equal("flex-start", CssValueConvert.ToCss(JustifyContent.FlexStart));

        [Fact]
        public void JustifyContentSpaceAround() => Assert.Equal("space-around", CssValueConvert.ToCss(JustifyContent.SpaceAround));

        [Fact]
        public void JustifyContentSpaceEvenly() => Assert.Equal("space-evenly", CssValueConvert.ToCss(JustifyContent.SpaceEvenly));

        [Fact]
        public void JustifyContentFlexEnd() => Assert.Equal("flex-end", CssValueConvert.ToCss(JustifyContent.FlexEnd));

        [Fact]
        public void JustifyContentCenter() => Assert.Equal("center", CssValueConvert.ToCss(JustifyContent.Center));

        // ── AlignItems ───────────────────────────────────────────────
        [Fact]
        public void AlignItemsCenter() => Assert.Equal("center", CssValueConvert.ToCss(AlignItems.Center));

        [Fact]
        public void AlignItemsStretch() => Assert.Equal("stretch", CssValueConvert.ToCss(AlignItems.Stretch));

        [Fact]
        public void AlignItemsFlexStart() => Assert.Equal("flex-start", CssValueConvert.ToCss(AlignItems.FlexStart));

        [Fact]
        public void AlignItemsFlexEnd() => Assert.Equal("flex-end", CssValueConvert.ToCss(AlignItems.FlexEnd));

        [Fact]
        public void AlignItemsBaseline() => Assert.Equal("baseline", CssValueConvert.ToCss(AlignItems.Baseline));

        // ── Overflow ─────────────────────────────────────────────────
        // C# 用 Clip 命名（CSS overflow:clip 标准），但 core OverflowMode::Hidden
        // 接受的 CSS 串是 "hidden"（parse_overflow 无 "clip" 分支），故映射 Clip→"hidden"。
        [Fact]
        public void OverflowClipEmitsHidden() => Assert.Equal("hidden", CssValueConvert.ToCss(Overflow.Clip));

        [Fact]
        public void OverflowVisible() => Assert.Equal("visible", CssValueConvert.ToCss(Overflow.Visible));

        [Fact]
        public void OverflowAuto() => Assert.Equal("auto", CssValueConvert.ToCss(Overflow.Auto));

        [Fact]
        public void OverflowScroll() => Assert.Equal("scroll", CssValueConvert.ToCss(Overflow.Scroll));

        [Fact]
        public void OverflowUnsetIsNull() => Assert.Null(CssValueConvert.ToCss(Overflow.Unset));

        // ── PositionMode ─────────────────────────────────────────────
        [Fact]
        public void PositionModeAbsolute() => Assert.Equal("absolute", CssValueConvert.ToCss(PositionMode.Absolute));

        [Fact]
        public void PositionModeRelative() => Assert.Equal("relative", CssValueConvert.ToCss(PositionMode.Relative));

        [Fact]
        public void PositionModeStatic() => Assert.Equal("static", CssValueConvert.ToCss(PositionMode.Static));

        [Fact]
        public void PositionModeUnsetIsNull() => Assert.Null(CssValueConvert.ToCss(PositionMode.Unset));

        // ── ToCss(object) dispatch ───────────────────────────────────
        [Fact]
        public void ObjectDispatchLength() => Assert.Equal("100px", CssValueConvert.ToCss((object)Length.Px(100)));

        [Fact]
        public void ObjectDispatchColor() => Assert.Equal("#ff0000ff", CssValueConvert.ToCss((object)new Color(1f, 0f, 0f, 1f)));

        [Fact]
        public void ObjectDispatchThickness() => Assert.Equal("10 20 30 40",
            CssValueConvert.ToCss((object)new Thickness(left: 40, top: 10, right: 20, bottom: 30)));

        [Fact]
        public void ObjectDispatchFloat() => Assert.Equal("1.5", CssValueConvert.ToCss((object)1.5f));

        [Fact]
        public void ObjectDispatchEnum() => Assert.Equal("flex", CssValueConvert.ToCss((object)DisplayMode.Flex));

        [Fact]
        public void ObjectDispatchUnsetLengthReturnsNull() => Assert.Null(CssValueConvert.ToCss((object)Length.Unset()));

        [Fact]
        public void ObjectDispatchUnsetEnumReturnsNull() => Assert.Null(CssValueConvert.ToCss((object)DisplayMode.Unset));

        [Fact]
        public void ObjectDispatchNullThrows() =>
            Assert.Throws<ArgumentNullException>(() => CssValueConvert.ToCss((object)null));

        [Fact]
        public void ObjectDispatchUnsupportedThrows() =>
            Assert.Throws<ArgumentException>(() => CssValueConvert.ToCss((object)"raw string"));
    }
}
