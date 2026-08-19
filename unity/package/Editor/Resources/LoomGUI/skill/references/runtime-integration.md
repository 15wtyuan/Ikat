# LoomGUI Runtime Integration (Unity)

How LoomGUI UI mounts in a Unity project, where build artifacts load from, and how UI and 3D gameplay interoperate. Audience: game programmers and coding agents working in the Unity project. Authoring UI (HTML/CSS fence rules, the `loom` CLI, the workspace layout) is documented workspace-side — see the UI workspace's `AGENTS.md` and the loomgui-editor skill.

## Mental model

- LoomGUI UI is its own fullscreen, camera-space layer. A dedicated orthographic UI camera renders self-drawn meshes and composites with your 3D camera by camera depth (the UI camera renders after the main camera, depth-only clear). No uGUI Canvas, no URP overlay-camera stack, no EventSystem.
- The UI scene is a typed C# object tree (`Container`, `Button`, `Slider`, `ListView`, ...). Game code reads and drives that tree; it never touches meshes or materials.
- One MonoBehaviour (`LoomStageDriver`) owns the whole frame pipeline: input collection → UI logic → layout/render tick → Unity mesh mirror → event dispatch. You never call a per-frame update yourself.

## Mount in a scene

1. Create a GameObject (e.g. `LoomGUI`) and add `LoomStageDriver` + `LoomInputCollector` (same GameObject; the collector is auto-found if omitted).
2. Configure the driver in the Inspector:
   - **Design Size** — the authoring resolution of the workspace (e.g. 1920x1080 landscape, 1080x1920 portrait). The UI scales shrink-to-fit onto the actual screen.
   - **UI Camera** — leave empty and the driver creates one (`LoomUICamera`). Assign your own if you want a specific camera; either way the driver configures it (orthographic, depth-only clear, UI layer).
   - **Safe Area** — shrink-to-fit into `Screen.safeArea` (notch-safe letterboxing). Off = full screen.
   - **Product Root** — where build artifacts live (next section). Empty = sensible defaults.
3. Camera ordering: your main 3D camera renders first, the UI camera renders after (higher depth) with clear flags = Depth only. Both can be plain Base cameras under URP.
4. Layer 6 is reserved by LoomGUI (UI meshes, UI camera, embedded native content). Do not use it for anything else.
5. Input: no EventSystem is needed or used. `LoomInputCollector` polls raw device state and supports both the Input System package and the legacy Input Manager (any Active Input Handling mode).
6. The driver is `[ExecuteAlways]` — the UI also renders in Edit Mode, handy for checking pages without entering Play Mode.

On startup the driver reads `loom.runtime.json` from the product root and loads everything it lists: packages (`ui/<name>.pkg.bin`), atlas manifests with lazily loaded atlas pages (`atlas/`), and fonts (`fonts/`). Missing pieces log warnings to the Console naming the exact file.

## Where artifacts load from

Product root resolution:

- **Product Root** set → that path.
- Empty, in the Editor → `Assets/Bundles` (where the packer writes when the workspace has Unity integration).
- Empty, in a built player → `Application.streamingAssetsPath`. Before building a player, copy the Bundles content into StreamingAssets (or point Product Root at your own location, or override loading below).

To serve artifacts from AssetBundles or Addressables, subclass `LoomStageDriver` and override the virtual loading hooks (defaults read plain files under the product root):

