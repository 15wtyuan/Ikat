# LoomGUI Runtime API Reference

Complete lookup table for the business-programmer API (nodes, controls,
events, lists, animation, styling). The authoritative source is the
shipped C# signatures (`LoomGUI.*.cs` in the Unity package) — this file
mirrors their contract so you never need the LoomGUI repository. Load
this on demand; the `loomgui-runtime` SKILL.md is the workflow manual.

## Object hierarchy

```text
Node
+-- Container (subtree = user content, arrangerble at runtime)
|   +-- AbsolutePanel (sugar: children auto position:absolute)
|   +-- TextElement (span)
|   +-- Button (button)
|   +-- ListView (role=list) / ListItem (role=listitem)
|   +-- OptionItem (role=option, owned by Dropdown)
|   +-- Slot / CustomElement
+-- TextNode / Image                        (leaf: content / drawing)
+-- Controls (leaf: subtree = control internals, not exposed)
    +-- TextField (role=textbox) / TextArea (role=textbox + aria-multiline)
    +-- NumberField (role=spinbutton)
    +-- Slider (role=slider) / ProgressBar (role=progressbar)
    +-- Toggle (role=switch) / RadioButton (role=radio)
    +-- Dropdown (role=combobox)
```

Container vs leaf is decided by **subtree ownership**, not tag: a
Container's children are user content (authored in HTML, editable at
runtime); a control leaf's children are control internals (thumb, fill)
managed by the framework. Node types are decided by the stable HTML
semantic signature (tag, or ARIA `role` + `aria-*`) — CSS never changes
the C# type.

## Node base

```csharp
public abstract class Node {
    public UIContext Context { get; }
    public string Id { get; }
    public Container Parent { get; }            // Root.Parent == null

    public NodeStyle Style { get; }             // writable, inline override layer
    public NodeTransform Transform { get; }     // writable, render layer
    public NodeGeometry Geometry { get; }       // read-only, layout product

    public bool Touchable { get; set; }
    public bool Focusable { get; set; }
    public ClassList Classes { get; }

    public bool IsDisposed { get; }
    public void RemoveFromParent();             // re-attachable, keeps subscriptions
    public void Dispose();                      // recursive destroy, clears subscriptions

    public T Get<T>(string id) where T : Node;  // scoped, throws UIContractException
    public bool TryGet<T>(string id, out T node) where T : Node;
    public IReadOnlyList<T> Query<T>() where T : Node;      // by type, document order
    public IReadOnlyList<Node> Query(string selector);      // ".class" / "tag.class"

    public AnimationHandle Play(string name);
    public AnimationHandle Play(string name, float durationSeconds); // override the 1s default
    public void Focus();
    public void Blur();

    public IDisposable OnUpdate(Action<float> cb);          // per-frame logic hook
    public EventRegistration On<T>(Action<T> handler, bool useCapture = false, bool once = false) where T : IRouteEvent;
}
```

Lookup invariants:

- `Get<T>`/`Query` are scoped to the current component instance — they
  do not cross nested component or list-item boundaries. To reach
  inside a nested scope: `Get` the scope root first, then `Get` from
  it. The scope root itself IS visible from outside (Shadow-DOM style).
- `Query` results are stable document order (pre-order traversal) —
  safe for aggregation patterns like a hand-rolled RadioGroup.
- Inside a virtualized `<ul>` do NOT use `:nth-child` selectors
  (parked slots count as children); key off item index / `data-*`
  attributes instead.
- Operations on a disposed node throw `ObjectDisposedException`.
- `OnUpdate`/`On<T>` subscriptions are auto-cleaned by `Dispose`;
  `RemoveFromParent` keeps them.

## Style / Transform / Geometry (three-part model)

- **Style** — writable inline-override layer (highest priority over
  the CSS cascade). Getters return only what the C# setter wrote;
  unset properties return an `Unset` sentinel. Assign `Unset` to drop
  the override. `Style.SetVar(name, value)` / `RemoveVar(name)` manage
  CSS custom properties `--*` (they cross scope roots; there is no
  GetVar).
- **Transform** — visual offset/scale/rotation/origin. Changing it does
  NOT trigger layout (no solve); it refreshes hit geometry and world
  matrices only. Final render position = layout position +
  Transform.Position.
