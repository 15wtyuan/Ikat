// Ikat Frozen Public API: Value types & enums

using System;

#pragma warning disable CS0169, CS0067, CS0649

namespace Ikat
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
        public Thickness(float left, float top, float right, float bottom)
        {
            Left = left; Top = top; Right = right; Bottom = bottom;
        }
    }

    public readonly struct IkatColor
    {
        public float R { get; }
        public float G { get; }
        public float B { get; }
        public float A { get; }
        public bool IsUnset { get; }   // true = 未被 typed 层覆盖（Unset 哨兵），getter 据此返回

        // 公共 ctor 强制 IsUnset=false（用户态颜色必然是已设置）；IsUnset=true 仅由 Unset factory 获得，
        // 故另设 private 5-参 ctor 让 Unset 走特化路径而不破公共 ctor 签名。
        public IkatColor(float r, float g, float b, float a = 1f) : this(r, g, b, a, isUnset: false) { }
        private IkatColor(float r, float g, float b, float a, bool isUnset)
        {
            R = r; G = g; B = b; A = a; IsUnset = isUnset;
        }
        public static IkatColor Unset => new IkatColor(0f, 0f, 0f, 0f, isUnset: true);
    }

    // 2D 向量（Position / Scale / Origin / 滚动点等）。值语义：等号按字段比较（struct 默认）。
    // 业务侧通过 new IkatVector2(x,y) 构造；Zero/One 是常用常量。投影层（NodeTransform）镜像
    // default 与业务语义对齐：Position/Origin 默认 Zero（不位移）、Scale 默认 One（不缩放）。
    public readonly struct IkatVector2
    {
        public float X { get; }
        public float Y { get; }
        public IkatVector2(float x, float y) { X = x; Y = y; }
        public static IkatVector2 Zero => default;   // (0,0)；default(IkatVector2) 直接给零值，免 alloc
        public static IkatVector2 One => new IkatVector2(1f, 1f);   // 不缩放 / 不位移语义哨兵
    }

    // 矩形（x/y/w/h，左上原点 + y 向下，与核心坐标系一致）。Geometry.LayoutRect/
    // WorldRect 返此。internal ctor 让同 assembly（NodeGeometry）FFI 读后构造；公共 ctor 留给业务
    // 通过 Geometry 拿到后再传 API 的场景（暂时未加——frozen 公共 ctor 暂留 internal，需要时升级 public）。
    public readonly struct IkatRect
    {
        public float X { get; }
        public float Y { get; }
        public float Width { get; }
        public float Height { get; }
        internal IkatRect(float x, float y, float w, float h)
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
    public enum ScrollBehavior { Instant, Smooth }   // 方法参数，非 Style 属性，无需 Unset
    // text-align 的 computed 读值（Style 写层无此 prop——围栏子集 text-align 走 CSS 声明）。
    public enum TextAlign { Left, Center, Right }

    // —— tween builder 面（#9 契约；判别值与 core TweenProp / ease_ffi 对齐，勿重排）——

    /// <summary>tween 动画通道（core TweenProp 镜像）。Transform = TRS 五元组
    /// [tx,ty,sx,sy,rot(rad)]；Translate/Scale 二元；Opacity/Rotation/FlexGrow 一元；
    /// 颜色四元 RGBA。Width/Height 二元 = [value, domainCode]（LenDomain 镜像）——
    /// 双端必须同域（core FFI 校验拒收）。BoxShadow 走 FromShadow/ToShadow 列表载荷，
    /// 不用 From/To 数组。</summary>
    public enum TweenChannel
    {
        Opacity = 0, Translate = 1, Scale = 2, Rotation = 3,
        BgColor = 4, TextColor = 5, Transform = 6,
        Width = 7, Height = 8, FlexGrow = 9, BoxShadow = 10,
    }

    /// <summary>layout 动画长度域（core LenDomain 镜像；Width/Height tween 载荷第 2 槽）。
    /// px↔px / %↔% / vw↔vw 各自动（端点同域），vw/vh/vmin/vmax 按画布尺寸解析。</summary>
    public enum LenDomain
    {
        Px = 0, Pct = 1, Vw = 2, Vh = 3, Vmin = 4, Vmax = 5,
    }

    /// <summary>box-shadow 单层（tween 载荷形态，CSS box-shadow 层镜像）。颜色是
    /// linear f32 RGBA（Public 层不引 UnityEngine）。多层动画 = params 数组；列表长度
    /// 不匹配时 core 按浏览器语义补透明零长阴影逐对插值，配对 inset 不匹配整体离散。</summary>
    public struct TweenShadow
    {
        public float OffsetX, OffsetY, Spread, Blur;
        public float R, G, B, A;
        /// <summary>true = inset（画在节点内）。双端同序号层 inset 不一致 → 整体离散跳变。</summary>
        public bool Inset;

        public static TweenShadow Outer(float ox, float oy, float blur, float spread, float r, float g, float b, float a)
            => new TweenShadow { OffsetX = ox, OffsetY = oy, Blur = blur, Spread = spread, R = r, G = g, B = b, A = a, Inset = false };
        public static TweenShadow InsetShadow(float ox, float oy, float blur, float spread, float r, float g, float b, float a)
            => new TweenShadow { OffsetX = ox, OffsetY = oy, Blur = blur, Spread = spread, R = r, G = g, B = b, A = a, Inset = true };
    }

    /// <summary>缓动 kind（core ease_ffi 镜像；与 CSS keyword 对应关系见 fence.md「缓动函数
    /// 全集」）。CSS 标准 keyword（ease/ease-in/...）是 bezier 曲线——运行时侧想精确复刻
    /// 用 <c>EaseBezier(0.25,0.1,0.25,1)</c> 等；keyword 形与 CSS 值的映射只在 DSL 侧。</summary>
    public enum EaseKind
    {
        Linear = 0, QuadIn = 1, QuadOut = 2, QuadInOut = 3,
        CubicIn = 4, CubicOut = 5, CubicInOut = 6,
        BackIn = 7, BackOut = 8, BackInOut = 9,
        StepEnd = 10, StepStart = 11,
        CubicBezier = 12,   // 参数走 ease_params
        ElasticIn = 13, ElasticOut = 14, ElasticInOut = 15,
        BounceIn = 16, BounceOut = 17, BounceInOut = 18,
    }

    // 指针键：对齐 web MouseEvent.button（0=左/1=中/2=右）但用枚举自解释。
    public enum PointerButton { Left, Middle, Right }

    public enum IkatKeyCode
    {
        // Values match Unity IkatKeyCode enum（raw u32 透传直接 cast；core 不解释语义）。
        None = 0,
        Enter = 13,
        Escape = 27,
        Tab = 9,
        Space = 32,
        Backspace = 8,
        Delete = 127,
        Left = 276,     // LeftArrow
        Right = 275,    // RightArrow
        Up = 273,       // UpArrow
        Down = 274,     // DownArrow
        A = 97, B = 98, C = 99, D = 100, E = 101, F = 102, G = 103,
        H = 104, I = 105, J = 106, K = 107, L = 108, M = 109,
        N = 110, O = 111, P = 112, Q = 113, R = 114, S = 115,
        T = 116, U = 117, V = 118, W = 119, X = 120, Y = 121, Z = 122,
        D0 = 48, D1 = 49, D2 = 50, D3 = 51, D4 = 52,
        D5 = 53, D6 = 54, D7 = 55, D8 = 56, D9 = 57,
        F1 = 282, F2 = 283, F3 = 284, F4 = 285, F5 = 286, F6 = 287,
        F7 = 288, F8 = 289, F9 = 290, F10 = 291, F11 = 292, F12 = 293,
    }

    [Flags]
    public enum KeyModifiers { None = 0, Shift = 1, Control = 2, Alt = 4 }

    public struct ValueChangedEvent<T>
    {
        // 投影层填（EventDemuxer 造本 struct 时赋）。core 控件事件 stream 只携新值（EVT_VALUE_CHANGED
        // x=新 float / EVT_CHECKED_CHANGED pad[0]=bool），无 OldValue——故 _oldValue 留 default(T)。
        // 公共 OldValue/NewValue 签名冻结；此处仅补 backing 字段把 throw 占位转实读（同控件属性壳填实）。
        internal T _oldValue;
        internal T _newValue;
        public T OldValue { get { return _oldValue; } }
        public T NewValue { get { return _newValue; } }
    }

    public struct SelectionChangedEvent
    {
        // 投影层填（Dropdown.SelectionChanged backing-dict 翻译时赋 _newIndex）。core 控件事件 stream
        // 只携新 index（EVT_SELECTION_CHANGED touch_id=新 selected_index），无 OldIndex——故 _oldIndex
        // 留 sentinel -1（表示「core 未携旧值」；0 是合法 index 故不用 default(0)）。
        // OldValue/NewValue 暂无数据源（core 无 per-option value getter FFI——option value 在打包期进
        // Dropdown.options side table，运行时未暴露）——保留 throw，待 option-value FFI 补后填。
        internal int _oldIndex;
        internal int _newIndex;
        public int OldIndex { get { return _oldIndex; } }
        public int NewIndex { get { return _newIndex; } }
        // core 事件流只携新 index（EVT_SELECTION_CHANGED touch_id），无旧值——OldValue 留
        // null（同 ValueChangedEvent.OldValue=default(T) 家族语义）。NewValue 由 Dropdown 派发
        // 时经 get_dropdown_selected_value 实取（事件泵出前 core 已应用新 index）；TabList 的
        // tab 无 value 语义 → null（值面归 NewIndex）。
        internal string _oldValue;
        internal string _newValue;
        public string OldValue { get { return _oldValue; } }
        public string NewValue { get { return _newValue; } }
    }

    /// <summary>
    /// 文本选区（字节偏移）：[<see cref="Start"/>, <see cref="End"/>）半开区间。
    /// 投影层填实为纯值类型（Start/End 自动属性 + ctor）——选区语义在 core（EditState.anchor/cursor），
    /// C# struct 仅作调用方传参载体，FFI set_selection/get_selection 在 TextField.Selection 访问器里转。
    /// Start≤End（get_selection 归一后）；退化选区 Start==End（零宽光标）。
    /// </summary>
    public struct TextSelection
    {
        /// <summary>选区起点（min(anchor,cursor)，字节偏移）。</summary>
        public int Start { get; set; }
        /// <summary>选区终点（max(anchor,cursor)，字节偏移；==Start 时零宽光标）。</summary>
        public int End { get; set; }

        public TextSelection(int start, int end)
        {
            Start = start;
            End = end;
        }
    }

    // 失败策略：运行时异常体系。UIContractException = 业务侧违反 API 契约（Get<T>
    // 未命中、Create<T> 非白名单、LoadPackage 同名重复、ListView 静态/数据驱动混用 等）。
    // 与 ObjectDisposedException
    // （操作已 Dispose 节点）/ InvalidOperationException （内部不变量违例 / FFI 残错）互补：
    // UIContractException 是「调用方写错了」，InvalidOperationException 是「投影层内部状态异常」。
    public class UIContractException : Exception
    {
        public UIContractException(string message) : base(message) { }
        public UIContractException(string message, Exception inner) : base(message, inner) { }
    }

    // UIPackageException = 包操作失败（LoadPackage 内部异常：pkg.bin 格式错 / 重复 pkg id /
    // 资源缺失 等）。与 UIContractException 互补：UIContractException 是调用方写错了
    // （Create<T> 非白名单 / 同名重复 load），UIPackageException 是包内部错了。
    public class UIPackageException : Exception
    {
        public UIPackageException(string message) : base(message) { }
        public UIPackageException(string message, Exception inner) : base(message, inner) { }
    }

    // UIStyleException = 运行时 CSS 解析失败（SetInlineStyle / 动态规则注入等
    // 运行时改 CSS 的路径，值非法或语法错时抛）。与 UIContractException（调用方写错 API 契约）/
    // UIPackageException（包内部错）互补：UIStyleException 专指 CSS 值/规则解析失败。
    // 构造签名对齐 UIContractException（message + message/inner 双参），补无参默认 ctor 兼容
    // default-activation 抛出场景（异常体系共四种，本类补齐 frozen 异常体系）。
    public class UIStyleException : Exception
    {
        public UIStyleException() { }
        public UIStyleException(string message) : base(message) { }
        public UIStyleException(string message, Exception inner) : base(message, inner) { }
    }
}
