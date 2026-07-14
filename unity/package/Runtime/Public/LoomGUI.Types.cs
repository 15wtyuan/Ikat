// LoomGUI Frozen Public API: Value types & enums
// See docs/superpowers/specs/2026-07-15-frozen-public-api-design.md

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
        static NotImplementedException NE() => new NotImplementedException();
    }

    public enum LengthUnit { Px, Percent, Auto }

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
        public Color(float r, float g, float b, float a = 1f) { throw NE(); }
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

    public enum DisplayMode { Block, Flex, None }
    public enum FlexDirection { Row, RowReverse, Column, ColumnReverse }
    public enum FlexWrap { NoWrap, Wrap, WrapReverse }
    public enum JustifyContent { FlexStart, FlexEnd, Center, SpaceBetween, SpaceAround, SpaceEvenly }
    public enum AlignItems { Stretch, FlexStart, FlexEnd, Center, Baseline }
    public enum Overflow { Visible, Clip, Auto, Scroll }
    public enum PositionMode { Static, Relative, Absolute }
    public enum Visibility { Visible, Hidden }
    public enum ScrollBehavior { Instant, Smooth }

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