- **Geometry** — read-only layout product (`LayoutRect` in parent
  space, `WorldRect`, `LocalToGlobal`/`GlobalToLocal`). Reflects the
  last completed solve — reads lag one frame behind Style writes (like
  web reflow). No forced synchronous solve exists.

Move things at runtime via `Transform`; size/position via `Style`;
read actual position via `Geometry`.

```csharp
public sealed class NodeStyle {
    public Length Width/Height/MinWidth/MaxWidth/MinHeight/MaxHeight { get; set; }
    public DisplayMode Display { get; set; }             // Block | Flex | None
    public FlexDirection FlexDirection { get; set; }
    public FlexWrap/JustifyContent/AlignItems { get; set; }
    public Length Gap { get; set; }
    public Thickness Padding/Margin/BorderWidth { get; set; }
    public Overflow OverflowX/OverflowY { get; set; }
    public Length Left/Top/Right/Bottom { get; set; }
    public PositionMode Position { get; set; }
    public int ZIndex { get; set; }                      // sibling stacking, paint+hit only
    public LoomColor BackgroundColor/TextColor { get; set; }   // text color = CSS color channel
    public float Opacity { get; set; }
    public void SetVar(string name, Length/LoomColor/float/string value);
    public void RemoveVar(string name);
}
public sealed class NodeTransform {
    public LoomVector2 Position { get; set; }
    public LoomVector2 Scale { get; set; }
    public float Rotation { get; set; }                  // radians
    public LoomVector2 Origin { get; set; }
}
```

## Container and tree operations

```csharp
public class Container : Node {
    public int ChildCount { get; }
    public IReadOnlyList<Node> Children { get; }
    public string TextContent { get; set; }              // DOM semantics, see below
    public T AddChild<T>(T child) where T : Node;
    public T InsertChild<T>(T child, int index) where T : Node;
    public void RemoveChild(Node child);
    public Node GetChildAt(int index);
    public int GetChildIndex(Node child);
    public void SetChildIndex(Node child, int index);
    public void SwapChildren(Node a, Node b);
    public void SwapChildrenAt(int indexA, int indexB);
    public LoomVector2 ScrollPos { get; }                    // (0,0) on non-scrolling
    public void RestartAnimations();                     // rebuild declarative players on this node AND its subtree; programmatic (Play) players untouched; node state kept
    public void ScrollTo(LoomVector2 pos, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public event Action<ScrollChangedEvent> Scrolled;
    public UITemplate GetTemplate(string name);
}
```

- **TextContent** follows DOM `textContent`: reading concatenates
  descendant text; **writing clears ALL children** and replaces them
  with a single text node. If the container mixes text and element
  children (e.g. a `<button>` with an `<img>`), isolate dynamic text
  into an id'd inline element and write that one instead:
  `buy.Get<TextElement>("price").TextContent = "200";`. Fast path: when
  the only direct child is a single TextNode, the write updates it
  in place (safe for per-frame value refresh).
- **Visibility**:

  | Mechanism | Effect | Use |
  |---|---|---|
  | `Style.Display = None` | hidden, no space, no hit | routine show/hide toggles |
  | `Style.Opacity = 0` | hidden but keeps layout space (add `pointer-events:none`) | placeholder hiding |
  | `Dispose()` | permanent destroy | never needed again |

- **Lifecycle**: `RemoveFromParent` keeps subscriptions and allows
  re-attaching; `Dispose` is recursive and final. Detached nodes are
  yours to track — the framework does no ref-counting.

## Events

Two paths, same underlying routing:

