using System;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    /// <summary>
    /// 验 C# <see cref="NodeKind"/> 抄写正确性：变体数 + 每个判别值与
    /// <c>crates/core/src/scene/node.rs</c> 的 <c>#[repr(u8)] NodeKind</c> 一致。
    ///
    /// 这是 B2 能独立验的全部（纯 managed，不调 dll）。
    /// get_node_kind 实际 round-trip（FFI byte → typed Node）defer 到 B3/C2。
    /// </summary>
    public class NodeKindTests
    {
        // ── 关键判别值（锁定防漂移；对应 node.rs repr_tests::kind_as_u8_is_discriminant）──

        [Fact]
        public void ContainerIsZero() => Assert.Equal((byte)0, (byte)NodeKind.Container);

        [Fact]
        public void TextNodeIsOne() => Assert.Equal((byte)1, (byte)NodeKind.TextNode);

        [Fact]
        public void ButtonIsSix() => Assert.Equal((byte)6, (byte)NodeKind.Button);

        [Fact]
        public void ImageIsEight() => Assert.Equal((byte)8, (byte)NodeKind.Image);

        [Fact]
        public void CanvasIsTwentyTwo() => Assert.Equal((byte)22, (byte)NodeKind.Canvas);

        // ── 全变体逐个验判别值（显式声明的 Rust 顺序 0..22）──────────────
        // 任何变体名抄错 / 顺序错位 / 判别值漂移 → 对应 Fact 红。

        [Fact]
        public void TextBlockIsTwo() => Assert.Equal((byte)2, (byte)NodeKind.TextBlock);

        [Fact]
        public void TextElementIsThree() => Assert.Equal((byte)3, (byte)NodeKind.TextElement);

        [Fact]
        public void LineBreakIsFour() => Assert.Equal((byte)4, (byte)NodeKind.LineBreak);

        [Fact]
        public void LabelIsFive() => Assert.Equal((byte)5, (byte)NodeKind.Label);

        [Fact]
        public void LinkIsSeven() => Assert.Equal((byte)7, (byte)NodeKind.Link);

        [Fact]
        public void TextFieldIsNine() => Assert.Equal((byte)9, (byte)NodeKind.TextField);

        [Fact]
        public void NumberFieldIsTen() => Assert.Equal((byte)10, (byte)NodeKind.NumberField);

        [Fact]
        public void SliderIsEleven() => Assert.Equal((byte)11, (byte)NodeKind.Slider);

        [Fact]
        public void ToggleIsTwelve() => Assert.Equal((byte)12, (byte)NodeKind.Toggle);

        [Fact]
        public void RadioButtonIsThirteen() => Assert.Equal((byte)13, (byte)NodeKind.RadioButton);

        [Fact]
        public void TextAreaIsFourteen() => Assert.Equal((byte)14, (byte)NodeKind.TextArea);

        [Fact]
        public void DropdownIsFifteen() => Assert.Equal((byte)15, (byte)NodeKind.Dropdown);

        [Fact]
        public void OptionItemIsSixteen() => Assert.Equal((byte)16, (byte)NodeKind.OptionItem);

        [Fact]
        public void ProgressBarIsSeventeen() => Assert.Equal((byte)17, (byte)NodeKind.ProgressBar);

        [Fact]
        public void ListViewIsEighteen() => Assert.Equal((byte)18, (byte)NodeKind.ListView);

        [Fact]
        public void ListItemIsNineteen() => Assert.Equal((byte)19, (byte)NodeKind.ListItem);

        [Fact]
        public void SlotIsTwenty() => Assert.Equal((byte)20, (byte)NodeKind.Slot);

        [Fact]
        public void CustomElementIsTwentyOne() => Assert.Equal((byte)21, (byte)NodeKind.CustomElement);

        // ── 结构不变量：变体数 + 紧凑 0..N-1（无空洞、无跳号）──────────

        /// <summary>
        /// Rust node.rs 当前 23 变体。若 Rust 加/删变体未同步 C# → 此测红，
        /// 提醒看护 ABI 对齐（同步两侧 enum）。
        /// </summary>
        [Fact]
        public void VariantCountMatchesRust() => Assert.Equal(23, Enum.GetNames<NodeKind>().Length);

        /// <summary>
        /// 显式赋值防隐式错位：最大判别值 == 变体数 - 1 验全变体紧凑连续
        /// （无重复赋值、无空洞）。配合上面逐变体 Fact，双重锁拷写正确性。
        /// </summary>
        [Fact]
        public void AllValuesContiguousFromZero()
        {
            byte max = (byte)NodeKind.Canvas; // 末变体（声明序最后）
            Assert.Equal((byte)(Enum.GetNames<NodeKind>().Length - 1), max);
        }
    }
}
