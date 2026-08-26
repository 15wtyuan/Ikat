---
name: loomgui-runtime
description: |
  Integrate LoomGUI UI into Unity game code — mount the stage driver,
  load build artifacts, instantiate pages, look up typed nodes by id,
  wire control events, drive UI from gameplay, gate 3D input on the UI,
  and embed 3D content in UI. Use for ANY C#/Unity task that touches
  LoomStageDriver, pages, nodes, or UI events; also when adding or
  renaming element ids in HTML (ids are the C# API surface).
---

# LoomGUI Runtime Integration (Unity)

Wire the built UI into the game: mount, instantiate, program, interop
with 3D. The design side (HTML/CSS fence authoring) is the
`loomgui-editor` skill; workspace/build operations are the `loom` skill.

## Prerequisites

- The LoomGUI Unity package is installed, version-matched to the `loom`
  CLI that built the artifacts (both come from the same release).
- Build artifacts exist and are current: read `.loom/config.json` at the
  session root — `ui_root` locates the workspace, `unity_root` (if
  present) is where `loom build` delivered `Assets/Bundles` (package
  picker: `loom.runtime.json`, `ui/*.pkg.bin`, `atlas/*`, `fonts/*`).
  Sources changed → rebuild before debugging C#.
- A default font was registered in the workspace, or all text renders
  blank.

## Mental model

- LoomGUI UI is its own fullscreen camera-space layer: a dedicated
  orthographic UI camera composites with your 3D camera by depth. No uGUI
  Canvas, no URP overlay stack, no EventSystem.
- The UI scene is a typed C# object tree (`Container`, `Button`,
  `Slider`, `ListView`, ...). Game code reads and drives that tree; it
  never touches meshes or materials.
- One MonoBehaviour (`LoomStageDriver`) owns the frame pipeline — input →
  UI logic → layout/render → mesh mirror → events. You never call a
  per-frame update yourself.

## Required workflow

1. **Mount.** Create a GameObject with `LoomStageDriver` +
   `LoomInputCollector`. Design resolution and adaptation mode come from
   `loom.runtime.json` (`design` + `match_mode`, set in the workspace via
   `loom design`); the Inspector Design Size / Adapt Mode fields are only
   the fallback when the manifest omits them. UI Camera empty (driver
   creates `LoomUICamera`) or your own; Safe Area for notch-safe
   letterboxing. Your main 3D camera renders first; the UI camera after
   (higher depth, clear flags = Depth only). Layer 6 is reserved by
   LoomGUI.
2. **Verify loading.** On startup the driver reads `loom.runtime.json`
   from the product root and loads everything it lists; missing pieces
   log Console warnings naming the file. Product root: Inspector value →
   in Editor `Assets/Bundles` → in players `Application.streamingAssetsPath`
   (copy the Bundles content there before building a player).
3. **Instantiate a page.**
   `driver.Instantiate("<package>", "<page-stem>")` — page stem = HTML
   filename without `.html` (`"game", "main"` ← `ui/game/main.html`).
   Returns the page root `Container`; `null` + console error = package
   not in `loom.runtime.json` or wrong stem.
4. **Look up nodes by id and wire events.**
5. **Drive UI from gameplay** where the game state leads.
6. **Gate 3D input on the UI** (`IsPointerOnUI`); embed 3D in UI via
   `BindNativeHost` where needed.
