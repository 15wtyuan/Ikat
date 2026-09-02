# Ikat Runtime API Reference

Complete lookup table for the business-programmer API (nodes, controls,
events, lists, animation, styling). The authoritative source is the
shipped C# signatures (`Ikat.*.cs` in the Unity package) — this file
mirrors their contract so you never need the Ikat repository. Load
this on demand; the `ikat-runtime` SKILL.md is the workflow manual.

## Object hierarchy

```text
Node
+-- Container (subtree = user content, arrangerble at runtime)
|   +-- AbsolutePanel (sugar: children auto position:absolute)
|   +-- TextElement (span)
|   +-- Link (a, rich-text link)
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
    public bool Draggable { get; set; }        // runtime face of the HTML draggable attr
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
    public IkatColor BackgroundColor/TextColor { get; set; }   // text color = CSS color channel
    public float Opacity { get; set; }
    public void SetVar(string name, Length/IkatColor/float/string value);
    public void RemoveVar(string name);
}
public sealed class NodeTransform {
    public IkatVector2 Position { get; set; }
    public IkatVector2 Scale { get; set; }
    public float Rotation { get; set; }                  // radians
    public IkatVector2 Origin { get; set; }
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
    public IkatVector2 ScrollPos { get; }                    // (0,0) on non-scrolling
    public void RestartAnimations();                     // rebuild declarative players on this node AND its subtree; programmatic (Play) players untouched; node state kept
    public void ScrollTo(IkatVector2 pos, ScrollBehavior behavior = ScrollBehavior.Smooth);
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

Drag & drop: dragging must be enabled explicitly — either
`draggable="true"` in the HTML (global attribute, values `true` /
`false`, default `false`) or `Node.Draggable` at runtime. An enabled
node joins the drag_target candidates (nearest draggable node on the
pointer-down hit chain); the framework handles threshold / capture /
arbitration, you apply the delta. Drop targets are a user
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
| div role=progressbar | ProgressBar : Node | Value, Min, Max (float), IsIndeterminate, AnimateValue |
| div role=tablist | TabList : Container | SelectedIndex, Activation, SelectionChanged (arrow axis follows `flex-direction`, clamped no wrap; panels linked via `aria-controls`. Activation model — `data-activation="manual"` in HTML or `Activation` at runtime: `Automatic` (default) selects as arrows move focus; `Manual` moves focus only, Enter/Space commits) |
| div role=tab | Tab : Container | (`aria-selected` synthesized from TabList.SelectedIndex) |
| div role=tree | Tree : Container | SelectedItem (TreeItem, null when empty), ExpandAll(), CollapseAll(), SelectionChanged (fires on click / keyboard interaction only — programmatic SelectedItem set does not emit; read SelectedItem at event time for the current item) |
| div role=treeitem | TreeItem : Container | IsBranch, Expanded (branch only — leaf reads/writes throw InvalidOperationException), Selected (derived from owning Tree), Level (aria-level: top = 1), Select(), ExpandedChanged (interaction only), Clicked (hit resolution lands on the treeitem) |
| a (inside rich text) | Link : Container | Href (readonly), Clicked |

- Numeric control values are `float`.
- **ProgressBar value domain**: `Value` is raw in `[Min, Max]` — NOT
  normalized 0..1. `Min`/`Max` default to the HTML `aria-valuemin`/
  `aria-valuemax` attributes (fallback 0 / 100). The fill width is
  `(Value - Min) / (Max - Min)` internally (ARIA semantics); with the
  default `Min = 0` write `Value = 70` with `Max = 100` for a 70% bar.
- **ProgressBar.AnimateValue(target, durationSec = 0.4)** —
  presentation sugar: eases the fill to `target` (easeOut) instead of
  snapping. `Value` reads back the target during the animation (the
  data value); the interpolated display value only feeds rendering.
  Assigning `Value` directly cancels a running animation and wins.
  Retargeting mid-animation re-anchors from the current display value.
  CSS `transition: width` animates the same channel as a plain visual
  tween — prefer `AnimateValue` for value-driven fills (health bars
  etc.): it keeps the data-value semantics and re-anchoring.
- RadioButtons sharing one `Name` auto-exclude; only the newly checked
  one fires `CheckedChanged`. Aggregating by name (RadioGroup) is user
  code, not framework.
- Control visual parts are addressed in CSS via `data-slot`
  (`data-slot=thumb` on a slider, `data-slot=fill` on a progressbar).

**Link (`<a>`, rich-text link)**: only legal inside a rich-text-block
(the fence rejects it elsewhere at pack time); children are text and
nested `span`s only. `Href` is a read-only **opaque identifier** — the
framework never parses or opens it; read it back and route yourself
(`link.Clicked += () => OpenShop(link.Href);`). Clicking uses the
existing `Clicked` semantic event (hit-testing resolves down to the a
node, including text inside nested spans). UA default style is blue
(#0000EE) + underline — override in author CSS (including `:hover`).
Keyboard focus/Enter activation is not in this stage.

Container-semantics roles (plain `Container` from the game-code side —
their structure rules live in the `ikat-editor` skill's fence
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

Item template priority: (1) runtime `TemplateSelector` (per-item
explicit; fetch design-time templates with `view.GetTemplate("name")`
and return them from the selector lambda) over (1b) `ItemTemplate`
(the default blueprint); (2) a single `<template id>` under the list —
used automatically; multiple `<template id>` without a selector throws
`UIContractException`; (3) the first design-time listitem structure as
fallback template.

`TemplateSelector` is strict: once set it answers EVERY index —
returning null throws `UIContractException` (return the default
`UITemplate` explicitly if you want a fallback). Sources must be
in-scene subtrees (`GetTemplate` results or `Instantiate()` clones);
raw package-component templates need `Instantiate()` first. Changing
the selector (or `Notify*` re-evaluation) re-materializes affected
items on the new blueprint.

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
`/* @ikat-hook name */` comment anchors in the CSS. The last iteration
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
    public TextMetrics MeasureText(string text, string fontFamily, float sizePx, float maxWidth = 0f);
    public void CallLater(float delay, Action callback);
    public void CallNextFrame(Action callback);
    public void CallAfterLayout(Action callback);   // fires after this frame's solve
    public bool IsPointerOnUI { get; }
    public Node Pick(IkatVector2 globalPoint);
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
public readonly struct TextMetrics {
    public float W { get; }        // px
    public float H { get; }        // px
    public uint LineCount { get; } // wrapped line count (1 when no maxWidth)
}
```

