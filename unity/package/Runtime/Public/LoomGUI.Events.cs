// LoomGUI Frozen Public API: Events
// See docs/design/public-api.md (权威契约) + docs/design/projection-layer.md (投影层机制)

using System;

#pragma warning disable CS0169, CS0067, CS0649

namespace LoomGUI
{
    public interface IRouteEvent
    {
        Node Target { get; }
        Node CurrentTarget { get; }
        bool DefaultPrevented { get; }
        bool PropagationStopped { get; }
        void StopPropagation();
        void PreventDefault();
    }

    public sealed class EventRegistration : IDisposable
    {
        public void Dispose() { throw NE(); }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Pointer
    public struct PointerDownEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        public PointerButton Button { get { throw NE(); } }
        public int TouchId { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerUpEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        public PointerButton Button { get { throw NE(); } }
        public int TouchId { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerMoveEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        public float DeltaX { get { throw NE(); } }
        public float DeltaY { get { throw NE(); } }
        public int TouchId { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerEnterEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct PointerLeaveEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct ClickEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        public int ClickCount { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Drag
    public struct DragStartEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        public Vector2 StartPosition { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct DragMoveEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        public float DeltaX { get { throw NE(); } }
        public float DeltaY { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct DragEndEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Vector2 Position { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Keyboard
    public struct KeyDownEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public KeyCode Key { get { throw NE(); } }
        public KeyModifiers Modifiers { get { throw NE(); } }
        public bool Repeat { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct KeyUpEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public KeyCode Key { get { throw NE(); } }
        public KeyModifiers Modifiers { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Focus
    public struct FocusEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Node PreviousFocused { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct BlurEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public Node NewFocused { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Scroll
    public struct ScrollChangedEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public float ScrollX { get { throw NE(); } }
        public float ScrollY { get { throw NE(); } }
        public float DeltaX { get { throw NE(); } }
        public float DeltaY { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    // Animation lifecycle
    public struct AnimationStartEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public string AnimationName { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct AnimationEndEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public string AnimationName { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct AnimationIterationEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public string AnimationName { get { throw NE(); } }
        public int IterationCount { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }

    public struct TransitionEndEvent : IRouteEvent
    {
        public Node Target { get { throw NE(); } }
        public Node CurrentTarget { get { throw NE(); } }
        public bool DefaultPrevented { get { throw NE(); } }
        public bool PropagationStopped { get { throw NE(); } }
        public void StopPropagation() { throw NE(); }
        public void PreventDefault() { throw NE(); }
        public string PropertyName { get { throw NE(); } }
        static NotImplementedException NE() => new NotImplementedException();
    }
}