- **Semantic events** (C# `event`): `button.Clicked += h` — "it
  happened" convenience sugar.
- **Routed events** (`On<T>`): `node.On<PointerDownEvent>(h,
  useCapture: true)` — coordinates, capture/bubble control. DOM
  three-phase model (capture → target → bubble);
  `IRouteEvent` exposes `Target`, `CurrentTarget`, `StopPropagation()`,
  `PreventDefault()`.

| Category | Events |
|---|---|
| Pointer | PointerDown/Up/Move/Enter/Leave, Click (`PointerButton` Left/Right/Middle) |
| Drag | DragStart/Move/End |
| Keyboard | KeyDown/Up |
| Focus | Focus/Blur |
| Scroll | ScrollChanged |
| AnimationHandle | AnimationStart/End/Iteration, TransitionEnd |
| Tween | TweenComplete (tag-routed; powers `TweenBuilder.OnComplete`) |

Unsubscribing: routed / per-frame / stylesheet return `IDisposable`
handles (`On<T>`, `OnUpdate`, `StyleSheet.Add`) — dispose to withdraw;
semantic events use `+=`/`-=`. `On<T>(..., once: true)` auto-withdraws
after one fire (prevents "wait for end event" leaks).

Drag & drop: registering a drag event opts the node into arbitration
(there is no `Draggable` property); the framework handles threshold /
capture / arbitration, you apply the delta. Drop targets are a user
pattern: `DragEnd` + `ui.Pick(position)`.

`DragMoveEvent.DeltaX/DeltaY` is the **per-move increment** since the
previous DragMove (the first one includes the pre-threshold travel —
accumulated, the element tracks the pointer exactly). Derive cumulative
offset from `DragStartEvent.StartPosition` + `DragMoveEvent.Position`.
Note `PointerMove` is not dispatched while a button is held — during a
drag, DragMove is the only pointer stream.

Routing rule: viewport panning/scrolling (big content, small viewport)
belongs to the `overflow:auto` scroll container (press-drag, wheel,
inertia, clamping, click-cancel all built in); the Drag API is the
low-level building block for object dragging (title bars,
drag-to-slot).

Focus: one focus per UIContext (`ui.FocusedNode`); `Focusable` is
writable at runtime; Tab/Shift+Tab focus-chain navigation is built in
(ascending positive tabindex, then DOM order, wraps at the ends);
arrow/gamepad navigation is the user-level pattern (`On<KeyDown>` +
`Focus()`).

## Controls

| HTML (role) | Type | Main API |
|---|---|---|
| button | Button : Container | Disabled, Clicked (text via Container.TextContent) |
| div role=textbox | TextField : Node | Value, Placeholder, Selection, ReadOnly, Disabled, ValueChanged, Submitted |
| div role=textbox aria-multiline=true | TextArea : Node | Value, Placeholder, Selection, ReadOnly, Disabled, ValueChanged |
| div role=spinbutton | NumberField : Node | Value, Min, Max, Step (float), Disabled, ValueChanged |
| div role=slider | Slider : Node | Value, Min, Max, Step (float), Disabled, ValueChanged, ChangeCommitted |
| div role=switch | Toggle : Node | IsChecked, Disabled, CheckedChanged |
| div role=radio | RadioButton : Node | IsChecked, Name (readonly), Disabled, CheckedChanged |
| div role=combobox | Dropdown : Node | SelectedIndex, SelectedValue, Disabled, SelectionChanged |
| div role=progressbar | ProgressBar : Node | Value, Max (float, 0-based), IsIndeterminate, AnimateValue |
| div role=tablist | TabList : Container | SelectedIndex, SelectionChanged (arrow keys/click; panels linked via `aria-controls`) |
| div role=tab | Tab : Container | (`aria-selected` synthesized from TabList.SelectedIndex) |

- Numeric control values are `float`.
- **ProgressBar value domain**: `Value` is raw in `[0, Max]` — NOT
  normalized 0..1. `Max` defaults to the HTML `aria-valuemax`
  attribute (fallback 100). The fill width is `Value / Max`
  internally; write `Value = 70` with `Max = 100` for a 70% bar.
- **ProgressBar.AnimateValue(target, durationSec = 0.4)** —
  presentation sugar: eases the fill to `target` (easeOut) instead of
  snapping. `Value` reads back the target during the animation (the
  data value); the interpolated display value only feeds rendering.
  Assigning `Value` directly cancels a running animation and wins.
  Retargeting mid-animation re-anchors from the current display value.
  CSS `transition` cannot do this (width is a layout channel); use
  this for health bars and other value-driven fills.
- RadioButtons sharing one `Name` auto-exclude; only the newly checked
  one fires `CheckedChanged`. Aggregating by name (RadioGroup) is user
  code, not framework.
- Control visual parts are addressed in CSS via `data-slot`
  (`data-slot=thumb` on a slider, `data-slot=fill` on a progressbar).

Container-semantics roles (plain `Container` from the game-code side —
their structure rules live in the `loomgui-editor` skill's fence
schema): `role=listbox` (option group), `role=tabpanel` (the panel a
tab points at via `aria-controls`), `role=dialog` (modal overlay
layer).

## ListView

```csharp
public class ListView : Container {
    public int ItemCount { get; set; }
    public UITemplate ItemTemplate { get; set; }
    public Func<int, UITemplate> TemplateSelector { get; set; }
    public Action<ListItem, int> BindItem { get; set; }
    public void ScrollToItem(int index, ScrollBehavior behavior = ScrollBehavior.Smooth);
    public void RefreshItem(int index);
    public void RefreshItems();
    public void NotifyInserted(int index, int count = 1);
    public void NotifyRemoved(int index, int count = 1);
    public void NotifyMoved(int fromIndex, int toIndex);
    public string ItemExitClass { get; set; }
}
```

`role=list` → ListView, `role=listitem` → ListItem. Layout is plain
CSS; virtualization is fully internal and is a runtime decision (never
expressed in HTML).

**Static vs data-driven — mutually exclusive, locked by first touch:**

- Setting `ItemCount` / `ItemTemplate` / `BindItem` (any one) enters
  data-driven mode: design-time listitems are cleared, virtualization
  takes over, and `AddChild`/`InsertChild`/`RemoveChild`/`Children`
  then throw `UIContractException` (`ChildCount` = `ItemCount`).
- Touching none of them keeps static mode: listitems are real content
  under the normal Container API.

Item template priority: (1) runtime `ItemTemplate` or
`TemplateSelector` (fetch design-time templates with
`view.GetTemplate("name")` and return them from the selector lambda);
(2) a single `<template id>` under the list — used automatically;
multiple `<template id>` without a selector throws
`UIContractException`; (3) the first design-time listitem structure as
fallback template.

`ItemExitClass`: when set, `NotifyRemoved` items get the class and are
recycled after `AnimationEnd`.

## AnimationHandle

Two authoring planes: **declarative CSS** (class toggle, `Play`) and the
**imperative `TweenBuilder`** (single-channel programmatic tween; see the
next section).

Declarative triggers:

1. Class toggle (declarative): `node.Classes.Add("slide-out")`; watch
   `On<AnimationEndEvent>`.
2. `node.Play("name")` → `AnimationHandle` handle (programmatic, hooks).
   Calling `Play` again with the same name is a deterministic restart
   from the beginning (replaces the previous playback of that
   animation on that node); different names coexist.
   Duration: a keyframes block with no `animation:` declaration has no
   declaration-level duration — `Play(name)` plays it at a fixed **1s**
   (no delay, single iteration, normal direction, fill both, cubic-out).
   `Play(name, durationSeconds)` overrides the 1s default.
   CSS contrast: the `animation` shorthand defaults to 0s — the 1s
   here is a programmatic-play default, not the CSS initial value.
3. Imperative single-channel tween: `node.Tween(channel)` →
   `TweenBuilder` (below).
4. `Style.SetVar` (dynamic values escape hatch).

Timing invariants: class / typed-style changes take effect at the next
frame's rematch (not immediately) — within one frame only the final
class set matters; transition baseline = the previous frame's computed
value. `:nth-child(An+B|odd|even|N)` + `animation-delay` produces
staggered entrances (list items fading in one by one).

```csharp
public sealed class AnimationHandle {
    public string Name { get; }
    public bool IsPlaying { get; }
    public float Time { get; set; }
    public void Pause(); public void Resume(); public void Stop();
    public AnimationHandle OnStart(Action cb);
    public AnimationHandle OnEnd(Action cb);
    public AnimationHandle OnKey(float percent, Action cb);
    public AnimationHandle OnHook(string name, Action cb);
}
```

Handle lifetime = that one playback; it dies when playback ends (hooks
auto-release; looped animations release on `Stop()`). Class-triggered
animations have no handle — listen for `AnimationEndEvent` globally
(`On<AnimationEndEvent>` broadcast). `OnKey(percent)` fires when the
timeline crosses a registered percentage; `OnHook(name)` crosses
`/* @loom-hook name */` comment anchors in the CSS. The last iteration
fires End only (browser `animationiteration` parity).

Scheduling:

```csharp
ui.CallLater(float delay, Action cb);   // one-shot, seconds, frame granularity
ui.CallNextFrame(Action cb);            // one-shot next frame, fires BEFORE solve
                                        // (fresh subtree Geometry still zero there)
ui.CallAfterLayout(Action cb);          // one-shot AFTER this frame's solve — Geometry
                                        // of a just-instantiated subtree is solved & readable
node.OnUpdate(Action<float> cb);        // recurring, dt = frame step
```

Style/data writes inside callbacks flush in the same frame's solve. A
throwing callback is isolated (logged, does not break other callbacks).

## TweenBuilder (imperative tween)

```csharp
node.Tween(TweenChannel.Height).FromPx(60).ToPx(220)
    .Duration(0.6f).Ease(EaseKind.CubicOut)
    .OnComplete(n => Debug.Log("done"))
    .Start();
```

Entry: `Node.Tween(TweenChannel channel) → TweenBuilder` (fluent, one
chain per tween). Defaults: duration **0.3s**, ease = the exact CSS
`ease` bezier (.25,.1,.25,1) — same truth as the CSS/fence side.

```csharp
public enum TweenChannel {
    Opacity = 0, Translate = 1, Scale = 2, Rotation = 3,
    BgColor = 4, TextColor = 5, Transform = 6,
    Width = 7, Height = 8, FlexGrow = 9, BoxShadow = 10,
}
```

`From`/`To` component counts: Opacity/Rotation 1; Translate/Scale 2;
BgColor/TextColor 4 (RGBA); Transform 5 (`[tx, ty, sx, sy, rotRad]` —
always px/radians at runtime; percent forms exist only in CSS
@keyframes). Width/Height payload is `[value, domainCode]` — prefer the
one-arg conveniences:

```csharp
FromPx(v) / ToPx(v) / FromPct(v) / ToPct(v) / FromVw(v) / ToVw(v)
// domainCode = LenDomain { Px, Pct, Vw, Vh, Vmin, Vmax }
```

**Width/Height endpoints must share one domain** (px↔px / %↔% / vw↔vw);
a cross-domain `Start()` throws `UIContractException`. Layout channels
(Width/Height/FlexGrow) relayout per frame — same-frame solve consumes
them, no extra solve is issued.

BoxShadow animates lists: `FromShadow(params TweenShadow[])` /
`ToShadow(...)` (empty array = `box-shadow:none` endpoint). Mismatched
list lengths pad with transparent zero-length shadows pairwise
(css-backgrounds-3); paired `Inset` mismatch degrades the whole list to
a discrete jump (browser semantics).

```csharp
public struct TweenShadow {
    public float OffsetX, OffsetY, Spread, Blur;
    public float R, G, B, A;          // linear RGBA
    public bool Inset;
    public static TweenShadow Outer(float ox, float oy, float blur,
                                    float spread, float r, float g, float b, float a);
    public static TweenShadow InsetShadow(... same args ...);
}
```

Chain tail: `Duration(s)` / `Delay(s)` / `Ease(EaseKind)` /
`EaseBezier(x1, y1, x2, y2)` (x∈[0,1]; out-of-range throws
`UIContractException` — exact CSS keyword curves: ease=(.25,.1,.25,1),
ease-in=(.42,0,1,1), ease-out=(0,0,.58,1), ease-in-out=(.42,0,.58,1)) /
`Repeat(extraRepeats, yoyo)` (0 = single run; yoyo reverses odd
iterations, CSS alternate) / `Tag(uint)` / `OnComplete(Action<Node>)` /
`Start()`.

```csharp
public enum EaseKind {
    Linear, QuadIn, QuadOut, QuadInOut,
    CubicIn, CubicOut, CubicInOut,
    BackIn, BackOut, BackInOut,
    StepEnd, StepStart, CubicBezier,   // CubicBezier params via EaseBezier(...)
    ElasticIn, ElasticOut, ElasticInOut,
    BounceIn, BounceOut, BounceInOut,
}
```

`OnComplete` routes through the `TweenComplete` event by tag: auto-
allocated when omitted; **one-shot** (fires once after ALL repeats, then
unregisters); re-registering the same tag replaces the earlier callback.
Transition-emitted tweens fire the legacy `TransitionEnd` path —
unregistered tags never disturb it. `Start()` contract exceptions
(also: BoxShadow channel without both shadow endpoints) throw
`UIContractException` instead of the FFI's defensive no-op.

Channels vs CSS transitions: same engine, no conflict resolution needed
beyond write order (animation players overwrite tweens on the same
channel within a frame). CSS-side cross-domain/auto endpoint combos snap
with a `TransitionSnap` debug-log event (=29); the TweenBuilder side
rejects them up front instead.

## Styling

Four paths, in order: HTML/CSS (design time) → `Classes` → typed
`Style` (inline override) → `Style.SetVar`.

```csharp
public sealed class ClassList {
    public void Add(string name);
    public void Remove(string name);
    public bool Contains(string name);
    public void Toggle(string name);
    public void Set(string name, bool on);
    public void Replace(string oldName, string newName);
}
public class StyleSheet {
    public IDisposable Add(string css);   // handle-based withdrawal; parse failure throws UIStyleException
    public void Clear();
}
```

`!important` is rejected at package build time.

## UIContext / packages / templates

```csharp
public sealed class UIContext {
    public Container Root { get; }
    public Node FocusedNode { get; }
    public StyleSheet StyleSheet { get; }
    public UIPackage LoadPackage(string name, byte[] bytes);
    public void UnloadPackage(string name);
    public T Create<T>() where T : Node;
    public void CallLater(float delay, Action callback);
    public void CallNextFrame(Action callback);
    public void CallAfterLayout(Action callback);   // fires after this frame's solve
    public bool IsPointerOnUI { get; }
    public Node Pick(LoomVector2 globalPoint);
}
public sealed class UIPackage {
    public string Name { get; }
    public Container Instantiate(string templatePath);   // real root type
    public UITemplate GetTemplate(string templatePath);
}
public sealed class UITemplate {
    public string Name { get; }
    public Container Instantiate();
}
```

- `Create<T>` whitelist: `Container`, `AbsolutePanel`, `TextNode`,
  `Image` only. Controls and scope roots come exclusively from
  template instantiation (`Instantiate`) — their semantics need the
  HTML signature. Other `T` throws `UIContractException`.
- `LoadPackage`/`Instantiate` are synchronous (fetch bytes async
  yourself). Duplicate `LoadPackage` with the same name throws
  `UIContractException`; load failures throw `UIPackageException`.
- `UnloadPackage` = prefab semantics: templates/package resources are
  released, live instances survive as independent copies.
- `Image.Src` is a string key (package-internal or runtime-registered
  via the engine backend, e.g. Unity `SpriteResolver.Register`).
  Package-internal keys are workspace-relative asset paths as baked
  into the atlas manifest, e.g. `"res/icons/item-potion.png"` (the
  `src` you wrote in HTML). Unknown key = silent error state + one
  warning per unique key in the console, no throw. To verify a key
  hit, set a known-good key (one referenced by existing HTML) on the
  same node and compare.

## LoomStageDriver serialized fields

Programmatic setup (`SerializedObject`) and Inspector scripting use
these field names (defaults in parentheses):

| Field | Type | Notes |
|---|---|---|
| `_designSize` | UnityEngine.Vector2 | authoring resolution **fallback** — used only when `loom.runtime.json` omits `design` (the workspace is the source of truth; set it there via `loom design`). Default (1080,1920) is portrait |
| `_adaptMode` | AdaptMode enum | adaptation mode **fallback** (`Letterbox` default / `FitWidth` / `FitHeight`) — used only when the manifest omits `match_mode` |
| `_safeArea` | bool | notch-safe letterboxing (true) |
| `_showFps` | bool | FPS overlay (false) |
| `_uiCamera` | Camera | null = driver creates `LoomUICamera` |
| `_inputCollector` | LoomInputCollector | null = `GetComponent` fallback |
| `_productRoot` | string | empty = Editor `Assets/Bundles` / player StreamingAssets |

## Exceptions

| Exception | Meaning |
|---|---|
| `UIContractException` | contract violation (bad `Get<T>` scope/id, illegal Create T, data-mode API misuse, duplicate package) |
| `ObjectDisposedException` | operation on a disposed node |
| `UIStyleException` | runtime CSS parse failure (`StyleSheet.Add`) |
| `UIPackageException` | package load failure |

## North-star snippet

```csharp
AbsolutePanel layer = ui.Create<AbsolutePanel>();
layer.Style.Width = Length.Pct(100);
layer.Style.Height = Length.Pct(100);
ui.Root.AddChild(layer);

Container inventory = game.Instantiate("views/inventory");
layer.AddChild(inventory);
inventory.Style.Left = Length.Px(300);
inventory.Style.Top = Length.Px(200);

// routine toggle: hide but keep node + state + subscriptions
closeButton.Clicked += () => inventory.Style.Display = DisplayMode.None;

// one-shot popup: play exit animation, then destroy (once prevents leak)
dismissButton.Clicked += () => {
    popup.Classes.Add("slide-out");
    popup.On<AnimationEndEvent>(_ => popup.Dispose(), once: true);
};
```
