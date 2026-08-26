# Preview simulation recipes

Copy-paste starting points, distilled from the showcase workspace's
battle-tested simulation stack (rect-diff verified against core). Import
what you need from `preview/lib/` — create it by copying from these
recipes; the convention only pins the two entry files (`main.js`,
`pages/<page>.js`), everything inside `lib/` is yours to organize via
ESM imports.

## main.js skeleton (shared, per package)

```js
// preview/main.js — injected into every page of the package.
import { expandComponents, fetchRegistry } from './lib/expand.js';
import { wireTabs, wireDialogs, wireProgressbars, wireSliders,
         wireSwitchesAndRadios, wireComboboxes, wireSpinbuttons,
         wireTextboxes } from './lib/controls.js';

export const ready = boot();          // pages/<page>.js awaits this

async function boot() {
  injectBaseCss();                    // preview/preview-base.css, head-FIRST
  try { expandComponents(await fetchRegistry()); } catch (_) {}
  wireTabs(); wireDialogs();
  wireProgressbars(); wireSliders(); wireSwitchesAndRadios();
  wireComboboxes(); wireSpinbuttons(); wireTextboxes();
}

function injectBaseCss() {
  // Base polyfill (@font-face for workspace fonts, box-sizing reset) must
  // ride the script channel — the fence validates every <link>, and the
  // polyfill is intentionally out-of-fence. Insert at head TOP: polyfill
  // first, page <style> after (same cascade the old inline stack had).
  const link = document.createElement('link');
  link.rel = 'stylesheet';
  link.href = 'preview/preview-base.css';
  document.head.insertBefore(link, document.head.firstChild);
}
```

`preview-base.css` essentials: `@font-face` for every workspace font
(`../../res/fonts/<file>.ttf` relative to `preview/`), `* { box-sizing:
border-box }`, button/img display resets matching fence semantics.

## lib/expand.js — component expansion

Semantics to mirror (pack-time Custom Element expansion): host keeps its
place/attrs; template root appended under it; `<slot name=x>` replaced at
its splice position by host light children with `slot="x"` (fallback
children kept when nothing assigned); whitespace-only light children
dropped; relative URLs inside the template resolved against the component
file location (`components/<name>.html`); nested components expand in
further passes to a fixpoint (≤16); component `<style>` prefixed with
`[data-loom-comp="name"]` for scope emulation. Registry data comes from
the server: `fetch('/api/workspace.json')` → `components: { name:
<workspace-rel-path> }` → fetch each `/ws/<path>` source.

Full reference implementation: `showcase/showcase/preview/lib/expand.js`
in the LoomGUI repository.

## lib/controls.js — control wiring (mirror core semantics)

- `role=progressbar`: drive `[data-slot=fill]` width from
  `aria-valuenow/min/max` (percent).
- `role=slider`: position `[data-slot=thumb]` (`left = (trackW − thumbW) ×
  pct`, vertically centered via translateY) and `[data-slot=fill]` width;
  pointer drag updates `aria-valuenow`, clamps, quantizes to `data-step`.
- `role=switch`: click toggles `aria-checked`.
- `role=radio`: click checks it and unchecks `[data-name=…]` siblings.
- `role=combobox`: click toggles `aria-expanded` + `listbox` display;
  clicking an `role=option` writes its text into `[data-slot=value]`,
  marks `aria-selected`, collapses; outside click closes; starts closed.
- `role=spinbutton`: render `aria-valuenow` as text; wheel/ArrowUp/Down
  adjust by `data-step`; contenteditable typing commits on blur/Enter
  (parse → quantize → clamp).
- `role=tab`: target panel keeps author CSS (`display=''`), all other
  panels `display='none'`; initialize from the first
  `aria-selected=true` tab.
- `role=textbox`: `contenteditable=true`; toggle `data-empty` attribute —
  the placeholder line renders via `[data-empty]::before` in CSS (an
  empty textbox must keep its placeholder line height, or rect-diff
  fabricates diffs).
- dialogs: `[data-open-dialog=id]` shows (`display=''`),
  `[data-close-dialog]` hides the closest `[role=dialog]`.

Full reference implementation: `showcase/showcase/preview/lib/controls.js`.

## pages/<page>.js — demo data

```js
// preview/pages/inventory.js — injected only into inventory.html.
import { ready } from '../main.js';
import { fillList, pageDir } from '../lib/fill.js';

const ICONS = ['item-potion', 'item-chest', 'item-gem'];
ready.then(() => {
  const dir = pageDir() + '../res/icons/';
  document.querySelectorAll('[role="list"][data-fill]').forEach((list) => {
    const count = parseInt(list.getAttribute('data-fill'), 10) || 8;
    fillList(list, count, (i, node) => {
      const img = node.querySelector('img');
      if (img) img.src = dir + ICONS[i % ICONS.length] + '.png';
    });
  });
});
```

`fillList(list, count, decorate)` clones the list's `<template>` until
`count` items exist (the template itself is item 0). Rect-diff tooling
removes these clones before measuring (core's static dump has no driver),
so fill freely — it never breaks the alignment gate.

## Trust list (what a preview can and cannot show)

- **Trustworthy**: flex layout, gap, px sizes, colors, gradient subset,
  `position:absolute`, `border-radius`, `@keyframes` timing, component
  expansion, control visual state (with the recipes above).
- **Approximate**: fonts (same files via @font-face, different rasterizer
  than the game), letterboxing (the shell scales per match_mode — check
  readability, not exact device pixels).
- **Runtime-only (preview cannot show)**: NativeHost 3D projection,
  driver-driven list virtualization beyond demo fill, C# tween callbacks,
  focus/keyboard routing beyond the simulated bits, safe-area insets
  (shell draws reference guides only; the core has no inset concept).

If the build emits a preview≠runtime warning (`FenceBorderWithoutStyle`,
`FenceBgImageWithoutSize`, non-transitionable `transition` properties,
`FenceDisplayInline`, dead sizing on inline text), the preview WILL lie
about exactly that property — fix the source, don't paper over it in a
preview script.
