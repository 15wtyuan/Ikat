---
name: loomgui-preview
description: |
  Write browser preview simulation scripts for LoomGUI pages and run the
  human preview loop. Use when a LoomGUI page is finished and a human needs
  to see it (`loom preview` workbench), when `loom check` reports
  PreviewDataFillWithoutSim, or when a preview script (preview/main.js,
  preview/pages/<page>.js) needs writing or fixing. The framework ships NO
  simulator — every preview script is workspace-owned AI code.
---

# LoomGUI Preview Simulation

Pages render statically in the browser, but runtime behavior (component
expansion, control state, data-driven lists, navigation) does not exist
there. You write preview simulation scripts so the human preview matches
the game. Rendering parity between browser and runtime is a framework
guarantee; your scripts only restore **behavior**, never re-layout.

## The mechanism (what the server does for you)

`loom preview <workspace>` (long-running; prints one JSON with `url`) serves
a workbench: package/page tree on the left, device-frame preview on the
right (scaled by the workspace `match_mode` — the page itself never
reflows). The server auto-injects at most two ES module entries per page,
**only when the files exist**:

```
<package-dir>/preview/main.js           ← every page in the package (shared sim)
<package-dir>/preview/pages/<page>.js   ← just that page (demo data)
```

HTML sources stay clean (zero `<script>` references); the `preview/`
directory never enters the build. Modules are deferred — the DOM is parsed
when they run; no DOMContentLoaded dance. Server restarts reuse a stable
port, so an open tab survives (human just refreshes).

## When you MUST write a script

- Page has `data-fill` lists (runtime-populated ListView) → the human
  preview shows empty lists otherwise. `loom check` warns
  `PreviewDataFillWithoutSim` when the per-page script is missing — that
  warning is your cue.
- Page uses custom elements (`<item-card>` …) → needs the shared
  `main.js` with component expansion, or components render as bare hosts.
- Page has interactive controls (slider/combobox/switch/spinbutton/
  progressbar/tabs/dialogs) → without wiring they look dead in preview.
- Page is driven by game code (readouts, dynamic content) → a per-page
  script simulating that data makes the preview honest.

A workspace with none of the above needs no scripts at all.

## Workflow (the human preview gate)

1. Write/fix the page HTML/CSS until `loom check` exits 0.
2. Write the simulation scripts (recipes in `references/recipes.md`).
3. Start or reuse the server: `loom preview` from the session root — if
   one is already running it prints the same URL (`reused: true`); find it
   in `.loom/preview.json` if stdout was swallowed by backgrounding.
4. Give the human the URL. They refresh (F5) after each of your edits —
   the server reads sources live, no restart needed.
5. Iterate on feedback; only after the human approves the preview does the
   page proceed to `loom build` / runtime wiring. Human preview is the
   gate before handing off to Unity.
6. Stop the server with `loom preview --stop` when the session is done
   (it also self-exits after 4h idle).

## Boundaries

- Never reference preview scripts from page HTML (the fence owns the
  document; injection is the server's job).
- Never simulate what the framework guarantees (flex layout, px math,
  gradients, keyframes timing) — scripts restore behavior, not pixels.
- Custom resolutions / safe-area guides are preview-shell UI (localStorage
  prefs); they never touch the workspace.
- Trust tiers for what a preview can and cannot show:
  `references/recipes.md` §Trust list.

## References

| File | Contents |
|---|---|
| `references/recipes.md` | copy-paste recipes: component expansion, each control's wiring, demo data fill, trust list |
