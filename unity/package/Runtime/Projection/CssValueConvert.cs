using System;
using System.Globalization;

namespace LoomGUI
{
    /// <summary>
    /// 把 typed 值（frozen 值类型 + NodeStyle enum）转成 core apply_decl 能解析的 CSS 串，
    /// 供 C3 StyleMirror 的 inline-override flush 路径用。返回 null 表示该值是 Unset 哨兵，
    /// 调用方应据此跳过该属性（用 unset_inline_override FFI，不写进 CSS）。
    ///
    /// 输出的 CSS keyword 必须匹配 core `crates/core/src/style/mapping.rs` `apply_decl` 实际接受的字符串，
    /// 不是 CSS 标准记忆。两个关键差异已在此处吸收：
    ///  - Overflow.Clip → "hidden"：Rust OverflowMode 没有 Clip 变体（Hidden 是等价语义），
    ///    parse_overflow 只认 "hidden" 不认 "clip"。
    ///  - Thickness 四值序 TRBL：parse_four 解析 [top, right, bottom, left] → Rect{top, right, bottom, left}。
    /// </summary>
    internal static class CssValueConvert
    {
        // ── typed 重载 ───────────────────────────────────────────────

        internal static string ToCss(Length l)
        {
            switch (l.Unit)
            {
                case LengthUnit.Px:      return $"{l.Value.ToString(CultureInfo.InvariantCulture)}px";
                case LengthUnit.Percent: return $"{l.Value.ToString(CultureInfo.InvariantCulture)}%";
                case LengthUnit.Auto:    return "auto";
                case LengthUnit.Unset:   return null;   // 撤销哨兵：调用方跳过 flush
                default:                 throw new ArgumentOutOfRangeException(nameof(l), l.Unit, "unknown LengthUnit");
            }
        }

        internal static string ToCss(Color c)
        {
            if (c.IsUnset) return null;
            // 8 位 hex（#rrggbbaa）。Color 字段是 0–1 float，× 255 后取整 clamp 到 byte。
            // hex 格式化无 culture 依赖，故插值串无需 InvariantCulture。
            byte r = ClampToByte(c.R);
            byte g = ClampToByte(c.G);
            byte b = ClampToByte(c.B);
            byte a = ClampToByte(c.A);
            return FormattableString.Invariant($"#{r:x2}{g:x2}{b:x2}{a:x2}");
        }

        internal static string ToCss(Thickness t)
        {
            // CSS 四值缩写顺序 TRBL（top right bottom left），匹配 mapping.rs parse_four。
            string top    = t.Top.ToString(CultureInfo.InvariantCulture);
            string right  = t.Right.ToString(CultureInfo.InvariantCulture);
            string bottom = t.Bottom.ToString(CultureInfo.InvariantCulture);
            string left   = t.Left.ToString(CultureInfo.InvariantCulture);
            return FormattableString.Invariant($"{top} {right} {bottom} {left}");
        }

        internal static string ToCss(float f) => f.ToString(CultureInfo.InvariantCulture);

        // ── enum → CSS keyword（每 enum 的 Unset 都返 null：撤销哨兵）────────

        internal static string ToCss(DisplayMode v) => v switch
        {
            DisplayMode.Unset => null,
            DisplayMode.Block => "block",
            DisplayMode.Flex  => "flex",
            DisplayMode.None  => "none",
            _ => throw BadEnum(v),
        };

        internal static string ToCss(FlexDirection v) => v switch
        {
            FlexDirection.Unset         => null,
            FlexDirection.Row           => "row",
            FlexDirection.RowReverse    => "row-reverse",
            FlexDirection.Column        => "column",
            FlexDirection.ColumnReverse => "column-reverse",
            _ => throw BadEnum(v),
        };

        internal static string ToCss(FlexWrap v) => v switch
        {
            // 注：core apply_decl "flex-wrap" 只显式认 "wrap"，其余值归 NoWrap——
            // 即 "wrap-reverse" 经 CSS 回到 core 会变 NoWrap（往返失真，属 core 限制）。
            // 投影层仍发标准 CSS 串表达 typed 意图，不替 core 静默降级。
            FlexWrap.Unset       => null,
            FlexWrap.NoWrap      => "nowrap",
            FlexWrap.Wrap        => "wrap",
            FlexWrap.WrapReverse => "wrap-reverse",
            _ => throw BadEnum(v),
        };

        internal static string ToCss(JustifyContent v) => v switch
        {
            JustifyContent.Unset        => null,
            JustifyContent.FlexStart    => "flex-start",
            JustifyContent.FlexEnd      => "flex-end",
            JustifyContent.Center       => "center",
            JustifyContent.SpaceBetween => "space-between",
            JustifyContent.SpaceAround  => "space-around",
            JustifyContent.SpaceEvenly  => "space-evenly",
            _ => throw BadEnum(v),
        };

        internal static string ToCss(AlignItems v) => v switch
        {
            AlignItems.Unset     => null,
            AlignItems.Stretch   => "stretch",
            AlignItems.FlexStart => "flex-start",
            AlignItems.FlexEnd   => "flex-end",
            AlignItems.Center    => "center",
            AlignItems.Baseline  => "baseline",
            _ => throw BadEnum(v),
        };

        internal static string ToCss(Overflow v) => v switch
        {
            // C# 用 CSS 标准名 Clip，但 core OverflowMode::Hidden 接受 "hidden"，
            // parse_overflow 无 "clip" 分支——映射到 "hidden" 保往返不丢。
            Overflow.Unset   => null,
            Overflow.Visible => "visible",
            Overflow.Clip    => "hidden",
            Overflow.Auto    => "auto",
            Overflow.Scroll  => "scroll",
            _ => throw BadEnum(v),
        };

        internal static string ToCss(PositionMode v) => v switch
        {
            // 注：core apply_decl "position" 仅显式认 "absolute"/"relative"，
            // "static" 落到 `_ => false`（围栏外，整条拒绝）。投影层仍发标准 CSS 串；
            // Static 是布局默认态，CSS 拒收等于"无操作"，与 typed 语义一致。
            PositionMode.Unset    => null,
            PositionMode.Static   => "static",
            PositionMode.Relative => "relative",
            PositionMode.Absolute => "absolute",
            _ => throw BadEnum(v),
        };

        // ── dispatch：供 C3 StyleMirror（属性值是 object 装箱）─────────

        internal static string ToCss(object value) => value switch
        {
            Length l         => ToCss(l),
            Color c          => ToCss(c),
            Thickness t      => ToCss(t),
            float f          => ToCss(f),
            DisplayMode v    => ToCss(v),
            FlexDirection v  => ToCss(v),
            FlexWrap v       => ToCss(v),
            JustifyContent v => ToCss(v),
            AlignItems v     => ToCss(v),
            Overflow v       => ToCss(v),
            PositionMode v   => ToCss(v),
            null             => throw new ArgumentNullException(nameof(value)),
            _                => throw new ArgumentException($"unsupported css value type: {value.GetType()}", nameof(value)),
        };

        // ── helpers ──────────────────────────────────────────────────

        private static byte ClampToByte(float f)
        {
            // Round * 255 后 clamp 到 [0,255] 防 float 越界（>1f / <0f）。
            // AwayFromZero：50% (0.5 * 255 = 127.5) → 128 = CSS 标准 0x80。
            int v = (int)Math.Round(f * 255f, MidpointRounding.AwayFromZero);
            return (byte)Math.Clamp(v, 0, 255);
        }

        private static ArgumentException BadEnum<T>(T v) =>
            new ArgumentException($"unsupported {typeof(T).Name} value: {v}", nameof(v));
    }
}
