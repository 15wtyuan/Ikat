using System;
using System.Collections.Generic;
using System.Reflection;
using System.Text;
using Ikat.Bindings;
using Xunit;

namespace Ikat.HeadlessTests
{
    /// <summary>
    /// D1 投影层事件 typed 层验收：
    /// - <see cref="RouteEventCore"/> 持 Target/CurrentTarget + 两个 flag + StopPropagation/PreventDefault。
    /// - 18 个 typed event struct（Public/Ikat.Events.cs）各持 <c>_core</c> 字段、转发 IRouteEvent
    ///   6 成员到 <c>_core</c>、并暴露 <c>internal static byte EventType</c>（D2 EventBus 订阅表 key）。
    ///
    /// 反射遍历 Public/Ikat.Events.cs 的全部 struct（而非逐 struct 写 18 份样板），
    /// 保证 D1 不漏 struct——后续若新增 struct 也自动被这条门覆盖。
    /// 全部纯 managed（D1 不涉 FFI/demux 接线）。
    /// </summary>
    public unsafe class EventRouteCoreTests
    {
        // Public/Ikat.Events.cs 里所有 typed event struct（手工列举以 lock 顺序 + 名字；
        // 反射扫 IRouteEvent 即可自动覆盖，但显式列表让「18 个」契约可读 + 新增 struct 时
        // 测试会显式失败提示维护者更新本清单 + 反射一致断言）。
        static readonly Type[] ExpectedEventStructs = new[]
        {
            typeof(PointerDownEvent), typeof(PointerUpEvent), typeof(PointerMoveEvent),
            typeof(PointerEnterEvent), typeof(PointerLeaveEvent),
            typeof(ClickEvent), typeof(LongPressEvent),
            typeof(DragStartEvent), typeof(DragMoveEvent), typeof(DragEndEvent),
            typeof(KeyDownEvent), typeof(KeyUpEvent),
            typeof(FocusEvent), typeof(BlurEvent),
            typeof(ScrollChangedEvent),
            typeof(AnimationStartEvent), typeof(AnimationEndEvent),
            typeof(AnimationIterationEvent), typeof(TransitionEndEvent),
            // M2 动画句柄私有事件（T11）：OnKey 跨越 / @ikat-hook 跨越。不广播 EventBus——
            // demux 按 playerKey 查 AnimationHandle 实例直接触发回调；struct 作载荷载体。
            typeof(AnimationKeyEvent), typeof(AnimationHookEvent),
            // 控件交互事件（internal route struct，P1 控件束）：携 payload 经 EventBus，控件类
            // 翻译为公共 ValueChangedEvent<*>。这 4 个 internal struct 同样实现 IRouteEvent +
            // 持 _core 首 field + 声明 EventType，故被本门覆盖。
            typeof(ControlValueChangedEvent), typeof(ControlCheckedChangedEvent),
            typeof(ControlChangeCommittedEvent),
            // 单行文本框 Enter 提交（Task 16）。payload 无额外字段——控件类 Submitted 访问器回读 value。
            typeof(ControlSubmittedEvent),
            // Dropdown 选中项变更（Task 14）。payload=新 index（touch_id）——控件类 SelectionChanged
            // 访问器翻译为公共 SelectionChangedEvent。
            typeof(ControlSelectionChangedEvent),
            // Tree branch 条目展开/折叠（#8）。payload=新态（touch_id 1/0）——TreeItem.ExpandedChanged
            // 订阅翻译为公共 ExpandChangedEvent。
            typeof(ControlExpandChangedEvent),
        };

        // 每个结构体映射到期望的 EventType 字节值见 EventTypeCases（xUnit MemberData 须 static，
        // 故对照表以 yield return 形式声明，不另存 dict）。

        // ── RouteEventCore 直接验收 ────────────────────────────────────

        [Fact]
        public void StopPropagationSetsFlag()
        {
            var core = new RouteEventCore();
            Assert.False(core._propagationStopped);
            core.StopPropagation();
            Assert.True(core._propagationStopped);
        }