7. **Verify in Play Mode** (unity-cli-loop skills: screenshot, simulated
   input). F8 during Play dumps core + mirror state to the Console and a
   `loom-dump-*.txt` next to `Assets/` — the first evidence when a page
   looks wrong (it separates "core computed wrong layout" from "Unity
   rendered it wrong").

The driver is `[ExecuteAlways]` — pages also render in Edit Mode.

## Programming the tree

```csharp
using LoomGUI;

public class GameUI : MonoBehaviour
{
    LoomStageDriver _driver;
    Container _page;

    void Start()
    {
        _driver = GetComponent<LoomStageDriver>();
        _page = _driver.Instantiate("game", "main");
        if (_page == null) { Debug.LogError("page failed to mount (package not in loom.runtime.json? wrong stem?)"); return; }

        _page.Get<Button>("btn-start").Clicked += OnStart;
    }

    void OnStart() { /* gameplay: load a scene, spawn units, ... */ }
    void OnDestroy() => _page?.Dispose();
}
```

- **The id contract.** `id` attributes in the workspace HTML are the API
  surface game code programs against — adding/renaming an id is a
  cross-side change (tell the `loomgui-editor` side). `Get<T>(id)` throws
  `UIContractException` on miss; `TryGet<T>(id, out var n)` for optional
  elements. Lookup is scoped to the current component instance — it does
  not cross nested custom-component or list-item boundaries; reach into
  a nested component through its host node.
- **UI drives gameplay** — subscribe to control events:
  `button.Clicked`, `slider.ValueChanged` (`e.NewValue`),
  `textField.Submitted` (`text =>`), `dropdown.SelectionChanged`
  (`e.NewIndex`), `toggle.CheckedChanged`; routed events via
  `node.On<PointerDownEvent>(...)` (returns an `IDisposable`).
- **Gameplay drives UI** — hold node references and mutate: text via
  `container.TextContent` / `TextNode.Text`, values via `slider.Value` /
  `progressBar.Value`, virtualized lists via `listView.ItemCount` +
  `BindItem`, visual state via `node.Classes.Add(...)`, show/hide via
  `node.Style.Display = DisplayMode.None` (collapses layout + removes
  hit-testing; `DisplayMode.Block` / `Flex` restores — no need to
  hand-roll `.hide` classes), declarative animations via
  `node.Play("name")` (no-declaration keyframes play at a fixed 1s;
  `Play(name, seconds)` overrides), imperative single-channel tweens
  via `node.Tween(TweenChannel.X)...Start()` (fluent builder — layout
  channels Width/Height/FlexGrow, box-shadow lists, full ease set;
  endpoints in `references/api-reference.md`), text color inline via `node.Style.TextColor`, per-node
  logic via `node.OnUpdate(dt)` and `driver.Context.CallLater` /
  `CallNextFrame` (fires before solve — fresh-subtree Geometry is still
  zero there; use `CallAfterLayout` to read solved Geometry the same
  frame). Inline style overrides sit on `node.Style.*`.
- **Lifecycle.** `_page.Dispose()` recursively destroys the subtree and
  clears event subscriptions. Page-swap pattern: Dispose old →
  Instantiate new → re-wire. Runtime package load/unload:
  `driver.Context.LoadPackage(name, bytes)` / `UnloadPackage(name)`
  (prefab semantics: live instances survive the unload).

Full signatures and invariants for everything below (all controls,
ListView modes, animation hooks, exceptions) are in
`references/api-reference.md` next to this file — consult it before
guessing an API name.

## Dynamic content paradigm

Repeated or data-driven UI (generated map nodes, battle status rows,
cards) is **template instantiation + Query injection** — not element-by-
element `Create<T>` assembly. The division of labor:

- **Structure and interaction styling are declarative.** Author a
  `<template id="...">` (or a custom component) in the workspace HTML:
  internal arrangement, hover/selected states, `:nth-child` staggering,
  transitions — all CSS, all previewable. Instantiate per data item and
  inject data through the instance-scoped `Get<T>(id)` / `Query`.
- **Coordinate-like data positions imperatively.** Map pins and similar
  absolute placement are legitimately `node.Style.Left = Length.Px(x)` —
  but the node's *internal* structure still comes from the template.
- **`Create<T>` is for one-off structural wrappers only.** Hand-assembling
  repeated content forfeits the CSS layer: no hover, no `:nth-child`,
  no transitions — and no fence validation of what you build.

```csharp
var tpl = page.GetTemplate("map-node");        // <template id="map-node">
foreach (var pin in pins)
{
    var node = tpl.Instantiate();              // fresh copy per item
    node.Get<TextElement>("name").TextContent = pin.Name;   // instance-scoped
    node.Get<TextElement>("count").TextContent = "LV." + pin.Level;
    node.Style.Left = Length.Px(pin.X);        // coordinates: imperative
    mapLayer.AddChild(node);
}
```

**Runtime class toggling is the styling channel.** Classes that only
exist for runtime states (`dyn-selected`, `dyn-armed`, ...) are declared
like any other CSS — put them in a dedicated stylesheet next to the page
(`my-page.dynamic.css`, loaded via `<link rel="stylesheet">`) so dynamic
rules have a declared home: fence-validated, packed, and previewed like
every other stylesheet. Game code never builds CSS strings — it toggles
classes (`node.Classes.Toggle("dyn-selected")`) and the cascade does the
rest.

**Pseudo-classes apply to instantiated templates and runtime-created
nodes alike.** `:hover` / `:active` / `:checked` recompute from live
pointer and control state every frame; `:nth-child` follows the current
tree position (runtime-appended children count). They are part of the
normal cascade — no C# event wiring needed for hover styling.

**Layout-time text sizing needs no magic numbers.** When content decides
geometry (tips panels, floating damage numbers, auto-width buttons), call
`driver.Context.MeasureText(text, family, sizePx, maxWidth)` before
layout: same wrapping code the solver uses, so the prediction is what
renders. Returns `(W, H, LineCount)`; `maxWidth <= 0` measures a single
line.

## Resolution adaptation

Design resolution and adaptation mode are workspace-level config
(`loom design 1920x1080 --match fit-width` in the UI workspace; `loom
build` bakes both into `loom.runtime.json`, the driver reads them at
startup). Three modes:

- `letterbox` (default) — contain: the canvas stays at the design
  resolution, uniformly scaled to fit the safe area, centered, with
  bars. Layout always matches the design draft exactly.
- `fit-width` / `fit-height` — barless reflow: one axis locks to the
  design, the other takes the real screen (canvas changes → the core
  re-lays-out next frame). `px` never distorts (scaling stays uniform);
  flexible content flows via `%`, `flex` and the `vw`/`vh`/`vmin`/
  `vmax` units (viewport-relative lengths: `100vw` = canvas width —
  unlike `%`, they resolve against the canvas, not the parent).
  Pages written in absolute `px` won't break in fit modes — extra
  canvas space just stays empty; author flow with the viewport units.

Runtime resolution/rotation changes are handled automatically (the
driver recomputes on screen or safe-area change and calls
`LoomHost.SetRootSize`). Safe area: fit modes size the canvas from the
safe rect (content never flows into a notch); letterbox fits inside it.

## UI ↔ 3D interop

- **Input gating.** LoomGUI never consumes or blocks input — your 3D
  picking must gate itself:

  ```csharp
  if (driver.Context.IsPointerOnUI) return;   // pointer is over UI — don't raycast the world
  if (Physics.Raycast(cameraRay, out var hit)) SelectUnit(hit.transform);
  ```

  `driver.Context.Pick(point)` hit-tests the UI from game code (design
  coordinates: pixels, origin top-left, y down).
- **Embedding 3D inside UI.** `driver.BindNativeHost(node, go)` pins any
  GameObject (3D model, particles, camera feed) to a UI node — character
  preview slots, decorated cards. Every frame the binding copies the
  node's world transform, visibility (`display:none` →
  `SetActive(false)`) and sort order (interleaved with UI meshes in the
  transparent queue; materials auto-clone to URP transparent). The GO's
  own hierarchy, scale and animations remain yours. Unbind with
  `driver.UnbindNativeHost(node)`; disposing the node auto-hides it.