- `Create<T>` whitelist: `Container`, `AbsolutePanel`, `TextNode`,
  `Image` only. Controls and scope roots come exclusively from
  template instantiation (`Instantiate`) — their semantics need the
  HTML signature. Other `T` throws `UIContractException`.
- **Custom elements have no runtime node type.** A `<my-widget>` in
  the workspace HTML is a CustomElement at *pack* time; from C# the
  expanded instance is an ordinary `Container` obtained via
  `Instantiate("my-widget")` (or `GetTemplate` + deferred
  `Instantiate()`) — the registered stem, no `components/` prefix and
  no extension (`ikat show <pkg>` lists them). Query its internals
  through the instance scope.
- **`MeasureText` is node-free pre-layout measurement** (tips line
  breaking, floating-text width, auto-width buttons — no hand-counted
  "N chars per line" constants). It runs the same wrapping code the
  solver uses (default wrap mode = `white-space: normal`), so the
  prediction is what renders: `maxWidth > 0` wraps greedily at that
  width; `maxWidth <= 0` (default) measures one line. An unbreakable
  word wider than `maxWidth` does **not** split — it returns one line
  with `W > maxWidth` (browser `overflow-wrap: normal` semantics); a
  page that must split such words declares `overflow-wrap:
  break-word` in CSS, and the node then wraps wider than this
  default-mode prediction. Line height = `normal`, letter-spacing 0,
  regular weight (matches default-styled text nodes). `fontFamily`
  must be a registered family (`IkatHost.RegisterFont` / runtime
  manifest) — unknown family throws `UIContractException` instead of
  silently falling back to the default
  font (measuring with the wrong font is worse than not measuring).
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

## IkatStageDriver serialized fields

Programmatic setup (`SerializedObject`) and Inspector scripting use
these field names (defaults in parentheses):

| Field | Type | Notes |
|---|---|---|
| `_designSize` | UnityEngine.Vector2 | authoring resolution **fallback** — used only when `ikat.runtime.json` omits `design` (the workspace is the source of truth; set it there via `ikat design`). Default (1080,1920) is portrait |
| `_adaptMode` | AdaptMode enum | adaptation mode **fallback** (`Letterbox` default / `FitWidth` / `FitHeight`) — used only when the manifest omits `match_mode` |
| `_safeArea` | bool | notch-safe letterboxing (true) |
| `_showFps` | bool | FPS overlay (false) |
| `_uiCamera` | Camera | null = driver creates `IkatUICamera` |
| `_inputCollector` | IkatInputCollector | null = `GetComponent` fallback |
| `_productRoot` | string | empty = Editor `Assets/Bundles` / player StreamingAssets |