        [Fact]
        public void PreventDefaultSetsFlag()
        {
            var core = new RouteEventCore();
            Assert.False(core._defaultPrevented);
            core.PreventDefault();
            Assert.True(core._defaultPrevented);
        }

        // ── 转发：struct 的 IRouteEvent 成员读到 _core 字段 ────────────────

        [Fact]
        public void ForwardingReadsCoreTarget()
        {
            // 拿一个真 Node（经 headless harness 走 create_root FFI）验证转发链路：set _core.Target
            // → ClickEvent.Target 读回同一引用。
            var (stage, ctx) = StageHarness.Create();
            try
            {
                Node n = ctx._registry.GetOrCreate(CreateRoot(stage, "div"));
                var e = new ClickEvent { _core = new RouteEventCore { Target = n } };
                Assert.Same(n, e.Target);
            }
            finally { StageHarness.Destroy(stage); }
        }

        [Fact]
        public void ForwardingStopPropagationReachesCore()
        {
            // struct.StopPropagation() 转发到 _core.StopPropagation() → _core._propagationStopped=true
            // → struct.PropagationStopped 读回 true（链路全过 _core）。
            var e = new ClickEvent { _core = new RouteEventCore() };
            Assert.False(e.PropagationStopped);
            e.StopPropagation();
            Assert.True(e.PropagationStopped);
        }

        [Fact]
        public void ForwardingPreventDefaultReachesCore()
        {
            var e = new ClickEvent { _core = new RouteEventCore() };
            Assert.False(e.DefaultPrevented);
            e.PreventDefault();
            Assert.True(e.DefaultPrevented);
        }

        // ── 反射：18 struct 全覆盖门 ─────────────────────────────────────
        //
        // 防止 D1 漏 struct：反射扫 Public/Ikat.Events.cs 全部 struct，断言各 (a) implement
        // IRouteEvent、(b) 有 internal _core 字段、(c) 有 internal static byte EventType 属性。
        // 新增 struct 时自动覆盖（无需改本测试）——除非名字忘了加进 ExpectedEventStructs 清单。

        [Fact]
        public void AllEventStructsImplementIRouteEvent()
        {
            foreach (var t in ExpectedEventStructs)
            {
                Assert.True(typeof(IRouteEvent).IsAssignableFrom(t),
                    $"{t.Name} 必须实现 IRouteEvent（D1 契约）");
            }
        }

        [Fact]
        public void AllEventStructsHaveCoreField()
        {
            foreach (var t in ExpectedEventStructs)
            {
                var f = t.GetField("_core", BindingFlags.Instance | BindingFlags.NonPublic);
                Assert.True(f != null, $"{t.Name} 必须有 internal RouteEventCore _core 字段");
                Assert.Equal(typeof(RouteEventCore), f!.FieldType);
            }
        }

        [Fact]
        public void AllEventStructsDeclareEventType()
        {
            foreach (var t in ExpectedEventStructs)
            {
                var p = t.GetProperty("EventType", BindingFlags.Static | BindingFlags.NonPublic);
                Assert.True(p != null, $"{t.Name} 必须有 internal static byte EventType 属性（D2 订阅表 key）");
                Assert.Equal(typeof(byte), p!.PropertyType);
            }
        }

        // 每个结构体的 EventType 字节值与设计契约一致（防 D1 把 byte 配错）。
        [Theory]
        [MemberData(nameof(EventTypeCases))]
        public void EventTypeMatchesExpected(Type structType, byte expected)
        {
            var p = structType.GetProperty("EventType", BindingFlags.Static | BindingFlags.NonPublic);
            byte actual = (byte)p!.GetValue(null)!;
            Assert.Equal(expected, actual);
        }

