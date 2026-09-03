---
name: ikat-runtime
description: |
  Integrate Ikat UI into Unity game code — mount the stage driver,
  load build artifacts, instantiate pages, look up typed nodes by id,
  wire control events, drive UI from gameplay, gate 3D input on the UI,
  embed 3D content in UI, pin UI to the 3D world (world anchors for
  damage numbers / health bars, world-space mounts for panels attached
  to 3D transforms), and run multiple UI stages in one scene. Use for
  ANY C#/Unity task that touches IkatStageDriver, pages, nodes, or UI
  events; also when adding or renaming element ids in HTML (ids are the
  C# API surface).
---

# Ikat Runtime Integration (Unity)

Wire the built UI into the game: mount, instantiate, program, interop
with 3D. The design side (HTML/CSS fence authoring) is the
`ikat-editor` skill; workspace/build operations are the `ikat` skill.

## Prerequisites

- The Ikat Unity package is installed, version-matched to the `ikat`
  CLI that built the artifacts (both come from the same release).
- Build artifacts exist and are current: read `.ikat/config.json` at the
  session root — `ui_root` locates the workspace, `unity_root` (if
  present) is where `ikat build` delivered `Assets/Bundles` (package
  picker: `ikat.runtime.json`, `ui/*.pkg.bin`, `atlas/*`, `fonts/*`).
  Sources changed → rebuild before debugging C#.
- A default font was registered in the workspace, or all text renders
  blank.

## Mental model

- Ikat UI is its own fullscreen camera-space layer: a dedicated
  orthographic UI camera composites with your 3D camera by depth. No uGUI
  Canvas, no URP overlay stack, no EventSystem. (World-attached UI —
  anchors and mounts — builds on top of this layer; see UI ↔ 3D
  interop below.)
- The UI scene is a typed C# object tree (`Container`, `Button`,
  `Slider`, `ListView`, ...). Game code reads and drives that tree; it
  never touches meshes or materials.
- One MonoBehaviour (`IkatStageDriver`) owns the frame pipeline — input →
  UI logic → layout/render → mesh mirror → events. You never call a
  per-frame update yourself.

## Required workflow

1. **Mount.** Create a GameObject with `IkatStageDriver` +
   `IkatInputCollector`. Design resolution and adaptation mode come from
   `ikat.runtime.json` (`design` + `match_mode`, set in the workspace via
   `ikat design`); the Inspector Design Size / Adapt Mode fields are only
   the fallback when the manifest omits them. UI Camera empty (driver
   creates `IkatUICamera`) or your own; Safe Area for notch-safe
   letterboxing. Your main 3D camera renders first; the UI camera after
   (higher depth, clear flags = Depth only). Ikat renders on Unity's
   **built-in `UI` layer (5)** — a fixed, non-renameable layer, so it can
   never collide with your project's user layers (layers 6–31 are
   user-definable; a framework squatting on one of those is a design
   bug). Standard Unity practice applies: exclude
   the `UI` layer from your 3D cameras' culling masks, or UI quads get
   drawn twice.
2. **Verify loading.** On startup the driver reads `ikat.runtime.json`
   from the product root and loads everything it lists; missing pieces
   log Console warnings naming the file. Product root: Inspector value →
   in Editor `Assets/Bundles` → in players `Application.streamingAssetsPath`
   (copy the Bundles content there before building a player).
3. **Instantiate a page.**
   `driver.Instantiate("<package>", "<page-stem>")` — page stem = HTML
   filename without `.html` (`"game", "main"` ← `ui/game/main.html`).
   Returns the page root `Container`; `null` + console error = package
   not in `ikat.runtime.json` or wrong stem.
4. **Look up nodes by id and wire events.**
5. **Drive UI from gameplay** where the game state leads.
6. **Gate 3D input on the UI** (`IsPointerOnUI`); embed 3D in UI via
   `BindNativeHost` where needed.
