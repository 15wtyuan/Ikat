# Task D3 Report: demux wiring + semantic sugar (Spec-4a 阶段 D 收尾)

## Status: DONE

## What was done

### 1. EventDemuxer (Projection/EventDemuxer.cs)

Created `EventDemuxer` internal class that translates raw `borrow_events` buffer (Rust `EventRecord[]` → C# `RawEventRecord`) to typed event structs and dispatches through `EventBus.Dispatch<T>`.

**Key design decisions:**
- **RawEventRecord struct**: Self-contained 20-byte `[StructLayout(LayoutKind.Sequential)]` mirror of Rust `EventRecord`. Defined inside `EventDemuxer.cs` to avoid dependency on `LoomEventHandler.cs`'s `LoomEvent` struct (not in headless test compilation chain).
- **Reuse, not rewrite**: Old `LoomEventHandler.DispatchPending` continues to run in parallel (backward compat for old `AddListener`-based subscribers). New `EventDemuxer.Pump` runs on the same buffer — independent subscriber pools, no double-fire.
- **Translation**: `_core.Target = _ctx._registry.GetOrCreate(nodeId)` — NodeFactory FFI ensures typed Node wrapper exists.
- **Event type mapping** (LoomEvent type byte → typed event struct):

| byte | EventType | Typed struct |
|------|-----------|-------------|
| 0 | Down | PointerDownEvent |
| 1 | Up | PointerUpEvent |
| 2 | Move | PointerMoveEvent |
| 3 | Click | ClickEvent |
| 4 | RollOver | PointerEnterEvent |
| 5 | RollOut | PointerLeaveEvent |
| 6 | DragStart | DragStartEvent |
| 7 | DragMove | DragMoveEvent |
| 8 | DragEnd | DragEndEvent |
| 12 | KeyDown | KeyDownEvent |
| 13 | KeyUp | KeyUpEvent |
| 14 | FocusIn | FocusEvent |
| 15 | FocusOut | BlurEvent |
| 16 | TweenComplete | AnimationEndEvent + TransitionEndEvent (both) |

### 2. UIContext + LoomStage wiring

- Added `internal readonly EventDemuxer _eventDemuxer` to `UIContext` (constructed with `EventDemuxer(this)`).
- Added `internal void SetEventDemuxer(EventDemuxer)` to `LoomStage`.
- `LoomStage.Tick` now calls `_eventDemuxer?.Pump(...)` alongside existing `_eventHandler.DispatchPending(...)` — null-safe: headless tests without SetEventDemuxer still work.

### 3. Semantic sugar (Button.Clicked + Link.Activated)

**Button.Clicked** (`event Action`):
```
add => On<ClickEvent>(e => value(), useCapture: false)
remove => reg.Dispose() via Dictionary<Action, EventRegistration> backing
```
- `action` wrapper converts typed `ClickEvent` → parameterless `Action` call (semantic sugar: "bubble to self").
- EventRegistration stored in `_clickedBacking` dictionary keyed by Action handler for remove.

**Link.Activated** (`event Action`):
- Same pattern as Button.Clicked, backed by `_activatedBacking` dictionary.

### 4. Container.Scrolled — deferred

`public event Action<ScrollChangedEvent> Scrolled;` remains a field-like event. Comment added: "ScrollChanged source 待补" — ScrollPane physics self-maintains tween, no `borrow_scroll_events` FFI.

### 5. Five source-less structs resolution

| EventType | Value | Source status | Action |
|-----------|-------|--------------|--------|
| AnimationEnd | 20 | **WIRED** from TweenComplete (type 16) | core TweenComplete `click_count = TweenProp`, `touch_id = tag` |
| TransitionEnd | 21 | **WIRED** from TweenComplete (type 16) | same TweenComplete event produces both events |
| ScrollChanged | 17 | **deferred** | no `borrow_scroll_events` FFI; ScrollPane physics self-managed |
| AnimationStart | 18 | **deferred** | tween start has no corresponding EventRecord |
| AnimationIteration | 19 | **deferred** | tween loop has no corresponding EventRecord |

Deferred types have commented rationale in EventDemuxer.Pump's default arm.

### 6. Test evidence (TDD)

11 new tests in `EventDemuxerTests.cs`, all green:
1. `DemuxTranslatesAndDispatches` — ClickEvent Target = registry.GetOrCreate
2. `DemuxTranslatesPointerDownAsDistinctType` — Down → PointerDownEvent mapping
3. `TweenCompleteProducesAnimationEndAndTransitionEnd` — TweenComplete → both events
4. `DemuxDispatchesMultipleEvents` — 3 events → 3 handler calls
5. `ClickedSugarFires` — Button.Clicked += h → h fires
6. `ClickedRemoveUnsubscribes` — += then -= → handler not called
7. `ActivatedSugarFires` — Link.Activated mechanism (On<ClickEvent> bubble to self)
8. `CaptureBubbleOrderViaDemux` — 3-tier tree capture/bubble order
9. `StopPropagationViaDemux` — child StopPropagation → root bubble not fired
10. `DisposeDuringDemuxSkipsHandler` — D2 IsDisposed fix path
11. `PumpWithEmptyBufferIsNoOp` — null/0 buffer safe

Total test count: **260 passed** (D2 249 + D3 11), 1 pre-existing skip.

`NativeEventBuffer` helper: writes raw 20-byte `EventRecord` entries into unmanaged memory via `AllocHGlobal` — simulates `borrow_events` return, feeds `EventDemuxer.Pump`.

## Files changed

| File | Change |
|------|--------|
| `unity/package/Runtime/Projection/EventDemuxer.cs` | **Created** — `EventDemuxer` class + `RawEventRecord` struct |
| `unity/package/Runtime/Public/LoomGUI.Nodes.cs` | `UIContext._eventDemuxer` field + ctor init; `Button.Clicked` event add/remove body; `Link.Activated` event add/remove body; `Container.Scrolled` deferral comment |
| `unity/package/Runtime/LoomStage.cs` | `_eventDemuxer` field + `SetEventDemuxer()` + Tick calls `_eventDemuxer?.Pump()` |
| `tests/dotnet/LoomGUI.HeadlessTests/EventDemuxerTests.cs` | **Created** — 11 tests + `NativeEventBuffer` helper |

## PublicApi build

`dotnet build tests/dotnet/LoomGUI.PublicApi` — **passes** (0 errors, 0 warnings). Event signatures (`Button.Clicked`, `Link.Activated`, `Container.Scrolled`) unchanged — only add/remove bodies changed.

## Self-Review

**Completeness**: Demux wired for all 13 core EventRecord types (0-16) → typed struct dispatch. TweenComplete splits to AnimationEnd + TransitionEnd. 3 source-less types deferred with clear rationale.

**Quality**: `RawEventRecord` self-contained avoids headless test compilation dependency on `LoomEventHandler.cs`. Demux uses same buffer as old LoomEventHandler (no double borrow). `Dictionary<Action, EventRegistration>` backing for semantic sugar remove — clean event add/remove semantics matching C# convention.

**Discipline**: No new branch; working on `spec4a-projection-layer`. Frozen signatures intact (PublicApi build green). Comments on deferred items are self-contained with WHY.

**Testing**: 11 tests covering demux translation, type mapping, TweenComplete split, Clicked sugar add/remove, capture/bubble order, StopPropagation, Dispose-during-demux. Plus full suite regression (D2 249 green).

## Concerns

1. **ScrollChanged (17) deferred**: Source genuinely missing — ScrollPane physics runs in core without event emission. Needs new FFI or C# callback hook.
2. **AnimationStart/AnimationIteration (18-19) deferred**: Tween start/loop without FFI events. Requires core-side tween lifecycle event emission.
3. **Business field filling**: Typed event struct properties (Position, Button, Key, etc.) still throw `NotImplementedException`. D3 focuses on demux wiring + routing; field filling is a subsequent task.
4. **Headless test limitation for Link.Activated**: "link" kind may not be a valid `create_root` kind in core's dynamic tree API (fence constraint). Test validates mechanism via `Container.On<ClickEvent>` which is equivalent to Link.Activated's body.

## Fix Report — reviewer 2 Important + 2 Minor

### Finding 1 (Important): shared RouteEventCore between AnimationEnd + TransitionEnd

**Fix**: `EventDemuxer.cs` TweenComplete case now creates two independent `NewCore()` calls — one per DispatchTyped, instead of a single shared `var core = NewCore(nodeId)`. Each event struct gets its own `RouteEventCore`, so `StopPropagation()` on AnimationEnd no longer leaks to TransitionEnd's bubble phase.

**Test**: Added `TweenCompleteProducesIndependentCores` — child handler calls `StopPropagation()` on `AnimationEndEvent`, then verifies `TransitionEndEvent` still bubbles to root. Passes.

### Finding 2 (Important): business fields not filled from available raw data

**Fix**:
- `LoomGUI.Events.cs`: All 18 typed event structs now have `internal` backing fields beside `_core` for every business property (`_position`, `_button`, `_clickCount`, `_touchId`, `_key`, `_modifiers`, etc.). Property getters changed from `throw NE()` to read the backing field.
- `EventDemuxer.cs` Pump: Each event dispatch fills `_position` (from x,y), `_touchId` (from touchId), `_clickCount` (from clickCount), `_key` (from touchId cast to KeyCode), `_modifiers` (from pad[0] cast to KeyModifiers). Fields without raw-source data (Button, DeltaX/Y, StartPosition, Repeat, PreviousFocused/NewFocused, Scroll*, AnimationName/PropertyName/IterationCount) stay at default — filled later when source arrives.
- `LoomGUI.Types.cs` KeyCode enum: Values changed from sequential (0..N) to Unity KeyCode values (Enter=13, A=97, LeftArrow=276, etc.) so the direct `(KeyCode)evt.touchId` cast works. Same member names kept — only underlying values changed to match the Unity KeyCode that core passes through.

**Tests**: Added 4 regression tests:
- `PointerDownPositionIsFilled` — verifies Position.X/Y and TouchId read after Pump (not NE)
- `ClickPositionAndCountAreFilled` — verifies Position and ClickCount from raw
- `KeyDownKeyAndModifiersAreFilled` — KeyCode.A + Shift modifier
- `KeyUpKeyAndModifiersFromRaw` — KeyCode.Enter + Ctrl|Alt combo

### Finding 3 (Minor): test hardcoded RecSize=20

**Fix**: `NativeEventBuffer.RecSize` changed from `const int RecSize = 20` to `static readonly int RecSize = Marshal.SizeOf<RawEventRecord>()`.

### Finding 4 (Minor): parallel dispatch paths no deprecation comment

**Fix**: Added deprecation comment above `_eventHandler.DispatchPending` / `_eventDemuxer?.Pump` in `LoomStage.Tick`: "DEPRECATION: 待所有 callers 从 AddListener 迁移到 On<T> 后移除 _eventHandler.DispatchPending（后续 cleanup）".

### Test summary

D3 original 11 tests + 5 new regression tests = 16 EventDemuxer tests. Full suite: **265 passed, 0 failed, 1 skipped** (pre-existing). `dotnet build tests/dotnet/LoomGUI.PublicApi` clean.