        public static IEnumerable<object[]> EventTypeCases()
        {
            // 反射读 private static dict（不值此一处 hardcode），直接借上面字典。
            // xUnit MemberData 须 static——本嵌套类无法访问外层实例字段，故重新声明对照表。
            yield return new object[] { typeof(PointerDownEvent),        (byte)EventType.Down };
            yield return new object[] { typeof(PointerUpEvent),          (byte)EventType.Up };
            yield return new object[] { typeof(PointerMoveEvent),        (byte)EventType.Move };
            yield return new object[] { typeof(PointerEnterEvent),       (byte)EventType.RollOver };
            yield return new object[] { typeof(PointerLeaveEvent),       (byte)EventType.RollOut };
            yield return new object[] { typeof(ClickEvent),              (byte)EventType.Click };
            // 长按（core 按住 ≥1.5s 发 EVT_LONG_PRESS=9，与 Click 独立）。
            yield return new object[] { typeof(LongPressEvent),          (byte)EventType.LongPress };
            yield return new object[] { typeof(DragStartEvent),          (byte)EventType.DragStart };
            yield return new object[] { typeof(DragMoveEvent),           (byte)EventType.DragMove };
            yield return new object[] { typeof(DragEndEvent),            (byte)EventType.DragEnd };
            yield return new object[] { typeof(KeyDownEvent),            (byte)EventType.KeyDown };
            yield return new object[] { typeof(KeyUpEvent),              (byte)EventType.KeyUp };
            yield return new object[] { typeof(FocusEvent),              (byte)EventType.FocusIn };
            yield return new object[] { typeof(BlurEvent),               (byte)EventType.FocusOut };
            yield return new object[] { typeof(ScrollChangedEvent),      (byte)EventType.ScrollChanged };
            yield return new object[] { typeof(AnimationStartEvent),     (byte)EventType.AnimationStart };
            yield return new object[] { typeof(AnimationEndEvent),       (byte)EventType.AnimationEnd };
            yield return new object[] { typeof(AnimationIterationEvent), (byte)EventType.AnimationIteration };
            yield return new object[] { typeof(TransitionEndEvent),      (byte)EventType.TransitionEnd };
            // M2 动画句柄私有事件（T11，core event.rs EVT_ANIMATION_KEY=27 / EVT_ANIMATION_HOOK=28）。
            yield return new object[] { typeof(AnimationKeyEvent),       (byte)EventType.AnimationKey };
            yield return new object[] { typeof(AnimationHookEvent),      (byte)EventType.AnimationHook };
            // 控件交互事件（22+，core EVT_*）。route struct 的 EventType 与 Rust EVT_ 常量一致。
            yield return new object[] { typeof(ControlValueChangedEvent),     (byte)EventType.ValueChanged };
            yield return new object[] { typeof(ControlCheckedChangedEvent),   (byte)EventType.CheckedChanged };
            yield return new object[] { typeof(ControlChangeCommittedEvent),  (byte)EventType.ChangeCommitted };
            // 单行文本框 Enter 提交（Task 16）。payload 无额外字段——控件类 Submitted 访问器回读 value。
            yield return new object[] { typeof(ControlSubmittedEvent),        (byte)EventType.Submitted };
            // Dropdown 选中项变更（Task 14）。payload=新 selected_index（core 装在 touch_id）。
            yield return new object[] { typeof(ControlSelectionChangedEvent), (byte)EventType.SelectionChanged };
        }

        // 反射兜底：扫 Public/Ikat.Events.cs assembly 里所有 IRouteEvent 实现 struct，
        // 断言数量 == ExpectedEventStructs.Length（23）。新增 struct 不更新清单 → 此测失败提醒。
        [Fact]
        public void NoUnexpectedEventStructsAdded()
        {
            var all = new List<Type>();
            foreach (var t in typeof(ClickEvent).Assembly.GetTypes())
            {
                if (t.IsValueType && !t.IsEnum && typeof(IRouteEvent).IsAssignableFrom(t))
                    all.Add(t);
            }
            Assert.Equal(ExpectedEventStructs.Length, all.Count);
        }

        // ── harness 辅助 ────────────────────────────────────────────────

        static ulong CreateRoot(IntPtr stage, string kind)
        {
            StageHandle* h = (StageHandle*)stage.ToPointer();
            byte[] k = Encoding.UTF8.GetBytes(kind ?? "");
            fixed (byte* kp = k)
                return Native.ikat_stage_create_root(h, kp, (nuint)k.Length, null, 0);
        }
    }
}
