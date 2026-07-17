// LoomGUI Frozen Public API: Value types & enums
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)

using System;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    public readonly struct Length
    {
        public float Value { get; }
        public LengthUnit Unit { get; }
        private Length(float value, LengthUnit unit) { Value = value; Unit = unit; }
        public static Length Px(float v) => new Length(v, LengthUnit.Px);
        public static Length Pct(float v) => new Length(v, LengthUnit.Percent);
        public static Length Auto() => new Length(0f, LengthUnit.Auto);
        public static Length Unset() => new Length(0f, LengthUnit.Unset);   // inline override 撤销哨兵：getter 未写过返回此，setter 写此 = 撤销回落 CSS
    }

    public enum LengthUnit { Px, Percent, Auto, Unset }

    public readonly struct Thickness
    {
        public float Left { get; }
        public float Top { get; }
        public float Right { get; }
        public float Bottom { get; }
        // 补全 ctor（frozen 仅约束既有成员不删/不改，补构造不算改签名）。
        // 参数顺序按字段声明序：left, top, right, bottom。
        public Thickness(float left, float top, float right, float bottom)
        {
            Left = left; Top = top; Right = right; Bottom = bottom;
        }
    }

    public readonly struct Color
    {
        public float R { get; }
        public float G { get; }
        public float B { get; }
        public float A { get; }
        public bool IsUnset { get; }   // true = 未被 typed 层覆盖（Unset 哨兵），getter 据此返回

        // 公共 ctor 强制 IsUnset=false（用户态颜色必然是已设置）；IsUnset=true 仅由 Unset factory 获得，
        // 故另设 private 5-参 ctor 让 Unset 走特化路径而不破公共 ctor 签名。
        public Color(float r, float g, float b, float a = 1f) : this(r, g, b, a, isUnset: false) { }
        private Color(float r, float g, float b, float a, bool isUnset)
        {
            R = r; G = g; B = b; A = a; IsUnset = isUnset;
        }
        public static Color Unset => new Color(0f, 0f, 0f, 0f, isUnset: true);
    }

    // 2D 向量（Position / Scale / Origin / 滚动点等）。值语义：等号按字段比较（struct 默认）。
    // 业务侧通过 new Vector2(x,y) 构造；Zero/One 是常用常量。投影层（C4 NodeTransform）镜像
    // default 与业务语义对齐：Position/Origin 默认 Zero（不位移）、Scale 默认 One（不缩放）。
    public readonly struct Vector2
    {
        public float X { get; }
        public float Y { get; }
        public Vector2(float x, float y) { X = x; Y = y; }
        public static Vector2 Zero => default;   // (0,0)；default(Vector2) 直接给零值，免 alloc
        public static Vector2 One => new Vector2(1f, 1f);   // 不缩放 / 不位移语义哨兵
    }

    // 矩形（x/y/w/h，左上原点 + y 向下，与核心坐标系一致）。projection §2.5：Geometry.LayoutRect/
    // WorldRect 返此。internal ctor 让同 assembly（NodeGeometry）FFI 读后构造；公共 ctor 留给业务
    // 通过 Geometry 拿到后再传 API 的场景（暂时未加——frozen 公共 ctor 暂留 internal，需要时升级 public）。
    public readonly struct Rect
    {
        public float X { get; }
        public float Y { get; }
        public float Width { get; }
        public float Height { get; }
        internal Rect(float x, float y, float w, float h)
        {
            X = x; Y = y; Width = w; Height = h;
        }
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

    // ── 异常类型 ──────────────────────────────────────────────────────
    // public-api.md §1.4 失败策略：运行时异常体系。UIContractException = 业务侧违反 API 契约（Get<T>
    // 未命中、Create<T> 非白名单、LoadPackage 同名重复、ListView 静态/数据驱动混用 等；另见 §3.1 Get /
    // §7 ListView / §11.1 Create<T> / §11.2 LoadPackage 各 API 处的抛出语义）。与 ObjectDisposedException
    // （操作已 Dispose 节点）/ InvalidOperationException （内部不变量违例 / FFI 残错）互补：
    // UIContractException 是「调用方写错了」，InvalidOperationException 是「投影层内部状态异常」。
    public class UIContractException : Exception
    {
        public UIContractException(string message) : base(message) { }
        public UIContractException(string message, Exception inner) : base(message, inner) { }
    }
}