| Hook | Loads | Example argument |
|---|---|---|
| `LoadTextFile(relPath)` | manifests (text) | `loom.runtime.json`, `atlas/ui.atlas.json` |
| `LoadBytes(relPath)` | raw bytes | `ui/game.pkg.bin` |
| `LoadPackageBytes(name)` | a package (default: `LoadBytes("ui/{name}.pkg.bin")`) | `game` |
| `LoadTexture(relPath)` | atlas page PNG | `atlas/ui.png` |
| `LoadFontBytes(fontFile)` | font bytes (the manifest's `file` value) | `NotoSansSC.ttc.bytes` |

## Show a page from game code

```csharp
using LoomGUI;

public class GameUI : MonoBehaviour
{
    LoomStageDriver _driver;
    Container _page;

    void Start()
    {
        _driver = GetComponent<LoomStageDriver>();
        _page = _driver.Instantiate("game", "main");   // package name + HTML file stem ("main.html" -> "main")
        if (_page == null) { Debug.LogError("page failed to mount (package not in loom.runtime.json? wrong stem?)"); return; }

        _page.Get<Button>("btn-start").Clicked += OnStart;
    }

    void OnStart()
    {
        // gameplay: load a scene, spawn units, play a cutscene ...
    }
}
```

- `Instantiate` returns the page root as a `Container` (custom-element roots carry their real type) and appends it to the UI scene root. `null` plus a console error means the package is not listed in `loom.runtime.json` or the stem doesn't exist.
- Tear down with `_page.Dispose()` — recursively destroys the subtree and clears its event subscriptions. The page-swap pattern is: Dispose old → Instantiate new → re-wire.
- Packages can also be loaded at runtime: `driver.Context.LoadPackage(name, bytes)` (bytes from your own pipeline; duplicate names throw) and `UnloadPackage(name)` (prefab semantics: live instances survive the unload).

## The id contract (workspace ↔ game code)

`id` attributes in the workspace HTML are the API surface game code programs against:

- `page.Get<Button>("btn-start")` throws `UIContractException` on miss; `page.TryGet<Button>(id, out var b)` for optional elements.
- Lookup is scoped to the current component instance — it does not cross nested custom-component or list-item boundaries. To reach into a nested component, go through its host node: `page.Get<CustomElement>("page-top").TryGet<Button>("back-home", out var b)`.
- Keep ids of interactive elements stable and semantic (`btn-start`, `hp-fill`, `lv-items`). Renaming an id in HTML silently breaks game code (`TryGet` misses, `Get` throws). When UI and game code are authored in the same effort, treat id names as a shared contract, not a private detail.

## UI ↔ 3D interop

**UI drives gameplay.** Subscribe to control events in C# and do the 3D/game work in the handler:

```csharp
page.Get<Button>("btn-start").Clicked  += () => SceneManager.LoadScene("battle");
page.Get<Slider>("sfx-volume").ValueChanged += e => audio.SetVolume(e.NewValue);
page.Get<TextField>("chat-input").Submitted   += text => net.Send(text);
page.Get<Dropdown>("difficulty").SelectionChanged += e => SetDifficulty(e.NewIndex);
```

Also available: `toggle.CheckedChanged`, routed events via `node.On<PointerDownEvent>(...)` (returns an `IDisposable` registration).

**Gameplay drives UI.** Hold node references and mutate them: text via `container.TextContent` / `TextNode.Text`, control values via `slider.Value` / `progressBar.Value`, virtualized lists via `listView.ItemCount` + `BindItem`, visual state via `node.Classes.Add(...)`, animations via `node.Play("name")`, per-node logic via `node.OnUpdate(dt)` and `driver.Context.CallLater` / `CallNextFrame`. Inline style overrides sit on `node.Style.*`.

**Clicks and the 3D scene.** LoomGUI never consumes or blocks input — there is no EventSystem integration and no consume flags; the core resolves what the pointer hits, full stop. Your own 3D picking code must gate on the UI:

```csharp
if (driver.Context.IsPointerOnUI) return;   // pointer is over UI — don't raycast the world
if (Physics.Raycast(cameraRay, out var hit)) SelectUnit(hit.transform);
```

`driver.Context.Pick(point)` hit-tests the UI from game code (design coordinates: pixels, origin top-left, y down).

**Embedding 3D content inside UI.** `driver.BindNativeHost(node, go)` pins any GameObject (3D model, particles, camera feed) to a UI node — character preview slots, decorated cards. Every frame the binding copies the node's world transform, visibility (`display:none` → `SetActive(false)`) and sort order (interleaved with UI meshes in the transparent queue; materials are auto-cloned to URP transparent). The GameObject's own hierarchy, scale and animations remain yours. `driver.UnbindNativeHost(node)` releases it without destroying the GO; disposing the node auto-hides it. A page that embeds 3D binds on mount and unbinds on teardown.

**Not supported — world-space UI.** The UI is always fullscreen camera-space; it cannot be pinned to a 3D transform or camera plane (diegetic screens, in-world panels). The supported direction is the inverse: 3D content embedded in UI via NativeHost. For genuinely in-world UI, build your own solution outside LoomGUI.

## Frame loop, scaling, diagnostics

- The driver's `LateUpdate` drives the whole pipeline with unscaled delta time — UI time keeps running under `Time.timeScale = 0` (pause menus work). Screen-size changes (editor Game view, window resize) re-fit automatically.
- Design resolution maps shrink-to-fit onto the screen (optionally into the safe area); y-flip and camera math are internal — game code only ever sees design coordinates.
- Press **F8** during Play to dump both the core state and the Unity mirror state to the Console and to a `loom-dump-*.txt` next to `Assets/` — the standard first evidence when a page looks wrong (it separates "core computed wrong layout" from "Unity rendered it wrong").

## Reference consumer

The LoomGUI repository ships `unity/showcase-unity/` — a complete Unity project with the driver mounted and nine pages instantiated and wired from `ShowcaseRunner.cs` (navigation, controls, virtualized lists, a NativeHost 3D character stage, runtime package load/unload). Treat it as the copy-paste source for integration patterns. The full business API contract (nodes, events, styles, ListView, animation) is `docs/design/public-api.md` in the repository.
