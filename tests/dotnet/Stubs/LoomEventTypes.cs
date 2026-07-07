using System;
using System.Collections.Generic;

namespace LoomGUI
{
    public enum EventType : byte
    {
        Down = 0, Up = 1, Move = 2, Click = 3,
        RollOver = 4, RollOut = 5,
        DragStart = 6, DragMove = 7, DragEnd = 8,
        LongPress = 9,
        KeyDown = 12, KeyUp = 13,
        FocusIn = 14, FocusOut = 15,
        TweenComplete = 16,
    }

    public enum Phase : byte { Capture = 0, Target = 1, Bubble = 2 }

    public delegate void EventCallback(EventContext ctx);

    /// dotnet 测试桩：摘除 Unity [RuntimeInitializeOnLoadMethod] 属性。
    /// 真实定义见 loomgui_unity_package/Runtime/LoomEventHandler.cs:72-103。
    public class EventContext
    {
        public uint target;
        public uint currentTarget;
        public Phase phase;
        public EventType type;
        public int touchId;
        public byte clickCount;
        public uint keyCode;
        public byte modifiers;
        public float x, y;
        public bool isDoubleClick => clickCount > 1;

        internal bool _stopsPropagation, _defaultPrevented, _touchCapture, _stopsImmediatePropagation;
        public void StopPropagation() => _stopsPropagation = true;
        public void PreventDefault() => _defaultPrevented = true;
        public void StopImmediatePropagation() { _stopsImmediatePropagation = true; _stopsPropagation = true; }
        public void CaptureTouch() => _touchCapture = true;

        static readonly Stack<EventContext> _pool = new();
        public static EventContext Get()
        {
            var ctx = _pool.Count > 0 ? _pool.Pop() : new EventContext();
            ctx._stopsPropagation = false; ctx._defaultPrevented = false;
            ctx._touchCapture = false; ctx._stopsImmediatePropagation = false;
            return ctx;
        }
        public static void Return(EventContext ctx) => _pool.Push(ctx);
    }
}
