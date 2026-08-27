---
name: ikat-preview
description: |
  Write browser preview simulation scripts for Ikat pages and run the
  human preview loop. Use when a Ikat page is finished and a human needs
  to see it (`ikat preview` workbench), when `ikat check` reports
  PreviewDataFillWithoutSim, or when a preview script (preview/main.js,
  preview/pages/<page>.js) needs writing or fixing. The server ships the
  behavior layer (component expansion, control semantics) itself; your
  scripts are workspace-owned consumer-layer code only.
---

# Ikat Preview Simulation

Pages render statically in the browser; runtime behavior does not exist
there. The preview server closes that gap in **two layers** (#92):

- **A layer — shipped by the framework.** For every HTML page the server
  auto-injects `/ikat-preview/lib/boot.js`, which performs component
  expansion (custom elements, slot projection, scoped styles), wires
  control semantics (slider/combobox/switch/spinbutton/tabs/dialogs/
  progressbar/textbox), and injects the structural base polyfill
  (`box-sizing`, button reset, placeholder line). Its content is embedded
  in the running `ikat` binary — always version-matched to the CLI. You
  never copy or reimplement any of this; a workspace copy would rot into a
  second source of truth (this happened in dogfooding; it is now a design
  error, not a recipe).
- **B layer — yours.** Demo data, page navigation, page-specific
  interactions, theming/fonts. These live in `preview/main.js`
  (per-package, optional) and `preview/pages/<page>.js` (per-page); the
  server injects them after boot when the files exist.

Rendering parity between browser and runtime is a framework guarantee;
your scripts restore **consumer behavior**, never re-layout.

## The mechanism (what the server does for you)

`ikat preview <workspace>` (long-running; prints one JSON with `url`) serves
a workbench: package/page tree on the left, device-frame preview on the
right (scaled by the workspace `match_mode` — the page itself never
reflows). Per page it injects at most three ES modules, in this order:

```
/ikat-preview/lib/boot.js        ← ALWAYS (framework behavior layer)
<package-dir>/preview/main.js    ← if present (shared consumer sim)
<package-dir>/preview/pages/<page>.js  ← if present (demo data)
```

Wait for A layer before touching filled DOM:

```js
import { ready } from '/ikat-preview/lib/boot.js';
ready.then(() => { /* fill lists, drive page state */ });
```

HTML sources stay clean (zero `<script>` references); the `preview/`
directory never enters the build. Modules are deferred — the DOM is parsed
when they run. Server restarts reuse a stable port, so an open tab
survives (human just refreshes).

## When you MUST write a script

- Page has `data-fill` lists (runtime-populated ListView) → the human
  preview shows empty lists otherwise. `ikat check` warns
  `PreviewDataFillWithoutSim` when the per-page script is missing — that
  warning is your cue.
- Page is driven by game code (readouts, dynamic content) → a per-page
  script simulating that data makes the preview honest.
- Page has custom navigation/hotkeys/scene-specific behavior → wire it in
  `main.js`.

A workspace with none of the above needs **no scripts at all** — component
pages and control pages are already alive through the A layer.

## Workflow (the human preview gate)

1. Write/fix the page HTML/CSS until `ikat check` exits 0.
2. Write the simulation scripts (recipes in `references/recipes.md`).
3. Start or reuse the server: `ikat preview` from the session root — if
   one is already running it prints the same URL (`reused: true`); find it
   in `.ikat/preview.json` if stdout was swallowed by backgrounding.
4. Give the human the URL. They refresh (F5) after each of your edits —
   the server reads sources live, no restart needed.
5. Iterate on feedback; only after the human approves the preview does the
   page proceed to `ikat build` / runtime wiring. Human preview is the
   gate before handing off to Unity.
6. Stop the server with `ikat preview --stop` when the session is done
   (it also self-exits after 4h idle).

## Boundaries

- Never reference preview scripts from page HTML (the fence owns the
  document; injection is the server's job).
- Never reimplement what the A layer owns (expansion, controls) — you
  cannot out-source-of-truth the framework; import `ready` from the boot
  module instead.
- Never simulate what the framework guarantees (flex layout, px math,
  gradients, keyframes timing) — scripts restore behavior, not pixels.
- Custom resolutions / safe-area guides are preview-shell UI (localStorage
  prefs); they never touch the workspace.
- Trust tiers for what a preview can and cannot show:
  `references/recipes.md` §Trust list.

## References

| File | Contents |
|---|---|
| `references/recipes.md` | copy-paste recipes: demo data fill, fonts/theming, navigation, trust list |