7. **Verify in Play Mode** (unity-cli-loop skills: screenshot, simulated
   input). F8 during Play dumps core + mirror state to the Console and a
   `ikat-dump-*.txt` next to `Assets/` — the first evidence when a page
   looks wrong (it separates "core computed wrong layout" from "Unity
   rendered it wrong").

The driver is `[ExecuteAlways]` — pages also render in Edit Mode.

## Programming the tree

```csharp
using Ikat;

public class GameUI : MonoBehaviour
{
    IkatStageDriver _driver;
    Container _page;

    void Start()
    {
        _driver = GetComponent<IkatStageDriver>();
        _page = _driver.Instantiate("game", "main");
        if (_page == null) { Debug.LogError("page failed to mount (package not in ikat.runtime.json? wrong stem?)"); return; }

        _page.Get<Button>("btn-start").Clicked += OnStart;
    }

    void OnStart() { /* gameplay: load a scene, spawn units, ... */ }
    void OnDestroy() => _page?.Dispose();
}
```

- **The id contract.** `id` attributes in the workspace HTML are the API
  surface game code programs against — adding/renaming an id is a
  cross-side change (tell the `ikat-editor` side). `Get<T>(id)` throws
  `UIContractException` on miss; `TryGet<T>(id, out var n)` for optional
  elements. Lookup is scoped to the current component instance — it does
  not cross nested custom-component or list-item boundaries; reach into
  a nested component through its host node.
- **UI drives gameplay** — subscribe to control events:
  `button.Clicked`, `slider.ValueChanged` (`e.NewValue`),
  `textField.Submitted` (`text =>`), `dropdown.SelectionChanged`
  (`e.NewIndex`), `tree.SelectionChanged` (`e.SelectedItem`),
  `treeItem.ExpandedChanged` (`e.Expanded`), `toggle.CheckedChanged`;
  routed events via
  `node.On<PointerDownEvent>(...)` (returns an `IDisposable`).
- **Gameplay drives UI** — hold node references and mutate: text via
  `container.TextContent` / `TextNode.Text`, values via `slider.Value` /
  `progressBar.Value`, trees via `tree.SelectedItem` /
  `treeItem.Expanded` (`ExpandAll()` / `CollapseAll()` for batches),
  virtualized lists via `listView.ItemCount` + `BindItem`, visual state via `node.Classes.Add(...)`, show/hide via
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
(`ikat design 1920x1080 --match fit-width` in the UI workspace; `ikat
build` bakes both into `ikat.runtime.json`, the driver reads them at
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
`IkatHost.SetRootSize`). Safe area: fit modes size the canvas from the
safe rect (content never flows into a notch); letterbox fits inside it.

## UI ↔ 3D interop

- **Input gating.** Ikat never consumes or blocks input — your 3D
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
- **UI pinned to the 3D world** (#109) — two routes, pick by need:
  - **World anchors (projection route)** — HUD elements that track a 3D
    point: `driver.SetWorldAnchor(node, camera, worldPos, offsetPx)`
    re-projects every frame and writes the node's transform; off-screen
    or behind the camera auto-hides the subtree (render-only, layout and
    hit-testing untouched). Damage numbers = anchor + `TweenChannel.
    Opacity` fade; health bars = anchor re-Set with the moving entity.
    The anchored node must be a page-root child styled
    `position:absolute; left:0; top:0` (layout slot (0,0) → transform
    acts as absolute coords).
  - **World-space mounts** — a whole panel rendered under a business 3D
    transform: `driver.BindWorldMount(node, worldParent)` re-bases the
    subtree's rows into the mount root's local frame and parents the
    mirror objects through a y-flip container (scene camera renders
    them; depth testing gives real 3D occlusion). Layout and hit-testing
    stay in screen space. v1 constraints: mount root declares a
    z-index; no dropdowns / scroll containers / overflow clip inside
    the mounted subtree.
  Full signatures: `references/api-reference.md` § "World anchoring &
  world-space mounts".

## Serving artifacts from AssetBundles / Addressables

Subclass `IkatStageDriver` and override the virtual loading hooks
(defaults read plain files under the product root):

| Hook | Loads | Example argument |
|---|---|---|
| `LoadTextFile(relPath)` | manifests (text) | `ikat.runtime.json`, `atlas/ui.atlas.json` |
| `LoadBytes(relPath)` | raw bytes | `ui/game.pkg.bin` |
| `LoadPackageBytes(name)` | a package (default: `LoadBytes("ui/{name}.pkg.bin")`) | `game` |
| `LoadTexture(relPath)` | atlas page PNG | `atlas/ui.png` |
| `LoadFontBytes(fontFile)` | font bytes (the manifest's `file` value) | `NotoSansSC.ttc.bytes` |

## Cursor affordance (optional)

By default hover shows the system cursor. To skin an intent, register a
texture (0 = arrow, 1 = pointer hand, 2 = hidden):

```csharp
// hotspot = offset from the texture's top-left to the pointing pixel.
// null unregisters back to the default.
_driver.SetCursorTexture(1u, handTexture, new Vector2(12f, 1f));
```

`cursor:none` hides the pointer per element out of the box.

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
| `Instantiate` returns null | package not listed in `ikat.runtime.json` or wrong stem → rebuild workspace; check package name |
| Page blank | artifacts stale or font missing → `ikat build`; register a default font |
| `Get<T>` throws `UIContractException` | id missing/renamed in HTML, or lookup crossing a component boundary → fix the contract (both sides), or go through the host node |
| Text renders but looks wrong | font fallback missing glyphs → register a `--fallback` font |
| Tofu boxes (□) in text | the Console logs every missing glyph as `[Ikat] missing glyphs (tofu): font-family "X" has no glyph for 'c' (U+....)` — fix by registering a font containing it with `--fallback`, or change the text |
| Clicks pass through UI to 3D | expected — Ikat never blocks input → gate your raycasts on `IsPointerOnUI` |
| Page looks wrong at runtime | press F8, read the `[Scene tree]` section first (one line per node with rect, text `font/lh/lines`, scroll `viewport/content/overlap`) → `lh=NN.00x` with a big NN means a unitless line-height multiplier; `ov 0x0` on a scroll container means it has nothing to scroll. If core dump is wrong it's a workspace/layout issue, if only Unity differs it's a backend issue. `IkatHost.DumpSceneTree("id-or-class")` prints just the matching subtrees |
| `[Ikat] wheel ignored: node N ... no overflow to scroll` | the wheel landed on a container whose content does not overflow (`overlap=0`) — fix content sizing or remove `overflow:auto` (editor/development builds only) |

## Reference consumer

- **Full API contract** (every node/control/event/list/animation
  signature and invariant): read `references/api-reference.md` next to
  this file — it mirrors the shipped C# signatures, so you never need
  the Ikat repository.
- **Complete working example**: `unity/showcase-unity/` in the Ikat
  repository (driver mounted, every showcase page wired from
  `ShowcaseRunner.cs` — including the world-anchor / mount / multi-stage
  / stress demo pages). Optional copy-paste source, not required
  reading.
