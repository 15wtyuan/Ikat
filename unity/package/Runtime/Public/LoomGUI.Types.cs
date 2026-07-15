// LoomGUI Frozen Public API: Value types & enums
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)

using System;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    public readonly struct Length
    {
        public float Value { get { throw NE(); } }
        public LengthUnit Unit { get { throw NE(); } }
        public static Length Px(float v) { throw NE(); }
        public static Length Pct(float v) { throw NE(); }
        public static Length Auto() { throw NE(); }
        public static Length Unset() { throw NE(); }   // inline override 撤销哨兵：getter 未写过返回此，setter 写此 = 撤销回落 CSS
        static NotImplementedException NE() => new NotImplementedException();
    }

    public enum LengthUnit { Px, Percent, Auto, Unset }

    public readonly struct Thickness
    {
        public float Left { get { throw NE(); } }
        public float Top { get { throw NE(); } }
        public float Right { get { throw NE(); } }
        public float Bottom { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public readonly struct Color
    {
        public float R { get { throw NE(); } }
        public float G { get { throw NE(); } }
        public float B { get { throw NE(); } }
        public float A { get { throw NE(); } }
        public bool IsUnset { get { throw NE(); } }   // true = 未被 typed 层覆盖（Unset 哨兵），getter 据此返回
        public Color(float r, float g, float b, float a = 1f) { throw NE(); }
        public static Color Unset { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public readonly struct Vector2
    {
        public float X { get { throw NE(); } }
        public float Y { get { throw NE(); } }
        public Vector2(float x, float y) { throw NE(); }
        public static Vector2 Zero { get { throw NE(); } }
        public static Vector2 One { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public readonly struct Rect
    {
        public float X { get { throw NE(); } }
        public float Y { get { throw NE(); } }
        public float Width { get { throw NE(); } }
        public float Height { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public enum DisplayMode { Unset, Block, Flex, None }
    public enum FlexDirection { Unset, Row, RowReverse, Column, ColumnReverse }
    public enum FlexWrap { Unset, NoWrap, Wrap, WrapReverse }
    public enum JustifyContent { Unset, FlexStart, FlexEnd, Center, SpaceBetween, SpaceAround, SpaceEvenly }
    public enum AlignItems { Unset, Stretch, FlexStart, FlexEnd, Center, Baseline }
    public enum Overflow { Unset, Visible, Clip, Auto, Scroll }
    public enum PositionMode { Unset, Static, Relative, Absolute }
    public enum Visibility { Unset, Visible, Hidden }
    public enum ScrollBehavior { Instant, Smooth }   // 方法参数，非 Style 属性，无需 Unset

    // 指针键：对齐 web MouseEvent.button（0=左/1=中/2=右）但用枚举自解释。
    public enum PointerButton { Left, Middle, Right }

    public enum KeyCode
    {
        None, Enter, Escape, Tab, Space, Backspace, Delete,
        Left, Right, Up, Down,
        A, B, C, D, E, F, G, H, I, J, K, L, M,
        N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        D0, D1, D2, D3, D4, D5, D6, D7, D8, D9,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    }

    [Flags]
    public enum KeyModifiers { None = 0, Shift = 1, Control = 2, Alt = 4 }

    public struct ValueChangedEvent<T>
    {
        public T OldValue { get { throw NE(); } }
        public T NewValue { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct SelectionChangedEvent
    {
        public int OldIndex { get { throw NE(); } }
        public int NewIndex { get { throw NE(); } }
        public string OldValue { get { throw NE(); } }
        public string NewValue { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct TextSelection
    {
        public int Start { get { throw NE(); } set { throw NE(); } }
        public int End { get { throw NE(); } set { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }
}
