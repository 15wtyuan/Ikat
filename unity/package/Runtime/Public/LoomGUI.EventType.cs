// LoomGUI Frozen Public API: EventType enum
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)
//
// EventType 既被 demux 实现（LoomEventHandler.DispatchPending 按 type 分流 LoomEvent）也被
// typed event struct 关联（Events.cs 每个 struct 的 internal static byte EventType 属性）。
// 放 Public/ 让 Events.cs（编译进 PublicApi 冻结门）能引用——demux 实现文件 LoomEventHandler.cs
// 不在 PublicApi 编译链，故 enum 不能只留在那。

namespace LoomGUI
{
    /// <summary>
    /// EventType 与 Rust <c>loomgui_core::input::EVT_*</c> 常量一致（<c>:byte</c> 对齐
    /// <c>event_type:u8</c>）。既驱动 <see cref="LoomEventHandler"/> demux 分流（LoomEvent stream），
    /// 也作 typed event struct 的 D2 EventBus 订阅表 key（<c>Events.cs</c> 每 struct 的
    /// <c>internal static byte EventType</c> 属性）。
    /// </summary>
    public enum EventType : byte
    {
        Down = 0,
        Up = 1,
        Move = 2,
        Click = 3,
        RollOver = 4,
        RollOut = 5,
        // drag（opt-in draggable，core 检测）+ longpress（universal）。
        DragStart = 6,
        DragMove = 7,
        DragEnd = 8,
        LongPress = 9,
        // 键盘 + 焦点（core 检测，C# 路由）。
        KeyDown = 12,
        KeyUp = 13,
        FocusIn = 14,
        FocusOut = 15,
        // tween 完成（core 产，C# 直派）。click_count 复用装 prop、touch_id 复用装 tag。
        TweenComplete = 16,
        // ── typed event struct 关联（D1，spec §3.4）──────────────────────────
        // 下列值不出现在 LoomEvent stream（demux 不产）——仅供 typed event struct 的
        // internal static byte EventType 属性返回，作 D2 EventBus 订阅表的 key。
        // D3 接线时为这些类型分别接源（ScrollChanged←ScrollPane 物理；
        // AnimationStart/Iteration←tween 回调；AnimationEnd/TransitionEnd←TweenComplete
        // 按 prop 名分流）。
        ScrollChanged = 17,
        AnimationStart = 18,
        AnimationIteration = 19,
        AnimationEnd = 20,
        TransitionEnd = 21,
    }
}
