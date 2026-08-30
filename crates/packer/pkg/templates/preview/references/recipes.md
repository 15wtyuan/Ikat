# Preview simulation recipes

Consumer-layer (B) starting points. The behavior layer (component
expansion, control wiring, structural polyfill) is served by the preview
server itself from the running binary (`/ikat-preview/lib/*`, auto-injected
boot entry — see SKILL.md). Everything below is what a workspace still
owns: demo data, fonts/theming, page navigation.

## pages/<page>.js — demo data

```js
// preview/pages/inventory.js — injected only into inventory.html.
import { ready } from '/ikat-preview/lib/boot.js';
import { fillList, pageDir } from '/ikat-preview/lib/fill.js';

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

Import both modules by absolute URL (`/ikat-preview/lib/...`) — they are
version-matched to the running CLI, never copied into the workspace.

## main.js (optional) — fonts, theming, shared page glue

```js
// preview/main.js — injected into every page of the package (after boot).
const link = document.createElement('link');
link.rel = 'stylesheet';
link.href = 'preview/preview-theme.css';
document.head.insertBefore(link, document.head.firstChild);
```

`preview-theme.css` (name it anything) holds workspace-owned styling:

- Theme colors/backgrounds/decoration. Do **not** re-declare structural
  resets already owned by `/ikat-preview/lib/base.css` (`box-sizing`,
  button reset, placeholder line) — same-name rules here would fight the
  framework copy as a second truth.
- Fonts need nothing here: the server auto-injects `@font-face` for every
  font in `ikat.workspace.json` plus a default-family `body` rule
  (injected before your stylesheets, so your CSS wins). Hand-written
  `@font-face` is only for **overrides** — a different source file, or
  `font-display: swap` for a very large font (the injected rules use
  `block`). The family name must match the registered name exactly
  (unmatched families silently fall back to system fonts). `.ttc` files
  cannot be injected at all (browsers reject TrueType Collections) — the
  server skips them with a warning on stderr.

## Navigation & page-specific interaction (main.js)

```js
const NAV = { 'nav-settings': 'settings', 'nav-mail': 'mail' };
for (const [id, page] of Object.entries(NAV)) {
  const el = document.getElementById(id);
  el?.addEventListener('click', () => {
    location.href =
      location.href.substring(0, location.href.lastIndexOf('/') + 1) +
      page + '.html';
  });
}
```

Page-private interactions (battle replay, hotkey demos, readouts) follow
the same shape: plain DOM listeners on top of booted state. If a control
needs programmatic driving, import nothing extra — after `ready`, the
elements carry live attributes (`aria-valuenow`, `aria-expanded`,
`aria-checked`); mutate them and dispatch the matching Event the way the
A-layer wiring does.

## Trust list (what a preview can and cannot show)

- **Trustworthy**: flex layout, gap, px sizes, colors, gradient subset,
  `position:absolute`, `border-radius`, `@keyframes timing` (including inside
  component `<style>`; same-name collisions resolve page-wins, matching the
  packer's host priority), component expansion with scoped styles (server
  rewritten — root-class rules on the template root DO apply), control visual
  state, `cursor` (browser-native; mirrors the #93 runtime hand-intent
  default — in the Unity runtime the drawn hand depends on the host
  registering a cursor texture).
- **Approximate**: fonts (same files via @font-face, different rasterizer
  than the game), letterboxing (the shell scales per match_mode — check
  readability, not exact device pixels), page rules leaking into component
  subtrees (the browser has no style wall; the rewritten selectors carry a
  +0,1,0 specificity bump that keeps component rules winning equal-specificity
  contests — a higher-specificity page rule can still pierce).
- **Font loading note (#96)**: the server revalidates workspace static
  assets (`no-cache` + `304`), so after the first load fonts come from
  cache and refresh instantly. The FIRST load of a multi-MB font still
  transfers once: with `font-display: block` that window shows laid-out
  but invisible text (`!important` cannot help — it is not a cascade
  issue). Prefer `font-display: swap` for very large fonts, or keep
  `block` and accept a one-time flash on first visit.
- **Preview rejects like the build does (shown as CSS comments in the served
  sheet)**: non-`@keyframes` at-rules (`@media` …) and out-of-fence selectors
  inside component `<style>` — dropped, never silently applied.
- **Runtime-only (preview cannot show)**: NativeHost 3D projection,
  driver-driven list virtualization beyond demo fill, C# tween callbacks,
  focus/keyboard routing beyond the simulated bits, safe-area insets
  (shell draws reference guides only).

If the build emits a preview≠runtime warning (`FenceBorderWithoutStyle`,
`FenceBgImageWithoutSize`, non-transitionable `transition` properties,
`FenceDisplayInline`, dead sizing on inline text), the preview WILL lie
about exactly that property — fix the source, don't paper over it in a
preview script.
