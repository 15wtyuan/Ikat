// LoomGUI Frozen Public API: EventType enum
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)
//
// EventType 既被 demux 实现（EventDemuxer.Pump 按 type 分流 raw EventRecord）也被
// typed event struct 关联（Events.cs 每个 struct 的 internal static byte EventType 属性）。
// 放 Public/ 让 Events.cs（编译进 PublicApi 冻结门）能引用——demux 实现文件 EventDemuxer.cs
// 不在 PublicApi 编译链，故 enum 不能只留在那。

namespace LoomGUI
{
    /// <summary>
    /// EventType 与 Rust <c>loomgui_core::input::EVT_*</c> 常量一致（<c>:byte</c> 对齐
    /// <c>event_type:u8</c>）。既驱动 <see cref="EventDemuxer"/> demux 分流（raw EventRecord stream），
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
        // 下列值中 17/21 不出现在 LoomEvent stream（demux 不产 typed struct）——仅供 typed
        // event struct 的 internal static byte EventType 属性返回，作 D2 EventBus 订阅表的 key。
        // D3 接线时为其接源（ScrollChanged←ScrollPane 物理；TransitionEnd←TweenComplete）。
        // 18/19/20 是 M2 真 core 事件源（crates/core/src/event.rs EVT_ANIMATION_*，T9）——
        // demux 直接读 stream 产 typed struct（class 触发 + node.Play 都发）。
        ScrollChanged = 17,
        AnimationStart = 18,
        AnimationIteration = 19,
        AnimationEnd = 20,
        TransitionEnd = 21,
        // ── 控件交互事件（22+，对齐 core EVT_VALUE_CHANGED / EVT_CHECKED_CHANGED /
        // EVT_CHANGE_COMMITTED，input.rs:83-85）──────────────────────────────
        // 这些走 LoomEvent stream（demux 产）：Slider 拖拽逐值 / Toggle·Radio 翻转 / Slider 松手提交。
        // payload 复用 EventRecord 现有字段（不扩 struct）：VALUE_CHANGED·CHANGE_COMMITTED 用 x（float），
        // CHECKED_CHANGED 用 pad[0]（0/1）。
        ValueChanged = 22,
        CheckedChanged = 23,
        ChangeCommitted = 24,
        /// 单行文本框（TextField）Enter=提交（core EVT_SUBMITTED）。
        /// TextArea 不发此事件（Enter 插换行）。payload 无额外字段（node_id 指向提交控件）。
        Submitted = 25,
        /// Dropdown 选中项变更（core EVT_SELECTION_CHANGED = 26，input.rs:114）。
        /// payload = 新 selected_index（装 EventRecord.touch_id:i32，见 control.rs:422——commit_dropdown_selection
        /// 把新 index 写 touch_id，与 Slider 装新值到 x 不同）。core 不携 OldIndex（同 change 语义只报新值）。
        SelectionChanged = 26,
        // ── M2 动画句柄私有事件（core event.rs EVT_ANIMATION_KEY=27 / EVT_ANIMATION_HOOK=28）──
        // 走 playerKey 句柄路由（demux 按 key 查 Animation 实例触发 OnKey/OnHook，spec §7.5），
        // 不广播 EventBus——枚举值仅供 typed struct 关联 + 文档完整（不参与 D2 订阅表）。
        AnimationKey = 27,
        AnimationHook = 28,
    }
}