- **Not supported — world-space UI.** The UI is always fullscreen
  camera-space; it cannot be pinned to a 3D transform or camera plane.
  The supported direction is the inverse (3D embedded in UI via
  NativeHost).

## Serving artifacts from AssetBundles / Addressables

Subclass `LoomStageDriver` and override the virtual loading hooks
(defaults read plain files under the product root):

| Hook | Loads | Example argument |
|---|---|---|
| `LoadTextFile(relPath)` | manifests (text) | `loom.runtime.json`, `atlas/ui.atlas.json` |
| `LoadBytes(relPath)` | raw bytes | `ui/game.pkg.bin` |
| `LoadPackageBytes(name)` | a package (default: `LoadBytes("ui/{name}.pkg.bin")`) | `game` |
| `LoadTexture(relPath)` | atlas page PNG | `atlas/ui.png` |
| `LoadFontBytes(fontFile)` | font bytes (the manifest's `file` value) | `NotoSansSC.ttc.bytes` |

## Checklist

- [ ] Driver + collector on one GameObject; Design Size matches the
      workspace; UI camera depth above the 3D camera.
- [ ] Page instantiated non-null; ids in `Get<T>` exist in the current
      HTML (no silent renames).
- [ ] Event subscriptions cleaned up on page Dispose.
- [ ] 3D raycasts gated on `driver.Context.IsPointerOnUI`.
- [ ] Frame-loop expectations: UI time runs under `Time.timeScale = 0`
      (pause menus work); screen-size changes re-fit automatically.
- [ ] Verified in Play Mode (screenshot / simulated input), not just
      "no compile errors".

## Error recovery

| Symptom | Cause → fix |
|---|---|
| `Instantiate` returns null | package not listed in `loom.runtime.json` or wrong stem → rebuild workspace; check package name |
| Page blank | artifacts stale or font missing → `loom build`; register a default font |
| `Get<T>` throws `UIContractException` | id missing/renamed in HTML, or lookup crossing a component boundary → fix the contract (both sides), or go through the host node |
| Text renders but looks wrong | font fallback missing glyphs → register a `--fallback` font |
| Tofu boxes (□) in text | the Console logs every missing glyph as `[LoomGUI] missing glyphs (tofu): font-family "X" has no glyph for 'c' (U+....)` — fix by registering a font containing it with `--fallback`, or change the text |
| Clicks pass through UI to 3D | expected — LoomGUI never blocks input → gate your raycasts on `IsPointerOnUI` |
| Page looks wrong at runtime | press F8, read the core-vs-mirror dump → if core dump is wrong it's a workspace/layout issue, if only Unity differs it's a backend issue |

## Reference consumer

- **Full API contract** (every node/control/event/list/animation
  signature and invariant): read `references/api-reference.md` next to
  this file — it mirrors the shipped C# signatures, so you never need
  the LoomGUI repository.
- **Complete working example**: `unity/showcase-unity/` in the LoomGUI
  repository (driver mounted, nine pages wired from
  `ShowcaseRunner.cs`). Optional copy-paste source, not required
  reading.