## World anchoring & world-space mounts (Unity driver)

Two complementary routes for tying UI to the 3D world (engine-integration
layer, same class as NativeHost — see `docs/design/public-api.md` §11.3):

```csharp
// Projection route — UI tracks a 3D point (damage numbers, health bars):
// driver projects worldPos through the camera every frame (before Step)
// and writes node.Transform.Position in design px. Off-screen / behind
// the camera auto-hides the node (render-only, subtree-inherited —
// layout and hit-testing are untouched). Node must be a direct child of
// the page root styled `position:absolute; left:0; top:0` so its layout
// slot is (0,0) and Transform.Position acts as absolute coords.
// Re-Set with the same node to update worldPos (following a moving
// entity); offsetPx is in design px (y-down: negative y = up).
public void SetWorldAnchor(Node node, Camera camera, Vector3 worldPos, Vector2 offsetPx);
public void ClearWorldAnchor(Node node);
public int WorldAnchorCount { get; }

// Runtime render-only visibility (the same channel the anchor auto-hide
// uses): false hides the node's whole subtree render output (inherited
// like CSS visibility:hidden; mirror objects are kept, layout and hit
// testing are untouched). Orthogonal to display:none.
public void SetNodeRenderVisible(Node node, bool visible);

// Second-stage bootstrapping (runtime-spawned drivers): set layer order
// and the shared-host flag BEFORE Awake — AddComponent on an inactive
// GameObject, call this, then SetActive(true). The spawned driver
// GameObject also needs its own input collector for probe/collect to
// see any input.
public void ConfigureStage(int stageOrder, bool useSharedHost);

// Mount route — a whole subtree renders under a business 3D transform:
// rows are re-based to the mount root's local frame (its design position
// becomes the local origin) and parented to worldParent through a y-flip
// container; row layer follows the container (scene camera renders it,
// ZTest LEqual gives real 3D occlusion). Layout/hit stay in screen space.
// v1 constraints: the mount root must be a stacking context (declare
// z-index); no dropdowns / scroll containers / outer-shadow roots /
// overflow clip inside the mounted subtree.
public void BindWorldMount(Node mountRoot, Transform worldParent);
public void UnbindWorldMount(Node mountRoot);

// Damage numbers = business-side TweenBuilder x anchor combo (e.g.
// TweenChannel.Opacity fade while the anchor offset floats upward).
```

Multi-stage hit etiquette: the page document root is never a hit target,
but a full-canvas overlay page should still declare `pointer-events: none`
on its root (`auto` on the interactive panel) — otherwise its content area
starves every stage below it of pointer input.

## Runtime diagnostics

```csharp
// IkatHost (public; dev bridges — not frozen API):
public string DumpSceneJson();                     // full scene JSON
public string DumpSceneTree(string filter = null); // human-readable tree
```

- **`DumpSceneTree(filter)`** — one line per node
  (`div#id.class (x,y,w,h)`), ASCII-nested; text nodes append
  `font=<px> lh=<multiplier>x lines=<n>` plus a content snippet,
  scroll containers append
  `scroll[vp WxH ct WxH ov WxH pos x,y tw a,b]`. `filter` = id/class
  substring → only matching subtrees print (no more grepping a full
  dump). A `lh=26.00x` next to `font=16` is the smoking gun for a
  unitless `line-height: 26` (multiplier, not pixels).
- **`DumpSceneJson()`** — machine-readable; same resolved fields as the
  tree view in `"text"` / `"scroll"` blocks per node.
- **F8** (editor / development builds) dumps blob state + mirror pool
  **+ a `[Scene tree]` section** to the console and
  `ikat-dump-<time>.txt` next to the project — read the `[Scene tree]`
  section first for layout attribution.
- **Runtime warnings** (`[Ikat]` prefix in the console,
  editor/development builds only): e.g.
  `wheel ignored: node N declares overflow:auto/scroll but has no
  overflow to scroll (content fits the viewport, overlap=0)` — the
  scroll container under the wheel never scrolls because its content
  does not overflow; fix content sizing or drop `overflow:auto`.

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
